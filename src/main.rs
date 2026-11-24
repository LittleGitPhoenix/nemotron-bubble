#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(unsafe_op_in_unsafe_fn)]

use anyhow::{Context, Result, anyhow};
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
use std::path::PathBuf;
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
    DEFAULT_PITCH, DT_CALCRECT, DT_CENTER, DT_EDITCONTROL, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_TOP, DT_VCENTER,
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
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyNameTextW, GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN, MapVirtualKeyW, RegisterHotKey, ReleaseCapture, SendInput, SetCapture,
    SetFocus, UnregisterHotKey, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN,
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
    PostMessageW, PostQuitMessage, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_SHOW,
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

const CLASS_NAME: &str = "NemotronBubbleWindow";
const TRAY_ID: u32 = 1;
const HOTKEY_ID: i32 = 42;
const ENTER_HOTKEY_ID: i32 = 43;
// Auto-stop recording after this many seconds without detected speech.
const SILENCE_AUTO_STOP_SECS: u64 = 15;
// Raw mic RMS above which we treat the user as speaking (for the silence auto-stop).
// Compared against true loudness, NOT the visually-boosted bar height.
const VOICE_RMS_THRESHOLD: f32 = 0.02;
const WM_TRAYICON: u32 = WM_USER + 1;
const WM_UI_UPDATE: u32 = WM_APP + 1;

// Floating bubble: a soft-shadowed pill rendered with per-pixel alpha. The window
// is larger than the pill so the drop shadow has room to fall off smoothly.
const BUBBLE_MARGIN: i32 = 10;
const PILL_W: i32 = 120;
const PILL_H: i32 = 54;
const WIDTH: i32 = PILL_W + BUBBLE_MARGIN * 2;
const HEIGHT: i32 = PILL_H + BUBBLE_MARGIN * 2;
const WAVE_BARS: usize = 18;
const ANIM_TIMER_ID: usize = 77;
const ANIM_INTERVAL_MS: u32 = 33; // ~30 fps

// Settings window: header, a tab bar (History | Model | Settings) and one card
// that shows the active tab. The width fits one column of settings rows.
const SETTINGS_WIDTH: i32 = 392;
const SETTINGS_HEIGHT: i32 = 816;
// Space below the card; holds the version label at the bottom right.
const SETTINGS_FOOTER_H: i32 = 36;
const SETTINGS_CARD_LEFT: i32 = 20;
const SETTINGS_CARD_TOP: i32 = 118;
// Content area inside the card.
const SETTINGS_INNER_LEFT: i32 = 40;
const SETTINGS_INNER_RIGHT: i32 = SETTINGS_WIDTH - 40;
const SETTINGS_TAB_TOP: i32 = 78;
const SETTINGS_TAB_BOTTOM: i32 = 108;
const SETTINGS_TABS: [&str; 3] = ["History", "Model", "Settings"];
const TAB_HISTORY: usize = 0;
const TAB_MODEL: usize = 1;
const TAB_SETTINGS: usize = 2;
// History tab: a title row with the trash button, then the custom-drawn,
// scrollable list of entries (copy icon + grey header line + text each).
const HISTORY_TITLE_TOP: i32 = SETTINGS_CARD_TOP + 14;
const HISTORY_LIST_TOP: i32 = SETTINGS_CARD_TOP + 52;
const HISTORY_LIST_BOTTOM: i32 = SETTINGS_HEIGHT - SETTINGS_FOOTER_H - 12;
// Text column (the scrollbar sits in the 12 px to the right of it).
const HISTORY_TEXT_LEFT: i32 = SETTINGS_INNER_LEFT;
const HISTORY_TEXT_RIGHT: i32 = SETTINGS_INNER_RIGHT - 8;
const HISTORY_HEADER_H: i32 = 16;
const HISTORY_ENTRY_GAP: i32 = 14;
// Pixels scrolled per mouse-wheel notch.
const HISTORY_WHEEL_STEP: i32 = 60;
// Timer that turns a copy icon's checkmark back into the copy icon.
const COPY_FLASH_TIMER_ID: usize = 78;
const COPY_FLASH_MS: u32 = 1200;
// Windows icon-font glyphs (Segoe MDL2 Assets / Segoe Fluent Icons).
const GLYPH_CLOSE: &str = "\u{E8BB}";
const GLYPH_COPY: &str = "\u{E8C8}";
const GLYPH_CHECK: &str = "\u{E73E}";
const GLYPH_TRASH: &str = "\u{E74D}";
// Model tab: Processing, Speech model, Language, Preload (top to bottom)
const COMPUTE_SECTION_TOP: i32 = 132;
const MODEL_SECTION_TOP: i32 = 230;
const LANGUAGE_ROW_TOP: i32 = 328;
const PRELOAD_ROW_TOP: i32 = 384;
// Settings tab
const SETTINGS_TAB_TOGGLES_TOP: i32 = 180;
// The bubble position row follows the last toggle.
const BUBBLE_POSITION_ROW_TOP: i32 = SETTINGS_TAB_TOGGLES_TOP + SETTINGS_TAB_TOGGLES.len() as i32 * 49;
const NEMOTRON_SAMPLE_RATE: f64 = 16_000.0;
const NEMOTRON_CHUNK_SIZE: usize = 8_960; // 560 ms at 16 kHz.

const IDM_START_STOP: usize = 1001;
const IDM_SHOW_HIDE: usize = 1002;
const IDM_CLEAR: usize = 1003;
const IDM_QUIT: usize = 1004;
// Tray "Speech model" submenu: one id per ModelChoice, in ModelChoice::ALL order.
const IDM_MODEL_BASE: usize = 1100;
// "Language" menus (tray submenu and the settings popup): one id per LANGUAGES entry.
const IDM_LANG_BASE: usize = 1200;
// Tray "Processing" submenu: one id per ComputeChoice, in ComputeChoice::ALL order.
const IDM_COMPUTE_BASE: usize = 1300;
// Bubble position popup menu: one id per BubblePosition, in BubblePosition::ALL order.
const IDM_BUBBLE_POS_BASE: usize = 1400;
/// App version from Cargo.toml; also embedded in the exe's version resource (build.rs).
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const SETTINGS_TOGGLE_STARTUP: usize = 2001;
const SETTINGS_TOGGLE_PRELOAD: usize = 2002;
const SETTINGS_TOGGLE_LIVE_TYPE: usize = 2003;
const SETTINGS_TOGGLE_FINAL_CLIPBOARD: usize = 2004;
const SETTINGS_TOGGLE_FINAL_PASTE: usize = 2005;
const SETTINGS_TOGGLE_SOUNDS: usize = 2006;
const SETTINGS_TOGGLE_WAVEFORM: usize = 2007;
const SETTINGS_TOGGLE_BUBBLE_CLICK_SETTINGS: usize = 2008;
const SETTINGS_TOGGLE_FLOATING_BUBBLE: usize = 2009;
const SETTINGS_TOGGLE_TRAY_WAVEFORM: usize = 2010;
const SETTINGS_TOGGLE_ENTER_STOPS: usize = 2011;
const SETTINGS_TOGGLE_SILENCE_STOP: usize = 2012;

static APP: OnceCell<Arc<AppState>> = OnceCell::new();
static UI_FONT: OnceCell<isize> = OnceCell::new();
static TITLE_FONT: OnceCell<isize> = OnceCell::new();
static SMALL_FONT: OnceCell<isize> = OnceCell::new();
static ICON_FONT: OnceCell<isize> = OnceCell::new();
static ICON_FONT_LARGE: OnceCell<isize> = OnceCell::new();
static TINY_FONT: OnceCell<isize> = OnceCell::new();
// The currently-installed tray HICON. Animated icons are swapped in via NIM_MODIFY
// and the previous one destroyed, so this tracks what needs cleanup.
static CURRENT_TRAY_ICON: AtomicIsize = AtomicIsize::new(0);
// Set by the cpal error callback (e.g. the mic was unplugged or disabled) so the
// next dictation start reopens the device instead of recording silence.
static MIC_FAILED: AtomicBool = AtomicBool::new(false);

thread_local! {
    // The live microphone stream. cpal streams are not Send, so it lives on the
    // UI thread (which is also the main thread) and is (re)opened lazily there.
    static AUDIO_STREAM: RefCell<Option<cpal::Stream>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug)]
enum EngineCommand {
    Preload,
    /// Drop the loaded model (frees its RAM), e.g. after switching models.
    Unload,
    /// Set the spoken language of the multilingual model (a LANGUAGES code).
    SetLanguage(String),
    Start,
    Audio(Vec<f32>),
    Stop,
    Shutdown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct AppSettings {
    start_with_windows: bool,
    preload_model: bool,
    live_type_into_cursor: bool,
    copy_final_to_clipboard: bool,
    paste_final_on_stop: bool,
    sounds_enabled: bool,
    waveform_enabled: bool,
    bubble_click_opens_settings: bool,
    show_floating_bubble: bool,
    tray_waveform_enabled: bool,
    enter_stops_recording: bool,
    silence_auto_stop: bool,
    hotkey_mods: u32,
    hotkey_vk: u32,
    paste_delay_ms: u64,
    history_limit: usize,
    /// Which speech model to load (chosen in the settings window). When missing
    /// from settings.json (first start), it is detected; see `detect_model_choice`.
    model: ModelChoice,
    /// Model folder used by the Predefined choice (after NEMOTRON_MODEL_DIR).
    model_dir: String,
    /// Spoken language for the multilingual model: a code from LANGUAGES, or
    /// "auto" to let the model detect it. Ignored by the English-only model.
    language: String,
    /// Where the model runs: GPU (DirectML), CPU, or Auto (GPU, else CPU).
    compute: ComputeChoice,
    /// Screen position of the floating bubble (one of six presets).
    bubble_position: BubblePosition,
}

/// Preset positions of the floating bubble, in the primary monitor's work area
/// (the screen minus the taskbar). Stored as e.g. "bottom-center" in settings.json.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
enum BubblePosition {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    #[default]
    BottomRight,
}

impl BubblePosition {
    /// Menu order.
    const ALL: [BubblePosition; 6] = [
        BubblePosition::TopLeft,
        BubblePosition::TopCenter,
        BubblePosition::TopRight,
        BubblePosition::BottomLeft,
        BubblePosition::BottomCenter,
        BubblePosition::BottomRight,
    ];

    fn label(self) -> &'static str {
        match self {
            BubblePosition::TopLeft => "Top left",
            BubblePosition::TopCenter => "Top center",
            BubblePosition::TopRight => "Top right",
            BubblePosition::BottomLeft => "Bottom left",
            BubblePosition::BottomCenter => "Bottom center",
            BubblePosition::BottomRight => "Bottom right",
        }
    }

    /// The settings.json spelling.
    fn key(self) -> &'static str {
        match self {
            BubblePosition::TopLeft => "top-left",
            BubblePosition::TopCenter => "top-center",
            BubblePosition::TopRight => "top-right",
            BubblePosition::BottomLeft => "bottom-left",
            BubblePosition::BottomCenter => "bottom-center",
            BubblePosition::BottomRight => "bottom-right",
        }
    }
}

impl From<String> for BubblePosition {
    fn from(value: String) -> Self {
        BubblePosition::ALL
            .into_iter()
            .find(|position| position.key().eq_ignore_ascii_case(value.trim()))
            .unwrap_or_default()
    }
}

impl From<BubblePosition> for String {
    fn from(value: BubblePosition) -> Self {
        value.key().to_string()
    }
}

/// The "Processing" setting. Stored as a plain string in settings.json; unknown
/// values fall back to the default (CPU).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
enum ComputeChoice {
    /// The default: an integrated GPU is often slower than the CPU for these
    /// small streaming chunks (measured: ~290 ms vs ~130 ms per chunk).
    #[default]
    Cpu,
    /// GPU only; loading fails with an error when no usable GPU is found.
    Gpu,
    /// GPU via DirectML when available, otherwise the CPU.
    Auto,
}

impl ComputeChoice {
    /// Display order in the settings switch and the tray submenu.
    const ALL: [ComputeChoice; 3] = [ComputeChoice::Cpu, ComputeChoice::Gpu, ComputeChoice::Auto];

    fn label(self) -> &'static str {
        match self {
            ComputeChoice::Auto => "Auto",
            ComputeChoice::Gpu => "GPU",
            ComputeChoice::Cpu => "CPU",
        }
    }
}

impl From<String> for ComputeChoice {
    fn from(value: String) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "gpu" => ComputeChoice::Gpu,
            "auto" => ComputeChoice::Auto,
            _ => ComputeChoice::Cpu,
        }
    }
}

impl From<ComputeChoice> for String {
    fn from(value: ComputeChoice) -> Self {
        value.label().to_ascii_lowercase()
    }
}

/// Languages offered for the multilingual model: "auto" plus NVIDIA's
/// transcription-ready languages. The codes are parakeet-rs prompt keys
/// (Japanese and Vietnamese only exist with their region suffix).
const LANGUAGES: &[(&str, &str)] = &[
    ("auto", "Auto-detect"),
    ("ar", "Arabic"),
    ("nl", "Dutch"),
    ("en", "English"),
    ("fr", "French"),
    ("de", "German"),
    ("hi", "Hindi"),
    ("it", "Italian"),
    ("ja-JP", "Japanese"),
    ("ko", "Korean"),
    ("pt", "Portuguese"),
    ("ru", "Russian"),
    ("es", "Spanish"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
    ("vi-VN", "Vietnamese"),
];

/// The canonical code for a saved language value; unknown values become "auto".
fn normalize_language(code: &str) -> &'static str {
    LANGUAGES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(code.trim()))
        .map(|(known, _)| *known)
        .unwrap_or("auto")
}

fn language_label(code: &str) -> &'static str {
    let code = normalize_language(code);
    LANGUAGES
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, label)| *label)
        .unwrap_or("Auto-detect")
}

/// Speech model selection. Stored as a plain string in settings.json.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
enum ModelChoice {
    /// The folder from NEMOTRON_MODEL_DIR, else `model_dir` in settings.json.
    Predefined,
    /// `models\nemotron`.
    #[default]
    English,
    /// `models\nemotron_multi`; the model detects the spoken language itself.
    Multilingual,
}

impl ModelChoice {
    /// Display order in the settings switch and the tray submenu.
    const ALL: [ModelChoice; 3] = [
        ModelChoice::English,
        ModelChoice::Multilingual,
        ModelChoice::Predefined,
    ];

    fn label(self) -> &'static str {
        match self {
            ModelChoice::Predefined => "Predefined",
            ModelChoice::English => "English",
            ModelChoice::Multilingual => "Multilingual",
        }
    }

    /// The settings.json spelling.
    fn key(self) -> &'static str {
        match self {
            ModelChoice::Predefined => "predefined",
            ModelChoice::English => "english",
            ModelChoice::Multilingual => "multilingual",
        }
    }

    /// Parse a settings.json value; None for unknown values (e.g. the old "auto").
    fn parse(value: &str) -> Option<ModelChoice> {
        ModelChoice::ALL
            .into_iter()
            .find(|choice| choice.key().eq_ignore_ascii_case(value.trim()))
    }

    /// Folder name under `models\` for the standard choices; None for Predefined.
    fn folder_name(self) -> Option<&'static str> {
        match self {
            ModelChoice::Predefined => None,
            ModelChoice::English => Some("nemotron"),
            ModelChoice::Multilingual => Some("nemotron_multi"),
        }
    }
}

impl From<String> for ModelChoice {
    /// Unknown values never invalidate the settings file; `load_settings`
    /// re-detects the choice for them.
    fn from(value: String) -> Self {
        ModelChoice::parse(&value).unwrap_or_default()
    }
}

