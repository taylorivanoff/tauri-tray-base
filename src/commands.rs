use tauri::{AppHandle, State};

use crate::settings::save;
use crate::state::TrayBaseState;
use crate::tray::rebuild_tray_menu;
use crate::window::{apply_always_on_top, apply_opacity};
use crate::{emit_to_renderer, sync_autostart};

#[tauri::command]
pub fn settings_get(state: State<'_, TrayBaseState>) -> serde_json::Value {
    state.settings.lock().to_value()
}

#[tauri::command]
pub fn settings_set(
    app: AppHandle,
    state: State<'_, TrayBaseState>,
    partial: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let next = {
        let mut settings = state.settings.lock();
        settings.merge_partial(&partial);
        let _ = save(&state.settings_path, &settings);
        settings.to_value()
    };

    if partial.get("alwaysOnTop").is_some() {
        let aot = state.settings.lock().always_on_top;
        apply_always_on_top(&app, aot);
    }
    if partial.get("opacity").is_some() {
        let opacity = state.settings.lock().opacity;
        apply_opacity(&app, opacity);
    }
    if partial.get("startMinimised").is_some() {
        sync_autostart(&app);
    }

    emit_to_renderer(&app, "settings:changed", next.clone());
    let _ = rebuild_tray_menu(&app);
    Ok(next)
}

#[tauri::command]
pub fn app_get_state(app: AppHandle, state: State<'_, TrayBaseState>) -> serde_json::Value {
    let settings = state.settings.lock().to_value();
    let version = app.package_info().version.to_string();
    serde_json::json!({
        "version": version,
        "settings": settings,
        "platform": std::env::consts::OS,
    })
}
