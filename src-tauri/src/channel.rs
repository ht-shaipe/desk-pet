use std::time::Duration;

use channel_client::channel::{
    client_message::Payload as ClientPayload, server_message::Payload as ServerPayload,
    ClientMessage, DeviceInfo, Hello, Heartbeat,
};
use channel_client::{ChannelClient, ChannelClientConfig};
use deskpet_os::device;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::state::AppState;

/// channel 服务 gRPC 端点。
const ENDPOINT: &str = "https://hub.htui.cc";
/// 心跳间隔。
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);
/// 初始重连退避。
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
/// 最大重连退避。
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// 实时连接句柄，存于 AppState 供前端回执与主动断开。
pub struct ChannelHandle {
    pub out: mpsc::Sender<ClientMessage>,
    pub cancel: mpsc::Sender<()>,
}

/// 启动实时连接运行时（在 tauri async runtime 上 spawn）。
/// 重复调用会先取消旧连接再启动新连接，保证同时只有一条长连接。
pub fn start(app: AppHandle, device_id: String, token: String) {
    let (out_tx, out_rx) = mpsc::channel::<ClientMessage>(32);
    let (cancel_tx, cancel_rx) = mpsc::channel::<()>(1);

    {
        let state = app.state::<AppState>();
        let mut handle = state.channel.lock().unwrap();
        if let Some(old) = handle.take() {
            let _ = old.cancel.try_send(());
        }
        handle.replace(ChannelHandle {
            out: out_tx.clone(),
            cancel: cancel_tx,
        });
    }

    tauri::async_runtime::spawn(run(app, device_id, token, out_tx, out_rx, cancel_rx));
}

