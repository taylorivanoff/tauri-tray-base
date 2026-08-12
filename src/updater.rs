//! Auto-updater (mirrors electron-tray-base updater.js).

use std::time::Duration;

use tauri::{AppHandle, Wry};
use tauri_plugin_updater::UpdaterExt;

/// Same cadence as electron-tray-base (`UPDATE_CHECK_INTERVAL_MS`).
pub const UPDATE_CHECK_INTERVAL_MS: u64 = 24 * 60 * 60 * 1000;

/// Start a background check now, then every 24h. No-ops when not packaged or misconfigured.
pub fn setup_updater(app: &AppHandle<Wry>) {
    if !is_packaged() {
        return;
    }
    let handle = app.clone();
    std::thread::Builder::new()
        .name("updater".into())
        .spawn(move || {
            loop {
                let app = handle.clone();
                let _ = tauri::async_runtime::block_on(check_and_install(&app));
                std::thread::sleep(Duration::from_millis(UPDATE_CHECK_INTERVAL_MS));
            }
        })
        .ok();
}

/// Manual "Check for Updates" from the tray (also used by the menu event).
pub fn request_update_check(app: &AppHandle<Wry>) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = check_and_install(&handle).await {
            eprintln!("[updater] check failed: {e}");
        }
    });
}

async fn check_and_install(app: &AppHandle<Wry>) -> tauri_plugin_updater::Result<()> {
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[updater] unavailable: {e}");
            return Ok(());
        }
    };

    let Some(update) = updater.check().await? else {
        return Ok(());
    };

    eprintln!(
        "[updater] found {} -> {}, downloading…",
        app.package_info().version,
        update.version
    );

    update
        .download_and_install(|_, _| {}, || {})
        .await?;

    // Windows NSIS install typically exits the process; restart covers other platforms.
    app.restart();
}

fn is_packaged() -> bool {
    // Skip background checks in `tauri dev` / debug builds.
    !cfg!(debug_assertions)
}