impl From<ModelChoice> for String {
    fn from(value: ModelChoice) -> Self {
        value.key().to_string()
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            start_with_windows: true,
            preload_model: true,
            // Default to the safe clipboard path: live keystroke injection only behaves
            // in real text fields, so it is opt-in. On stop we paste once (one Ctrl+V).
            live_type_into_cursor: false,
            copy_final_to_clipboard: true,
            paste_final_on_stop: true,
            sounds_enabled: true,
            waveform_enabled: true,
            bubble_click_opens_settings: true,
            show_floating_bubble: true,
            tray_waveform_enabled: true,
            enter_stops_recording: false,
            silence_auto_stop: true,
            hotkey_mods: MOD_CONTROL,
            hotkey_vk: VK_SPACE as u32,
            paste_delay_ms: 60,
            history_limit: 20,
            model: ModelChoice::English,
            model_dir: String::new(),
            language: "auto".to_string(),
            compute: ComputeChoice::Cpu,
            bubble_position: BubblePosition::BottomRight,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct HistoryItem {
    timestamp: String,
    text: String,
    /// Where the dictation was processed: "CPU" or "GPU". Missing in older entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    processing: Option<String>,
    /// Average inference time per 560 ms audio chunk, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ms_per_chunk: Option<u32>,
}

impl HistoryItem {
    /// Header line shown above the text, e.g. "Nov 24, 11:34 AM · CPU · 130 ms per chunk".
    fn header(&self) -> String {
        match (&self.processing, self.ms_per_chunk) {
            (Some(device), Some(ms)) => format!("{} · {device} · {ms} ms per chunk", self.timestamp),
            _ => self.timestamp.clone(),
        }
    }
}

/// Processing statistics of one dictation, stored with its history entry.
struct DictationStats {
    on_gpu: bool,
    ms_per_chunk: f32,
}

#[derive(Debug)]
struct UiState {
    recording: bool,
    visible: bool,
    status: String,
    transcript: String,
    model_dir: String,
    /// Short model state for the settings window, e.g. "Loaded: Multilingual".
    model_status: String,
    waveform: Vec<f32>,
    current_level: f32,
    last_voice: Instant,
}

struct AppState {
    engine_tx: Sender<EngineCommand>,
    recording: Arc<AtomicBool>,
    hwnd: AtomicIsize,
    target_hwnd: AtomicIsize,
    settings_hwnd: AtomicIsize,
    /// History list: scroll offset and total content height (px), the clickable
    /// copy icons of the last paint (rect, history index), and the entry whose
    /// icon currently shows the "copied" checkmark.
    history_scroll: AtomicI32,
    history_content_h: AtomicI32,
    history_hits: Mutex<Vec<(RectI, usize)>>,
    copied_flash: Mutex<Option<usize>>,
    capturing_hotkey: AtomicBool,
    /// Active tab of the settings window (TAB_HISTORY / TAB_MODEL / TAB_SETTINGS).
    settings_tab: AtomicUsize,
    enter_pending: AtomicBool,
    drag: Mutex<Option<DragState>>,
    settings: Mutex<AppSettings>,
    history: Mutex<Vec<HistoryItem>>,
    ui: Mutex<UiState>,
}

struct DragState {
    hwnd: isize,
    start_cursor: POINT,
    start_rect: RECT,
    moved: bool,
}

#[derive(Copy, Clone)]
struct RectI {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl RectI {
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }
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

fn run_app() -> Result<()> {
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
        settings_hwnd: AtomicIsize::new(0),
        history_scroll: AtomicI32::new(0),
        history_content_h: AtomicI32::new(0),
        history_hits: Mutex::new(Vec::new()),
        copied_flash: Mutex::new(None),
        capturing_hotkey: AtomicBool::new(false),
        settings_tab: AtomicUsize::new(TAB_HISTORY),
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

    // A missing/disabled microphone is not fatal: tell the user, keep running in
    // the tray, and retry opening the device the next time dictation starts.
    if !ensure_audio_capture(&app) {
        update_ui(&app, |ui| {
            ui.status = "No microphone available. Connect one, then start dictation.".to_string();
        });
    }
    if settings_snapshot(&app).preload_model {
        let _ = app.engine_tx.send(EngineCommand::Preload);
    }

    unsafe {
        run_window(app).context("failed to run window")?;
    }

    AUDIO_STREAM.with(|slot| slot.borrow_mut().take());
    Ok(())
}

/// Make sure a working microphone stream is open. Returns false (after logging
/// and notifying the user) when no usable input device is available. Must run on
/// the main/UI thread, which owns the stream.
fn ensure_audio_capture(app: &Arc<AppState>) -> bool {
    let healthy = AUDIO_STREAM.with(|slot| slot.borrow().is_some()) && !MIC_FAILED.load(Ordering::SeqCst);
    if healthy {
        return true;
    }
    // Drop a stream whose device has gone away before opening a fresh one.
    AUDIO_STREAM.with(|slot| slot.borrow_mut().take());
    MIC_FAILED.store(false, Ordering::SeqCst);

    match start_audio_capture(app.clone()) {
        Ok(stream) => {
            AUDIO_STREAM.with(|slot| *slot.borrow_mut() = Some(stream));
            true
        }
        Err(err) => {
            let message = format!("{err:#}");
            log_error(&format!("Microphone unavailable: {message}"));
            notify_user(
                "Nemotron Bubble: no microphone",
                &format!(
                    "No usable microphone was found ({message}).\n\n\
                     Connect or enable a microphone (Windows Settings > System > Sound > Input), \
                     then press your shortcut again. The app keeps running in the tray.\n\n\
                     Log file: {}",
                    log_path().display()
                ),
                MB_ICONWARNING,
            );
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostics: log file + DebugView + user-facing message boxes.
// ---------------------------------------------------------------------------

fn log_path() -> PathBuf {
    log_dir().join("nemotron-bubble.log")
}

const LOG_MAX_BYTES: u64 = 1024 * 1024;

fn log_info(message: &str) {
    write_log("INFO ", message);
}

fn log_error(message: &str) {
    write_log("ERROR", message);
}

/// Append one line to <exe dir>\log\nemotron-bubble.log (rotated to
/// .log.old at 1 MB) and mirror it to OutputDebugString so DebugView sees it.
fn write_log(level: &str, message: &str) {
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

fn show_message_blocking(title: &str, text: &str, icon: MESSAGEBOX_STYLE) {
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
fn notify_user(title: &str, text: &str, icon: MESSAGEBOX_STYLE) {
    let title = title.to_string();
    let text = text.to_string();
    thread::spawn(move || show_message_blocking(&title, &text, icon));
}

/// Append any panic (message + location, from any thread) to crash.log. The
/// release build has no console, so without this a panic that aborts the process
/// leaves no trace. Runs before unwinding, so it captures the cause even when the
/// panic later aborts by crossing an FFI callback boundary.
fn install_crash_logger() {
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

fn self_test() -> Result<()> {
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
    let _ = model.transcribe_chunk(&vec![0.0; NEMOTRON_CHUNK_SIZE])?;
    const TIMED_CHUNKS: u32 = 5;
    let started = Instant::now();
    for _ in 0..TIMED_CHUNKS {
        let _ = model.transcribe_chunk(&vec![0.0; NEMOTRON_CHUNK_SIZE])?;
    }
    let per_chunk_ms = started.elapsed().as_secs_f32() * 1000.0 / TIMED_CHUNKS as f32;
    println!("Nemotron loaded: {mode}");
    println!("Running on: {}", if loaded.on_gpu { "GPU (DirectML)" } else { "CPU" });
    println!("Time per 560 ms audio chunk: {per_chunk_ms:.0} ms");
    println!("Model dir: {}", model_dir.display());
    log_info(&format!(
        "Self-test: {mode} on the {}, {per_chunk_ms:.0} ms per 560 ms chunk",
        if loaded.on_gpu { "GPU" } else { "CPU" }
    ));
    Ok(())
}

fn apply_startup_setting(enabled: bool) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok((run_key, _)) =
        hkcu.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
    else {
        return;
    };

    if enabled {
        let Ok(exe) = env::current_exe() else {
            return;
        };
        let value = format!("\"{}\"", exe.display());
        let _ = run_key.set_value("NemotronBubble", &value);
    } else {
        let _ = run_key.delete_value("NemotronBubble");
    }
}

fn settings_snapshot(app: &Arc<AppState>) -> AppSettings {
    app.settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn history_snapshot(app: &Arc<AppState>) -> Vec<HistoryItem> {
    app.history
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn toggle_setting(app: &Arc<AppState>, id: usize) {
    let settings = {
        let mut settings = app
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match id {
            SETTINGS_TOGGLE_STARTUP => settings.start_with_windows = !settings.start_with_windows,
            SETTINGS_TOGGLE_PRELOAD => settings.preload_model = !settings.preload_model,
            SETTINGS_TOGGLE_LIVE_TYPE => {
                settings.live_type_into_cursor = !settings.live_type_into_cursor
            }
            SETTINGS_TOGGLE_FINAL_CLIPBOARD => {
                settings.copy_final_to_clipboard = !settings.copy_final_to_clipboard
            }
            SETTINGS_TOGGLE_FINAL_PASTE => {
                settings.paste_final_on_stop = !settings.paste_final_on_stop
            }
            SETTINGS_TOGGLE_SOUNDS => settings.sounds_enabled = !settings.sounds_enabled,
            SETTINGS_TOGGLE_WAVEFORM => settings.waveform_enabled = !settings.waveform_enabled,
            SETTINGS_TOGGLE_BUBBLE_CLICK_SETTINGS => {
                settings.bubble_click_opens_settings = !settings.bubble_click_opens_settings
            }
            SETTINGS_TOGGLE_FLOATING_BUBBLE => {
                settings.show_floating_bubble = !settings.show_floating_bubble
            }
            SETTINGS_TOGGLE_TRAY_WAVEFORM => {
                settings.tray_waveform_enabled = !settings.tray_waveform_enabled
            }
            SETTINGS_TOGGLE_ENTER_STOPS => {
                settings.enter_stops_recording = !settings.enter_stops_recording
            }
            SETTINGS_TOGGLE_SILENCE_STOP => {
                settings.silence_auto_stop = !settings.silence_auto_stop
            }
            _ => {}
        }
        settings.clone()
    };

    save_settings(&settings);
    apply_startup_setting(settings.start_with_windows);
    // If toggled during a live recording, add/remove the Enter hotkey immediately.
    if id == SETTINGS_TOGGLE_ENTER_STOPS && app.recording.load(Ordering::SeqCst) {
        unsafe {
            let hwnd = hwnd_from_app(app);
            if settings.enter_stops_recording {
                register_enter_hotkey(hwnd);
            } else {
                unregister_enter_hotkey(hwnd);
            }
        }
    }
    if settings.preload_model {
        let _ = app.engine_tx.send(EngineCommand::Preload);
    }
    unsafe {
        let hwnd = hwnd_from_app(app);
        if hwnd != null_mut() {
            if settings.show_floating_bubble {
                render_bubble(hwnd);
                ShowWindow(hwnd, SW_SHOWNA);
            } else {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
    }
    refresh_tray_idle(app);
    update_ui(app, |ui| {
        ui.status = "Settings saved.".to_string();
    });
}

/// Switch the speech model: unload the current one (freeing its RAM) and, with
/// preloading on, load the new one in the background right away.
fn select_model(app: &Arc<AppState>, choice: ModelChoice) {
    if app.recording.load(Ordering::SeqCst) {
        update_ui(app, |ui| {
            ui.status = "Stop dictation before switching models.".to_string();
        });
        return;
    }
    if !model_choice_available(&settings_snapshot(app), choice) {
        update_ui(app, |ui| {
            ui.status = "Predefined needs NEMOTRON_MODEL_DIR or \"model_dir\" in config\\settings.json \
                         (restart the app after setting it)."
                .to_string();
        });
        return;
    }
    let settings = {
        let mut settings = app
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if settings.model == choice {
            return;
        }
        settings.model = choice;
        settings.clone()
    };
    save_settings(&settings);
    log_info(&format!("Speech model changed to {}", choice.label()));

    let _ = app.engine_tx.send(EngineCommand::Unload);
    if settings.preload_model {
        let _ = app.engine_tx.send(EngineCommand::Preload);
    }
    update_ui(app, |ui| {
        ui.status = if settings.preload_model {
            format!("Switching to {} model...", choice.label())
        } else {
            format!("{} model selected. It loads on first dictation.", choice.label())
        };
    });
}

/// Switch between GPU and CPU processing. The device is fixed when a model is
/// loaded, so this unloads it and (with preloading on) loads it again.
fn select_compute(app: &Arc<AppState>, choice: ComputeChoice) {
    if app.recording.load(Ordering::SeqCst) {
        update_ui(app, |ui| {
            ui.status = "Stop dictation before switching processing.".to_string();
        });
        return;
    }
    let settings = {
        let mut settings = app
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if settings.compute == choice {
            return;
        }
        settings.compute = choice;
        settings.clone()
    };
    save_settings(&settings);
    log_info(&format!("Processing changed to {}", choice.label()));

    let _ = app.engine_tx.send(EngineCommand::Unload);
    if settings.preload_model {
        let _ = app.engine_tx.send(EngineCommand::Preload);
    }
    update_ui(app, |ui| {
        ui.status = if settings.preload_model {
            format!("Reloading the model for processing: {}...", choice.label())
        } else {
            format!("Processing: {}. Applies on the next dictation.", choice.label())
        };
    });
}

/// The language only matters for the multilingual model, so the Language
/// controls are greyed out when the English model is selected. (Predefined may
/// point at either model, so it keeps them enabled.)
fn language_selectable(settings: &AppSettings) -> bool {
    settings.model != ModelChoice::English
}

/// Set the spoken language for the multilingual model. Applied immediately to a
/// loaded model (no reload) and saved for future loads.
fn select_language(app: &Arc<AppState>, code: &'static str) {
    if app.recording.load(Ordering::SeqCst) {
        update_ui(app, |ui| {
            ui.status = "Stop dictation before changing the language.".to_string();
        });
        return;
    }
    let settings = {
        let mut settings = app
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !language_selectable(&settings) {
            drop(settings);
            update_ui(app, |ui| {
                ui.status = "The language applies to the multilingual model only.".to_string();
            });
            return;
        }
        if normalize_language(&settings.language) == code {
            return;
        }
        settings.language = code.to_string();
        settings.clone()
    };
    save_settings(&settings);
    let _ = app.engine_tx.send(EngineCommand::SetLanguage(code.to_string()));
    update_ui(app, |ui| {
        ui.status = format!("Language set to {}.", language_label(code));
    });
}

/// Popup menu under the settings window's Language button.
unsafe fn show_language_menu(app: &Arc<AppState>) {
    let menu = build_language_menu(&settings_snapshot(app), app.recording.load(Ordering::SeqCst));
    show_menu_below(app, menu, settings_language_rect());
}

/// Popup menu with the bubble position presets, current one checked.
unsafe fn show_bubble_position_menu(app: &Arc<AppState>) {
    let menu = CreatePopupMenu();
    if menu == null_mut() {
        return;
    }
    let current = settings_snapshot(app).bubble_position;
    for (index, position) in BubblePosition::ALL.iter().enumerate() {
        let mut flags = MF_STRING;
        if *position == current {
            flags |= MF_CHECKED;
        }
        let text = wide_null(position.label());
        AppendMenuW(menu, flags, IDM_BUBBLE_POS_BASE + index, text.as_ptr());
        if index == 2 {
            AppendMenuW(menu, MF_SEPARATOR, 0, null()); // between top and bottom row
        }
    }
    show_menu_below(app, menu, settings_bubble_position_rect());
}

/// Show `menu` below a button of the settings window and destroy it afterwards.
/// The choice arrives as WM_COMMAND and is handled by handle_command.
unsafe fn show_menu_below(app: &Arc<AppState>, menu: HMENU, button: RectI) {
    let hwnd = settings_hwnd_from_app(app);
    if hwnd == null_mut() || menu == null_mut() {
        if menu != null_mut() {
            DestroyMenu(menu);
        }
        return;
    }
    let mut point = POINT {
        x: button.left,
        y: button.bottom + 2,
    };
    ClientToScreen(hwnd, &mut point);
    SetForegroundWindow(hwnd);
    TrackPopupMenu(menu, 0, point.x, point.y, 0, hwnd, null());
    DestroyMenu(menu);
}

/// Move the floating bubble to another preset position and save it.
fn select_bubble_position(app: &Arc<AppState>, position: BubblePosition) {
    let settings = {
        let mut settings = app
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        settings.bubble_position = position;
        settings.clone()
    };
    save_settings(&settings);
    unsafe {
        let hwnd = hwnd_from_app(app);
        if hwnd != null_mut() {
            place_window(hwnd);
        }
    }
    update_ui(app, |ui| {
        ui.status = format!("Bubble position: {}.", position.label());
    });
}

/// The Language menu (settings popup and tray submenu): current one checked,
/// everything greyed out while dictating or with the English model selected.
unsafe fn build_language_menu(settings: &AppSettings, recording: bool) -> HMENU {
    let menu = CreatePopupMenu();
    if menu == null_mut() {
        return menu;
    }
    let current = normalize_language(&settings.language);
    let enabled = !recording && language_selectable(settings);
    for (index, (code, label)) in LANGUAGES.iter().enumerate() {
        let mut flags = MF_STRING;
        if *code == current {
            flags |= MF_CHECKED;
        }
        if !enabled {
            flags |= MF_GRAYED;
        }
        let text = wide_null(label);
        // AppendMenuW copies the string, so the buffer may be dropped afterwards.
        AppendMenuW(menu, flags, IDM_LANG_BASE + index, text.as_ptr());
        if index == 0 {
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
        }
    }
    menu
}

fn load_settings() -> AppSettings {
    let path = settings_path();
    let Ok(json) = fs::read_to_string(&path) else {
        // First start: no settings yet, so pick the model from what is installed.
        let mut settings = AppSettings::default();
        settings.model = detect_model_choice(&settings.model_dir);
        return settings;
    };
    match serde_json::from_str::<AppSettings>(&json) {
        Ok(mut settings) => {
            // No (valid) model choice saved yet, e.g. settings from before the
            // model picker or the old "auto" value: detect it like a first start.
            let saved = serde_json::from_str::<serde_json::Value>(&json)
                .ok()
                .and_then(|value| value.get("model").and_then(|m| m.as_str()).map(str::to_string));
            if saved.as_deref().and_then(ModelChoice::parse).is_none() {
                settings.model = detect_model_choice(&settings.model_dir);
            }
            settings
        }
        Err(err) => {
            // The file is rewritten with defaults right after this, so keep a copy.
            let backup = path.with_extension("json.invalid");
            let _ = fs::copy(&path, &backup);
            log_error(&format!(
                "{} is invalid ({err}); using defaults. Old file kept as {}",
                path.display(),
                backup.display()
            ));
            let mut settings = AppSettings::default();
            settings.model = detect_model_choice(&settings.model_dir);
            settings
        }
    }
}

fn save_settings(settings: &AppSettings) {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = fs::write(path, json);
    }
}

fn load_history(limit: usize) -> Vec<HistoryItem> {
    let path = history_path();
    let mut history = fs::read_to_string(path)
        .ok()
        .and_then(|json| serde_json::from_str::<Vec<HistoryItem>>(&json).ok())
        .unwrap_or_default();
    history.truncate(limit);
    history
}

fn save_history(history: &[HistoryItem]) {
    let path = history_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(history) {
        let _ = fs::write(path, json);
    }
}

fn add_history(app: &Arc<AppState>, text: String, stats: Option<DictationStats>) {
    let clean = text.trim().to_string();
    if clean.is_empty() {
        return;
    }
    let limit = settings_snapshot(app).history_limit;
    let mut history = app
        .history
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    history.insert(
        0,
        HistoryItem {
            timestamp: Local::now().format("%b %-d, %-I:%M %p").to_string(),
            text: clean,
            processing: stats
                .as_ref()
                .map(|s| if s.on_gpu { "GPU" } else { "CPU" }.to_string()),
            ms_per_chunk: stats.as_ref().map(|s| s.ms_per_chunk.round() as u32),
        },
    );
    history.truncate(limit);
    save_history(&history);
    drop(history);
    refresh_history_view(app);
}

/// After the history changed: scroll back to the newest entry and repaint the
/// settings window (posted, so this is safe from the engine thread too).
fn refresh_history_view(app: &Arc<AppState>) {
    app.history_scroll.store(0, Ordering::SeqCst);
    *app.copied_flash.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    unsafe {
        let hwnd = settings_hwnd_from_app(app);
        if hwnd != null_mut() {
            PostMessageW(hwnd, WM_UI_UPDATE, 0, 0);
        }
    }
}

/// Copy one history entry to the clipboard and flash its icon as a checkmark.
fn copy_history_item(app: &Arc<AppState>, index: usize) {
    let Some(item) = history_snapshot(app).get(index).cloned() else {
        return;
    };
    let hwnd = settings_hwnd_from_app(app);
    match set_clipboard_text(&item.text, hwnd) {
        Ok(()) => {
            *app.copied_flash.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(index);
            unsafe {
                if hwnd != null_mut() {
                    SetTimer(hwnd, COPY_FLASH_TIMER_ID, COPY_FLASH_MS, None);
                    InvalidateRect(hwnd, null(), 0);
                }
            }
        }
        Err(err) => log_error(&format!("Copying a history entry failed: {err:#}")),
    }
}

/// Scroll the history list by `delta` pixels (positive = further down).
fn scroll_history(app: &Arc<AppState>, delta: i32) {
    let view_h = HISTORY_LIST_BOTTOM - HISTORY_LIST_TOP;
    let max = (app.history_content_h.load(Ordering::SeqCst) - view_h).max(0);
    let old = app.history_scroll.load(Ordering::SeqCst);
    let new = (old + delta).clamp(0, max);
    if new != old {
        app.history_scroll.store(new, Ordering::SeqCst);
        unsafe {
            let hwnd = settings_hwnd_from_app(app);
            if hwnd != null_mut() {
                InvalidateRect(hwnd, null(), 0);
            }
        }
    }
}

fn clear_history(app: &Arc<AppState>) {
    {
        let mut history = app
            .history
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        history.clear();
        save_history(&history);
    }
    update_ui(app, |ui| {
        ui.status = "History cleared.".to_string();
    });
    refresh_history_view(app);
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

fn history_path() -> PathBuf {
    config_dir().join("history.json")
}

/// The folder containing the exe. The app is portable: everything it writes
/// lives below this folder. Deliberately NOT the working directory — "Start with
/// Windows" launches us with the working directory set to System32.
fn app_root_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Settings, history and generated sound cues: `<exe dir>\config`.
fn config_dir() -> PathBuf {
    app_root_dir().join("config")
}

/// Diagnostic log and crash log: `<exe dir>\log`.
fn log_dir() -> PathBuf {
    app_root_dir().join("log")
}

fn ensure_sound_assets() {
    let dir = config_dir().join("sounds");
    let _ = fs::create_dir_all(&dir);
    let start = dir.join("start.wav");
    let stop = dir.join("stop.wav");
    let _ = write_chime(&start, &[(329.63, 0.08), (392.00, 0.12)]);
    let _ = write_chime(&stop, &[(392.00, 0.07), (293.66, 0.11)]);
}

fn sound_path(name: &str) -> PathBuf {
    config_dir().join("sounds").join(name)
}

fn write_chime(path: &PathBuf, notes: &[(f32, f32)]) -> Result<()> {
    const SAMPLE_RATE: u32 = 22_050;
    let mut samples = Vec::<i16>::new();
    for (freq, seconds) in notes {
        let count = (SAMPLE_RATE as f32 * seconds) as usize;
        for i in 0..count {
            let t = i as f32 / SAMPLE_RATE as f32;
            let progress = i as f32 / count.max(1) as f32;
            let attack = (progress / 0.18).min(1.0);
            let release = ((1.0 - progress) / 0.35).min(1.0);
            let envelope = attack.min(release).powf(1.4);
            let wave = (2.0 * std::f32::consts::PI * freq * t).sin();
            samples.push((wave * envelope * 6_500.0) as i16);
        }
    }

    let mut file = fs::File::create(path)?;
    let data_len = samples.len() as u32 * 2;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_len).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&SAMPLE_RATE.to_le_bytes())?;
    file.write_all(&(SAMPLE_RATE * 2).to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    for sample in samples {
        file.write_all(&sample.to_le_bytes())?;
    }
    Ok(())
}

unsafe fn run_window(app: Arc<AppState>) -> Result<()> {
    let hinstance = GetModuleHandleW(null());
    if hinstance == null_mut() {
        return Err(anyhow!("GetModuleHandleW failed"));
    }

    let class_name = wide_null(CLASS_NAME);
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: null_mut(),
        lpszClassName: class_name.as_ptr(),
        ..std::mem::zeroed()
    };

    if RegisterClassW(&wc) == 0 {
        return Err(anyhow!("RegisterClassW failed"));
    }

    let title = wide_null("Nemotron Bubble");
    // No WS_VISIBLE: per-pixel-alpha layered windows are made visible only after the
    // first UpdateLayeredWindow so there is never a blank flash.
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_POPUP,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        WIDTH,
        HEIGHT,
        null_mut(),
        null_mut(),
        hinstance,
        null_mut(),
    );

    if hwnd == null_mut() {
        return Err(anyhow!("CreateWindowExW failed"));
    }
    // NOTE: a per-pixel-alpha window driven by UpdateLayeredWindow must NOT also call
    // SetLayeredWindowAttributes, and needs no window region — the alpha gives the shape.

    app.hwnd.store(hwnd as isize, Ordering::SeqCst);
    place_window(hwnd);
    render_bubble(hwnd);
    add_tray_icon(hwnd);

    let settings_title = wide_null("Nemotron Bubble Settings");
    let settings_hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
        class_name.as_ptr(),
        settings_title.as_ptr(),
        WS_POPUP,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        SETTINGS_WIDTH,
        SETTINGS_HEIGHT,
        null_mut(),
        null_mut(),
        hinstance,
        null_mut(),
    );
    if settings_hwnd != null_mut() {
        SetLayeredWindowAttributes(settings_hwnd, 0, 246, LWA_ALPHA);
        app.settings_hwnd
            .store(settings_hwnd as isize, Ordering::SeqCst);
        place_settings_window(settings_hwnd, SettingsOrigin::Tray); // re-placed whenever shown
        apply_settings_rounding(settings_hwnd);
        ShowWindow(settings_hwnd, SW_HIDE);
    }

    let snapshot = settings_snapshot(&app);
    if !register_global_hotkey(hwnd, snapshot.hotkey_mods, snapshot.hotkey_vk) {
        log_error(&format!(
            "Global shortcut (mods=0x{:x}, vk=0x{:x}) is already taken by another app.",
            snapshot.hotkey_mods, snapshot.hotkey_vk
        ));
        update_ui(&app, |ui| {
            ui.status = "Shortcut unavailable — open settings to choose another.".to_string();
        });
    }

    if settings_snapshot(&app).show_floating_bubble {
        ShowWindow(hwnd, SW_SHOWNA);
    } else {
        ShowWindow(hwnd, SW_HIDE);
    }

    // Drive waveform + tray animation from a single UI-thread timer.
    SetTimer(hwnd, ANIM_TIMER_ID, ANIM_INTERVAL_MS, None);

    let mut msg: MSG = std::mem::zeroed();
    while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    Ok(())
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => 0,
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            if is_settings_window(hwnd) {
                paint_settings(hwnd);
            } else {
                paint(hwnd);
            }
            0
        }
        WM_HOTKEY => {
            match wparam as i32 {
                HOTKEY_ID => toggle_recording(),
                ENTER_HOTKEY_ID => {
                    if let Some(app) = APP.get() {
                        if app.recording.load(Ordering::SeqCst) {
                            // Stop, then the engine re-injects Enter once the full
                            // transcript has been typed (so the text submits).
                            app.enter_pending.store(true, Ordering::SeqCst);
                            stop_recording(app);
                        }
                    }
                }
                _ => {}
            }
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if is_settings_window(hwnd) {
                if let Some(app) = APP.get() {
                    if app.capturing_hotkey.load(Ordering::SeqCst) {
                        handle_capture_key(app, wparam as u32);
                        return 0;
                    }
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = lparam_point(lparam);
            if should_start_drag(hwnd, x, y) {
                begin_drag(hwnd);
            }
            0
        }
        WM_MOUSEMOVE => {
            continue_drag();
            0
        }
        WM_LBUTTONUP => {
            let (x, y) = lparam_point(lparam);
            let was_click = end_drag();
            if was_click {
                if is_settings_window(hwnd) {
                    handle_settings_click(x, y);
                } else {
                    handle_click(x, y);
                }
            }
            0
        }
        WM_TRAYICON => {
            match lparam as u32 {
                WM_LBUTTONUP => toggle_settings_window(SettingsOrigin::Tray),
                WM_RBUTTONUP => show_tray_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            handle_command(loword(wparam as usize) as usize);
            0
        }
        WM_TIMER => {
            if wparam == ANIM_TIMER_ID {
                on_anim_tick(hwnd);
            } else if wparam == COPY_FLASH_TIMER_ID {
                // the "copied" checkmark has been shown long enough
                KillTimer(hwnd, COPY_FLASH_TIMER_ID);
                if let Some(app) = APP.get() {
                    *app.copied_flash.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
                }
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_MOUSEWHEEL => {
            if let Some(app) = APP.get() {
                if is_settings_window(hwnd) && app.settings_tab.load(Ordering::SeqCst) == TAB_HISTORY {
                    // high word of wparam: signed wheel delta, 120 per notch
                    let delta = ((wparam >> 16) & 0xFFFF) as u16 as i16 as i32;
                    scroll_history(app, -delta * HISTORY_WHEEL_STEP / 120);
                }
            }
            0
        }
        WM_UI_UPDATE => {
            if is_settings_window(hwnd) {
                InvalidateRect(hwnd, null(), 0);
            } else {
                render_bubble(hwnd);
            }
            0
        }
        WM_DESTROY => {
            if let Some(app) = APP.get() {
                if hwnd == hwnd_from_app(app) {
                    let _ = app.engine_tx.send(EngineCommand::Shutdown);
                    KillTimer(hwnd, ANIM_TIMER_ID);
                    UnregisterHotKey(hwnd, HOTKEY_ID);
                    remove_tray_icon(hwnd);
                    PostQuitMessage(0);
                }
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn run_engine(rx: Receiver<EngineCommand>, app: Arc<AppState>) {
    let mut model: Option<Nemotron> = None;
    let mut model_path = PathBuf::new();
    let mut on_gpu = false;
    // Inference time of the current dictation, logged on stop (GPU vs CPU check).
    let mut chunk_time = Duration::ZERO;
    let mut chunk_count = 0u32;
    let mut active = false;
    let mut audio_buffer = Vec::<f32>::with_capacity(NEMOTRON_CHUNK_SIZE * 2);
    // Set after a failed background preload so that unrelated settings toggles
    // (each of which re-sends Preload) don't pop the same error dialog again.
    // Cleared by a model switch (Unload); an explicit Start always retries.
    let mut preload_failed = false;

    loop {
        match rx.recv() {
            Ok(EngineCommand::Preload) => {
                if model.is_some() || preload_failed {
                    continue;
                }
                update_ui(&app, |ui| {
                    ui.status = "Loading Nemotron...".to_string();
                    ui.model_status = "Loading...".to_string();
                });
                match load_nemotron(&settings_snapshot(&app)) {
                    Ok(loaded) => {
                        let label = model_status_label(
                            &loaded.model,
                            &settings_snapshot(&app).language,
                            loaded.on_gpu,
                        );
                        model = Some(loaded.model);
                        model_path = loaded.dir;
                        on_gpu = loaded.on_gpu;
                        update_ui(&app, |ui| {
                            ui.status = "Ready. Nemotron loaded. Ctrl+Space toggles.".to_string();
                            ui.model_dir = model_path.display().to_string();
                            ui.model_status = label;
                            if ui.transcript.trim().is_empty() {
                                ui.transcript = "Tap Ctrl+Space to dictate.".to_string();
                            }
                        });
                    }
                    Err(err) => {
                        preload_failed = true;
                        report_model_error(&err);
                        update_ui(&app, |ui| {
                            ui.status = "Nemotron model missing or failed to load.".to_string();
                            ui.transcript = format!("{err:#}");
                            ui.model_dir = expected_model_hint();
                            ui.model_status = "Failed to load - see log.".to_string();
                        });
                    }
                }
            }
            Ok(EngineCommand::Unload) => {
                active = false;
                audio_buffer.clear();
                preload_failed = false;
                if model.take().is_some() {
                    log_info(&format!("Unloaded Nemotron model from {}", model_path.display()));
                }
                update_ui(&app, |ui| {
                    ui.model_status = "Not loaded yet.".to_string();
                });
            }
            Ok(EngineCommand::SetLanguage(language)) => {
                // Nothing loaded (or English-only): the saved setting is applied
                // when the multilingual model loads.
                if let Some(loaded) = model.as_mut() {
                    if loaded.mode() == NemotronMode::Multilingual {
                        match apply_language(loaded, &language) {
                            Ok(()) => {
                                let label = model_status_label(loaded, &language, on_gpu);
                                update_ui(&app, |ui| ui.model_status = label);
                            }
                            Err(err) => log_error(&format!("{err:#}")),
                        }
                    }
                }
            }
            Ok(EngineCommand::Start) => {
                audio_buffer.clear();
                chunk_time = Duration::ZERO;
                chunk_count = 0;
                if model.is_none() {
                    update_ui(&app, |ui| {
                        ui.status = "Loading Nemotron model...".to_string();
                        ui.model_status = "Loading...".to_string();
                    });
                    match load_nemotron(&settings_snapshot(&app)) {
                        Ok(loaded) => {
                            preload_failed = false;
                            let label = model_status_label(
                                &loaded.model,
                                &settings_snapshot(&app).language,
                                loaded.on_gpu,
                            );
                            update_ui(&app, |ui| ui.model_status = label);
                            model = Some(loaded.model);
                            model_path = loaded.dir;
                            on_gpu = loaded.on_gpu;
                        }
                        Err(err) => {
                            report_model_error(&err);
                            app.recording.store(false, Ordering::SeqCst);
                            update_ui(&app, |ui| {
                                ui.model_status = "Failed to load - see log.".to_string();
                                ui.recording = false;
                                ui.status = "Nemotron model missing or failed to load.".to_string();
                                ui.transcript = format!("{err:#}");
                                ui.model_dir = expected_model_hint();
                            });
                            continue;
                        }
                    }
                }

                if let Some(model) = model.as_mut() {
                    model.reset();
                    active = true;
                    update_ui(&app, |ui| {
                        ui.recording = true;
                        ui.status = "Listening...".to_string();
                        ui.transcript.clear();
                        ui.model_dir = model_path.display().to_string();
                    });
                    play_start_sound(&app);
                }
            }
            Ok(EngineCommand::Audio(samples)) => {
                if !active {
                    continue;
                }
                let Some(model) = model.as_mut() else {
                    continue;
                };

                audio_buffer.extend(samples);
                while audio_buffer.len() >= NEMOTRON_CHUNK_SIZE {
                    let chunk: Vec<f32> = audio_buffer.drain(..NEMOTRON_CHUNK_SIZE).collect();
                    let chunk_started = Instant::now();
                    let result = model.transcribe_chunk(&chunk);
                    chunk_time += chunk_started.elapsed();
                    chunk_count += 1;
                    match result {
                        Ok(text) if !text.is_empty() => {
                            let settings = settings_snapshot(&app);
                            let live_type = settings.live_type_into_cursor;
                            // Type the new words directly via Unicode keystrokes —
                            // the clipboard stays untouched until the final stop.
                            let type_result = if live_type {
                                type_unicode_text(
                                    &text,
                                    hwnd_from_app(&app),
                                    target_hwnd_from_app(&app),
                                    settings.paste_delay_ms,
                                )
                            } else {
                                Ok(())
                            };
                            update_ui(&app, |ui| {
                                ui.status = if let Err(err) = type_result {
                                    format!("Live typing failed: {err}")
                                } else if live_type {
                                    "Listening... typing live.".to_string()
                                } else {
                                    "Listening...".to_string()
                                };
                                ui.transcript.push_str(&text);
                            });
                        }
                        Ok(_) => {}
                        Err(err) => {
                            log_error(&format!("ASR error while transcribing: {err:#}"));
                            active = false;
                            app.recording.store(false, Ordering::SeqCst);
                            update_ui(&app, |ui| {
                                ui.recording = false;
                                ui.status = "ASR error.".to_string();
                                ui.transcript = format!("{err:#}");
                            });
                        }
                    }
                }
            }
            Ok(EngineCommand::Stop) => {
                app.recording.store(false, Ordering::SeqCst);
                // Enter-to-stop wants a near-instant finish, so skip the extra
                // silence-flush passes (they only squeeze out a touch more tail).
                let fast = app.enter_pending.load(Ordering::SeqCst);
                if active {
                    active = false;
                    if let Some(model) = model.as_mut() {
                        if !audio_buffer.is_empty() {
                            audio_buffer.resize(NEMOTRON_CHUNK_SIZE, 0.0);
                            if let Ok(text) = model.transcribe_chunk(&audio_buffer) {
                                paste_live_tail(&app, &text);
                            }
                            audio_buffer.clear();
                        }
                        let flush_passes = if fast { 0 } else { 3 };
                        for _ in 0..flush_passes {
                            if let Ok(text) = model.transcribe_chunk(&vec![0.0; NEMOTRON_CHUNK_SIZE])
                            {
                                paste_live_tail(&app, &text);
                            }
                        }

                        let stats = (chunk_count > 0).then(|| DictationStats {
                            on_gpu,
                            ms_per_chunk: chunk_time.as_secs_f32() * 1000.0 / chunk_count as f32,
                        });
                        if let Some(stats) = &stats {
                            log_info(&format!(
                                "Dictation: {chunk_count} chunks, {:.0} ms per 560 ms chunk on the {}",
                                stats.ms_per_chunk,
                                if stats.on_gpu { "GPU" } else { "CPU" }
                            ));
                        }
                        chunk_time = Duration::ZERO;
                        chunk_count = 0;

                        let final_text = model.get_transcript();
                        let has_final = !final_text.trim().is_empty();
                        let settings = settings_snapshot(&app);
                        let mut final_status = if has_final {
                            "Stopped. Text was typed live.".to_string()
                        } else {
                            "Ready. Press Ctrl+Space to start.".to_string()
                        };

                        if has_final {
                            add_history(&app, final_text.clone(), stats);
                            // Paste-on-stop already writes the clipboard via Ctrl+V, so
                            // only do a standalone copy when not pasting — never both.
                            if settings.paste_final_on_stop {
                                final_status = match paste_text_to_target(
                                    &final_text,
                                    hwnd_from_app(&app),
                                    target_hwnd_from_app(&app),
                                    settings.paste_delay_ms,
                                ) {
                                    Ok(()) => "Stopped. Final transcript pasted.".to_string(),
                                    Err(err) => {
                                        log_error(&format!("Final paste failed: {err:#}"));
                                        format!("Final paste failed: {err}")
                                    }
                                };
                            } else if settings.copy_final_to_clipboard {
                                final_status =
                                    match set_clipboard_text(&final_text, hwnd_from_app(&app)) {
                                        Ok(()) => {
                                            "Stopped. Final transcript copied to clipboard."
                                                .to_string()
                                        }
                                        Err(err) => {
                                            log_error(&format!("Clipboard failed: {err:#}"));
                                            format!("Clipboard failed: {err}")
                                        }
                                    };
                            }
                        }

                        update_ui(&app, |ui| {
                            ui.recording = false;
                            ui.status = final_status;
                            ui.transcript = if !has_final {
                                "No speech detected.".to_string()
                            } else {
                                final_text
                            };
                        });
                    }
                } else {
                    update_ui(&app, |ui| {
                        ui.recording = false;
                        ui.status = "Ready. Press Ctrl+Space to start.".to_string();
                    });
                }
                // If this stop was triggered by Enter, submit now that the full
                // transcript has been typed.
                if app.enter_pending.swap(false, Ordering::SeqCst) {
                    let _ = unsafe { send_enter() };
                }
            }
            Ok(EngineCommand::Shutdown) | Err(_) => break,
        }
    }
}

fn report_model_error(err: &anyhow::Error) {
    let message = format!("{err:#}");
    log_error(&format!("Nemotron model failed to load: {message}"));
    notify_user(
        "Nemotron Bubble: speech model failed to load",
        &format!(
            "{message}\n\nYou can choose the speech model in the settings window.\n\n\
             Log file: {}",
            log_path().display()
        ),
        MB_ICONERROR,
    );
}

/// The process-wide ONNX Runtime environment (parakeet-rs >= 0.4 requires one).
/// Created on first use and shared by every model load, e.g. when switching models.
fn ort_environment() -> Result<Environment> {
    static ENV: OnceCell<Environment> = OnceCell::new();
    ENV.get_or_try_init(|| parakeet_rs::ort::init().build())
        .cloned()
        .map_err(|err| anyhow!("failed to initialize ONNX Runtime: {err}"))
}

fn model_status_label(model: &Nemotron, language: &str, on_gpu: bool) -> String {
    let model_part = match model.mode() {
        NemotronMode::EnglishOnly => "Loaded: English".to_string(),
        NemotronMode::Multilingual if normalize_language(language) == "auto" => {
            "Loaded: Multilingual (detects language)".to_string()
        }
        NemotronMode::Multilingual => format!("Loaded: Multilingual ({})", language_label(language)),
    };
    format!("{model_part} · {}", if on_gpu { "GPU" } else { "CPU" })
}

/// Point the multilingual model at `language` (a LANGUAGES code; "auto" lets
/// the model detect it). Takes effect from the next audio chunk, no reload needed.
fn apply_language(model: &mut Nemotron, language: &str) -> Result<()> {
    let code = normalize_language(language);
    model
        .set_target_lang(code)
        .map_err(|err| anyhow!("failed to set language '{code}': {err}"))?;
    log_info(&format!("Multilingual language: {} ({code})", language_label(code)));
    Ok(())
}

/// Check that DirectML can open a GPU, by registering it (with "fail if
/// unavailable") on a throwaway session builder. parakeet-rs itself registers
/// DirectML with a silent CPU fallback, so without this check "GPU" could quietly
/// run on the CPU.
fn probe_directml(environment: &Environment) -> Result<()> {
    use parakeet_rs::ort::ep::DirectML;
    use parakeet_rs::ort::session::Session;
    let builder = Session::builder(environment).map_err(|err| anyhow!("{err}"))?;
    builder
        .with_execution_providers([DirectML::default().build().error_on_failure()])
        .map_err(|err| anyhow!("{err}"))?;
    Ok(())
}

fn load_with_device(model_dir: &std::path::Path, on_gpu: bool) -> Result<Nemotron> {
    let mut exec_config = ExecutionConfig::new().with_environment(ort_environment()?);
    if on_gpu {
        // DirectML requires memory patterns off and sequential execution.
        exec_config = exec_config
            .with_execution_provider(ExecutionProvider::DirectML)
            .with_custom_configure(|builder| {
                Ok(builder
                    .with_memory_pattern(false)?
                    .with_parallel_execution(false)?)
            });
    }
    Nemotron::from_pretrained(model_dir, Some(exec_config))
        .map_err(|err| anyhow!("failed to load {}: {err}", model_dir.display()))
}

/// A loaded model plus where it came from and where it runs.
struct LoadedModel {
    model: Nemotron,
    dir: PathBuf,
    on_gpu: bool,
}

fn load_nemotron(settings: &AppSettings) -> Result<LoadedModel> {
    let choice = settings.model;
    let model_dir = find_model_dir(choice, &settings.model_dir)?;
    log_info(&format!(
        "Loading Nemotron ({} selected, processing: {}) from {}",
        choice.label(),
        settings.compute.label(),
        model_dir.display()
    ));
    check_model_files(&model_dir)?;

    let started = Instant::now();
    let gpu_usable = match settings.compute {
        ComputeChoice::Cpu => false,
        ComputeChoice::Gpu => {
            probe_directml(&ort_environment()?).map_err(|err| {
                anyhow!(
                    "GPU processing was selected, but no usable GPU was found (DirectML: {err}).\n\n\
                     Choose Auto or CPU under Processing in the settings."
                )
            })?;
            true
        }
        ComputeChoice::Auto => match probe_directml(&ort_environment()?) {
            Ok(()) => true,
            Err(err) => {
                log_info(&format!("No usable GPU (DirectML: {err}); using the CPU."));
                false
            }
        },
    };
    let (mut model, on_gpu) = match load_with_device(&model_dir, gpu_usable) {
        Ok(model) => (model, gpu_usable),
        Err(err) if gpu_usable && settings.compute == ComputeChoice::Auto => {
            log_error(&format!("Loading on the GPU failed, retrying on the CPU: {err:#}"));
            (load_with_device(&model_dir, false)?, false)
        }
        Err(err) => return Err(err),
    };

    let mode = match model.mode() {
        NemotronMode::EnglishOnly => "English-only",
        NemotronMode::Multilingual => "Multilingual",
    };
    if model.mode() == NemotronMode::Multilingual {
        apply_language(&mut model, &settings.language)?;
    }
    log_info(&format!(
        "Nemotron loaded ({mode}, on the {}) in {:.1}s",
        if on_gpu { "GPU via DirectML" } else { "CPU" },
        started.elapsed().as_secs_f32()
    ));
    let mismatch = matches!(
        (choice, model.mode()),
        (ModelChoice::English, NemotronMode::Multilingual)
            | (ModelChoice::Multilingual, NemotronMode::EnglishOnly)
    );
    if mismatch {
        log_error(&format!(
            "{} was selected, but {} contains the {mode} model.",
            choice.label(),
            model_dir.display()
        ));
    }

    Ok(LoadedModel {
        model,
        dir: model_dir,
        on_gpu,
    })
}

/// Catch the most common broken-download cases (missing weights file, or a file
/// left truncated by an interrupted download — the script skips files that
/// already exist) with a readable message instead of an opaque ONNX error.
fn check_model_files(model_dir: &std::path::Path) -> Result<()> {
    const MIN_SIZES: &[(&str, u64)] = &[
        ("encoder.onnx", 1_000_000),
        ("encoder.onnx.data", 2_000_000_000),
        ("decoder_joint.onnx", 10_000_000),
        ("tokenizer.model", 100_000),
    ];
    let mut problems = Vec::new();
    for (name, min) in MIN_SIZES {
        let path = model_dir.join(name);
        match fs::metadata(&path) {
            Ok(meta) => {
                log_info(&format!("  {name}: {} bytes", meta.len()));
                if meta.len() < *min {
                    problems.push(format!(
                        "{name} is only {} bytes — the download looks incomplete",
                        meta.len()
                    ));
                }
            }
            Err(_) => problems.push(format!("{name} is missing")),
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "Model files in {} are not usable:\n  - {}\nDelete the affected file(s) and run the download script again.",
            model_dir.display(),
            problems.join("\n  - ")
        ))
    }
}

/// The Predefined folders, in priority order: NEMOTRON_MODEL_DIR, then
/// `model_dir` from settings.json. Empty when neither is set.
fn predefined_model_dirs(configured_dir: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(path) = env::var("NEMOTRON_MODEL_DIR") {
        if !path.trim().is_empty() {
            dirs.push(PathBuf::from(path.trim()));
        }
    }
    if !configured_dir.trim().is_empty() {
        dirs.push(PathBuf::from(configured_dir.trim()));
    }
    dirs
}

/// Whether `choice` can be selected. Only Predefined can be unavailable: it
/// needs NEMOTRON_MODEL_DIR or `model_dir`. Both are fixed while the app runs
/// (settings are read once at startup, and a process keeps its environment), so
/// the UI can grey it out for the whole session.
fn model_choice_available(settings: &AppSettings, choice: ModelChoice) -> bool {
    choice != ModelChoice::Predefined || !predefined_model_dirs(&settings.model_dir).is_empty()
}

/// The model choice for a first start (or an unknown saved value): Predefined
/// when a predefined folder is set, otherwise the installed model (English
/// first). Falls back to English, whose error message then explains the download.
fn detect_model_choice(configured_dir: &str) -> ModelChoice {
    let choice = if !predefined_model_dirs(configured_dir).is_empty() {
        ModelChoice::Predefined
    } else if find_model_dir(ModelChoice::English, configured_dir).is_ok() {
        ModelChoice::English
    } else if find_model_dir(ModelChoice::Multilingual, configured_dir).is_ok() {
        ModelChoice::Multilingual
    } else {
        ModelChoice::English
    };
    log_info(&format!("No speech model chosen yet; selected {}", choice.label()));
    choice
}

fn has_model_files(dir: &std::path::Path) -> bool {
    dir.join("encoder.onnx").exists()
        && dir.join("decoder_joint.onnx").exists()
        && dir.join("tokenizer.model").exists()
}

fn find_model_dir(choice: ModelChoice, configured_dir: &str) -> Result<PathBuf> {
    let Some(folder_name) = choice.folder_name() else {
        // Predefined: only the configured folders, never the standard ones.
        let dirs = predefined_model_dirs(configured_dir);
        if dirs.is_empty() {
            return Err(anyhow!(
                "No predefined model folder is set.\n\nSet the NEMOTRON_MODEL_DIR environment \
                 variable or \"model_dir\" in {}, or choose English or Multilingual in the settings.",
                settings_path().display()
            ));
        }
        return dirs.iter().find(|dir| has_model_files(dir)).cloned().ok_or_else(|| {
            anyhow!(
                "The predefined model folder does not contain the Nemotron ONNX files:\n{}",
                dirs.iter()
                    .map(|p| format!("  - {}", p.display()))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        });
    };

    let mut candidates = Vec::<PathBuf>::new();
    let mut search_roots = Vec::<PathBuf>::new();
    if let Ok(cwd) = env::current_dir() {
        search_roots.push(cwd);
    }
    // Walk up from the executable so the model is found no matter the working
    // directory. This matters for "Start with Windows", where Windows launches us
    // with the working directory set to System32 — and for the cargo layout, where
    // the exe sits in target\release\ but the model is in the project root.
    if let Ok(exe) = env::current_exe() {
        let mut dir = exe.parent();
        for _ in 0..5 {
            let Some(d) = dir else { break };
            search_roots.push(d.to_path_buf());
            dir = d.parent();
        }
    }
    for root in &search_roots {
        let candidate = root.join("models").join(folder_name);
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }

    if let Some(found) = candidates.iter().find(|dir| has_model_files(dir)) {
        return Ok(found.clone());
    }

    let hint = if choice == ModelChoice::Multilingual {
        "The multilingual model is not installed. Download it with \
         scripts\\download-nemotron.ps1 -Multilingual, or pick another model in the settings.\n\n"
    } else {
        "The English model is not installed. Download it with \
         scripts\\download-nemotron.ps1, or pick another model in the settings.\n\n"
    };
    Err(anyhow!(
        "{hint}Expected Nemotron ONNX files in one of these folders:\n{}",
        candidates
            .iter()
            .map(|p| format!("  - {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

fn expected_model_hint() -> String {
    env::current_dir()
        .map(|cwd| cwd.join("models").join("nemotron").display().to_string())
        .unwrap_or_else(|_| "models\\nemotron".to_string())
}

fn start_audio_capture(app: Arc<AppState>) -> Result<cpal::Stream> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("no default microphone found"))?;
    let device_name = device.name().unwrap_or_else(|_| "<unknown>".to_string());
    let input_config = device
        .default_input_config()
        .with_context(|| format!("microphone '{device_name}' has no usable input format"))?;
    let sample_rate = input_config.sample_rate().0 as f64;
    let channels = input_config.channels() as usize;
    let config = input_config.config();
    log_info(&format!(
        "Microphone: '{device_name}', {} Hz, {channels} channel(s), {:?}",
        sample_rate as u32,
        input_config.sample_format()
    ));

    update_ui(&app, |ui| {
        ui.status = format!(
            "Ready. Mic: {} Hz, {} channel(s). Ctrl+Space toggles.",
            sample_rate as u32, channels
        );
    });

    let tx = app.engine_tx.clone();
    let recording = app.recording.clone();
    let err_app = app.clone();
    let err_fn = move |err: cpal::StreamError| {
        log_error(&format!("Microphone stream error: {err}"));
        MIC_FAILED.store(true, Ordering::SeqCst);
        update_ui(&err_app, |ui| {
            ui.status = "Microphone error.".to_string();
            ui.transcript = err.to_string();
        });
    };

    let stream = match input_config.sample_format() {
        cpal::SampleFormat::F32 => {
            let mut callback = InputCallback::new(channels, sample_rate, recording, tx, app.clone());
            device.build_input_stream(
                &config,
                move |data: &[f32], _| callback.push_f32(data),
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let mut callback = InputCallback::new(channels, sample_rate, recording, tx, app.clone());
            device.build_input_stream(
                &config,
                move |data: &[i16], _| callback.push_i16(data),
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::U16 => {
            let mut callback = InputCallback::new(channels, sample_rate, recording, tx, app.clone());
            device.build_input_stream(
                &config,
                move |data: &[u16], _| callback.push_u16(data),
                err_fn,
                None,
            )?
        }
        other => return Err(anyhow!("unsupported microphone sample format: {other:?}")),
    };

    stream
        .play()
        .with_context(|| format!("could not start recording from '{device_name}'"))?;
    Ok(stream)
}

struct InputCallback {
    channels: usize,
    tx: Sender<EngineCommand>,
    recording: Arc<AtomicBool>,
    app: Arc<AppState>,
    was_recording: bool,
    resampler: LinearResampler,
}

impl InputCallback {
    fn new(
        channels: usize,
        source_rate: f64,
        recording: Arc<AtomicBool>,
        tx: Sender<EngineCommand>,
        app: Arc<AppState>,
    ) -> Self {
        Self {
            channels,
            tx,
            recording,
            app,
            was_recording: false,
            resampler: LinearResampler::new(source_rate, NEMOTRON_SAMPLE_RATE),
        }
    }

    fn push_f32(&mut self, data: &[f32]) {
        self.process(data.iter().copied());
    }

    fn push_i16(&mut self, data: &[i16]) {
        self.process(data.iter().map(|s| *s as f32 / 32768.0));
    }

    fn push_u16(&mut self, data: &[u16]) {
        self.process(data.iter().map(|s| (*s as f32 / 65535.0) * 2.0 - 1.0));
    }

    fn process<I>(&mut self, samples: I)
    where
        I: Iterator<Item = f32>,
    {
        let now_recording = self.recording.load(Ordering::Relaxed);
        if !now_recording {
            self.was_recording = false;
            return;
        }

        if !self.was_recording {
            self.resampler.reset();
            self.was_recording = true;
        }

        let mut mono = Vec::new();
        let mut frame = Vec::with_capacity(self.channels);
        for sample in samples {
            frame.push(sample);
            if frame.len() == self.channels {
                let sum = frame.iter().sum::<f32>();
                mono.push(sum / self.channels as f32);
                frame.clear();
            }
        }

        if !mono.is_empty() {
            // Loudness -> bar height. NOISE_FLOOR gates hiss; GAIN sets sensitivity so
            // ordinary speech reaches most of the height; the sub-1.0 exponent lifts
            // quiet syllables so they read as tall bumps instead of tiny specks.
            const NOISE_FLOOR: f32 = 0.006;
            const GAIN: f32 = 14.0;
            const SHAPE: f32 = 0.6;
            let rms = (mono.iter().map(|v| v * v).sum::<f32>() / mono.len() as f32).sqrt();
            let norm = ((rms - NOISE_FLOOR) * GAIN).clamp(0.0, 1.0);
            let level = norm.powf(SHAPE);
            report_level(&self.app, level, rms);
        }

        let mut out = Vec::new();
        self.resampler.push(&mono, &mut out);
        if !out.is_empty() {
            let _ = self.tx.try_send(EngineCommand::Audio(out));
        }
    }
}

struct LinearResampler {
    source_rate: f64,
    target_rate: f64,
    position: f64,
    last_sample: Option<f32>,
}

impl LinearResampler {
    fn new(source_rate: f64, target_rate: f64) -> Self {
        Self {
            source_rate,
            target_rate,
            position: 0.0,
            last_sample: None,
        }
    }

    fn reset(&mut self) {
        self.position = 0.0;
        self.last_sample = None;
    }

    fn push(&mut self, input: &[f32], output: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }

        let mut data = Vec::with_capacity(input.len() + usize::from(self.last_sample.is_some()));
        if let Some(last) = self.last_sample {
            data.push(last);
        }
        data.extend_from_slice(input);

        let step = self.source_rate / self.target_rate;
        while self.position + 1.0 < data.len() as f64 {
            let index = self.position.floor() as usize;
            let frac = (self.position - index as f64) as f32;
            let a = data[index];
            let b = data[index + 1];
            output.push(a + (b - a) * frac);
            self.position += step;
        }

        self.last_sample = data.last().copied();
        self.position -= (data.len() - 1) as f64;
    }
}

fn toggle_recording() {
    let Some(app) = APP.get() else {
        return;
    };

    if app.recording.load(Ordering::SeqCst) {
        stop_recording(app);
    } else {
        start_recording(app);
    }
}

fn start_recording(app: &Arc<AppState>) {
    if !ensure_audio_capture(app) {
        update_ui(app, |ui| {
            ui.recording = false;
            ui.status = "No microphone available. Connect one and try again.".to_string();
        });
        return;
    }
    remember_target_window(app);
    app.recording.store(true, Ordering::SeqCst);
    app.enter_pending.store(false, Ordering::SeqCst);
    let _ = app.engine_tx.send(EngineCommand::Start);
    update_ui(app, |ui| {
        ui.recording = true;
        ui.visible = true;
        ui.status = "Starting...".to_string();
        ui.transcript.clear();
        ui.last_voice = Instant::now(); // reset the silence timer
    });
    unsafe {
        let hwnd = hwnd_from_app(app);
        // While recording, optionally let Enter end the session.
        if settings_snapshot(app).enter_stops_recording {
            register_enter_hotkey(hwnd);
        }
        if hwnd != null_mut() && settings_snapshot(app).show_floating_bubble {
            ShowWindow(hwnd, SW_SHOWNA);
        }
    }
}

fn remember_target_window(app: &Arc<AppState>) {
    unsafe {
        let foreground = GetForegroundWindow();
        let own = hwnd_from_app(app);
        if foreground != null_mut() && foreground != own {
            app.target_hwnd
                .store(foreground as isize, Ordering::SeqCst);
        }
    }
}

fn stop_recording(app: &Arc<AppState>) {
    app.recording.store(false, Ordering::SeqCst);
    unsafe {
        // Release the temporary Enter hotkey before the engine re-injects Enter.
        unregister_enter_hotkey(hwnd_from_app(app));
    }
    play_stop_sound(app);
    let _ = app.engine_tx.send(EngineCommand::Stop);
    update_ui(app, |ui| {
        ui.recording = false;
        ui.status = "Finalizing...".to_string();
    });
}

unsafe fn register_enter_hotkey(hwnd: HWND) {
    if hwnd != null_mut() {
        RegisterHotKey(hwnd, ENTER_HOTKEY_ID, MOD_NOREPEAT, VK_RETURN as u32);
    }
}

unsafe fn unregister_enter_hotkey(hwnd: HWND) {
    if hwnd != null_mut() {
        UnregisterHotKey(hwnd, ENTER_HOTKEY_ID);
    }
}

fn play_start_sound(app: &Arc<AppState>) {
    if !settings_snapshot(app).sounds_enabled {
        return;
    }
    play_sound_file(sound_path("start.wav"));
}

fn play_stop_sound(app: &Arc<AppState>) {
    if !settings_snapshot(app).sounds_enabled {
        return;
    }
    play_sound_file(sound_path("stop.wav"));
}

fn play_sound_file(path: PathBuf) {
    thread::spawn(move || unsafe {
        let wide = wide_null(&path.display().to_string());
        PlaySoundW(wide.as_ptr(), null_mut(), SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
    });
}

fn paste_live_tail(app: &Arc<AppState>, text: &str) {
    if text.is_empty() || !settings_snapshot(app).live_type_into_cursor {
        return;
    }

    let delay_ms = settings_snapshot(app).paste_delay_ms;
    if let Err(err) =
        type_unicode_text(text, hwnd_from_app(app), target_hwnd_from_app(app), delay_ms)
    {
        update_ui(app, |ui| {
            ui.status = format!("Live typing failed: {err}");
        });
    }
}

fn paste_text_to_target(
    text: &str,
    owner_hwnd: HWND,
    target_hwnd: HWND,
    delay_ms: u64,
) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }

    set_clipboard_text(text, owner_hwnd)?;

    unsafe {
        let target = if target_hwnd == null_mut() {
            GetForegroundWindow()
        } else {
            target_hwnd
        };

        if target != null_mut()
            && target != owner_hwnd
            && IsWindow(target) != 0
            && SetForegroundWindow(target) != 0
        {
            thread::sleep(Duration::from_millis(delay_ms));
        }

        send_ctrl_v()
    }
}

fn set_clipboard_text(text: &str, owner_hwnd: HWND) -> Result<()> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * std::mem::size_of::<u16>();

    unsafe {
        if OpenClipboard(owner_hwnd) == 0 {
            return Err(anyhow!("clipboard is busy"));
        }

        EmptyClipboard();
        let handle = GlobalAlloc(GMEM_MOVEABLE, byte_len);
        if handle == null_mut() {
            CloseClipboard();
            return Err(anyhow!("clipboard allocation failed"));
        }

        let locked = GlobalLock(handle) as *mut u16;
        if locked == null_mut() {
            GlobalFree(handle as HGLOBAL);
            CloseClipboard();
            return Err(anyhow!("clipboard lock failed"));
        }

        std::ptr::copy_nonoverlapping(wide.as_ptr(), locked, wide.len());
        GlobalUnlock(handle);

        if SetClipboardData(CF_UNICODETEXT as u32, handle) == null_mut() {
            GlobalFree(handle as HGLOBAL);
            CloseClipboard();
            return Err(anyhow!("clipboard set failed"));
        }

        CloseClipboard();
    }

    Ok(())
}

/// Prepend key-up events for any modifier the user is physically holding, so an
/// injected keystroke isn't folded into a Ctrl/Alt/Shift/Win shortcut (e.g. the
/// Ctrl from the Ctrl+Space hotkey turning a typed char, a paste, or Enter into a
/// command). Shared by every injection path.
unsafe fn push_modifier_releases(inputs: &mut Vec<INPUT>) {
    for vk in [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN] {
        if (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 {
            inputs.push(keyboard_input(vk, KEYEVENTF_KEYUP));
        }
    }
}

unsafe fn send_ctrl_v() -> Result<()> {
    let mut inputs: Vec<INPUT> = Vec::with_capacity(8);
    push_modifier_releases(&mut inputs); // clear stray Shift/Alt/Win before our clean Ctrl+V
    inputs.push(keyboard_input(VK_CONTROL, 0));
    inputs.push(keyboard_input('V' as u16, 0));
    inputs.push(keyboard_input('V' as u16, KEYEVENTF_KEYUP));
    inputs.push(keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP));

    let sent = SendInput(
        inputs.len() as u32,
        inputs.as_ptr(),
        std::mem::size_of::<INPUT>() as i32,
    );

    if sent != inputs.len() as u32 {
        return Err(anyhow!("Ctrl+V injection failed"));
    }

    Ok(())
}

fn keyboard_input(vk: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn unicode_input(unit: u16, extra_flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: unit,
                dwFlags: KEYEVENTF_UNICODE | extra_flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Type text directly as synthesized Unicode keystrokes. Unlike the Ctrl+V path
/// this never touches the clipboard, so live dictation can stream into the cursor
/// without clobbering whatever the user has copied. UTF-16 (incl. surrogate pairs)
/// is sent verbatim.
///
/// Critically, it first releases any modifier the user is physically holding
/// (e.g. the Ctrl from the Ctrl+Space stop hotkey). Otherwise Windows folds the
/// injected characters into Ctrl/Alt+key shortcuts and the focused app fires them
/// (opening tabs, switching apps, etc.).
unsafe fn send_unicode(text: &str) -> Result<()> {
    let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 2 + 4);
    push_modifier_releases(&mut inputs);
    for unit in text.encode_utf16() {
        inputs.push(unicode_input(unit, 0));
        inputs.push(unicode_input(unit, KEYEVENTF_KEYUP));
    }
    if inputs.is_empty() {
        return Ok(());
    }
    let sent = SendInput(
        inputs.len() as u32,
        inputs.as_ptr(),
        std::mem::size_of::<INPUT>() as i32,
    );
    if sent != inputs.len() as u32 {
        return Err(anyhow!("unicode injection failed"));
    }
    Ok(())
}

/// Synthesize a single Enter key press (used by Enter-to-stop to submit the text
/// after the full transcript has been typed).
unsafe fn send_enter() -> Result<()> {
    let mut inputs: Vec<INPUT> = Vec::with_capacity(7);
    push_modifier_releases(&mut inputs); // ensure a clean Enter, not Ctrl/Shift+Enter
    inputs.push(keyboard_input(VK_RETURN, 0));
    inputs.push(keyboard_input(VK_RETURN, KEYEVENTF_KEYUP));
    let sent = SendInput(
        inputs.len() as u32,
        inputs.as_ptr(),
        std::mem::size_of::<INPUT>() as i32,
    );
    if sent != inputs.len() as u32 {
        return Err(anyhow!("Enter injection failed"));
    }
    Ok(())
}

/// Live-type a chunk into the remembered target window without using the clipboard.
/// Only steals focus back to the target if our own window is currently foreground
/// (e.g. recording was started from the tray menu); otherwise it types into whatever
/// already has focus, keeping things snappy with no per-chunk focus flicker.
fn type_unicode_text(
    text: &str,
    owner_hwnd: HWND,
    target_hwnd: HWND,
    delay_ms: u64,
) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }
    unsafe {
        let foreground = GetForegroundWindow();
        // Only grab focus back if our own window currently holds it (e.g. recording
        // was started from the tray). Validate the target still exists first so a
        // stale handle can't redirect keystrokes into the wrong window.
        if (foreground == null_mut() || foreground == owner_hwnd)
            && target_hwnd != null_mut()
            && target_hwnd != owner_hwnd
            && IsWindow(target_hwnd) != 0
            && SetForegroundWindow(target_hwnd) != 0
        {
            thread::sleep(Duration::from_millis(delay_ms));
        }
        send_unicode(text)
    }
}

fn clear_transcript() {
    if let Some(app) = APP.get() {
        update_ui(app, |ui| {
            ui.transcript.clear();
            ui.status = "Cleared. Press Ctrl+Space to start.".to_string();
        });
    }
}

fn toggle_window_visibility() {
    let Some(app) = APP.get() else {
        return;
    };
    unsafe {
        let hwnd = hwnd_from_app(app);
        if hwnd == null_mut() {
            return;
        }
        let visible = {
            let mut ui = lock_ui(app);
            ui.visible = !ui.visible;
            ui.visible
        };
        ShowWindow(hwnd, if visible { SW_SHOWNA } else { SW_HIDE });
    }
}

// --- Global shortcut (customizable hotkey) ---------------------------------

/// (Re)register the global hotkey on `hwnd`. MOD_NOREPEAT stops held keys from
/// firing repeatedly. Returns false if the combo is reserved / already taken.
fn register_global_hotkey(hwnd: HWND, mods: u32, vk: u32) -> bool {
    if hwnd == null_mut() {
        return false;
    }
    unsafe {
        UnregisterHotKey(hwnd, HOTKEY_ID);
        RegisterHotKey(hwnd, HOTKEY_ID, mods | MOD_NOREPEAT, vk) != 0
    }
}

fn key_is_down(vk: u16) -> bool {
    unsafe { (GetKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

/// Localised name of a single key ("Space", "F8", "A", ...).
fn key_name(vk: u32) -> String {
    // Windows names Space in the system language ("LEER" on German systems),
    // which reads oddly next to the English "Ctrl"/"Alt"; name it ourselves.
    if vk == VK_SPACE as u32 {
        return "Space".to_string();
    }
    unsafe {
        let scan = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC);
        if scan != 0 {
            let lparam = (scan << 16) as i32;
            let mut buf = [0u16; 64];
            let len = GetKeyNameTextW(lparam, buf.as_mut_ptr(), buf.len() as i32);
            if len > 0 {
                return String::from_utf16_lossy(&buf[..len as usize]);
            }
        }
    }
    format!("Key {vk}")
}

/// Human-readable shortcut label like "Ctrl + Shift + Space".
fn hotkey_label(mods: u32, vk: u32) -> String {
    let mut parts: Vec<String> = Vec::new();
    if mods & MOD_CONTROL != 0 {
        parts.push("Ctrl".to_string());
    }
    if mods & MOD_ALT != 0 {
        parts.push("Alt".to_string());
    }
    if mods & MOD_SHIFT != 0 {
        parts.push("Shift".to_string());
    }
    if mods & MOD_WIN != 0 {
        parts.push("Win".to_string());
    }
    parts.push(key_name(vk));
    parts.join(" + ")
}

/// Enter capture mode: free the current hotkey so the next chord arrives as
/// WM_KEYDOWN, and pull keyboard focus to the settings window.
fn start_hotkey_capture(app: &Arc<AppState>) {
    app.capturing_hotkey.store(true, Ordering::SeqCst);
    unsafe {
        let bubble = hwnd_from_app(app);
        if bubble != null_mut() {
            UnregisterHotKey(bubble, HOTKEY_ID);
        }
        let settings = settings_hwnd_from_app(app);
        if settings != null_mut() {
            SetFocus(settings);
        }
    }
    update_ui(app, |ui| {
        ui.status = "Press your shortcut...  (Esc to cancel)".to_string();
    });
}

fn cancel_hotkey_capture(app: &Arc<AppState>) {
    if !app.capturing_hotkey.swap(false, Ordering::SeqCst) {
        return;
    }
    let s = settings_snapshot(app);
    register_global_hotkey(hwnd_from_app(app), s.hotkey_mods, s.hotkey_vk);
    update_ui(app, |ui| {
        ui.status = "Shortcut unchanged.".to_string();
    });
}

fn apply_captured_hotkey(app: &Arc<AppState>, mods: u32, vk: u32) {
    app.capturing_hotkey.store(false, Ordering::SeqCst);
    let bubble = hwnd_from_app(app);
    if register_global_hotkey(bubble, mods, vk) {
        {
            let mut s = app
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            s.hotkey_mods = mods;
            s.hotkey_vk = vk;
        }
        save_settings(&settings_snapshot(app));
        let label = hotkey_label(mods, vk);
        update_ui(app, |ui| {
            ui.status = format!("Shortcut set to {label}.");
        });
    } else {
        // Roll back to the previous (still-working) binding.
        let s = settings_snapshot(app);
        register_global_hotkey(bubble, s.hotkey_mods, s.hotkey_vk);
        update_ui(app, |ui| {
            ui.status = "That combo is unavailable - try another.".to_string();
        });
    }
}

/// Handle a key press while capturing a new shortcut.
fn handle_capture_key(app: &Arc<AppState>, vk: u32) {
    let v = vk as u16;
    if v == VK_CONTROL || v == VK_SHIFT || v == VK_MENU || v == VK_LWIN || v == VK_RWIN {
        return; // a bare modifier — keep waiting for the main key
    }
    if v == VK_ESCAPE {
        cancel_hotkey_capture(app);
        return;
    }
    let mut mods = 0u32;
    if key_is_down(VK_CONTROL) {
        mods |= MOD_CONTROL;
    }
    if key_is_down(VK_MENU) {
        mods |= MOD_ALT;
    }
    if key_is_down(VK_SHIFT) {
        mods |= MOD_SHIFT;
    }
    if key_is_down(VK_LWIN) || key_is_down(VK_RWIN) {
        mods |= MOD_WIN;
    }
    // Letters and digits would hijack normal typing without a modifier.
    let needs_mod = (0x30..=0x5A).contains(&vk);
    if mods == 0 && needs_mod {
        update_ui(app, |ui| {
            ui.status = "Add Ctrl, Alt, or Shift to that key.".to_string();
        });
        return; // stay in capture mode
    }
    apply_captured_hotkey(app, mods, vk);
}

fn handle_command(command: usize) {
    match command {
        IDM_START_STOP => toggle_recording(),
        IDM_SHOW_HIDE => toggle_window_visibility(),
        IDM_CLEAR => clear_transcript(),
        id if (IDM_MODEL_BASE..IDM_MODEL_BASE + ModelChoice::ALL.len()).contains(&id) => {
            if let Some(app) = APP.get() {
                select_model(app, ModelChoice::ALL[id - IDM_MODEL_BASE]);
            }
        }
        id if (IDM_LANG_BASE..IDM_LANG_BASE + LANGUAGES.len()).contains(&id) => {
            if let Some(app) = APP.get() {
                select_language(app, LANGUAGES[id - IDM_LANG_BASE].0);
            }
        }
        id if (IDM_BUBBLE_POS_BASE..IDM_BUBBLE_POS_BASE + BubblePosition::ALL.len()).contains(&id) => {
            if let Some(app) = APP.get() {
                select_bubble_position(app, BubblePosition::ALL[id - IDM_BUBBLE_POS_BASE]);
            }
        }
        id if (IDM_COMPUTE_BASE..IDM_COMPUTE_BASE + ComputeChoice::ALL.len()).contains(&id) => {
            if let Some(app) = APP.get() {
                select_compute(app, ComputeChoice::ALL[id - IDM_COMPUTE_BASE]);
            }
        }
        IDM_QUIT => unsafe {
            let Some(app) = APP.get() else {
                return;
            };
            let hwnd = hwnd_from_app(app);
            if hwnd != null_mut() {
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
            }
        },
        _ => {}
    }
}

fn handle_click(x: i32, y: i32) {
    let _ = (x, y);
    let Some(app) = APP.get() else {
        return;
    };
    if settings_snapshot(app).bubble_click_opens_settings {
        toggle_settings_window(SettingsOrigin::Bubble);
    } else {
        toggle_recording();
    }
}

fn should_start_drag(hwnd: HWND, x: i32, y: i32) -> bool {
    if is_settings_window(hwnd) {
        y < 84 && !settings_close_rect().contains(x, y)
    } else {
        true
    }
}

fn begin_drag(hwnd: HWND) {
    let Some(app) = APP.get() else {
        return;
    };
    unsafe {
        let mut point: POINT = std::mem::zeroed();
        let mut rect: RECT = std::mem::zeroed();
        if GetCursorPos(&mut point) == 0 || GetWindowRect(hwnd, &mut rect) == 0 {
            return;
        }
        SetCapture(hwnd);
        let mut drag = app
            .drag
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *drag = Some(DragState {
            hwnd: hwnd as isize,
            start_cursor: point,
            start_rect: rect,
            moved: false,
        });
    }
}

fn continue_drag() {
    let Some(app) = APP.get() else {
        return;
    };
    let mut drag = app
        .drag
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(state) = drag.as_mut() else {
        return;
    };

    unsafe {
        let mut point: POINT = std::mem::zeroed();
        if GetCursorPos(&mut point) == 0 {
            return;
        }
        let dx = point.x - state.start_cursor.x;
        let dy = point.y - state.start_cursor.y;
        if dx.abs() > 3 || dy.abs() > 3 {
            state.moved = true;
        }
        SetWindowPos(
            state.hwnd as HWND,
            HWND_TOPMOST,
            state.start_rect.left + dx,
            state.start_rect.top + dy,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOSIZE,
        );
    }
}

fn end_drag() -> bool {
    let Some(app) = APP.get() else {
        return true;
    };
    unsafe {
        ReleaseCapture();
    }
    let mut drag = app
        .drag
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let was_click = drag.as_ref().map(|state| !state.moved).unwrap_or(true);
    *drag = None;
    was_click
}

fn handle_settings_click(x: i32, y: i32) {
    let Some(app) = APP.get() else {
        return;
    };

    let tab = app.settings_tab.load(Ordering::SeqCst);

    // Shortcut rebind button: click to begin capture, click again to cancel.
    if tab == TAB_SETTINGS && settings_shortcut_rect().contains(x, y) {
        if app.capturing_hotkey.load(Ordering::SeqCst) {
            cancel_hotkey_capture(app);
        } else {
            start_hotkey_capture(app);
        }
        return;
    }
    // Any other click ends an in-progress capture before doing its thing.
    if app.capturing_hotkey.load(Ordering::SeqCst) {
        cancel_hotkey_capture(app);
    }

    if settings_close_rect().contains(x, y) {
        unsafe {
            let hwnd = settings_hwnd_from_app(app);
            if hwnd != null_mut() {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
        return;
    }

    for index in 0..SETTINGS_TABS.len() {
        if settings_tab_rect(index).contains(x, y) {
            set_settings_tab(app, index);
            return;
        }
    }

    match tab {
        TAB_HISTORY => handle_history_tab_click(app, x, y),
        TAB_MODEL => handle_model_tab_click(app, x, y),
        _ => {
            for id in SETTINGS_TAB_TOGGLES {
                if settings_toggle_rect(id).contains(x, y) {
                    toggle_setting(app, id);
                    return;
                }
            }
            if settings_bubble_position_rect().contains(x, y) {
                unsafe { show_bubble_position_menu(app) };
            }
        }
    }
}

fn handle_history_tab_click(app: &Arc<AppState>, x: i32, y: i32) {
    if settings_clear_history_rect().contains(x, y) {
        clear_history(app);
        return;
    }
    // Copy icons, as laid out by the last paint (they move with scrolling).
    let hit = app
        .history_hits
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .find(|(rect, _)| rect.contains(x, y))
        .map(|(_, index)| *index);
    if let Some(index) = hit {
        copy_history_item(app, index);
    }
}

fn handle_model_tab_click(app: &Arc<AppState>, x: i32, y: i32) {
    for choice in ModelChoice::ALL {
        if settings_model_rect(choice).contains(x, y) {
            select_model(app, choice);
            return;
        }
    }

    for choice in ComputeChoice::ALL {
        if settings_compute_rect(choice).contains(x, y) {
            select_compute(app, choice);
            return;
        }
    }

    if settings_language_rect().contains(x, y) {
        if language_selectable(&settings_snapshot(app)) {
            unsafe { show_language_menu(app) };
        } else {
            update_ui(app, |ui| {
                ui.status = "The language applies to the multilingual model only.".to_string();
            });
        }
        return;
    }

    if settings_toggle_rect(SETTINGS_TOGGLE_PRELOAD).contains(x, y) {
        toggle_setting(app, SETTINGS_TOGGLE_PRELOAD);
    }
}

fn update_ui<F>(app: &Arc<AppState>, update: F)
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
        let settings_hwnd = settings_hwnd_from_app(app);
        if settings_hwnd != null_mut() {
            PostMessageW(settings_hwnd, WM_UI_UPDATE, 0, 0);
        }
    }
}

/// Called from the audio thread with the latest instantaneous bar height (0..1).
/// Smooths it with a fast attack / slow release so the meter feels responsive but
/// never jittery, then stashes it for the UI-thread animation timer to sample.
/// Does no GDI work — drawing only happens on the UI thread.
fn report_level(app: &Arc<AppState>, target: f32, rms: f32) {
    let target = target.clamp(0.0, 1.0);
    let mut ui = lock_ui(app);
    let prev = ui.current_level;
    let smoothed = if target > prev {
        prev * 0.4 + target * 0.6
    } else {
        prev * 0.78 + target * 0.22
    };
    ui.current_level = smoothed.clamp(0.0, 1.0);
    if rms > VOICE_RMS_THRESHOLD {
        ui.last_voice = Instant::now();
    }
}

/// UI-thread tick: scroll the rolling waveform buffer by one sample of the current
/// level. When idle the level decays smoothly to zero so the bars settle instead of
/// freezing. Returns the peak bar height so callers can decide whether to keep
/// animating. Runs at the timer rate, which decouples scroll speed from the mic
/// callback rate for a steady, even flow.
fn advance_waveform(app: &Arc<AppState>) -> f32 {
    let mut ui = lock_ui(app);
    if !ui.recording {
        ui.current_level *= 0.80;
        if ui.current_level < 0.001 {
            ui.current_level = 0.0;
        }
    }
    let level = ui.current_level;
    ui.waveform.push(level);
    while ui.waveform.len() > WAVE_BARS {
        ui.waveform.remove(0);
    }
    ui.waveform.iter().copied().fold(level, f32::max)
}

fn lock_ui(app: &Arc<AppState>) -> std::sync::MutexGuard<'_, UiState> {
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

struct Canvas {
    w: i32,
    h: i32,
    hdc: HDC,
    bitmap: HBITMAP,
    old_bitmap: isize,
    bits: *mut u32,
}

impl Canvas {
    unsafe fn new(w: i32, h: i32) -> Option<Canvas> {
        if w <= 0 || h <= 0 {
            return None;
        }
        let screen = GetDC(null_mut());
        let hdc = CreateCompatibleDC(screen);
        ReleaseDC(null_mut(), screen);
        if hdc == null_mut() {
            return None;
        }
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h; // negative => top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB as u32;
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if bitmap == null_mut() || bits == null_mut() {
            DeleteDC(hdc);
            return None;
        }
        let old_bitmap = SelectObject(hdc, bitmap as _) as isize;
        Some(Canvas {
            w,
            h,
            hdc,
            bitmap,
            old_bitmap,
            bits: bits as *mut u32,
        })
    }

    fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h) as usize) }
    }

    fn clear_transparent(&mut self) {
        for p in self.pixels() {
            *p = 0;
        }
    }

    /// Push the buffer to a per-pixel-alpha layered window. Position is left
    /// unchanged (so dragging via SetWindowPos keeps working).
    unsafe fn present_layered(&self, hwnd: HWND) {
        GdiFlush();
        let screen = GetDC(null_mut());
        let size = SIZE {
            cx: self.w,
            cy: self.h,
        };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        UpdateLayeredWindow(
            hwnd,
            screen,
            null(),
            &size,
            self.hdc,
            &src,
            0,
            &blend,
            ULW_ALPHA,
        );
        if screen != null_mut() {
            ReleaseDC(null_mut(), screen);
        }
    }

    /// Copy the buffer to a destination DC (opaque double-buffer blit).
    unsafe fn blit_to(&self, dst: HDC) {
        GdiFlush();
        BitBlt(dst, 0, 0, self.w, self.h, self.hdc, 0, 0, SRCCOPY);
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.hdc, self.old_bitmap as _);
            DeleteObject(self.bitmap as _);
            DeleteDC(self.hdc);
        }
    }
}

type Rgb = (u8, u8, u8);

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Premultiplied source-over blend of a straight-alpha colour onto one pixel.
#[inline]
fn blend_px(buf: &mut [u32], idx: usize, r: f32, g: f32, b: f32, a: f32) {
    if a <= 0.0 {
        return;
    }
    let a = a.min(1.0);
    let d = buf[idx];
    let da = ((d >> 24) & 0xff) as f32;
    let dr = ((d >> 16) & 0xff) as f32;
    let dg = ((d >> 8) & 0xff) as f32;
    let db = (d & 0xff) as f32;
    let inv = 1.0 - a;
    let or = r * a + dr * inv;
    let og = g * a + dg * inv;
    let ob = b * a + db * inv;
    let oa = a * 255.0 + da * inv;
    buf[idx] = ((oa as u32) << 24) | ((or as u32) << 16) | ((og as u32) << 8) | (ob as u32);
}

/// Signed distance from a point to a rounded rectangle (negative inside).
#[inline]
fn sd_round_rect(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hx - r);
    let qy = (py - cy).abs() - (hy - r);
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    (ax * ax + ay * ay).sqrt() + qx.max(qy).min(0.0) - r
}

/// Anti-aliased rounded rectangle with an optional vertical colour gradient.
#[allow(clippy::too_many_arguments)]
fn fill_round_rect(
    buf: &mut [u32],
    cw: i32,
    ch: i32,
    x: f32,
    y: f32,
    rw: f32,
    rh: f32,
    radius: f32,
    top: Rgb,
    bot: Rgb,
    alpha: f32,
) {
    if rw <= 0.0 || rh <= 0.0 {
        return;
    }
    let cx = x + rw / 2.0;
    let cy = y + rh / 2.0;
    let hx = rw / 2.0;
    let hy = rh / 2.0;
    let r = radius.min(hx).min(hy).max(0.0);
    let x0 = (x.floor() as i32 - 1).max(0);
    let y0 = (y.floor() as i32 - 1).max(0);
    let x1 = ((x + rw).ceil() as i32 + 1).min(cw);
    let y1 = ((y + rh).ceil() as i32 + 1).min(ch);
    for py in y0..y1 {
        let fy = py as f32 + 0.5;
        let t = ((fy - y) / rh).clamp(0.0, 1.0);
        let cr = lerp(top.0 as f32, bot.0 as f32, t);
        let cg = lerp(top.1 as f32, bot.1 as f32, t);
        let cb = lerp(top.2 as f32, bot.2 as f32, t);
        let row = (py * cw) as usize;
        for px in x0..x1 {
            let fx = px as f32 + 0.5;
            let d = sd_round_rect(fx, fy, cx, cy, hx, hy, r);
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend_px(buf, row + px as usize, cr, cg, cb, cov * alpha);
            }
        }
    }
}

/// Soft drop shadow: a rounded-rect silhouette whose alpha falls off over `blur`.
#[allow(clippy::too_many_arguments)]
fn fill_shadow(
    buf: &mut [u32],
    cw: i32,
    ch: i32,
    x: f32,
    y: f32,
    rw: f32,
    rh: f32,
    radius: f32,
    blur: f32,
    max_alpha: f32,
) {
    let cx = x + rw / 2.0;
    let cy = y + rh / 2.0;
    let hx = rw / 2.0;
    let hy = rh / 2.0;
    let r = radius.min(hx).min(hy).max(0.0);
    let x0 = ((x - blur).floor() as i32).max(0);
    let y0 = ((y - blur).floor() as i32).max(0);
    let x1 = ((x + rw + blur).ceil() as i32).min(cw);
    let y1 = ((y + rh + blur).ceil() as i32).min(ch);
    for py in y0..y1 {
        let fy = py as f32 + 0.5;
        let row = (py * cw) as usize;
        for px in x0..x1 {
            let fx = px as f32 + 0.5;
            let d = sd_round_rect(fx, fy, cx, cy, hx, hy, r).max(0.0);
            let cov = (1.0 - d / blur).clamp(0.0, 1.0);
            if cov > 0.0 {
                // ease the falloff so the shadow looks soft, not linear
                let a = cov * cov * max_alpha;
                blend_px(buf, row + px as usize, 0.0, 0.0, 0.0, a);
            }
        }
    }
}

/// Render the floating bubble into a layered window: drop shadow, glassy pill,
/// status dot, and the mirrored anti-aliased waveform.
unsafe fn render_bubble(hwnd: HWND) {
    if hwnd == null_mut() {
        return;
    }
    let Some(app) = APP.get() else {
        return;
    };
    let (waveform, recording, wave_enabled) = {
        let ui = lock_ui(app);
        let settings = settings_snapshot(app);
        (ui.waveform.clone(), ui.recording, settings.waveform_enabled)
    };

    let Some(mut canvas) = Canvas::new(WIDTH, HEIGHT) else {
        return;
    };
    canvas.clear_transparent();
    {
        let buf = canvas.pixels();
        paint_bubble_into(buf, WIDTH, HEIGHT, 0.0, 0.0, &waveform, recording, wave_enabled);
    }
    canvas.present_layered(hwnd);
}

/// Draw the bubble (shadow, glassy pill, status dot, waveform) into a premultiplied
/// pixel buffer. The bubble's margin box is placed at (ox, oy) so it can be composited
/// anywhere — used both by the live layered window and the asset exporter.
#[allow(clippy::too_many_arguments)]
fn paint_bubble_into(
    buf: &mut [u32],
    cw: i32,
    ch: i32,
    ox: f32,
    oy: f32,
    waveform: &[f32],
    recording: bool,
    wave_enabled: bool,
) {
    let px = ox + BUBBLE_MARGIN as f32;
    let py = oy + BUBBLE_MARGIN as f32;
    let pw = PILL_W as f32;
    let ph = PILL_H as f32;
    let radius = ph / 2.0;

    // soft drop shadow, nudged down a touch; it must stay within BUBBLE_MARGIN
    // (sides: 1 + blur = 9 px, bottom: 2 + blur = 10 px) or it gets clipped
    fill_shadow(buf, cw, ch, px - 1.0, py + 1.0, pw + 2.0, ph + 1.0, radius, 8.0, 0.45);

    // glassy pill body with a subtle top-to-bottom gradient
    fill_round_rect(buf, cw, ch, px, py, pw, ph, radius, (46, 51, 62), (26, 29, 36), 0.98);
    // faint inner highlight along the top edge
    fill_round_rect(
        buf,
        cw,
        ch,
        px + 2.0,
        py + 1.5,
        pw - 4.0,
        ph * 0.5,
        radius,
        (255, 255, 255),
        (255, 255, 255),
        0.05,
    );

    // status dot on the left
    let dot_r = 4.5;
    let dot_cx = px + 15.0;
    let dot_cy = py + ph / 2.0;
    let (dot_col, dot_a): (Rgb, f32) = if recording {
        ((255, 92, 92), 0.95)
    } else {
        ((104, 112, 126), 0.7)
    };
    fill_round_rect(
        buf,
        cw,
        ch,
        dot_cx - dot_r,
        dot_cy - dot_r,
        dot_r * 2.0,
        dot_r * 2.0,
        dot_r,
        dot_col,
        dot_col,
        dot_a,
    );

    // waveform fills the remainder of the pill
    let active = recording && wave_enabled;
    draw_wave_bars(buf, cw, ch, waveform, active, dot_cx + dot_r, px + pw, py, ph);
}

/// Mirrored, rounded, anti-aliased waveform bars centred in the pill.
#[allow(clippy::too_many_arguments)]
fn draw_wave_bars(
    buf: &mut [u32],
    cw: i32,
    ch: i32,
    wave: &[f32],
    active: bool,
    left_edge: f32,
    pill_right: f32,
    py: f32,
    ph: f32,
) {
    let n = wave.len().max(1);
    let area_left = left_edge + 6.0;
    let area_right = pill_right - 12.0;
    let area_w = (area_right - area_left).max(1.0);
    let pitch = area_w / n as f32;
    let bw = (pitch * 0.55).clamp(2.0, 4.0);
    let cy = py + ph / 2.0;
    let max_half = ph / 2.0 - 8.0;
    let min_half = 1.3;
    let (top, bot): (Rgb, Rgb) = if active {
        ((150, 192, 255), (86, 148, 250))
    } else {
        ((78, 86, 100), (60, 66, 78))
    };
    let alpha = if active { 0.97 } else { 0.5 };
    for (i, &lvl) in wave.iter().enumerate() {
        let v = if active { lvl.clamp(0.0, 1.0) } else { 0.0 };
        let half = (v * max_half).max(min_half);
        let cx = area_left + pitch * (i as f32 + 0.5);
        let x = cx - bw / 2.0;
        fill_round_rect(buf, cw, ch, x, cy - half, bw, half * 2.0, bw / 2.0, top, bot, alpha);
    }
}

unsafe fn paint(hwnd: HWND) {
    // Bubble content is delivered by UpdateLayeredWindow, not GDI. Validate the
    // paint cycle and push a fresh frame so it stays correct after restores.
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    BeginPaint(hwnd, &mut ps);
    EndPaint(hwnd, &ps);
    render_bubble(hwnd);
}

static ANIM_TICK: AtomicUsize = AtomicUsize::new(0);
static TRAY_ANIMATING: AtomicBool = AtomicBool::new(false);

/// 30 fps UI-thread heartbeat: scroll the waveform, repaint the bubble when there
/// is motion, and pulse the tray icon while recording. Keeps all GDI on the UI
/// thread; the audio thread only feeds levels.
unsafe fn on_anim_tick(hwnd: HWND) {
    let Some(app) = APP.get() else {
        return;
    };
    let peak = advance_waveform(app);
    let settings = settings_snapshot(app);
    let (recording, silent_for) = {
        let ui = lock_ui(app);
        (ui.recording, ui.last_voice.elapsed())
    };

    // Auto-stop after a stretch of silence so a forgotten recording cannot run
    // forever (and let the model's audio buffer grow without bound). Optional.
    if recording
        && settings.silence_auto_stop
        && silent_for >= Duration::from_secs(SILENCE_AUTO_STOP_SECS)
    {
        stop_recording(app);
        update_ui(app, |ui| {
            ui.status = format!("Auto-stopped after {SILENCE_AUTO_STOP_SECS}s of silence.");
        });
        return;
    }

    if settings.show_floating_bubble && IsWindowVisible(hwnd) != 0 && (recording || peak > 0.004) {
        render_bubble(hwnd);
    }

    let tick = ANIM_TICK.fetch_add(1, Ordering::Relaxed);
    if recording && settings.tray_waveform_enabled {
        if tick % 3 == 0 {
            let level = lock_ui(app).current_level;
            update_tray_icon(hwnd, make_tray_icon(level, true));
        }
        TRAY_ANIMATING.store(true, Ordering::Relaxed);
    } else if TRAY_ANIMATING.swap(false, Ordering::Relaxed) {
        // Recording just ended (or animation was switched off): settle to idle.
        update_tray_icon(hwnd, make_tray_icon(0.0, false));
    }
}

struct SettingRow {
    id: usize,
    title: &'static str,
    detail: &'static str,
    enabled: bool,
}

fn setting_rows(settings: &AppSettings) -> Vec<SettingRow> {
    vec![
        SettingRow {
            id: SETTINGS_TOGGLE_STARTUP,
            title: "Start with Windows",
            detail: "Launch automatically after sign-in.",
            enabled: settings.start_with_windows,
        },
        SettingRow {
            id: SETTINGS_TOGGLE_PRELOAD,
            title: "Preload Nemotron",
            detail: "Use RAM so the first dictation starts faster.",
            enabled: settings.preload_model,
        },
        SettingRow {
            id: SETTINGS_TOGGLE_LIVE_TYPE,
            title: "Type live into cursor",
            detail: "As you speak. Text fields only - off is safer.",
            enabled: settings.live_type_into_cursor,
        },
        SettingRow {
            id: SETTINGS_TOGGLE_FINAL_CLIPBOARD,
            title: "Copy final to clipboard",
            detail: "Copy the full transcript once on stop.",
            enabled: settings.copy_final_to_clipboard,
        },
        SettingRow {
            id: SETTINGS_TOGGLE_FINAL_PASTE,
            title: "Paste final on stop",
            detail: "Off by default to avoid duplicates.",
            enabled: settings.paste_final_on_stop,
        },
        SettingRow {
            id: SETTINGS_TOGGLE_SOUNDS,
            title: "Soft sound cues",
            detail: "Short low-volume start and stop chimes.",
            enabled: settings.sounds_enabled,
        },
        SettingRow {
            id: SETTINGS_TOGGLE_WAVEFORM,
            title: "Waveform animation",
            detail: "Show live mic levels while recording.",
            enabled: settings.waveform_enabled,
        },
        SettingRow {
            id: SETTINGS_TOGGLE_BUBBLE_CLICK_SETTINGS,
            title: "Bubble click opens settings",
            detail: "Off makes a bubble click toggle recording.",
            enabled: settings.bubble_click_opens_settings,
        },
        SettingRow {
            id: SETTINGS_TOGGLE_FLOATING_BUBBLE,
            title: "Show floating bubble",
            detail: "Off keeps control in the tray only.",
            enabled: settings.show_floating_bubble,
        },
        SettingRow {
            id: SETTINGS_TOGGLE_TRAY_WAVEFORM,
            title: "Animate the tray icon",
            detail: "Pulse the taskbar tray icon while speaking.",
            enabled: settings.tray_waveform_enabled,
        },
        SettingRow {
            id: SETTINGS_TOGGLE_ENTER_STOPS,
            title: "Enter stops recording",
            detail: "Press Enter to finish and submit the text.",
            enabled: settings.enter_stops_recording,
        },
        SettingRow {
            id: SETTINGS_TOGGLE_SILENCE_STOP,
            title: "Auto-stop on silence",
            detail: "Stop after 15s with no speech. Off to pause freely.",
            enabled: settings.silence_auto_stop,
        },
    ]
}

unsafe fn paint_settings(hwnd: HWND) {
    let Some(app) = APP.get() else {
        return;
    };
    let settings = settings_snapshot(app);

    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc == null_mut() {
        return;
    }
    let capturing = app.capturing_hotkey.load(Ordering::SeqCst);
    let model_status = lock_ui(app).model_status.clone();
    if let Some(mut canvas) = Canvas::new(SETTINGS_WIDTH, SETTINGS_HEIGHT) {
        let tab = app.settings_tab.load(Ordering::SeqCst);
        let items = history_snapshot(app);
        let view = HistoryView {
            items: &items,
            scroll: app.history_scroll.load(Ordering::SeqCst),
            copied: *app.copied_flash.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
        };
        let layout = render_settings_canvas(&mut canvas, &settings, &model_status, &view, capturing, tab);
        canvas.blit_to(hdc);
        // Remember what the list looked like, for clicks and wheel scrolling.
        app.history_content_h.store(layout.content_h, Ordering::SeqCst);
        if layout.scroll != view.scroll {
            app.history_scroll.store(layout.scroll, Ordering::SeqCst);
        }
        *app.history_hits.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = layout.hits;
    }
    EndPaint(hwnd, &ps);
}

/// What the History tab shows: the entries, how far the list is scrolled, and
/// which entry's copy icon currently shows the "copied" checkmark.
struct HistoryView<'a> {
    items: &'a [HistoryItem],
    scroll: i32,
    copied: Option<usize>,
}

/// The History list as painted: copy-icon hit areas (rect, entry index), the
/// total content height and the scroll offset actually used (clamped).
#[derive(Default)]
struct HistoryLayout {
    hits: Vec<(RectI, usize)>,
    content_h: i32,
    scroll: i32,
}

/// Vertical layout of the history entries in list coordinates (0 = top of the
/// list content): (entry top, wrapped text height), plus the total height.
unsafe fn measure_history(dc: HDC, items: &[HistoryItem]) -> (Vec<(i32, i32)>, i32) {
    SelectObject(dc, ui_font() as _);
    let mut entries = Vec::with_capacity(items.len());
    let mut y = 0;
    for item in items {
        let mut rect = RECT {
            left: HISTORY_TEXT_LEFT,
            top: 0,
            right: HISTORY_TEXT_RIGHT,
            bottom: 0,
        };
        let wide = wide_null(item.text.trim());
        DrawTextW(
            dc,
            wide.as_ptr(),
            -1,
            &mut rect,
            DT_CALCRECT | DT_LEFT | DT_WORDBREAK | DT_EDITCONTROL | DT_NOPREFIX,
        );
        let text_h = (rect.bottom - rect.top).max(19);
        entries.push((y, text_h));
        y += HISTORY_HEADER_H + 2 + text_h + HISTORY_ENTRY_GAP;
    }
    (entries, (y - HISTORY_ENTRY_GAP).max(0))
}

/// Render the entire settings surface into a canvas: header, tab bar and the card
/// of the active `tab` with its switches, buttons and ClearType text. Returns the
/// layout of the History list (copy-icon hit areas, content height).
unsafe fn render_settings_canvas(
    canvas: &mut Canvas,
    settings: &AppSettings,
    model_status: &str,
    history: &HistoryView,
    capturing: bool,
    tab: usize,
) -> HistoryLayout {
    let rows = setting_rows(settings);
    let cw = canvas.w;
    let ch = canvas.h;
    let card_left = SETTINGS_CARD_LEFT as f32;
    let card_top = SETTINGS_CARD_TOP as f32;
    let card_w = (cw - 2 * SETTINGS_CARD_LEFT) as f32;
    let card_h = (ch - SETTINGS_CARD_TOP - SETTINGS_FOOTER_H) as f32;

    // History list geometry: measure first (needs the DC), then draw.
    let view_h = HISTORY_LIST_BOTTOM - HISTORY_LIST_TOP;
    let (entries, content_h) = if tab == TAB_HISTORY {
        measure_history(canvas.hdc, history.items)
    } else {
        (Vec::new(), 0)
    };
    let scroll = history.scroll.clamp(0, (content_h - view_h).max(0));
    let mut layout = HistoryLayout {
        hits: Vec::new(),
        content_h,
        scroll,
    };

    // -------- pass 1: anti-aliased shapes straight into the pixel buffer --------
    {
        let buf = canvas.pixels();
        fill_round_rect(buf, cw, ch, 0.0, 0.0, cw as f32, ch as f32, 0.0, (25, 27, 33), (16, 17, 21), 1.0);
        fill_round_rect(buf, cw, ch, 28.0, 26.0, 4.0, 32.0, 2.0, (110, 162, 255), (84, 138, 250), 1.0);

        // tab bar: thin baseline, accent underline below the active tab
        let base_y = SETTINGS_TAB_BOTTOM as f32;
        fill_round_rect(buf, cw, ch, card_left, base_y - 1.0, card_w, 1.0, 0.0, (44, 48, 58), (44, 48, 58), 1.0);
        let active = settings_tab_rect(tab);
        let underline_w = 56.0;
        let underline_x = (active.left + active.right) as f32 / 2.0 - underline_w / 2.0;
        fill_round_rect(buf, cw, ch, underline_x, base_y - 3.0, underline_w, 3.0, 1.5, (110, 162, 255), (84, 138, 250), 1.0);

        // the card of the active tab
        fill_round_rect(buf, cw, ch, card_left, card_top, card_w, card_h, 16.0, (30, 33, 41), (25, 28, 35), 1.0);

        match tab {
            TAB_HISTORY => {
                draw_button_bg(buf, cw, ch, settings_clear_history_rect(), false);
                // thin scrollbar, only when the list is longer than the view
                if content_h > view_h {
                    let track_x = (SETTINGS_INNER_RIGHT + 4) as f32;
                    let track_top = HISTORY_LIST_TOP as f32;
                    let track_h = view_h as f32;
                    let thumb_h = (track_h * view_h as f32 / content_h as f32).max(24.0);
                    let thumb_top = track_top
                        + (track_h - thumb_h) * scroll as f32 / (content_h - view_h) as f32;
                    fill_round_rect(buf, cw, ch, track_x, track_top, 4.0, track_h, 2.0, (40, 44, 54), (40, 44, 54), 1.0);
                    fill_round_rect(buf, cw, ch, track_x, thumb_top, 4.0, thumb_h, 2.0, (92, 100, 116), (92, 100, 116), 1.0);
                }
            }
            TAB_MODEL => {
                draw_segment_track(buf, cw, ch, settings_compute_track_rect());
                draw_button_bg(buf, cw, ch, settings_compute_rect(settings.compute), true);
                draw_divider(buf, cw, ch, MODEL_SECTION_TOP - 12);
                draw_segment_track(buf, cw, ch, settings_model_track_rect());
                draw_button_bg(buf, cw, ch, settings_model_rect(settings.model), true);
                draw_divider(buf, cw, ch, LANGUAGE_ROW_TOP - 12);
                draw_button_bg(buf, cw, ch, settings_language_rect(), false);
                draw_divider(buf, cw, ch, PRELOAD_ROW_TOP - 8);
                draw_toggle(buf, cw, ch, settings_toggle_rect(SETTINGS_TOGGLE_PRELOAD), settings.preload_model);
            }
            _ => {
                // shortcut rebind button (accent while capturing)
                draw_button_bg(buf, cw, ch, settings_shortcut_rect(), capturing);
                for row in rows.iter().filter(|row| row.id != SETTINGS_TOGGLE_PRELOAD) {
                    draw_toggle(buf, cw, ch, settings_toggle_rect(row.id), row.enabled);
                }
                draw_button_bg(buf, cw, ch, settings_bubble_position_rect(), false);
            }
        }
    }

    // -------- pass 2: ClearType text on top via GDI --------
    let dc = canvas.hdc;
    SetBkMode(dc, TRANSPARENT as i32);

    SelectObject(dc, title_font() as _);
    SetTextColor(dc, rgb(237, 241, 247));
    text_left(dc, "Nemotron Bubble", 42, 22, cw - 110, 34);

    // close: the ChromeClose glyph of the Windows icon font, like a title bar's X
    SelectObject(dc, icon_font() as _);
    SetTextColor(dc, rgb(205, 212, 224));
    text_center(dc, GLYPH_CLOSE, settings_close_rect());

    // tab labels
    SelectObject(dc, ui_font() as _);
    for (index, label) in SETTINGS_TABS.iter().enumerate() {
        if index == tab {
            SetTextColor(dc, rgb(245, 248, 252));
        } else {
            SetTextColor(dc, rgb(140, 150, 164));
        }
        let rect = settings_tab_rect(index);
        text_center(dc, label, RectI { bottom: rect.bottom - 4, ..rect });
    }

    match tab {
        TAB_HISTORY => {
            // title row: "Recent dictations" left, trash button right
            SelectObject(dc, ui_font() as _);
            SetTextColor(dc, rgb(233, 238, 244));
            text_left(dc, "Recent dictations", SETTINGS_INNER_LEFT, HISTORY_TITLE_TOP, 220, 20);
            SelectObject(dc, icon_font_large() as _);
            SetTextColor(dc, rgb(214, 220, 230));
            text_center(dc, GLYPH_TRASH, settings_clear_history_rect());

            if history.items.is_empty() {
                SelectObject(dc, small_font() as _);
                SetTextColor(dc, rgb(126, 136, 150));
                text_left(dc, "No dictations yet.", HISTORY_TEXT_LEFT, HISTORY_LIST_TOP, 260, 18);
            }

            // entries, clipped to the list area and shifted by the scroll offset
            IntersectClipRect(dc, SETTINGS_CARD_LEFT, HISTORY_LIST_TOP, SETTINGS_INNER_RIGHT, HISTORY_LIST_BOTTOM);
            let text_w = HISTORY_TEXT_RIGHT - HISTORY_TEXT_LEFT;
            for (index, (item, (top, text_h))) in history.items.iter().zip(&entries).enumerate() {
                let y = HISTORY_LIST_TOP + top - scroll;
                if y + HISTORY_HEADER_H + 2 + text_h < HISTORY_LIST_TOP || y > HISTORY_LIST_BOTTOM {
                    continue;
                }

                // header: copy icon (checkmark right after copying), then the grey info line
                let icon = RectI {
                    left: HISTORY_TEXT_LEFT - 3,
                    top: y - 1,
                    right: HISTORY_TEXT_LEFT + 15,
                    bottom: y + HISTORY_HEADER_H + 1,
                };
                SelectObject(dc, icon_font() as _);
                if history.copied == Some(index) {
                    SetTextColor(dc, rgb(110, 200, 140));
                    text_center(dc, GLYPH_CHECK, icon);
                } else {
                    SetTextColor(dc, rgb(150, 160, 176));
                    text_center(dc, GLYPH_COPY, icon);
                }
                // clickable while at least partly visible (the first icon starts 1 px above the list)
                if icon.bottom > HISTORY_LIST_TOP && icon.top < HISTORY_LIST_BOTTOM {
                    layout.hits.push((icon, index));
                }
                SelectObject(dc, tiny_font() as _);
                SetTextColor(dc, rgb(126, 136, 150));
                text_left(dc, &item.header(), HISTORY_TEXT_LEFT + 20, y + 1, text_w - 20, HISTORY_HEADER_H);

                // the dictated text, bright and word-wrapped
                SelectObject(dc, ui_font() as _);
                SetTextColor(dc, rgb(226, 231, 239));
                let mut rect = RECT {
                    left: HISTORY_TEXT_LEFT,
                    top: y + HISTORY_HEADER_H + 2,
                    right: HISTORY_TEXT_RIGHT,
                    bottom: y + HISTORY_HEADER_H + 2 + text_h,
                };
                let wide = wide_null(item.text.trim());
                DrawTextW(
                    dc,
                    wide.as_ptr(),
                    -1,
                    &mut rect,
                    DT_LEFT | DT_TOP | DT_WORDBREAK | DT_EDITCONTROL | DT_NOPREFIX,
                );
            }
            SelectClipRgn(dc, null_mut());
        }
        TAB_MODEL => {
            // Processing (first: decides how the model is loaded)
            section_heading(
                dc,
                COMPUTE_SECTION_TOP,
                "Processing",
                "Where the model runs. Auto tries the GPU, then the CPU.",
            );
            SelectObject(dc, ui_font() as _);
            for choice in ComputeChoice::ALL {
                if choice == settings.compute {
                    SetTextColor(dc, rgb(245, 248, 252));
                } else {
                    SetTextColor(dc, rgb(184, 192, 205));
                }
                text_center(dc, choice.label(), settings_compute_rect(choice));
            }

            // Speech model (detail line: what is loaded, and where)
            section_heading(dc, MODEL_SECTION_TOP, "Speech model", model_status);
            SelectObject(dc, ui_font() as _);
            for choice in ModelChoice::ALL {
                if choice == settings.model {
                    SetTextColor(dc, rgb(245, 248, 252));
                } else if !model_choice_available(settings, choice) {
                    SetTextColor(dc, rgb(92, 100, 114)); // greyed out: nothing predefined
                } else {
                    SetTextColor(dc, rgb(184, 192, 205));
                }
                text_center(dc, choice.label(), settings_model_rect(choice));
            }

            // Language: label + detail on the left, popup button on the right
            SelectObject(dc, ui_font() as _);
            SetTextColor(dc, rgb(233, 238, 244));
            text_left(dc, "Language", SETTINGS_INNER_LEFT, LANGUAGE_ROW_TOP + 1, 150, 20);
            SelectObject(dc, small_font() as _);
            SetTextColor(dc, rgb(124, 134, 148));
            text_left(dc, "Multilingual model only.", SETTINGS_INNER_LEFT, LANGUAGE_ROW_TOP + 21, 160, 18);
            SelectObject(dc, ui_font() as _);
            if language_selectable(settings) {
                SetTextColor(dc, rgb(245, 248, 252));
            } else {
                SetTextColor(dc, rgb(92, 100, 114));
            }
            text_center(
                dc,
                &format!("{}  \u{25BE}", language_label(&settings.language)),
                settings_language_rect(),
            );

            // Preload toggle
            if let Some(row) = rows.iter().find(|row| row.id == SETTINGS_TOGGLE_PRELOAD) {
                draw_toggle_text(dc, row);
            }
        }
        _ => {
            // shortcut row
            SelectObject(dc, ui_font() as _);
            SetTextColor(dc, rgb(233, 238, 244));
            text_left(dc, "Global shortcut", 40, SETTINGS_CARD_TOP + 13, 160, 20);
            SelectObject(dc, small_font() as _);
            SetTextColor(dc, rgb(124, 134, 148));
            text_left(dc, "Click, then press your keys.", 40, SETTINGS_CARD_TOP + 33, 160, 18);
            SelectObject(dc, ui_font() as _);
            SetTextColor(dc, rgb(245, 248, 252));
            let shortcut_label = if capturing {
                "Press keys...".to_string()
            } else {
                hotkey_label(settings.hotkey_mods, settings.hotkey_vk)
            };
            text_center(dc, &shortcut_label, settings_shortcut_rect());

            for row in rows.iter().filter(|row| row.id != SETTINGS_TOGGLE_PRELOAD) {
                draw_toggle_text(dc, row);
            }

            // bubble position: label + detail on the left, popup button on the right
            SelectObject(dc, ui_font() as _);
            SetTextColor(dc, rgb(233, 238, 244));
            text_left(dc, "Bubble position", 40, BUBBLE_POSITION_ROW_TOP + 1, 160, 20);
            SelectObject(dc, small_font() as _);
            SetTextColor(dc, rgb(124, 134, 148));
            text_left(dc, "Preset spot on the screen.", 40, BUBBLE_POSITION_ROW_TOP + 21, 160, 18);
            SelectObject(dc, ui_font() as _);
            SetTextColor(dc, rgb(245, 248, 252));
            text_center(
                dc,
                &format!("{}  \u{25BE}", settings.bubble_position.label()),
                settings_bubble_position_rect(),
            );
        }
    }

    // version footer, bottom right below the card
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(108, 118, 132));
    text_right(
        dc,
        &format!("Version {APP_VERSION}"),
        RectI {
            left: cw - 236,
            top: ch - SETTINGS_FOOTER_H + 10,
            right: cw - 34,
            bottom: ch - SETTINGS_FOOTER_H + 28,
        },
    );

    layout
}

/// Title and detail line of a Model-tab section (its control sits below them).
unsafe fn section_heading(dc: HDC, top: i32, title: &str, detail: &str) {
    let width = SETTINGS_INNER_RIGHT - SETTINGS_INNER_LEFT;
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(233, 238, 244));
    text_left(dc, title, SETTINGS_INNER_LEFT, top, width, 20);
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(124, 134, 148));
    text_left(dc, detail, SETTINGS_INNER_LEFT, top + 20, width, 18);
}

/// Title and detail text of a toggle row (the switch itself is drawn in pass 1).
unsafe fn draw_toggle_text(dc: HDC, row: &SettingRow) {
    let rect = settings_toggle_rect(row.id);
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(233, 238, 244));
    text_left(dc, row.title, rect.left + 8, rect.top + 1, 232, 20);
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(124, 134, 148));
    text_left(dc, row.detail, rect.left + 8, rect.top + 21, 248, 18);
}

/// Dark rounded track behind a segmented control.
fn draw_segment_track(buf: &mut [u32], cw: i32, ch: i32, track: RectI) {
    fill_round_rect(
        buf,
        cw,
        ch,
        track.left as f32,
        track.top as f32,
        (track.right - track.left) as f32,
        (track.bottom - track.top) as f32,
        13.0,
        (40, 44, 54),
        (36, 40, 49),
        1.0,
    );
}

/// Thin horizontal line between sections of a tab.
fn draw_divider(buf: &mut [u32], cw: i32, ch: i32, y: i32) {
    let width = (SETTINGS_INNER_RIGHT - SETTINGS_INNER_LEFT) as f32;
    fill_round_rect(
        buf,
        cw,
        ch,
        SETTINGS_INNER_LEFT as f32,
        y as f32,
        width,
        1.0,
        0.0,
        (44, 48, 58),
        (44, 48, 58),
        1.0,
    );
}

/// Anti-aliased toggle switch (track + knob) inside a settings row.
fn draw_toggle(buf: &mut [u32], cw: i32, ch: i32, row: RectI, enabled: bool) {
    let w = 46.0;
    let h = 24.0;
    let x = (row.right - 50) as f32;
    let y = (row.top + 10) as f32;
    let (track_top, track_bot): (Rgb, Rgb) = if enabled {
        ((92, 152, 252), (58, 118, 240))
    } else {
        ((66, 72, 84), (52, 57, 67))
    };
    fill_round_rect(buf, cw, ch, x, y, w, h, h / 2.0, track_top, track_bot, 1.0);

    let kr = 9.0;
    let ky = y + h / 2.0;
    let kx = if enabled { x + w - kr - 3.0 } else { x + kr + 3.0 };
    // subtle knob shadow then the knob
    fill_round_rect(buf, cw, ch, kx - kr, ky - kr + 1.0, kr * 2.0, kr * 2.0, kr, (0, 0, 0), (0, 0, 0), 0.20);
    fill_round_rect(buf, cw, ch, kx - kr, ky - kr, kr * 2.0, kr * 2.0, kr, (255, 255, 255), (236, 240, 247), 1.0);
}

/// Anti-aliased button background. Primary buttons get the accent gradient.
fn draw_button_bg(buf: &mut [u32], cw: i32, ch: i32, rect: RectI, primary: bool) {
    let x = rect.left as f32;
    let y = rect.top as f32;
    let w = (rect.right - rect.left) as f32;
    let h = (rect.bottom - rect.top) as f32;
    let (top, bot): (Rgb, Rgb) = if primary {
        ((98, 152, 250), (62, 116, 238))
    } else {
        ((60, 66, 78), (47, 52, 62))
    };
    fill_round_rect(buf, cw, ch, x, y, w, h, 13.0, top, bot, 1.0);
}

unsafe fn text_left(dc: HDC, text: &str, x: i32, y: i32, w: i32, h: i32) {
    let mut rect = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    let wide = wide_null(text);
    DrawTextW(
        dc,
        wide.as_ptr(),
        -1,
        &mut rect,
        DT_LEFT | DT_TOP | DT_WORDBREAK | DT_NOPREFIX,
    );
}

unsafe fn text_right(dc: HDC, text: &str, rect: RectI) {
    let mut r = RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    };
    let wide = wide_null(text);
    DrawTextW(dc, wide.as_ptr(), -1, &mut r, DT_RIGHT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX);
}

unsafe fn text_center(dc: HDC, text: &str, rect: RectI) {
    let mut r = RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    };
    let wide = wide_null(text);
    DrawTextW(
        dc,
        wide.as_ptr(),
        -1,
        &mut r,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
}

/// The work area (screen minus taskbar) of the monitor containing `point`, or
/// the primary monitor's for None.
unsafe fn work_area(point: Option<POINT>) -> RECT {
    let monitor = match point {
        Some(point) => MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST),
        None => MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY),
    };
    let mut info: MONITORINFO = std::mem::zeroed();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if monitor != null_mut() && GetMonitorInfoW(monitor, &mut info) != 0 {
        info.rcWork
    } else {
        RECT {
            left: 0,
            top: 0,
            right: GetSystemMetrics(SM_CXSCREEN),
            bottom: GetSystemMetrics(SM_CYSCREEN),
        }
    }
}

/// Move the floating bubble to its preset position on the primary monitor.
unsafe fn place_window(hwnd: HWND) {
    let position = APP
        .get()
        .map(|app| settings_snapshot(app).bubble_position)
        .unwrap_or_default();
    let work = work_area(None);
    // Margin between the visible pill and the screen edge / taskbar. The window
    // is BUBBLE_MARGIN larger than the pill on every side (transparent shadow
    // room), so the window itself sits that much closer; with PILL_GAP equal to
    // BUBBLE_MARGIN the window edge lands exactly on the work-area edge.
    const PILL_GAP: i32 = 10;
    const GAP: i32 = PILL_GAP - BUBBLE_MARGIN;
    let x = match position {
        BubblePosition::TopLeft | BubblePosition::BottomLeft => work.left + GAP,
        BubblePosition::TopCenter | BubblePosition::BottomCenter => {
            work.left + (work.right - work.left - WIDTH) / 2
        }
        BubblePosition::TopRight | BubblePosition::BottomRight => work.right - WIDTH - GAP,
    };
    let y = match position {
        BubblePosition::TopLeft | BubblePosition::TopCenter | BubblePosition::TopRight => work.top + GAP,
        _ => work.bottom - HEIGHT - GAP,
    };
    // Position only — visibility is controlled separately so the first frame is
    // rendered (via UpdateLayeredWindow) before the window is shown.
    SetWindowPos(hwnd, HWND_TOPMOST, x, y, WIDTH, HEIGHT, SWP_NOACTIVATE);
}

/// A zero-size rectangle at the mouse cursor.
unsafe fn cursor_rect() -> RECT {
    let mut point: POINT = std::mem::zeroed();
    GetCursorPos(&mut point);
    RECT {
        left: point.x,
        top: point.y,
        right: point.x,
        bottom: point.y,
    }
}

/// What opened the settings window; decides where it appears.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsOrigin {
    /// A click on the floating bubble: open next to the bubble.
    Bubble,
    /// A click on the tray icon: open next to the tray (at the cursor).
    Tray,
}

/// Position the settings window next to what opened it, kept inside that
/// monitor's work area: above the anchor when it is in the lower half of the
/// screen (bubble at the bottom, taskbar at the bottom), otherwise below it.
unsafe fn place_settings_window(hwnd: HWND, origin: SettingsOrigin) {
    const GAP: i32 = 8;
    // Anchor rectangle: the bubble's pill, or a point at the cursor (tray click).
    let anchor = match origin {
        SettingsOrigin::Bubble => {
            let bubble = APP.get().map(|app| hwnd_from_app(app)).unwrap_or(null_mut());
            let mut rect: RECT = std::mem::zeroed();
            if bubble != null_mut() && IsWindowVisible(bubble) != 0 && GetWindowRect(bubble, &mut rect) != 0 {
                // Ignore the transparent shadow margin around the pill.
                RECT {
                    left: rect.left + BUBBLE_MARGIN,
                    top: rect.top + BUBBLE_MARGIN,
                    right: rect.right - BUBBLE_MARGIN,
                    bottom: rect.bottom - BUBBLE_MARGIN,
                }
            } else {
                cursor_rect()
            }
        }
        SettingsOrigin::Tray => cursor_rect(),
    };
    let center = POINT {
        x: (anchor.left + anchor.right) / 2,
        y: (anchor.top + anchor.bottom) / 2,
    };
    let work = work_area(Some(center));

    let x = (center.x - SETTINGS_WIDTH / 2)
        .min(work.right - SETTINGS_WIDTH - GAP)
        .max(work.left + GAP);
    let in_lower_half = center.y > (work.top + work.bottom) / 2;
    let y = if in_lower_half {
        anchor.top - SETTINGS_HEIGHT - GAP
    } else {
        anchor.bottom + GAP
    }
    .min(work.bottom - SETTINGS_HEIGHT - GAP)
    .max(work.top + GAP);

    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        x,
        y,
        SETTINGS_WIDTH,
        SETTINGS_HEIGHT,
        SWP_NOACTIVATE,
    );
}

