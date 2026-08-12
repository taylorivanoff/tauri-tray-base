# tauri-tray-base

Shared Tauri 2 tray-app scaffold — close-to-tray, tray menu, settings, autostart, single-instance.

Mirrors [`electron-tray-base`](https://github.com/taylorivanoff/electron-tray-base).

## Usage

```rust
use tauri_tray_base::{
    apply_window_settings, install_state, on_window_event, set_on_before_quit, setup_tray,
    with_common_plugins, TrayBaseOptions, TrayClickAction, TrayExtraItem, TraySetupOptions,
};

fn main() {
    with_common_plugins(tauri::Builder::default())
        .setup(|app| {
            install_state(
                app.handle(),
                TrayBaseOptions {
                    app_name: "My App".into(),
                    show_hide: false,
                    show_always_on_top: false,
                    tray_on_click: TrayClickAction::Toggle,
                    extra_tray_items: vec![TrayExtraItem {
                        id: "refresh".into(),
                        label: "Refresh".into(),
                    }],
                    ..Default::default()
                },
            )?;
            // create WebviewWindow labeled "main"
            setup_tray(app.handle(), TraySetupOptions::default())?;
            apply_window_settings(app.handle());
            set_on_before_quit(app.handle(), || { /* flush */ });
            Ok(())
        })
        .on_window_event(|window, event| on_window_event(window, &event))
        .run(tauri::generate_context!())
        .expect("error");
}
```

Listen for `tray:action` (extra item ids) and `tray:check-updates` from the tray menu.
