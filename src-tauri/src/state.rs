use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;

use deskpet_os::monitor::Monitor;
use deskpet_store::{ClipEntry, Store};

use crate::channel::ChannelHandle;
use crate::mcp::McpServerHandle;

pub struct AppState {
    pub clipboard: Mutex<Vec<ClipEntry>>,
    pub store: Mutex<Store>,
    pub channel: Mutex<Option<ChannelHandle>>,
    /// 已连接的 MCP 服务器（AI 对话可调用的外部工具），key 为服务器配置 id
    pub mcp: Mutex<HashMap<String, McpServerHandle>>,
    /// 图片管理器后台任务取消标志：key 为 dir_id(扫描) / -1(去重)
    pub image_tasks: Mutex<HashMap<i64, Arc<AtomicBool>>>,
    /// 文档管理后台扫描取消标志：key 为 doc dir_id
    pub doc_tasks: Mutex<HashMap<i64, Arc<AtomicBool>>>,
    /// 系统监控采样器：跨调用保存计数，用于计算 CPU 使用率与网络速率差值
    pub monitor: Mutex<Monitor>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            clipboard: Mutex::new(Vec::new()),
            store: Mutex::new(store),
            channel: Mutex::new(None),
            mcp: Mutex::new(HashMap::new()),
            image_tasks: Mutex::new(HashMap::new()),
            doc_tasks: Mutex::new(HashMap::new()),
            monitor: Mutex::new(Monitor::new()),
        }
    }
}
