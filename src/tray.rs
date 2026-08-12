use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::state::TrayBaseState;
use crate::window::{hide_main, request_quit, show_main, toggle_main};
use crate::TrayClickAction;

pub struct TraySetupOptions {
    pub tooltip: Option<String>,
}

impl Default for TraySetupOptions {
    fn default() -> Self {
        Self { tooltip: None }
    }
}

pub fn setup_tray(app: &AppHandle<Wry>, opts: TraySetupOptions) -> tauri::Result<()> {
    let state = app.state::<TrayBaseState>();
    let tooltip = opts.tooltip.unwrap_or_else(|| state.app_name.clone());
    let menu = build_menu(app)?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?;
    let on_click = state.tray_on_click;

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip(&tooltip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            handle_menu_event(app, event.id.as_ref());
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                match on_click {
                    TrayClickAction::Toggle => toggle_main(tray.app_handle()),
                    TrayClickAction::Show => show_main(tray.app_handle()),
                }
            }
        })
        .build(app)?;

    crate::updater::setup_updater(app);

    Ok(())
}

pub fn rebuild_tray_menu(app: &AppHandle<Wry>) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.set_menu(Some(menu))?;
    }
    Ok(())
}

fn build_menu(app: &AppHandle<Wry>) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<TrayBaseState>();
    let settings = state.settings.lock().clone();
    let app_name = state.app_name.clone();
    let version = app.package_info().version.to_string();

    let mut items: Vec<Box<dyn tauri::menu::IsMenuItem<Wry>>> = Vec::new();

    if state.show_hide {
        items.push(Box::new(MenuItem::with_id(
            app,
            "show",
            format!("Show {app_name}"),
            true,
            None::<&str>,
        )?));
        items.push(Box::new(MenuItem::with_id(
            app,
            "hide",
            format!("Hide {app_name}"),
            true,
            None::<&str>,
        )?));
    } else {
        items.push(Box::new(MenuItem::with_id(
            app,
            "show",
            format!("Show {app_name}"),
            true,
            None::<&str>,
        )?));
    }

    items.push(Box::new(PredefinedMenuItem::separator(app)?));

    if state.show_always_on_top {
        items.push(Box::new(CheckMenuItem::with_id(
            app,
            "always-on-top",
            "Always on Top",
            true,
            settings.always_on_top,
            None::<&str>,
        )?));
    }

    items.push(Box::new(CheckMenuItem::with_id(
        app,
        "start-minimised",
        "Start Minimised",
        true,
        settings.start_minimised,
        None::<&str>,
    )?));

    let extras = state.extra_tray_items.lock().clone();
    if !extras.is_empty() {
        items.push(Box::new(PredefinedMenuItem::separator(app)?));
        for extra in extras {
            items.push(Box::new(MenuItem::with_id(
                app,
                format!("extra:{}", extra.id),
                &extra.label,
                true,
                None::<&str>,
            )?));
        }
    }

    items.push(Box::new(PredefinedMenuItem::separator(app)?));
    items.push(Box::new(MenuItem::with_id(
        app,
        "check-updates",
        "Check for Updates",
        true,
        None::<&str>,
    )?));
    items.push(Box::new(MenuItem::with_id(
        app,
        "version",
        format!("Version {version}"),
        false,
        None::<&str>,
    )?));
    items.push(Box::new(MenuItem::with_id(
        app,
        "quit",
        "Quit",
        true,
        None::<&str>,
    )?));

    let refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = items.iter().map(|i| i.as_ref()).collect();
    Menu::with_items(app, &refs)
}

fn handle_menu_event(app: &AppHandle<Wry>, id: &str) {
    match id {
        "show" => show_main(app),
        "hide" => hide_main(app),
        "always-on-top" => {
            if let Some(state) = app.try_state::<TrayBaseState>() {
                let next = {
                    let mut s = state.settings.lock();
                    s.always_on_top = !s.always_on_top;
                    let _ = crate::settings::save(&state.settings_path, &s);
                    s.always_on_top
                };
                crate::window::apply_always_on_top(app, next);
                crate::emit_to_renderer(app, "settings:changed", state.settings.lock().to_value());
                let _ = rebuild_tray_menu(app);
            }
        }
        "start-minimised" => {
            if let Some(state) = app.try_state::<TrayBaseState>() {
                let next = {
                    let mut s = state.settings.lock();
                    s.start_minimised = !s.start_minimised;
                    let _ = crate::settings::save(&state.settings_path, &s);
                    s.start_minimised
                };
                crate::sync_autostart(app);
                if !next {
                    show_main(app);
                }
                crate::emit_to_renderer(app, "settings:changed", state.settings.lock().to_value());
                let _ = rebuild_tray_menu(app);
            }
        }
        "check-updates" => {
            crate::updater::request_update_check(app);
            let _ = app.emit("tray:check-updates", ());
        }
        "quit" => request_quit(app),
        other if other.starts_with("extra:") => {
            let action = other.trim_start_matches("extra:");
            let _ = app.emit("tray:action", action);
        }
        _ => {}
    }
}
