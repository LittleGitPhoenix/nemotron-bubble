#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(unsafe_op_in_unsafe_fn)]

// Every module starts with `use crate::*;`, so items from all modules (and the
// external imports below) are available everywhere, as in the former single file.
#[cfg(feature = "assets")]
mod assets;
mod audio;
mod diagnostics;
mod download;
mod engine;
mod history;
mod hotkey;
mod model;
mod output;
mod settings;
mod sound;
mod ui;
mod util;

#[cfg(feature = "assets")]
use assets::*;
use audio::*;
use diagnostics::*;
use download::*;
use engine::*;
use history::*;
use hotkey::*;
use model::*;
use output::*;
use settings::*;
use sound::*;
use ui::*;
use util::*;

use anyhow::{Context, Result, anyhow, bail};
use chrono::Local;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};
use once_cell::sync::OnceCell;
use parakeet_rs::ort::environment::Environment;
use parakeet_rs::{ExecutionConfig, ExecutionProvider, Nemotron, NemotronMode};
use core::ffi::c_void;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    GlobalFree, HGLOBAL, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, ClientToScreen, CreateBitmap,
    CreateFontW, CreateRoundRectRgn, DEFAULT_CHARSET,
    DEFAULT_PITCH, DT_CALCRECT, DT_CENTER, DT_EDITCONTROL, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_TOP, DT_VCENTER,
    DT_WORDBREAK, DeleteObject, DrawTextW, EndPaint, FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD,
    HDC, IntersectClipRect, InvalidateRect, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SelectClipRgn,
    SelectObject,
    SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BitBlt,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, GdiFlush, GetDC, HBITMAP,
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint, ReleaseDC, SRCCOPY,
};
use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringW;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
use windows_sys::Win32::UI::Controls::{
    ICC_WIN95_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, TOOLTIPS_CLASSW, TTF_IDISHWND,
    TTF_SUBCLASS, TTM_ACTIVATE, TTM_ADDTOOLW, TTM_SETMAXTIPWIDTH, TTM_UPDATETIPTEXTW,
    TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW, WM_MOUSELEAVE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyNameTextW, GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN, MapVirtualKeyW, RegisterHotKey, ReleaseCapture, SendInput, SetCapture,
    SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, UnregisterHotKey, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN,
    VK_SHIFT, VK_SPACE,
};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW,
    CreateIconIndirect, DefWindowProcW, DestroyIcon, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetForegroundWindow, GetMessageW,
    GetSystemMetrics, GetWindowRect, HICON, HWND_TOPMOST, ICONINFO, IDC_ARROW, IDI_APPLICATION,
    IsWindow, IsWindowVisible, KillTimer, LoadCursorW, LoadIconW, LWA_ALPHA, MB_ICONERROR,
    MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MESSAGEBOX_STYLE, MF_CHECKED, MF_GRAYED,
    HMENU, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, MessageBoxW,
    PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_SHOW,
    SW_SHOWNA, SWP_NOACTIVATE, SWP_NOSIZE, SetForegroundWindow,
    SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, TPM_RIGHTBUTTON,
    TrackPopupMenu, TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WM_APP, WM_COMMAND, WM_CREATE,
    WM_DESTROY, WM_ERASEBKGND, WM_HOTKEY, WM_LBUTTONDOWN,
    WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONUP, WM_SYSKEYDOWN,
    WM_TIMER, WM_USER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

/// App version from Cargo.toml; also embedded in the exe's version resource (build.rs).
pub(crate) const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) static APP: OnceCell<Arc<AppState>> = OnceCell::new();
#[derive(Debug)]
pub(crate) struct UiState {
    pub(crate) recording: bool,
    pub(crate) visible: bool,
    pub(crate) status: String,
    pub(crate) transcript: String,
    pub(crate) model_dir: String,
    /// Short model state for the application window, e.g. "Loaded: Multilingual".
    pub(crate) model_status: String,
    /// Why no microphone could be opened, None while one is available. The bubble
    /// and the tray icon show it and give the reason in their tooltips.
    pub(crate) mic_error: Option<String>,
    pub(crate) waveform: Vec<f32>,
    pub(crate) current_level: f32,
    pub(crate) last_voice: Instant,
}

