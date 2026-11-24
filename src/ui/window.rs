//! Window class, message loop and message dispatch.

use crate::*;

pub(crate) const CLASS_NAME: &str = "NemotronBubbleWindow";
pub(crate) const WM_UI_UPDATE: u32 = WM_APP + 1;

pub(crate) unsafe fn run_window(app: Arc<AppState>) -> Result<()> {
    let hinstance = GetModuleHandleW(null());
    if hinstance == null_mut() {
        return Err(anyhow!("GetModuleHandleW failed"));
    }

    let class_name = wide_null(CLASS_NAME);
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: null_mut(),
        lpszClassName: class_name.as_ptr(),
        ..std::mem::zeroed()
    };

    if RegisterClassW(&wc) == 0 {
        return Err(anyhow!("RegisterClassW failed"));
    }

    let title = wide_null("Nemotron Bubble");
    // No WS_VISIBLE: per-pixel-alpha layered windows are made visible only after the
    // first UpdateLayeredWindow so there is never a blank flash.
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_POPUP,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        WIDTH,
        HEIGHT,
        null_mut(),
        null_mut(),
        hinstance,
        null_mut(),
    );

    if hwnd == null_mut() {
        return Err(anyhow!("CreateWindowExW failed"));
    }
    // NOTE: a per-pixel-alpha window driven by UpdateLayeredWindow must NOT also call
    // SetLayeredWindowAttributes, and needs no window region — the alpha gives the shape.

    app.hwnd.store(hwnd as isize, Ordering::SeqCst);
    place_window(hwnd);
    render_bubble(hwnd);
    add_tray_icon(hwnd);

    let settings_title = wide_null("Nemotron Bubble Settings");
    let settings_hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
        class_name.as_ptr(),
        settings_title.as_ptr(),
        WS_POPUP,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        SETTINGS_WIDTH,
        SETTINGS_HEIGHT,
        null_mut(),
        null_mut(),
        hinstance,
        null_mut(),
    );
    if settings_hwnd != null_mut() {
        SetLayeredWindowAttributes(settings_hwnd, 0, 246, LWA_ALPHA);
        app.settings_hwnd
            .store(settings_hwnd as isize, Ordering::SeqCst);
        place_settings_window(settings_hwnd, SettingsOrigin::Tray); // re-placed whenever shown
        apply_settings_rounding(settings_hwnd);
        ShowWindow(settings_hwnd, SW_HIDE);
    }

    let snapshot = settings_snapshot(&app);
    if !register_global_hotkey(hwnd, snapshot.hotkey_mods, snapshot.hotkey_vk) {
        log_error(&format!(
            "Global shortcut (mods=0x{:x}, vk=0x{:x}) is already taken by another app.",
            snapshot.hotkey_mods, snapshot.hotkey_vk
        ));
        update_ui(&app, |ui| {
            ui.status = "Shortcut unavailable — open settings to choose another.".to_string();
        });
    }

    if settings_snapshot(&app).show_floating_bubble {
        ShowWindow(hwnd, SW_SHOWNA);
    } else {
        ShowWindow(hwnd, SW_HIDE);
    }

    // Drive waveform + tray animation from a single UI-thread timer.
    SetTimer(hwnd, ANIM_TIMER_ID, ANIM_INTERVAL_MS, None);

    let mut msg: MSG = std::mem::zeroed();
    while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    Ok(())
}

