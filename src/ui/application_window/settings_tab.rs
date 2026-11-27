//! The Settings tab: global shortcut, microphone, behavior toggles and the floating
//! bubble options. The tab is taller than the card, so it scrolls.

use crate::*;

// Layout in content coordinates, which equal window coordinates at scroll offset 0.
pub(crate) const MICROPHONE_SECTION_TOP: i32 = WINDOW_CARD_TOP + 62;
pub(crate) const SETTINGS_TAB_TOGGLES_TOP: i32 = MICROPHONE_SECTION_TOP + 94;
// The auto-stop row sits between the switches for dictation and those for the bubble.
pub(crate) const AUTO_STOP_ROW_INDEX: usize = 8;
pub(crate) const AUTO_STOP_ROW_TOP: i32 = settings_row_top(AUTO_STOP_ROW_INDEX);
// The bubble position row follows the switches and the auto-stop row.
pub(crate) const BUBBLE_POSITION_ROW_TOP: i32 = settings_row_top(SETTINGS_TAB_TOGGLES.len() + 1);
// The visible part of the tab is the card. The rest is reached by scrolling.
pub(crate) const SETTINGS_VIEW_TOP: i32 = WINDOW_CARD_TOP;
pub(crate) const SETTINGS_VIEW_BOTTOM: i32 = WINDOW_HEIGHT - WINDOW_FOOTER_H;
// The bubble position picker: a small screen with a taskbar strip at the bottom and
// six clickable spots (two rows of three) above it.
const PICKER_WIDTH: i32 = 96;
const PICKER_HEIGHT: i32 = 42;
const PICKER_TASKBAR_HEIGHT: i32 = 6;
const PICKER_COLUMNS: i32 = 3;
// The auto-stop button fits its widest value ("5 min") plus padding.
const AUTO_STOP_BUTTON_WIDTH: i32 = 88;
// Bottom of the content: the bubble position picker plus the card's padding.
pub(crate) const SETTINGS_CONTENT_BOTTOM: i32 = BUBBLE_POSITION_ROW_TOP + 3 + PICKER_HEIGHT + 12;
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
            let hwnd = application_window_hwnd_from_app(app);
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

/// Popup menu with the auto-stop durations, current one checked.
pub(crate) unsafe fn show_auto_stop_menu(app: &Arc<AppState>) {
    let menu = CreatePopupMenu();
    if menu == null_mut() {
        return;
    }
    let current = settings_snapshot(app).silence_auto_stop_secs;
    for (index, seconds) in SILENCE_AUTO_STOP_CHOICES.iter().enumerate() {
        let mut flags = MF_STRING;
        if *seconds == current {
            flags |= MF_CHECKED;
        }
        append_menu_item(menu, flags, IDM_AUTO_STOP_BASE + index, &silence_auto_stop_label(*seconds));
        if index == 0 {
            append_menu_separator(menu); // between Off and the durations
        }
    }
    show_menu_below(app, menu, settings_on_screen(app, settings_auto_stop_rect()));
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
    append_menu_item(menu, checked(current.is_empty()), IDM_MIC_BASE, "Windows default");
    append_menu_separator(menu);
    if names.is_empty() {
        append_menu_item(menu, MF_STRING | MF_GRAYED, 0, "No microphone found");
    }
    for (index, name) in names.iter().enumerate() {
        append_menu_item(menu, checked(*name == current), IDM_MIC_BASE + 1 + index, name);
    }
    if !current.is_empty() && !names.contains(&current) {
        let text = format!("{current} (not connected)");
        append_menu_item(menu, MF_STRING | MF_GRAYED | MF_CHECKED, 0, &text);
    }
    MICROPHONE_MENU.with(|menu_names| *menu_names.borrow_mut() = names);
    show_menu_below(app, menu, settings_on_screen(app, settings_microphone_rect()));
}

/// The clickable shortcut-rebind button at the top of the Settings tab.
pub(crate) fn settings_shortcut_rect() -> RectI {
    RectI {
        left: WINDOW_INNER_RIGHT - 150,
        top: WINDOW_CARD_TOP + 12,
        right: WINDOW_INNER_RIGHT,
        bottom: WINDOW_CARD_TOP + 46,
    }
}

