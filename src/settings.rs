//! Settings: the persisted options, their choice types and loading/saving, plus the portable file locations.

use crate::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppSettings {
    pub(crate) start_with_windows: bool,
    pub(crate) preload_model: bool,
    pub(crate) live_type_into_cursor: bool,
    pub(crate) copy_final_to_clipboard: bool,
    pub(crate) paste_final_on_stop: bool,
    pub(crate) sounds_enabled: bool,
    pub(crate) waveform_enabled: bool,
    /// Called "bubble_click_opens_settings" before 1.0.0, still read under that name.
    #[serde(alias = "bubble_click_opens_settings")]
    pub(crate) bubble_click_opens_window: bool,
    pub(crate) show_floating_bubble: bool,
    pub(crate) tray_waveform_enabled: bool,
    pub(crate) enter_stops_recording: bool,
    pub(crate) silence_auto_stop: bool,
    pub(crate) hotkey_mods: u32,
    pub(crate) hotkey_vk: u32,
    pub(crate) paste_delay_ms: u64,
    pub(crate) history_limit: usize,
    /// Which speech model to load (chosen in the application window). When missing
    /// from settings.json (first start), it is detected; see `detect_model_choice`.
    pub(crate) model: ModelChoice,
    /// Model folder used by the Predefined choice (after NEMOTRON_MODEL_DIR).
    pub(crate) model_dir: String,
    /// Spoken language for the multilingual model: a code from LANGUAGES, or
    /// "auto" to let the model detect it. Ignored by the English-only model.
    pub(crate) language: String,
    /// Where the model runs: GPU (DirectML), CPU, or Auto (GPU, else CPU).
    pub(crate) compute: ComputeChoice,
    /// Screen position of the floating bubble (one of six presets).
    pub(crate) bubble_position: BubblePosition,
    /// Input device by its Windows name, e.g. "Microphone (USB Audio)". Empty:
    /// follow the Windows default input device.
    pub(crate) microphone: String,
}

/// Preset positions of the floating bubble, in the primary monitor's work area
/// (the screen minus the taskbar). Stored as e.g. "bottom-center" in settings.json.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub(crate) enum BubblePosition {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    #[default]
    BottomRight,
}

impl BubblePosition {
    /// Menu order.
    pub(crate) const ALL: [BubblePosition; 6] = [
        BubblePosition::TopLeft,
        BubblePosition::TopCenter,
        BubblePosition::TopRight,
        BubblePosition::BottomLeft,
        BubblePosition::BottomCenter,
        BubblePosition::BottomRight,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            BubblePosition::TopLeft => "Top left",
            BubblePosition::TopCenter => "Top center",
            BubblePosition::TopRight => "Top right",
            BubblePosition::BottomLeft => "Bottom left",
            BubblePosition::BottomCenter => "Bottom center",
            BubblePosition::BottomRight => "Bottom right",
        }
    }

    /// The settings.json spelling.
    pub(crate) fn key(self) -> &'static str {
        match self {
            BubblePosition::TopLeft => "top-left",
            BubblePosition::TopCenter => "top-center",
            BubblePosition::TopRight => "top-right",
            BubblePosition::BottomLeft => "bottom-left",
            BubblePosition::BottomCenter => "bottom-center",
            BubblePosition::BottomRight => "bottom-right",
        }
    }
}

impl From<String> for BubblePosition {
    fn from(value: String) -> Self {
        BubblePosition::ALL
            .into_iter()
            .find(|position| position.key().eq_ignore_ascii_case(value.trim()))
            .unwrap_or_default()
    }
}

impl From<BubblePosition> for String {
    fn from(value: BubblePosition) -> Self {
        value.key().to_string()
    }
}

/// The "Processing" setting. Stored as a plain string in settings.json; unknown
/// values fall back to the default (CPU).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub(crate) enum ComputeChoice {
    /// The default: an integrated GPU is often slower than the CPU for these
    /// small streaming chunks (measured: ~290 ms vs ~130 ms per chunk).
    #[default]
    Cpu,
    /// GPU only; loading fails with an error when no usable GPU is found.
    Gpu,
    /// GPU via DirectML when available, otherwise the CPU.
    Auto,
}

