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
    // The live microphone stream. It lives on the UI thread (which is also the
    // main thread) and is (re)opened lazily there.
    pub(crate) static AUDIO_STREAM: RefCell<Option<cpal::Stream>> = const { RefCell::new(None) };
    // Windows name of the device AUDIO_STREAM records from.
    pub(crate) static OPEN_MICROPHONE: RefCell<Option<String>> = const { RefCell::new(None) };
    // The device names offered by the last microphone popup menu, in menu order.
    pub(crate) static MICROPHONE_MENU: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The name Windows shows for `device` (its friendly name, e.g. "Microphone (USB Audio)").
fn device_name(device: &cpal::Device) -> Option<String> {
    device
        .description()
        .ok()
        .map(|description| description.name().to_string())
}

/// Names of all active input devices, as Windows shows them.
pub(crate) fn input_device_names() -> Vec<String> {
    match cpal::default_host().input_devices() {
        Ok(devices) => devices.filter_map(|device| device_name(&device)).collect(),
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
        .and_then(|device| device_name(&device))
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
/// ask for. Returns false when no usable input device is available, after
/// recording the reason in `UiState::mic_error` (shown by the bubble and the tray
/// icon) and, with `notify`, telling the user in a message box. Must run on the
/// main/UI thread, which owns the stream.
pub(crate) fn ensure_audio_capture(app: &Arc<AppState>, notify: bool) -> bool {
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
            if lock_ui(app).mic_error.is_some() {
                update_ui(app, |ui| ui.mic_error = None);
            }
            true
        }
        Err(err) => {
            let message = format!("{err:#}");
            // The retry timer runs into the same error every few seconds. Log it once.
            if lock_ui(app).mic_error.as_deref() != Some(message.as_str()) {
                log_error(&format!("Microphone unavailable: {message}"));
            }
            update_ui(app, |ui| ui.mic_error = Some(message.clone()));
            if notify {
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
            }
            false
        }
    }
}

