//! The Settings tab: global shortcut, microphone, behavior toggles and the floating
//! bubble options. The tab is taller than the card, so it scrolls.

use crate::*;

// Layout in content coordinates, which equal window coordinates at scroll offset 0.
pub(crate) const MICROPHONE_SECTION_TOP: i32 = SETTINGS_CARD_TOP + 62;
pub(crate) const SETTINGS_TAB_TOGGLES_TOP: i32 = MICROPHONE_SECTION_TOP + 94;
// The bubble position row follows the last toggle.
pub(crate) const BUBBLE_POSITION_ROW_TOP: i32 = SETTINGS_TAB_TOGGLES_TOP + SETTINGS_TAB_TOGGLES.len() as i32 * 49;
// The visible part of the tab is the card. The rest is reached by scrolling.
pub(crate) const SETTINGS_VIEW_TOP: i32 = SETTINGS_CARD_TOP;
pub(crate) const SETTINGS_VIEW_BOTTOM: i32 = SETTINGS_HEIGHT - SETTINGS_FOOTER_H;
// Bottom of the content: the bubble position button plus the card's padding.
pub(crate) const SETTINGS_CONTENT_BOTTOM: i32 = BUBBLE_POSITION_ROW_TOP + 38 + 12;
pub(crate) const SETTINGS_MAX_SCROLL: i32 = if SETTINGS_CONTENT_BOTTOM > SETTINGS_VIEW_BOTTOM {
    SETTINGS_CONTENT_BOTTOM - SETTINGS_VIEW_BOTTOM
} else {
    0
};

/// What the Settings tab shows besides the settings: how far it is scrolled and
/// which input device the open microphone stream records from.
pub(crate) struct SettingsTabView {
    pub(crate) scroll: i32,
    pub(crate) microphone_in_use: Option<String>,
}

/// The current scroll offset of the Settings tab.
pub(crate) fn settings_tab_scroll(app: &Arc<AppState>) -> i32 {
    app.settings_scroll.load(Ordering::SeqCst).clamp(0, SETTINGS_MAX_SCROLL)
}

/// Scroll the Settings tab by `delta` pixels (mouse wheel).
pub(crate) fn scroll_settings_tab(app: &Arc<AppState>, delta: i32) {
    let old = settings_tab_scroll(app);
    let new = (old + delta).clamp(0, SETTINGS_MAX_SCROLL);
    if new != old {
        app.settings_scroll.store(new, Ordering::SeqCst);
        unsafe {
            let hwnd = settings_hwnd_from_app(app);
            if hwnd != null_mut() {
                InvalidateRect(hwnd, null(), 0);
            }
        }
    }
}

/// Content y of a click at window y, or None when the click is outside the card.
fn settings_content_y(app: &Arc<AppState>, y: i32) -> Option<i32> {
    (SETTINGS_VIEW_TOP..SETTINGS_VIEW_BOTTOM)
        .contains(&y)
        .then(|| y + settings_tab_scroll(app))
}

/// Where a content rectangle currently is in the window.
fn settings_on_screen(app: &Arc<AppState>, rect: RectI) -> RectI {
    rect.shifted(-settings_tab_scroll(app))
}

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
    show_menu_below(app, menu, settings_on_screen(app, settings_bubble_position_rect()));
}