impl ComputeChoice {
    /// Display order in the settings switch and the tray submenu.
    pub(crate) const ALL: [ComputeChoice; 3] = [ComputeChoice::Cpu, ComputeChoice::Gpu, ComputeChoice::Auto];

    pub(crate) fn label(self) -> &'static str {
        match self {
            ComputeChoice::Auto => "Auto",
            ComputeChoice::Gpu => "GPU",
            ComputeChoice::Cpu => "CPU",
        }
    }
}

impl From<String> for ComputeChoice {
    fn from(value: String) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "gpu" => ComputeChoice::Gpu,
            "auto" => ComputeChoice::Auto,
            _ => ComputeChoice::Cpu,
        }
    }
}

impl From<ComputeChoice> for String {
    fn from(value: ComputeChoice) -> Self {
        value.label().to_ascii_lowercase()
    }
}

/// Languages offered for the multilingual model: "auto" plus NVIDIA's
/// transcription-ready languages. The codes are parakeet-rs prompt keys
/// (Japanese and Vietnamese only exist with their region suffix).
pub(crate) const LANGUAGES: &[(&str, &str)] = &[
    ("auto", "Auto-detect"),
    ("ar", "Arabic"),
    ("nl", "Dutch"),
    ("en", "English"),
    ("fr", "French"),
    ("de", "German"),
    ("hi", "Hindi"),
    ("it", "Italian"),
    ("ja-JP", "Japanese"),
    ("ko", "Korean"),
    ("pt", "Portuguese"),
    ("ru", "Russian"),
    ("es", "Spanish"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
    ("vi-VN", "Vietnamese"),
];

/// The canonical code for a saved language value; unknown values become "auto".
pub(crate) fn normalize_language(code: &str) -> &'static str {
    LANGUAGES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(code.trim()))
        .map(|(known, _)| *known)
        .unwrap_or("auto")
}

pub(crate) fn language_label(code: &str) -> &'static str {
    let code = normalize_language(code);
    LANGUAGES
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, label)| *label)
        .unwrap_or("Auto-detect")
}

/// Speech model selection. Stored as a plain string in settings.json.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub(crate) enum ModelChoice {
    /// The folder from NEMOTRON_MODEL_DIR, else `model_dir` in settings.json.
    Predefined,
    /// `models\nemotron`.
    #[default]
    English,
    /// `models\nemotron_multi`; the model detects the spoken language itself.
    Multilingual,
}

impl ModelChoice {
    /// Display order in the settings switch and the tray submenu.
    pub(crate) const ALL: [ModelChoice; 3] = [
        ModelChoice::English,
        ModelChoice::Multilingual,
        ModelChoice::Predefined,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            ModelChoice::Predefined => "Predefined",
            ModelChoice::English => "English",
            ModelChoice::Multilingual => "Multilingual",
        }
    }

    /// The settings.json spelling.
    pub(crate) fn key(self) -> &'static str {
        match self {
            ModelChoice::Predefined => "predefined",
            ModelChoice::English => "english",
            ModelChoice::Multilingual => "multilingual",
        }
    }

    /// Parse a settings.json value; None for unknown values (e.g. the old "auto").
    pub(crate) fn parse(value: &str) -> Option<ModelChoice> {
        ModelChoice::ALL
            .into_iter()
            .find(|choice| choice.key().eq_ignore_ascii_case(value.trim()))
    }

    /// Folder name under `models\` for the standard choices; None for Predefined.
    pub(crate) fn folder_name(self) -> Option<&'static str> {
        match self {
            ModelChoice::Predefined => None,
            ModelChoice::English => Some("nemotron"),
            ModelChoice::Multilingual => Some("nemotron_multi"),
        }
    }
}

