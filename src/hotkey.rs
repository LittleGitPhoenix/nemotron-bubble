//! The global shortcut: registration, labels and capturing a new combo.

use crate::*;

pub(crate) const HOTKEY_ID: i32 = 42;
pub(crate) const ENTER_HOTKEY_ID: i32 = 43;
pub(crate) unsafe fn register_enter_hotkey(hwnd: HWND) {
    if hwnd != null_mut() {
        RegisterHotKey(hwnd, ENTER_HOTKEY_ID, MOD_NOREPEAT, VK_RETURN as u32);
    }
}

pub(crate) unsafe fn unregister_enter_hotkey(hwnd: HWND) {
    if hwnd != null_mut() {
        UnregisterHotKey(hwnd, ENTER_HOTKEY_ID);
    }
}

/// (Re)register the global hotkey on `hwnd`. MOD_NOREPEAT stops held keys from
/// firing repeatedly. Returns false if the combo is reserved / already taken.
pub(crate) fn register_global_hotkey(hwnd: HWND, mods: u32, vk: u32) -> bool {
    if hwnd == null_mut() {
        return false;
    }
    unsafe {
        UnregisterHotKey(hwnd, HOTKEY_ID);
        RegisterHotKey(hwnd, HOTKEY_ID, mods | MOD_NOREPEAT, vk) != 0
    }
}

pub(crate) fn key_is_down(vk: u16) -> bool {
    unsafe { (GetKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

/// Localised name of a single key ("Space", "F8", "A", ...).
pub(crate) fn key_name(vk: u32) -> String {
    // Windows names Space in the system language ("LEER" on German systems),
    // which reads oddly next to the English "Ctrl"/"Alt"; name it ourselves.
    if vk == VK_SPACE as u32 {
        return "Space".to_string();
    }
    unsafe {
        let scan = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC);
        if scan != 0 {
            let lparam = (scan << 16) as i32;
            let mut buf = [0u16; 64];
            let len = GetKeyNameTextW(lparam, buf.as_mut_ptr(), buf.len() as i32);
            if len > 0 {
                return String::from_utf16_lossy(&buf[..len as usize]);
            }
        }
    }
    format!("Key {vk}")
}

/// Human-readable shortcut label like "Ctrl + Shift + Space".
pub(crate) fn hotkey_label(mods: u32, vk: u32) -> String {
    let mut parts: Vec<String> = Vec::new();
    if mods & MOD_CONTROL != 0 {
        parts.push("Ctrl".to_string());
    }
    if mods & MOD_ALT != 0 {
        parts.push("Alt".to_string());
    }
    if mods & MOD_SHIFT != 0 {
        parts.push("Shift".to_string());
    }
    if mods & MOD_WIN != 0 {
        parts.push("Win".to_string());
    }
    parts.push(key_name(vk));
    parts.join(" + ")
}

/// Enter capture mode: free the current hotkey so the next chord arrives as
/// WM_KEYDOWN, and pull keyboard focus to the settings window.
pub(crate) fn start_hotkey_capture(app: &Arc<AppState>) {
    app.capturing_hotkey.store(true, Ordering::SeqCst);
    unsafe {
        let bubble = hwnd_from_app(app);
        if bubble != null_mut() {
            UnregisterHotKey(bubble, HOTKEY_ID);
        }
        let settings = settings_hwnd_from_app(app);
        if settings != null_mut() {
            SetFocus(settings);
        }
    }
    update_ui(app, |ui| {
        ui.status = "Press your shortcut...  (Esc to cancel)".to_string();
    });
}

pub(crate) fn cancel_hotkey_capture(app: &Arc<AppState>) {
    if !app.capturing_hotkey.swap(false, Ordering::SeqCst) {
        return;
    }
    let s = settings_snapshot(app);
    register_global_hotkey(hwnd_from_app(app), s.hotkey_mods, s.hotkey_vk);
    update_ui(app, |ui| {
        ui.status = "Shortcut unchanged.".to_string();
    });
}

pub(crate) fn apply_captured_hotkey(app: &Arc<AppState>, mods: u32, vk: u32) {
    app.capturing_hotkey.store(false, Ordering::SeqCst);
    let bubble = hwnd_from_app(app);
    if register_global_hotkey(bubble, mods, vk) {
        {
            let mut s = app
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            s.hotkey_mods = mods;
            s.hotkey_vk = vk;
        }
        save_settings(&settings_snapshot(app));
        let label = hotkey_label(mods, vk);
        update_ui(app, |ui| {
            ui.status = format!("Shortcut set to {label}.");
        });
    } else {
        // Roll back to the previous (still-working) binding.
        let s = settings_snapshot(app);
        register_global_hotkey(bubble, s.hotkey_mods, s.hotkey_vk);
        update_ui(app, |ui| {
            ui.status = "That combo is unavailable - try another.".to_string();
        });
    }
}

/// Handle a key press while capturing a new shortcut.
pub(crate) fn handle_capture_key(app: &Arc<AppState>, vk: u32) {
    let v = vk as u16;
    if v == VK_CONTROL || v == VK_SHIFT || v == VK_MENU || v == VK_LWIN || v == VK_RWIN {
        return; // a bare modifier — keep waiting for the main key
    }
    if v == VK_ESCAPE {
        cancel_hotkey_capture(app);
        return;
    }
    let mut mods = 0u32;
    if key_is_down(VK_CONTROL) {
        mods |= MOD_CONTROL;
    }
    if key_is_down(VK_MENU) {
        mods |= MOD_ALT;
    }
    if key_is_down(VK_SHIFT) {
        mods |= MOD_SHIFT;
    }
    if key_is_down(VK_LWIN) || key_is_down(VK_RWIN) {
        mods |= MOD_WIN;
    }
    // Letters and digits would hijack normal typing without a modifier.
    let needs_mod = (0x30..=0x5A).contains(&vk);
    if mods == 0 && needs_mod {
        update_ui(app, |ui| {
            ui.status = "Add Ctrl, Alt, or Shift to that key.".to_string();
        });
        return; // stay in capture mode
    }
    apply_captured_hotkey(app, mods, vk);
}

