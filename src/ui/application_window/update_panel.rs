//! App updates in the application window: the version pill in the footer and the
//! update panel, a callout over the dimmed card like the model notice. The panel
//! offers the update (Update now, Skip this version, Later and the release notes)
//! and shows its download and installation.

use crate::*;

// The same colors as the model notice.
const PANEL_ACCENT: Rgb = (92, 146, 248);
const PANEL_SCRIM: Rgb = (13, 15, 19);
const PANEL_SCRIM_ALPHA: f32 = 0.62;
const PANEL_FILL: Rgb = (42, 47, 59);
const PANEL_BORDER: Rgb = (70, 102, 163);

const PANEL_LEFT: i32 = WINDOW_CARD_LEFT + 12;
const PANEL_RIGHT: i32 = WINDOW_WIDTH - WINDOW_CARD_LEFT - 12;
const PANEL_TOP: i32 = 236;
const PANEL_PADDING: i32 = 16;
const BUTTON_H: i32 = 34;

// Segoe MDL2 Assets glyphs: "Download" in front of the title, "Open in new window" after the link.
const GLYPH_DOWNLOAD: &str = "\u{E896}";
const GLYPH_OPEN_IN_NEW: &str = "\u{E8A7}";
const RELEASE_NOTES: &str = "Release notes on GitHub";

/// The footer pill with the available version, right of "Version x.y.z".
pub(crate) fn footer_pill_rect(version: &str) -> RectI {
    let text_w = unsafe { text_width_in(small_font(), version) };
    let right = WINDOW_WIDTH - 34;
    let top = WINDOW_HEIGHT - WINDOW_FOOTER_H + 9;
    RectI {
        left: right - (8 + 10 + 4 + text_w + 9),
        top,
        right,
        bottom: top + 20,
    }
}

/// Pass 1 of the footer pill.
pub(crate) fn draw_footer_pill_shape(buf: &mut [u32], cw: i32, ch: i32, pill: RectI, hot: bool) {
    let color: Rgb = if hot { (48, 71, 117) } else { (38, 55, 90) };
    let (w, h) = ((pill.right - pill.left) as f32, (pill.bottom - pill.top) as f32);
    fill_round_rect(buf, cw, ch, pill.left as f32, pill.top as f32, w, h, h / 2.0, color, color, 1.0);
}

/// Where "Version x.y.z" is drawn (right-aligned): at the right edge, or left of
/// the pill of an available `update`.
pub(crate) fn footer_version_rect(update: Option<&str>) -> RectI {
    let right = match update {
        Some(version) => footer_pill_rect(version).left - 8,
        None => WINDOW_WIDTH - 34,
    };
    RectI {
        left: right - 202,
        top: WINDOW_HEIGHT - WINDOW_FOOTER_H + 10,
        right,
        bottom: WINDOW_HEIGHT - WINDOW_FOOTER_H + 28,
    }
}

/// Pass 2 of the footer: "Version x.y.z" and, with an update, the pill's arrow and version.
pub(crate) unsafe fn draw_footer_text(dc: HDC, update: Option<&str>) {
    if let Some(version) = update {
        let pill = footer_pill_rect(version);
        SelectObject(dc, icon_font() as _);
        SetTextColor(dc, rgb(110, 162, 255));
        text_center(dc, GLYPH_UPDATE, RectI { left: pill.left + 6, right: pill.left + 20, ..pill });
        SelectObject(dc, small_font() as _);
        SetTextColor(dc, rgb(169, 200, 255));
        text_left_ellipsis(dc, version, RectI { left: pill.left + 22, ..pill });
    }
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(108, 118, 132));
    text_right(dc, &format!("Version {APP_VERSION}"), footer_version_rect(update));
}

/// What the panel shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PanelMode {
    /// The update is offered: Update now (or Retry), Skip this version, Later.
    Offer,
    /// Downloading, cancelling or installing: progress and Cancel.
    Busy,
}

fn panel_mode(view: &UpdateView) -> PanelMode {
    match view.state.activity {
        UpdateActivity::Downloading { .. } | UpdateActivity::Cancelling | UpdateActivity::Installing => PanelMode::Busy,
        UpdateActivity::Idle | UpdateActivity::Checking => PanelMode::Offer,
    }
}

