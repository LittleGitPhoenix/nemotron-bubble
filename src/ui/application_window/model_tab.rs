//! The Model tab: Processing, Speech model, Downloads, Language and Preload.

use crate::*;

// Model tab: Processing, Speech model, Downloads, Language, Preload (top to bottom)
pub(crate) const COMPUTE_SECTION_TOP: i32 = 132;
pub(crate) const MODEL_SECTION_TOP: i32 = 230;
pub(crate) const DOWNLOADS_SECTION_TOP: i32 = 328;
// Downloads: one row per downloadable model below the section heading.
pub(crate) const DOWNLOAD_ROWS_TOP: i32 = DOWNLOADS_SECTION_TOP + 46;
pub(crate) const DOWNLOAD_ROW_H: i32 = 56;
pub(crate) const LANGUAGE_ROW_TOP: i32 = DOWNLOAD_ROWS_TOP + DOWNLOADABLE_MODELS.len() as i32 * DOWNLOAD_ROW_H + 12;
pub(crate) const PRELOAD_ROW_TOP: i32 = LANGUAGE_ROW_TOP + 56;

/// Popup menu under the application window's Language button.
pub(crate) unsafe fn show_language_menu(app: &Arc<AppState>) {
    let menu = build_language_menu(&settings_snapshot(app), app.recording.load(Ordering::SeqCst));
    show_menu_below(app, menu, language_rect());
}

/// The Language menu (popup in the application window and tray submenu): current one checked,
/// everything greyed out while dictating or with the English model selected.
pub(crate) unsafe fn build_language_menu(settings: &AppSettings, recording: bool) -> HMENU {
    let menu = CreatePopupMenu();
    if menu == null_mut() {
        return menu;
    }
    let current = normalize_language(&settings.language);
    let enabled = !recording && language_selectable(settings);
    for (index, (code, label)) in LANGUAGES.iter().enumerate() {
        let mut flags = MF_STRING;
        if *code == current {
            flags |= MF_CHECKED;
        }
        if !enabled {
            flags |= MF_GRAYED;
        }
        let text = wide_null(label);
        // AppendMenuW copies the string, so the buffer may be dropped afterwards.
        AppendMenuW(menu, flags, IDM_LANG_BASE + index, text.as_ptr());
        if index == 0 {
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
        }
    }
    menu
}

/// Background track of the Processing (CPU / GPU / Auto) segmented control.
pub(crate) fn compute_track_rect() -> RectI {
    section_track_rect(COMPUTE_SECTION_TOP)
}

/// One segment of the Processing control.
pub(crate) fn compute_rect(choice: ComputeChoice) -> RectI {
    let index = ComputeChoice::ALL.iter().position(|c| *c == choice).unwrap_or(0);
    segment_rect(compute_track_rect(), index, ComputeChoice::ALL.len())
}

/// Background track of the speech-model segmented control.
pub(crate) fn model_track_rect() -> RectI {
    section_track_rect(MODEL_SECTION_TOP)
}

/// One segment (English / Multilingual / Predefined) of the speech-model control.
pub(crate) fn model_rect(choice: ModelChoice) -> RectI {
    let index = ModelChoice::ALL.iter().position(|c| *c == choice).unwrap_or(0);
    segment_rect(model_track_rect(), index, ModelChoice::ALL.len())
}

/// The Language button (opens the language popup menu), right of its label.
pub(crate) fn language_rect() -> RectI {
    RectI {
        left: WINDOW_INNER_RIGHT - 140,
        top: LANGUAGE_ROW_TOP + 4,
        right: WINDOW_INNER_RIGHT,
        bottom: LANGUAGE_ROW_TOP + 36,
    }
}

pub(crate) fn handle_model_tab_click(app: &Arc<AppState>, x: i32, y: i32) {
    for choice in ModelChoice::ALL {
        if model_rect(choice).contains(x, y) {
            select_model(app, choice);
            return;
        }
    }

    for choice in ComputeChoice::ALL {
        if compute_rect(choice).contains(x, y) {
            select_compute(app, choice);
            return;
        }
    }

    if language_rect().contains(x, y) {
        if language_selectable(&settings_snapshot(app)) {
            unsafe { show_language_menu(app) };
        } else {
            update_ui(app, |ui| {
                ui.status = "The language applies to the multilingual model only.".to_string();
            });
        }
        return;
    }

    if handle_downloads_click(app, x, y) {
        return;
    }

    if toggle_rect(SETTINGS_TOGGLE_PRELOAD).contains(x, y) {
        toggle_setting(app, SETTINGS_TOGGLE_PRELOAD);
    }
}


