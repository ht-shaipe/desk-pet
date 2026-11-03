use std::sync::Mutex;

use deskpet_store::{ClipEntry, Store};

use crate::channel::ChannelHandle;

pub struct AppState {
    pub clipboard: Mutex<Vec<ClipEntry>>,
    pub store: Mutex<Store>,
    pub channel: Mutex<Option<ChannelHandle>>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            clipboard: Mutex::new(Vec::new()),
            store: Mutex::new(store),
            channel: Mutex::new(None),
        }
    }
}
