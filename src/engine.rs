//! The speech engine thread and recording control (start, stop, chunk timing).

use crate::*;

pub(crate) const NEMOTRON_SAMPLE_RATE: f64 = 16_000.0;
pub(crate) const NEMOTRON_CHUNK_SIZE: usize = 8_960; // 560 ms at 16 kHz.

#[derive(Clone, Debug)]
pub(crate) enum EngineCommand {
    Preload,
    /// Drop the loaded model (frees its RAM), e.g. after switching models.
    Unload,
    /// Set the spoken language of the multilingual model (a LANGUAGES code).
    SetLanguage(String),
    Start,
    Audio(Vec<f32>),
    Stop,
    Shutdown,
}

pub(crate) fn run_engine(rx: Receiver<EngineCommand>, app: Arc<AppState>) {
    let mut model: Option<Nemotron> = None;
    let mut model_path = PathBuf::new();
    let mut on_gpu = false;
    // Inference time of the current dictation, logged on stop (GPU vs CPU check).
    let mut chunk_time = Duration::ZERO;
    let mut chunk_count = 0u32;
    let mut active = false;
    let mut audio_buffer = Vec::<f32>::with_capacity(NEMOTRON_CHUNK_SIZE * 2);
    // Set after a failed background preload so that unrelated settings toggles
    // (each of which re-sends Preload) don't pop the same error dialog again.
    // Cleared by a model switch (Unload); an explicit Start always retries.
    let mut preload_failed = false;

    loop {
        match rx.recv() {
            Ok(EngineCommand::Preload) => {
                if model.is_some() || preload_failed {
                    continue;
                }
                update_ui(&app, |ui| {
                    ui.status = "Loading Nemotron...".to_string();
                    ui.model_status = "Loading...".to_string();
                });
                match load_nemotron(&settings_snapshot(&app)) {
                    Ok(loaded) => {
                        let label = model_status_label(
                            &loaded.model,
                            &settings_snapshot(&app).language,
                            loaded.on_gpu,
                        );
                        model = Some(loaded.model);
                        model_path = loaded.dir;
                        on_gpu = loaded.on_gpu;
                        update_ui(&app, |ui| {
                            ui.status = "Ready. Nemotron loaded. Ctrl+Space toggles.".to_string();
                            ui.model_dir = model_path.display().to_string();
                            ui.model_status = label;
                            if ui.transcript.trim().is_empty() {
                                ui.transcript = "Tap Ctrl+Space to dictate.".to_string();
                            }
                        });
                    }
                    Err(err) => {
                        preload_failed = true;
                        report_model_error(&err);
                        update_ui(&app, |ui| {
                            ui.status = "Nemotron model missing or failed to load.".to_string();
                            ui.transcript = format!("{err:#}");
                            ui.model_dir = expected_model_hint();
                            ui.model_status = "Failed to load - see log.".to_string();
                        });
                    }
                }
            }
            Ok(EngineCommand::Unload) => {
                active = false;
                audio_buffer.clear();
                preload_failed = false;
                if model.take().is_some() {
                    log_info(&format!("Unloaded Nemotron model from {}", model_path.display()));
                }
                update_ui(&app, |ui| {
                    ui.model_status = "Not loaded yet.".to_string();
                });
            }
            Ok(EngineCommand::SetLanguage(language)) => {
                // Nothing loaded (or English-only): the saved setting is applied
                // when the multilingual model loads.
                if let Some(loaded) = model.as_mut() {
                    if loaded.mode() == NemotronMode::Multilingual {
                        match apply_language(loaded, &language) {
                            Ok(()) => {
                                let label = model_status_label(loaded, &language, on_gpu);
                                update_ui(&app, |ui| ui.model_status = label);
                            }
                            Err(err) => log_error(&format!("{err:#}")),
                        }
                    }
                }
            }
            Ok(EngineCommand::Start) => {
                audio_buffer.clear();
                chunk_time = Duration::ZERO;
                chunk_count = 0;
                if model.is_none() {
                    update_ui(&app, |ui| {
                        ui.status = "Loading Nemotron model...".to_string();
                        ui.model_status = "Loading...".to_string();
                    });
                    match load_nemotron(&settings_snapshot(&app)) {
                        Ok(loaded) => {
                            preload_failed = false;
                            let label = model_status_label(
                                &loaded.model,
                                &settings_snapshot(&app).language,
                                loaded.on_gpu,
                            );
                            update_ui(&app, |ui| ui.model_status = label);
                            model = Some(loaded.model);
                            model_path = loaded.dir;
                            on_gpu = loaded.on_gpu;
                        }
                        Err(err) => {
                            report_model_error(&err);
                            app.recording.store(false, Ordering::SeqCst);
                            update_ui(&app, |ui| {
                                ui.model_status = "Failed to load - see log.".to_string();
                                ui.recording = false;
                                ui.status = "Nemotron model missing or failed to load.".to_string();
                                ui.transcript = format!("{err:#}");
                                ui.model_dir = expected_model_hint();
                            });
                            continue;
                        }
                    }
                }

                if let Some(model) = model.as_mut() {
                    model.reset();
                    active = true;
                    update_ui(&app, |ui| {
                        ui.recording = true;
                        ui.status = "Listening...".to_string();
                        ui.transcript.clear();
                        ui.model_dir = model_path.display().to_string();
                    });
                    play_start_sound(&app);
                }
            }
            Ok(EngineCommand::Audio(samples)) => {
                if !active {
                    continue;
                }
                let Some(model) = model.as_mut() else {
                    continue;
                };

                audio_buffer.extend(samples);
                while audio_buffer.len() >= NEMOTRON_CHUNK_SIZE {
                    let chunk: Vec<f32> = audio_buffer.drain(..NEMOTRON_CHUNK_SIZE).collect();
                    let chunk_started = Instant::now();
                    let result = model.transcribe_chunk(&chunk);
                    chunk_time += chunk_started.elapsed();
                    chunk_count += 1;
                    match result {
                        Ok(text) if !text.is_empty() => {
                            let settings = settings_snapshot(&app);
                            let live_type = settings.live_type_into_cursor;
                            // Type the new words directly via Unicode keystrokes —
                            // the clipboard stays untouched until the final stop.
                            let type_result = if live_type {
                                type_unicode_text(
                                    &text,
                                    hwnd_from_app(&app),
                                    target_hwnd_from_app(&app),
                                    settings.paste_delay_ms,
                                )
                            } else {
                                Ok(())
                            };
                            update_ui(&app, |ui| {
                                ui.status = if let Err(err) = type_result {
                                    format!("Live typing failed: {err}")
                                } else if live_type {
                                    "Listening... typing live.".to_string()
                                } else {
                                    "Listening...".to_string()
                                };
                                ui.transcript.push_str(&text);
                            });
                        }
                        Ok(_) => {}
                        Err(err) => {
                            log_error(&format!("ASR error while transcribing: {err:#}"));
                            active = false;
                            app.recording.store(false, Ordering::SeqCst);
                            update_ui(&app, |ui| {
                                ui.recording = false;
                                ui.status = "ASR error.".to_string();
                                ui.transcript = format!("{err:#}");
                            });
                        }
                    }
                }
            }
            Ok(EngineCommand::Stop) => {
                app.recording.store(false, Ordering::SeqCst);
                // Enter-to-stop wants a near-instant finish, so skip the extra
                // silence-flush passes (they only squeeze out a touch more tail).
                let fast = app.enter_pending.load(Ordering::SeqCst);
                if active {
                    active = false;
                    if let Some(model) = model.as_mut() {
                        if !audio_buffer.is_empty() {
                            audio_buffer.resize(NEMOTRON_CHUNK_SIZE, 0.0);
                            if let Ok(text) = model.transcribe_chunk(&audio_buffer) {
                                paste_live_tail(&app, &text);
                            }
                            audio_buffer.clear();
                        }
                        let flush_passes = if fast { 0 } else { 3 };
                        for _ in 0..flush_passes {
                            if let Ok(text) = model.transcribe_chunk(&vec![0.0; NEMOTRON_CHUNK_SIZE])
                            {
                                paste_live_tail(&app, &text);
                            }
                        }

                        let stats = (chunk_count > 0).then(|| DictationStats {
                            on_gpu,
                            ms_per_chunk: chunk_time.as_secs_f32() * 1000.0 / chunk_count as f32,
                        });
                        if let Some(stats) = &stats {
                            log_info(&format!(
                                "Dictation: {chunk_count} chunks, {:.0} ms per 560 ms chunk on the {}",
                                stats.ms_per_chunk,
                                if stats.on_gpu { "GPU" } else { "CPU" }
                            ));
                        }
                        chunk_time = Duration::ZERO;
                        chunk_count = 0;

                        let final_text = model.get_transcript();
                        let has_final = !final_text.trim().is_empty();
                        let settings = settings_snapshot(&app);
                        let mut final_status = if has_final {
                            "Stopped. Text was typed live.".to_string()
                        } else {
                            "Ready. Press Ctrl+Space to start.".to_string()
                        };

                        if has_final {
                            add_history(&app, final_text.clone(), stats);
                            // Paste-on-stop already writes the clipboard via Ctrl+V, so
                            // only do a standalone copy when not pasting — never both.
                            if settings.paste_final_on_stop {
                                final_status = match paste_text_to_target(
                                    &final_text,
                                    hwnd_from_app(&app),
                                    target_hwnd_from_app(&app),
                                    settings.paste_delay_ms,
                                ) {
                                    Ok(()) => "Stopped. Final transcript pasted.".to_string(),
                                    Err(err) => {
                                        log_error(&format!("Final paste failed: {err:#}"));
                                        format!("Final paste failed: {err}")
                                    }
                                };
                            } else if settings.copy_final_to_clipboard {
                                final_status =
                                    match set_clipboard_text(&final_text, hwnd_from_app(&app)) {
                                        Ok(()) => {
                                            "Stopped. Final transcript copied to clipboard."
                                                .to_string()
                                        }
                                        Err(err) => {
                                            log_error(&format!("Clipboard failed: {err:#}"));
                                            format!("Clipboard failed: {err}")
                                        }
                                    };
                            }
                        }

                        update_ui(&app, |ui| {
                            ui.recording = false;
                            ui.status = final_status;
                            ui.transcript = if !has_final {
                                "No speech detected.".to_string()
                            } else {
                                final_text
                            };
                        });
                    }
                } else {
                    update_ui(&app, |ui| {
                        ui.recording = false;
                        ui.status = "Ready. Press Ctrl+Space to start.".to_string();
                    });
                }
                // If this stop was triggered by Enter, submit now that the full
                // transcript has been typed.
                if app.enter_pending.swap(false, Ordering::SeqCst) {
                    let _ = unsafe { send_enter() };
                }
            }
            Ok(EngineCommand::Shutdown) | Err(_) => break,
        }
    }
}