pub(crate) unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => 0,
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            if is_settings_window(hwnd) {
                paint_settings(hwnd);
            } else {
                paint(hwnd);
            }
            0
        }
        WM_HOTKEY => {
            match wparam as i32 {
                HOTKEY_ID => toggle_recording(),
                ENTER_HOTKEY_ID => {
                    if let Some(app) = APP.get() {
                        if app.recording.load(Ordering::SeqCst) {
                            // Stop, then the engine re-injects Enter once the full
                            // transcript has been typed (so the text submits).
                            app.enter_pending.store(true, Ordering::SeqCst);
                            stop_recording(app);
                        }
                    }
                }
                _ => {}
            }
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if is_settings_window(hwnd) {
                if let Some(app) = APP.get() {
                    if app.capturing_hotkey.load(Ordering::SeqCst) {
                        handle_capture_key(app, wparam as u32);
                        return 0;
                    }
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = lparam_point(lparam);
            if should_start_drag(hwnd, x, y) {
                begin_drag(hwnd);
            }
            0
        }
        WM_MOUSEMOVE => {
            continue_drag();
            0
        }
        WM_LBUTTONUP => {
            let (x, y) = lparam_point(lparam);
            let was_click = end_drag();
            if was_click {
                if is_settings_window(hwnd) {
                    handle_settings_click(x, y);
                } else {
                    handle_click(x, y);
                }
            }
            0
        }
        WM_TRAYICON => {
            match lparam as u32 {
                WM_LBUTTONUP => toggle_settings_window(SettingsOrigin::Tray),
                WM_RBUTTONUP => show_tray_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            handle_command(loword(wparam as usize) as usize);
            0
        }
        WM_TIMER => {
            if wparam == ANIM_TIMER_ID {
                on_anim_tick(hwnd);
            } else if wparam == COPY_FLASH_TIMER_ID {
                // the "copied" checkmark has been shown long enough
                KillTimer(hwnd, COPY_FLASH_TIMER_ID);
                if let Some(app) = APP.get() {
                    *app.copied_flash.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
                }
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_MOUSEWHEEL => {
            if let Some(app) = APP.get() {
                if is_settings_window(hwnd) && app.settings_tab.load(Ordering::SeqCst) == TAB_HISTORY {
                    // high word of wparam: signed wheel delta, 120 per notch
                    let delta = ((wparam >> 16) & 0xFFFF) as u16 as i16 as i32;
                    scroll_history(app, -delta * HISTORY_WHEEL_STEP / 120);
                }
            }
            0
        }
        WM_UI_UPDATE => {
            if is_settings_window(hwnd) {
                InvalidateRect(hwnd, null(), 0);
            } else {
                render_bubble(hwnd);
            }
            0
        }
        WM_DESTROY => {
            if let Some(app) = APP.get() {
                if hwnd == hwnd_from_app(app) {
                    let _ = app.engine_tx.send(EngineCommand::Shutdown);
                    KillTimer(hwnd, ANIM_TIMER_ID);
                    UnregisterHotKey(hwnd, HOTKEY_ID);
                    remove_tray_icon(hwnd);
                    PostQuitMessage(0);
                }
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub(crate) fn clear_transcript() {
    if let Some(app) = APP.get() {
        update_ui(app, |ui| {
            ui.transcript.clear();
            ui.status = "Cleared. Press Ctrl+Space to start.".to_string();
        });
    }
}

pub(crate) fn handle_command(command: usize) {
    match command {
        IDM_START_STOP => toggle_recording(),
        IDM_SHOW_HIDE => toggle_window_visibility(),
        IDM_CLEAR => clear_transcript(),
        id if (IDM_MODEL_BASE..IDM_MODEL_BASE + ModelChoice::ALL.len()).contains(&id) => {
            if let Some(app) = APP.get() {
                select_model(app, ModelChoice::ALL[id - IDM_MODEL_BASE]);
            }
        }
        id if (IDM_LANG_BASE..IDM_LANG_BASE + LANGUAGES.len()).contains(&id) => {
            if let Some(app) = APP.get() {
                select_language(app, LANGUAGES[id - IDM_LANG_BASE].0);
            }
        }
        id if (IDM_BUBBLE_POS_BASE..IDM_BUBBLE_POS_BASE + BubblePosition::ALL.len()).contains(&id) => {
            if let Some(app) = APP.get() {
                select_bubble_position(app, BubblePosition::ALL[id - IDM_BUBBLE_POS_BASE]);
            }
        }
        id if (IDM_COMPUTE_BASE..IDM_COMPUTE_BASE + ComputeChoice::ALL.len()).contains(&id) => {
            if let Some(app) = APP.get() {
                select_compute(app, ComputeChoice::ALL[id - IDM_COMPUTE_BASE]);
            }
        }
        IDM_QUIT => unsafe {
            let Some(app) = APP.get() else {
                return;
            };
            let hwnd = hwnd_from_app(app);
            if hwnd != null_mut() {
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
            }
        },
        _ => {}
    }
}

