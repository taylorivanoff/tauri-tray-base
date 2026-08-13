use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;

use crate::TrayClickAction;

#[derive(Default)]
pub struct Quitting(pub std::sync::atomic::AtomicBool);

impl Quitting {
    pub fn set(&self, value: bool) {
        self.0
            .store(value, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn get(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub struct TrayBaseState {
    pub app_name: String,
    pub settings_path: PathBuf,
    pub settings: Arc<Mutex<crate::settings::PersistedSettings>>,
    pub defaults: HashMap<String, Value>,
    pub extra_tray_items: Arc<Mutex<Vec<TrayExtraItem>>>,
    pub show_hide: bool,
    pub show_always_on_top: bool,
    pub tray_on_click: TrayClickAction,
    pub main_window_label: String,
    /// When false, the main window stays hidden until the app calls `show_main`.
    pub auto_show_main_on_ready: bool,
    pub on_before_quit: Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>,
}

#[derive(Debug, Clone)]
pub struct TrayExtraItem {
    pub id: String,
    pub label: String,
}
