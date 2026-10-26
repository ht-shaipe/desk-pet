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
    /// 文本内容（kind="image" 时存 PNG 文件名）
    pub text: String,
    /// "text" | "image"
    #[serde(default = "default_kind")]
    pub kind: String,
    pub updated_at: u64,
    /// 相同内容累计捕获次数（同内容再次出现在系统剪贴板时 +1）
    #[serde(default = "one")]
    pub copy_count: u32,
    /// 内容签名（kind="image" 时为 RGBA 字节的 FNV-1a 64 哈希，用于跨重启去重；文本条目为空串）
    #[serde(default)]
    pub sig: String,
}

fn default_kind() -> String {
    "text".to_string()
}

fn one() -> u32 {
    1
}

/// 收藏条目：字段名与 TS 类型 / stash 导入导出映射（lib/stash.ts）一致。
/// 注意：Tauri 传输键保持 snake_case（fav_type 等），camelCase 转换只发生在
/// 前端与 stash 导出格式 / 云端契约交互时，故这里不能加 rename_all = "camelCase"。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Favorite {
    /// 收藏主键：仓库/网页地址，笔记为 stash://note/<uuid> 伪地址
    pub url: String,
    pub name: String,
    pub description: String,
    /// repo | article | video | product | social | image | bookmark | note
    pub fav_type: String,
    /// 附加载荷 JSON（note 正文 content、image 缩略 src 等），由前端序列化
    pub payload: String,
    pub languages: Vec<String>,
    pub stars: i64,
    pub forks: i64,
    pub tags: Vec<String>,
    pub category: i64,
    pub category_name: String,
    /// ISO 时间字符串
    pub create_time: String,
    pub update_time: String,
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