unsafe fn apply_settings_rounding(hwnd: HWND) {
    let region = CreateRoundRectRgn(0, 0, SETTINGS_WIDTH + 1, SETTINGS_HEIGHT + 1, 18, 18);
    if region != null_mut() {
        SetWindowRgn(hwnd, region, 1);
    }
}

/// Open the settings window if hidden, close it if already showing.
fn toggle_settings_window(origin: SettingsOrigin) {
    let Some(app) = APP.get() else {
        return;
    };
    unsafe {
        let hwnd = settings_hwnd_from_app(app);
        if hwnd == null_mut() {
            return;
        }
        if IsWindowVisible(hwnd) != 0 {
            cancel_hotkey_capture(app);
            ShowWindow(hwnd, SW_HIDE);
        } else {
            place_settings_window(hwnd, origin);
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
    }
}

fn is_settings_window(hwnd: HWND) -> bool {
    APP.get()
        .map(|app| hwnd == settings_hwnd_from_app(app))
        .unwrap_or(false)
}

unsafe fn add_tray_icon(hwnd: HWND) {
    // Track only the icon we own. If creation fails, fall back to the shared system
    // icon for display but store 0 so it is never passed to DestroyIcon.
    let icon = make_tray_icon(0.0, false);
    let shown = if icon == null_mut() {
        LoadIconW(null_mut(), IDI_APPLICATION)
    } else {
        icon
    };
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ID;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAYICON;
    nid.hIcon = shown;
    fill_wide_fixed(&mut nid.szTip, &format!("Nemotron Bubble {APP_VERSION}"));
    Shell_NotifyIconW(NIM_ADD, &mut nid);
    CURRENT_TRAY_ICON.store(icon as isize, Ordering::SeqCst);
}

unsafe fn remove_tray_icon(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ID;
    Shell_NotifyIconW(NIM_DELETE, &mut nid);
    let prev = CURRENT_TRAY_ICON.swap(0, Ordering::SeqCst);
    if prev != 0 {
        DestroyIcon(prev as HICON);
    }
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    if menu == null_mut() {
        return;
    }

    let recording = APP
        .get()
        .map(|app| app.recording.load(Ordering::SeqCst))
        .unwrap_or(false);

    let start_text = wide_null(if recording { "Stop" } else { "Start" });
    let show_text = wide_null("Show / Hide Bubble");
    let clear_text = wide_null("Clear");
    let quit_text = wide_null("Quit");
    let model_text = wide_null("Speech model");
    let version_text = wide_null(&format!("Nemotron Bubble {APP_VERSION}"));

    // "Speech model" submenu, current choice checked; locked while dictating.
    let menu_settings = APP.get().map(settings_snapshot).unwrap_or_default();
    let current_model = menu_settings.model;
    let model_menu = CreatePopupMenu();
    let model_labels: Vec<Vec<u16>> = ModelChoice::ALL
        .iter()
        .map(|choice| wide_null(choice.label()))
        .collect();
    if model_menu != null_mut() {
        for (index, choice) in ModelChoice::ALL.iter().enumerate() {
            let mut flags = MF_STRING;
            if *choice == current_model {
                flags |= MF_CHECKED;
            }
            if recording || !model_choice_available(&menu_settings, *choice) {
                flags |= MF_GRAYED;
            }
            AppendMenuW(model_menu, flags, IDM_MODEL_BASE + index, model_labels[index].as_ptr());
        }
    }

    AppendMenuW(menu, MF_STRING, IDM_START_STOP, start_text.as_ptr());
    AppendMenuW(menu, MF_STRING, IDM_SHOW_HIDE, show_text.as_ptr());
    AppendMenuW(menu, MF_STRING, IDM_CLEAR, clear_text.as_ptr());
    if model_menu != null_mut() {
        // DestroyMenu(menu) below also destroys this attached submenu.
        AppendMenuW(menu, MF_POPUP, model_menu as usize, model_text.as_ptr());
    }
    // "Processing" submenu: Auto / GPU / CPU, current one checked.
    let compute_menu = CreatePopupMenu();
    let compute_text = wide_null(&format!("Processing: {}", menu_settings.compute.label()));
    if compute_menu != null_mut() {
        for (index, choice) in ComputeChoice::ALL.iter().enumerate() {
            let mut flags = MF_STRING;
            if *choice == menu_settings.compute {
                flags |= MF_CHECKED;
            }
            if recording {
                flags |= MF_GRAYED;
            }
            let label = wide_null(choice.label());
            AppendMenuW(compute_menu, flags, IDM_COMPUTE_BASE + index, label.as_ptr());
        }
    }
    let language_menu = build_language_menu(&menu_settings, recording);
    let language_text = wide_null(&format!(
        "Language: {}",
        language_label(&menu_settings.language)
    ));
    if language_menu != null_mut() {
        let flags = if language_selectable(&menu_settings) {
            MF_POPUP
        } else {
            MF_POPUP | MF_GRAYED
        };
        AppendMenuW(menu, flags, language_menu as usize, language_text.as_ptr());
    }
    if compute_menu != null_mut() {
        AppendMenuW(menu, MF_POPUP, compute_menu as usize, compute_text.as_ptr());
    }
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, IDM_QUIT, quit_text.as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, version_text.as_ptr());

    let mut point: POINT = std::mem::zeroed();
    GetCursorPos(&mut point);
    SetForegroundWindow(hwnd);
    TrackPopupMenu(menu, TPM_RIGHTBUTTON, point.x, point.y, 0, hwnd, null());
    DestroyMenu(menu);
}

