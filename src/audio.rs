//! Microphone capture and resampling to 16 kHz.

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
}

/// Make sure a working microphone stream is open. Returns false (after logging
/// and notifying the user) when no usable input device is available. Must run on
/// the main/UI thread, which owns the stream.
pub(crate) fn ensure_audio_capture(app: &Arc<AppState>) -> bool {
    let healthy = AUDIO_STREAM.with(|slot| slot.borrow().is_some()) && !MIC_FAILED.load(Ordering::SeqCst);
    if healthy {
        return true;
    }
    // Drop a stream whose device has gone away before opening a fresh one.
    AUDIO_STREAM.with(|slot| slot.borrow_mut().take());
    MIC_FAILED.store(false, Ordering::SeqCst);

    match start_audio_capture(app.clone()) {
        Ok(stream) => {
            AUDIO_STREAM.with(|slot| *slot.borrow_mut() = Some(stream));
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

// ---------------------------------------------------------------------------
// Diagnostics: log file + DebugView + user-facing message boxes.
// ---------------------------------------------------------------------------

pub(crate) fn start_audio_capture(app: Arc<AppState>) -> Result<cpal::Stream> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("no default microphone found"))?;
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
    Ok(stream)
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