pub(crate) struct AppState {
    pub(crate) engine_tx: Sender<EngineCommand>,
    pub(crate) recording: Arc<AtomicBool>,
    pub(crate) hwnd: AtomicIsize,
    pub(crate) target_hwnd: AtomicIsize,
    pub(crate) application_window_hwnd: AtomicIsize,
    /// History list: scroll offset and total content height (px), the clickable
    /// copy icons of the last paint (rect, history index), and the entry whose
    /// icon currently shows the "copied" checkmark.
    pub(crate) history_scroll: AtomicI32,
    pub(crate) history_content_h: AtomicI32,
    pub(crate) history_hits: Mutex<Vec<(RectI, usize)>>,
    pub(crate) copied_flash: Mutex<Option<usize>>,
    pub(crate) capturing_hotkey: AtomicBool,
    /// Active tab of the application window (TAB_HISTORY / TAB_MODEL / TAB_SETTINGS).
    pub(crate) settings_tab: AtomicUsize,
    /// Scroll offset (px) of the Settings tab.
    pub(crate) settings_scroll: AtomicI32,
    /// Model tab downloads: one state per DOWNLOADABLE_MODELS entry, and the
    /// cancel request for the running download (only one runs at a time).
    pub(crate) downloads: Mutex<Vec<ModelDownloadState>>,
    pub(crate) download_cancel: AtomicBool,
    pub(crate) enter_pending: AtomicBool,
    pub(crate) drag: Mutex<Option<DragState>>,
    pub(crate) settings: Mutex<AppSettings>,
    pub(crate) history: Mutex<Vec<HistoryItem>>,
    pub(crate) ui: Mutex<UiState>,
}

fn main() -> Result<()> {
    install_crash_logger();

    if env::args().any(|arg| arg == "--self-test") {
        // Release builds are GUI-subsystem and have no console; borrow the one we
        // were started from so the self-test output is visible.
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
        let result = self_test();
        match &result {
            Ok(()) => log_info("Self-test passed."),
            Err(err) => log_error(&format!("Self-test failed: {err:#}")),
        }
        return result;
    }

    match run_app() {
        Ok(()) => {
            log_info("Exiting normally.");
            Ok(())
        }
        Err(err) => {
            let message = format!("{err:#}");
            log_error(&format!("Fatal: {message}"));
            show_message_blocking(
                "Nemotron Bubble could not start",
                &format!("{message}\n\nLog file: {}", log_path().display()),
                MB_ICONERROR,
            );
            Err(err)
        }
    }
}

pub(crate) fn run_app() -> Result<()> {
    log_info(&format!(
        "Starting Nemotron Bubble {} (exe: {}, cwd: {})",
        env!("CARGO_PKG_VERSION"),
        env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "?".into()),
        env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "?".into()),
    ));

    #[cfg(feature = "assets")]
    {
        let args: Vec<String> = env::args().collect();
        if let Some(pos) = args.iter().position(|a| a == "--render-assets") {
            let out = args
                .get(pos + 1)
                .cloned()
                .unwrap_or_else(|| "docs".to_string());
            return render_assets(&out);
        }
        if let Some(pos) = args.iter().position(|a| a == "--render-icon") {
            let out = args
                .get(pos + 1)
                .cloned()
                .unwrap_or_else(|| "assets/nemotron-bubble.ico".to_string());
            return render_icon(std::path::Path::new(&out));
        }
    }

    let settings = load_settings();
    save_settings(&settings);
    apply_startup_setting(settings.start_with_windows);
    ensure_sound_assets();
    let history = load_history(settings.history_limit);

    let (engine_tx, engine_rx) = bounded::<EngineCommand>(256);
    let app = Arc::new(AppState {
        engine_tx,
        recording: Arc::new(AtomicBool::new(false)),
        hwnd: AtomicIsize::new(0),
        target_hwnd: AtomicIsize::new(0),
        application_window_hwnd: AtomicIsize::new(0),
        history_scroll: AtomicI32::new(0),
        history_content_h: AtomicI32::new(0),
        history_hits: Mutex::new(Vec::new()),
        copied_flash: Mutex::new(None),
        capturing_hotkey: AtomicBool::new(false),
        settings_tab: AtomicUsize::new(TAB_HISTORY),
        settings_scroll: AtomicI32::new(0),
        downloads: Mutex::new(vec![ModelDownloadState::default(); DOWNLOADABLE_MODELS.len()]),
        download_cancel: AtomicBool::new(false),
        enter_pending: AtomicBool::new(false),
        drag: Mutex::new(None),
        settings: Mutex::new(settings),
        history: Mutex::new(history),
        ui: Mutex::new(UiState {
            recording: false,
            visible: true,
            status: "Ready. Press Ctrl+Space to start.".to_string(),
            transcript: "Tap Ctrl+Space to dictate.".to_string(),
            model_dir: String::new(),
            model_status: "Not loaded yet.".to_string(),
            mic_error: None,
            waveform: vec![0.0; WAVE_BARS],
            current_level: 0.0,
            last_voice: Instant::now(),
        }),
    });

    APP.set(app.clone()).ok();

    thread::spawn({
        let app = app.clone();
        move || run_engine(engine_rx, app)
    });

    // A missing/disabled microphone is not fatal and shows no message box here:
    // the bubble and the tray icon show it (with the reason on hover), and the
    // device is opened as soon as it is back (see retry_audio_capture).
    if !ensure_audio_capture(&app, false) {
        update_ui(&app, |ui| {
            ui.status = "No microphone available. Connect one, then start dictation.".to_string();
        });
    }
    refresh_local_model_state(&app);
    if settings_snapshot(&app).preload_model {
        let _ = app.engine_tx.send(EngineCommand::Preload);
    }

    unsafe {
        run_window(app).context("failed to run window")?;
    }

    close_audio_capture();
    Ok(())
}

