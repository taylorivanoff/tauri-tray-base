use std::sync::atomic::{AtomicBool, Ordering};

use tauri::webview::{PageLoadEvent, PageLoadPayload};
use tauri::{AppHandle, Manager, Runtime, Webview, WebviewWindow, Wry};

use crate::state::{Quitting, TrayBaseState};
use crate::was_launched_minimised;

pub const MAIN_WINDOW_LABEL: &str = "main";

fn main_label<R: Runtime>(app: &AppHandle<R>) -> String {
    app.try_state::<TrayBaseState>()
        .map(|s| s.main_window_label.clone())
        .unwrap_or_else(|| MAIN_WINDOW_LABEL.to_string())
}

fn should_suppress_initial_show<R: Runtime>(app: &AppHandle<R>) -> bool {
    if was_launched_minimised() {
        return true;
    }
    app.try_state::<TrayBaseState>()
        .map(|s| s.settings.lock().start_minimised)
        .unwrap_or(false)
}

/// Show the main window on first page-load Finished (unless start-minimised).
///
/// Wire this on the app [`tauri::Builder`] so config-created windows are covered.
/// Call only once; subsequent SPA navigations must not re-show a tray-hidden window.
pub fn attach_show_main_when_ready(builder: tauri::Builder<Wry>) -> tauri::Builder<Wry> {
    static REVEALED: AtomicBool = AtomicBool::new(false);

    builder.on_page_load(move |webview: &Webview<Wry>, payload: &PageLoadPayload<'_>| {
        if payload.event() != PageLoadEvent::Finished {
            return;
        }
        let app = webview.app_handle();
        if webview.label() != main_label(app) {
            return;
        }
        if REVEALED.swap(true, Ordering::SeqCst) {
            return;
        }
        if should_suppress_initial_show(app) {
            return;
        }
        show_main(app);
    })
}

/// Show a builder-created window after its first page-load Finished.
pub fn reveal_webview_when_ready(
) -> impl Fn(WebviewWindow, PageLoadPayload<'_>) + Send + Sync + 'static {
    move |win, payload| {
        if payload.event() != PageLoadEvent::Finished {
            return;
        }
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    let label = main_label(app);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn hide_main<R: Runtime>(app: &AppHandle<R>) {
    let label = main_label(app);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.hide();
    }
}

pub fn toggle_main<R: Runtime>(app: &AppHandle<R>) {
    let label = main_label(app);
    if let Some(window) = app.get_webview_window(&label) {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            show_main(app);
        }
    }
}

pub fn apply_always_on_top<R: Runtime>(app: &AppHandle<R>, value: bool) {
    let label = main_label(app);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.set_always_on_top(value);
    }
}

pub fn apply_opacity<R: Runtime>(app: &AppHandle<R>, opacity: f64) {
    let label = main_label(app);
    let opacity = opacity.clamp(0.0, 1.0);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.eval(&format!(
            "document.documentElement.style.opacity = '{opacity}';"
        ));
    }
}

pub fn request_quit<R: Runtime>(app: &AppHandle<R>) {
    if let Some(q) = app.try_state::<Quitting>() {
        q.set(true);
    }

    // Run before-quit off the UI thread. WebView2 cookie/IO hooks deadlock if the
    // tray menu handler joins them on the main thread.
    if let Some(state) = app.try_state::<TrayBaseState>() {
        let hook = state.on_before_quit.lock().take();
        if let Some(hook) = hook {
            let _ = std::thread::Builder::new()
                .name("before-quit".into())
                .spawn(move || hook());
        }
    }

    // Tear down the main window so close-to-tray cannot keep the process alive.
    let label = main_label(app);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.destroy();
    }
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_visible(false);
    }

    app.exit(0);
}

pub fn on_window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        let quitting = window
            .app_handle()
            .try_state::<Quitting>()
            .map(|q| q.get())
            .unwrap_or(false);
        if quitting {
            return;
        }
        let label = main_label(window.app_handle());
        if window.label() == label {
            api.prevent_close();
            let _ = window.hide();
        }
    }
}
