use std::collections::HashMap;
use std::sync::Mutex;

use deskpet_store::{ClipEntry, Store};

use crate::channel::ChannelHandle;
use crate::mcp::McpServerHandle;

pub struct AppState {
    pub clipboard: Mutex<Vec<ClipEntry>>,
    pub store: Mutex<Store>,
    pub channel: Mutex<Option<ChannelHandle>>,
    /// 已连接的 MCP 服务器（AI 对话可调用的外部工具），key 为服务器配置 id
    pub mcp: Mutex<HashMap<String, McpServerHandle>>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            clipboard: Mutex::new(Vec::new()),
            store: Mutex::new(store),
            channel: Mutex::new(None),
            mcp: Mutex::new(HashMap::new()),
        }
    }
}
