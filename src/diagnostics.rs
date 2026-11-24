//! Diagnostics: log file, DebugView output, crash logger and user-facing message boxes.

use crate::*;

pub(crate) fn log_path() -> PathBuf {
    log_dir().join("nemotron-bubble.log")
}

pub(crate) const LOG_MAX_BYTES: u64 = 1024 * 1024;

pub(crate) fn log_info(message: &str) {
    write_log("INFO ", message);
}

pub(crate) fn log_error(message: &str) {
    write_log("ERROR", message);
}

/// Append one line to <exe dir>\log\nemotron-bubble.log (rotated to
/// .log.old at 1 MB) and mirror it to OutputDebugString so DebugView sees it.
pub(crate) fn write_log(level: &str, message: &str) {
    static LOG_LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOG_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let line = format!(
        "[{}] {level} {message}\n",
        Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
    );

    let debug = wide_null(&format!("[NemotronBubble] {level} {message}\n"));
    unsafe {
        OutputDebugStringW(debug.as_ptr());
    }
    #[cfg(debug_assertions)]
    eprint!("{line}");

    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if fs::metadata(&path).map(|m| m.len() > LOG_MAX_BYTES).unwrap_or(false) {
        let _ = fs::rename(&path, path.with_extension("log.old"));
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

pub(crate) fn show_message_blocking(title: &str, text: &str, icon: MESSAGEBOX_STYLE) {
    let title = wide_null(title);
    let text = wide_null(text);
    unsafe {
        MessageBoxW(
            null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | icon | MB_TOPMOST | MB_SETFOREGROUND,
        );
    }
}

/// Show a message box without blocking the caller (UI or engine thread).
pub(crate) fn notify_user(title: &str, text: &str, icon: MESSAGEBOX_STYLE) {
    let title = title.to_string();
    let text = text.to_string();
    thread::spawn(move || show_message_blocking(&title, &text, icon));
}

/// Append any panic (message + location, from any thread) to crash.log. The
/// release build has no console, so without this a panic that aborts the process
/// leaves no trace. Runs before unwinding, so it captures the cause even when the
/// panic later aborts by crossing an FFI callback boundary.
pub(crate) fn install_crash_logger() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let line = format!(
            "[{}] {}\n",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            info
        );
        let path = log_dir().join("crash.log");
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = file.write_all(line.as_bytes());
        }
        write_log("PANIC", &info.to_string());
        default(info);
    }));
}