pub(crate) fn toggle_recording() {
    let Some(app) = APP.get() else {
        return;
    };

    if app.recording.load(Ordering::SeqCst) {
        stop_recording(app);
    } else {
        start_recording(app);
    }
}

pub(crate) fn start_recording(app: &Arc<AppState>) {
    if !ensure_audio_capture(app) {
        update_ui(app, |ui| {
            ui.recording = false;
            ui.status = "No microphone available. Connect one and try again.".to_string();
        });
        return;
    }
    remember_target_window(app);
    app.recording.store(true, Ordering::SeqCst);
    app.enter_pending.store(false, Ordering::SeqCst);
    let _ = app.engine_tx.send(EngineCommand::Start);
    update_ui(app, |ui| {
        ui.recording = true;
        ui.visible = true;
        ui.status = "Starting...".to_string();
        ui.transcript.clear();
        ui.last_voice = Instant::now(); // reset the silence timer
    });
    unsafe {
        let hwnd = hwnd_from_app(app);
        // While recording, optionally let Enter end the session.
        if settings_snapshot(app).enter_stops_recording {
            register_enter_hotkey(hwnd);
        }
        if hwnd != null_mut() && settings_snapshot(app).show_floating_bubble {
            ShowWindow(hwnd, SW_SHOWNA);
        }
    }
}

pub(crate) fn remember_target_window(app: &Arc<AppState>) {
    unsafe {
        let foreground = GetForegroundWindow();
        let own = hwnd_from_app(app);
        if foreground != null_mut() && foreground != own {
            app.target_hwnd
                .store(foreground as isize, Ordering::SeqCst);
        }
    }
}

pub(crate) fn stop_recording(app: &Arc<AppState>) {
    app.recording.store(false, Ordering::SeqCst);
    unsafe {
        // Release the temporary Enter hotkey before the engine re-injects Enter.
        unregister_enter_hotkey(hwnd_from_app(app));
    }
    play_stop_sound(app);
    let _ = app.engine_tx.send(EngineCommand::Stop);
    update_ui(app, |ui| {
        ui.recording = false;
        ui.status = "Finalizing...".to_string();
    });
}

