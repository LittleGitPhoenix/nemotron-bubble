//! Building blocks shared by the tabs: toggle switches, buttons, segmented controls, dividers and section headings.

use crate::*;

/// Title and detail line of a Model-tab section (its control sits below them).
pub(crate) unsafe fn section_heading(dc: HDC, top: i32, title: &str, detail: &str) {
    let width = WINDOW_INNER_RIGHT - WINDOW_INNER_LEFT;
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(233, 238, 244));
    text_left(dc, title, WINDOW_INNER_LEFT, top, width, 20);
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(124, 134, 148));
    text_left(dc, detail, WINDOW_INNER_LEFT, top + 20, width, 18);
}

/// Title and detail text of a toggle row at `rect` (the switch itself is drawn in pass 1).
pub(crate) unsafe fn draw_toggle_text(dc: HDC, row: &SettingRow, rect: RectI) {
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(233, 238, 244));
    text_left(dc, row.title, rect.left + 8, rect.top + 1, 232, 20);
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(124, 134, 148));
    text_left(dc, row.detail, rect.left + 8, rect.top + 21, 248, 18);
}

/// Dark rounded track behind a segmented control.
pub(crate) fn draw_segment_track(buf: &mut [u32], cw: i32, ch: i32, track: RectI) {
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
pub(crate) fn draw_divider(buf: &mut [u32], cw: i32, ch: i32, y: i32) {
    let width = (WINDOW_INNER_RIGHT - WINDOW_INNER_LEFT) as f32;
    fill_round_rect(
        buf,
        cw,
        ch,
        WINDOW_INNER_LEFT as f32,
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
pub(crate) fn draw_toggle(buf: &mut [u32], cw: i32, ch: i32, row: RectI, enabled: bool) {
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
pub(crate) fn draw_button_bg(buf: &mut [u32], cw: i32, ch: i32, rect: RectI, primary: bool) {
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

/// A full-width segmented-control track below a section's title and detail line.
pub(crate) fn section_track_rect(section_top: i32) -> RectI {
    RectI {
        left: WINDOW_INNER_LEFT,
        top: section_top + 44,
        right: WINDOW_INNER_RIGHT,
        bottom: section_top + 76,
    }
}

/// Segment `index` of `count` equal segments within `track`.
pub(crate) fn segment_rect(track: RectI, index: usize, count: usize) -> RectI {
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


/// Thin vertical scrollbar (track and thumb) at `x`, spanning `top` to `top + view_h`.
/// Drawn only when the content is taller than the view.
pub(crate) fn draw_scrollbar(buf: &mut [u32], cw: i32, ch: i32, x: i32, top: i32, view_h: i32, content_h: i32, scroll: i32) {
    if content_h <= view_h {
        return;
    }
    let track_x = x as f32;
    let track_top = top as f32;
    let track_h = view_h as f32;
    let thumb_h = (track_h * view_h as f32 / content_h as f32).max(24.0);
    let thumb_top = track_top + (track_h - thumb_h) * scroll as f32 / (content_h - view_h) as f32;
    fill_round_rect(buf, cw, ch, track_x, track_top, 4.0, track_h, 2.0, (40, 44, 54), (40, 44, 54), 1.0);
    fill_round_rect(buf, cw, ch, track_x, thumb_top, 4.0, thumb_h, 2.0, (92, 100, 116), (92, 100, 116), 1.0);
}
