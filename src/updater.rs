//! Auto-updater (mirrors electron-tray-base updater.js).

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, Wry};
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
                let _ = tauri::async_runtime::block_on(check_and_install(&app, false));
                std::thread::sleep(Duration::from_millis(UPDATE_CHECK_INTERVAL_MS));
            }
        })
        .ok();
}

/// Manual "Check for Updates" from the tray (also used by the menu event).
pub fn request_update_check(app: &AppHandle<Wry>) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        check_and_install(&handle, true).await;
    });
}

async fn check_and_install(app: &AppHandle<Wry>, manual: bool) {
    let app_name = app
        .try_state::<crate::TrayBaseState>()
        .map(|s| s.app_name.clone())
        .unwrap_or_else(|| app.package_info().name.clone());
    let can_install = is_packaged();

    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[updater] unavailable: {e}");
            if manual {
                show_message(
                    app,
                    &format!("{app_name} — Check for Updates"),
                    &format!("Could not check for updates.\n\n{e}"),
                );
            }
            emit_status(app, "error", Some(e.to_string()));
            return;
        }
    };

    if manual {
        emit_status(app, "checking", None);
    }

    match updater.check().await {
        Ok(Some(update)) => {
            let new_version = update.version.clone();
            eprintln!(
                "[updater] found {} -> {}, downloading…",
                app.package_info().version,
                new_version
            );

            if !can_install {
                if manual {
                    show_message(
                        app,
                        &format!("{app_name} — Update Available"),
                        &format!(
                            "Version {new_version} is available (you have {}).\n\nInstall a release build to apply updates.",
                            app.package_info().version
                        ),
                    );
                }
                emit_status(app, "available", Some(new_version));
                return;
            }

            if manual {
                show_message(
                    app,
                    &format!("{app_name} — Update Available"),
                    &format!(
                        "Version {new_version} is available (you have {}).\n\nDownloading and installing…",
                        app.package_info().version
                    ),
                );
            } else {
                emit_status(app, "available", Some(new_version));
            }

            if let Err(e) = update.download_and_install(|_, _| {}, || {}).await {
                eprintln!("[updater] install failed: {e}");
                if manual {
                    show_message(
                        app,
                        &format!("{app_name} — Update Failed"),
                        &format!("Could not install the update.\n\n{e}"),
                    );
                }
                emit_status(app, "error", Some(e.to_string()));
                return;
            }

            // Windows NSIS install typically exits the process; restart covers other platforms.
            app.restart();
        }
        Ok(None) => {
            let version = app.package_info().version.to_string();
            if manual {
                show_message(
                    app,
                    &format!("{app_name} — Up to Date"),
                    &format!("You're running the latest version ({version})."),
                );
            }
            emit_status(app, "uptodate", Some(version));
        }
        Err(e) => {
            eprintln!("[updater] check failed: {e}");
            if manual {
                show_message(
                    app,
                    &format!("{app_name} — Check for Updates"),
                    &format!("Could not check for updates.\n\n{e}"),
                );
            }
            emit_status(app, "error", Some(e.to_string()));
        }
    }
}

fn show_message(app: &AppHandle<Wry>, title: &str, message: &str) {
    let title = title.to_owned();
    let message = message.to_owned();
    let _ = app.run_on_main_thread(move || {
        rfd::MessageDialog::new()
            .set_title(title)
            .set_description(message)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    });
}

fn emit_status(app: &AppHandle<Wry>, status: &str, detail: Option<String>) {
    let payload = serde_json::json!({
        "status": status,
        "detail": detail,
    });
    let _ = app.emit("updater:status", payload);
}

fn is_packaged() -> bool {
    // Skip background checks in `tauri dev` / debug builds.
    !cfg!(debug_assertions)
}
