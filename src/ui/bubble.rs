//! The floating bubble: drawing, animation, placement and dragging.

use crate::*;

// Floating bubble: a soft-shadowed pill rendered with per-pixel alpha. The window
// is larger than the pill so the drop shadow has room to fall off smoothly.
pub(crate) const BUBBLE_MARGIN: i32 = 10;
pub(crate) const PILL_W: i32 = 120;
pub(crate) const PILL_H: i32 = 54;
pub(crate) const WIDTH: i32 = PILL_W + BUBBLE_MARGIN * 2;
pub(crate) const HEIGHT: i32 = PILL_H + BUBBLE_MARGIN * 2;
pub(crate) const WAVE_BARS: usize = 18;
pub(crate) const ANIM_TIMER_ID: usize = 77;
pub(crate) const ANIM_INTERVAL_MS: u32 = 33; // ~30 fps

pub(crate) struct DragState {
    pub(crate) hwnd: isize,
    pub(crate) start_cursor: POINT,
    pub(crate) start_rect: RECT,
    pub(crate) moved: bool,
}

/// Move the floating bubble to another preset position and save it.
pub(crate) fn select_bubble_position(app: &Arc<AppState>, position: BubblePosition) {
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

pub(crate) fn toggle_window_visibility() {
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

pub(crate) fn handle_click(x: i32, y: i32) {
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

pub(crate) fn should_start_drag(hwnd: HWND, x: i32, y: i32) -> bool {
    if is_settings_window(hwnd) {
        y < 84 && !settings_close_rect().contains(x, y)
    } else {
        true
    }
}

pub(crate) fn begin_drag(hwnd: HWND) {
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

pub(crate) fn continue_drag() {
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

pub(crate) fn end_drag() -> bool {
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

/// Called from the audio thread with the latest instantaneous bar height (0..1).
/// Smooths it with a fast attack / slow release so the meter feels responsive but
/// never jittery, then stashes it for the UI-thread animation timer to sample.
/// Does no GDI work — drawing only happens on the UI thread.
pub(crate) fn report_level(app: &Arc<AppState>, target: f32, rms: f32) {
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
pub(crate) fn advance_waveform(app: &Arc<AppState>) -> f32 {
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

/// Render the floating bubble into a layered window: drop shadow, glassy pill,
/// status dot, and the mirrored anti-aliased waveform.
pub(crate) unsafe fn render_bubble(hwnd: HWND) {
    if hwnd == null_mut() {
        return;
    }
    let Some(app) = APP.get() else {
        return;
    };
    let (waveform, recording, wave_enabled, mic_missing) = {
        let ui = lock_ui(app);
        let settings = settings_snapshot(app);
        (
            ui.waveform.clone(),
            ui.recording,
            settings.waveform_enabled,
            ui.mic_error.is_some(),
        )
    };

    let Some(mut canvas) = Canvas::new(WIDTH, HEIGHT) else {
        return;
    };
    canvas.clear_transparent();
    {
        let buf = canvas.pixels();
        paint_bubble_into(
            buf,
            WIDTH,
            HEIGHT,
            0.0,
            0.0,
            &waveform,
            recording,
            wave_enabled,
            mic_missing && !recording,
        );
    }
    canvas.present_layered(hwnd);
}

/// Draw the bubble (shadow, glassy pill, status dot, waveform) into a premultiplied
/// pixel buffer. The bubble's margin box is placed at (ox, oy) so it can be composited
/// anywhere — used both by the live layered window and the asset exporter. With
/// `mic_missing` the dot turns amber and a crossed-out microphone replaces the waveform.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_bubble_into(
    buf: &mut [u32],
    cw: i32,
    ch: i32,
    ox: f32,
    oy: f32,
    waveform: &[f32],
    recording: bool,
    wave_enabled: bool,
    mic_missing: bool,
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
    } else if mic_missing {
        (MIC_MISSING_RGB, 0.95)
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

    // waveform fills the remainder of the pill (the same area draw_wave_bars uses)
    if mic_missing {
        let area_left = dot_cx + dot_r + 6.0;
        let area_right = px + pw - 12.0;
        draw_mic_off(buf, cw, ch, (area_left + area_right) / 2.0, py + ph / 2.0);
        return;
    }
    let active = recording && wave_enabled;
    draw_wave_bars(buf, cw, ch, waveform, active, dot_cx + dot_r, px + pw, py, ph);
}

/// Amber of the "no microphone" state, used by the bubble and the tray icon.
pub(crate) const MIC_MISSING_RGB: Rgb = (255, 179, 71);

/// Crossed-out microphone glyph, about 22 px tall, centred at (cx, cy).
fn draw_mic_off(buf: &mut [u32], cw: i32, ch: i32, cx: f32, cy: f32) {
    let color = MIC_MISSING_RGB;
    let alpha = 0.95;
    // capsule
    fill_round_rect(buf, cw, ch, cx - 4.0, cy - 11.0, 8.0, 13.0, 4.0, color, color, alpha);
    // holder: the lower half of a ring around the capsule
    let (ring_cy, ring_r) = (cy - 2.0, 7.5);
    fill_sdf(buf, cw, ch, (cx - 9.0, ring_cy, cx + 9.0, ring_cy + 9.0), color, alpha, |x, y| {
        let ring = ((x - cx).hypot(y - ring_cy) - ring_r).abs() - 0.9;
        ring.max(ring_cy - y)
    });
    // stand and foot
    fill_sdf(buf, cw, ch, (cx - 1.0, cy + 5.0, cx + 1.0, cy + 10.0), color, alpha, |x, y| {
        sd_segment(x, y, cx, cy + 6.0, cx, cy + 9.0) - 0.9
    });
    fill_sdf(buf, cw, ch, (cx - 5.0, cy + 8.0, cx + 5.0, cy + 11.0), color, alpha, |x, y| {
        sd_segment(x, y, cx - 3.5, cy + 9.5, cx + 3.5, cy + 9.5) - 0.9
    });
    // slash with a gap in the pill colour, so it reads as crossed out
    let (ax, ay, bx, by) = (cx - 8.0, cy - 11.0, cx + 8.0, cy + 10.0);
    let slash_box = (ax - 4.0, ay - 4.0, bx + 4.0, by + 4.0);
    fill_sdf(buf, cw, ch, slash_box, (36, 40, 49), 1.0, |x, y| {
        sd_segment(x, y, ax, ay, bx, by) - 2.6
    });
    fill_sdf(buf, cw, ch, slash_box, color, alpha, |x, y| {
        sd_segment(x, y, ax, ay, bx, by) - 1.1
    });
}

// --- "No microphone" indicator ----------------------------------------------

/// Retry timer that reopens the microphone once one is back.
pub(crate) const MIC_RETRY_TIMER_ID: usize = 79;
pub(crate) const MIC_RETRY_INTERVAL_MS: u32 = 3000;
/// Tooltip control of the floating bubble (0 when it could not be created).
pub(crate) static BUBBLE_TOOLTIP: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    // The mic_error the tray icon, the tooltips and the retry timer last showed.
    static SHOWN_MIC_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Create the bubble's tooltip. It stays inactive while a microphone is available.
pub(crate) unsafe fn create_bubble_tooltip(bubble: HWND) {
    let controls = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_WIN95_CLASSES,
    };
    InitCommonControlsEx(&controls);
    let tooltip = CreateWindowExW(
        WS_EX_TOPMOST,
        TOOLTIPS_CLASSW,
        null(),
        WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        bubble,
        null_mut(),
        GetModuleHandleW(null()),
        null(),
    );
    if tooltip == null_mut() {
        log_error("Could not create the tooltip of the bubble.");
        return;
    }
    let mut text = wide_null("");
    let info = bubble_tool_info(bubble, text.as_mut_ptr());
    SendMessageW(tooltip, TTM_ADDTOOLW, 0, &info as *const _ as LPARAM);
    SendMessageW(tooltip, TTM_SETMAXTIPWIDTH, 0, 360);
    SendMessageW(tooltip, TTM_ACTIVATE, 0, 0);
    BUBBLE_TOOLTIP.store(tooltip as isize, Ordering::SeqCst);
}

/// The bubble's only tool: the whole window, subclassed so the tooltip sees the mouse.
unsafe fn bubble_tool_info(bubble: HWND, text: *mut u16) -> TTTOOLINFOW {
    let mut info: TTTOOLINFOW = std::mem::zeroed();
    // Without lpReserved (the "V2" size): accepted by comctl32 5 and 6 alike. The
    // exe has no common controls 6 manifest, and version 5 rejects the full size.
    info.cbSize = (std::mem::size_of::<TTTOOLINFOW>() - std::mem::size_of::<*mut c_void>()) as u32;
    info.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
    info.hwnd = bubble;
    info.uId = bubble as usize;
    info.lpszText = text;
    info
}

/// Bring the tray icon, both tooltips and the retry timer in line with
/// `UiState::mic_error`. Runs on the UI thread after every UI update.
pub(crate) unsafe fn sync_mic_indicator(hwnd: HWND) {
    let Some(app) = APP.get() else {
        return;
    };
    let (error, recording) = {
        let ui = lock_ui(app);
        (ui.mic_error.clone(), ui.recording)
    };
    let changed = SHOWN_MIC_ERROR.with(|shown| {
        let mut shown = shown.borrow_mut();
        if *shown == error {
            false
        } else {
            *shown = error.clone();
            true
        }
    });
    if !changed {
        return;
    }

    update_tray_tooltip(hwnd, error.as_deref());
    if !recording {
        update_tray_icon(hwnd, make_tray_icon(0.0, false, error.is_some()));
    }

    let tooltip = BUBBLE_TOOLTIP.load(Ordering::SeqCst) as HWND;
    if tooltip != null_mut() {
        let mut text = wide_null(&match &error {
            Some(reason) => format!(
                "No microphone available\r\nReason: {reason}\r\n\r\n\
                 Connect or enable a microphone (Windows Settings > System > Sound > Input). \
                 It is picked up automatically."
            ),
            None => String::new(),
        });
        let info = bubble_tool_info(hwnd, text.as_mut_ptr());
        SendMessageW(tooltip, TTM_UPDATETIPTEXTW, 0, &info as *const _ as LPARAM);
        SendMessageW(tooltip, TTM_ACTIVATE, usize::from(error.is_some()), 0);
    }

    if error.is_some() {
        SetTimer(hwnd, MIC_RETRY_TIMER_ID, MIC_RETRY_INTERVAL_MS, None);
    } else {
        KillTimer(hwnd, MIC_RETRY_TIMER_ID);
    }
}

/// Mirrored, rounded, anti-aliased waveform bars centred in the pill.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_wave_bars(
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

pub(crate) unsafe fn paint(hwnd: HWND) {
    // Bubble content is delivered by UpdateLayeredWindow, not GDI. Validate the
    // paint cycle and push a fresh frame so it stays correct after restores.
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    BeginPaint(hwnd, &mut ps);
    EndPaint(hwnd, &ps);
    render_bubble(hwnd);
}

pub(crate) static ANIM_TICK: AtomicUsize = AtomicUsize::new(0);
pub(crate) static TRAY_ANIMATING: AtomicBool = AtomicBool::new(false);

/// 30 fps UI-thread heartbeat: scroll the waveform, repaint the bubble when there
/// is motion, and pulse the tray icon while recording. Keeps all GDI on the UI
/// thread; the audio thread only feeds levels.
pub(crate) unsafe fn on_anim_tick(hwnd: HWND) {
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
            update_tray_icon(hwnd, make_tray_icon(level, true, false));
        }
        TRAY_ANIMATING.store(true, Ordering::Relaxed);
    } else if TRAY_ANIMATING.swap(false, Ordering::Relaxed) {
        // Recording just ended (or animation was switched off): settle to idle.
        update_tray_icon(hwnd, make_idle_tray_icon(app));
    }
}

/// The work area (screen minus taskbar) of the monitor containing `point`, or
/// the primary monitor's for None.
pub(crate) unsafe fn work_area(point: Option<POINT>) -> RECT {
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
pub(crate) unsafe fn place_window(hwnd: HWND) {
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
pub(crate) unsafe fn cursor_rect() -> RECT {
    let mut point: POINT = std::mem::zeroed();
    GetCursorPos(&mut point);
    RECT {
        left: point.x,
        top: point.y,
        right: point.x,
        bottom: point.y,
    }
}

