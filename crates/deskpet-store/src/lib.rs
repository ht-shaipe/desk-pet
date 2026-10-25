pub mod avatar;
mod error;
mod migrate;
mod models;
pub mod settings;
mod store;

pub use error::StoreError;
pub use models::{ChatEntry, ClipEntry, Favorite, LauncherConfig, LauncherItem};
pub use settings::AppSettings;
pub use store::Store;

pub type Result<T> = std::result::Result<T, StoreError>;
