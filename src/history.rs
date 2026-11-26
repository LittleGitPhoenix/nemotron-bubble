//! Recent dictations: entries, persistence and the list state of the History tab.

use crate::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct HistoryItem {
    pub(crate) timestamp: String,
    pub(crate) text: String,
    /// Where the dictation was processed: "CPU" or "GPU". Missing in older entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) processing: Option<String>,
    /// Average inference time per audio chunk (560 ms with the downloadable models), in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ms_per_chunk: Option<u32>,
}

impl HistoryItem {
    /// Header line shown above the text, e.g. "Nov 24, 11:34 AM · CPU · 130 ms per chunk".
    pub(crate) fn header(&self) -> String {
        match (&self.processing, self.ms_per_chunk) {
            (Some(device), Some(ms)) => format!("{} · {device} · {ms} ms per chunk", self.timestamp),
            _ => self.timestamp.clone(),
        }
    }
}

/// Processing statistics of one dictation, stored with its history entry.
pub(crate) struct DictationStats {
    pub(crate) on_gpu: bool,
    pub(crate) ms_per_chunk: f32,
}

pub(crate) fn history_snapshot(app: &Arc<AppState>) -> Vec<HistoryItem> {
    app.history
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

pub(crate) fn load_history(limit: usize) -> Vec<HistoryItem> {
    let path = history_path();
    let mut history = read_text_file(&path)
        .ok()
        .and_then(|json| serde_json::from_str::<Vec<HistoryItem>>(&json).ok())
        .unwrap_or_default();
    history.truncate(limit);
    history
}

pub(crate) fn save_history(history: &[HistoryItem]) {
    let path = history_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(history) {
        let _ = fs::write(path, json);
    }
}

pub(crate) fn add_history(app: &Arc<AppState>, text: String, stats: Option<DictationStats>) {
    let clean = text.trim().to_string();
    if clean.is_empty() {
        return;
    }
    let limit = settings_snapshot(app).history_limit;
    let mut history = app
        .history
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    history.insert(
        0,
        HistoryItem {
            timestamp: Local::now().format("%b %-d, %-I:%M %p").to_string(),
            text: clean,
            processing: stats
                .as_ref()
                .map(|s| if s.on_gpu { "GPU" } else { "CPU" }.to_string()),
            ms_per_chunk: stats.as_ref().map(|s| s.ms_per_chunk.round() as u32),
        },
    );
    history.truncate(limit);
    save_history(&history);
    drop(history);
    refresh_history_view(app);
}

/// After the history changed: scroll back to the newest entry and repaint the
/// settings window (posted, so this is safe from the engine thread too).
pub(crate) fn refresh_history_view(app: &Arc<AppState>) {
    app.history_scroll.store(0, Ordering::SeqCst);
    *app.copied_flash.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    unsafe {
        let hwnd = settings_hwnd_from_app(app);
        if hwnd != null_mut() {
            PostMessageW(hwnd, WM_UI_UPDATE, 0, 0);
        }
    }
}

/// Copy one history entry to the clipboard and flash its icon as a checkmark.
pub(crate) fn copy_history_item(app: &Arc<AppState>, index: usize) {
    let Some(item) = history_snapshot(app).get(index).cloned() else {
        return;
    };
    let hwnd = settings_hwnd_from_app(app);
    match set_clipboard_text(&item.text, hwnd) {
        Ok(()) => {
            *app.copied_flash.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(index);
            unsafe {
                if hwnd != null_mut() {
                    SetTimer(hwnd, COPY_FLASH_TIMER_ID, COPY_FLASH_MS, None);
                    InvalidateRect(hwnd, null(), 0);
                }
            }
        }
        Err(err) => log_error(&format!("Copying a history entry failed: {err:#}")),
    }
}

/// Scroll the history list by `delta` pixels (positive = further down).
pub(crate) fn scroll_history(app: &Arc<AppState>, delta: i32) {
    let view_h = HISTORY_LIST_BOTTOM - HISTORY_LIST_TOP;
    let max = (app.history_content_h.load(Ordering::SeqCst) - view_h).max(0);
    let old = app.history_scroll.load(Ordering::SeqCst);
    let new = (old + delta).clamp(0, max);
    if new != old {
        app.history_scroll.store(new, Ordering::SeqCst);
        unsafe {
            let hwnd = settings_hwnd_from_app(app);
            if hwnd != null_mut() {
                InvalidateRect(hwnd, null(), 0);
            }
        }
    }
}

pub(crate) fn clear_history(app: &Arc<AppState>) {
    {
        let mut history = app
            .history
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        history.clear();
        save_history(&history);
    }
    update_ui(app, |ui| {
        ui.status = "History cleared.".to_string();
    });
    refresh_history_view(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(processing: Option<&str>, ms_per_chunk: Option<u32>) -> HistoryItem {
        HistoryItem {
            timestamp: "Nov 24, 11:34 AM".to_string(),
            text: "Hello".to_string(),
            processing: processing.map(str::to_string),
            ms_per_chunk,
        }
    }

    #[test]
    fn header_includes_statistics_when_known() {
        assert_eq!(
            item(Some("CPU"), Some(130)).header(),
            "Nov 24, 11:34 AM · CPU · 130 ms per chunk"
        );
    }

    #[test]
    fn header_is_timestamp_only_without_statistics() {
        assert_eq!(item(None, None).header(), "Nov 24, 11:34 AM");
        assert_eq!(item(Some("GPU"), None).header(), "Nov 24, 11:34 AM");
    }

    #[test]
    fn older_entries_without_statistics_still_load() {
        let items: Vec<HistoryItem> =
            serde_json::from_str(r#"[{ "timestamp": "Nov 1, 9:00 AM", "text": "Old" }]"#).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].processing, None);
        assert_eq!(items[0].ms_per_chunk, None);
    }

    #[test]
    fn missing_statistics_are_not_written() {
        let json = serde_json::to_value(item(None, None)).unwrap();
        assert!(json.get("processing").is_none());
        assert!(json.get("ms_per_chunk").is_none());

        let json = serde_json::to_value(item(Some("GPU"), Some(290))).unwrap();
        assert_eq!(json["processing"], "GPU");
        assert_eq!(json["ms_per_chunk"], 290);
    }
}