/// The full-width microphone button (opens the device popup menu).
pub(crate) fn settings_microphone_rect() -> RectI {
    section_track_rect(MICROPHONE_SECTION_TOP)
}

/// The toggles of the Settings tab, top to bottom. The auto-stop row is inserted at
/// AUTO_STOP_ROW_INDEX. The bubble settings come last, followed by the bubble position row.
pub(crate) const SETTINGS_TAB_TOGGLES: [usize; 10] = [
    SETTINGS_TOGGLE_STARTUP,
    SETTINGS_TOGGLE_LIVE_TYPE,
    SETTINGS_TOGGLE_FINAL_CLIPBOARD,
    SETTINGS_TOGGLE_FINAL_PASTE,
    SETTINGS_TOGGLE_SOUNDS,
    SETTINGS_TOGGLE_WAVEFORM,
    SETTINGS_TOGGLE_TRAY_WAVEFORM,
    SETTINGS_TOGGLE_ENTER_STOPS,
    SETTINGS_TOGGLE_FLOATING_BUBBLE,
    SETTINGS_TOGGLE_BUBBLE_CLICK_WINDOW,
];

/// Content y of the row at `index` in the list of switches and drop-down rows.
pub(crate) const fn settings_row_top(index: usize) -> i32 {
    SETTINGS_TAB_TOGGLES_TOP + index as i32 * 49
}

/// The auto-stop button (opens the duration popup menu), right of its label.
pub(crate) fn settings_auto_stop_rect() -> RectI {
    RectI {
        left: WINDOW_INNER_RIGHT - AUTO_STOP_BUTTON_WIDTH,
        top: AUTO_STOP_ROW_TOP + 6,
        right: WINDOW_INNER_RIGHT,
        bottom: AUTO_STOP_ROW_TOP + 38,
    }
}

/// The small screen of the bubble position picker, right of its label.
pub(crate) fn bubble_position_picker_rect() -> RectI {
    let top = BUBBLE_POSITION_ROW_TOP + 3;
    RectI {
        left: WINDOW_INNER_RIGHT - PICKER_WIDTH,
        top,
        right: WINDOW_INNER_RIGHT,
        bottom: top + PICKER_HEIGHT,
    }
}

/// The clickable cell of one position in the picker. Cells are in BubblePosition::ALL
/// order: the top row left to right, then the bottom row.
pub(crate) fn bubble_position_spot_rect(position: BubblePosition) -> RectI {
    let index = BubblePosition::ALL.iter().position(|p| *p == position).unwrap_or(0) as i32;
    let picker = bubble_position_picker_rect();
    let cell_width = PICKER_WIDTH / PICKER_COLUMNS;
    let cell_height = (PICKER_HEIGHT - PICKER_TASKBAR_HEIGHT) / 2;
    let left = picker.left + (index % PICKER_COLUMNS) * cell_width;
    let top = picker.top + (index / PICKER_COLUMNS) * cell_height;
    RectI {
        left,
        top,
        right: left + cell_width,
        bottom: top + cell_height,
    }
}

/// The picker at `picker` (already shifted to the drawing position): a screen with a
/// taskbar strip and a small pill per position, the selected one in the accent color.
fn draw_bubble_position_picker(buf: &mut [u32], cw: i32, ch: i32, picker: RectI, selected: BubblePosition, dy: i32) {
    let frame = (66, 72, 84);
    let (x, y) = (picker.left as f32, picker.top as f32);
    let (w, h) = (PICKER_WIDTH as f32, PICKER_HEIGHT as f32);
    // The frame is the outer shape, the screen surface is inset by its width.
    fill_round_rect(buf, cw, ch, x, y, w, h, 7.0, frame, frame, 1.0);
    fill_round_rect(buf, cw, ch, x + 1.5, y + 1.5, w - 3.0, h - 3.0, 5.5, (36, 40, 49), (36, 40, 49), 1.0);
    let taskbar = PICKER_TASKBAR_HEIGHT as f32;
    fill_round_rect(buf, cw, ch, x + 1.5, y + h - taskbar - 1.5, w - 3.0, taskbar, 2.0, frame, frame, 1.0);
    for position in BubblePosition::ALL {
        let cell = bubble_position_spot_rect(position).shifted(dy);
        let (cx, cy) = ((cell.left + cell.right) as f32 / 2.0, (cell.top + cell.bottom) as f32 / 2.0);
        if position == selected {
            fill_round_rect(buf, cw, ch, cx - 9.0, cy - 4.0, 18.0, 8.0, 4.0, (110, 162, 255), (84, 138, 250), 1.0);
        } else {
            fill_round_rect(buf, cw, ch, cx - 8.0, cy - 3.5, 16.0, 7.0, 3.5, frame, frame, 1.0);
        }
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
        if toggle_rect(id).contains(x, y) {
            toggle_setting(app, id);
            return;
        }
    }
    if settings_microphone_rect().contains(x, y) {
        unsafe { show_microphone_menu(app) };
    } else if settings_auto_stop_rect().contains(x, y) {
        unsafe { show_auto_stop_menu(app) };
    } else if let Some(position) = BubblePosition::ALL
        .into_iter()
        .find(|position| bubble_position_spot_rect(*position).contains(x, y))
    {
        select_bubble_position(app, position);
    }
}