/// The close "X" in the top-right corner of the header.
fn settings_close_rect() -> RectI {
    RectI {
        left: SETTINGS_WIDTH - 54,
        top: 18,
        right: SETTINGS_WIDTH - 14,
        bottom: 50,
    }
}

/// One tab of the tab bar (History | Model | Settings), spanning the card width.
fn settings_tab_rect(tab: usize) -> RectI {
    let width = (SETTINGS_WIDTH - 2 * SETTINGS_CARD_LEFT) / SETTINGS_TABS.len() as i32;
    let left = SETTINGS_CARD_LEFT + tab as i32 * width;
    RectI {
        left,
        top: SETTINGS_TAB_TOP,
        right: left + width,
        bottom: SETTINGS_TAB_BOTTOM,
    }
}

/// The trash button at the top right of the History tab (clears the history).
fn settings_clear_history_rect() -> RectI {
    RectI {
        left: SETTINGS_INNER_RIGHT - 32,
        top: HISTORY_TITLE_TOP - 6,
        right: SETTINGS_INNER_RIGHT,
        bottom: HISTORY_TITLE_TOP + 26,
    }
}

/// A full-width segmented-control track below a section's title and detail line.
fn section_track_rect(section_top: i32) -> RectI {
    RectI {
        left: SETTINGS_INNER_LEFT,
        top: section_top + 44,
        right: SETTINGS_INNER_RIGHT,
        bottom: section_top + 76,
    }
}