/// Timer callback while no microphone is available: open the stream as soon as
/// a usable input device is back, so the bubble and the tray icon recover on
/// their own. Only a cheap device lookup runs while none is there.
pub(crate) fn retry_audio_capture(app: &Arc<AppState>) {
    if app.recording.load(Ordering::SeqCst) || lock_ui(app).mic_error.is_none() {
        return;
    }
    let wanted = settings_snapshot(app).microphone;
    let available = default_input_device_name().is_some()
        || (!wanted.is_empty() && input_device_names().contains(&wanted));
    if available {
        ensure_audio_capture(app, false);
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
    if ensure_audio_capture(app, true) {
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
            .and_then(|mut devices| devices.find(|device| device_name(device).is_some_and(|name| name == wanted)));
        if let Some(device) = found {
            return Ok(device);
        }
        log_error(&format!("Microphone '{wanted}' is not connected. Using the Windows default."));
    }
    host.default_input_device()
        .ok_or_else(|| anyhow!("no microphone is connected or enabled"))
}

/// Open and start the input stream. Returns it with the name of its device.
pub(crate) fn start_audio_capture(app: Arc<AppState>) -> Result<(cpal::Stream, String)> {
    let host = cpal::default_host();
    let device = pick_input_device(&app, &host)?;
    let device_name = device_name(&device).unwrap_or_else(|| "<unknown>".to_string());
    let input_config = device
        .default_input_config()
        .with_context(|| format!("microphone '{device_name}' has no usable input format"))?;
    let sample_rate = input_config.sample_rate() as f64;
    let channels = input_config.channels() as usize;
    let config = input_config.config();
    log_info(&format!(
        "Microphone: '{device_name}', {} Hz, {channels} channel(s), {:?}",
        sample_rate as u32,
        input_config.sample_format()
    ));

    let status = ready_status(&settings_snapshot(&app));
    update_ui(&app, |ui| ui.status = status);

    let tx = app.engine_tx.clone();
    let recording = app.recording.clone();
    let err_app = app.clone();
    let err_fn = move |err: cpal::Error| match err.kind() {
        // A short drop-out (the system was busy for a moment) or a refused
        // real-time priority. The stream keeps running.
        cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied => {}
        // The stream followed a new Windows default device on its own.
        cpal::ErrorKind::DeviceChanged => log_info(&format!("Microphone stream: {err}")),
        _ => {
            log_error(&format!("Microphone stream error: {err}"));
            MIC_FAILED.store(true, Ordering::SeqCst);
            update_ui(&err_app, |ui| {
                ui.status = "Microphone error. See the log.".to_string();
                // Shown by the bubble and the tray icon until retry_audio_capture
                // has reopened the stream.
                ui.mic_error = Some(err.to_string());
            });
        }
    };

    use cpal::SampleFormat as Format;
    let callback = InputCallback::new(channels, sample_rate, recording, tx, app.clone());
    let stream = match input_config.sample_format() {
        Format::F32 => build_input_stream::<f32>(&device, config, callback, err_fn),
        Format::F64 => build_input_stream::<f64>(&device, config, callback, err_fn),
        Format::I8 => build_input_stream::<i8>(&device, config, callback, err_fn),
        Format::I16 => build_input_stream::<i16>(&device, config, callback, err_fn),
        Format::I24 => build_input_stream::<cpal::I24>(&device, config, callback, err_fn),
        Format::I32 => build_input_stream::<i32>(&device, config, callback, err_fn),
        Format::I64 => build_input_stream::<i64>(&device, config, callback, err_fn),
        Format::U8 => build_input_stream::<u8>(&device, config, callback, err_fn),
        Format::U16 => build_input_stream::<u16>(&device, config, callback, err_fn),
        Format::U24 => build_input_stream::<cpal::U24>(&device, config, callback, err_fn),
        Format::U32 => build_input_stream::<u32>(&device, config, callback, err_fn),
        Format::U64 => build_input_stream::<u64>(&device, config, callback, err_fn),
        other => Err(anyhow!("unsupported microphone sample format: {other:?}")),
    }?;

    stream
        .play()
        .with_context(|| format!("could not start recording from '{device_name}'"))?;
    Ok((stream, device_name))
}

/// Build the (paused) input stream for samples of type `T`, which `callback`
/// receives converted to f32.
fn build_input_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut callback: InputCallback,
    err_fn: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    Ok(device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| callback.push(data),
        err_fn,
        None,
    )?)
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

    /// Samples of any format, converted to f32 in -1.0..1.0.
    pub(crate) fn push<T>(&mut self, data: &[T])
    where
        T: cpal::Sample,
        f32: cpal::FromSample<T>,
    {
        self.process(data.iter().map(|sample| <f32 as cpal::FromSample<T>>::from_sample_(*sample)));
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

        let mono = downmix_to_mono(samples, self.channels);
        if !mono.is_empty() {
            let (level, rms) = audio_level(&mono);
            report_level(&self.app, level, rms);
        }

        let mut out = Vec::new();
        self.resampler.push(&mono, &mut out);
        if !out.is_empty() {
            let _ = self.tx.try_send(EngineCommand::Audio(out));
        }
    }
}

/// Averages interleaved frames of `channels` samples into one mono sample each.
/// An incomplete frame at the end of the block is dropped.
pub(crate) fn downmix_to_mono<I>(samples: I, channels: usize) -> Vec<f32>
where
    I: Iterator<Item = f32>,
{
    let mut mono = Vec::new();
    let mut frame = Vec::with_capacity(channels);
    for sample in samples {
        frame.push(sample);
        if frame.len() == channels {
            let sum = frame.iter().sum::<f32>();
            mono.push(sum / channels as f32);
            frame.clear();
        }
    }
    mono
}

