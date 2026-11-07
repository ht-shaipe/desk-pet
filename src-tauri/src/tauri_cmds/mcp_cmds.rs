use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::mcp::{self, McpToolDto};

/// 手动连接 MCP 服务器（读取 settings.json 中的配置），返回工具列表。
#[tauri::command]
pub async fn mcp_connect(app: AppHandle, server_id: String) -> Result<Vec<McpToolDto>, String> {
    let dir = crate::paths::data_dir(&app)?;
    let cfg = deskpet_store::settings::load(&dir)
        .mcp_servers
        .into_iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| "MCP 服务器配置不存在".to_string())?;
    mcp::connect_and_register(&app, cfg).await
}

/// 断开 MCP 服务器连接。
#[tauri::command]
pub fn mcp_disconnect(app: AppHandle, server_id: String) -> Result<(), String> {
    mcp::disconnect(&app, &server_id);
    Ok(())
}

/// 指定服务器的工具列表；未连接且配置为启用时自动懒连接。
#[tauri::command]
pub async fn mcp_server_tools(
    app: AppHandle,
    server_id: String,
) -> Result<Vec<McpToolDto>, String> {
    {
        let state = app.state::<crate::state::AppState>();
        let map = state.mcp.lock().unwrap();
        if let Some(handle) = map.get(&server_id) {
            return Ok(handle.tools.clone());
        }
    }
    // 未连接：配置存在且启用则懒连接，否则报错
    let dir = crate::paths::data_dir(&app)?;
    let cfg = deskpet_store::settings::load(&dir)
        .mcp_servers
        .into_iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| "MCP 服务器配置不存在".to_string())?;
    if !cfg.enabled.unwrap_or(true) {
        return Err("MCP 服务器未启用".into());
    }
    mcp::connect_and_register(&app, cfg).await
}

/// 调用 MCP 服务器工具（arguments 为 JSON 对象字符串），返回 {ok, text, structured}。
#[tauri::command]
pub async fn mcp_call_tool(
    app: AppHandle,
    server_id: String,
    tool: String,
    arguments: String,
) -> Result<Value, String> {
    mcp::call_tool(&app, &server_id, &tool, &arguments).await
}

/// 已连接的服务器 id 列表。
#[tauri::command]
pub fn mcp_server_status(app: AppHandle) -> Vec<String> {
    mcp::connected_ids(&app)
}
