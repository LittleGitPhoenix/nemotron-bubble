//! The application window: frame (header, tab bar, card, version), placement and the
//! dispatch of painting and clicks to the active tab.

mod history_tab;
mod model_notice;
mod model_tab;
mod settings_tab;
mod widgets;

pub(crate) use history_tab::*;
pub(crate) use model_notice::*;
pub(crate) use model_tab::*;
pub(crate) use settings_tab::*;
pub(crate) use widgets::*;

use crate::*;

// Application window: header, a tab bar (History | Model | Settings) and one card
// that shows the active tab. The width fits one column of settings rows.
pub(crate) const WINDOW_WIDTH: i32 = 392;
pub(crate) const WINDOW_HEIGHT: i32 = 816;
// Space below the card; holds the version label at the bottom right.
pub(crate) const WINDOW_FOOTER_H: i32 = 36;
pub(crate) const WINDOW_CARD_LEFT: i32 = 20;
pub(crate) const WINDOW_CARD_TOP: i32 = 118;
// Content area inside the card.
pub(crate) const WINDOW_INNER_LEFT: i32 = 40;
pub(crate) const WINDOW_INNER_RIGHT: i32 = WINDOW_WIDTH - 40;
pub(crate) const TAB_BAR_TOP: i32 = 78;
pub(crate) const TAB_BAR_BOTTOM: i32 = 108;
pub(crate) const TAB_LABELS: [&str; 3] = ["History", "Model", "Settings"];
pub(crate) const TAB_HISTORY: usize = 0;
pub(crate) const TAB_MODEL: usize = 1;
pub(crate) const TAB_SETTINGS: usize = 2;
// Windows icon-font glyphs (Segoe MDL2 Assets / Segoe Fluent Icons).
pub(crate) const GLYPH_CLOSE: &str = "\u{E8BB}";
pub(crate) const SETTINGS_TOGGLE_STARTUP: usize = 2001;
pub(crate) const SETTINGS_TOGGLE_PRELOAD: usize = 2002;
pub(crate) const SETTINGS_TOGGLE_LIVE_TYPE: usize = 2003;
pub(crate) const SETTINGS_TOGGLE_FINAL_CLIPBOARD: usize = 2004;
pub(crate) const SETTINGS_TOGGLE_FINAL_PASTE: usize = 2005;
pub(crate) const SETTINGS_TOGGLE_SOUNDS: usize = 2006;
pub(crate) const SETTINGS_TOGGLE_WAVEFORM: usize = 2007;
pub(crate) const SETTINGS_TOGGLE_BUBBLE_CLICK_WINDOW: usize = 2008;
pub(crate) const SETTINGS_TOGGLE_FLOATING_BUBBLE: usize = 2009;
pub(crate) const SETTINGS_TOGGLE_TRAY_WAVEFORM: usize = 2010;
pub(crate) const SETTINGS_TOGGLE_ENTER_STOPS: usize = 2011;
pub(crate) const SETTINGS_TOGGLE_SILENCE_STOP: usize = 2012;