async fn run(
    app: AppHandle,
    device_id: String,
    token: String,
    mut out_tx: mpsc::Sender<ClientMessage>,
    mut out_rx: mpsc::Receiver<ClientMessage>,
    mut cancel_rx: mpsc::Receiver<()>,
) {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let mut backoff = INITIAL_BACKOFF;

    loop {
        // 连接 channel 服务 gRPC
        let mut client =
            match ChannelClient::connect(ChannelClientConfig::new(ENDPOINT, &token)).await {
                Ok(c) => {
                    eprintln!(
                        "[channel] 已连接 {ENDPOINT}（token={}）",
                        if token.is_empty() { "无" } else { "有" }
                    );
                    c
                }
                Err(e) => {
                    eprintln!(
                        "[channel] 连接失败: {}（token={}，退避 {backoff:?} 后重试）",
                        err_chain(&e),
                        if token.is_empty() { "无" } else { "有" }
                    );
                    if sleep_with_cancel(&mut cancel_rx, backoff).await {
                        return;
                    }
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                    continue;
                }
            };

        // 首帧 Hello（携带设备信息快照）
        let device_info = collect_device_info(&app).await;
        let hello = ClientMessage {
            payload: Some(ClientPayload::Hello(Hello {
                device_id: device_id.clone(),
                app_key: "desk-pet".into(),
                version: version.clone(),
                ip: String::new(),
                device_info: Some(device_info),
            })),
        };
        if out_tx.send(hello).await.is_err() {
            return;
        }

        // 打开双向流（消费 out_rx）
        let mut inbound = match client.open(ReceiverStream::new(out_rx)).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[channel] 打开流失败: {}", err_chain(&e));
                if sleep_with_cancel(&mut cancel_rx, backoff).await {
                    return;
                }
                backoff = (backoff * 2).min(MAX_BACKOFF);
                let (ntx, nrx) = mpsc::channel(32);
                out_tx = ntx;
                out_rx = nrx;
                update_sender(&app, out_tx.clone());
                continue;
            }
        };

        backoff = INITIAL_BACKOFF;
        let mut heartbeat_seq = 0i64;
        let mut ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
        ticker.tick().await; // 跳过首次立即触发

        // 收发循环
        loop {
            tokio::select! {
                _ = cancel_rx.recv() => return,
                msg = inbound.message() => match msg {
                    Ok(Some(msg)) => {
                        if !handle_message(&app, msg) {
                            // 握手被拒（HelloAck.success=false）：按契约关闭流，退避重连
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("[channel] 流错误: {}", err_chain(&e));
                        break;
                    }
                },
                _ = ticker.tick() => {
                    heartbeat_seq += 1;
                    let hb = ClientMessage {
                        payload: Some(ClientPayload::Heartbeat(Heartbeat { seq: heartbeat_seq })),
                    };
                    if out_tx.send(hb).await.is_err() {
                        break;
                    }
                }
            }
        }

        // 流断开：重建 channel、更新 state、退避重连
        let (ntx, nrx) = mpsc::channel(32);
        out_tx = ntx;
        out_rx = nrx;
        update_sender(&app, out_tx.clone());
        if sleep_with_cancel(&mut cancel_rx, backoff).await {
            return;
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// 退避等待，返回 true 表示被取消。
async fn sleep_with_cancel(cancel: &mut mpsc::Receiver<()>, dur: Duration) -> bool {
    tokio::select! {
        _ = cancel.recv() => true,
        _ = tokio::time::sleep(dur) => false,
    }
}

/// 展开错误源链：transport error 这类包装错误需溯源到根因（TLS/DNS/HTTP2 等）。
fn err_chain(e: &(dyn std::error::Error + 'static)) -> String {
    let mut chain = e.to_string();
    let mut src = e.source();
    while let Some(s) = src {
        chain.push_str(" ← ");
        chain.push_str(&s.to_string());
        src = s.source();
    }
    chain
}

/// 采集设备信息快照（系统调用在阻塞线程池执行），随 Hello 上报服务端登记。
async fn collect_device_info(app: &AppHandle) -> DeviceInfo {
    let snap = tokio::task::spawn_blocking(device::snapshot)
        .await
        .unwrap_or_default();
    let (screen_width, screen_height) = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| (m.size().width as i32, m.size().height as i32))
        .unwrap_or((0, 0));
    DeviceInfo {
        device_brand: snap.device_brand,
        device_model: snap.device_model,
        device_type: snap.device_type,
        screen_width,
        screen_height,
        os_name: snap.os_name,
        os_version: snap.os_version,
        wifi_enabled: snap.wifi_enabled,
        bluetooth_enabled: snap.bluetooth_enabled,
        host_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// 把新的 outbound sender 刷回 state（重连后旧 sender 已失效）。
fn update_sender(app: &AppHandle, sender: mpsc::Sender<ClientMessage>) {
    if let Some(h) = app.state::<AppState>().channel.lock().unwrap().as_mut() {
        h.out = sender;
    }
}

/// 处理服务端消息：转为 serde JSON 经 Tauri 事件推给前端。
/// 返回 false 表示会话应终止（握手被拒），由调用方断流重连。
fn handle_message(app: &AppHandle, msg: channel_client::channel::ServerMessage) -> bool {
    match msg.payload {
        Some(ServerPayload::HelloAck(ack)) => {
            if !ack.success {
                eprintln!("[channel] 握手被拒: {}", ack.message);
            }
            let _ = app.emit(
                "channel://hello-ack",
                json!({
                    "success": ack.success,
                    "message": ack.message,
                    "server_time": ack.server_time,
                    "pending_tasks": ack.pending_tasks.iter().map(|t| json!({
                        "task_id": t.task_id,
                        "task_type": t.task_type,
                        "attempt": t.attempt,
                        "payload": t.payload,
                    })).collect::<Vec<_>>(),
                }),
            );
            ack.success
        }
        Some(ServerPayload::HeartbeatAck(_)) => true,
        Some(ServerPayload::TaskAssign(task)) => {
            let _ = app.emit(
                "channel://task-assign",
                json!({
                    "task_id": task.task_id,
                    "task_type": task.task_type,
                    "attempt": task.attempt,
                    "payload": task.payload,
                }),
            );
            true
        }
        Some(ServerPayload::TaskCancel(cancel)) => {
            let _ = app.emit("channel://task-cancel", json!({ "task_id": cancel.task_id }));
            true
        }
        Some(ServerPayload::Upgrade(upgrade)) => {
            let _ = app.emit(
                "channel://upgrade",
                json!({
                    "version": upgrade.version,
                    "url": upgrade.url,
                    "md5": upgrade.md5,
                    "force": upgrade.force,
                }),
            );
            true
        }
        Some(ServerPayload::Push(push)) => {
            eprintln!("[channel] ────── 收到推送 ──────");
            eprintln!("[channel]   message_type = {:?}", push.message_type);
            eprintln!("[channel]   message_id   = {:?}", push.message_id);
            eprintln!("[channel]   server_time  = {}", push.server_time);
            eprintln!("[channel]   payload 原文 = {:?}", push.payload);
            // 直接推送统一走屏幕大字浮层（notify 窗口）；
            // 需要可靠投递/回执的业务消息应走 TaskAssign，不走此路径。
            show_notice(app, &push.payload, push.server_time);
            true
        }
        None => true,
    }
}

/// 把通知内容解析为 {title, message} 并转发到 notify 浮层窗口。
/// 浮层窗口缺失时按需创建；创建失败才回退主窗口角落提示。
fn show_notice(app: &AppHandle, payload: &str, server_time: i64) {
    let (title, message) = parse_notice(payload);
    eprintln!("[channel]   解析结果     → title={title:?} message={message:?}");

    if app.get_webview_window("notify").is_none() {
        eprintln!("[channel] notify 浮层窗口不存在，按需创建");
        if !create_notify_window(app) {
            eprintln!("[channel] 浮层创建失败，回退主窗口角落提示");
            let _ = app.emit(
                "channel://push",
                json!({ "message_id": "", "message_type": "notice", "payload": payload, "server_time": server_time }),
            );
            return;
        }
        // 新建窗口页面需要加载时间，延迟投递避免事件早于监听注册
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(800)).await;
            let _ = app.emit_to(
                "notify",
                "channel://push-notice",
                json!({ "title": title, "message": message, "time": server_time }),
            );
        });
        return;
    }
    eprintln!("[channel] 通知已转发浮层窗口");
    let _ = app.emit_to(
        "notify",
        "channel://push-notice",
        json!({ "title": title, "message": message, "time": server_time }),
    );
}

/// 通知 payload 解析为 (title, message)，尽力兼容多种服务端格式：
/// - JSON 对象：title/msgTitle/标题 + content/message/msg/body/text/内容；
/// - 包装对象：{"data": {...}} / {"payload": {...}} 等取内层继续解析；
/// - 键值对数组：["title","系统通知","content","内容"]；
/// - 两元素字符串数组：[标题, 内容]；
/// - 多层编码：JSON 字符串里还装着一份 JSON（逐层剥开）；
/// - 纯文本：整串作为 message，title 留空由浮层兜底。
fn parse_notice(payload: &str) -> (String, String) {
    /// 从 JSON 对象里按候选键取字符串/数字字段
    fn field(v: &serde_json::Value, keys: &[&str]) -> Option<String> {
        let map = v.as_object()?;
        keys.iter().find_map(|k| {
            map.get(*k).and_then(|x| match x {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                serde_json::Value::Bool(b) => Some(b.to_string()),
                _ => None,
            })
        })
    }

    /// 从 JSON 值提取 (title, message)；无法提取时返回 (空, 原文)
    fn extract(v: &serde_json::Value, raw: &str, depth: usize) -> (String, String) {
        match v {
            serde_json::Value::Object(_) => {
                if let Some(t) = field(v, &["title", "msgTitle", "标题"]) {
                    let m = field(v, &["content", "message", "msg", "body", "text", "内容"])
                        .unwrap_or_else(|| raw.to_string());
                    return (t, m);
                }
                // 包装层：data/payload/notice/result 取内层继续
                if depth < 3 {
                    for k in ["data", "payload", "notice", "result"] {
                        if let Some(inner) = v.get(k) {
                            let (t, m) = extract(inner, raw, depth + 1);
                            if !t.is_empty() || m != raw {
                                return (t, m);
                            }
                        }
                    }
                }
                (String::new(), raw.to_string())
            }
            serde_json::Value::Array(items) => {
                // 键值对数组：["title","x","content","y"]
                let mut map = serde_json::Map::new();
                let mut it = items.iter();
                while let (Some(k), Some(val)) = (it.next(), it.next()) {
                    if let Some(ks) = k.as_str() {
                        map.insert(ks.to_string(), val.clone());
                    }
                }
                let kv = serde_json::Value::Object(map);
                if field(&kv, &["title", "msgTitle", "标题"]).is_some() {
                    return extract(&kv, raw, depth + 1);
                }
                // 两元素字符串数组：[标题, 内容]
                if items.len() == 2 {
                    if let (Some(a), Some(b)) = (items[0].as_str(), items[1].as_str()) {
                        return (a.to_string(), b.to_string());
                    }
                }
                (String::new(), raw.to_string())
            }
            // JSON 字符串：里面可能还装着一份 JSON（双层编码），继续剥
            serde_json::Value::String(s) if depth < 3 => {
                match serde_json::from_str::<serde_json::Value>(s) {
                    Ok(inner) => extract(&inner, raw, depth + 1),
                    Err(_) => (String::new(), s.clone()),
                }
            }
            _ => (String::new(), raw.to_string()),
        }
    }

    let raw = payload.trim().to_string();
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => extract(&v, &raw, 0),
        Err(_) => (String::new(), raw),
    }
}