/// Popup menu with "Windows default" and the connected input devices, current
/// one checked. A selected device that is not connected is listed greyed out.
pub(crate) unsafe fn show_microphone_menu(app: &Arc<AppState>) {
    let menu = CreatePopupMenu();
    if menu == null_mut() {
        return;
    }
    let current = settings_snapshot(app).microphone;
    let mut names = input_device_names();
    names.truncate(IDM_MIC_MAX - 1);

    let checked = |selected: bool| if selected { MF_STRING | MF_CHECKED } else { MF_STRING };
    let text = wide_null("Windows default");
    AppendMenuW(menu, checked(current.is_empty()), IDM_MIC_BASE, text.as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    if names.is_empty() {
        let text = wide_null("No microphone found");
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, text.as_ptr());
    }
    for (index, name) in names.iter().enumerate() {
        // "&" would turn the next letter into a menu accelerator
        let text = wide_null(&name.replace('&', "&&"));
        AppendMenuW(menu, checked(*name == current), IDM_MIC_BASE + 1 + index, text.as_ptr());
    }
    if !current.is_empty() && !names.contains(&current) {
        let text = wide_null(&format!("{} (not connected)", current.replace('&', "&&")));
        AppendMenuW(menu, MF_STRING | MF_GRAYED | MF_CHECKED, 0, text.as_ptr());
    }
    MICROPHONE_MENU.with(|menu_names| *menu_names.borrow_mut() = names);
    show_menu_below(app, menu, settings_on_screen(app, settings_microphone_rect()));
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

/// The full-width microphone button (opens the device popup menu).
pub(crate) fn settings_microphone_rect() -> RectI {
    section_track_rect(MICROPHONE_SECTION_TOP)
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
    let Some(y) = settings_content_y(app, y) else {
        return false;
    };
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
    let Some(y) = settings_content_y(app, y) else {
        return;
    };
    for id in SETTINGS_TAB_TOGGLES {
        if settings_toggle_rect(id).contains(x, y) {
            toggle_setting(app, id);
            return;
        }
    }
    if settings_microphone_rect().contains(x, y) {
        unsafe { show_microphone_menu(app) };
    } else if settings_bubble_position_rect().contains(x, y) {
        unsafe { show_bubble_position_menu(app) };
    }
}

/// Pass 1 of the Settings tab: shortcut button, microphone button, toggle
/// switches, Bubble position button (all clipped to the card), and the scrollbar.
pub(crate) fn draw_settings_tab_shapes(
    buf: &mut [u32],
    cw: i32,
    ch: i32,
    rows: &[SettingRow],
    view: &SettingsTabView,
    capturing: bool,
) {
    let view_h = SETTINGS_VIEW_BOTTOM - SETTINGS_VIEW_TOP;
    let start = (SETTINGS_VIEW_TOP * cw) as usize;
    let end = (SETTINGS_VIEW_BOTTOM * cw) as usize;
    if end > buf.len() {
        return;
    }
    {
        // Drawing into the card's rows only clips everything scrolled out of it.
        let card = &mut buf[start..end];
        let dy = -view.scroll - SETTINGS_VIEW_TOP;
        // shortcut rebind button (accent while capturing)
        draw_button_bg(card, cw, view_h, settings_shortcut_rect().shifted(dy), capturing);
        draw_button_bg(card, cw, view_h, settings_microphone_rect().shifted(dy), false);
        for row in rows.iter().filter(|row| row.id != SETTINGS_TOGGLE_PRELOAD) {
            draw_toggle(card, cw, view_h, settings_toggle_rect(row.id).shifted(dy), row.enabled);
        }
        draw_button_bg(card, cw, view_h, settings_bubble_position_rect().shifted(dy), false);
    }
    // scrollbar inside the card's padding, on the same line as the History tab's
    let content_h = SETTINGS_CONTENT_BOTTOM - SETTINGS_VIEW_TOP;
    let track_x = SETTINGS_INNER_RIGHT + 4;
    draw_scrollbar(buf, cw, ch, track_x, SETTINGS_VIEW_TOP + 12, view_h - 24, content_h - 24, view.scroll);
}

/// Pass 2 of the Settings tab: shortcut row, microphone section, toggle texts
/// and the Bubble position row, clipped to the card.
pub(crate) unsafe fn draw_settings_tab_text(
    dc: HDC,
    settings: &AppSettings,
    rows: &[SettingRow],
    view: &SettingsTabView,
    capturing: bool,
) {
    let dy = -view.scroll;
    IntersectClipRect(dc, SETTINGS_CARD_LEFT, SETTINGS_VIEW_TOP, SETTINGS_WIDTH - SETTINGS_CARD_LEFT, SETTINGS_VIEW_BOTTOM);

    // shortcut row
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(233, 238, 244));
    text_left(dc, "Global shortcut", 40, SETTINGS_CARD_TOP + 13 + dy, 160, 20);
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(124, 134, 148));
    text_left(dc, "Click, then press your keys.", 40, SETTINGS_CARD_TOP + 33 + dy, 160, 18);
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(245, 248, 252));
    let shortcut_label = if capturing {
        "Press keys...".to_string()
    } else {
        hotkey_label(settings.hotkey_mods, settings.hotkey_vk)
    };
    text_center(dc, &shortcut_label, settings_shortcut_rect().shifted(dy));

    // microphone: heading with the device in use, then the device button
    let in_use = match &view.microphone_in_use {
        Some(name) => format!("In use: {name}."),
        None => "No microphone is open.".to_string(),
    };
    section_heading(dc, MICROPHONE_SECTION_TOP + dy, "Microphone", &in_use);
    let button = settings_microphone_rect().shifted(dy);
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(245, 248, 252));
    text_left_ellipsis(
        dc,
        microphone_label(&settings.microphone),
        RectI {
            left: button.left + 14,
            right: button.right - 34,
            ..button
        },
    );
    text_center(
        dc,
        "\u{25BE}",
        RectI {
            left: button.right - 34,
            right: button.right - 10,
            ..button
        },
    );

    for row in rows.iter().filter(|row| row.id != SETTINGS_TOGGLE_PRELOAD) {
        draw_toggle_text(dc, row, settings_toggle_rect(row.id).shifted(dy));
    }

    // bubble position: label + detail on the left, popup button on the right
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(233, 238, 244));
    text_left(dc, "Bubble position", 40, BUBBLE_POSITION_ROW_TOP + 1 + dy, 160, 20);
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(124, 134, 148));
    text_left(dc, "Preset spot on the screen.", 40, BUBBLE_POSITION_ROW_TOP + 21 + dy, 160, 18);
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(245, 248, 252));
    text_center(
        dc,
        &format!("{}  \u{25BE}", settings.bubble_position.label()),
        settings_bubble_position_rect().shifted(dy),
    );

    SelectClipRgn(dc, null_mut());
}
