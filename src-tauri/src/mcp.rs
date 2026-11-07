//! MCP（Model Context Protocol）客户端管理。
//!
//! 按 settings.json 里的 `mcp_servers` 配置连接外部 MCP 服务器（stdio 子进程或
//! Streamable HTTP），把它们的工具暴露给 AI 对话。连接句柄（rmcp 运行服务）保存在
//! `AppState.mcp`，断开 = 取消令牌 + 从表中移除（DropGuard 优雅关闭子进程）。

use std::sync::Arc;

use rmcp::{
    model::{CallToolRequestParam, ClientCapabilities, ClientInfo, Implementation, JsonObject},
    service::{RoleClient, RunningService, RunningServiceCancellationToken, ServiceExt},
    transport::{StreamableHttpClientTransport, TokioChildProcess},
};
use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};
use tokio::process::Command;

use crate::state::AppState;
use deskpet_store::settings::McpServerConfig;

/// 暴露给前端的工具描述。
#[derive(Serialize, Clone)]
pub struct McpToolDto {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// 工具入参 JSON Schema
    pub input_schema: Value,
}

impl From<&rmcp::model::Tool> for McpToolDto {
    fn from(tool: &rmcp::model::Tool) -> Self {
        Self {
            name: tool.name.to_string(),
            title: tool.title.clone(),
            description: tool.description.as_ref().map(|s| s.to_string()),
            input_schema: serde_json::to_value(tool.input_schema.as_ref())
                .unwrap_or_else(|_| json!({})),
        }
    }
}

/// 已连接服务器的句柄：共享运行服务（供并发调用）+ 工具缓存 + 取消令牌。
pub struct McpServerHandle {
    service: Arc<RunningService<RoleClient, ClientInfo>>,
    pub tools: Vec<McpToolDto>,
    cancel: RunningServiceCancellationToken,
}

/// 本应用作为 MCP 客户端的握手信息。
fn client_info() -> ClientInfo {
    ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "desk-pet".into(),
            title: Some("Desk Pet".into()),
            version: env!("CARGO_PKG_VERSION").into(),
            ..Default::default()
        },
    }
}

/// 连接一个 MCP 服务器：完成初始化握手并缓存工具列表。
async fn connect_server(cfg: &McpServerConfig) -> Result<McpServerHandle, String> {
    let service = match cfg.transport.as_deref() {
        Some("http") => {
            let url = cfg.url.clone().unwrap_or_default();
            if url.trim().is_empty() {
                return Err("http 传输必须填写 URL".into());
            }
            client_info()
                .serve(StreamableHttpClientTransport::from_uri(url))
                .await
        }
        // 缺省按 stdio 处理
        _ => {
            let program = cfg.command.clone().unwrap_or_default();
            if program.trim().is_empty() {
                return Err("stdio 传输必须填写启动命令".into());
            }
            let mut cmd = Command::new(&program);
            cmd.args(&cfg.args).envs(&cfg.env);
            let transport =
                TokioChildProcess::new(cmd).map_err(|e| format!("启动 MCP 进程失败: {e}"))?;
            client_info().serve(transport).await
        }
    }
    .map_err(|e| format!("MCP 握手失败: {e}"))?;

    let tools = service
        .list_all_tools()
        .await
        .map_err(|e| format!("获取 MCP 工具列表失败: {e}"))?
        .iter()
        .map(McpToolDto::from)
        .collect();
    let cancel = service.cancellation_token();
    Ok(McpServerHandle {
        service: Arc::new(service),
        tools,
        cancel,
    })
}

/// 连接并注册到 AppState（同名旧连接先断开）。返回工具列表。
pub async fn connect_and_register(app: &AppHandle, cfg: McpServerConfig) -> Result<Vec<McpToolDto>, String> {
    let handle = connect_server(&cfg).await?;
    let tools = handle.tools.clone();
    let state = app.state::<AppState>();
    let mut map = state.mcp.lock().unwrap();
    if let Some(old) = map.insert(cfg.id.clone(), handle) {
        old.cancel.cancel();
    }
    Ok(tools)
}

/// 断开指定服务器。返回是否原有连接。
pub fn disconnect(app: &AppHandle, server_id: &str) -> bool {
    if let Some(old) = app.state::<AppState>().mcp.lock().unwrap().remove(server_id) {
        old.cancel.cancel();
        true
    } else {
        false
    }
}

/// 已连接的服务器 id 列表（前端显示状态点）。
pub fn connected_ids(app: &AppHandle) -> Vec<String> {
    app.state::<AppState>().mcp.lock().unwrap().keys().cloned().collect()
}

/// 调用已连接服务器上的工具，聚合文本内容返回 `{ok, text, structured}`。
pub async fn call_tool(
    app: &AppHandle,
    server_id: &str,
    tool: &str,
    arguments: &str,
) -> Result<Value, String> {
    // 先克隆出 Arc 再释放锁，锁不得跨 await
    let service = {
        let state = app.state::<AppState>();
        let map = state.mcp.lock().unwrap();
        map.get(server_id)
            .ok_or_else(|| "MCP 服务器未连接".to_string())?
            .service
            .clone()
    };
    let arguments: Option<JsonObject> = if arguments.trim().is_empty() {
        None
    } else {
        let v: Value =
            serde_json::from_str(arguments).map_err(|e| format!("工具参数不是合法 JSON: {e}"))?;
        Some(
            v.as_object()
                .cloned()
                .ok_or_else(|| "工具参数必须是 JSON 对象".to_string())?,
        )
    };
    let result = service
        .call_tool(CallToolRequestParam {
            // Cow<'static>：必须拿所有权，&str 生命周期逃不出本函数
            name: tool.to_owned().into(),
            arguments,
        })
        .await
        .map_err(|e| format!("MCP 工具调用失败: {e}"))?;
    let mut text = String::new();
    for item in &result.content {
        // Content = Annotated<RawContent>，Deref 到 RawContent::as_text
        if let Some(t) = item.as_text() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&t.text);
        }
    }
    Ok(json!({
        "ok": !result.is_error.unwrap_or(false),
        "text": text,
        "structured": result.structured_content,
    }))
}

/// 启动时对 enabled 的服务器静默连接（失败只记日志，不阻断应用）。
pub fn connect_enabled_at_startup(app: &AppHandle) {
    let dir = crate::paths::data_dir(app).unwrap_or_default();
    let servers = deskpet_store::settings::load(&dir).mcp_servers;
    for cfg in servers.into_iter().filter(|s| s.enabled.unwrap_or(true)) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(err) = connect_and_register(&app, cfg).await {
                eprintln!("[desk-pet] MCP 服务器自动连接失败: {err}");
            }
        });
    }
}
