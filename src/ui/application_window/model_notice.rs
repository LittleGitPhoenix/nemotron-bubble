//! The annotation over the Model tab that explains how to get a missing speech
//! model: the tab is dimmed, the controls that solve the problem are ringed and
//! marked with badges, and a callout below the Downloads rows explains the badges.

use crate::*;

const CALLOUT_LEFT: i32 = WINDOW_CARD_LEFT + 12;
const CALLOUT_RIGHT: i32 = WINDOW_WIDTH - WINDOW_CARD_LEFT - 12;
const CALLOUT_PADDING_X: i32 = 16;
const CALLOUT_PADDING_Y: i32 = 13;
/// Gap between the last Downloads button and the callout. The pointer sits in it.
const CALLOUT_GAP: i32 = 14;
const STEP_BADGE_SIZE: i32 = 18;
const STEP_TEXT_INDENT: i32 = STEP_BADGE_SIZE + 8;
const STEP_GAP: i32 = 6;
/// How far the ring lies outside the control it marks.
const RING_OFFSET: f32 = 4.0;
/// Corner radius of the buttons and segment tracks that get a ring.
const CONTROL_RADIUS: f32 = 13.0;

const ACCENT: Rgb = (92, 146, 248);
const SCRIM: Rgb = (13, 15, 19);
const SCRIM_ALPHA: f32 = 0.62;
const CARD: Rgb = (30, 33, 41);
const CALLOUT_FILL: Rgb = (42, 47, 59);
const CALLOUT_BORDER: Rgb = (70, 102, 163);

/// The texts of a notice: title, explanation and the steps with their badge labels.
pub(crate) struct NoticeTexts {
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) steps: Vec<(&'static str, String)>,
}

pub(crate) fn model_notice_texts(notice: ModelNotice) -> NoticeTexts {
    match notice {
        ModelNotice::NoModel => NoticeTexts {
            title: "Download a speech model".to_string(),
            body: "Nemotron Bubble needs a speech model to turn your voice into text. None is installed yet."
                .to_string(),
            steps: vec![
                (
                    "1",
                    "Click Download next to any of the models. A model is about 2.5\u{A0}GB. \
                     The first downloaded model is automatically selected."
                        .to_string(),
                ),
                (
                    "2",
                    "Optionally download another model and then select which to use.".to_string(),
                ),
            ],
        },
        ModelNotice::SelectedMissing { selected, installed } => {
            let use_installed = if installed == ModelChoice::English {
                "Select English to dictate in English right away.".to_string()
            } else {
                format!("Select {} to dictate right away.", installed.label())
            };
            NoticeTexts {
                title: format!("{} model not found", selected.label()),
                body: format!(
                    "Your settings use the {} model, but it is not on this computer. The {} model is.",
                    selected.label(),
                    installed.label()
                ),
                steps: vec![
                    ("A", use_installed),
                    (
                        "B",
                        format!("Or click Download to get {} (about 2.5\u{A0}GB).", selected.label()),
                    ),
                ],
            }
        }
    }
}