/// Pass 1 of the Model tab: segmented controls, Downloads, dividers, Language button, Preload switch.
pub(crate) fn draw_model_shapes(buf: &mut [u32], cw: i32, ch: i32, settings: &AppSettings, downloads: &[ModelDownloadState]) {
    draw_segment_track(buf, cw, ch, compute_track_rect());
    draw_button_bg(buf, cw, ch, compute_rect(settings.compute), true);
    draw_divider(buf, cw, ch, MODEL_SECTION_TOP - 12);
    draw_segment_track(buf, cw, ch, model_track_rect());
    draw_button_bg(buf, cw, ch, model_rect(settings.model), true);
    draw_divider(buf, cw, ch, DOWNLOADS_SECTION_TOP - 12);
    draw_downloads_shapes(buf, cw, ch, downloads);
    draw_divider(buf, cw, ch, LANGUAGE_ROW_TOP - 12);
    draw_button_bg(buf, cw, ch, language_rect(), false);
    draw_divider(buf, cw, ch, PRELOAD_ROW_TOP - 8);
    draw_toggle(buf, cw, ch, toggle_rect(SETTINGS_TOGGLE_PRELOAD), settings.preload_model);
}

/// Pass 2 of the Model tab: section headings, segment labels, Downloads, Language and Preload texts.
pub(crate) unsafe fn draw_model_text(
    dc: HDC,
    settings: &AppSettings,
    model_status: &str,
    rows: &[SettingRow],
    downloads: &[ModelDownloadState],
) {
    // Processing (first: decides how the model is loaded)
    section_heading(
        dc,
        COMPUTE_SECTION_TOP,
        "Processing",
        "Where the model runs. Auto tries the GPU, then the CPU.",
    );
    SelectObject(dc, ui_font() as _);
    for choice in ComputeChoice::ALL {
        if choice == settings.compute {
            SetTextColor(dc, rgb(245, 248, 252));
        } else {
            SetTextColor(dc, rgb(184, 192, 205));
        }
        text_center(dc, choice.label(), compute_rect(choice));
    }

    // Speech model (detail line: what is loaded, and where)
    section_heading(dc, MODEL_SECTION_TOP, "Speech model", model_status);
    SelectObject(dc, ui_font() as _);
    for choice in ModelChoice::ALL {
        if choice == settings.model {
            SetTextColor(dc, rgb(245, 248, 252));
        } else if !model_choice_available(settings, choice) {
            SetTextColor(dc, rgb(92, 100, 114)); // greyed out: nothing predefined
        } else {
            SetTextColor(dc, rgb(184, 192, 205));
        }
        text_center(dc, choice.label(), model_rect(choice));
    }

    draw_downloads_text(dc, downloads);

    // Language: label + detail on the left, popup button on the right
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(233, 238, 244));
    text_left(dc, "Language", WINDOW_INNER_LEFT, LANGUAGE_ROW_TOP + 1, 150, 20);
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(124, 134, 148));
    text_left(dc, "Multilingual model only.", WINDOW_INNER_LEFT, LANGUAGE_ROW_TOP + 21, 160, 18);
    SelectObject(dc, ui_font() as _);
    if language_selectable(settings) {
        SetTextColor(dc, rgb(245, 248, 252));
    } else {
        SetTextColor(dc, rgb(92, 100, 114));
    }
    text_center(
        dc,
        &format!("{}  \u{25BE}", language_label(&settings.language)),
        language_rect(),
    );

    // Preload toggle
    if let Some(row) = rows.iter().find(|row| row.id == SETTINGS_TOGGLE_PRELOAD) {
        draw_toggle_text(dc, row, toggle_rect(row.id));
    }
}

