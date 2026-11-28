//! Tray icon and tray menu.

use crate::*;

pub(crate) const TRAY_ID: u32 = 1;
pub(crate) const WM_TRAYICON: u32 = WM_USER + 1;
pub(crate) const IDM_START_STOP: usize = 1001;
pub(crate) const IDM_SHOW_HIDE: usize = 1002;
pub(crate) const IDM_QUIT: usize = 1004;
pub(crate) const IDM_UPDATE: usize = 1005;
// Segoe MDL2 Assets "Up" arrow: the mark of an available update (tray menu, footer pill).
pub(crate) const GLYPH_UPDATE: &str = "\u{E74A}";
// Tray "Speech model" submenu: one id per ModelChoice, in ModelChoice::ALL order.
pub(crate) const IDM_MODEL_BASE: usize = 1100;
// "Language" menus (tray submenu and the popup in the application window): one id per LANGUAGES entry.
pub(crate) const IDM_LANG_BASE: usize = 1200;
// Tray "Processing" submenu: one id per ComputeChoice, in ComputeChoice::ALL order.
pub(crate) const IDM_COMPUTE_BASE: usize = 1300;
// Microphone popup menu: 0 = Windows default, then one id per listed input device.
pub(crate) const IDM_MIC_BASE: usize = 1500;
pub(crate) const IDM_MIC_MAX: usize = 64;
// Auto-stop popup menu: one id per SILENCE_AUTO_STOP_CHOICES entry, in that order.
pub(crate) const IDM_AUTO_STOP_BASE: usize = 1600;
// The currently-installed tray HICON. Animated icons are swapped in via NIM_MODIFY
// and the previous one destroyed, so this tracks what needs cleanup.
pub(crate) static CURRENT_TRAY_ICON: AtomicIsize = AtomicIsize::new(0);
pub(crate) unsafe fn add_tray_icon(hwnd: HWND) {
    // Track only the icon we own. If creation fails, fall back to the shared system
    // icon for display but store 0 so it is never passed to DestroyIcon.
    let icon = make_tray_icon(0.0, false, false, false);
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
    fill_wide_fixed(&mut nid.szTip, &tray_tooltip(None, None));
    Shell_NotifyIconW(NIM_ADD, &mut nid);
    CURRENT_TRAY_ICON.store(icon as isize, Ordering::SeqCst);
}

/// Tray tooltip text: the version, an available update, and the reason when no
/// microphone is available. Windows cuts it off after 127 characters, so the
/// update line comes before the reason, which may be long.
fn tray_tooltip(mic_error: Option<&str>, update: Option<&str>) -> String {
    let mut text = format!("Nemotron Bubble {APP_VERSION}");
    if let Some(version) = update {
        text.push_str(&format!("\nUpdate available: {version}"));
    }
    if let Some(reason) = mic_error {
        text.push_str(&format!("\nNo microphone: {reason}"));
    }
    text
}