impl From<String> for ModelChoice {
    /// Unknown values never invalidate the settings file; `load_settings`
    /// re-detects the choice for them.
    fn from(value: String) -> Self {
        ModelChoice::parse(&value).unwrap_or_default()
    }
}

impl From<ModelChoice> for String {
    fn from(value: ModelChoice) -> Self {
        value.key().to_string()
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            // Off by default: the app is portable and may be tried out and deleted
            // or moved again, so it must not register itself without being asked.
            start_with_windows: false,
            preload_model: true,
            // Default to the safe clipboard path: live keystroke injection only behaves
            // in real text fields, so it is opt-in. On stop we paste once (one Ctrl+V).
            live_type_into_cursor: false,
            copy_final_to_clipboard: true,
            paste_final_on_stop: true,
            sounds_enabled: true,
            waveform_enabled: true,
            bubble_click_opens_window: true,
            show_floating_bubble: true,
            tray_waveform_enabled: true,
            enter_stops_recording: false,
            silence_auto_stop: true,
            hotkey_mods: MOD_CONTROL,
            hotkey_vk: VK_SPACE as u32,
            paste_delay_ms: 60,
            history_limit: 20,
            model: ModelChoice::English,
            model_dir: String::new(),
            language: "auto".to_string(),
            compute: ComputeChoice::Cpu,
            bubble_position: BubblePosition::BottomRight,
            microphone: String::new(),
        }
    }
}

pub(crate) fn apply_startup_setting(enabled: bool) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok((run_key, _)) =
        hkcu.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
    else {
        return;
    };

    let Ok(exe) = env::current_exe() else {
        return;
    };
    let value = format!("\"{}\"", exe.display());
    if enabled {
        let _ = run_key.set_value("NemotronBubble", &value);
    } else {
        // Only remove an entry that starts this exe. Another copy of the portable
        // app in a different folder may own it, and a copy that is merely tried
        // out (autostart off by default) must not unregister that one.
        let current: Result<String, _> = run_key.get_value("NemotronBubble");
        if current.is_ok_and(|current| current.eq_ignore_ascii_case(&value)) {
            let _ = run_key.delete_value("NemotronBubble");
        }
    }
}