/// Where everything of the panel goes (window coordinates).
struct PanelLayout {
    callout: RectI,
    title: RectI,
    versions: RectI,
    meta: RectI,
    /// The release notes link (Offer) or the progress bar (Busy).
    link: RectI,
    note: RectI,
    primary: RectI,
    skip: Option<RectI>,
    later: Option<RectI>,
}

fn panel_layout(mode: PanelMode) -> PanelLayout {
    let (left, right) = (PANEL_LEFT + PANEL_PADDING, PANEL_RIGHT - PANEL_PADDING);
    let top = PANEL_TOP;
    let row = |from: i32, height: i32| RectI {
        left,
        top: top + from,
        right,
        bottom: top + from + height,
    };
    let primary = row(186, BUTTON_H);
    let (skip, later, bottom) = match mode {
        PanelMode::Offer => {
            let second = row(228, BUTTON_H);
            let middle = (left + right) / 2;
            (
                Some(RectI { right: middle - 4, ..second }),
                Some(RectI { left: middle + 4, ..second }),
                second.bottom + PANEL_PADDING,
            )
        }
        PanelMode::Busy => (None, None, primary.bottom + PANEL_PADDING),
    };
    PanelLayout {
        callout: RectI {
            left: PANEL_LEFT,
            top,
            right: PANEL_RIGHT,
            bottom,
        },
        title: row(14, 22),
        versions: row(44, 32),
        meta: row(80, 18),
        link: row(108, 22),
        note: row(138, 36),
        primary,
        skip,
        later,
    }
}

