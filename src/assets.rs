//! README art and exe icon export (only with the `assets` feature).
//!
//! Reuses the real renderer to emit images on a clean, generated background:
//!
//! | File | Content |
//! | --- | --- |
//! | `bubble.png` | the floating bubble on transparency, with its soft shadow |
//! | `demo.gif` | the waveform pulsing on a gradient, as a seamless loop |
//! | `settings-history.png`, `settings-model.png`, `settings-settings.png` | the tabs of the settings window with demo data |
//!
//! Run with `cargo run --release --features assets -- --render-assets docs`. The exe
//! icon comes from `--render-icon assets/nemotron-bubble.ico`.

use crate::*;

#[cfg(feature = "assets")]
pub(crate) fn render_assets(out_dir: &str) -> Result<()> {
    use std::path::Path;
    let dir = Path::new(out_dir);
    std::fs::create_dir_all(dir)?;

    let bars = WAVE_BARS;
    let pi = std::f32::consts::PI;
    // A lively per-bar pattern. Integer "speed" per bar keeps the loop seamless.
    let frame_wave = |t: f32, frames: f32| -> Vec<f32> {
        (0..bars)
            .map(|i| {
                let phase = i as f32 * 0.55;
                let speed = 1.0 + (i % 3) as f32; // 1,2,3 -> periodic over [0, frames)
                let hump = (i as f32 / (bars - 1) as f32 * pi).sin(); // taller in the middle
                let env = 0.42 + 0.58 * hump;
                let s = (2.0 * pi * (t / frames) * speed + phase).sin();
                (env * (0.30 + 0.70 * (0.5 + 0.5 * s))).clamp(0.06, 1.0)
            })
            .collect()
    };

    // ---- bubble.png : transparent, 2x upscaled ----
    {
        let wave = frame_wave(9.0, 48.0);
        let mut buf = vec![0u32; (WIDTH * HEIGHT) as usize];
        paint_bubble_into(&mut buf, WIDTH, HEIGHT, 0.0, 0.0, &wave, true, true);
        let (big, bw, bh) = upscale_premul(&buf, WIDTH as usize, HEIGHT as usize, 2);
        let rgba: Vec<u8> = big.iter().flat_map(|&p| unpremultiply(p)).collect();
        write_png(&dir.join("bubble.png"), bw as u32, bh as u32, &rgba)?;
    }

    // ---- demo.gif : bubble pulsing on a gradient ----
    {
        const HERO_W: usize = 480;
        const HERO_H: usize = 220;
        const FRAMES: usize = 48;
        let scale = 2usize;
        let bg = gradient_bg(HERO_W, HERO_H);
        let bw = WIDTH as usize * scale;
        let bh = HEIGHT as usize * scale;
        let ox = (HERO_W - bw) / 2;
        let oy = (HERO_H - bh) / 2;
        let mut frames: Vec<Vec<u32>> = Vec::with_capacity(FRAMES);
        for t in 0..FRAMES {
            let wave = frame_wave(t as f32, FRAMES as f32);
            let mut buf = vec![0u32; (WIDTH * HEIGHT) as usize];
            paint_bubble_into(&mut buf, WIDTH, HEIGHT, 0.0, 0.0, &wave, true, true);
            let (big, _, _) = upscale_premul(&buf, WIDTH as usize, HEIGHT as usize, scale);
            let mut frame = bg.clone();
            composite_over(&mut frame, HERO_W, HERO_H, &big, bw, bh, ox, oy);
            frames.push(frame);
        }
        write_gif(&dir.join("demo.gif"), HERO_W, HERO_H, &frames, 3)?;
    }

    // ---- settings-<tab>.png : one image per tab of the settings window ----
    {
        let history = demo_history();
        let cw = SETTINGS_WIDTH;
        let ch = SETTINGS_HEIGHT;
        // The Model tab shows the multilingual model with a language chosen, so
        // the Language control is not greyed out in the README.
        let multilingual = AppSettings {
            model: ModelChoice::Multilingual,
            language: "de".to_string(),
            ..AppSettings::default()
        };
        for (tab, name, settings, status) in [
            (TAB_HISTORY, "settings-history.png", AppSettings::default(), ""),
            (TAB_MODEL, "settings-model.png", multilingual, "Loaded: Multilingual (German) · CPU"),
            (TAB_SETTINGS, "settings-settings.png", AppSettings::default(), ""),
        ] {
            if let Some(mut canvas) = unsafe { Canvas::new(cw, ch) } {
                let view = HistoryView {
                    items: &history,
                    scroll: 0,
                    copied: None,
                };
                let settings_view = SettingsTabView {
                    scroll: 0,
                    microphone_in_use: Some("Microphone (USB Audio)".to_string()),
                };
                // Downloads: English halfway through its download, Multilingual installed and current.
                let downloads = [
                    ModelDownloadState {
                        activity: DownloadActivity::Downloading {
                            done: 1_032_000_000,
                            total: 2_517_642_476,
                        },
                        ..ModelDownloadState::default()
                    },
                    ModelDownloadState {
                        installed_dir: Some(PathBuf::from("models")),
                        installed_bytes: 2_597_582_765,
                        local_date: Some("2026-10-22".to_string()),
                        up_to_date: Some(true),
                        ..ModelDownloadState::default()
                    },
                ];
                unsafe {
                    render_settings_canvas(&mut canvas, &settings, status, &view, &settings_view, &downloads, false, tab)
                };
                let rgba = settings_to_rgba(canvas.pixels(), cw as usize, ch as usize, 16.0);
                write_png(&dir.join(name), cw as u32, ch as u32, &rgba)?;
            }
        }
    }

    println!("Wrote bubble.png, demo.gif and settings-<tab>.png to {out_dir}");
    Ok(())
}

