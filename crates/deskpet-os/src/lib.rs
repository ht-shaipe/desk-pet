pub mod actions;
pub mod apps;
pub mod clipboard;
pub mod disk;
pub mod error;
pub mod files;
pub mod http;
pub mod icons;
pub mod launch;
mod proc;
pub mod pointer;
#[cfg(target_os = "macos")]
pub mod window;

pub use error::OsError;

pub type Result<T> = std::result::Result<T, OsError>;