pub(crate) fn settings_snapshot(app: &Arc<AppState>) -> AppSettings {
    app.settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

pub(crate) fn toggle_setting(app: &Arc<AppState>, id: usize) {
    let settings = {
        let mut settings = app
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match id {
            SETTINGS_TOGGLE_STARTUP => settings.start_with_windows = !settings.start_with_windows,
            SETTINGS_TOGGLE_PRELOAD => settings.preload_model = !settings.preload_model,
            SETTINGS_TOGGLE_LIVE_TYPE => {
                settings.live_type_into_cursor = !settings.live_type_into_cursor
            }
            SETTINGS_TOGGLE_FINAL_CLIPBOARD => {
                settings.copy_final_to_clipboard = !settings.copy_final_to_clipboard
            }
            SETTINGS_TOGGLE_FINAL_PASTE => {
                settings.paste_final_on_stop = !settings.paste_final_on_stop
            }
            SETTINGS_TOGGLE_SOUNDS => settings.sounds_enabled = !settings.sounds_enabled,
            SETTINGS_TOGGLE_WAVEFORM => settings.waveform_enabled = !settings.waveform_enabled,
            SETTINGS_TOGGLE_BUBBLE_CLICK_WINDOW => {
                settings.bubble_click_opens_window = !settings.bubble_click_opens_window
            }
            SETTINGS_TOGGLE_FLOATING_BUBBLE => {
                settings.show_floating_bubble = !settings.show_floating_bubble
            }
            SETTINGS_TOGGLE_TRAY_WAVEFORM => {
                settings.tray_waveform_enabled = !settings.tray_waveform_enabled
            }
            SETTINGS_TOGGLE_ENTER_STOPS => {
                settings.enter_stops_recording = !settings.enter_stops_recording
            }
            SETTINGS_TOGGLE_SILENCE_STOP => {
                settings.silence_auto_stop = !settings.silence_auto_stop
            }
            _ => {}
        }
        settings.clone()
    };

    save_settings(&settings);
    apply_startup_setting(settings.start_with_windows);
    // If toggled during a live recording, add/remove the Enter hotkey immediately.
    if id == SETTINGS_TOGGLE_ENTER_STOPS && app.recording.load(Ordering::SeqCst) {
        unsafe {
            let hwnd = hwnd_from_app(app);
            if settings.enter_stops_recording {
                register_enter_hotkey(hwnd);
            } else {
                unregister_enter_hotkey(hwnd);
            }
        }
    }
    if settings.preload_model {
        let _ = app.engine_tx.send(EngineCommand::Preload);
    }
    unsafe {
        let hwnd = hwnd_from_app(app);
        if hwnd != null_mut() {
            if settings.show_floating_bubble {
                render_bubble(hwnd);
                ShowWindow(hwnd, SW_SHOWNA);
            } else {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
    }
    refresh_tray_idle(app);
    update_ui(app, |ui| {
        ui.status = "Settings saved.".to_string();
    });
}

pub(crate) fn load_settings() -> AppSettings {
    let path = settings_path();
    let Ok(json) = read_text_file(&path) else {
        // First start: no settings yet, so pick the model from what is installed.
        let mut settings = AppSettings::default();
        settings.model = detect_model_choice(&settings.model_dir);
        return settings;
    };
    match serde_json::from_str::<AppSettings>(&json) {
        Ok(mut settings) => {
            // No (valid) model choice saved yet, e.g. settings from before the
            // model picker or the old "auto" value: detect it like a first start.
            let saved = serde_json::from_str::<serde_json::Value>(&json)
                .ok()
                .and_then(|value| value.get("model").and_then(|m| m.as_str()).map(str::to_string));
            if saved.as_deref().and_then(ModelChoice::parse).is_none() {
                settings.model = detect_model_choice(&settings.model_dir);
            }
            settings
        }
        Err(err) => {
            // The file is rewritten with defaults right after this, so keep a copy.
            let backup = path.with_extension("json.invalid");
            let _ = fs::copy(&path, &backup);
            log_error(&format!(
                "{} is invalid ({err}); using defaults. Old file kept as {}",
                path.display(),
                backup.display()
            ));
            let mut settings = AppSettings::default();
            settings.model = detect_model_choice(&settings.model_dir);
            settings
        }
    }
}

pub(crate) fn save_settings(settings: &AppSettings) {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = fs::write(path, json);
    }
}

pub(crate) fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub(crate) fn history_path() -> PathBuf {
    config_dir().join("history.json")
}

/// The folder containing the exe. The app is portable: everything it writes
/// lives below this folder. Deliberately NOT the working directory — "Start with
/// Windows" launches us with the working directory set to System32.
pub(crate) fn app_root_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Settings, history and generated sound cues: `<exe dir>\config`.
pub(crate) fn config_dir() -> PathBuf {
    app_root_dir().join("config")
}

/// Diagnostic log and crash log: `<exe dir>\log`.
pub(crate) fn log_dir() -> PathBuf {
    app_root_dir().join("log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_language_returns_canonical_code() {
        assert_eq!(normalize_language("de"), "de");
        assert_eq!(normalize_language(" DE "), "de");
        assert_eq!(normalize_language("ja-jp"), "ja-JP");
    }

    #[test]
    fn normalize_language_falls_back_to_auto() {
        assert_eq!(normalize_language("xx"), "auto");
        assert_eq!(normalize_language(""), "auto");
        // Japanese only exists with its region suffix.
        assert_eq!(normalize_language("ja"), "auto");
    }

    #[test]
    fn language_label_matches_code() {
        assert_eq!(language_label("de"), "German");
        assert_eq!(language_label("VI-vn"), "Vietnamese");
        assert_eq!(language_label("unknown"), "Auto-detect");
    }

    #[test]
    fn language_codes_are_unique() {
        for (index, (code, _)) in LANGUAGES.iter().enumerate() {
            assert!(
                LANGUAGES[index + 1..].iter().all(|(other, _)| !other.eq_ignore_ascii_case(code)),
                "{code} is listed twice"
            );
        }
    }

    #[test]
    fn bubble_position_round_trips_through_its_key() {
        for position in BubblePosition::ALL {
            assert_eq!(BubblePosition::from(String::from(position)), position);
        }
        assert_eq!(BubblePosition::from(" Top-Center ".to_string()), BubblePosition::TopCenter);
        assert_eq!(BubblePosition::from("middle".to_string()), BubblePosition::BottomRight);
    }

    #[test]
    fn compute_choice_round_trips_and_defaults_to_cpu() {
        for choice in ComputeChoice::ALL {
            assert_eq!(ComputeChoice::from(String::from(choice)), choice);
        }
        assert_eq!(ComputeChoice::from(" GPU ".to_string()), ComputeChoice::Gpu);
        assert_eq!(ComputeChoice::from("quantum".to_string()), ComputeChoice::Cpu);
    }

    #[test]
    fn model_choice_parse_rejects_unknown_values() {
        for choice in ModelChoice::ALL {
            assert_eq!(ModelChoice::parse(choice.key()), Some(choice));
        }
        assert_eq!(ModelChoice::parse(" Multilingual "), Some(ModelChoice::Multilingual));
        // The old "auto" value must be re-detected by load_settings.
        assert_eq!(ModelChoice::parse("auto"), None);
    }

    #[test]
    fn model_choice_folder_names() {
        assert_eq!(ModelChoice::English.folder_name(), Some("nemotron"));
        assert_eq!(ModelChoice::Multilingual.folder_name(), Some("nemotron_multi"));
        assert_eq!(ModelChoice::Predefined.folder_name(), None);
    }

    fn to_json(settings: &AppSettings) -> serde_json::Value {
        serde_json::to_value(settings).expect("settings serialize")
    }

    #[test]
    fn empty_settings_file_yields_defaults() {
        let settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(to_json(&settings), to_json(&AppSettings::default()));
    }

    #[test]
    fn partial_settings_keep_defaults_for_missing_fields() {
        let settings: AppSettings =
            serde_json::from_str(r#"{ "model": "multilingual", "language": "de", "unknown_field": 1 }"#).unwrap();
        assert_eq!(settings.model, ModelChoice::Multilingual);
        assert_eq!(settings.language, "de");
        assert_eq!(settings.history_limit, AppSettings::default().history_limit);
    }

    #[test]
    fn invalid_choice_values_do_not_invalidate_settings() {
        let settings: AppSettings =
            serde_json::from_str(r#"{ "compute": "quantum", "bubble_position": "middle", "model": "auto" }"#).unwrap();
        assert_eq!(settings.compute, ComputeChoice::Cpu);
        assert_eq!(settings.bubble_position, BubblePosition::BottomRight);
        assert_eq!(settings.model, ModelChoice::English);
    }

    #[test]
    fn settings_round_trip_with_settings_json_spelling() {
        let mut settings = AppSettings::default();
        settings.model = ModelChoice::Predefined;
        settings.compute = ComputeChoice::Auto;
        settings.bubble_position = BubblePosition::TopLeft;
        settings.microphone = "Microphone (USB Audio)".to_string();

        let json = to_json(&settings);
        assert_eq!(json["model"], "predefined");
        assert_eq!(json["compute"], "auto");
        assert_eq!(json["bubble_position"], "top-left");

        let restored: AppSettings = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(to_json(&restored), json);
    }

    #[test]
    fn old_bubble_click_key_is_still_read() {
        let settings: AppSettings = serde_json::from_str(r#"{ "bubble_click_opens_settings": false }"#).unwrap();
        assert!(!settings.bubble_click_opens_window);
        assert!(to_json(&settings).get("bubble_click_opens_settings").is_none());
    }
}
