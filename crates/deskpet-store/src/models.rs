/// 便签
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub id: String,
    pub content: String,
    pub color: String,
    pub updated_at: u64,
}

/// 剪贴板历史条目
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ClipEntry {
    pub id: String,
    pub text: String,
    pub updated_at: u64,
}

/// 启动器条目
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LauncherItem {
    pub id: String,
    pub name: String,
    pub icon: String,
    /// app | url | command
    pub item_type: String,
    pub value: String,
    pub color: String,
}

/// 启动器配置（前端契约保持 { items: [...] } 形状）
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct LauncherConfig {
    pub items: Vec<LauncherItem>,
}

/// 聊天消息（SQLite 持久化）
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ChatEntry {
    pub id: i64,
    /// user | assistant
    pub role: String,
    pub content: String,
    pub created_at: u64,
}