/// The "Check for updates" button at the right of the Downloads heading.
pub(crate) fn check_updates_rect() -> RectI {
    RectI {
        left: WINDOW_INNER_RIGHT - 150,
        top: DOWNLOADS_SECTION_TOP + 4,
        right: WINDOW_INNER_RIGHT,
        bottom: DOWNLOADS_SECTION_TOP + 36,
    }
}

/// Top of the Downloads row of model `index` (in DOWNLOADABLE_MODELS order).
fn download_row_top(index: usize) -> i32 {
    DOWNLOAD_ROWS_TOP + index as i32 * DOWNLOAD_ROW_H
}

/// The action button (Download, Update, Resume, Retry, Cancel) at the right of a Downloads row.
pub(crate) fn download_button_rect(index: usize) -> RectI {
    let top = download_row_top(index);
    RectI {
        left: WINDOW_INNER_RIGHT - 100,
        top: top + 4,
        right: WINDOW_INNER_RIGHT,
        bottom: top + 36,
    }
}

/// The thin progress bar below the texts of a Downloads row.
fn download_progress_rect(index: usize) -> RectI {
    let top = download_row_top(index) + 42;
    RectI {
        left: WINDOW_INNER_LEFT,
        top,
        right: WINDOW_INNER_RIGHT,
        bottom: top + 4,
    }
}

/// What a Downloads row shows: the detail line and its color, the action
/// button (if any) and the progress (0..1) of a running download or verification.
struct DownloadRowView {
    detail: String,
    color: Rgb,
    button: Option<&'static str>,
    progress: Option<f32>,
}

const DETAIL_GREY: Rgb = (124, 134, 148);

fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
}

fn download_row_view(state: &ModelDownloadState) -> DownloadRowView {
    let row = |detail: String, color: Rgb, button: Option<&'static str>, progress: Option<f32>| DownloadRowView {
        detail,
        color,
        button,
        progress,
    };
    let remote_size = state.remote.as_ref().map(|remote| remote.total_size());
    match &state.activity {
        DownloadActivity::Checking => row("Checking for updates...".to_string(), DETAIL_GREY, None, None),
        DownloadActivity::Verifying { percent } => row(
            format!("Verifying the files... {percent} %"),
            DETAIL_GREY,
            None,
            Some(*percent as f32 / 100.0),
        ),
        DownloadActivity::Downloading { done, total } if *total > 0 => row(
            format!("Downloading {} % of {}", done * 100 / total, gigabytes(*total)),
            DETAIL_GREY,
            Some("Cancel"),
            Some(*done as f32 / *total as f32),
        ),
        DownloadActivity::Downloading { .. } => {
            row("Starting the download...".to_string(), DETAIL_GREY, Some("Cancel"), None)
        }
        DownloadActivity::Cancelling => row("Cancelling...".to_string(), DETAIL_GREY, None, None),
        DownloadActivity::Installing => row("Installing...".to_string(), DETAIL_GREY, None, None),
        DownloadActivity::Idle => {
            if let Some(error) = &state.error {
                // A failed download can be retried. A failed check is repeated with its own button.
                let retry = state.installed_dir.is_none() || state.up_to_date == Some(false);
                return row(format!("Failed: {error}"), (240, 128, 128), retry.then_some("Retry"), None);
            }
            if state.installed_dir.is_none() {
                return if state.partial_bytes > 0 {
                    row(
                        format!("Partly downloaded ({})", gigabytes(state.partial_bytes)),
                        DETAIL_GREY,
                        Some("Resume"),
                        None,
                    )
                } else {
                    let size = remote_size.map(|size| format!(", {}", gigabytes(size))).unwrap_or_default();
                    row(format!("Not downloaded{size}"), DETAIL_GREY, Some("Download"), None)
                };
            }
            match (state.up_to_date, &state.remote) {
                (Some(false), Some(remote)) => row(
                    format!("Update available: {}", remote.date),
                    (130, 176, 255),
                    Some("Update"),
                    None,
                ),
                (Some(true), _) => row(
                    format!("Version {}, up to date", state.local_date.as_deref().unwrap_or("unknown")),
                    (110, 200, 140),
                    None,
                    None,
                ),
                _ => match &state.local_date {
                    Some(date) => row(
                        format!("Version {date}, {}", gigabytes(state.installed_bytes)),
                        DETAIL_GREY,
                        None,
                        None,
                    ),
                    None => row(
                        format!("Downloaded, {}. Version unknown.", gigabytes(state.installed_bytes)),
                        DETAIL_GREY,
                        None,
                        None,
                    ),
                },
            }
        }
    }
}

