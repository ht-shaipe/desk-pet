pub mod actions;
pub mod apps;
pub mod clipboard;
pub mod disk;
pub mod error;
pub mod files;
pub mod icons;
pub mod launch;
mod proc;

pub use error::OsError;

pub type Result<T> = std::result::Result<T, OsError>;