pub(crate) unsafe fn update_tray_tooltip(hwnd: HWND, mic_error: Option<&str>, update: Option<&str>) {
    if hwnd == null_mut() {
        return;
    }
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ID;
    nid.uFlags = NIF_TIP;
    fill_wide_fixed(&mut nid.szTip, &tray_tooltip(mic_error, update));
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

    // "Speech model" submenu, current choice checked; locked while dictating.
    let menu_settings = APP.get().map(settings_snapshot).unwrap_or_default();
    let current_model = menu_settings.model;
    let model_menu = CreatePopupMenu();
    if model_menu != null_mut() {
        for (index, choice) in ModelChoice::ALL.iter().enumerate() {
            let mut flags = MF_STRING;
            if *choice == current_model {
                flags |= MF_CHECKED;
            }
            if recording || !model_choice_available(&menu_settings, *choice) {
                flags |= MF_GRAYED;
            }
            append_menu_item(model_menu, flags, IDM_MODEL_BASE + index, choice.label());
        }
    }

    // The version heads the menu as information, Quit ends it like in other Windows apps.
    append_menu_item(menu, MF_STRING | MF_GRAYED, 0, &format!("Nemotron Bubble {APP_VERSION}"));
    // An available update sits right below the version. It opens the update panel.
    if let Some(version) = APP.get().and_then(pending_update_version) {
        append_accent_menu_item(menu, IDM_UPDATE, &format!("Update to {version}..."), GLYPH_UPDATE);
    }
    append_menu_separator(menu);
    append_menu_item(menu, MF_STRING, IDM_START_STOP, if recording { "Stop" } else { "Start" });
    append_menu_item(menu, MF_STRING, IDM_SHOW_HIDE, "Show / Hide Bubble");
    if model_menu != null_mut() {
        // Destroying the menu after it closed also destroys this attached submenu.
        append_menu_item(menu, MF_POPUP, model_menu as usize, "Speech model");
    }
    // "Processing" submenu: Auto / GPU / CPU, current one checked.
    let compute_menu = CreatePopupMenu();
    if compute_menu != null_mut() {
        for (index, choice) in ComputeChoice::ALL.iter().enumerate() {
            let mut flags = MF_STRING;
            if *choice == menu_settings.compute {
                flags |= MF_CHECKED;
            }
            if recording {
                flags |= MF_GRAYED;
            }
            append_menu_item(compute_menu, flags, IDM_COMPUTE_BASE + index, choice.label());
        }
    }
    let language_menu = build_language_menu(&menu_settings, recording);
    if language_menu != null_mut() {
        let flags = if language_selectable(&menu_settings) {
            MF_POPUP
        } else {
            MF_POPUP | MF_GRAYED
        };
        let text = format!("Language: {}", language_label(&menu_settings.language));
        append_menu_item(menu, flags, language_menu as usize, &text);
    }
    if compute_menu != null_mut() {
        let text = format!("Processing: {}", menu_settings.compute.label());
        append_menu_item(menu, MF_POPUP, compute_menu as usize, &text);
    }
    append_menu_separator(menu);
    append_menu_item(menu, MF_STRING, IDM_QUIT, "Quit");

    let mut point: POINT = std::mem::zeroed();
    GetCursorPos(&mut point);
    SetForegroundWindow(hwnd);
    track_popup_menu(menu, TPM_RIGHTBUTTON, point.x, point.y, hwnd);
}

/// Build a 32x32 tray icon. While recording the equalizer bars scale with `level`
/// and a small red dot appears; idle shows a static dim glyph, crossed out in amber
/// with `mic_missing`. `update_dot` adds a blue dot at the bottom right for an
/// available update. The caller owns the returned HICON and must DestroyIcon it
/// when replaced.
pub(crate) unsafe fn make_tray_icon(level: f32, recording: bool, mic_missing: bool, update_dot: bool) -> HICON {
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

    if update_dot {
        // Accent dot in a dark ring, so it stands out on the glyph and on any taskbar.
        let (cx, cy) = (25.5f32, 25.5f32);
        for y in 0..SIZE {
            for x in 0..SIZE {
                let d = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy);
                let idx = (y * SIZE + x) as usize;
                let ring = (6.4 - d).clamp(0.0, 1.0);
                if ring > 0.0 {
                    pixels[idx] = argb(0x1A, 0x1D, 0x21, ring.max(pixel_alpha(pixels[idx])));
                }
                let dot = (4.6 - d).clamp(0.0, 1.0);
                if dot > 0.0 {
                    pixels[idx] = blend_over(pixels[idx], (0x6E, 0xA2, 0xFF), dot);
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

/// Alpha (0..1) of a 0xAARRGGBB icon pixel.
fn pixel_alpha(pixel: u32) -> f32 {
    (pixel >> 24) as f32 / 255.0
}

/// A 0xAARRGGBB icon pixel (straight, not premultiplied alpha) of (r, g, b) with `alpha`.
fn argb(r: u8, g: u8, b: u8, alpha: f32) -> u32 {
    ((alpha * 255.0).round() as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}

/// Paint the opaque `color` over the icon `pixel` with coverage `amount`.
fn blend_over(pixel: u32, color: (u8, u8, u8), amount: f32) -> u32 {
    let mix = |shift: u32, value: u8| {
        let below = ((pixel >> shift) & 0xFF) as f32;
        ((value as f32 * amount + below * (1.0 - amount)).round() as u32) << shift
    };
    mix(24, 0xFF) | mix(16, color.0) | mix(8, color.1) | mix(0, color.2)
}

/// The idle tray icon, crossed out while no microphone is available, with a dot
/// while an update is available.
pub(crate) unsafe fn make_idle_tray_icon(app: &Arc<AppState>) -> HICON {
    let mic_missing = lock_ui(app).mic_error.is_some();
    make_tray_icon(0.0, false, mic_missing, pending_update_version(app).is_some())
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