/// Segment `index` of `count` equal segments within `track`.
fn segment_rect(track: RectI, index: usize, count: usize) -> RectI {
    let width = (track.right - track.left) / count as i32;
    let left = track.left + index as i32 * width;
    let right = if index + 1 == count { track.right } else { left + width };
    RectI {
        left,
        top: track.top,
        right,
        bottom: track.bottom,
    }
}

/// Background track of the Processing (CPU / GPU / Auto) segmented control.
fn settings_compute_track_rect() -> RectI {
    section_track_rect(COMPUTE_SECTION_TOP)
}

/// One segment of the Processing control.
fn settings_compute_rect(choice: ComputeChoice) -> RectI {
    let index = ComputeChoice::ALL.iter().position(|c| *c == choice).unwrap_or(0);
    segment_rect(settings_compute_track_rect(), index, ComputeChoice::ALL.len())
}

/// Background track of the speech-model segmented control.
fn settings_model_track_rect() -> RectI {
    section_track_rect(MODEL_SECTION_TOP)
}

/// One segment (English / Multilingual / Predefined) of the speech-model control.
fn settings_model_rect(choice: ModelChoice) -> RectI {
    let index = ModelChoice::ALL.iter().position(|c| *c == choice).unwrap_or(0);
    segment_rect(settings_model_track_rect(), index, ModelChoice::ALL.len())
}