pub(crate) fn self_test() -> Result<()> {
    println!("Loading Nemotron (this can take a while)...");
    let settings = load_settings();
    println!("Model choice: {}", settings.model.label());
    println!("Language (multilingual only): {}", language_label(&settings.language));
    println!("Processing setting: {}", settings.compute.label());
    let loaded = load_nemotron(&settings)?;
    let (mut model, model_dir) = (loaded.model, loaded.dir);
    model.reset();
    let mode = match model.mode() {
        NemotronMode::EnglishOnly => "English-only",
        NemotronMode::Multilingual => "Multilingual",
    };
    // A few chunks of silence: the first one includes warm-up, so time the rest.
    let silence = vec![0.0; model.chunk_samples()];
    let chunk_ms = chunk_duration_ms(&model);
    let _ = model.transcribe_chunk(&silence)?;
    const TIMED_CHUNKS: u32 = 5;
    let started = Instant::now();
    for _ in 0..TIMED_CHUNKS {
        let _ = model.transcribe_chunk(&silence)?;
    }
    let per_chunk_ms = started.elapsed().as_secs_f32() * 1000.0 / TIMED_CHUNKS as f32;
    println!("Nemotron loaded: {mode}");
    println!("Running on: {}", if loaded.on_gpu { "GPU (DirectML)" } else { "CPU" });
    println!("Time per {chunk_ms:.0} ms audio chunk: {per_chunk_ms:.0} ms");
    println!("Model dir: {}", model_dir.display());
    log_info(&format!(
        "Self-test: {mode} on the {}, {per_chunk_ms:.0} ms per {chunk_ms:.0} ms chunk",
        if loaded.on_gpu { "GPU" } else { "CPU" }
    ));
    Ok(())
}

pub(crate) fn update_ui<F>(app: &Arc<AppState>, update: F)
where
    F: FnOnce(&mut UiState),
{
    {
        let mut ui = lock_ui(app);
        update(&mut ui);
    }
    unsafe {
        let hwnd = hwnd_from_app(app);
        if hwnd != null_mut() {
            PostMessageW(hwnd, WM_UI_UPDATE, 0, 0);
        }
        let application_window_hwnd = application_window_hwnd_from_app(app);
        if application_window_hwnd != null_mut() {
            PostMessageW(application_window_hwnd, WM_UI_UPDATE, 0, 0);
        }
    }
}

pub(crate) fn lock_ui(app: &Arc<AppState>) -> std::sync::MutexGuard<'_, UiState> {
    app.ui.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ===========================================================================
// Software canvas
//
// A top-down 32-bit DIB section we can both poke pixels into directly (for
// anti-aliased vector shapes computed with signed-distance fields) and hand to
// GDI (for ClearType text). Pixels are stored premultiplied as 0xAARRGGBB so the
// buffer can be fed straight to UpdateLayeredWindow for a smooth, shaped, shadowed
// floating bubble — no jagged window region, no GDI blockiness.
// ===========================================================================

pub(crate) fn hwnd_from_app(app: &Arc<AppState>) -> HWND {
    app.hwnd.load(Ordering::SeqCst) as HWND
}

pub(crate) fn target_hwnd_from_app(app: &Arc<AppState>) -> HWND {
    app.target_hwnd.load(Ordering::SeqCst) as HWND
}

pub(crate) fn application_window_hwnd_from_app(app: &Arc<AppState>) -> HWND {
    app.application_window_hwnd.load(Ordering::SeqCst) as HWND
}
