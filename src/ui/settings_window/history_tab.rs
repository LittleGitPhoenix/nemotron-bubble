//! The History tab: the scrollable list of recent dictations with copy icons and the trash button.

use crate::*;

// History tab: a title row with the trash button, then the custom-drawn,
// scrollable list of entries (copy icon + grey header line + text each).
pub(crate) const HISTORY_TITLE_TOP: i32 = SETTINGS_CARD_TOP + 14;
pub(crate) const HISTORY_LIST_TOP: i32 = SETTINGS_CARD_TOP + 52;
pub(crate) const HISTORY_LIST_BOTTOM: i32 = SETTINGS_HEIGHT - SETTINGS_FOOTER_H - 12;
// Text column (the scrollbar sits in the 12 px to the right of it).
pub(crate) const HISTORY_TEXT_LEFT: i32 = SETTINGS_INNER_LEFT;
pub(crate) const HISTORY_TEXT_RIGHT: i32 = SETTINGS_INNER_RIGHT - 8;
pub(crate) const HISTORY_HEADER_H: i32 = 16;
pub(crate) const HISTORY_ENTRY_GAP: i32 = 14;
// Pixels scrolled per mouse-wheel notch.
pub(crate) const HISTORY_WHEEL_STEP: i32 = 60;
// Timer that turns a copy icon's checkmark back into the copy icon.
pub(crate) const COPY_FLASH_TIMER_ID: usize = 78;
pub(crate) const COPY_FLASH_MS: u32 = 1200;
pub(crate) const GLYPH_COPY: &str = "\u{E8C8}";
pub(crate) const GLYPH_CHECK: &str = "\u{E73E}";
pub(crate) const GLYPH_TRASH: &str = "\u{E74D}";

/// What the History tab shows: the entries, how far the list is scrolled, and
/// which entry's copy icon currently shows the "copied" checkmark.
pub(crate) struct HistoryView<'a> {
    pub(crate) items: &'a [HistoryItem],
    pub(crate) scroll: i32,
    pub(crate) copied: Option<usize>,
}

/// The History list as painted: copy-icon hit areas (rect, entry index), the
/// total content height and the scroll offset actually used (clamped).
#[derive(Default)]
pub(crate) struct HistoryLayout {
    pub(crate) hits: Vec<(RectI, usize)>,
    pub(crate) content_h: i32,
    pub(crate) scroll: i32,
}

/// Vertical layout of the history entries in list coordinates (0 = top of the
/// list content): (entry top, wrapped text height), plus the total height.
pub(crate) unsafe fn measure_history(dc: HDC, items: &[HistoryItem]) -> (Vec<(i32, i32)>, i32) {
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

/// The trash button at the top right of the History tab (clears the history).
pub(crate) fn settings_clear_history_rect() -> RectI {
    RectI {
        left: SETTINGS_INNER_RIGHT - 32,
        top: HISTORY_TITLE_TOP - 6,
        right: SETTINGS_INNER_RIGHT,
        bottom: HISTORY_TITLE_TOP + 26,
    }
}

pub(crate) fn handle_history_tab_click(app: &Arc<AppState>, x: i32, y: i32) {
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


/// The History list prepared for painting: each entry's (top, text height) in
/// list coordinates, plus the layout handed back to the window (hit areas,
/// content height, clamped scroll offset).
#[derive(Default)]
pub(crate) struct HistoryList {
    pub(crate) entries: Vec<(i32, i32)>,
    pub(crate) layout: HistoryLayout,
}

/// Measure the entries and clamp the scroll offset to the content.
pub(crate) unsafe fn measure_history_list(dc: HDC, history: &HistoryView) -> HistoryList {
    let view_h = HISTORY_LIST_BOTTOM - HISTORY_LIST_TOP;
    let (entries, content_h) = measure_history(dc, history.items);
    let scroll = history.scroll.clamp(0, (content_h - view_h).max(0));
    HistoryList {
        entries,
        layout: HistoryLayout {
            hits: Vec::new(),
            content_h,
            scroll,
        },
    }
}

/// Pass 1 of the History tab: the trash button and the scrollbar.
pub(crate) fn draw_history_shapes(buf: &mut [u32], cw: i32, ch: i32, list: &HistoryList) {
    let view_h = HISTORY_LIST_BOTTOM - HISTORY_LIST_TOP;
    let content_h = list.layout.content_h;
    let scroll = list.layout.scroll;
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

/// Pass 2 of the History tab: title, trash glyph and the entries. Records the
/// copy-icon hit areas in `list.layout`.
pub(crate) unsafe fn draw_history_text(dc: HDC, history: &HistoryView, list: &mut HistoryList) {
    let scroll = list.layout.scroll;

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
    for (index, (item, (top, text_h))) in history.items.iter().zip(&list.entries).enumerate() {
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
            list.layout.hits.push((icon, index));
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
