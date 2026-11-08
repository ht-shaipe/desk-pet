pub mod avatar;
pub mod docs;
pub mod gallery;
mod error;
mod migrate;
mod models;
pub mod settings;
mod store;

pub use error::StoreError;
pub use docs::{DocDir, DocItem, DocItemMeta, DocQuery};
pub use gallery::{ImageDupGroup, ImageItemMeta, ImageQuery};
pub use models::{ChatEntry, ClipEntry, Favorite, ImageDir, ImageItem, ImageTag, LauncherConfig, LauncherItem};
pub use settings::AppSettings;
pub use store::Store;

pub type Result<T> = std::result::Result<T, StoreError>;
