use channel_client::channel::{
    client_message::Payload as ClientPayload, ClientMessage, TaskAck, TaskProgress,
};
use tauri::{AppHandle, Manager};

use crate::paths::data_dir;
use crate::state::AppState;

/// 启动实时连接（应用启动与登录态变化时调用）。token 由前端传入，空串表示匿名连接。
/// 返回设备指纹。
#[tauri::command]
pub fn channel_start(app: AppHandle, token: String) -> Result<String, String> {
    let dir = data_dir(&app)?;
    let mut settings = deskpet_store::settings::load(&dir);
    let device_id = settings.device_id.clone().unwrap_or_else(|| {
        let id = uuid::Uuid::new_v4().to_string();
        settings.device_id = Some(id.clone());
        let _ = deskpet_store::settings::save(&dir, &settings);
        id
    });
    crate::channel::start(app, device_id.clone(), token);
    Ok(device_id)
}

/// 停止实时连接（退出登录时调用）。
#[tauri::command]
pub fn channel_stop(app: AppHandle) -> Result<(), String> {
    if let Some(h) = app.state::<AppState>().channel.lock().unwrap().take() {
        let _ = h.cancel.try_send(());
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
