//! Tray icon and tray menu.

use crate::*;

pub(crate) const TRAY_ID: u32 = 1;
pub(crate) const WM_TRAYICON: u32 = WM_USER + 1;
pub(crate) const IDM_START_STOP: usize = 1001;
pub(crate) const IDM_SHOW_HIDE: usize = 1002;
pub(crate) const IDM_QUIT: usize = 1004;
// Tray "Speech model" submenu: one id per ModelChoice, in ModelChoice::ALL order.
pub(crate) const IDM_MODEL_BASE: usize = 1100;
// "Language" menus (tray submenu and the popup in the application window): one id per LANGUAGES entry.
pub(crate) const IDM_LANG_BASE: usize = 1200;
// Tray "Processing" submenu: one id per ComputeChoice, in ComputeChoice::ALL order.
pub(crate) const IDM_COMPUTE_BASE: usize = 1300;
// Bubble position popup menu: one id per BubblePosition, in BubblePosition::ALL order.
pub(crate) const IDM_BUBBLE_POS_BASE: usize = 1400;
// Microphone popup menu: 0 = Windows default, then one id per listed input device.
pub(crate) const IDM_MIC_BASE: usize = 1500;
pub(crate) const IDM_MIC_MAX: usize = 64;
// The currently-installed tray HICON. Animated icons are swapped in via NIM_MODIFY
// and the previous one destroyed, so this tracks what needs cleanup.
pub(crate) static CURRENT_TRAY_ICON: AtomicIsize = AtomicIsize::new(0);
pub(crate) unsafe fn add_tray_icon(hwnd: HWND) {
    // Track only the icon we own. If creation fails, fall back to the shared system
    // icon for display but store 0 so it is never passed to DestroyIcon.
    let icon = make_tray_icon(0.0, false, false);
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
    fill_wide_fixed(&mut nid.szTip, &tray_tooltip(None));
    Shell_NotifyIconW(NIM_ADD, &mut nid);
    CURRENT_TRAY_ICON.store(icon as isize, Ordering::SeqCst);
}

/// Tray tooltip text: the version, plus the reason when no microphone is available.
/// Windows cuts it off after 127 characters.
fn tray_tooltip(mic_error: Option<&str>) -> String {
    match mic_error {
        Some(reason) => format!("Nemotron Bubble {APP_VERSION}\nNo microphone: {reason}"),
        None => format!("Nemotron Bubble {APP_VERSION}"),
    }
}

pub(crate) unsafe fn update_tray_tooltip(hwnd: HWND, mic_error: Option<&str>) {
    if hwnd == null_mut() {
        return;
    }
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ID;
    nid.uFlags = NIF_TIP;
    fill_wide_fixed(&mut nid.szTip, &tray_tooltip(mic_error));
    Shell_NotifyIconW(NIM_MODIFY, &mut nid);
}

pub(crate) unsafe fn remove_tray_icon(hwnd: HWND) {
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

pub(crate) unsafe fn show_tray_menu(hwnd: HWND) {
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

    // The version heads the menu as information, Quit ends it like in other Windows apps.
    AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, version_text.as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, IDM_START_STOP, start_text.as_ptr());
    AppendMenuW(menu, MF_STRING, IDM_SHOW_HIDE, show_text.as_ptr());
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

    let mut point: POINT = std::mem::zeroed();
    GetCursorPos(&mut point);
    SetForegroundWindow(hwnd);
    TrackPopupMenu(menu, TPM_RIGHTBUTTON, point.x, point.y, 0, hwnd, null());
    DestroyMenu(menu);
}

/// Build a 32x32 tray icon. While recording the equalizer bars scale with `level`
/// and a small red dot appears; idle shows a static dim glyph, crossed out in amber
/// with `mic_missing`. The caller owns the returned HICON and must DestroyIcon it
/// when replaced.
pub(crate) unsafe fn make_tray_icon(level: f32, recording: bool, mic_missing: bool) -> HICON {
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

    if mic_missing && !recording {
        // Anti-aliased slash with a gap in the background colour. All its pixels
        // lie inside the opaque circle, so only the colour channels are mixed.
        let mix = |from: u32, to: u32, t: f32| -> u32 {
            let channel = |shift: u32| {
                let a = ((from >> shift) & 0xFF) as f32;
                let b = ((to >> shift) & 0xFF) as f32;
                ((a + (b - a) * t).round() as u32) << shift
            };
            0xFF00_0000 | channel(16) | channel(8) | channel(0)
        };
        let (r, g, b) = MIC_MISSING_RGB;
        let amber = 0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let d = sd_segment(x as f32 + 0.5, y as f32 + 0.5, 8.0, 8.0, 24.0, 24.0);
                let idx = (y * SIZE + x) as usize;
                let gap = (4.0 - d).clamp(0.0, 1.0);
                if gap > 0.0 {
                    pixels[idx] = mix(pixels[idx], bg, gap);
                }
                let line = (2.3 - d).clamp(0.0, 1.0);
                if line > 0.0 {
                    pixels[idx] = mix(pixels[idx], amber, line);
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
pub(crate) unsafe fn update_tray_icon(hwnd: HWND, icon: HICON) {
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

/// The idle tray icon, crossed out while no microphone is available.
pub(crate) unsafe fn make_idle_tray_icon(app: &Arc<AppState>) -> HICON {
    let mic_missing = lock_ui(app).mic_error.is_some();
    make_tray_icon(0.0, false, mic_missing)
}

/// Reset the tray icon to its idle glyph.
pub(crate) fn refresh_tray_idle(app: &Arc<AppState>) {
    unsafe {
        let hwnd = hwnd_from_app(app);
        if hwnd != null_mut() {
            update_tray_icon(hwnd, make_idle_tray_icon(app));
        }
    }
}