/// The controls a notice points at, each with the badge it carries.
fn notice_targets(notice: ModelNotice) -> Vec<(RectI, &'static str)> {
    let download_button = |choice: ModelChoice| {
        DOWNLOADABLE_MODELS
            .iter()
            .position(|model| *model == choice)
            .map(download_button_rect)
    };
    match notice {
        ModelNotice::NoModel => {
            let mut targets: Vec<(RectI, &'static str)> = (0..DOWNLOADABLE_MODELS.len())
                .map(|index| (download_button_rect(index), "1"))
                .collect();
            targets.push((model_track_rect(), "2"));
            targets
        }
        ModelNotice::SelectedMissing { selected, installed } => {
            let mut targets = vec![(model_rect(installed), "A")];
            targets.extend(download_button(selected).map(|rect| (rect, "B")));
            targets
        }
    }
}

/// Top of the callout: below the last Downloads row, leaving room for the pointer.
fn callout_top() -> i32 {
    download_button_rect(DOWNLOADABLE_MODELS.len() - 1).bottom + CALLOUT_GAP
}

/// The ✕ that closes the notice, at the top right of the callout.
pub(crate) fn model_notice_close_rect() -> RectI {
    let top = callout_top();
    RectI {
        left: CALLOUT_RIGHT - 40,
        top: top + 4,
        right: CALLOUT_RIGHT - 8,
        bottom: top + 32,
    }
}

/// Where everything of a notice goes, measured with the fonts it is drawn with.
pub(crate) struct NoticeLayout {
    targets: Vec<(RectI, &'static str)>,
    pub(crate) callout: RectI,
    texts: NoticeTexts,
    title: RectI,
    body: RectI,
    steps: Vec<RectI>,
}

unsafe fn wrapped_text_height(dc: HDC, text: &str, width: i32) -> i32 {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: 0,
    };
    let wide = wide_null(text);
    DrawTextW(
        dc,
        wide.as_ptr(),
        -1,
        &mut rect,
        DT_CALCRECT | DT_LEFT | DT_WORDBREAK | DT_NOPREFIX,
    );
    rect.bottom - rect.top
}

pub(crate) unsafe fn measure_model_notice(dc: HDC, notice: ModelNotice) -> NoticeLayout {
    let texts = model_notice_texts(notice);
    let top = callout_top();
    let text_left = CALLOUT_LEFT + CALLOUT_PADDING_X;
    let text_right = CALLOUT_RIGHT - CALLOUT_PADDING_X;
    let mut y = top + CALLOUT_PADDING_Y;

    let title = RectI {
        left: text_left,
        top: y,
        right: model_notice_close_rect().left,
        bottom: y + 20,
    };
    y = title.bottom + 6;

    SelectObject(dc, small_font() as _);
    let body_h = wrapped_text_height(dc, &texts.body, text_right - text_left);
    let body = RectI {
        left: text_left,
        top: y,
        right: text_right,
        bottom: y + body_h,
    };
    y = body.bottom + 10;

    let mut steps = Vec::new();
    for (_, text) in &texts.steps {
        let left = text_left + STEP_TEXT_INDENT;
        let h = wrapped_text_height(dc, text, text_right - left).max(STEP_BADGE_SIZE);
        steps.push(RectI {
            left,
            top: y,
            right: text_right,
            bottom: y + h,
        });
        y += h + STEP_GAP;
    }
    let bottom = y - STEP_GAP + CALLOUT_PADDING_Y + 1;

    NoticeLayout {
        targets: notice_targets(notice),
        callout: RectI {
            left: CALLOUT_LEFT,
            top,
            right: CALLOUT_RIGHT,
            bottom,
        },
        texts,
        title,
        body,
        steps,
    }
}

fn grown(rect: RectI, by: f32) -> (f32, f32, f32, f32) {
    (
        rect.left as f32 - by,
        rect.top as f32 - by,
        rect.right as f32 + by,
        rect.bottom as f32 + by,
    )
}

/// Signed distance to a rounded rectangle given by its edges.
fn sd_box(px: f32, py: f32, (x0, y0, x1, y1): (f32, f32, f32, f32), radius: f32) -> f32 {
    sd_round_rect(px, py, (x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0) / 2.0, (y1 - y0) / 2.0, radius)
}

fn fill_circle(buf: &mut [u32], cw: i32, ch: i32, cx: f32, cy: f32, radius: f32, color: Rgb) {
    let bounds = (cx - radius, cy - radius, cx + radius, cy + radius);
    fill_sdf(buf, cw, ch, bounds, color, 1.0, |px, py| (px - cx).hypot(py - cy) - radius);
}

/// A square standing on its tip (the callout's pointer), `radius` from center to tip.
fn fill_diamond(buf: &mut [u32], cw: i32, ch: i32, cx: f32, cy: f32, radius: f32, color: Rgb) {
    let bounds = (cx - radius, cy - radius, cx + radius, cy + radius);
    fill_sdf(buf, cw, ch, bounds, color, 1.0, |px, py| {
        ((px - cx).abs() + (py - cy).abs() - radius) / std::f32::consts::SQRT_2
    });
}

/// Center of the badge of a ringed control: on the ring's top-left corner.
fn ring_badge_center(target: RectI) -> (f32, f32) {
    (target.left as f32 - RING_OFFSET + 2.0, target.top as f32 - RING_OFFSET + 2.0)
}

/// Center of the badge in front of a step of the callout.
fn step_badge_center(step: RectI) -> (f32, f32) {
    let radius = STEP_BADGE_SIZE as f32 / 2.0;
    ((step.left - STEP_TEXT_INDENT) as f32 + radius, step.top as f32 + radius - 1.0)
}

/// Pass 1: scrim with holes, rings, badges and the callout with its pointer.
pub(crate) fn draw_model_notice_shapes(buf: &mut [u32], cw: i32, ch: i32, layout: &NoticeLayout) {
    let card = (
        WINDOW_CARD_LEFT as f32,
        WINDOW_CARD_TOP as f32,
        (cw - WINDOW_CARD_LEFT) as f32,
        (ch - WINDOW_FOOTER_H) as f32,
    );
    let holes: Vec<_> = layout
        .targets
        .iter()
        .map(|(target, _)| grown(*target, RING_OFFSET + 4.0))
        .collect();
    fill_sdf(buf, cw, ch, card, SCRIM, SCRIM_ALPHA, |px, py| {
        holes
            .iter()
            .fold(sd_box(px, py, card, 16.0), |distance, hole| {
                distance.max(-sd_box(px, py, *hole, CONTROL_RADIUS + RING_OFFSET + 4.0))
            })
    });

    for (target, _) in &layout.targets {
        let ring = grown(*target, RING_OFFSET);
        let radius = CONTROL_RADIUS + RING_OFFSET;
        let bounds = grown(*target, RING_OFFSET + 5.0);
        fill_sdf(buf, cw, ch, bounds, ACCENT, 0.22, |px, py| sd_box(px, py, ring, radius).abs() - 4.0);
        fill_sdf(buf, cw, ch, bounds, ACCENT, 1.0, |px, py| sd_box(px, py, ring, radius).abs() - 1.0);
    }
    for (target, _) in &layout.targets {
        let (cx, cy) = ring_badge_center(*target);
        fill_circle(buf, cw, ch, cx, cy, 13.0, CARD);
        fill_circle(buf, cw, ch, cx, cy, 10.0, ACCENT);
    }

    let callout = layout.callout;
    let (x, y) = (callout.left as f32, callout.top as f32);
    let (w, h) = ((callout.right - callout.left) as f32, (callout.bottom - callout.top) as f32);
    fill_shadow(buf, cw, ch, x, y + 4.0, w, h, 12.0, 18.0, 0.5);
    // The pointer points up at the lowest ringed control, a Download button.
    let tip_target = layout
        .targets
        .iter()
        .map(|(target, _)| *target)
        .max_by_key(|target| target.bottom)
        .unwrap_or(callout);
    let tip_x = (tip_target.left + tip_target.right) as f32 / 2.0;
    fill_diamond(buf, cw, ch, tip_x, y, 8.5, CALLOUT_BORDER);
    fill_round_rect(buf, cw, ch, x, y, w, h, 12.0, CALLOUT_BORDER, CALLOUT_BORDER, 1.0);
    fill_round_rect(buf, cw, ch, x + 1.0, y + 1.0, w - 2.0, h - 2.0, 11.0, CALLOUT_FILL, CALLOUT_FILL, 1.0);
    fill_diamond(buf, cw, ch, tip_x, y + 0.5, 8.5 - std::f32::consts::SQRT_2, CALLOUT_FILL);

    for step in &layout.steps {
        let (cx, cy) = step_badge_center(*step);
        fill_circle(buf, cw, ch, cx, cy, STEP_BADGE_SIZE as f32 / 2.0, ACCENT);
    }
}

/// Pass 2: badge labels, title, close glyph, explanation and steps.
pub(crate) unsafe fn draw_model_notice_text(dc: HDC, layout: &NoticeLayout) {
    let badge_rect = |(cx, cy): (f32, f32)| RectI {
        left: cx as i32 - 10,
        top: cy as i32 - 10,
        right: cx as i32 + 10,
        bottom: cy as i32 + 10,
    };
    SelectObject(dc, badge_font() as _);
    SetTextColor(dc, rgb(255, 255, 255));
    for (target, label) in &layout.targets {
        text_center(dc, label, badge_rect(ring_badge_center(*target)));
    }
    for (step, (label, _)) in layout.steps.iter().zip(&layout.texts.steps) {
        text_center(dc, label, badge_rect(step_badge_center(*step)));
    }

    SelectObject(dc, heading_font() as _);
    SetTextColor(dc, rgb(237, 241, 247));
    text_left_ellipsis(dc, &layout.texts.title, layout.title);

    SelectObject(dc, icon_font() as _);
    SetTextColor(dc, rgb(184, 192, 205));
    text_center(dc, GLYPH_CLOSE, model_notice_close_rect());

    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(184, 192, 205));
    let body = layout.body;
    text_left(dc, &layout.texts.body, body.left, body.top, body.right - body.left, body.bottom - body.top);
    SetTextColor(dc, rgb(233, 238, 244));
    for (step, (_, text)) in layout.steps.iter().zip(&layout.texts.steps) {
        text_left(dc, text, step.left, step.top, step.right - step.left, step.bottom - step.top);
    }
}