/// 按需创建通知浮层窗口（兜底；正常由 tauri.conf.json 启动时创建，配置一致）。
fn create_notify_window(app: &AppHandle) -> bool {
    let builder = WebviewWindowBuilder::new(app, "notify", WebviewUrl::App("notify.html".into()))
        .title("Desk Pet")
        .inner_size(460.0, 190.0)
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .visible(false)
        .visible_on_all_workspaces(true)
        .accept_first_mouse(true);
    match builder.build() {
        Ok(win) => {
            // 与 lib.rs 启动路径一致：窗口内容层裁圆角（通知窗卡片贴边，半径 20）
            #[cfg(target_os = "macos")]
            if let Ok(ptr) = win.ns_window() {
                unsafe { deskpet_os::window::round_window(ptr, 20.0) };
            }
            eprintln!("[channel] notify 浮层窗口已创建");
            true
        }
        Err(e) => {
            eprintln!("[channel] 创建 notify 浮层窗口失败: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_notice;

    #[test]
    fn notice_payload_formats() {
        // 标准对象
        assert_eq!(
            parse_notice(r#"{"title":"标题","content":"内容"}"#),
            ("标题".into(), "内容".into())
        );
        // 别名键
        assert_eq!(
            parse_notice(r#"{"msgTitle":"t","msg":"m"}"#),
            ("t".into(), "m".into())
        );
        // 键值对数组
        assert_eq!(
            parse_notice(r#"["title","系统通知","content","今晚维护"]"#),
            ("系统通知".into(), "今晚维护".into())
        );
        // 两元素字符串数组
        assert_eq!(
            parse_notice(r#"["系统通知","请更新配置"]"#),
            ("系统通知".into(), "请更新配置".into())
        );
        // 双层编码
        assert_eq!(
            parse_notice(r#""{\"title\":\"t2\",\"content\":\"c2\"}""#),
            ("t2".into(), "c2".into())
        );
        // 包装对象
        assert_eq!(
            parse_notice(r#"{"code":0,"data":{"title":"t3","content":"c3"}}"#),
            ("t3".into(), "c3".into())
        );
        // 纯文本
        assert_eq!(parse_notice("服务器维护通知"), ("".into(), "服务器维护通知".into()));
        // 数字字段值
        assert_eq!(
            parse_notice(r#"{"title":123,"content":true}"#),
            ("123".into(), "true".into())
        );
    }
}
