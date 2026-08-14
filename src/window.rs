use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::webview::{PageLoadEvent, PageLoadPayload};
use tauri::{AppHandle, Manager, Runtime, Webview, WebviewWindow, Wry};

use crate::state::{Quitting, TrayBaseState};
use crate::was_launched_minimised;

pub const MAIN_WINDOW_LABEL: &str = "main";

/// Windows: drop native decorations and keep a shadow so apps can use
/// shared-styles `.window-controls` in the titlebar.
pub fn enable_frameless_chrome<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(windows)]
    {
        if let Some(main) = app.get_webview_window(MAIN_WINDOW_LABEL) {
            let _ = main.set_decorations(false);
            let _ = main.set_shadow(true);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

/// Whether the user last saw the main window (show/hide/toggle). WebView2 on Windows
/// often reports `is_visible() == true` for hidden tray windows, so we track this ourselves.
static MAIN_USER_VISIBLE: AtomicBool = AtomicBool::new(false);

/// Set when `show_main` has successfully activated the window at least once.
/// Distinct from `MAIN_USER_VISIBLE`, which tracks focus and is cleared on blur.
static INITIAL_SHOWN: AtomicBool = AtomicBool::new(false);

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

    if let Some(state) = app.try_state::<TrayBaseState>() {
        let opacity = state.settings.lock().opacity;
        let _ = set_native_opacity(window, opacity);
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
    builder.on_page_load(move |webview: &Webview<Wry>, payload: &PageLoadPayload<'_>| {
        if payload.event() != PageLoadEvent::Finished {
            return;
        }
        let app = webview.app_handle();
        if webview.label() != main_label(app) {
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

/// Reveal the main window at startup. Waiting only for page-load Finished
/// deadlocks on Windows: WebView2 often never completes navigation while the
/// window is hidden (`visible: false` in tauri.conf).
pub fn reveal_main_on_startup<R: Runtime>(app: &AppHandle<R>) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    if should_suppress_initial_show(app) || !should_auto_show_main(app) {
        return;
    }

    show_main(app);

    let app = app.clone();
    std::thread::spawn(move || {
        for delay_ms in [150_u64, 400, 1200] {
            std::thread::sleep(Duration::from_millis(delay_ms));
            if MAIN_USER_VISIBLE.load(Ordering::SeqCst) || INITIAL_SHOWN.load(Ordering::SeqCst) {
                return;
            }
            if should_suppress_initial_show(&app) {
                return;
            }
            show_main(&app);
        }
    });
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

fn show_main_now<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = main_window_with_retry(app) {
        activate_main_window(app, &window);
        MAIN_USER_VISIBLE.store(true, Ordering::SeqCst);
        INITIAL_SHOWN.store(true, Ordering::SeqCst);
    }
}

pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    let app_for_show = app.clone();
    if app
        .run_on_main_thread(move || {
            show_main_now(&app_for_show);
        })
        .is_err()
    {
        show_main_now(app);
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

/// Electron `BrowserWindow.setOpacity` equivalent. Tauri has no cross-platform
/// API, so this talks to the OS window handle. CSS opacity is only a fallback.
pub fn apply_opacity<R: Runtime>(app: &AppHandle<R>, opacity: f64) {
    let opacity = opacity.clamp(0.0, 1.0);
    if let Some(window) = main_window(app) {
        if set_native_opacity(&window, opacity) {
            let _ = window.eval("document.documentElement.style.opacity = '';");
        } else {
            let _ = window.eval(&format!(
                "document.documentElement.style.opacity = '{opacity}';"
            ));
        }
    }
}

fn set_native_opacity<R: Runtime>(window: &WebviewWindow<R>, opacity: f64) -> bool {
    #[cfg(windows)]
    {
        return set_windows_opacity(window, opacity);
    }
    #[cfg(target_os = "macos")]
    {
        return set_macos_opacity(window, opacity);
    }
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    {
        return set_linux_opacity(window, opacity);
    }
    #[allow(unreachable_code)]
    {
        let _ = (window, opacity);
        false
    }
}

#[cfg(windows)]
fn set_windows_opacity<R: Runtime>(window: &WebviewWindow<R>, opacity: f64) -> bool {
    use std::ffi::c_void;

    type Hwnd = *mut c_void;

    extern "system" {
        fn GetWindowLongPtrW(hwnd: Hwnd, n_index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: Hwnd, n_index: i32, dw_new_long: isize) -> isize;
        fn SetLayeredWindowAttributes(hwnd: Hwnd, cr_key: u32, b_alpha: u8, dw_flags: u32) -> i32;
        fn SetWindowPos(
            hwnd: Hwnd,
            hwnd_insert_after: Hwnd,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
    }

    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_LAYERED: isize = 0x0008_0000;
    const LWA_ALPHA: u32 = 0x0000_0002;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_FRAMECHANGED: u32 = 0x0020;

    let Ok(hwnd) = window.hwnd() else {
        return false;
    };
    let hwnd = hwnd.0 as Hwnd;
    let alpha = (opacity * 255.0).round().clamp(0.0, 255.0) as u8;

    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if alpha >= 255 {
            if ex & WS_EX_LAYERED != 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex & !WS_EX_LAYERED);
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            }
        } else {
            if ex & WS_EX_LAYERED == 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED);
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            }
            if SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA) == 0 {
                return false;
            }
        }
    }
    true
}