/// The clickable part of the release notes link: its text and glyph.
fn release_notes_link_rect(layout: &PanelLayout) -> RectI {
    let width = unsafe { text_width_in(ui_font(), RELEASE_NOTES) } + 22;
    RectI {
        right: layout.link.left + width,
        ..layout.link
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// Pass 1 of the panel: scrim over the card, the callout, buttons and progress bar.
pub(crate) fn draw_update_panel_shapes(buf: &mut [u32], cw: i32, ch: i32, view: &UpdateView) {
    let mode = panel_mode(view);
    let layout = panel_layout(mode);
    let card = (
        WINDOW_CARD_LEFT as f32,
        WINDOW_CARD_TOP as f32,
        (cw - WINDOW_CARD_LEFT) as f32,
        (ch - WINDOW_FOOTER_H) as f32,
    );
    let (cx, cy) = ((card.0 + card.2) / 2.0, (card.1 + card.3) / 2.0);
    let (hx, hy) = ((card.2 - card.0) / 2.0, (card.3 - card.1) / 2.0);
    fill_sdf(buf, cw, ch, card, PANEL_SCRIM, PANEL_SCRIM_ALPHA, |px, py| {
        sd_round_rect(px, py, cx, cy, hx, hy, 16.0)
    });

    let callout = layout.callout;
    let (x, y) = (callout.left as f32, callout.top as f32);
    let (w, h) = ((callout.right - callout.left) as f32, (callout.bottom - callout.top) as f32);
    fill_shadow(buf, cw, ch, x, y + 4.0, w, h, 12.0, 18.0, 0.5);
    fill_round_rect(buf, cw, ch, x, y, w, h, 12.0, PANEL_BORDER, PANEL_BORDER, 1.0);
    fill_round_rect(buf, cw, ch, x + 1.0, y + 1.0, w - 2.0, h - 2.0, 11.0, PANEL_FILL, PANEL_FILL, 1.0);

    match (mode, view.state.activity) {
        (PanelMode::Offer, _) => {
            draw_button_bg(buf, cw, ch, layout.primary, true);
            if let Some(skip) = layout.skip {
                draw_button_bg(buf, cw, ch, skip, false);
            }
        }
        (PanelMode::Busy, activity) => {
            if let UpdateActivity::Downloading { done, total } = activity {
                draw_button_bg(buf, cw, ch, layout.primary, false);
                let bar = RectI {
                    top: layout.link.top + 9,
                    bottom: layout.link.top + 13,
                    ..layout.link
                };
                let width = (bar.right - bar.left) as f32;
                let (bx, by, bh) = (bar.left as f32, bar.top as f32, (bar.bottom - bar.top) as f32);
                fill_round_rect(buf, cw, ch, bx, by, width, bh, 2.0, (58, 64, 78), (58, 64, 78), 1.0);
                let progress = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                let filled = (width * progress.clamp(0.0, 1.0)).max(bh);
                fill_round_rect(buf, cw, ch, bx, by, filled, bh, 2.0, (110, 162, 255), (84, 138, 250), 1.0);
            }
        }
    }
}

/// Pass 2 of the panel: title, versions, release date and size, link, note and button labels.
pub(crate) unsafe fn draw_update_panel_text(dc: HDC, view: &UpdateView) {
    let Some(update) = &view.state.available else {
        return;
    };
    let mode = panel_mode(view);
    let layout = panel_layout(mode);
    let version = update.version.to_string();

    // title with the download glyph in front
    SelectObject(dc, icon_font_large() as _);
    SetTextColor(dc, rgb(PANEL_ACCENT.0, PANEL_ACCENT.1, PANEL_ACCENT.2));
    text_center(dc, GLYPH_DOWNLOAD, RectI { right: layout.title.left + 18, ..layout.title });
    SelectObject(dc, heading_font() as _);
    SetTextColor(dc, rgb(245, 248, 252));
    let title = match mode {
        PanelMode::Offer => "Update available".to_string(),
        PanelMode::Busy => format!("Updating to {version}"),
    };
    text_left_ellipsis(dc, &title, RectI { left: layout.title.left + 26, ..layout.title });

    // "1.1.0 → 1.2.0": the installed version dimmed, the new one bright
    SelectObject(dc, title_font() as _);
    let old = APP_VERSION;
    let arrow = "  \u{2192}  ";
    let mut x = layout.versions.left;
    SetTextColor(dc, rgb(140, 150, 164));
    text_left_ellipsis(dc, old, RectI { left: x, ..layout.versions });
    x += text_width_in(title_font(), old);
    SetTextColor(dc, rgb(108, 118, 132));
    text_left_ellipsis(dc, arrow, RectI { left: x, ..layout.versions });
    x += text_width_in(title_font(), arrow);
    SetTextColor(dc, rgb(245, 248, 252));
    text_left_ellipsis(dc, &version, RectI { left: x, ..layout.versions });

    // release date and size, or the download progress
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(140, 150, 164));
    let meta = match view.state.activity {
        UpdateActivity::Downloading { done, total } if total > 0 => {
            format!("Downloading {} % of {}", done * 100 / total, megabytes(total))
        }
        UpdateActivity::Downloading { .. } => "Starting the download...".to_string(),
        UpdateActivity::Cancelling => "Cancelling...".to_string(),
        UpdateActivity::Installing => "Installing...".to_string(),
        UpdateActivity::Idle | UpdateActivity::Checking => match &update.date {
            Some(date) => format!("Released {date} \u{B7} {}", megabytes(update.size)),
            None => megabytes(update.size),
        },
    };
    text_left_ellipsis(dc, &meta, layout.meta);

    if mode == PanelMode::Offer {
        let link = release_notes_link_rect(&layout);
        SelectObject(dc, ui_font() as _);
        SetTextColor(dc, rgb(110, 162, 255));
        text_left_ellipsis(dc, RELEASE_NOTES, link);
        SelectObject(dc, icon_font() as _);
        text_center(dc, GLYPH_OPEN_IN_NEW, RectI { left: link.right - 16, ..link });
    }

    // note, or the error of a failed attempt in red
    SelectObject(dc, small_font() as _);
    let note = match (&view.state.install_error, view.state.activity) {
        (Some(error), UpdateActivity::Idle) => {
            SetTextColor(dc, rgb(240, 128, 128));
            format!("Failed: {error}")
        }
        (_, UpdateActivity::Installing) => {
            SetTextColor(dc, rgb(140, 150, 164));
            "The app restarts in a moment.".to_string()
        }
        (_, UpdateActivity::Downloading { .. } | UpdateActivity::Cancelling) => {
            SetTextColor(dc, rgb(140, 150, 164));
            "Then the SHA-256 is checked and the app restarts on its own.".to_string()
        }
        _ if view.skipped => {
            SetTextColor(dc, rgb(140, 150, 164));
            "You skipped this version. Update now installs it anyway.".to_string()
        }
        _ => {
            SetTextColor(dc, rgb(140, 150, 164));
            "The app downloads the update, restarts and keeps its settings, history and models.".to_string()
        }
    };
    let mut note_rect = RECT {
        left: layout.note.left,
        top: layout.note.top,
        right: layout.note.right,
        bottom: layout.note.bottom,
    };
    let wide = wide_null(&note);
    DrawTextW(
        dc,
        wide.as_ptr(),
        -1,
        &mut note_rect,
        DT_LEFT | DT_TOP | DT_WORDBREAK | DT_EDITCONTROL | DT_END_ELLIPSIS | DT_NOPREFIX,
    );

    // button labels
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(245, 248, 252));
    match (mode, view.state.activity) {
        (PanelMode::Offer, _) => {
            let primary = if view.state.install_error.is_some() { "Retry" } else { "Update now" };
            text_center(dc, primary, layout.primary);
            if let Some(skip) = layout.skip {
                text_center(dc, "Skip this version", skip);
            }
            if let Some(later) = layout.later {
                SetTextColor(dc, rgb(170, 178, 192));
                text_center(dc, "Later", later);
            }
        }
        (PanelMode::Busy, UpdateActivity::Downloading { .. }) => text_center(dc, "Cancel", layout.primary),
        (PanelMode::Busy, _) => {}
    }
}

