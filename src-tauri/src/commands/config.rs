use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppSettings {
    pub pet_x: Option<i32>,
    pub pet_y: Option<i32>,
    pub always_on_top: bool,
    #[serde(default)]
    pub locale: Option<String>,
    /// light | dark，缺省视为 light
    #[serde(default)]
    pub theme: Option<String>,
    /// 精灵图标显示大小（逻辑像素，32-64），缺省 48
    #[serde(default)]
    pub sprite_size: Option<u32>,
    /// 主题色（#rrggbb），缺省用内置蓝色
    #[serde(default)]
    pub accent_color: Option<String>,
    /// 远端账户 token（同步用，裸 Authorization 头）
    #[serde(default)]
    pub auth_token: Option<String>,
    /// 已登录账户（手机号）
    #[serde(default)]
    pub auth_user: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            pet_x: None,
            pet_y: None,
            always_on_top: true,
            locale: None,
            theme: None,
            sprite_size: None,
            accent_color: None,
            auth_token: None,
            auth_user: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LauncherItem {
    pub id: String,
    pub name: String,
    pub icon: String,
    /// app | url | command
    pub item_type: String,
    pub value: String,
    pub color: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LauncherConfig {
    pub items: Vec<LauncherItem>,
}

pub(crate) fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取应用数据目录: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建应用数据目录: {e}"))?;
    Ok(dir)
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned + Default>(
    app: &AppHandle,
    file: &str,
) -> Result<T, String> {
    let path = data_dir(app)?.join(file);
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

pub(crate) fn read_json_or_default<T: serde::de::DeserializeOwned + Default>(
    app: &AppHandle,
    file: &str,
) -> T {
    read_json(app, file).unwrap_or_default()
}

pub(crate) fn write_json<T: Serialize>(app: &AppHandle, file: &str, data: &T) -> Result<(), String> {
    let path = data_dir(app)?.join(file);
    let content = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    fs::write(&path, content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> AppSettings {
    read_json_or_default(&app, "settings.json")
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    write_json(&app, "settings.json", &settings)
}

#[tauri::command]
pub fn get_launcher(app: AppHandle) -> LauncherConfig {
    let state = app.state::<crate::state::AppState>();
    let conn = state.db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT id, name, icon, item_type, value, color FROM launcher ORDER BY sort_order",
    ) {
        Ok(s) => s,
        Err(_) => return LauncherConfig::default(),
    };
    let rows = stmt.query_map([], |row| {
        Ok(LauncherItem {
            id: row.get(0)?,
            name: row.get(1)?,
            icon: row.get(2)?,
            item_type: row.get(3)?,
            value: row.get(4)?,
            color: row.get(5)?,
        })
    });
    match rows {
        Ok(iter) => LauncherConfig {
            items: iter.filter_map(|r| r.ok()).collect(),
        },
        Err(_) => LauncherConfig::default(),
    }
}

#[tauri::command]
pub fn save_launcher(app: AppHandle, config: LauncherConfig) -> Result<(), String> {
    let state = app.state::<crate::state::AppState>();
    let conn = state.db.lock().unwrap();
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM launcher", [])
        .map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT OR REPLACE INTO launcher (id, name, icon, item_type, value, color, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(|e| e.to_string())?;
        for (i, item) in config.items.iter().enumerate() {
            stmt.execute(rusqlite::params![
                item.id,
                item.name,
                item.icon,
                item.item_type,
                item.value,
                item.color,
                i as i64
            ])
            .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())
}
