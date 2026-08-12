//! Shared tray-first Tauri 2 scaffold (mirrors electron-tray-base).

mod commands;
mod settings;
mod state;
mod tray;
mod window;

pub use commands::{app_get_state, settings_get, settings_set};
pub use settings::{
    clamp_opacity, save_settings, CommonSettings, PersistedSettings, DEFAULT_OPACITY, MIN_OPACITY,
    START_MINIMISED_ARG,
};
pub use state::{Quitting, TrayBaseState, TrayExtraItem};
pub use tray::{rebuild_tray_menu, setup_tray, TraySetupOptions};
pub use window::{
    apply_always_on_top, apply_opacity, hide_main, on_window_event, request_quit, show_main,
    toggle_main, MAIN_WINDOW_LABEL,
};

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, Runtime, Wry};

#[derive(Clone)]
pub struct TrayBaseOptions {
    pub app_name: String,
    pub settings_file_name: String,
    pub defaults: HashMap<String, Value>,
    pub extra_tray_items: Vec<TrayExtraItem>,
    pub show_hide: bool,
    pub show_always_on_top: bool,
    pub tray_on_click: TrayClickAction,
    pub main_window_label: String,
}

impl Default for TrayBaseOptions {
    fn default() -> Self {
        Self {
            app_name: "App".into(),
            settings_file_name: "settings.json".into(),
            defaults: HashMap::new(),
            extra_tray_items: Vec::new(),
            show_hide: true,
            show_always_on_top: true,
            tray_on_click: TrayClickAction::Toggle,
            main_window_label: MAIN_WINDOW_LABEL.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayClickAction {
    Toggle,
    Show,
}

pub fn install_state<R: Runtime>(app: &AppHandle<R>, options: TrayBaseOptions) -> tauri::Result<()> {
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| ".".into()));
    let _ = std::fs::create_dir_all(&data_dir);

    let settings_path = data_dir.join(&options.settings_file_name);
    let settings = settings::load_or_create(&settings_path, &options.defaults)?;

    let state = TrayBaseState {
        app_name: options.app_name.clone(),
        settings_path,
        settings: Arc::new(Mutex::new(settings)),
        defaults: options.defaults,
        extra_tray_items: Arc::new(Mutex::new(options.extra_tray_items)),
        show_hide: options.show_hide,
        show_always_on_top: options.show_always_on_top,
        tray_on_click: options.tray_on_click,
        main_window_label: options.main_window_label,
        on_before_quit: Arc::new(Mutex::new(None)),
    };

    app.manage(state);
    app.manage(Quitting::default());
    Ok(())
}

pub fn with_common_plugins(builder: tauri::Builder<Wry>) -> tauri::Builder<Wry> {
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_autostart::Builder::new().build())
}

pub fn apply_window_settings<R: Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<TrayBaseState>() else {
        return;
    };
    let settings = state.settings.lock().clone();
    apply_always_on_top(app, settings.always_on_top);
    apply_opacity(app, settings.opacity);
}

pub fn sync_autostart<R: Runtime>(app: &AppHandle<R>) {
    use tauri_plugin_autostart::ManagerExt;

    let Some(state) = app.try_state::<TrayBaseState>() else {
        return;
    };
    let enabled = state.settings.lock().start_minimised;
    let autostart = app.autolaunch();
    if enabled {
        let _ = autostart.enable();
    } else {
        let _ = autostart.disable();
    }
}

pub fn set_on_before_quit(app: &AppHandle<Wry>, hook: impl Fn() + Send + Sync + 'static) {
    let Some(state) = app.try_state::<TrayBaseState>() else {
        return;
    };
    *state.on_before_quit.lock() = Some(Box::new(hook));
}

pub fn emit_to_renderer<R: Runtime, S: serde::Serialize + Clone>(
    app: &AppHandle<R>,
    event: &str,
    payload: S,
) {
    let _ = app.emit(event, payload);
}

pub fn was_launched_minimised() -> bool {
    std::env::args().any(|a| a == START_MINIMISED_ARG)
}
