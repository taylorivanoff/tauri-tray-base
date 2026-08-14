//! Debug-only stderr logging for `tauri dev` / `bun start`.
//! Compiles out of release builds.

/// Print a `[dev]` line to stderr. No-op in release.
///
/// ```ignore
/// tauri_tray_base::dev_log!("main inner {}x{}", width, height);
/// ```
#[macro_export]
macro_rules! dev_log {
    ($($arg:tt)*) => {{
        #[cfg(debug_assertions)]
        {
            eprintln!("[dev] {}", format_args!($($arg)*));
        }
    }};
}