/// The Language button (opens the language popup menu), right of its label.
fn settings_language_rect() -> RectI {
    RectI {
        left: SETTINGS_INNER_RIGHT - 140,
        top: LANGUAGE_ROW_TOP + 4,
        right: SETTINGS_INNER_RIGHT,
        bottom: LANGUAGE_ROW_TOP + 36,
    }
}

/// The clickable shortcut-rebind button at the top of the Settings tab.
fn settings_shortcut_rect() -> RectI {
    RectI {
        left: SETTINGS_INNER_RIGHT - 150,
        top: SETTINGS_CARD_TOP + 12,
        right: SETTINGS_INNER_RIGHT,
        bottom: SETTINGS_CARD_TOP + 46,
    }
}

/// The toggles of the Settings tab, top to bottom. The bubble settings come
/// last, followed by the bubble position row.
const SETTINGS_TAB_TOGGLES: [usize; 11] = [
    SETTINGS_TOGGLE_STARTUP,
    SETTINGS_TOGGLE_LIVE_TYPE,
    SETTINGS_TOGGLE_FINAL_CLIPBOARD,
    SETTINGS_TOGGLE_FINAL_PASTE,
    SETTINGS_TOGGLE_SOUNDS,
    SETTINGS_TOGGLE_WAVEFORM,
    SETTINGS_TOGGLE_TRAY_WAVEFORM,
    SETTINGS_TOGGLE_ENTER_STOPS,
    SETTINGS_TOGGLE_SILENCE_STOP,
    SETTINGS_TOGGLE_FLOATING_BUBBLE,
    SETTINGS_TOGGLE_BUBBLE_CLICK_SETTINGS,
];

