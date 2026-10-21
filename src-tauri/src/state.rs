use std::sync::Mutex;

use crate::commands::clipboard::ClipEntry;

pub struct AppState {
    pub clipboard: Mutex<Vec<ClipEntry>>,
    pub db: Mutex<rusqlite::Connection>,
}

impl AppState {
    pub fn new(db: rusqlite::Connection) -> Self {
        Self {
            clipboard: Mutex::new(Vec::new()),
            db: Mutex::new(db),
        }
    }
}
