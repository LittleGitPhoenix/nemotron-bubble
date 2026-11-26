# Changelog

All notable changes to **Nemotron Bubble** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
___

## 0.2.0

📅 _2026-??-??_
✏️ _LittlePhoenix_

### Added

- Choice of the speech model (*English*, *Multilingual*, *Predefined*) in the settings window and the tray menu. On first start the installed model is selected automatically.
- Language selection for the multilingual model. Besides *Auto-detect*, the 15 languages NVIDIA rates as transcription-ready are offered. A change applies instantly without reloading the model.
- GPU processing via DirectML, which works with any DirectX 12 graphics adapter. The new *Processing* setting offers *CPU* (default), *GPU* and *Auto*.
- Each history entry records the device and the average processing time per 560 ms audio chunk.
- Six preset positions for the floating bubble (top or bottom, each left, center or right). The choice is saved in `settings.json`.
- Microphone selection on the *Settings* tab. *Windows default* follows the Windows default input device, and a change there is picked up at the next dictation start without restarting the app. A selected device that is not connected falls back to the Windows default until it is back.
- Model downloads on the *Model* tab, replacing the PowerShell download script. Downloads can be cancelled and resumed, and every file is verified with the SHA-256 published by Hugging Face. Each model shows the date of its upstream version, and *Check for updates* compares the installed files with Hugging Face on request. An update only downloads the files that changed and keeps the previous version in a backup folder such as `models\nemotron_2026-10-22`.
- A log file `log\nemotron-bubble.log` with startup, microphone, model loading and all errors. The same lines go to `OutputDebugString` for DebugView.
- A missing microphone is shown by the bubble (amber dot and a crossed-out microphone) and the tray icon (crossed out), and both give the reason in their tooltip. There is no message box at startup. The app checks every 3 seconds whether a microphone is back and picks it up on its own. Starting dictation without a microphone still shows a message.
- A message for a model that fails to load.
- The new `--self-test` parameter prints to the console and reports the device and the processing time per chunk.
- The version is shown in the settings window, the tray menu and the file properties of the executable.
- An icon for the executable.

### Changed

- The main window is redesigned. It is about half as wide, splits into the tabs *History*, *Model* and *Settings*, and opens next to the bubble or the tray icon, depending on what was clicked. The close button is the Windows ✕.
- The history list is drawn by the app itself. Each entry has a copy icon and a smaller grey header line, and the list scrolls with the mouse wheel. A trash button replaces *Clear*, and *Copy Latest* is gone.
- The *Settings* tab scrolls with the mouse wheel and shows a scrollbar, so the window keeps its height.
- The app is portable. Settings, history and sounds are stored in `config\` and the logs in `log\` next to the exe instead of in `%APPDATA%`.
- The bubble keeps a 10 pixel margin to the screen edges and the taskbar and its shadow is smaller so it no longer overlaps the taskbar.
- The Space key is labelled *Space* in the shortcut instead of its localized Windows name.
- The source code is split from one 5,300-line `main.rs` into modules under `src/` and `src/ui/`. The behavior is unchanged.
- *Start with Windows* is off by default, because a portable app that is only tried out should not register itself. Turning it off only removes the autostart entry if it points to this copy of the exe, so another copy in a different folder keeps its entry.

### Fixed

- The multilingual model crashed while loading, which left the app without speech recognition.
- With both models installed, the English model was always loaded and the multilingual one could not be chosen.
- The app quit silently when no microphone was available. It now shows a warning and retries when dictation starts.
- A `settings.json` that an editor saved with a byte order mark (UTF-8 or UTF-16) was rejected, so the app fell back to the default settings. Settings, history and `model-info.json` are now read with or without one.

### Removed

- `scripts/download-nemotron.ps1`. Models are downloaded on the *Model* tab.

### References

⚪ embed-resource **3.0.12**  
🔵 parakeet-rs ~~0.3.6~~ → **0.4.4**  
⚪ sha2 **0.10.9**  
⚪ ureq **3.3.0**  
___

## 0.1.0

📅 _2026-06-27_
✏️ _snipemanmike_

Initial state of this fork, cloned from [snipemanmike/nemotron-bubble](https://github.com/snipemanmike/nemotron-bubble) at commit [`6ca5b21`](https://github.com/snipemanmike/nemotron-bubble/commit/6ca5b21) (*Make silence auto-stop an optional setting*).
