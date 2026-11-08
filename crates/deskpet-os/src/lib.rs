pub mod actions;
pub mod animation;
pub mod apps;
pub mod clipboard;
pub mod device;
pub mod disk;
pub mod error;
pub mod files;
pub mod http;
pub mod icons;
pub mod imaging;
pub mod launch;
// menubar 模块跨平台编译：ChipSections 是悬浮条/菜单栏共用的纯数据类型；
// AppKit 状态项绘制仅在 macOS 编译（模块内 cfg 门控）
pub mod menubar;
pub mod monitor;
mod proc;
pub mod pointer;
pub mod watermark;
#[cfg(target_os = "macos")]
pub mod window;

pub use error::OsError;

pub type Result<T> = std::result::Result<T, OsError>;
