//! Desk Pet 磁盘核心：扫描、计量、分类、删除护栏与执行。
//!
//! 职责边界（对齐 disktree 的分层思路，仅借鉴设计、未移植代码）：
//! - 输入是路径 + 选项，输出是候选/计划/结果，不依赖 Tauri、UI 与 SSE；
//! - 数字按磁盘分配大小计量（删除后真正能释放的字节），硬链接只计一次；
//! - 所有危险操作集中在 [`removal`]，配真实临时目录测试；
//! - 平台差异用 `#[cfg]` 收口。

pub mod classify;
pub mod insights;
pub mod pool;
pub mod removal;
pub mod scan;
pub mod size;
pub mod space;
pub mod tree;
pub mod treemap;

/// 核心 crate 统一错误。
#[derive(Debug, thiserror::Error)]
pub enum DiskError {
    #[error("扫描被取消")]
    Cancelled,
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}
