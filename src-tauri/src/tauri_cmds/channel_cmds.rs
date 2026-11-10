use channel_client::channel::{
    client_message::Payload as ClientPayload, ClientMessage, TaskAck, TaskProgress, UserInfo,
};
use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::paths::data_dir;
use crate::state::AppState;

/// 登录态用户资料载体（前端传入，随 Hello 与 user_info 帧上报服务端绑定设备归属）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserReport {
    pub user_id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub mobile: String,
    #[serde(default)]
    pub avatar: String,
}

impl UserReport {
    /// 业务载体 → 协议 UserInfo。空 user_id 视为解绑（服务端契约：不产生绑定）。
    fn to_info(&self) -> UserInfo {
        UserInfo {
            user_id: self.user_id.trim().to_string(),
            username: self.username.clone(),
            nickname: self.nickname.clone(),
            mobile: self.mobile.clone(),
            avatar: self.avatar.clone(),
        }
    }
}

/// user 载体 → Hello/补报共用的登录态：None / user_id 空白 → None（匿名不填）。
fn active_user(user: Option<UserReport>) -> Option<UserInfo> {
    user.filter(|u| !u.user_id.trim().is_empty())
        .map(|u| u.to_info())
}

/// user 载体 → 上行补报帧 UserInfo：解绑时必须发空 UserInfo 帧（显式解绑，服务端契约）。
fn unbind_or_frame(user: Option<UserReport>) -> UserInfo {
    active_user(user).unwrap_or_default()
}

/// 启动实时连接（应用启动与登录态变化时调用）。token 由前端传入，空串表示匿名连接。
/// `user` 为登录态用户资料，未登录传 null；随 Hello 首帧上报。
/// 返回设备指纹。
#[tauri::command]
pub fn channel_start(
    app: AppHandle,
    token: String,
    user: Option<UserReport>,
) -> Result<String, String> {
    let dir = data_dir(&app)?;
    let mut settings = deskpet_store::settings::load(&dir);
    let device_id = settings.device_id.clone().unwrap_or_else(|| {
        let id = uuid::Uuid::new_v4().to_string();
        settings.device_id = Some(id.clone());
        let _ = deskpet_store::settings::save(&dir, &settings);
        id
    });
    crate::channel::start(app, device_id.clone(), token, active_user(user));
    Ok(device_id)
}

/// 停止实时连接（关闭连接用；退出登录走 channel_report_user 解绑后再匿名重连）。
#[tauri::command]
pub fn channel_stop(app: AppHandle) -> Result<(), String> {
    if let Some(h) = app.state::<AppState>().channel.lock().unwrap().take() {
        let _ = h.cancel.try_send(());
    }
    Ok(())
}

/// 登录态用户资料补报（连接保持期间登录/变更时调用）。
/// `user` 传 null 或空 user_id = 显式解绑（服务端契约：仅匿名重连不解绑，登出必须发空帧）。
/// 无活跃连接时为 no-op（当前服务端无会话可改写；下次连接按当时的登录态随 Hello 上报）。
#[tauri::command]
pub async fn channel_report_user(app: AppHandle, user: Option<UserReport>) -> Result<(), String> {
    let user_info = unbind_or_frame(user);
    let unbind = user_info.user_id.is_empty();
    let frame = ClientMessage {
        payload: Some(ClientPayload::UserInfo(user_info)),
    };
    let sender = {
        let state = app.state::<AppState>();
        let handle = state.channel.lock().unwrap();
        handle.as_ref().map(|h| h.out.clone())
    };
    match sender {
        Some(tx) => {
            tx.send(frame).await.map_err(|e| e.to_string())?;
            // 解绑帧需在旧会话被匿名重连替换前送达服务端处理，给 flush 留缓冲
            if unbind {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        }
        None => {
            eprintln!("[channel] 用户资料上报时无活跃连接，忽略（将随下次 Hello 上报）");
        }
    }
    Ok(())
}

/// 任务接收确认 / 拒绝（幂等键 task_id + attempt）。
#[tauri::command]
pub async fn channel_task_ack(
    app: AppHandle,
    task_id: i64,
    attempt: i32,
    success: bool,
    message: String,
) -> Result<(), String> {
    let sender = app
        .state::<AppState>()
        .channel
        .lock()
        .unwrap()
        .as_ref()
        .map(|h| h.out.clone());
    if let Some(tx) = sender {
        tx.send(ClientMessage {
            payload: Some(ClientPayload::TaskAck(TaskAck {
                task_id,
                attempt,
                success,
                message,
            })),
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 任务进度 / 结果（status: 3 执行中 / 4 成功 / 5 失败）。
#[tauri::command]
pub async fn channel_task_progress(
    app: AppHandle,
    task_id: i64,
    status: i32,
    result: String,
) -> Result<(), String> {
    let sender = app
        .state::<AppState>()
        .channel
        .lock()
        .unwrap()
        .as_ref()
        .map(|h| h.out.clone());
    if let Some(tx) = sender {
        tx.send(ClientMessage {
            payload: Some(ClientPayload::TaskProgress(TaskProgress {
                task_id,
                status,
                result,
            })),
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
