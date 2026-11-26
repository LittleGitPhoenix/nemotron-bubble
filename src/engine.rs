//! The speech engine thread and recording control (start, stop, chunk timing).

use crate::*;

pub(crate) const NEMOTRON_SAMPLE_RATE: f64 = 16_000.0;

/// Length of the model's streaming chunk in milliseconds, e.g. 560 for the
/// default export. The chunk size itself comes from the model (`chunk_samples`),
/// because it depends on the latency profile the model was exported with.
pub(crate) fn chunk_duration_ms(model: &Nemotron) -> f64 {
    model.chunk_samples() as f64 * 1000.0 / NEMOTRON_SAMPLE_RATE
}

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

/// The audio side of one dictation, free of UI and output: collects 16 kHz
/// samples into the model's chunks, transcribes them, and on stop transcribes the
/// buffered rest and a few chunks of silence. Tests feed recorded audio through it.
pub(crate) struct Dictation {
    buffer: Vec<f32>,
    // Inference time of the current dictation, logged on stop (GPU vs CPU check).
    chunk_time: Duration,
    chunk_count: u32,
}

/// The outcome of a finished dictation.
pub(crate) struct FinishedDictation {
    pub(crate) transcript: String,
    /// Chunks transcribed while recording. The rest and the silence flush on stop are not counted.
    pub(crate) chunk_count: u32,
    /// Average inference time per chunk. None when no full chunk was transcribed.
    pub(crate) ms_per_chunk: Option<f32>,
}

impl Dictation {
    pub(crate) fn new() -> Self {
        Self {
            buffer: Vec::new(),
            chunk_time: Duration::ZERO,
            chunk_count: 0,
        }
    }

    /// Starts a new dictation and resets the model's streaming state.
    pub(crate) fn start(&mut self, model: &mut Nemotron) {
        self.buffer.clear();
        self.chunk_time = Duration::ZERO;
        self.chunk_count = 0;
        model.reset();
    }

    /// Adds 16 kHz samples and transcribes every complete chunk. `on_text`
    /// receives the new text of each chunk that produced some. Stops at the
    /// first error.
    pub(crate) fn push_audio(&mut self, model: &mut Nemotron, samples: &[f32], mut on_text: impl FnMut(&str)) -> Result<()> {
        let chunk_size = model.chunk_samples();
        self.buffer.extend_from_slice(samples);
        while self.buffer.len() >= chunk_size {
            let chunk: Vec<f32> = self.buffer.drain(..chunk_size).collect();
            let started = Instant::now();
            let result = model.transcribe_chunk(&chunk);
            self.chunk_time += started.elapsed();
            self.chunk_count += 1;
            let text = result.map_err(|err| anyhow!("{err:#}"))?;
            if !text.is_empty() {
                on_text(&text);
            }
        }
        Ok(())
    }

    /// Transcribes the buffered rest (padded with silence to a full chunk) and
    /// `flush_passes` chunks of silence, which let the model emit its last words.
    /// `on_text` receives the text these passes add. Their errors are ignored,
    /// because the text recognized so far is still worth keeping.
    pub(crate) fn finish(&mut self, model: &mut Nemotron, flush_passes: usize, mut on_text: impl FnMut(&str)) -> FinishedDictation {
        let chunk_size = model.chunk_samples();
        let mut transcribe = |chunk: &[f32]| {
            if let Ok(text) = model.transcribe_chunk(chunk) {
                if !text.is_empty() {
                    on_text(&text);
                }
            }
        };
        if !self.buffer.is_empty() {
            self.buffer.resize(chunk_size, 0.0);
            transcribe(&self.buffer);
            self.buffer.clear();
        }
        let silence = vec![0.0; chunk_size];
        for _ in 0..flush_passes {
            transcribe(&silence);
        }

        let finished = FinishedDictation {
            transcript: model.get_transcript(),
            chunk_count: self.chunk_count,
            ms_per_chunk: (self.chunk_count > 0)
                .then(|| self.chunk_time.as_secs_f32() * 1000.0 / self.chunk_count as f32),
        };
        self.chunk_time = Duration::ZERO;
        self.chunk_count = 0;
        finished
    }
}

