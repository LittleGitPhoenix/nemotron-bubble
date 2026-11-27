---
name: operating
description: Start an isolated test copy of Nemotron Bubble, open its application window, click tabs, buttons and switches, scroll, pick popup-menu entries, take screenshots and read its settings and log. Use whenever a UI change of this app should be seen before and verified after, a reported UI issue reproduced, or a setting, download or menu flow checked in the running app. Never operates the user's own instance.
---

# Operating Nemotron Bubble

The scripts let a model run a test copy of the app, look at it and click through it. Use them for every UI task: take a baseline screenshot first, make the change, then build, run and verify with a second screenshot. README_Technical.md describes how to build. Machine-specific build setups (such as a WSL cross-build) may be described in local, untracked files under `.claude/rules/`, read them when they exist.

All input is posted window messages. The mouse never moves and the scripts never activate a window. The app itself brings its application window and popup menus to the front when they open, which the scripts cannot prevent.

## Workflow

1. **Work folder**: one per chat. Use the scratch folder that the user's instructions name, with the sub-folders they ask for. Without such a rule, or when that drive is missing, use the session's scratch directory. Never a folder inside the repository. The folder holds the test exe, its `config\` and `log\`, and is deleted at the end.
2. **Baseline**: `Start-Nemotron.ps1 -WorkDir <dir> -Build`, then open the screen in question and take a screenshot. View it.
3. **Change** the code.
4. **Verify**: `Start-Nemotron.ps1 -WorkDir <dir> -Build` again (it stops the old test instance first), repeat the steps, take the second screenshot and compare. Also check `Get-State.ps1` for what was saved and logged.
5. **Finish**: `Remove-WorkDir.ps1 -WorkDir <dir>`. Updating the user's own installation is a separate step (see "Running next to the user's instance").

## Running next to the user's instance

The user usually keeps a stable release running. **That is no reason to stop or to ask the user to quit it.** Test copies run in parallel: each instance keeps its settings, history and log next to its own exe, has its own windows and tray icon, and Windows shares the microphone. Never stop a process that `Start-Nemotron.ps1` did not start.

| Shared resource | What happens |
| --- | --- |
| Global shortcut, Enter while dictating | The instance started first owns the key, the test copy logs "already taken". Scripted tests do not use the shortcut. |
| Windows autostart entry | Every start applies the setting, but a copy only removes an entry that points to its own exe. `Start-Nemotron.ps1` restores the entry afterwards as a safety net. |
| Model RAM | About 2.5 GB per loaded model. Test copies start with `preload_model` false. |
| Screen | A second bubble and tray icon appear while the test runs. |

Only overwriting a **running exe** needs it to be quit, because Windows locks it. That concerns the deploy into the folder the user's instance runs from, never a test build into the work folder. If the user's instance runs from the deploy folder (for example `target\wsl-release`), ask the user to quit it before deploying there. Never kill it. A stable release kept in its own folder outside the repository avoids this entirely.

Run the scripts in place from `.claude\skills\operating\scripts`. From Git Bash call them as `powershell.exe -NoProfile -ExecutionPolicy Bypass -File <script> ...`.

```powershell
$s = '.claude\skills\operating\scripts'
$w = '<scratch>\nemotron-bubble\<chat>\app'
& "$s\Start-Nemotron.ps1" -WorkDir $w -Build -Settings '{"model":"multilingual"}'   # prints "PID <n>"
& "$s\Invoke-Target.ps1" -ProcessId <n> -Target Tab:Model                         # opens the window if needed
& "$s\Get-Screenshot.ps1" -ProcessId <n> -Out "$w\..\shots\model.png"
& "$s\Get-State.ps1" -WorkDir $w
& "$s\Remove-WorkDir.ps1" -WorkDir $w
```

## Scripts

| Script | Purpose |
| --- | --- |
| `Start-Nemotron.ps1` | `-WorkDir` (required), `-Build` (build and copy the exe into the work folder, works while the user's app runs: the local `scripts\build-wsl.ps1` when present, otherwise `cargo build --release`), `-Settings '<json>'` (merged into `config\settings.json`), `-UseModels [-ModelsDir <dir>]` (finds already downloaded models by starting in the folder that holds them, by default `target\wsl-release\models` or `models\`. Nothing is linked or copied.), `-NoStart`. Stops a previous instance of the same folder, waits for the bubble window, prints `PID <n>`. |
| `Open-Window.ps1` | Opens the application window via the tray-click message (works with the bubble hidden), `-Close` closes it, `-Bubble` clicks the bubble instead. Idempotent. |
| `Invoke-Target.ps1` | Clicks a named element (`-List` prints all). Opens the window, switches the tab and scrolls the Settings tab as needed. `-X -Y [-Tab]` clicks raw window pixels. |
| `Send-Wheel.ps1` | Scrolls the active tab, `-Notches` positive is down. One notch is 60 px. |
| `Select-MenuItem.ps1` | Picks an entry of the open popup menu: `-Index n` (Down n times, then Enter), `-Cancel`. `-Tray` opens the tray menu first, `-Right -SubIndex m` goes into a submenu. |
| `Get-Screenshot.ps1` | Application window via `PrintWindow` (no focus needed, no popups). `-Screen` adds open popup menus (screen capture). `-Window Bubble`, `-Delay <seconds>` (e.g. `0.5`, at most 60), `-Crop x,y,w,h`, `-Zoom n`. |
| `Get-State.ps1` | `config\settings.json`, the end of the log (`-Log n`, `-Grep`), `-WaitFor <regex> -Timeout s` waits for a new log line, `-Models` lists the model folders with version. |
| `Stop-Nemotron.ps1` | Stops the test instance of a work folder. |
| `Remove-WorkDir.ps1` | Stops the instance and deletes the work folder. It refuses when a link is inside, so linked data can never be deleted with it. |
| `Crop-Image.ps1` | Crops and enlarges any image, for example a screenshot of the user (`-Info` prints the size). |
| `Targets.ps1` | The table of named targets (no script to run). |

## Named targets

`Invoke-Target.ps1 -List` prints them with tab and coordinates. The groups:

| Tab | Targets |
| --- | --- |
| all | `Close`, `Tab:History`, `Tab:Model`, `Tab:Settings` |
| History | `Trash` (clears the history without asking), `Copy:First` |
| Model | `Processing:CPU/GPU/Auto`, `Model:English/Multilingual/Predefined`, `CheckForUpdates`, `Download:English/Multilingual` (whatever the row's button says: Download, Update, Resume, Retry, Cancel), `Language` (menu), `Toggle:Preload`, `Notice:Close` (the ✕ of the model notice. While the notice shows, clicks on its callout do nothing, so `Language` and `Toggle:Preload` below it are blocked.) |
| Settings | `Shortcut`, `Microphone` (menu), `AutoStop` (menu), `BubblePosition:TopLeft/TopCenter/TopRight/BottomLeft/BottomCenter/BottomRight` (the spots of the screen picker), `Toggle:StartWithWindows`, `Toggle:LiveType`, `Toggle:CopyFinal`, `Toggle:PasteFinal`, `Toggle:Sounds`, `Toggle:Waveform`, `Toggle:TrayAnimation`, `Toggle:EnterStops`, `Toggle:FloatingBubble`, `Toggle:BubbleClickOpensWindow` |

The coordinates mirror the layout constants in `src/ui/application_window/*.rs`. **When a change moves, adds or removes an element, update `Targets.ps1` in the same change** and check the affected targets with a click and a screenshot.

## Facts about the app

- There are no UI Automation elements. Everything is custom-drawn into two windows of class `NemotronBubbleWindow`: the bubble (title "Nemotron Floating Bubble", it also receives the tray icon's messages) and the application window ("Nemotron Bubble", 392 x 816, no frame, so window and client pixels are the same). The scripts always find them by process id, because the user's own instance has the same class and titles.
- A button-up without a preceding drag counts as a click. Popup menus (`TrackPopupMenu`) block the app until closed and only react to keys.
- The application window opens next to the bubble after a bubble click and next to the mouse cursor after a tray click.
- The app applies `start_with_windows` at every start: true points the Windows autostart entry (`HKCU\...\Run\NemotronBubble`) at the running exe, false removes the entry only if it points to this exe. The default is false. `Start-Nemotron.ps1` forces it to false and afterwards restores the entry to what it was before the start, so a test never changes the user's autostart. If it reports a restore, say so.
- `settings.json` is read as UTF-8 with or without BOM and as UTF-16 with BOM. The app always writes UTF-8 without BOM. A file that is not valid JSON makes the app log "settings.json is invalid", keep it as `.invalid` and run with its defaults (preload on). `Start-Nemotron.ps1` stops the instance if that happens.
- A model that fails to load shows a message box on the user's screen. Keep `preload_model` false unless the test needs a loaded model and the models are linked.
- Without `-UseModels` the test copy has no models. That is the right state for testing downloads (2.5 GB per model, so check the connection speed first). With `-UseModels` the models are the user's: never download, update or edit them in such a test, because an update writes where the model was found.
- `preload_model` defaults to false in test copies, so no 2.5 GB load happens unless a test needs it. Load time is about 2 s on the CPU. Wait with `Get-State.ps1 -WaitFor 'Nemotron loaded'`.
- The app is not DPI-aware. Click coordinates are always the layout constants. On a scaled display, screenshot pixels can differ from them.
- The Settings tab scrolls by up to 90 px (two notches). `Invoke-Target.ps1` scrolls to the top or bottom before clicking a Settings-tab target. Clicks outside the card are ignored.
- **Cannot be tested from a script**: the global hotkey, real dictation, microphone input, typing into other apps, hover effects. Say so and ask the user to try them.
- `--self-test` needs no UI: `Start-Nemotron.ps1 -NoStart -UseModels`, then run `<dir>\nemotron-bubble.exe --self-test` with the working directory it prints.

## Recipes

**Pick a popup-menu entry** (Language, Microphone, Auto-stop, tray menu):

```powershell
& "$s\Invoke-Target.ps1" -ProcessId <n> -Target Microphone
& "$s\Get-Screenshot.ps1" -ProcessId <n> -Screen -Out <shots>\menu.png   # count the entries
& "$s\Select-MenuItem.ps1" -ProcessId <n> -Index 3                       # 1 = first entry
```

**Tray menu**: `Select-MenuItem.ps1 -ProcessId <n> -Tray` opens it. Pick with `-Index`, or open a submenu with `-Index k -Right -SubIndex m`.

**Download test**: start without `-UseModels`, `Invoke-Target -Target CheckForUpdates`, then `Download:English`. Screenshot during the download, `Get-State -WaitFor 'model downloaded' -Timeout 120 -Models` afterwards.

**Toggle a switch and check it**: `Invoke-Target -Target Toggle:Sounds`, then `Get-State` shows the new value in `settings.json`.
