pub mod actions;
pub mod clipboard;
pub mod error;
pub mod files;
pub mod launch;
mod proc;

pub use error::OsError;

pub type Result<T> = std::result::Result<T, OsError>;
