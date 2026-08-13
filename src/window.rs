use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::webview::{PageLoadEvent, PageLoadPayload};
use tauri::{AppHandle, Manager, Runtime, Webview, WebviewWindow, Wry};

use crate::state::{Quitting, TrayBaseState};
use crate::was_launched_minimised;

pub const MAIN_WINDOW_LABEL: &str = "main";

/// Whether the user last saw the main window (show/hide/toggle). WebView2 on Windows
/// often reports `is_visible() == true` for hidden tray windows, so we track this ourselves.
static MAIN_USER_VISIBLE: AtomicBool = AtomicBool::new(false);

fn main_label<R: Runtime>(app: &AppHandle<R>) -> String {
    app.try_state::<TrayBaseState>()
        .map(|s| s.main_window_label.clone())
        .unwrap_or_else(|| MAIN_WINDOW_LABEL.to_string())
}

fn main_window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    let label = main_label(app);
    app.get_webview_window(&label)
        .or_else(|| {
            app.webview_windows()
                .into_values()
                .find(|w| w.label() == label)
        })
        .or_else(|| app.webview_windows().into_values().next())
}

fn main_window_with_retry<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    #[cfg(windows)]
    const RETRIES: usize = 6;
    #[cfg(not(windows))]
    const RETRIES: usize = 1;

    for attempt in 0..RETRIES {
        if let Some(window) = main_window(app) {
            return Some(window);
        }
        if attempt + 1 < RETRIES {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    None
}

#[cfg(windows)]
fn win32_activate<R: Runtime>(window: &WebviewWindow<R>) {
    use std::ffi::c_void;

    type Hwnd = *mut c_void;

    extern "system" {
        fn ShowWindow(hwnd: Hwnd, n_cmd_show: i32) -> i32;
        fn SetForegroundWindow(hwnd: Hwnd) -> i32;
        fn IsIconic(hwnd: Hwnd) -> i32;
    }

    const SW_RESTORE: i32 = 9;
    const SW_SHOW: i32 = 5;

    if let Ok(hwnd) = window.hwnd() {
        let hwnd = hwnd.0 as Hwnd;
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

fn activate_main_window<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) {
    let always_on_top = app
        .try_state::<TrayBaseState>()
        .map(|s| s.settings.lock().always_on_top)
        .unwrap_or(false);

    let _ = window.unminimize();
    let _ = window.show();
    #[cfg(windows)]
    win32_activate(window);

    // With show_menu_on_left_click(false), Windows may restore behind other apps unless
    // we briefly raise z-order before focusing (see tauri#14795).
    if !always_on_top {
        let _ = window.set_always_on_top(true);
    }
    let _ = window.set_focus();
    if !always_on_top {
        let _ = window.set_always_on_top(false);
        let _ = window.set_always_on_top(always_on_top);
    }
}

fn should_suppress_initial_show<R: Runtime>(app: &AppHandle<R>) -> bool {
    if was_launched_minimised() {
        return true;
    }
    app.try_state::<TrayBaseState>()
        .map(|s| s.settings.lock().start_minimised)
        .unwrap_or(false)
}

fn should_auto_show_main<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<TrayBaseState>()
        .map(|s| s.auto_show_main_on_ready)
        .unwrap_or(true)
}

/// Show the main window on first page-load Finished (unless start-minimised).
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
        if !should_auto_show_main(app) {
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
    if let Some(window) = main_window_with_retry(app) {
        activate_main_window(app, &window);
        MAIN_USER_VISIBLE.store(true, Ordering::SeqCst);
    }
}

pub fn hide_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = main_window(app) {
        let _ = window.hide();
        MAIN_USER_VISIBLE.store(false, Ordering::SeqCst);
    }
}

pub fn toggle_main<R: Runtime>(app: &AppHandle<R>) {
    if MAIN_USER_VISIBLE.load(Ordering::SeqCst) {
        hide_main(app);
    } else {
        show_main(app);
    }
}

pub fn apply_always_on_top<R: Runtime>(app: &AppHandle<R>, value: bool) {
    if let Some(window) = main_window(app) {
        let _ = window.set_always_on_top(value);
    }
}

pub fn apply_opacity<R: Runtime>(app: &AppHandle<R>, opacity: f64) {
    let opacity = opacity.clamp(0.0, 1.0);
    if let Some(window) = main_window(app) {
        let _ = window.eval(&format!(
            "document.documentElement.style.opacity = '{opacity}';"
        ));
    }
}

pub fn request_quit<R: Runtime>(app: &AppHandle<R>) {
    if let Some(q) = app.try_state::<Quitting>() {
        q.set(true);
    }

    if let Some(state) = app.try_state::<TrayBaseState>() {
        let hook = state.on_before_quit.lock().take();
        if let Some(hook) = hook {
            let _ = std::thread::Builder::new()
                .name("before-quit".into())
                .spawn(move || hook());
        }
    }

    if let Some(window) = main_window(app) {
        let _ = window.destroy();
    }
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_visible(false);
    }

    app.exit(0);
}

pub fn on_window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
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
                hide_main(window.app_handle());
            }
        }
        tauri::WindowEvent::Focused(focused) => {
            let label = main_label(window.app_handle());
            if window.label() == label {
                MAIN_USER_VISIBLE.store(*focused, Ordering::SeqCst);
            }
        }
        _ => {}
    }
}