/// A click while the panel is open. Clicks on the dimmed card outside the
/// callout close it like Later. Returns true when the click was handled.
pub(crate) fn handle_update_panel_click(app: &Arc<AppState>, x: i32, y: i32) -> bool {
    let view = update_view(app);
    if !view.state.panel_open || view.state.available.is_none() {
        return false;
    }
    let in_card = (WINDOW_CARD_TOP..WINDOW_HEIGHT - WINDOW_FOOTER_H).contains(&y);
    if !in_card {
        return false;
    }
    let mode = panel_mode(&view);
    let layout = panel_layout(mode);
    if !layout.callout.contains(x, y) {
        if mode == PanelMode::Offer {
            close_update_panel(app);
        }
        return true;
    }
    match mode {
        PanelMode::Offer => {
            if layout.primary.contains(x, y) {
                start_app_update(app);
            } else if layout.skip.is_some_and(|skip| skip.contains(x, y)) {
                skip_update(app);
            } else if layout.later.is_some_and(|later| later.contains(x, y)) {
                close_update_panel(app);
            } else if release_notes_link_rect(&layout).contains(x, y) {
                open_release_notes(app);
            }
        }
        PanelMode::Busy => {
            if layout.primary.contains(x, y) {
                cancel_app_update(app);
            }
        }
    }
    true
}

/// A click on the footer pill opens the panel. Returns true when it was hit.
pub(crate) fn handle_footer_click(app: &Arc<AppState>, x: i32, y: i32) -> bool {
    match pending_update_version(app) {
        Some(version) if footer_pill_rect(&version).contains(x, y) => {
            open_update_panel(app, WindowOrigin::Tray);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_fits_into_the_card() {
        for mode in [PanelMode::Offer, PanelMode::Busy] {
            let layout = panel_layout(mode);
            assert!(layout.callout.top > WINDOW_CARD_TOP);
            assert!(layout.callout.bottom < WINDOW_HEIGHT - WINDOW_FOOTER_H);
            assert!(layout.primary.bottom < layout.callout.bottom);
        }
    }

    #[test]
    fn offer_buttons_do_not_overlap() {
        let layout = panel_layout(PanelMode::Offer);
        let (skip, later) = (layout.skip.unwrap(), layout.later.unwrap());
        assert!(skip.right < later.left);
        assert!(layout.primary.bottom < skip.top);
        assert!(layout.note.bottom < layout.primary.top);
    }

    #[test]
    fn the_footer_pill_and_the_version_fit_side_by_side() {
        let (pill_padding, longest_own) = (22 + 9, unsafe { text_width_in(small_font(), "Version 10.20.30") });
        for version in ["1.2.0", "10.20.30", "123.456.789"] {
            let pill = footer_pill_rect(version);
            let text = footer_version_rect(Some(version));
            let version_width = unsafe { text_width_in(small_font(), version) };
            assert!(pill.right <= WINDOW_WIDTH - 34, "{version}: the pill leaves the window's margin");
            assert!(pill.right - pill.left >= version_width + pill_padding, "{version}: the pill is too narrow");
            assert!(text.right < pill.left, "{version}: the version text runs into the pill");
            assert!(text.right - text.left >= longest_own, "{version}: no room for the own version");
            assert!(text.left >= WINDOW_CARD_LEFT, "{version}: the version text leaves the window");
        }
        // Without an update the version sits where it always did.
        assert_eq!(footer_version_rect(None).right, WINDOW_WIDTH - 34);
    }
}
