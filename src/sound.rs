//! Start/stop sound cues.

use crate::*;

pub(crate) fn ensure_sound_assets() {
    let dir = config_dir().join("sounds");
    let _ = fs::create_dir_all(&dir);
    let start = dir.join("start.wav");
    let stop = dir.join("stop.wav");
    let _ = write_chime(&start, &[(329.63, 0.08), (392.00, 0.12)]);
    let _ = write_chime(&stop, &[(392.00, 0.07), (293.66, 0.11)]);
}

pub(crate) fn sound_path(name: &str) -> PathBuf {
    config_dir().join("sounds").join(name)
}

pub(crate) fn write_chime(path: &PathBuf, notes: &[(f32, f32)]) -> Result<()> {
    const SAMPLE_RATE: u32 = 22_050;
    let mut samples = Vec::<i16>::new();
    for (freq, seconds) in notes {
        let count = (SAMPLE_RATE as f32 * seconds) as usize;
        for i in 0..count {
            let t = i as f32 / SAMPLE_RATE as f32;
            let progress = i as f32 / count.max(1) as f32;
            let attack = (progress / 0.18).min(1.0);
            let release = ((1.0 - progress) / 0.35).min(1.0);
            let envelope = attack.min(release).powf(1.4);
            let wave = (2.0 * std::f32::consts::PI * freq * t).sin();
            samples.push((wave * envelope * 6_500.0) as i16);
        }
    }

    let mut file = fs::File::create(path)?;
    let data_len = samples.len() as u32 * 2;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_len).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&SAMPLE_RATE.to_le_bytes())?;
    file.write_all(&(SAMPLE_RATE * 2).to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    for sample in samples {
        file.write_all(&sample.to_le_bytes())?;
    }
    Ok(())
}

pub(crate) fn play_start_sound(app: &Arc<AppState>) {
    if !settings_snapshot(app).sounds_enabled {
        return;
    }
    play_sound_file(sound_path("start.wav"));
}

pub(crate) fn play_stop_sound(app: &Arc<AppState>) {
    if !settings_snapshot(app).sounds_enabled {
        return;
    }
    play_sound_file(sound_path("stop.wav"));
}

pub(crate) fn play_sound_file(path: PathBuf) {
    thread::spawn(move || unsafe {
        let wide = wide_null(&path.display().to_string());
        PlaySoundW(wide.as_ptr(), null_mut(), SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
    });
}