/// Clicks on the Downloads section. Returns true when one was handled.
fn handle_downloads_click(app: &Arc<AppState>, x: i32, y: i32) -> bool {
    let downloads = downloads_snapshot(app);
    let busy = downloads.iter().any(ModelDownloadState::busy);
    if check_updates_rect().contains(x, y) {
        if !busy {
            check_for_model_updates(app);
        }
        return true;
    }
    for (index, state) in downloads.iter().enumerate() {
        if !download_button_rect(index).contains(x, y) {
            continue;
        }
        let choice = DOWNLOADABLE_MODELS[index];
        match download_row_view(state).button {
            Some("Cancel") => cancel_model_download(app, choice),
            Some(_) if busy => update_ui(app, |ui| {
                ui.status = "Wait until the running download or check has finished.".to_string();
            }),
            Some(_) => start_model_download(app, choice),
            None => {}
        }
        return true;
    }
    false
}

/// Pass 1 of the Downloads section: Check button, row buttons, progress bars.
fn draw_downloads_shapes(buf: &mut [u32], cw: i32, ch: i32, downloads: &[ModelDownloadState]) {
    draw_button_bg(buf, cw, ch, check_updates_rect(), false);
    for (index, state) in downloads.iter().enumerate() {
        let view = download_row_view(state);
        if let Some(label) = view.button {
            draw_button_bg(buf, cw, ch, download_button_rect(index), label != "Cancel");
        }
        if let Some(progress) = view.progress {
            let bar = download_progress_rect(index);
            let width = (bar.right - bar.left) as f32;
            let (x, y, h) = (bar.left as f32, bar.top as f32, (bar.bottom - bar.top) as f32);
            fill_round_rect(buf, cw, ch, x, y, width, h, 2.0, (40, 44, 54), (40, 44, 54), 1.0);
            let filled = (width * progress.clamp(0.0, 1.0)).max(h);
            fill_round_rect(buf, cw, ch, x, y, filled, h, 2.0, (110, 162, 255), (84, 138, 250), 1.0);
        }
    }
}

/// Pass 2 of the Downloads section: heading, model names, detail lines, button labels.
unsafe fn draw_downloads_text(dc: HDC, downloads: &[ModelDownloadState]) {
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(233, 238, 244));
    text_left(dc, "Downloads", WINDOW_INNER_LEFT, DOWNLOADS_SECTION_TOP + 1, 150, 20);
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(124, 134, 148));
    text_left(dc, "From Hugging Face.", WINDOW_INNER_LEFT, DOWNLOADS_SECTION_TOP + 21, 160, 18);
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(245, 248, 252));
    text_center(dc, "Check for updates", check_updates_rect());

    for (index, state) in downloads.iter().enumerate() {
        let view = download_row_view(state);
        let top = download_row_top(index);
        let button = download_button_rect(index);
        let text_right = if view.button.is_some() { button.left - 10 } else { WINDOW_INNER_RIGHT };
        SelectObject(dc, ui_font() as _);
        SetTextColor(dc, rgb(233, 238, 244));
        text_left(dc, DOWNLOADABLE_MODELS[index].label(), WINDOW_INNER_LEFT, top + 1, 200, 20);
        SelectObject(dc, small_font() as _);
        SetTextColor(dc, rgb(view.color.0, view.color.1, view.color.2));
        text_left_ellipsis(
            dc,
            &view.detail,
            RectI {
                left: WINDOW_INNER_LEFT,
                top: top + 21,
                right: text_right,
                bottom: top + 39,
            },
        );
        if let Some(label) = view.button {
            SelectObject(dc, ui_font() as _);
            SetTextColor(dc, rgb(245, 248, 252));
            text_center(dc, label, button);
        }
    }
}
