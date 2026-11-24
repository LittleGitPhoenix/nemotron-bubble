//! Microphone selection, capture and resampling to 16 kHz.

use crate::*;

// Auto-stop recording after this many seconds without detected speech.
pub(crate) const SILENCE_AUTO_STOP_SECS: u64 = 15;
// Raw mic RMS above which we treat the user as speaking (for the silence auto-stop).
// Compared against true loudness, NOT the visually-boosted bar height.
pub(crate) const VOICE_RMS_THRESHOLD: f32 = 0.02;
// Set by the cpal error callback (e.g. the mic was unplugged or disabled) so the
// next dictation start reopens the device instead of recording silence.
pub(crate) static MIC_FAILED: AtomicBool = AtomicBool::new(false);

thread_local! {
    // The live microphone stream. cpal streams are not Send, so it lives on the
    // UI thread (which is also the main thread) and is (re)opened lazily there.
    pub(crate) static AUDIO_STREAM: RefCell<Option<cpal::Stream>> = const { RefCell::new(None) };
    // Windows name of the device AUDIO_STREAM records from.
    pub(crate) static OPEN_MICROPHONE: RefCell<Option<String>> = const { RefCell::new(None) };
    // The device names offered by the last microphone popup menu, in menu order.
    pub(crate) static MICROPHONE_MENU: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Names of all active input devices, as Windows shows them.
pub(crate) fn input_device_names() -> Vec<String> {
    match cpal::default_host().input_devices() {
        Ok(devices) => devices.filter_map(|device| device.name().ok()).collect(),
        Err(err) => {
            log_error(&format!("Could not list microphones: {err}"));
            Vec::new()
        }
    }
}

/// Name of the Windows default input device, if there is one.
pub(crate) fn default_input_device_name() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .and_then(|device| device.name().ok())
}

/// Name of the device the open stream records from (None when no stream is open).
pub(crate) fn open_microphone_name() -> Option<String> {
    OPEN_MICROPHONE.with(|name| name.borrow().clone())
}

/// True when the open stream no longer records from the device the settings ask
/// for: the Windows default changed (with "Windows default" selected), or the
/// selected device is connected again after a fallback to the default.
fn microphone_choice_changed(app: &Arc<AppState>) -> bool {
    let wanted = settings_snapshot(app).microphone;
    let open = open_microphone_name().unwrap_or_default();
    if wanted.is_empty() {
        default_input_device_name().is_some_and(|name| name != open)
    } else if wanted == open {
        false
    } else {
        input_device_names().iter().any(|name| *name == wanted)
    }
}

/// Close the microphone stream. The next ensure_audio_capture opens it again.
pub(crate) fn close_audio_capture() {
    AUDIO_STREAM.with(|slot| slot.borrow_mut().take());
    OPEN_MICROPHONE.with(|name| name.borrow_mut().take());
}

/// Make sure a working microphone stream is open, on the device the settings
/// ask for. Returns false (after logging and notifying the user) when no usable
/// input device is available. Must run on the main/UI thread, which owns the stream.
pub(crate) fn ensure_audio_capture(app: &Arc<AppState>) -> bool {
    let open = AUDIO_STREAM.with(|slot| slot.borrow().is_some());
    if open && !MIC_FAILED.load(Ordering::SeqCst) {
        if !microphone_choice_changed(app) {
            return true;
        }
        log_info("The microphone to use has changed. Reopening the input stream.");
    }
    // Drop a stream whose device has gone away before opening a fresh one.
    close_audio_capture();
    MIC_FAILED.store(false, Ordering::SeqCst);

    match start_audio_capture(app.clone()) {
        Ok((stream, name)) => {
            AUDIO_STREAM.with(|slot| *slot.borrow_mut() = Some(stream));
            OPEN_MICROPHONE.with(|slot| *slot.borrow_mut() = Some(name));
            true
        }
        Err(err) => {
            let message = format!("{err:#}");
            log_error(&format!("Microphone unavailable: {message}"));
            notify_user(
                "Nemotron Bubble: no microphone",
                &format!(
                    "No usable microphone was found ({message}).\n\n\
                     Connect or enable a microphone (Windows Settings > System > Sound > Input), \
                     then press your shortcut again. The app keeps running in the tray.\n\n\
                     Log file: {}",
                    log_path().display()
                ),
                MB_ICONWARNING,
            );
            false
        }
    }
}

