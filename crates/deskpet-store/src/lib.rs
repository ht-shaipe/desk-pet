pub mod ai_usage;
pub mod avatar;
pub mod docs;
pub mod gallery;
mod error;
mod migrate;
mod models;
pub mod opencode;
pub mod settings;
mod store;

pub use error::StoreError;
pub use ai_usage::{
    AiUsageEventInput, AiUsageFileState, AiUsageGroupRow, AiUsageRangeRaw, AiUsageToolModelRow,
};
pub use opencode::{read_opencode_messages, OpenCodeMessageRow};
pub use docs::{DocDir, DocExtStat, DocItem, DocItemMeta, DocQuery};
pub use gallery::{ImageDupGroup, ImageItemMeta, ImageQuery};
pub use models::{ChatEntry, ClipEntry, Favorite, ImageDir, ImageItem, ImageTag, LauncherConfig, LauncherItem};
pub use settings::AppSettings;
pub use store::Store;

pub type Result<T> = std::result::Result<T, StoreError>;
