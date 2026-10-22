mod error;
mod migrate;
mod models;
mod store;

pub use error::StoreError;
pub use models::{ClipEntry, LauncherConfig, LauncherItem, Note};
pub use store::Store;

pub type Result<T> = std::result::Result<T, StoreError>;