/// Use the input device `name` (empty: the Windows default), save the choice
/// and reopen the stream right away. Refused while dictating.
pub(crate) fn select_microphone(app: &Arc<AppState>, name: String) {
    if app.recording.load(Ordering::SeqCst) {
        update_ui(app, |ui| {
            ui.status = "Stop dictation before switching the microphone.".to_string();
        });
        return;
    }
    let settings = {
        let mut settings = app
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        settings.microphone = name;
        settings.clone()
    };
    save_settings(&settings);
    log_info(&format!("Microphone changed to {}", microphone_label(&settings.microphone)));

    close_audio_capture();
    if ensure_audio_capture(app) {
        let in_use = open_microphone_name().unwrap_or_default();
        update_ui(app, |ui| {
            ui.status = format!("Microphone: {in_use}.");
        });
    }
}

/// Entry `index` of the microphone popup menu: 0 is "Windows default", the
/// others are the devices listed when the menu was shown.
pub(crate) fn select_microphone_menu_item(app: &Arc<AppState>, index: usize) {
    let name = if index == 0 {
        Some(String::new())
    } else {
        MICROPHONE_MENU.with(|names| names.borrow().get(index - 1).cloned())
    };
    if let Some(name) = name {
        select_microphone(app, name);
    }
}

/// Display text of a microphone setting.
pub(crate) fn microphone_label(name: &str) -> &str {
    if name.is_empty() { "Windows default" } else { name }
}

/// The input device the settings ask for, or the Windows default when that one
/// is not connected. The setting is kept, so the device is used again once it is back.
fn pick_input_device(app: &Arc<AppState>, host: &cpal::Host) -> Result<cpal::Device> {
    let wanted = settings_snapshot(app).microphone;
    if !wanted.is_empty() {
        let found = host
            .input_devices()
            .ok()
            .and_then(|mut devices| devices.find(|device| device.name().is_ok_and(|name| name == wanted)));
        if let Some(device) = found {
            return Ok(device);
        }
        log_error(&format!("Microphone '{wanted}' is not connected. Using the Windows default."));
    }
    host.default_input_device()
        .ok_or_else(|| anyhow!("no default microphone found"))
}