pub(crate) fn run_engine(rx: Receiver<EngineCommand>, app: Arc<AppState>) {
    let mut model: Option<Nemotron> = None;
    let mut model_path = PathBuf::new();
    let mut on_gpu = false;
    let mut active = false;
    let mut dictation = Dictation::new();
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
                    dictation.start(model);
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

                let result = dictation.push_audio(model, &samples, |text| {
                    let settings = settings_snapshot(&app);
                    let live_type = settings.live_type_into_cursor;
                    // Type the new words directly via Unicode keystrokes —
                    // the clipboard stays untouched until the final stop.
                    let type_result = if live_type {
                        type_unicode_text(
                            text,
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
                        ui.transcript.push_str(text);
                    });
                });
                if let Err(err) = result {
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
            Ok(EngineCommand::Stop) => {
                app.recording.store(false, Ordering::SeqCst);
                // Enter-to-stop wants a near-instant finish, so skip the extra
                // silence-flush passes (they only squeeze out a touch more tail).
                let fast = app.enter_pending.load(Ordering::SeqCst);
                if active {
                    active = false;
                    if let Some(model) = model.as_mut() {
                        let flush_passes = if fast { 0 } else { 3 };
                        let finished = dictation.finish(model, flush_passes, |text| paste_live_tail(&app, text));

                        let stats = finished
                            .ms_per_chunk
                            .map(|ms_per_chunk| DictationStats { on_gpu, ms_per_chunk });
                        if let Some(stats) = &stats {
                            log_info(&format!(
                                "Dictation: {} chunks, {:.0} ms per {:.0} ms chunk on the {}",
                                finished.chunk_count,
                                chunk_duration_ms(model),
                                stats.ms_per_chunk,
                                if stats.on_gpu { "GPU" } else { "CPU" }
                            ));
                        }

                        let final_text = finished.transcript;
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
    if !ensure_audio_capture(app, true) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// "Hello world, what a wonderful day." (16-bit PCM, mono, 24 kHz).
    const SAMPLE: &[u8] = include_bytes!("../tests/data/audio-test.wav");
    const SAMPLE_WORDS: [&str; 4] = ["hello", "world", "wonderful", "day"];

    /// Each model needs about 2.5 GB of RAM, so the model tests run one after another.
    static MODEL_LOCK: Mutex<()> = Mutex::new(());

    /// Loads a model on the CPU from the folder that NEMOTRON_TEST_MODELS points to.
    /// The app never reads that variable, and build-wsl.ps1 -Test -WithModels sets
    /// it for the test process only.
    fn load_test_model(choice: ModelChoice) -> Nemotron {
        let models = env::var_os("NEMOTRON_TEST_MODELS").map(PathBuf::from).expect(
            "NEMOTRON_TEST_MODELS must point to a models folder with nemotron and nemotron_multi \
             (build-wsl.ps1 -Test -WithModels sets it)",
        );
        let dir = models.join(choice.folder_name().expect("a standard model"));
        assert!(has_model_files(&dir), "no model files in {}", dir.display());
        load_with_device(&dir, false).unwrap_or_else(|err| panic!("{err:#}"))
    }

    /// Feeds the sample through the app's path from the capture callback on:
    /// downmix, resampling to 16 kHz in 20 ms blocks and the dictation itself.
    /// Returns the final transcript and the text that would have been typed live.
    fn dictate_sample(model: &mut Nemotron) -> (String, String) {
        let wav = read_wav(SAMPLE).expect("the sample is a valid WAV file");
        let mut resampler = LinearResampler::new(wav.sample_rate as f64, NEMOTRON_SAMPLE_RATE);
        let mut dictation = Dictation::new();
        let mut live = String::new();

        dictation.start(model);
        let block = wav.channels * wav.sample_rate as usize / 50;
        for samples in wav.samples.chunks(block) {
            let mono = downmix_to_mono(samples.iter().copied(), wav.channels);
            let mut resampled = Vec::new();
            resampler.push(&mono, &mut resampled);
            dictation
                .push_audio(model, &resampled, |text| live.push_str(text))
                .unwrap_or_else(|err| panic!("{err:#}"));
        }
        let finished = dictation.finish(model, 3, |text| live.push_str(text));
        assert!(finished.chunk_count > 0, "the sample is longer than one chunk");
        assert!(finished.ms_per_chunk.is_some());
        (finished.transcript, live)
    }

    fn assert_recognized(transcript: &str, live: &str) {
        let lowercase = transcript.to_lowercase();
        let words: Vec<&str> = lowercase
            .split(|character: char| !character.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect();
        for expected in SAMPLE_WORDS {
            assert!(words.contains(&expected), "\"{expected}\" is missing in \"{transcript}\"");
        }
        assert_eq!(live.trim(), transcript.trim(), "live text and final transcript differ");
    }

    #[test]
    #[ignore = "needs the speech models, run with build-wsl.ps1 -Test -WithModels"]
    fn english_model_recognizes_the_sample() {
        let _lock = MODEL_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut model = load_test_model(ModelChoice::English);
        assert_eq!(model.mode(), NemotronMode::EnglishOnly);
        // The README and CHANGELOG describe the downloadable models with 560 ms chunks.
        assert_eq!(chunk_duration_ms(&model), 560.0);

        let (transcript, live) = dictate_sample(&mut model);
        println!("English: {transcript}");
        assert_recognized(&transcript, &live);

        // A second dictation starts from a clean state.
        let (again, _) = dictate_sample(&mut model);
        assert_eq!(again, transcript);
    }

    #[test]
    #[ignore = "needs the speech models, run with build-wsl.ps1 -Test -WithModels"]
    fn multilingual_model_recognizes_the_sample() {
        let _lock = MODEL_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut model = load_test_model(ModelChoice::Multilingual);
        assert_eq!(model.mode(), NemotronMode::Multilingual);
        assert_eq!(chunk_duration_ms(&model), 560.0);

        for language in ["auto", "en"] {
            apply_language(&mut model, language).unwrap_or_else(|err| panic!("{err:#}"));
            let (transcript, live) = dictate_sample(&mut model);
            println!("Multilingual ({language}): {transcript}");
            assert_recognized(&transcript, &live);
        }
    }
}

