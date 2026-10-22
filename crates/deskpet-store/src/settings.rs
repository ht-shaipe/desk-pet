use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::Result;

/// 应用设置（settings.json）。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppSettings {
    pub pet_x: Option<i32>,
    pub pet_y: Option<i32>,
    pub always_on_top: bool,
    #[serde(default)]
    pub locale: Option<String>,
    /// light | dark | auto，缺省视为 auto（跟随系统）
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
    /// 自定义精灵头像（app data 内绝对路径），None 用内置图标
    #[serde(default)]
    pub pet_avatar: Option<String>,
    /// 应用显示名称，缺省为 "Desk Pet"
    #[serde(default)]
    pub app_name: Option<String>,
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
            pet_avatar: None,
            app_name: None,
        }
    }
}

const SETTINGS_FILE: &str = "settings.json";

/// 读取设置（不存在或损坏时返回默认值）。
pub fn load(dir: &Path) -> AppSettings {
    read(dir).unwrap_or_default()
}

/// 保存设置。
pub fn save(dir: &Path, settings: &AppSettings) -> Result<()> {
    let content = serde_json::to_string_pretty(settings)?;
    fs::write(dir.join(SETTINGS_FILE), content)?;
    Ok(())
}

fn read(dir: &Path) -> Result<AppSettings> {
    let content = fs::read_to_string(dir.join(SETTINGS_FILE))?;
    Ok(serde_json::from_str(&content)?)
}
