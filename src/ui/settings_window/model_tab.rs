//! The Model tab: Processing, Speech model, Language and Preload.

use crate::*;

// Model tab: Processing, Speech model, Language, Preload (top to bottom)
pub(crate) const COMPUTE_SECTION_TOP: i32 = 132;
pub(crate) const MODEL_SECTION_TOP: i32 = 230;
pub(crate) const LANGUAGE_ROW_TOP: i32 = 328;
pub(crate) const PRELOAD_ROW_TOP: i32 = 384;

/// Popup menu under the settings window's Language button.
pub(crate) unsafe fn show_language_menu(app: &Arc<AppState>) {
    let menu = build_language_menu(&settings_snapshot(app), app.recording.load(Ordering::SeqCst));
    show_menu_below(app, menu, settings_language_rect());
}

/// The Language menu (settings popup and tray submenu): current one checked,
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
pub(crate) fn settings_compute_track_rect() -> RectI {
    section_track_rect(COMPUTE_SECTION_TOP)
}

/// One segment of the Processing control.
pub(crate) fn settings_compute_rect(choice: ComputeChoice) -> RectI {
    let index = ComputeChoice::ALL.iter().position(|c| *c == choice).unwrap_or(0);
    segment_rect(settings_compute_track_rect(), index, ComputeChoice::ALL.len())
}

/// Background track of the speech-model segmented control.
pub(crate) fn settings_model_track_rect() -> RectI {
    section_track_rect(MODEL_SECTION_TOP)
}

/// One segment (English / Multilingual / Predefined) of the speech-model control.
pub(crate) fn settings_model_rect(choice: ModelChoice) -> RectI {
    let index = ModelChoice::ALL.iter().position(|c| *c == choice).unwrap_or(0);
    segment_rect(settings_model_track_rect(), index, ModelChoice::ALL.len())
}

/// The Language button (opens the language popup menu), right of its label.
pub(crate) fn settings_language_rect() -> RectI {
    RectI {
        left: SETTINGS_INNER_RIGHT - 140,
        top: LANGUAGE_ROW_TOP + 4,
        right: SETTINGS_INNER_RIGHT,
        bottom: LANGUAGE_ROW_TOP + 36,
    }
}

pub(crate) fn handle_model_tab_click(app: &Arc<AppState>, x: i32, y: i32) {
    for choice in ModelChoice::ALL {
        if settings_model_rect(choice).contains(x, y) {
            select_model(app, choice);
            return;
        }
    }

    for choice in ComputeChoice::ALL {
        if settings_compute_rect(choice).contains(x, y) {
            select_compute(app, choice);
            return;
        }
    }

    if settings_language_rect().contains(x, y) {
        if language_selectable(&settings_snapshot(app)) {
            unsafe { show_language_menu(app) };
        } else {
            update_ui(app, |ui| {
                ui.status = "The language applies to the multilingual model only.".to_string();
            });
        }
        return;
    }

    if settings_toggle_rect(SETTINGS_TOGGLE_PRELOAD).contains(x, y) {
        toggle_setting(app, SETTINGS_TOGGLE_PRELOAD);
    }
}


/// Pass 1 of the Model tab: segmented controls, dividers, Language button, Preload switch.
pub(crate) fn draw_model_shapes(buf: &mut [u32], cw: i32, ch: i32, settings: &AppSettings) {
    draw_segment_track(buf, cw, ch, settings_compute_track_rect());
    draw_button_bg(buf, cw, ch, settings_compute_rect(settings.compute), true);
    draw_divider(buf, cw, ch, MODEL_SECTION_TOP - 12);
    draw_segment_track(buf, cw, ch, settings_model_track_rect());
    draw_button_bg(buf, cw, ch, settings_model_rect(settings.model), true);
    draw_divider(buf, cw, ch, LANGUAGE_ROW_TOP - 12);
    draw_button_bg(buf, cw, ch, settings_language_rect(), false);
    draw_divider(buf, cw, ch, PRELOAD_ROW_TOP - 8);
    draw_toggle(buf, cw, ch, settings_toggle_rect(SETTINGS_TOGGLE_PRELOAD), settings.preload_model);
}

/// Pass 2 of the Model tab: section headings, segment labels, Language and Preload texts.
pub(crate) unsafe fn draw_model_text(dc: HDC, settings: &AppSettings, model_status: &str, rows: &[SettingRow]) {
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
        text_center(dc, choice.label(), settings_compute_rect(choice));
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
        text_center(dc, choice.label(), settings_model_rect(choice));
    }

    // Language: label + detail on the left, popup button on the right
    SelectObject(dc, ui_font() as _);
    SetTextColor(dc, rgb(233, 238, 244));
    text_left(dc, "Language", SETTINGS_INNER_LEFT, LANGUAGE_ROW_TOP + 1, 150, 20);
    SelectObject(dc, small_font() as _);
    SetTextColor(dc, rgb(124, 134, 148));
    text_left(dc, "Multilingual model only.", SETTINGS_INNER_LEFT, LANGUAGE_ROW_TOP + 21, 160, 18);
    SelectObject(dc, ui_font() as _);
    if language_selectable(settings) {
        SetTextColor(dc, rgb(245, 248, 252));
    } else {
        SetTextColor(dc, rgb(92, 100, 114));
    }
    text_center(
        dc,
        &format!("{}  \u{25BE}", language_label(&settings.language)),
        settings_language_rect(),
    );

    // Preload toggle
    if let Some(row) = rows.iter().find(|row| row.id == SETTINGS_TOGGLE_PRELOAD) {
        draw_toggle_text(dc, row, settings_toggle_rect(row.id));
    }
}
