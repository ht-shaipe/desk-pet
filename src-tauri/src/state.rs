use std::sync::Mutex;

use deskpet_store::{ClipEntry, Store};

pub struct AppState {
    pub clipboard: Mutex<Vec<ClipEntry>>,
    pub store: Mutex<Store>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            clipboard: Mutex::new(Vec::new()),
            store: Mutex::new(store),
        }
    }
}