/// The bubble position button (opens the preset popup menu), right of its label.
fn settings_bubble_position_rect() -> RectI {
    RectI {
        left: SETTINGS_INNER_RIGHT - 150,
        top: BUBBLE_POSITION_ROW_TOP + 6,
        right: SETTINGS_INNER_RIGHT,
        bottom: BUBBLE_POSITION_ROW_TOP + 38,
    }
}

/// The row of a toggle setting: Preload sits on the Model tab, all others on
/// the Settings tab below the shortcut row.
fn settings_toggle_rect(id: usize) -> RectI {
    let top = if id == SETTINGS_TOGGLE_PRELOAD {
        PRELOAD_ROW_TOP
    } else {
        let index = SETTINGS_TAB_TOGGLES.iter().position(|t| *t == id).unwrap_or(0);
        SETTINGS_TAB_TOGGLES_TOP + index as i32 * 49
    };
    RectI {
        left: 32,
        top,
        right: SETTINGS_WIDTH - 36,
        bottom: top + 44,
    }
}

/// Switch the settings window to `tab`.
fn set_settings_tab(app: &Arc<AppState>, tab: usize) {
    app.settings_tab.store(tab, Ordering::SeqCst);
    unsafe {
        let hwnd = settings_hwnd_from_app(app);
        if hwnd != null_mut() {
            InvalidateRect(hwnd, null(), 0);
        }
    }
}

fn wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn create_font(height: i32, weight: i32) -> isize {
    create_font_face("Segoe UI", height, weight)
}

unsafe fn create_font_face(face: &str, height: i32, weight: i32) -> isize {
    let face = wide_null(face);
    CreateFontW(
        height,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32,
        CLEARTYPE_QUALITY as u32,
        (DEFAULT_PITCH | FF_DONTCARE) as u32,
        face.as_ptr(),
    ) as isize
}

/// Windows' icon font (Windows 10 and 11), for the title-bar style close glyph.
fn icon_font() -> isize {
    *ICON_FONT.get_or_init(|| unsafe { create_font_face("Segoe MDL2 Assets", -11, FW_NORMAL as i32) })
}

fn ui_font() -> isize {
    *UI_FONT.get_or_init(|| unsafe { create_font(-15, FW_NORMAL as i32) })
}

fn title_font() -> isize {
    *TITLE_FONT.get_or_init(|| unsafe { create_font(-25, FW_SEMIBOLD as i32) })
}

fn small_font() -> isize {
    *SMALL_FONT.get_or_init(|| unsafe { create_font(-12, FW_NORMAL as i32) })
}

/// Header lines of history entries (date, device, time per chunk).
fn tiny_font() -> isize {
    *TINY_FONT.get_or_init(|| unsafe { create_font(-11, FW_NORMAL as i32) })
}

/// Larger icon-font glyphs, e.g. the trash bin.
fn icon_font_large() -> isize {
    *ICON_FONT_LARGE.get_or_init(|| unsafe { create_font_face("Segoe MDL2 Assets", -15, FW_NORMAL as i32) })
}

