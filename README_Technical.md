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
| `src/update.rs` | App updates from GitHub: update check, download, verification, installation and restart |
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
| `src/ui/application_window/update_panel.rs` | The update panel over the card and the version pill in the footer |
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

# Regenerate the README art in docs\ (bubble.png, demo.gif, one image per tab of the application window: window-history.png, window-model.png, window-settings.png, and the update panel: window-update.png)
cargo run --release --features assets -- --render-assets docs

# Regenerate the exe icon (embedded by build.rs together with the version info)
cargo run --release --features assets -- --render-icon assets/nemotron-bubble.ico
```

### Tests

The unit tests live next to the code they test, in a `#[cfg(test)] mod tests` block at the end of each file. They cover the pure logic: settings and history files, language and choice values, model manifest checks and backups, hashing, downloading with resume and verification (against a local HTTP server), the app update manifest, unpacking and installing an update (including hostile archives and locked files), resampling, downmixing, the level meter and the Win32 string helpers. The UI, the hotkey, the capture device and typing into other windows are not covered.

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

## App updates

### Update check

The app looks for a new version in `updater.json`, the asset of the GitHub release `nemotron-bubble-Updater` (the layout of the Mandos updater). It is read on *Check now* on the *Settings* tab and, with *Check for updates automatically* on, 15 s after the start and then every hour. The setting is off by default, so the app only goes online when asked.

```json
{
  "generated": "2026-12-05T10:00:00Z",
  "projects": {
    "nemotron-bubble": {
      "version": "1.2.0",
      "release": "nemotron-bubble.1.2.0",
      "date": "2026-12-05",
      "changelog": "## 1.2.0 ...",
      "assets": { "win-x64": "https://github.com/LittleGitPhoenix/nemotron-bubble/releases/download/nemotron-bubble.1.2.0/nemotron-bubble.1.2.0.win-x64.tgz" },
      "sha256": { "win-x64": "<SHA-256 of the archive>" },
      "size": { "win-x64": 18886659 }
    }
  }
}
```

| Rule | Detail |
| --- | --- |
| Offered versions | Only a stable version newer than the running one. Pre-release versions are never offered, but someone running `1.2.0-beta.1` is offered `1.2.0`. |
| Download address | Must be a release asset of this repository. Anything else is refused. |
| Skipped version | *Skip this version* saves it as `skipped_update` in `config\settings.json`. Badges appear again only for a newer version. |
| Release notes | The `changelog` is not shown in the app. *Release notes on GitHub* opens the release page, where GitHub renders them. |

### Installation

*Update now* is refused while dictating or downloading a model. The archive is downloaded into `update\` next to the exe as `<name>.part`, resumed after an interruption, and checked against `size` and `sha256` (the download code of the models). It is unpacked into `update\<version>\`. It may only hold plain files, no folders.

Windows does not allow overwriting a running exe, but it allows renaming it. Every installed file the archive replaces is renamed to `<name>.old` (`<name>.old-2` and up while an older one is still locked), and the new file is moved in. A failure puts everything back. The exe keeps its current name. The app then quits and starts the new exe only after its window is gone, so the new instance can take over the shortcut and the tray icon. At startup the app deletes `*.old` files and everything in `update\` except a partial download of a newer version.

### Release workflow

`.github/workflows/release.yml` creates the release of a new version from `main`. For a stable version it also writes the version, release tag, CHANGELOG date and entry, download address, SHA-256 and size of the archive into `updater.json`. It downloads the current file, replaces the entry of this app and uploads it again. On the first stable release it creates `nemotron-bubble-Updater` as a pre-release, so it never becomes the "Latest" release. A version with a pre-release suffix becomes a GitHub pre-release and leaves `updater.json` unchanged. `tests/data/updater.json` is a file written by that step, and a unit test checks that the app understands it and that `release.yml` still uses the names the app reads. Regenerate the file when the step changes what it writes.

### Testing updates

`NEMOTRON_UPDATE_MANIFEST` makes the app read `updater.json` from a local file instead of GitHub, for example to offer a published release under a higher version number. The download address must still be a release of this repository, and the archive is still checked against its SHA-256. Set the variable only for the process under test.

```powershell
$env:NEMOTRON_UPDATE_MANIFEST = 'C:\temp\updater-test.json'
Start-Process .\nemotron-bubble.exe
Remove-Item Env:\NEMOTRON_UPDATE_MANIFEST
```

___

## Files and settings

### settings.json

`config\settings.json` can be edited by hand. It is read with or without a UTF-8 byte order mark (and as UTF-16 with one), so the encoding your editor saves with does not matter.

### Start with Windows

The *Start with Windows* setting is the only thing the app writes outside its folder. It is the registry entry `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\NemotronBubble`, which is off by default. The app removes it again when you turn that setting off, but only if it points to this copy of the exe, so trying out a second copy never unregisters the first one.

### Diagnostics

The log lines in `log\nemotron-bubble.log` also go to `OutputDebugString`, so [DebugView](https://learn.microsoft.com/sysinternals/downloads/debugview) shows them live (filter on `NemotronBubble`).