/// Show `menu` below a button of the application window and destroy it afterwards.
/// The choice arrives as WM_COMMAND and is handled by handle_command.
pub(crate) unsafe fn show_menu_below(app: &Arc<AppState>, menu: HMENU, button: RectI) {
    let hwnd = application_window_hwnd_from_app(app);
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

pub(crate) struct SettingRow {
    pub(crate) id: usize,
    pub(crate) title: &'static str,
    pub(crate) detail: &'static str,
    pub(crate) enabled: bool,
}

pub(crate) fn setting_rows(settings: &AppSettings) -> Vec<SettingRow> {
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
            detail: "As you speak. Text fields only.",
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
            id: SETTINGS_TOGGLE_BUBBLE_CLICK_WINDOW,
            title: "Bubble click opens window",
            detail: "Off makes a bubble click toggle recording.",
            enabled: settings.bubble_click_opens_window,
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

pub(crate) unsafe fn paint_application_window(hwnd: HWND) {
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
    let (status, model_status, model_notice) = {
        let ui = lock_ui(app);
        (ui.status.clone(), ui.model_status.clone(), ui.model_notice)
    };
    if let Some(mut canvas) = Canvas::new(WINDOW_WIDTH, WINDOW_HEIGHT) {
        let tab = app.settings_tab.load(Ordering::SeqCst);
        let items = history_snapshot(app);
        let view = HistoryView {
            items: &items,
            scroll: app.history_scroll.load(Ordering::SeqCst),
            copied: *app.copied_flash.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
        };
        let settings_view = SettingsTabView {
            scroll: settings_tab_scroll(app),
            microphone_in_use: open_microphone_name(),
        };
        let downloads = downloads_snapshot(app);
        let layout = render_application_window(
            &mut canvas,
            &settings,
            &status,
            &model_status,
            &view,
            &settings_view,
            &downloads,
            model_notice,
            capturing,
            tab,
        );
        canvas.blit_to(hdc);
        // Remember what the list looked like, for clicks and wheel scrolling.
        let history = layout.history;
        app.history_content_h.store(history.content_h, Ordering::SeqCst);
        if history.scroll != view.scroll {
            app.history_scroll.store(history.scroll, Ordering::SeqCst);
        }
        *app.history_hits.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = history.hits;
        *app.model_notice_callout.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
            layout.model_notice_callout;
    }
    EndPaint(hwnd, &ps);
}

/// What opened the application window; decides where it appears.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowOrigin {
    /// A click on the floating bubble: open next to the bubble.
    Bubble,
    /// A click on the tray icon: open next to the tray (at the cursor).
    Tray,
}

/// Position the application window next to what opened it, kept inside that
/// monitor's work area: above the anchor when it is in the lower half of the
/// screen (bubble at the bottom, taskbar at the bottom), otherwise below it.
pub(crate) unsafe fn place_application_window(hwnd: HWND, origin: WindowOrigin) {
    const GAP: i32 = 8;
    // Anchor rectangle: the bubble's pill, or a point at the cursor (tray click).
    let anchor = match origin {
        WindowOrigin::Bubble => {
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
        WindowOrigin::Tray => cursor_rect(),
    };
    let center = POINT {
        x: (anchor.left + anchor.right) / 2,
        y: (anchor.top + anchor.bottom) / 2,
    };
    let work = work_area(Some(center));

    let x = (center.x - WINDOW_WIDTH / 2)
        .min(work.right - WINDOW_WIDTH - GAP)
        .max(work.left + GAP);
    let in_lower_half = center.y > (work.top + work.bottom) / 2;
    let y = if in_lower_half {
        anchor.top - WINDOW_HEIGHT - GAP
    } else {
        anchor.bottom + GAP
    }
    .min(work.bottom - WINDOW_HEIGHT - GAP)
    .max(work.top + GAP);

    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        x,
        y,
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        SWP_NOACTIVATE,
    );
}

pub(crate) unsafe fn apply_application_window_rounding(hwnd: HWND) {
    let region = CreateRoundRectRgn(0, 0, WINDOW_WIDTH + 1, WINDOW_HEIGHT + 1, 18, 18);
    if region != null_mut() {
        SetWindowRgn(hwnd, region, 1);
    }
}

/// Open the application window if hidden, close it if already showing.
pub(crate) fn toggle_application_window(origin: WindowOrigin) {
    let Some(app) = APP.get() else {
        return;
    };
    unsafe {
        let hwnd = application_window_hwnd_from_app(app);
        if hwnd == null_mut() {
            return;
        }
        if IsWindowVisible(hwnd) != 0 {
            cancel_hotkey_capture(app);
            ShowWindow(hwnd, SW_HIDE);
        } else {
            show_application_window(origin);
        }
    }
}

/// Open the application window next to `origin`, or bring it to the front if it is already open.
pub(crate) fn show_application_window(origin: WindowOrigin) {
    let Some(app) = APP.get() else {
        return;
    };
    unsafe {
        let hwnd = application_window_hwnd_from_app(app);
        if hwnd == null_mut() {
            return;
        }
        if IsWindowVisible(hwnd) == 0 {
            place_application_window(hwnd, origin);
            ShowWindow(hwnd, SW_SHOW);
        }
        SetForegroundWindow(hwnd);
    }
}

pub(crate) fn is_application_window(hwnd: HWND) -> bool {
    APP.get()
        .map(|app| hwnd == application_window_hwnd_from_app(app))
        .unwrap_or(false)
}

/// The close "X" in the top-right corner of the header.
pub(crate) fn close_button_rect() -> RectI {
    RectI {
        left: WINDOW_WIDTH - 54,
        top: 18,
        right: WINDOW_WIDTH - 14,
        bottom: 50,
    }
}

/// One tab of the tab bar (History | Model | Settings), spanning the card width.
pub(crate) fn settings_tab_rect(tab: usize) -> RectI {
    let width = (WINDOW_WIDTH - 2 * WINDOW_CARD_LEFT) / TAB_LABELS.len() as i32;
    let left = WINDOW_CARD_LEFT + tab as i32 * width;
    RectI {
        left,
        top: TAB_BAR_TOP,
        right: left + width,
        bottom: TAB_BAR_BOTTOM,
    }
}

/// The row of a toggle setting: Preload sits on the Model tab, all others on
/// the Settings tab below the shortcut row.
pub(crate) fn toggle_rect(id: usize) -> RectI {
    let top = if id == SETTINGS_TOGGLE_PRELOAD {
        PRELOAD_ROW_TOP
    } else {
        let index = SETTINGS_TAB_TOGGLES.iter().position(|t| *t == id).unwrap_or(0);
        SETTINGS_TAB_TOGGLES_TOP + index as i32 * 49
    };
    RectI {
        left: 32,
        top,
        right: WINDOW_WIDTH - 36,
        bottom: top + 44,
    }
}

/// Switch the application window to `tab`.
pub(crate) fn set_settings_tab(app: &Arc<AppState>, tab: usize) {
    app.settings_tab.store(tab, Ordering::SeqCst);
    unsafe {
        let hwnd = application_window_hwnd_from_app(app);
        if hwnd != null_mut() {
            InvalidateRect(hwnd, null(), 0);
        }
    }
}


pub(crate) fn handle_application_window_click(x: i32, y: i32) {
    let Some(app) = APP.get() else {
        return;
    };

    let tab = app.settings_tab.load(Ordering::SeqCst);

    // Shortcut rebind button: click to begin capture, click again to cancel.
    if tab == TAB_SETTINGS && handle_shortcut_click(app, x, y) {
        return;
    }
    // Any other click ends an in-progress capture before doing its thing.
    if app.capturing_hotkey.load(Ordering::SeqCst) {
        cancel_hotkey_capture(app);
    }

    if close_button_rect().contains(x, y) {
        unsafe {
            let hwnd = application_window_hwnd_from_app(app);
            if hwnd != null_mut() {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
        return;
    }

    for index in 0..TAB_LABELS.len() {
        if settings_tab_rect(index).contains(x, y) {
            set_settings_tab(app, index);
            return;
        }
    }

    match tab {
        TAB_HISTORY => handle_history_tab_click(app, x, y),
        TAB_MODEL => {
            if !handle_model_notice_click(app, x, y) {
                handle_model_tab_click(app, x, y);
            }
        }
        _ => handle_settings_tab_click(app, x, y),
    }
}

/// What a paint of the application window leaves behind for clicks and scrolling.
pub(crate) struct WindowLayout {
    /// The History list (copy-icon hit areas, content height), empty on the other tabs.
    pub(crate) history: HistoryLayout,
    /// The callout of the model notice, when it was drawn.
    pub(crate) model_notice_callout: Option<RectI>,
}

/// Render the entire application window into a canvas: the frame (header, tab bar,
/// card, version footer) and the content of the active `tab`. `status` is the latest
/// status message, shown under the title. `model_notice` is drawn over the Model tab.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn render_application_window(
    canvas: &mut Canvas,
    settings: &AppSettings,
    status: &str,
    model_status: &str,
    history: &HistoryView,
    settings_view: &SettingsTabView,
    downloads: &[ModelDownloadState],
    model_notice: Option<ModelNotice>,
    capturing: bool,
    tab: usize,
) -> WindowLayout {
    let rows = setting_rows(settings);
    let cw = canvas.w;
    let ch = canvas.h;
    let card_left = WINDOW_CARD_LEFT as f32;
    let card_top = WINDOW_CARD_TOP as f32;
    let card_w = (cw - 2 * WINDOW_CARD_LEFT) as f32;
    let card_h = (ch - WINDOW_CARD_TOP - WINDOW_FOOTER_H) as f32;

    // History list geometry: measure first (needs the DC), then draw.
    let mut list = if tab == TAB_HISTORY {
        measure_history_list(canvas.hdc, history)
    } else {
        HistoryList::default()
    };
    let notice = match model_notice {
        Some(notice) if tab == TAB_MODEL => Some(measure_model_notice(canvas.hdc, notice)),
        _ => None,
    };

    // -------- pass 1: anti-aliased shapes straight into the pixel buffer --------
    {
        let buf = canvas.pixels();
        fill_round_rect(buf, cw, ch, 0.0, 0.0, cw as f32, ch as f32, 0.0, (25, 27, 33), (16, 17, 21), 1.0);
        fill_round_rect(buf, cw, ch, 28.0, 26.0, 4.0, 32.0, 2.0, (110, 162, 255), (84, 138, 250), 1.0);

        // tab bar: thin baseline, accent underline below the active tab
        let base_y = TAB_BAR_BOTTOM as f32;
        fill_round_rect(buf, cw, ch, card_left, base_y - 1.0, card_w, 1.0, 0.0, (44, 48, 58), (44, 48, 58), 1.0);
        let active = settings_tab_rect(tab);
        let underline_w = 56.0;
        let underline_x = (active.left + active.right) as f32 / 2.0 - underline_w / 2.0;
        fill_round_rect(buf, cw, ch, underline_x, base_y - 3.0, underline_w, 3.0, 1.5, (110, 162, 255), (84, 138, 250), 1.0);

        // the card of the active tab
        fill_round_rect(buf, cw, ch, card_left, card_top, card_w, card_h, 16.0, (30, 33, 41), (25, 28, 35), 1.0);

        match tab {
            TAB_HISTORY => draw_history_shapes(buf, cw, ch, &list),
            TAB_MODEL => draw_model_shapes(buf, cw, ch, settings, downloads),
            _ => draw_settings_tab_shapes(buf, cw, ch, &rows, settings_view, capturing),
        }
    }

    // -------- pass 2: ClearType text on top via GDI --------
    let dc = canvas.hdc;
    SetBkMode(dc, TRANSPARENT as i32);

    SelectObject(dc, title_font() as _);
    SetTextColor(dc, rgb(237, 241, 247));
    text_left(dc, "Nemotron Bubble", 42, 22, cw - 110, 34);

    // Status line between the title and the tab bar, cut off with "..." when too long.
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(140, 150, 164));
    text_left_ellipsis(
        dc,
        status,
        RectI {
            left: 42,
            top: 57,
            right: cw - 34,
            bottom: TAB_BAR_TOP - 2,
        },
    );

    // close: the ChromeClose glyph of the Windows icon font, like a title bar's X
    SelectObject(dc, icon_font() as _);
    SetTextColor(dc, rgb(205, 212, 224));
    text_center(dc, GLYPH_CLOSE, close_button_rect());

    // tab labels
    SelectObject(dc, ui_font() as _);
    for (index, label) in TAB_LABELS.iter().enumerate() {
        if index == tab {
            SetTextColor(dc, rgb(245, 248, 252));
        } else {
            SetTextColor(dc, rgb(140, 150, 164));
        }
        let rect = settings_tab_rect(index);
        text_center(dc, label, RectI { bottom: rect.bottom - 4, ..rect });
    }

    match tab {
        TAB_HISTORY => draw_history_text(dc, history, &mut list),
        TAB_MODEL => draw_model_text(dc, settings, model_status, &rows, downloads),
        _ => draw_settings_tab_text(dc, settings, &rows, settings_view, capturing),
    }
    // The model notice goes on top of the finished Model tab, so its scrim dims
    // the tab's texts too: shapes into the pixels (after GDI is done), then its texts.
    if let Some(notice) = &notice {
        GdiFlush();
        draw_model_notice_shapes(canvas.pixels(), cw, ch, notice);
        draw_model_notice_text(dc, notice);
    }

    // version footer, bottom right below the card
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(108, 118, 132));
    text_right(
        dc,
        &format!("Version {APP_VERSION}"),
        RectI {
            left: cw - 236,
            top: ch - WINDOW_FOOTER_H + 10,
            right: cw - 34,
            bottom: ch - WINDOW_FOOTER_H + 28,
        },
    );

    WindowLayout {
        history: list.layout,
        model_notice_callout: notice.map(|notice| notice.callout),
    }
}
