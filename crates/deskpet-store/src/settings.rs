use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::Result;

/// AI 模型配置（OpenAI 兼容接口）。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AiModel {
    pub id: String,
    pub name: String,
    /// 接口基础地址，如 https://api.openai.com/v1
    pub base_url: String,
    pub api_key: String,
    /// 模型名，如 gpt-4o-mini
    pub model: String,
}

/// 聊天消息。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    /// user | assistant
    pub role: String,
    pub content: String,
}

/// MCP 服务器配置（前端在 AI 对话里管理，Rust 侧据此建立连接）。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct McpServerConfig {
    /// 生成的唯一 id（m-xxxx 同款生成规则）
    pub id: String,
    /// 显示名（同时用于派生工具前缀，需可净化为 ASCII 标识符）
    pub name: String,
    /// stdio | http
    #[serde(default)]
    pub transport: Option<String>,
    /// stdio：可执行命令，如 npx / node / python
    #[serde(default)]
    pub command: Option<String>,
    /// stdio：命令参数列表
    #[serde(default)]
    pub args: Vec<String>,
    /// stdio：附加环境变量
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    /// http：Streamable HTTP 端点 URL
    #[serde(default)]
    pub url: Option<String>,
    /// 启用后随应用启动自动连接
    #[serde(default)]
    pub enabled: Option<bool>,
}

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
    /// 稳定设备指纹（安装后生成并持久化，gRPC 通道握手用，重连/重启不变）
    #[serde(default)]
    pub device_id: Option<String>,
    /// 自定义精灵头像（app data 内绝对路径），None 用内置图标
    #[serde(default)]
    pub pet_avatar: Option<String>,
    /// 应用显示名称，缺省为 "Desk Pet"
    #[serde(default)]
    pub app_name: Option<String>,
    /// 是否在 Dock 中显示（仅 macOS），缺省 true
    #[serde(default)]
    pub show_dock: Option<bool>,
    /// AI 模型配置列表
    #[serde(default)]
    pub ai_models: Vec<AiModel>,
    /// 当前使用的 AI 模型 id
    #[serde(default)]
    pub ai_active: Option<String>,
    /// 聊天历史（不与远端同步）
    #[serde(default)]
    pub ai_messages: Vec<ChatMessage>,
    /// 收藏云同步开关（鉴权 token 复用 auth_token）
    #[serde(default)]
    pub favorite_sync: Option<bool>,
    /// 收藏增量拉取基线（服务端时间 serverNow）
    #[serde(default)]
    pub favorite_last_sync: Option<String>,
    /// 剪贴板云同步开关（鉴权 token 复用 auth_token）
    #[serde(default)]
    pub clipboard_sync: Option<bool>,
    /// 剪贴板增量拉取基线（服务端时间 serverNow）
    #[serde(default)]
    pub clipboard_last_sync: Option<String>,
    /// 番茄钟专注时长（分钟），缺省 25
    #[serde(default)]
    pub pomo_focus_min: Option<u32>,
    /// 番茄钟短休息时长（分钟），缺省 5
    #[serde(default)]
    pub pomo_short_min: Option<u32>,
    /// 番茄钟长休息时长（分钟），缺省 15
    #[serde(default)]
    pub pomo_long_min: Option<u32>,
    /// 收起面板时在精灵下方显示时钟，缺省开启（由前端归一化）
    #[serde(default)]
    pub pet_clock: Option<bool>,
    /// MCP 服务器配置列表（AI 对话可调用的外部工具来源）
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    /// 悬浮监控条是否显示（主窗口启动时据此自动恢复）
    #[serde(default)]
    pub monitor_bar_open: Option<bool>,
    /// 悬浮监控条鼠标穿透
    #[serde(default)]
    pub monitor_bar_click_through: Option<bool>,
    /// 悬浮监控条分区开关：CPU / 内存 / 网速，缺省均显示
    #[serde(default)]
    pub monitor_bar_cpu: Option<bool>,
    #[serde(default)]
    pub monitor_bar_mem: Option<bool>,
    #[serde(default)]
    pub monitor_bar_net: Option<bool>,
    /// 菜单栏监控文字（macOS 托盘标题）是否显示
    #[serde(default)]
    pub monitor_menubar_open: Option<bool>,
    /// 菜单栏监控文字显示期间是否保留桌宠托盘图标，缺省保留
    #[serde(default)]
    pub monitor_menubar_show_icon: Option<bool>,
    /// 监控刷新间隔（毫秒，1000/2000/5000）：工具页、悬浮条、菜单栏共用
    #[serde(default)]
    pub monitor_refresh_ms: Option<u32>,
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
            device_id: None,
            pet_avatar: None,
            app_name: None,
            show_dock: None,
            ai_models: Vec::new(),
            ai_active: None,
            ai_messages: Vec::new(),
            favorite_sync: None,
            favorite_last_sync: None,
            clipboard_sync: None,
            clipboard_last_sync: None,
            pomo_focus_min: None,
            pomo_short_min: None,
            pomo_long_min: None,
            pet_clock: None,
            mcp_servers: Vec::new(),
            monitor_bar_open: None,
            monitor_bar_click_through: None,
            monitor_bar_cpu: None,
            monitor_bar_mem: None,
            monitor_bar_net: None,
            monitor_menubar_open: None,
            monitor_menubar_show_icon: None,
            monitor_refresh_ms: None,
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