/// Loudness of a non-empty mono block: the waveform bar height in 0.0..=1.0 and the RMS.
fn audio_level(mono: &[f32]) -> (f32, f32) {
    // Loudness -> bar height. NOISE_FLOOR gates hiss; GAIN sets sensitivity so
    // ordinary speech reaches most of the height; the sub-1.0 exponent lifts
    // quiet syllables so they read as tall bumps instead of tiny specks.
    const NOISE_FLOOR: f32 = 0.006;
    const GAIN: f32 = 14.0;
    const SHAPE: f32 = 0.6;
    let rms = (mono.iter().map(|v| v * v).sum::<f32>() / mono.len() as f32).sqrt();
    let norm = ((rms - NOISE_FLOOR) * GAIN).clamp(0.0, 1.0);
    let level = norm.powf(SHAPE);
    (level, rms)
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

/// Audio from a WAV file: interleaved samples in -1.0..1.0.
#[cfg(test)]
pub(crate) struct WavAudio {
    pub(crate) channels: usize,
    pub(crate) sample_rate: u32,
    pub(crate) samples: Vec<f32>,
}

/// Reads a 16-bit PCM WAV file, the format test recordings use. Other formats are rejected.
#[cfg(test)]
pub(crate) fn read_wav(bytes: &[u8]) -> Result<WavAudio> {
    let u16_at = |offset: usize| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    let u32_at = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"));

    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("not a RIFF/WAVE file");
    }
    let mut format = None;
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32_at(offset + 4) as usize;
        let body = offset + 8;
        let end = body.checked_add(size).filter(|end| *end <= bytes.len());
        let Some(end) = end else {
            bail!("chunk {:?} is truncated", String::from_utf8_lossy(id));
        };
        match id {
            b"fmt " if size >= 16 => {
                let (tag, channels, rate, bits) = (u16_at(body), u16_at(body + 2), u32_at(body + 4), u16_at(body + 14));
                if tag != 1 || bits != 16 || channels == 0 {
                    bail!("only 16-bit PCM is supported (format {tag}, {bits} bits, {channels} channels)");
                }
                format = Some((channels as usize, rate));
            }
            b"data" => {
                let Some((channels, sample_rate)) = format else {
                    bail!("data chunk before fmt chunk");
                };
                let samples = bytes[body..end]
                    .chunks_exact(2)
                    .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32_768.0)
                    .collect();
                return Ok(WavAudio { channels, sample_rate, samples });
            }
            _ => {}
        }
        // Chunks are padded to an even size.
        offset = end + (size & 1);
    }
    bail!("no data chunk")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// A 16-bit PCM WAV file with an extra chunk before the data, as many editors write it.
    fn wav_file(channels: u16, sample_rate: u32, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1u16.to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&sample_rate.to_le_bytes());
        fmt.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
        fmt.extend_from_slice(&(channels * 2).to_le_bytes());
        fmt.extend_from_slice(&16u16.to_le_bytes());

        let mut body = b"WAVE".to_vec();
        for (id, content) in [(b"fmt ", fmt), (b"LIST", b"odd".to_vec()), (b"data", data)] {
            body.extend_from_slice(id);
            body.extend_from_slice(&(content.len() as u32).to_le_bytes());
            body.extend_from_slice(&content);
            if content.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }

    #[test]
    fn read_wav_reads_16_bit_pcm() {
        let wav = read_wav(&wav_file(2, 24_000, &[0, 16_384, -32_768, 32_767])).unwrap();
        assert_eq!((wav.channels, wav.sample_rate), (2, 24_000));
        assert_eq!(wav.samples, [0.0, 0.5, -1.0, 32_767.0 / 32_768.0]);
    }

    #[test]
    fn read_wav_rejects_other_formats() {
        let mut float = wav_file(1, 16_000, &[0, 0]);
        float[20] = 3; // Format tag 3 is IEEE float.
        assert!(read_wav(&float).is_err());
        assert!(read_wav(b"RIFF\0\0\0\0WAVX").is_err());
        assert!(read_wav(b"").is_err());
    }

    #[test]
    fn read_wav_rejects_truncated_data() {
        let mut wav = wav_file(1, 16_000, &[1, 2, 3, 4]);
        wav.truncate(wav.len() - 2);
        assert!(read_wav(&wav).is_err());
    }

    fn sine(frequency: f32, rate: f32, count: usize) -> Vec<f32> {
        (0..count).map(|index| (TAU * frequency * index as f32 / rate).sin()).collect()
    }

    fn resample_whole(source_rate: f64, input: &[f32]) -> Vec<f32> {
        let mut resampler = LinearResampler::new(source_rate, NEMOTRON_SAMPLE_RATE);
        let mut output = Vec::new();
        resampler.push(input, &mut output);
        output
    }

    /// Feeds `input` in pieces of changing size, as audio callbacks deliver it.
    fn resample_in_pieces(source_rate: f64, input: &[f32]) -> Vec<f32> {
        const PIECE_SIZES: [usize; 6] = [1, 7, 480, 13, 1024, 2];
        let mut resampler = LinearResampler::new(source_rate, NEMOTRON_SAMPLE_RATE);
        let mut output = Vec::new();
        let mut rest = input;
        for size in PIECE_SIZES.iter().cycle() {
            if rest.is_empty() {
                break;
            }
            let (piece, remaining) = rest.split_at((*size).min(rest.len()));
            resampler.push(piece, &mut output);
            rest = remaining;
        }
        output
    }

    #[test]
    fn resampler_produces_one_third_of_48_khz_samples() {
        let output = resample_whole(48_000.0, &sine(440.0, 48_000.0, 4_800));
        assert_eq!(output.len(), 1_600);
    }

    #[test]
    fn resampler_keeps_the_frequency() {
        for source_rate in [48_000.0, 44_100.0, 24_000.0] {
            let output = resample_whole(source_rate, &sine(440.0, source_rate as f32, source_rate as usize / 10));
            let expected = sine(440.0, NEMOTRON_SAMPLE_RATE as f32, output.len());
            for (index, (actual, wanted)) in output.iter().zip(&expected).enumerate() {
                // Linear interpolation of a 440 Hz sine stays well within this.
                assert!((actual - wanted).abs() < 0.01, "{source_rate} Hz, sample {index}: {actual} vs {wanted}");
            }
        }
    }

    #[test]
    fn resampler_output_does_not_depend_on_block_sizes() {
        for source_rate in [48_000.0, 44_100.0, 24_000.0] {
            let input = sine(440.0, source_rate as f32, 10_000);
            let whole = resample_whole(source_rate, &input);
            let pieces = resample_in_pieces(source_rate, &input);
            assert_eq!(whole.len(), pieces.len(), "{source_rate} Hz");
            for (index, (a, b)) in whole.iter().zip(&pieces).enumerate() {
                assert!((a - b).abs() < 1e-5, "{source_rate} Hz, sample {index}: {a} vs {b}");
            }
        }
    }

    #[test]
    fn resampler_at_same_rate_holds_back_only_the_last_sample() {
        let input = [0.1, 0.2, 0.3, 0.4];
        assert_eq!(resample_whole(NEMOTRON_SAMPLE_RATE, &input), [0.1, 0.2, 0.3]);
    }

    #[test]
    fn resampler_reset_forgets_the_previous_block() {
        let mut resampler = LinearResampler::new(48_000.0, NEMOTRON_SAMPLE_RATE);
        let mut output = Vec::new();
        resampler.push(&[1.0; 100], &mut output);
        resampler.reset();

        let mut after_reset = Vec::new();
        resampler.push(&[0.0; 100], &mut after_reset);
        assert!(after_reset.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn resampler_ignores_empty_input() {
        let mut resampler = LinearResampler::new(48_000.0, NEMOTRON_SAMPLE_RATE);
        let mut output = Vec::new();
        resampler.push(&[], &mut output);
        assert!(output.is_empty());
        assert_eq!(resampler.last_sample, None);
    }

    #[test]
    fn downmix_averages_each_frame() {
        let stereo = [1.0, 0.0, 0.5, -0.5, -1.0, -1.0];
        assert_eq!(downmix_to_mono(stereo.into_iter(), 2), [0.5, 0.0, -1.0]);
    }

    #[test]
    fn downmix_keeps_mono_and_drops_incomplete_frames() {
        assert_eq!(downmix_to_mono([0.1, 0.2].into_iter(), 1), [0.1, 0.2]);
        assert_eq!(downmix_to_mono([0.3, 0.3, 0.3, 0.9].into_iter(), 3), [0.3]);
    }

    #[test]
    fn level_is_zero_for_silence_and_noise() {
        assert_eq!(audio_level(&[0.0; 64]), (0.0, 0.0));
        let (level, rms) = audio_level(&[0.005; 64]);
        assert_eq!(level, 0.0);
        assert!((rms - 0.005).abs() < 1e-6);
    }

    #[test]
    fn level_rises_with_loudness_and_is_capped() {
        let (quiet, _) = audio_level(&[0.02; 64]);
        let (louder, _) = audio_level(&[0.05; 64]);
        assert!(0.0 < quiet && quiet < louder && louder < 1.0, "{quiet} < {louder}");
        let (loud, rms) = audio_level(&[0.5; 64]);
        assert_eq!(loud, 1.0);
        assert!((rms - 0.5).abs() < 1e-6);
    }
}