#[cfg(feature = "assets")]
pub(crate) fn demo_history() -> Vec<HistoryItem> {
    let make = |timestamp: &str, device: &str, ms: u32, text: &str| HistoryItem {
        timestamp: timestamp.to_string(),
        text: text.to_string(),
        processing: Some(device.to_string()),
        ms_per_chunk: Some(ms),
    };
    vec![
        make("Nov 24, 2:14 PM", "CPU", 128, "Let's ship the new dictation bubble — it looks fantastic."),
        make("Nov 24, 2:11 PM", "CPU", 131, "Meeting notes: confirm the roadmap and follow up with the design team tomorrow."),
        make("Nov 24, 1:58 PM", "GPU", 287, "Reminder to refactor the audio capture pipeline next sprint."),
        make("Nov 24, 1:42 PM", "CPU", 126, "The quick brown fox jumps over the lazy dog."),
        make("Nov 23, 6:30 PM", "CPU", 133, "Draft the release notes and update the screenshots in the README."),
    ]
}

#[cfg(feature = "assets")]
pub(crate) fn unpremultiply(p: u32) -> [u8; 4] {
    let a = ((p >> 24) & 0xff) as u32;
    if a == 0 {
        return [0, 0, 0, 0];
    }
    let un = |c: u32| (((c & 0xff) * 255 + a / 2) / a).min(255) as u8;
    [un(p >> 16), un(p >> 8), un(p), a as u8]
}

#[cfg(feature = "assets")]
pub(crate) fn lerp_argb(a: u32, b: u32, t: f32) -> u32 {
    let mix = |sh: u32| {
        let ca = ((a >> sh) & 0xff) as f32;
        let cb = ((b >> sh) & 0xff) as f32;
        (((ca + (cb - ca) * t).round() as u32).min(255)) << sh
    };
    mix(24) | mix(16) | mix(8) | mix(0)
}

/// Bilinear upscale of a premultiplied ARGB buffer (interpolating alpha too).
#[cfg(feature = "assets")]
pub(crate) fn upscale_premul(src: &[u32], sw: usize, sh: usize, scale: usize) -> (Vec<u32>, usize, usize) {
    let dw = sw * scale;
    let dh = sh * scale;
    let mut dst = vec![0u32; dw * dh];
    for dy in 0..dh {
        let fy = (dy as f32 + 0.5) / scale as f32 - 0.5;
        let y0 = fy.floor().max(0.0) as usize;
        let y1 = (y0 + 1).min(sh - 1);
        let ty = (fy - y0 as f32).clamp(0.0, 1.0);
        for dx in 0..dw {
            let fx = (dx as f32 + 0.5) / scale as f32 - 0.5;
            let x0 = fx.floor().max(0.0) as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let tx = (fx - x0 as f32).clamp(0.0, 1.0);
            let top = lerp_argb(src[y0 * sw + x0], src[y0 * sw + x1], tx);
            let bot = lerp_argb(src[y1 * sw + x0], src[y1 * sw + x1], tx);
            dst[dy * dw + dx] = lerp_argb(top, bot, ty);
        }
    }
    (dst, dw, dh)
}

