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
#[cfg(target_os = "macos")]
pub mod menubar;
pub mod monitor;
mod proc;
pub mod pointer;
pub mod watermark;
#[cfg(target_os = "macos")]
pub mod window;

pub use error::OsError;

pub type Result<T> = std::result::Result<T, OsError>;
