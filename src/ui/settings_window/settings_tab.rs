//! The Settings tab: global shortcut, behavior toggles and the floating bubble options.

use crate::*;

// Settings tab
pub(crate) const SETTINGS_TAB_TOGGLES_TOP: i32 = 180;
// The bubble position row follows the last toggle.
pub(crate) const BUBBLE_POSITION_ROW_TOP: i32 = SETTINGS_TAB_TOGGLES_TOP + SETTINGS_TAB_TOGGLES.len() as i32 * 49;

/// Popup menu with the bubble position presets, current one checked.
pub(crate) unsafe fn show_bubble_position_menu(app: &Arc<AppState>) {
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

/// The clickable shortcut-rebind button at the top of the Settings tab.
pub(crate) fn settings_shortcut_rect() -> RectI {
    RectI {
        left: SETTINGS_INNER_RIGHT - 150,
        top: SETTINGS_CARD_TOP + 12,
        right: SETTINGS_INNER_RIGHT,
        bottom: SETTINGS_CARD_TOP + 46,
    }
}

/// The toggles of the Settings tab, top to bottom. The bubble settings come
/// last, followed by the bubble position row.
pub(crate) const SETTINGS_TAB_TOGGLES: [usize; 11] = [
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
pub(crate) fn settings_bubble_position_rect() -> RectI {
    RectI {
        left: SETTINGS_INNER_RIGHT - 150,
        top: BUBBLE_POSITION_ROW_TOP + 6,
        right: SETTINGS_INNER_RIGHT,
        bottom: BUBBLE_POSITION_ROW_TOP + 38,
    }
}


/// Click on the shortcut button: start capturing a new combo, or cancel a
/// running capture. Returns false when the click was elsewhere.
pub(crate) fn handle_shortcut_click(app: &Arc<AppState>, x: i32, y: i32) -> bool {
    if !settings_shortcut_rect().contains(x, y) {
        return false;
    }
    if app.capturing_hotkey.load(Ordering::SeqCst) {
        cancel_hotkey_capture(app);
    } else {
        start_hotkey_capture(app);
    }
    true
}

pub(crate) fn handle_settings_tab_click(app: &Arc<AppState>, x: i32, y: i32) {
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

/// Pass 1 of the Settings tab: shortcut button, toggle switches, Bubble position button.
pub(crate) fn draw_settings_tab_shapes(buf: &mut [u32], cw: i32, ch: i32, rows: &[SettingRow], capturing: bool) {
    // shortcut rebind button (accent while capturing)
    draw_button_bg(buf, cw, ch, settings_shortcut_rect(), capturing);
    for row in rows.iter().filter(|row| row.id != SETTINGS_TOGGLE_PRELOAD) {
        draw_toggle(buf, cw, ch, settings_toggle_rect(row.id), row.enabled);
    }
    draw_button_bg(buf, cw, ch, settings_bubble_position_rect(), false);
}

/// Pass 2 of the Settings tab: shortcut row, toggle texts and the Bubble position row.
pub(crate) unsafe fn draw_settings_tab_text(dc: HDC, settings: &AppSettings, rows: &[SettingRow], capturing: bool) {
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