/// A tasteful opaque gradient backdrop with a soft blue glow near the top-centre.
#[cfg(feature = "assets")]
pub(crate) fn gradient_bg(w: usize, h: usize) -> Vec<u32> {
    let mut v = vec![0u32; w * h];
    for y in 0..h {
        let t = y as f32 / (h - 1).max(1) as f32;
        for x in 0..w {
            let base_r = lerp(34.0, 14.0, t);
            let base_g = lerp(38.0, 16.0, t);
            let base_b = lerp(56.0, 23.0, t);
            let dx = (x as f32 - w as f32 * 0.5) / (w as f32 * 0.6);
            let dy = (y as f32 - h as f32 * 0.30) / (h as f32 * 0.6);
            let glow = (1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0).powf(2.0) * 24.0;
            let r = (base_r + glow * 0.5).min(255.0) as u32;
            let g = (base_g + glow * 0.8).min(255.0) as u32;
            let b = (base_b + glow * 1.5).min(255.0) as u32;
            v[y * w + x] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
        }
    }
    v
}

/// Composite a premultiplied ARGB sprite over an opaque background buffer.
#[cfg(feature = "assets")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_over(
    bg: &mut [u32],
    bgw: usize,
    bgh: usize,
    fg: &[u32],
    fw: usize,
    fh: usize,
    ox: usize,
    oy: usize,
) {
    for y in 0..fh {
        let by = oy + y;
        if by >= bgh {
            break;
        }
        for x in 0..fw {
            let bx = ox + x;
            if bx >= bgw {
                continue;
            }
            let s = fg[y * fw + x];
            let sa = ((s >> 24) & 0xff) as f32 / 255.0;
            if sa <= 0.0 {
                continue;
            }
            let inv = 1.0 - sa;
            let d = bg[by * bgw + bx];
            let r = (((s >> 16) & 0xff) as f32 + ((d >> 16) & 0xff) as f32 * inv).min(255.0) as u32;
            let g = (((s >> 8) & 0xff) as f32 + ((d >> 8) & 0xff) as f32 * inv).min(255.0) as u32;
            let b = ((s & 0xff) as f32 + (d & 0xff) as f32 * inv).min(255.0) as u32;
            bg[by * bgw + bx] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
        }
    }
}

/// Convert an opaque settings canvas to RGBA: GDI text leaves the alpha byte
/// undefined, so force alpha opaque and apply a rounded-corner mask for polish.
#[cfg(feature = "assets")]
pub(crate) fn settings_to_rgba(buf: &[u32], w: usize, h: usize, radius: f32) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;
    let hx = w as f32 / 2.0;
    let hy = h as f32 / 2.0;
    for y in 0..h {
        for x in 0..w {
            let p = buf[y * w + x];
            let d = sd_round_rect(x as f32 + 0.5, y as f32 + 0.5, cx, cy, hx, hy, radius);
            let cov = (0.5 - d).clamp(0.0, 1.0);
            let i = (y * w + x) * 4;
            out[i] = ((p >> 16) & 0xff) as u8;
            out[i + 1] = ((p >> 8) & 0xff) as u8;
            out[i + 2] = (p & 0xff) as u8;
            out[i + 3] = (cov * 255.0) as u8;
        }
    }
    out
}

#[cfg(feature = "assets")]
pub(crate) fn write_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

/// Write the exe icon (embedded by build.rs) as a multi-size .ico.
/// Run with:  cargo run --release --features assets -- --render-icon assets/nemotron-bubble.ico
#[cfg(feature = "assets")]
pub(crate) fn render_icon(path: &std::path::Path) -> Result<()> {
    const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];
    let mut images = Vec::new();
    for size in SIZES {
        let rgba = app_icon_rgba(size);
        let data = if size >= 256 {
            // 256 px: PNG-compressed, as Windows expects for the large size.
            let mut png_bytes = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut png_bytes, size, size);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.write_header()?.write_image_data(&rgba)?;
            }
            png_bytes
        } else {
            icon_dib(size, &rgba)
        };
        images.push((size, data));
    }

    // ICO container: uncompressed 32-bit DIBs for the small sizes (readable by
    // every tool), PNG for 256 px.
    let mut ico = Vec::new();
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    ico.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        let dim = if *size >= 256 { 0u8 } else { *size as u8 }; // 0 means 256
        ico.extend_from_slice(&[dim, dim, 0, 0]); // width, height, palette, reserved
        ico.extend_from_slice(&1u16.to_le_bytes()); // planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        ico.extend_from_slice(data);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, ico)?;
    println!("Wrote {} ({} sizes)", path.display(), images.len());
    Ok(())
}

