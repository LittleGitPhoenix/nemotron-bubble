# Nemotron Bubble - Technical details

This file is for developers and power users. It describes how the app works, how the repository is organized, how to build and test it, and details of the model handling and the settings files. The end-user documentation is in the [README](README.md).

___

## How it works

Nemotron Bubble is a Rust app built directly on the Win32 API. There is no Electron and no web view. It ships as one exe with ONNX Runtime linked in, plus `DirectML.dll` for GPU processing. Microphone audio is captured with [`cpal`](https://crates.io/crates/cpal), streamed through the [`parakeet-rs`](https://crates.io/crates/parakeet-rs) Nemotron ONNX model, and injected into the focused window as Unicode keystrokes. The floating bubble is a **per-pixel-alpha layered window** drawn by a small software canvas with signed-distance anti-aliasing, so the pill, shadow, and waveform stay crisp and smooth.

### Source layout

| File | Content |
| --- | --- |
| `src/main.rs` | Startup, `--self-test`, the shared application state |
| `src/settings.rs` | Persisted options, their choice types, loading and saving, file locations |
| `src/history.rs` | Recent dictations and the state of the History list |
| `src/model.rs` | Finding, checking and loading the speech model, GPU probing, languages |
| `src/download.rs` | Model downloads from Hugging Face: update check, resume, SHA-256 verification |
| `src/engine.rs` | The speech engine thread and recording control |
| `src/audio.rs` | Microphone selection, capture and resampling to 16 kHz |
| `src/output.rs` | Clipboard, Ctrl+V and Unicode keystrokes |
| `src/hotkey.rs` | The global shortcut |
| `src/sound.rs` | Start and stop sound cues |
| `src/diagnostics.rs` | Log file, crash logger, message boxes |
| `src/util.rs` | Small Win32 helpers |
| `src/assets.rs` | README art and exe icon export (`assets` feature only) |
| `src/ui/canvas.rs` | Drawing toolkit: canvas, anti-aliased shapes, text, fonts |
| `src/ui/bubble.rs` | The floating bubble: drawing, animation, placement, dragging |
| `src/ui/application_window/mod.rs` | The application window frame: header, tab bar, placement, dispatch to the tabs |
| `src/ui/application_window/history_tab.rs` | The *History* tab |
| `src/ui/application_window/model_tab.rs` | The *Model* tab |
| `src/ui/application_window/model_notice.rs` | The note over the *Model* tab when the selected model is not installed |
| `src/ui/application_window/settings_tab.rs` | The *Settings* tab |
| `src/ui/application_window/widgets.rs` | Controls shared by the tabs: toggles, buttons, segmented controls, scrollbar |
| `src/ui/tray.rs` | Tray icon and tray menu |
| `src/ui/window.rs` | Window class, message loop, message dispatch |

Every module starts with `use crate::*;`, so all items are available in every file.

___

## Build

Building from source needs the [Rust toolchain](https://rustup.rs) (MSVC).

```powershell
git clone https://github.com/LittleGitPhoenix/nemotron-bubble.git
cd nemotron-bubble

# Build the app
cargo build --release

# Run it
cargo run --release

# Regenerate the README art in docs\ (bubble.png, demo.gif and one image per tab of the application window: window-history.png, window-model.png, window-settings.png)
cargo run --release --features assets -- --render-assets docs

# Regenerate the exe icon (embedded by build.rs together with the version info)
cargo run --release --features assets -- --render-icon assets/nemotron-bubble.ico
```

### Tests

The unit tests live next to the code they test, in a `#[cfg(test)] mod tests` block at the end of each file. They cover the pure logic: settings and history files, language and choice values, model manifest checks and backups, hashing, resampling, downmixing, the level meter and the Win32 string helpers. The UI, the hotkey, the capture device and typing into other windows are not covered.

```powershell
cargo test
```

Two model tests feed the recording in `tests/data/audio-test.wav` ("Hello world, what a wonderful day.") through the same downmix, resampling and dictation code the app uses, once with the English and once with the multilingual model, and check the recognized words. They need both models, so they are marked `#[ignore]`. Point `NEMOTRON_TEST_MODELS` at a `models` folder that contains `nemotron` and `nemotron_multi`, for the test run only:

```powershell
$env:NEMOTRON_TEST_MODELS = "$PWD\target\release\models"
cargo test -- --include-ignored
Remove-Item Env:\NEMOTRON_TEST_MODELS
```

The app itself never reads `NEMOTRON_TEST_MODELS`.

When cross-compiling from Linux or WSL with [`cargo-xwin`](https://github.com/rust-cross/cargo-xwin), the test exe is a Windows program. cargo-xwin starts it through Wine by default. Inside WSL it can run directly on Windows instead:

```bash
CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=env cargo xwin test --target x86_64-pc-windows-msvc
```

___

## Model handling

### Model location

| Choice | Loads |
| --- | --- |
| **English** | `models\nemotron` |
| **Multilingual** | `models\nemotron_multi` |
| **Predefined** | the folder in `NEMOTRON_MODEL_DIR`, otherwise `"model_dir"` from `config\settings.json` |

A custom model folder can be set for the current session with `$env:NEMOTRON_MODEL_DIR = "C:\path\to\nemotron"`.

`models\` is searched in the working directory, next to the exe and up to four parent folders. On first start the choice is detected: **Predefined** if one of its folders is set, otherwise **English** if installed, otherwise **Multilingual**. Switching unloads the old model first. With *Preload Nemotron* on, the new one loads right away.

When the selected **English** or **Multilingual** model is not installed, the app shows no error. At startup, and when loading fails for that reason, it opens the application window on the *Model* tab with a note over it (`ModelNotice` in `src/model.rs`, drawn by `src/ui/application_window/model_notice.rs`). The tab is dimmed, the controls that help are ringed and marked with badges, and a callout below the downloads explains them. Only its ✕ closes it. It also disappears when a download starts or an installed model is selected. When the selected model is missing, the first model whose download finishes becomes the selected one. A **Predefined** folder without model files still reports an error.

### Downloads, updates and backups

Downloads come from Hugging Face. Each file is written as `<name>.part`, checked against the SHA-256 that Hugging Face publishes and then renamed. An interrupted `.part` is resumed with a `Range` request.

The version and the file hashes are kept in `model-info.json` inside each model folder. It stores the upstream date, the commit and the size, modification time and SHA-256 of each file, so an update check only hashes files that changed. Models downloaded by other means are hashed once on the first check (takes a few seconds).

Before an update replaces files, the installed version is kept as a backup folder next to it, e.g. `models\nemotron_2026-10-22`. Unchanged files are shared with the model folder via hard links (or copied where links fail), so a backup takes the size of the files that changed, usually about 2.5 GB. `model-info.json` is copied because it is rewritten in place. To go back, point *Predefined* at the backup folder or rename the folders. Backups are not deleted automatically.

### Processing measurements

Each entry in the recent dictations (and in `config\history.json`) records the device (`processing`, "CPU" or "GPU") and the average processing time per 560 ms audio chunk (`ms_per_chunk`). `--self-test` prints the same measurement.

___

## Files and settings

### settings.json

`config\settings.json` can be edited by hand. It is read with or without a UTF-8 byte order mark (and as UTF-16 with one), so the encoding your editor saves with does not matter.

### Start with Windows

The *Start with Windows* setting is the only thing the app writes outside its folder. It is the registry entry `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\NemotronBubble`, which is off by default. The app removes it again when you turn that setting off, but only if it points to this copy of the exe, so trying out a second copy never unregisters the first one.

### Diagnostics

The log lines in `log\nemotron-bubble.log` also go to `OutputDebugString`, so [DebugView](https://learn.microsoft.com/sysinternals/downloads/debugview) shows them live (filter on `NemotronBubble`).