#[cfg(target_os = "macos")]
fn set_macos_opacity<R: Runtime>(window: &WebviewWindow<R>, opacity: f64) -> bool {
    use objc::{msg_send, runtime::Object, sel, sel_impl};

    let Ok(ns_window) = window.ns_window() else {
        return false;
    };
    unsafe {
        let ns_window = ns_window as *mut Object;
        let _: () = msg_send![ns_window, setAlphaValue: opacity];
    }
    true
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn set_linux_opacity<R: Runtime>(window: &WebviewWindow<R>, opacity: f64) -> bool {
    use gtk::prelude::WidgetExt;

    let Ok(gtk_window) = window.gtk_window() else {
        return false;
    };
    gtk_window.set_opacity(opacity);
    true
}

/// Run the app's before-quit hook (cookie flush, etc.).
///
/// Must not run WebView2 cookie APIs on the UI thread, so this always joins a
/// worker. `consume` removes the hook so it cannot run twice on quit.
pub fn run_before_quit<R: Runtime>(app: &AppHandle<R>, consume: bool) {
    let Some(state) = app.try_state::<TrayBaseState>() else {
        return;
    };
    let hook = {
        let mut slot = state.on_before_quit.lock();
        if consume {
            slot.take()
        } else {
            slot.clone()
        }
    };
    let Some(hook) = hook else {
        return;
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("before-quit".into())
        .spawn(move || {
            hook();
            let _ = tx.send(());
        })
        .ok();
    if spawned.is_some() {
        let _ = rx.recv_timeout(Duration::from_secs(5));
    }
}

pub fn request_quit<R: Runtime>(app: &AppHandle<R>) {
    if let Some(q) = app.try_state::<Quitting>() {
        q.set(true);
    }

    run_before_quit(app, true);

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
        #[cfg(debug_assertions)]
        tauri::WindowEvent::Resized(size) => log_window_resize(window, size, None),
        #[cfg(debug_assertions)]
        tauri::WindowEvent::ScaleFactorChanged {
            scale_factor,
            new_inner_size,
            ..
        } => log_window_resize(window, new_inner_size, Some(*scale_factor)),
        _ => {}
    }
}

#[cfg(debug_assertions)]
fn log_window_resize<R: Runtime>(
    window: &tauri::Window<R>,
    physical: &tauri::PhysicalSize<u32>,
    scale_override: Option<f64>,
) {
    let scale = scale_override.unwrap_or_else(|| window.scale_factor().unwrap_or(1.0));
    let logical = physical.to_logical::<f64>(scale);
    let outer = window
        .outer_size()
        .ok()
        .map(|size| size.to_logical::<f64>(scale));
    let outer_part = outer
        .map(|size| format!("  outer {:.0}x{:.0}", size.width, size.height))
        .unwrap_or_default();
    crate::dev_log!(
        "{} resized  inner {:.0}x{:.0}  physical {}x{}  scale {:.2}x{}",
        window.label(),
        logical.width,
        logical.height,
        physical.width,
        physical.height,
        scale,
        outer_part
    );
}
