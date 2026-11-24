//! Getting text into other apps: clipboard, Ctrl+V and Unicode keystrokes.

use crate::*;

pub(crate) fn paste_live_tail(app: &Arc<AppState>, text: &str) {
    if text.is_empty() || !settings_snapshot(app).live_type_into_cursor {
        return;
    }

    let delay_ms = settings_snapshot(app).paste_delay_ms;
    if let Err(err) =
        type_unicode_text(text, hwnd_from_app(app), target_hwnd_from_app(app), delay_ms)
    {
        update_ui(app, |ui| {
            ui.status = format!("Live typing failed: {err}");
        });
    }
}

pub(crate) fn paste_text_to_target(
    text: &str,
    owner_hwnd: HWND,
    target_hwnd: HWND,
    delay_ms: u64,
) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }

    set_clipboard_text(text, owner_hwnd)?;

    unsafe {
        let target = if target_hwnd == null_mut() {
            GetForegroundWindow()
        } else {
            target_hwnd
        };

        if target != null_mut()
            && target != owner_hwnd
            && IsWindow(target) != 0
            && SetForegroundWindow(target) != 0
        {
            thread::sleep(Duration::from_millis(delay_ms));
        }

        send_ctrl_v()
    }
}

pub(crate) fn set_clipboard_text(text: &str, owner_hwnd: HWND) -> Result<()> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * std::mem::size_of::<u16>();

    unsafe {
        if OpenClipboard(owner_hwnd) == 0 {
            return Err(anyhow!("clipboard is busy"));
        }

        EmptyClipboard();
        let handle = GlobalAlloc(GMEM_MOVEABLE, byte_len);
        if handle == null_mut() {
            CloseClipboard();
            return Err(anyhow!("clipboard allocation failed"));
        }

        let locked = GlobalLock(handle) as *mut u16;
        if locked == null_mut() {
            GlobalFree(handle as HGLOBAL);
            CloseClipboard();
            return Err(anyhow!("clipboard lock failed"));
        }

        std::ptr::copy_nonoverlapping(wide.as_ptr(), locked, wide.len());
        GlobalUnlock(handle);

        if SetClipboardData(CF_UNICODETEXT as u32, handle) == null_mut() {
            GlobalFree(handle as HGLOBAL);
            CloseClipboard();
            return Err(anyhow!("clipboard set failed"));
        }

        CloseClipboard();
    }

    Ok(())
}

/// Prepend key-up events for any modifier the user is physically holding, so an
/// injected keystroke isn't folded into a Ctrl/Alt/Shift/Win shortcut (e.g. the
/// Ctrl from the Ctrl+Space hotkey turning a typed char, a paste, or Enter into a
/// command). Shared by every injection path.
pub(crate) unsafe fn push_modifier_releases(inputs: &mut Vec<INPUT>) {
    for vk in [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN] {
        if (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 {
            inputs.push(keyboard_input(vk, KEYEVENTF_KEYUP));
        }
    }
}

pub(crate) unsafe fn send_ctrl_v() -> Result<()> {
    let mut inputs: Vec<INPUT> = Vec::with_capacity(8);
    push_modifier_releases(&mut inputs); // clear stray Shift/Alt/Win before our clean Ctrl+V
    inputs.push(keyboard_input(VK_CONTROL, 0));
    inputs.push(keyboard_input('V' as u16, 0));
    inputs.push(keyboard_input('V' as u16, KEYEVENTF_KEYUP));
    inputs.push(keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP));

    let sent = SendInput(
        inputs.len() as u32,
        inputs.as_ptr(),
        std::mem::size_of::<INPUT>() as i32,
    );

    if sent != inputs.len() as u32 {
        return Err(anyhow!("Ctrl+V injection failed"));
    }

    Ok(())
}

pub(crate) fn keyboard_input(vk: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

pub(crate) fn unicode_input(unit: u16, extra_flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: unit,
                dwFlags: KEYEVENTF_UNICODE | extra_flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Type text directly as synthesized Unicode keystrokes. Unlike the Ctrl+V path
/// this never touches the clipboard, so live dictation can stream into the cursor
/// without clobbering whatever the user has copied. UTF-16 (incl. surrogate pairs)
/// is sent verbatim.
///
/// Critically, it first releases any modifier the user is physically holding
/// (e.g. the Ctrl from the Ctrl+Space stop hotkey). Otherwise Windows folds the
/// injected characters into Ctrl/Alt+key shortcuts and the focused app fires them
/// (opening tabs, switching apps, etc.).
pub(crate) unsafe fn send_unicode(text: &str) -> Result<()> {
    let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 2 + 4);
    push_modifier_releases(&mut inputs);
    for unit in text.encode_utf16() {
        inputs.push(unicode_input(unit, 0));
        inputs.push(unicode_input(unit, KEYEVENTF_KEYUP));
    }
    if inputs.is_empty() {
        return Ok(());
    }
    let sent = SendInput(
        inputs.len() as u32,
        inputs.as_ptr(),
        std::mem::size_of::<INPUT>() as i32,
    );
    if sent != inputs.len() as u32 {
        return Err(anyhow!("unicode injection failed"));
    }
    Ok(())
}

/// Synthesize a single Enter key press (used by Enter-to-stop to submit the text
/// after the full transcript has been typed).
pub(crate) unsafe fn send_enter() -> Result<()> {
    let mut inputs: Vec<INPUT> = Vec::with_capacity(7);
    push_modifier_releases(&mut inputs); // ensure a clean Enter, not Ctrl/Shift+Enter
    inputs.push(keyboard_input(VK_RETURN, 0));
    inputs.push(keyboard_input(VK_RETURN, KEYEVENTF_KEYUP));
    let sent = SendInput(
        inputs.len() as u32,
        inputs.as_ptr(),
        std::mem::size_of::<INPUT>() as i32,
    );
    if sent != inputs.len() as u32 {
        return Err(anyhow!("Enter injection failed"));
    }
    Ok(())
}

/// Live-type a chunk into the remembered target window without using the clipboard.
/// Only steals focus back to the target if our own window is currently foreground
/// (e.g. recording was started from the tray menu); otherwise it types into whatever
/// already has focus, keeping things snappy with no per-chunk focus flicker.
pub(crate) fn type_unicode_text(
    text: &str,
    owner_hwnd: HWND,
    target_hwnd: HWND,
    delay_ms: u64,
) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }
    unsafe {
        let foreground = GetForegroundWindow();
        // Only grab focus back if our own window currently holds it (e.g. recording
        // was started from the tray). Validate the target still exists first so a
        // stale handle can't redirect keystrokes into the wrong window.
        if (foreground == null_mut() || foreground == owner_hwnd)
            && target_hwnd != null_mut()
            && target_hwnd != owner_hwnd
            && IsWindow(target_hwnd) != 0
            && SetForegroundWindow(target_hwnd) != 0
        {
            thread::sleep(Duration::from_millis(delay_ms));
        }
        send_unicode(text)
    }
}