/// Build a 32x32 tray icon. While recording the equalizer bars scale with `level`
/// and a small red dot appears; idle shows a static dim glyph. The caller owns the
/// returned HICON and must DestroyIcon it when replaced.
unsafe fn make_tray_icon(level: f32, recording: bool) -> HICON {
    const SIZE: i32 = 32;
    let mut pixels = vec![0u32; (SIZE * SIZE) as usize];
    let center = (SIZE as f32 - 1.0) / 2.0;
    let bg = if recording { 0xFF20242E } else { 0xFF1A1D21 };
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            if (dx * dx + dy * dy).sqrt() <= 14.5 {
                pixels[(y * SIZE + x) as usize] = bg;
            }
        }
    }

    // Equalizer bars. The shape array gives a pleasant resting silhouette; while
    // recording it is modulated by the live level so the icon "breathes".
    let shape = [0.30f32, 0.58, 0.86, 1.0, 0.72, 0.46, 0.66, 0.9, 0.5];
    let lvl = level.clamp(0.0, 1.0);
    let accent = if recording { 0xFF7AB8FF } else { 0xFF59616F };
    for (i, &s) in shape.iter().enumerate() {
        let frac = if recording {
            (0.18 + s * lvl * 0.95).min(1.0)
        } else {
            s * 0.5
        };
        let bar_h = (frac * 22.0) as i32;
        let x = 7 + i as i32 * 2;
        let top = 16 - bar_h / 2;
        let bottom = 16 + bar_h / 2;
        for yy in top..=bottom {
            for xx in x..=x + 1 {
                if (0..SIZE).contains(&xx) && (0..SIZE).contains(&yy) {
                    pixels[(yy * SIZE + xx) as usize] = accent;
                }
            }
        }
    }

    if recording {
        for y in 3..9 {
            for x in 23..29 {
                let dx = x as f32 - 25.5;
                let dy = y as f32 - 5.5;
                if dx * dx + dy * dy <= 6.0 {
                    pixels[(y * SIZE + x) as usize] = 0xFFFF5C5C;
                }
            }
        }
    }

    let color = CreateBitmap(SIZE, SIZE, 1, 32, pixels.as_ptr() as _);
    let mask_bits = vec![0u8; ((SIZE * SIZE) / 8) as usize];
    let mask = CreateBitmap(SIZE, SIZE, 1, 1, mask_bits.as_ptr() as _);
    let info = ICONINFO {
        fIcon: 1,
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask,
        hbmColor: color,
    };
    let icon = CreateIconIndirect(&info);
    if color != null_mut() {
        DeleteObject(color as _);
    }
    if mask != null_mut() {
        DeleteObject(mask as _);
    }
    // Return null on failure (never a shared system icon): update_tray_icon only ever
    // DestroyIcon's handles it owns, so a system icon must never enter that path.
    icon
}

/// Swap the live tray icon, destroying the one it replaces.
unsafe fn update_tray_icon(hwnd: HWND, icon: HICON) {
    if hwnd == null_mut() || icon == null_mut() {
        return;
    }
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ID;
    nid.uFlags = NIF_ICON;
    nid.hIcon = icon;
    Shell_NotifyIconW(NIM_MODIFY, &mut nid);
    let prev = CURRENT_TRAY_ICON.swap(icon as isize, Ordering::SeqCst);
    if prev != 0 && prev != icon as isize {
        DestroyIcon(prev as HICON);
    }
}

/// Reset the tray icon to its idle glyph.
fn refresh_tray_idle(app: &Arc<AppState>) {
    unsafe {
        let hwnd = hwnd_from_app(app);
        if hwnd != null_mut() {
            update_tray_icon(hwnd, make_tray_icon(0.0, false));
        }
    }
}

fn rgb(red: u8, green: u8, blue: u8) -> u32 {
    red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
}

fn fill_wide_fixed<const N: usize>(target: &mut [u16; N], text: &str) {
    target.fill(0);
    for (slot, value) in target
        .iter_mut()
        .take(N.saturating_sub(1))
        .zip(text.encode_utf16())
    {
        *slot = value;
    }
}

fn lparam_point(lparam: LPARAM) -> (i32, i32) {
    let x = (lparam & 0xffff) as i16 as i32;
    let y = ((lparam >> 16) & 0xffff) as i16 as i32;
    (x, y)
}

fn loword(value: usize) -> u16 {
    (value & 0xffff) as u16
}

fn hwnd_from_app(app: &Arc<AppState>) -> HWND {
    app.hwnd.load(Ordering::SeqCst) as HWND
}

fn target_hwnd_from_app(app: &Arc<AppState>) -> HWND {
    app.target_hwnd.load(Ordering::SeqCst) as HWND
}

fn settings_hwnd_from_app(app: &Arc<AppState>) -> HWND {
    app.settings_hwnd.load(Ordering::SeqCst) as HWND
}

// ===========================================================================
// Asset exporter (feature = "assets")
//
// Reuses the real renderer to emit README art on a clean, generated background:
//   bubble.png   — the floating bubble on transparency (soft shadow preserved)
//   demo.gif     — the waveform pulsing on a tasteful gradient (seamless loop)
//   settings.png — the settings window with demo data and a modern scrollbar
// Run with:  cargo run --release --features assets -- --render-assets docs
// ===========================================================================

#[cfg(feature = "assets")]
fn render_assets(out_dir: &str) -> Result<()> {
    use std::path::Path;
    let dir = Path::new(out_dir);
    std::fs::create_dir_all(dir)?;

    let bars = WAVE_BARS;
    let pi = std::f32::consts::PI;
    // A lively per-bar pattern. Integer "speed" per bar keeps the loop seamless.
    let frame_wave = |t: f32, frames: f32| -> Vec<f32> {
        (0..bars)
            .map(|i| {
                let phase = i as f32 * 0.55;
                let speed = 1.0 + (i % 3) as f32; // 1,2,3 -> periodic over [0, frames)
                let hump = (i as f32 / (bars - 1) as f32 * pi).sin(); // taller in the middle
                let env = 0.42 + 0.58 * hump;
                let s = (2.0 * pi * (t / frames) * speed + phase).sin();
                (env * (0.30 + 0.70 * (0.5 + 0.5 * s))).clamp(0.06, 1.0)
            })
            .collect()
    };

    // ---- bubble.png : transparent, 2x upscaled ----
    {
        let wave = frame_wave(9.0, 48.0);
        let mut buf = vec![0u32; (WIDTH * HEIGHT) as usize];
        paint_bubble_into(&mut buf, WIDTH, HEIGHT, 0.0, 0.0, &wave, true, true);
        let (big, bw, bh) = upscale_premul(&buf, WIDTH as usize, HEIGHT as usize, 2);
        let rgba: Vec<u8> = big.iter().flat_map(|&p| unpremultiply(p)).collect();
        write_png(&dir.join("bubble.png"), bw as u32, bh as u32, &rgba)?;
    }

    // ---- demo.gif : bubble pulsing on a gradient ----
    {
        const HERO_W: usize = 480;
        const HERO_H: usize = 220;
        const FRAMES: usize = 48;
        let scale = 2usize;
        let bg = gradient_bg(HERO_W, HERO_H);
        let bw = WIDTH as usize * scale;
        let bh = HEIGHT as usize * scale;
        let ox = (HERO_W - bw) / 2;
        let oy = (HERO_H - bh) / 2;
        let mut frames: Vec<Vec<u32>> = Vec::with_capacity(FRAMES);
        for t in 0..FRAMES {
            let wave = frame_wave(t as f32, FRAMES as f32);
            let mut buf = vec![0u32; (WIDTH * HEIGHT) as usize];
            paint_bubble_into(&mut buf, WIDTH, HEIGHT, 0.0, 0.0, &wave, true, true);
            let (big, _, _) = upscale_premul(&buf, WIDTH as usize, HEIGHT as usize, scale);
            let mut frame = bg.clone();
            composite_over(&mut frame, HERO_W, HERO_H, &big, bw, bh, ox, oy);
            frames.push(frame);
        }
        write_gif(&dir.join("demo.gif"), HERO_W, HERO_H, &frames, 3)?;
    }

    // ---- settings-<tab>.png : one image per tab of the settings window ----
    {
        let history = demo_history();
        let cw = SETTINGS_WIDTH;
        let ch = SETTINGS_HEIGHT;
        // The Model tab shows the multilingual model with a language chosen, so
        // the Language control is not greyed out in the README.
        let multilingual = AppSettings {
            model: ModelChoice::Multilingual,
            language: "de".to_string(),
            ..AppSettings::default()
        };
        for (tab, name, settings, status) in [
            (TAB_HISTORY, "settings-history.png", AppSettings::default(), ""),
            (TAB_MODEL, "settings-model.png", multilingual, "Loaded: Multilingual (German) · CPU"),
            (TAB_SETTINGS, "settings-settings.png", AppSettings::default(), ""),
        ] {
            if let Some(mut canvas) = unsafe { Canvas::new(cw, ch) } {
                let view = HistoryView {
                    items: &history,
                    scroll: 0,
                    copied: None,
                };
                unsafe {
                    render_settings_canvas(&mut canvas, &settings, status, &view, false, tab)
                };
                let rgba = settings_to_rgba(canvas.pixels(), cw as usize, ch as usize, 16.0);
                write_png(&dir.join(name), cw as u32, ch as u32, &rgba)?;
            }
        }
    }

    println!("Wrote bubble.png, demo.gif and settings-<tab>.png to {out_dir}");
    Ok(())
}

#[cfg(feature = "assets")]
fn demo_history() -> Vec<HistoryItem> {
    let make = |timestamp: &str, device: &str, ms: u32, text: &str| HistoryItem {
        timestamp: timestamp.to_string(),
        text: text.to_string(),
        processing: Some(device.to_string()),
        ms_per_chunk: Some(ms),
    };
    vec![
        make("Nov 24, 2:14 PM", "CPU", 128, "Let's ship the new dictation bubble — it looks fantastic."),
        make("Nov 24, 2:11 PM", "CPU", 131, "Meeting notes: confirm the roadmap and follow up with the design team tomorrow."),
        make("Nov 24, 1:58 PM", "GPU", 287, "Reminder to refactor the audio capture pipeline next sprint."),
        make("Nov 24, 1:42 PM", "CPU", 126, "The quick brown fox jumps over the lazy dog."),
        make("Nov 23, 6:30 PM", "CPU", 133, "Draft the release notes and update the screenshots in the README."),
    ]
}

#[cfg(feature = "assets")]
fn unpremultiply(p: u32) -> [u8; 4] {
    let a = ((p >> 24) & 0xff) as u32;
    if a == 0 {
        return [0, 0, 0, 0];
    }
    let un = |c: u32| (((c & 0xff) * 255 + a / 2) / a).min(255) as u8;
    [un(p >> 16), un(p >> 8), un(p), a as u8]
}

#[cfg(feature = "assets")]
fn lerp_argb(a: u32, b: u32, t: f32) -> u32 {
    let mix = |sh: u32| {
        let ca = ((a >> sh) & 0xff) as f32;
        let cb = ((b >> sh) & 0xff) as f32;
        (((ca + (cb - ca) * t).round() as u32).min(255)) << sh
    };
    mix(24) | mix(16) | mix(8) | mix(0)
}

/// Bilinear upscale of a premultiplied ARGB buffer (interpolating alpha too).
#[cfg(feature = "assets")]
fn upscale_premul(src: &[u32], sw: usize, sh: usize, scale: usize) -> (Vec<u32>, usize, usize) {
    let dw = sw * scale;
    let dh = sh * scale;
    let mut dst = vec![0u32; dw * dh];
    for dy in 0..dh {
        let fy = (dy as f32 + 0.5) / scale as f32 - 0.5;
        let y0 = fy.floor().max(0.0) as usize;
        let y1 = (y0 + 1).min(sh - 1);
        let ty = (fy - y0 as f32).clamp(0.0, 1.0);
        for dx in 0..dw {
            let fx = (dx as f32 + 0.5) / scale as f32 - 0.5;
            let x0 = fx.floor().max(0.0) as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let tx = (fx - x0 as f32).clamp(0.0, 1.0);
            let top = lerp_argb(src[y0 * sw + x0], src[y0 * sw + x1], tx);
            let bot = lerp_argb(src[y1 * sw + x0], src[y1 * sw + x1], tx);
            dst[dy * dw + dx] = lerp_argb(top, bot, ty);
        }
    }
    (dst, dw, dh)
}

/// A tasteful opaque gradient backdrop with a soft blue glow near the top-centre.
#[cfg(feature = "assets")]
fn gradient_bg(w: usize, h: usize) -> Vec<u32> {
    let mut v = vec![0u32; w * h];
    for y in 0..h {
        let t = y as f32 / (h - 1).max(1) as f32;
        for x in 0..w {
            let base_r = lerp(34.0, 14.0, t);
            let base_g = lerp(38.0, 16.0, t);
            let base_b = lerp(56.0, 23.0, t);
            let dx = (x as f32 - w as f32 * 0.5) / (w as f32 * 0.6);
            let dy = (y as f32 - h as f32 * 0.30) / (h as f32 * 0.6);
            let glow = (1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0).powf(2.0) * 24.0;
            let r = (base_r + glow * 0.5).min(255.0) as u32;
            let g = (base_g + glow * 0.8).min(255.0) as u32;
            let b = (base_b + glow * 1.5).min(255.0) as u32;
            v[y * w + x] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
        }
    }
    v
}

/// Composite a premultiplied ARGB sprite over an opaque background buffer.
#[cfg(feature = "assets")]
#[allow(clippy::too_many_arguments)]
fn composite_over(
    bg: &mut [u32],
    bgw: usize,
    bgh: usize,
    fg: &[u32],
    fw: usize,
    fh: usize,
    ox: usize,
    oy: usize,
) {
    for y in 0..fh {
        let by = oy + y;
        if by >= bgh {
            break;
        }
        for x in 0..fw {
            let bx = ox + x;
            if bx >= bgw {
                continue;
            }
            let s = fg[y * fw + x];
            let sa = ((s >> 24) & 0xff) as f32 / 255.0;
            if sa <= 0.0 {
                continue;
            }
            let inv = 1.0 - sa;
            let d = bg[by * bgw + bx];
            let r = (((s >> 16) & 0xff) as f32 + ((d >> 16) & 0xff) as f32 * inv).min(255.0) as u32;
            let g = (((s >> 8) & 0xff) as f32 + ((d >> 8) & 0xff) as f32 * inv).min(255.0) as u32;
            let b = ((s & 0xff) as f32 + (d & 0xff) as f32 * inv).min(255.0) as u32;
            bg[by * bgw + bx] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
        }
    }
}

/// Convert an opaque settings canvas to RGBA: GDI text leaves the alpha byte
/// undefined, so force alpha opaque and apply a rounded-corner mask for polish.
#[cfg(feature = "assets")]
fn settings_to_rgba(buf: &[u32], w: usize, h: usize, radius: f32) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;
    let hx = w as f32 / 2.0;
    let hy = h as f32 / 2.0;
    for y in 0..h {
        for x in 0..w {
            let p = buf[y * w + x];
            let d = sd_round_rect(x as f32 + 0.5, y as f32 + 0.5, cx, cy, hx, hy, radius);
            let cov = (0.5 - d).clamp(0.0, 1.0);
            let i = (y * w + x) * 4;
            out[i] = ((p >> 16) & 0xff) as u8;
            out[i + 1] = ((p >> 8) & 0xff) as u8;
            out[i + 2] = (p & 0xff) as u8;
            out[i + 3] = (cov * 255.0) as u8;
        }
    }
    out
}

#[cfg(feature = "assets")]
fn write_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

/// Write the exe icon (embedded by build.rs) as a multi-size .ico.
/// Run with:  cargo run --release --features assets -- --render-icon assets/nemotron-bubble.ico
#[cfg(feature = "assets")]
fn render_icon(path: &std::path::Path) -> Result<()> {
    const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];
    let mut images = Vec::new();
    for size in SIZES {
        let rgba = app_icon_rgba(size);
        let data = if size >= 256 {
            // 256 px: PNG-compressed, as Windows expects for the large size.
            let mut png_bytes = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut png_bytes, size, size);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.write_header()?.write_image_data(&rgba)?;
            }
            png_bytes
        } else {
            icon_dib(size, &rgba)
        };
        images.push((size, data));
    }

    // ICO container: uncompressed 32-bit DIBs for the small sizes (readable by
    // every tool), PNG for 256 px.
    let mut ico = Vec::new();
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    ico.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        let dim = if *size >= 256 { 0u8 } else { *size as u8 }; // 0 means 256
        ico.extend_from_slice(&[dim, dim, 0, 0]); // width, height, palette, reserved
        ico.extend_from_slice(&1u16.to_le_bytes()); // planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        ico.extend_from_slice(data);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, ico)?;
    println!("Wrote {} ({} sizes)", path.display(), images.len());
    Ok(())
}

/// One ICO entry as a 32-bit DIB: BITMAPINFOHEADER (height doubled to cover the
/// AND mask), BGRA rows bottom-up, then an all-zero 1-bit AND mask (alpha rules).
#[cfg(feature = "assets")]
fn icon_dib(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mask_row = size.div_ceil(32) * 4; // 1 bpp rows padded to 32 bits
    let mut dib = Vec::new();
    dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
    dib.extend_from_slice(&(size as i32).to_le_bytes()); // biWidth
    dib.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // biHeight: XOR + AND
    dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    dib.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    dib.extend_from_slice(&(size * size * 4 + mask_row * size).to_le_bytes()); // biSizeImage
    dib.extend_from_slice(&[0u8; 16]); // resolution + palette fields
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            dib.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    dib.resize(dib.len() + (mask_row * size) as usize, 0);
    dib
}

/// The app icon as straight-alpha RGBA: a smooth, resolution-independent take on
/// the tray glyph (dark disc with the equalizer bars in the recording blue).
/// Geometry is defined on the tray icon's 32-unit grid and supersampled 4x4.
#[cfg(feature = "assets")]
fn app_icon_rgba(size: u32) -> Vec<u8> {
    const SHAPE: [f32; 9] = [0.30, 0.58, 0.86, 1.0, 0.72, 0.46, 0.66, 0.9, 0.5];
    const SS: u32 = 4;
    let disc = (26.0f32, 29.0, 35.0); // tray background 0x1A1D21
    let bar = (122.0f32, 184.0, 255.0); // recording accent 0x7AB8FF
    let rim = (72.0f32, 80.0, 96.0);
    let scale = 32.0 / size as f32;

    // Capsule (rounded bar) membership for a point on the 32-unit grid.
    let in_bar = |x: f32, y: f32| -> bool {
        let radius = 0.75;
        SHAPE.iter().enumerate().any(|(i, s)| {
            let cx = 8.0 + i as f32 * 2.0;
            let half = (s * 0.8 * 22.0) / 2.0;
            let top = 16.0 - half + radius;
            let bottom = 16.0 + half - radius;
            let dy = if y < top { top - y } else if y > bottom { y - bottom } else { 0.0 };
            let dx = x - cx;
            dx * dx + dy * dy <= radius * radius
        })
    };

    let mut out = vec![0u8; (size * size * 4) as usize];
    for py in 0..size {
        for px in 0..size {
            let (mut disc_hits, mut bar_hits, mut rim_hits) = (0u32, 0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) * scale;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) * scale;
                    let (dx, dy) = (x - 16.0, y - 16.0);
                    let dist_sq = dx * dx + dy * dy;
                    if dist_sq <= 15.0 * 15.0 {
                        disc_hits += 1;
                        if in_bar(x, y) {
                            bar_hits += 1;
                        } else if dist_sq >= 14.0 * 14.0 {
                            // thin lighter rim so the disc stays visible on dark backgrounds
                            rim_hits += 1;
                        }
                    }
                }
            }
            if disc_hits == 0 {
                continue;
            }
            let hits = disc_hits as f32;
            let (tb, tr) = (bar_hits as f32 / hits, rim_hits as f32 / hits);
            let td = 1.0 - tb - tr;
            let mix = |d: f32, b: f32, r: f32| (d * td + b * tb + r * tr).round() as u8;
            let i = ((py * size + px) * 4) as usize;
            out[i] = mix(disc.0, bar.0, rim.0);
            out[i + 1] = mix(disc.1, bar.1, rim.1);
            out[i + 2] = mix(disc.2, bar.2, rim.2);
            out[i + 3] = ((disc_hits as f32 / (SS * SS) as f32) * 255.0).round() as u8;
        }
    }
    out
}

#[cfg(feature = "assets")]
fn write_gif(
    path: &std::path::Path,
    w: usize,
    h: usize,
    frames: &[Vec<u32>],
    delay_cs: u16,
) -> Result<()> {
    let mut file = std::fs::File::create(path)?;
    let mut encoder = gif::Encoder::new(&mut file, w as u16, h as u16, &[])?;
    encoder.set_repeat(gif::Repeat::Infinite)?;
    for frame in frames {
        let mut rgba: Vec<u8> = Vec::with_capacity(frame.len() * 4);
        for &p in frame {
            rgba.push(((p >> 16) & 0xff) as u8);
            rgba.push(((p >> 8) & 0xff) as u8);
            rgba.push((p & 0xff) as u8);
            rgba.push(255);
        }
        let mut f = gif::Frame::from_rgba_speed(w as u16, h as u16, &mut rgba, 10);
        f.delay = delay_cs;
        encoder.write_frame(&f)?;
    }
    Ok(())
}
