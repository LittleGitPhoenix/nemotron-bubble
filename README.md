<div align="center">

# Nemotron Bubble

**A tiny, beautiful push-to-talk dictation bubble for Windows.**

Press your shortcut, speak, and your words appear at the cursor — powered by **on-device** NVIDIA Nemotron speech recognition.

<img src="docs/demo.gif" width="460" alt="Nemotron Bubble live waveform" />

![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6?logo=windows&logoColor=white)
![Built with Rust](https://img.shields.io/badge/built%20with-Rust-CE412B?logo=rust&logoColor=white)
![100% offline](https://img.shields.io/badge/100%25-offline-2ea44f)
![License: MIT](https://img.shields.io/badge/license-MIT-blue)

</div>

---

## Features

- **Push-to-talk** — one shortcut starts and stops dictation from any app.
- **Customizable shortcut** — click *Global shortcut* in settings and press any combo (Ctrl / Alt / Shift + key). Defaults to `Ctrl+Space`.
- **Safe insertion by default** — on stop the transcript is pasted once (clipboard + a single Ctrl+V), so it never trips app shortcuts. *Type live into cursor* is an opt-in for real text editors.
- **Auto-stops on silence** — forgets nothing but won't run forever; stops after 15 s of quiet.
- **Private and offline** — runs the Nemotron ONNX model locally. No internet, no accounts, no telemetry.
- **Live waveform bubble** — a smooth, anti-aliased floating mic meter you can drag anywhere.
- **Tray mode** — hide the bubble and watch the waveform pulse right in the taskbar tray instead.
- **Recent dictations** — scrollable history with one-click *Copy Latest*.
- **One clean clipboard copy on stop** — with optional auto-paste.
- **Starts with Windows**, soft sound cues, model preloading — all toggleable.

## Settings

<img src="docs/settings.png" width="620" alt="Settings window" />

## Requirements

- **Windows 10 or 11** (64-bit).
- A **microphone**.
- **~2.4 GB free disk** for the speech model (downloaded once).
- **~3 GB of free RAM** while dictating — the model runs entirely in memory.
- To build from source: the [Rust toolchain](https://rustup.rs) (MSVC).

> **Memory note:** the Nemotron model is ~0.6B parameters and uses roughly **2.5–3 GB of RAM** while loaded. With *Preload Nemotron* enabled (default) that memory is held so the first dictation is instant. Turn it off to keep RAM free when idle — the model then loads on first use instead.

## Quick start

```powershell
git clone https://github.com/snipemanmike/nemotron-bubble.git
cd nemotron-bubble

# One-time: download the Nemotron speech model (~2.4 GB) into models\nemotron\
.\scripts\download-nemotron.ps1

# Build and run
cargo run --release
```

Prefer the multilingual model? Run `.\scripts\download-nemotron.ps1 -Multilingual`.
You can also point at a custom folder with `$env:NEMOTRON_MODEL_DIR = "C:\path\to\nemotron"`.

Or grab a prebuilt binary from the [Releases](https://github.com/snipemanmike/nemotron-bubble/releases) page and run the download script next to it.

### Choosing the model

Pick the model under **Speech model** in the settings window:

| Choice | Loads |
| --- | --- |
| **Predefined** | the folder in `NEMOTRON_MODEL_DIR`, otherwise `"model_dir"` from `config\settings.json` |
| **English** | `models\nemotron` |
| **Multilingual** | `models\nemotron_multi`, which detects the spoken language automatically |

`models\` is searched in the working directory, next to the exe and up to four parent folders.
On first start the choice is detected: **Predefined** if one of its folders is set, otherwise
**English** if installed, otherwise **Multilingual**. Switching unloads the old model first;
with *Preload Nemotron* on, the new one loads right away. The line under *Speech model* shows
what is loaded, as reported by the model itself.

### Processing (GPU or CPU)

Under **Processing** choose where the model runs: **CPU** (default) the processor, **GPU**
DirectML (any DirectX 12 graphics adapter, including integrated ones), and **Auto** tries the
GPU first and falls back to the CPU. Switching reloads the model. The line under *Speech
model* shows the device in use, and each entry in the recent dictations (and in
`config\history.json`) records the device and the average processing time per 560 ms audio
chunk, so you can compare both on your machine. An integrated GPU is not necessarily faster
than the CPU. `--self-test` prints the same measurement.

### Language (multilingual model)

By default the multilingual model works out the spoken language itself. If you know the
language, set it under **Language** in the settings window or the tray menu; NVIDIA's
documentation says this is more accurate than auto-detection. The choice applies instantly,
without reloading the model. Offered are *Auto-detect* plus the 15 languages NVIDIA rates
transcription-ready: Arabic, Dutch, English, French, German, Hindi, Italian, Japanese,
Korean, Portuguese, Russian, Spanish, Turkish, Ukrainian, Vietnamese. With the English
model selected the setting is greyed out.

### Portable layout

The app keeps all its files next to the exe, so the folder can be copied or deleted as a whole:

```
nemotron-bubble.exe
DirectML.dll
config\    settings.json, history.json, sounds\
log\       nemotron-bubble.log, crash.log
models\    nemotron\, nemotron_multi\
```

The folder must be writable, so don't put it under `C:\Program Files`. The only thing outside
it is the *Start with Windows* entry (`HKCU\...\CurrentVersion\Run\NemotronBubble`), which
the app removes when you turn that setting off.

## Troubleshooting

- **Log file:** `log\nemotron-bubble.log` (next to the exe) records startup, microphone,
  model loading (folder, file sizes, mode, load time) and every error. The same lines go to
  `OutputDebugString`, so [DebugView](https://learn.microsoft.com/sysinternals/downloads/debugview)
  shows them live (filter on `NemotronBubble`).
- **Self-test:** run `nemotron-bubble.exe --self-test` from a terminal. It loads the model,
  runs one chunk, and prints the result or the full error.
- **No microphone:** the app shows a warning and keeps running in the tray. It reopens the
  microphone the next time you start dictation.
- **Model fails to load:** a dialog shows the reason. An interrupted download is the usual
  cause: `encoder.onnx.data` must be about 2.5 GB. Delete the bad file and rerun the script.

## Controls

| Action | How |
| --- | --- |
| Start / stop dictation | your shortcut (default `Ctrl+Space`) |
| Change the shortcut | open settings, click *Global shortcut*, press the new combo |
| Open / close settings | click the bubble **or** the tray icon |
| Move the bubble | drag it |
| Tray menu (start, show/hide, quit) | right-click the tray icon |

## How it works

Nemotron Bubble is a single-file Rust app built directly on the Win32 API — no Electron, no web view, ~5 MB binary. Microphone audio is captured with [`cpal`](https://crates.io/crates/cpal), streamed through the [`parakeet-rs`](https://crates.io/crates/parakeet-rs) Nemotron ONNX model, and injected into the focused window as Unicode keystrokes. The floating bubble is a **per-pixel-alpha layered window** drawn by a small software canvas with signed-distance anti-aliasing, so the pill, shadow, and waveform stay crisp and smooth.

## Build

```powershell
cargo build --release            # the app

# Regenerate the README art (bubble.png, demo.gif, settings.png)
cargo run --release --features assets -- --render-assets docs

# Regenerate the exe icon (embedded by build.rs together with the version info)
cargo run --release --features assets -- --render-icon assets/nemotron-bubble.ico
```

## Credits

- Speech recognition via [parakeet-rs](https://crates.io/crates/parakeet-rs) and NVIDIA's Nemotron streaming ASR.
- Model weights hosted on Hugging Face.

## License

MIT — see [LICENSE](LICENSE).