/// Open and start the input stream. Returns it with the name of its device.
pub(crate) fn start_audio_capture(app: Arc<AppState>) -> Result<(cpal::Stream, String)> {
    let host = cpal::default_host();
    let device = pick_input_device(&app, &host)?;
    let device_name = device.name().unwrap_or_else(|_| "<unknown>".to_string());
    let input_config = device
        .default_input_config()
        .with_context(|| format!("microphone '{device_name}' has no usable input format"))?;
    let sample_rate = input_config.sample_rate().0 as f64;
    let channels = input_config.channels() as usize;
    let config = input_config.config();
    log_info(&format!(
        "Microphone: '{device_name}', {} Hz, {channels} channel(s), {:?}",
        sample_rate as u32,
        input_config.sample_format()
    ));

    update_ui(&app, |ui| {
        ui.status = format!(
            "Ready. Mic: {} Hz, {} channel(s). Ctrl+Space toggles.",
            sample_rate as u32, channels
        );
    });

    let tx = app.engine_tx.clone();
    let recording = app.recording.clone();
    let err_app = app.clone();
    let err_fn = move |err: cpal::StreamError| {
        log_error(&format!("Microphone stream error: {err}"));
        MIC_FAILED.store(true, Ordering::SeqCst);
        update_ui(&err_app, |ui| {
            ui.status = "Microphone error.".to_string();
            ui.transcript = err.to_string();
        });
    };

    let stream = match input_config.sample_format() {
        cpal::SampleFormat::F32 => {
            let mut callback = InputCallback::new(channels, sample_rate, recording, tx, app.clone());
            device.build_input_stream(
                &config,
                move |data: &[f32], _| callback.push_f32(data),
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let mut callback = InputCallback::new(channels, sample_rate, recording, tx, app.clone());
            device.build_input_stream(
                &config,
                move |data: &[i16], _| callback.push_i16(data),
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::U16 => {
            let mut callback = InputCallback::new(channels, sample_rate, recording, tx, app.clone());
            device.build_input_stream(
                &config,
                move |data: &[u16], _| callback.push_u16(data),
                err_fn,
                None,
            )?
        }
        other => return Err(anyhow!("unsupported microphone sample format: {other:?}")),
    };

    stream
        .play()
        .with_context(|| format!("could not start recording from '{device_name}'"))?;
    Ok((stream, device_name))
}

pub(crate) struct InputCallback {
    pub(crate) channels: usize,
    pub(crate) tx: Sender<EngineCommand>,
    pub(crate) recording: Arc<AtomicBool>,
    pub(crate) app: Arc<AppState>,
    pub(crate) was_recording: bool,
    pub(crate) resampler: LinearResampler,
}

impl InputCallback {
    pub(crate) fn new(
        channels: usize,
        source_rate: f64,
        recording: Arc<AtomicBool>,
        tx: Sender<EngineCommand>,
        app: Arc<AppState>,
    ) -> Self {
        Self {
            channels,
            tx,
            recording,
            app,
            was_recording: false,
            resampler: LinearResampler::new(source_rate, NEMOTRON_SAMPLE_RATE),
        }
    }

    pub(crate) fn push_f32(&mut self, data: &[f32]) {
        self.process(data.iter().copied());
    }

    pub(crate) fn push_i16(&mut self, data: &[i16]) {
        self.process(data.iter().map(|s| *s as f32 / 32768.0));
    }

    pub(crate) fn push_u16(&mut self, data: &[u16]) {
        self.process(data.iter().map(|s| (*s as f32 / 65535.0) * 2.0 - 1.0));
    }

    pub(crate) fn process<I>(&mut self, samples: I)
    where
        I: Iterator<Item = f32>,
    {
        let now_recording = self.recording.load(Ordering::Relaxed);
        if !now_recording {
            self.was_recording = false;
            return;
        }

        if !self.was_recording {
            self.resampler.reset();
            self.was_recording = true;
        }

        let mut mono = Vec::new();
        let mut frame = Vec::with_capacity(self.channels);
        for sample in samples {
            frame.push(sample);
            if frame.len() == self.channels {
                let sum = frame.iter().sum::<f32>();
                mono.push(sum / self.channels as f32);
                frame.clear();
            }
        }

        if !mono.is_empty() {
            // Loudness -> bar height. NOISE_FLOOR gates hiss; GAIN sets sensitivity so
            // ordinary speech reaches most of the height; the sub-1.0 exponent lifts
            // quiet syllables so they read as tall bumps instead of tiny specks.
            const NOISE_FLOOR: f32 = 0.006;
            const GAIN: f32 = 14.0;
            const SHAPE: f32 = 0.6;
            let rms = (mono.iter().map(|v| v * v).sum::<f32>() / mono.len() as f32).sqrt();
            let norm = ((rms - NOISE_FLOOR) * GAIN).clamp(0.0, 1.0);
            let level = norm.powf(SHAPE);
            report_level(&self.app, level, rms);
        }

        let mut out = Vec::new();
        self.resampler.push(&mono, &mut out);
        if !out.is_empty() {
            let _ = self.tx.try_send(EngineCommand::Audio(out));
        }
    }
}

pub(crate) struct LinearResampler {
    pub(crate) source_rate: f64,
    pub(crate) target_rate: f64,
    pub(crate) position: f64,
    pub(crate) last_sample: Option<f32>,
}

impl LinearResampler {
    pub(crate) fn new(source_rate: f64, target_rate: f64) -> Self {
        Self {
            source_rate,
            target_rate,
            position: 0.0,
            last_sample: None,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.position = 0.0;
        self.last_sample = None;
    }

    pub(crate) fn push(&mut self, input: &[f32], output: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }

        let mut data = Vec::with_capacity(input.len() + usize::from(self.last_sample.is_some()));
        if let Some(last) = self.last_sample {
            data.push(last);
        }
        data.extend_from_slice(input);

        let step = self.source_rate / self.target_rate;
        while self.position + 1.0 < data.len() as f64 {
            let index = self.position.floor() as usize;
            let frac = (self.position - index as f64) as f32;
            let a = data[index];
            let b = data[index + 1];
            output.push(a + (b - a) * frac);
            self.position += step;
        }

        self.last_sample = data.last().copied();
        self.position -= (data.len() - 1) as f64;
    }
}