/// A click on the Model tab while the notice shows. The ✕ closes the notice,
/// the rest of the callout swallows clicks so they do not reach the controls
/// below it. Returns true when the click was handled.
pub(crate) fn handle_model_notice_click(app: &Arc<AppState>, x: i32, y: i32) -> bool {
    if lock_ui(app).model_notice.is_none() {
        return false;
    }
    if model_notice_close_rect().contains(x, y) {
        clear_model_notice(app);
        return true;
    }
    let callout = *app
        .model_notice_callout
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    callout.is_some_and(|callout| callout.contains(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_start_notice_marks_every_download_button_and_the_model_selection() {
        let targets = notice_targets(ModelNotice::NoModel);
        let ones = targets.iter().filter(|(_, label)| *label == "1").count();
        assert_eq!(ones, DOWNLOADABLE_MODELS.len());
        assert!(targets.iter().any(|(_, label)| *label == "2"));
        let texts = model_notice_texts(ModelNotice::NoModel);
        assert_eq!(texts.steps.iter().map(|(label, _)| *label).collect::<Vec<_>>(), ["1", "2"]);
    }

    #[test]
    fn missing_model_notice_offers_the_installed_model_and_the_download() {
        let notice = ModelNotice::SelectedMissing {
            selected: ModelChoice::Multilingual,
            installed: ModelChoice::English,
        };
        let texts = model_notice_texts(notice);
        assert_eq!(texts.title, "Multilingual model not found");
        assert_eq!(texts.steps[0].1, "Select English to dictate in English right away.");
        assert_eq!(texts.steps[1].1, "Or click Download to get Multilingual (about 2.5\u{A0}GB).");
        let targets = notice_targets(notice);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].0.left, model_rect(ModelChoice::English).left);
    }

    #[test]
    fn callout_starts_below_the_last_download_button() {
        let last = download_button_rect(DOWNLOADABLE_MODELS.len() - 1);
        assert!(model_notice_close_rect().top > last.bottom);
    }
}