/// One ICO entry as a 32-bit DIB: BITMAPINFOHEADER (height doubled to cover the
/// AND mask), BGRA rows bottom-up, then an all-zero 1-bit AND mask (alpha rules).
#[cfg(feature = "assets")]
pub(crate) fn icon_dib(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mask_row = size.div_ceil(32) * 4; // 1 bpp rows padded to 32 bits
    let mut dib = Vec::new();
    dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
    dib.extend_from_slice(&(size as i32).to_le_bytes()); // biWidth
    dib.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // biHeight: XOR + AND
    dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    dib.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    dib.extend_from_slice(&(size * size * 4 + mask_row * size).to_le_bytes()); // biSizeImage
    dib.extend_from_slice(&[0u8; 16]); // resolution + palette fields
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            dib.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    dib.resize(dib.len() + (mask_row * size) as usize, 0);
    dib
}

/// The app icon as straight-alpha RGBA: a smooth, resolution-independent take on
/// the tray glyph (dark disc with the equalizer bars in the recording blue).
/// Geometry is defined on the tray icon's 32-unit grid and supersampled 4x4.
#[cfg(feature = "assets")]
pub(crate) fn app_icon_rgba(size: u32) -> Vec<u8> {
    const SHAPE: [f32; 9] = [0.30, 0.58, 0.86, 1.0, 0.72, 0.46, 0.66, 0.9, 0.5];
    const SS: u32 = 4;
    let disc = (26.0f32, 29.0, 35.0); // tray background 0x1A1D21
    let bar = (122.0f32, 184.0, 255.0); // recording accent 0x7AB8FF
    let rim = (72.0f32, 80.0, 96.0);
    let scale = 32.0 / size as f32;

    // Capsule (rounded bar) membership for a point on the 32-unit grid.
    let in_bar = |x: f32, y: f32| -> bool {
        let radius = 0.75;
        SHAPE.iter().enumerate().any(|(i, s)| {
            let cx = 8.0 + i as f32 * 2.0;
            let half = (s * 0.8 * 22.0) / 2.0;
            let top = 16.0 - half + radius;
            let bottom = 16.0 + half - radius;
            let dy = if y < top { top - y } else if y > bottom { y - bottom } else { 0.0 };
            let dx = x - cx;
            dx * dx + dy * dy <= radius * radius
        })
    };

    let mut out = vec![0u8; (size * size * 4) as usize];
    for py in 0..size {
        for px in 0..size {
            let (mut disc_hits, mut bar_hits, mut rim_hits) = (0u32, 0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) * scale;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) * scale;
                    let (dx, dy) = (x - 16.0, y - 16.0);
                    let dist_sq = dx * dx + dy * dy;
                    if dist_sq <= 15.0 * 15.0 {
                        disc_hits += 1;
                        if in_bar(x, y) {
                            bar_hits += 1;
                        } else if dist_sq >= 14.0 * 14.0 {
                            // thin lighter rim so the disc stays visible on dark backgrounds
                            rim_hits += 1;
                        }
                    }
                }
            }
            if disc_hits == 0 {
                continue;
            }
            let hits = disc_hits as f32;
            let (tb, tr) = (bar_hits as f32 / hits, rim_hits as f32 / hits);
            let td = 1.0 - tb - tr;
            let mix = |d: f32, b: f32, r: f32| (d * td + b * tb + r * tr).round() as u8;
            let i = ((py * size + px) * 4) as usize;
            out[i] = mix(disc.0, bar.0, rim.0);
            out[i + 1] = mix(disc.1, bar.1, rim.1);
            out[i + 2] = mix(disc.2, bar.2, rim.2);
            out[i + 3] = ((disc_hits as f32 / (SS * SS) as f32) * 255.0).round() as u8;
        }
    }
    out
}

#[cfg(feature = "assets")]
pub(crate) fn write_gif(
    path: &std::path::Path,
    w: usize,
    h: usize,
    frames: &[Vec<u32>],
    delay_cs: u16,
) -> Result<()> {
    let mut file = std::fs::File::create(path)?;
    let mut encoder = gif::Encoder::new(&mut file, w as u16, h as u16, &[])?;
    encoder.set_repeat(gif::Repeat::Infinite)?;
    for frame in frames {
        let mut rgba: Vec<u8> = Vec::with_capacity(frame.len() * 4);
        for &p in frame {
            rgba.push(((p >> 16) & 0xff) as u8);
            rgba.push(((p >> 8) & 0xff) as u8);
            rgba.push((p & 0xff) as u8);
            rgba.push(255);
        }
        let mut f = gif::Frame::from_rgba_speed(w as u16, h as u16, &mut rgba, 10);
        f.delay = delay_cs;
        encoder.write_frame(&f)?;
    }
    Ok(())
}