/// Pass 1 of the Settings tab: shortcut button, microphone button, toggle switches,
/// auto-stop button and bubble position picker (all clipped to the card), and the scrollbar.
pub(crate) fn draw_settings_tab_shapes(
    buf: &mut [u32],
    cw: i32,
    ch: i32,
    settings: &AppSettings,
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
            draw_toggle(card, cw, view_h, toggle_rect(row.id).shifted(dy), row.enabled);
        }
        draw_button_bg(card, cw, view_h, settings_auto_stop_rect().shifted(dy), false);
        let picker = bubble_position_picker_rect().shifted(dy);
        draw_bubble_position_picker(card, cw, view_h, picker, settings.bubble_position, dy);
    }
    // scrollbar inside the card's padding, on the same line as the History tab's
    let content_h = SETTINGS_CONTENT_BOTTOM - SETTINGS_VIEW_TOP;
    let track_x = WINDOW_INNER_RIGHT + 4;
    draw_scrollbar(buf, cw, ch, track_x, SETTINGS_VIEW_TOP + 12, view_h - 24, content_h - 24, view.scroll);
}

/// Pass 2 of the Settings tab: shortcut row, microphone section, toggle texts
/// and the auto-stop and Bubble position rows, clipped to the card.
pub(crate) unsafe fn draw_settings_tab_text(
    dc: HDC,
    settings: &AppSettings,
    rows: &[SettingRow],
    view: &SettingsTabView,
    capturing: bool,
) {
    let dy = -view.scroll;
    IntersectClipRect(dc, WINDOW_CARD_LEFT, SETTINGS_VIEW_TOP, WINDOW_WIDTH - WINDOW_CARD_LEFT, SETTINGS_VIEW_BOTTOM);

    // shortcut row
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(233, 238, 244));
    text_left(dc, "Global shortcut", 40, WINDOW_CARD_TOP + 13 + dy, 160, 20);
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(124, 134, 148));
    text_left(dc, "Click, then press your keys.", 40, WINDOW_CARD_TOP + 33 + dy, 160, 18);
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
        draw_toggle_text(dc, row, toggle_rect(row.id).shifted(dy));
    }

    // auto-stop: label + detail on the left, popup button on the right
    row_label(dc, AUTO_STOP_ROW_TOP + dy, "Auto-stop on silence", "Ends a forgotten dictation.");
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(245, 248, 252));
    let value = silence_auto_stop_label(settings.silence_auto_stop_secs);
    text_center(dc, &format!("{value}  \u{25BE}"), settings_auto_stop_rect().shifted(dy));

    // bubble position: the detail names the position, the picker is drawn in pass 1
    let position = format!("{}.", settings.bubble_position.label());
    row_label(dc, BUBBLE_POSITION_ROW_TOP + dy, "Bubble position", &position);

    SelectClipRgn(dc, null_mut());
}

/// Title and detail line of a row at `row_top` whose control sits on the right.
unsafe fn row_label(dc: HDC, row_top: i32, title: &str, detail: &str) {
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(233, 238, 244));
    text_left(dc, title, 40, row_top + 1, 200, 20);
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(124, 134, 148));
    text_left(dc, detail, 40, row_top + 21, 200, 18);
}
