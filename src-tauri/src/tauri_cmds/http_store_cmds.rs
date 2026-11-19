//! 接口请求收藏、环境变量、响应历史、Cookie 与 SSE 流的命令；请求发送在 toolbox_cmds。

use std::collections::HashMap;
use std::sync::{Mutex as StdMutex, OnceLock};

use deskpet_os::template::Vars;
use deskpet_store::{HttpEnv, HttpHistory, HttpRespRecord, HttpSaved};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use crate::state::AppState;

/// 当前启用环境的变量（无启用环境返回空表）。
pub(crate) fn active_env_vars(app: &AppHandle) -> Vars {
    let vars = app
        .state::<AppState>()
        .store
        .lock()
        .unwrap()
        .active_http_env()
        .ok()
        .flatten()
        .and_then(|env| serde_json::from_str::<Vec<deskpet_os::http::HeaderPair>>(&env.vars).ok())
        .unwrap_or_default();
    vars.into_iter()
        .filter(|p| !p.name.trim().is_empty())
        .map(|p| (p.name.trim().to_string(), p.value))
        .collect()
}

/// 全部接口收藏（默认分组在前，组内按 sort 升序）。
#[tauri::command]
pub fn list_http_saved(app: AppHandle) -> Vec<HttpSaved> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .list_http_saved()
        .inspect_err(|e| eprintln!("[http] list saved failed: {e}"))
        .unwrap_or_default()
}

/// 新增或更新接口收藏（id 为空时生成 uuid）。
#[tauri::command]
pub fn upsert_http_saved(app: AppHandle, mut item: HttpSaved) -> Result<(), String> {
    if item.id.trim().is_empty() {
        item.id = format!("s-{}", uuid::Uuid::new_v4());
    }
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .upsert_http_saved(&item)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 删除接口收藏（按 id），返回受影响条数。
#[tauri::command]
pub fn delete_http_saved(app: AppHandle, id: String) -> Result<usize, String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .delete_http_saved(&id)
        .map_err(|e| e.to_string())
}

/// 重命名集合（整组迁移），返回受影响条数。
#[tauri::command]
pub fn rename_http_group(app: AppHandle, old: String, new: String) -> Result<usize, String> {
    if new.trim().is_empty() {
        return Err("集合名不能为空".into());
    }
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .rename_http_group(&old, &new)
        .map_err(|e| e.to_string())
}

/// 删除集合（连同其中收藏），返回受影响条数。
#[tauri::command]
pub fn delete_http_group(app: AppHandle, group: String) -> Result<usize, String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .delete_http_group(&group)
        .map_err(|e| e.to_string())
}

/// 最近请求历史（新 → 旧，上限 100 条）。
#[tauri::command]
pub fn list_http_history(app: AppHandle) -> Vec<HttpHistory> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .list_http_history(100)
        .inspect_err(|e| eprintln!("[http] list history failed: {e}"))
        .unwrap_or_default()
}

/// 清空请求历史。
#[tauri::command]
pub fn clear_http_history(app: AppHandle) -> Result<(), String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .clear_http_history()
        .map_err(|e| e.to_string())
}

/* ---------- 环境变量 ---------- */

/// 全部环境（启用环境在前）。
#[tauri::command]
pub fn list_http_envs(app: AppHandle) -> Vec<HttpEnv> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .list_http_envs()
        .inspect_err(|e| eprintln!("[http] list envs failed: {e}"))
        .unwrap_or_default()
}

/// 新增或更新环境（id 为空时生成 uuid）。
#[tauri::command]
pub fn upsert_http_env(app: AppHandle, mut item: HttpEnv) -> Result<(), String> {
    if item.id.trim().is_empty() {
        item.id = format!("e-{}", uuid::Uuid::new_v4());
    }
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .upsert_http_env(&item)
        .map_err(|e| e.to_string())
}

/// 删除环境。
#[tauri::command]
pub fn delete_http_env(app: AppHandle, id: String) -> Result<usize, String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .delete_http_env(&id)
        .map_err(|e| e.to_string())
}

/// 切换启用环境（null 表示不启用任何环境）。
#[tauri::command]
pub fn set_active_http_env(app: AppHandle, id: Option<String>) -> Result<(), String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .set_active_http_env(id.as_deref())
        .map_err(|e| e.to_string())
}

/* ---------- 响应历史 ---------- */

/// 某请求键下的响应历史（新 → 旧）。
#[tauri::command]
pub fn list_http_responses(app: AppHandle, spec_key: String) -> Vec<HttpRespRecord> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .list_http_responses(&spec_key, 20)
        .inspect_err(|e| eprintln!("[http] list responses failed: {e}"))
        .unwrap_or_default()
}

/// 清空响应历史（spec_key 为空表示全部）。
#[tauri::command]
pub fn clear_http_responses(app: AppHandle, spec_key: Option<String>) -> Result<usize, String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .clear_http_responses(spec_key.as_deref())
        .map_err(|e| e.to_string())
}

/* ---------- Cookie ---------- */

/// 当前共享 Cookie jar 的全部 Cookie。
#[tauri::command]
pub fn http_cookie_list() -> Vec<deskpet_os::http::CookieEntry> {
    deskpet_os::http::cookie_list()
}

/// 清空共享 Cookie jar。
#[tauri::command]
pub fn http_cookie_clear() {
    deskpet_os::http::cookie_clear();
}

/* ---------- SSE 流式请求 ---------- */

/// 运行中的 SSE 任务（streamId → 句柄），用于停止。
static SSE_TASKS: OnceLock<StdMutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>> =
    OnceLock::new();

/// 启动 SSE 流式请求：每个事件经 `http-sse` 事件推送，结束（或出错）经 `http-sse-done` 推送，返回 streamId。
#[tauri::command]
pub async fn http_sse_start(
    app: AppHandle,
    window: WebviewWindow,
    spec: deskpet_os::http::HttpRequestSpec,
) -> Result<String, String> {
    let vars = active_env_vars(&app);
    let id = format!("sse-{}", uuid::Uuid::new_v4());
    let stream_id = id.clone();
    let task = tauri::async_runtime::spawn(async move {
        let window = window.clone();
        let result = deskpet_os::http::send_sse(&spec, &vars, |event| {
            let payload = serde_json::json!({ "streamId": stream_id, "event": event });
            let _ = window.emit("http-sse", payload);
        })
        .await;
        let payload = serde_json::json!({ "streamId": stream_id, "stats": result });
        let _ = window.emit("http-sse-done", payload);
    });
    SSE_TASKS
        .get_or_init(|| StdMutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(id.clone(), task);
    Ok(id)
}

/// 停止 SSE 流式请求。
#[tauri::command]
pub fn http_sse_stop(id: String) {
    if let Some(map) = SSE_TASKS.get() {
        if let Some(handle) = map.lock().unwrap().remove(&id) {
            handle.abort();
        }
    }
}
