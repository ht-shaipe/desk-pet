use std::sync::Mutex;

use crate::commands::clipboard::ClipEntry;

#[derive(Default)]
pub struct AppState {
    pub clipboard: Mutex<Vec<ClipEntry>>,
}
