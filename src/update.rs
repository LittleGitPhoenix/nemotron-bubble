//! App updates from GitHub: the update check, the download with SHA-256
//! verification and the installation.
//!
//! The release workflow keeps a GitHub release named `nemotron-bubble-Updater`
//! whose asset `updater.json` describes the newest stable version (the layout of
//! the Mandos updater):
//!
//! ```json
//! { "generated": "...", "projects": { "nemotron-bubble": {
//!     "version": "1.2.0", "release": "nemotron-bubble.1.2.0", "date": "2026-12-05",
//!     "changelog": "...", "assets": { "win-x64": "https://github.com/..." },
//!     "sha256": { "win-x64": "..." }, "size": { "win-x64": 12345678 } } } }
//! ```
//!
//! An update is downloaded into `<exe dir>\update`, verified and unpacked there.
//! Windows refuses to overwrite a running exe but allows renaming it, so the
//! installed files are renamed to `<name>.old` and the new ones moved in. The app
//! then quits and starts the new exe once the old one has released the shortcut,
//! the tray icon and the microphone. The new instance deletes the `.old` files.

use crate::*;
use semver::Version;
use std::collections::HashMap;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// The GitHub repository that publishes the releases.
const REPOSITORY: &str = "LittleGitPhoenix/nemotron-bubble";
/// The release whose asset `updater.json` describes the newest version.
const UPDATER_TAG: &str = "nemotron-bubble-Updater";
const UPDATER_ASSET: &str = "updater.json";
/// Key of this app in the `projects` of updater.json.
const PROJECT_KEY: &str = "nemotron-bubble";
/// Key of the Windows x64 archive in the `assets`, `sha256` and `size` maps.
const RUNTIME_ID: &str = "win-x64";
/// The name of the exe inside a release archive.
const ARCHIVE_EXE: &str = "nemotron-bubble.exe";
/// Folder next to the exe that holds downloads and the unpacked update.
const UPDATE_FOLDER: &str = "update";
/// Suffix of installed files that an update replaced.
const OLD_SUFFIX: &str = ".old";
/// Automatic checks: how often, and how often the timer asks whether one is due.
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
pub(crate) const UPDATE_TIMER_ID: usize = 80;
pub(crate) const UPDATE_TIMER_MS: u32 = 15_000;
// How often a running download repaints its progress.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// The exe the app starts after it has quit for an installed update.
static RESTART_EXE: OnceCell<PathBuf> = OnceCell::new();

/// The newest stable version on GitHub, when it is newer than this one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AvailableUpdate {
    pub(crate) version: Version,
    /// Tag of its GitHub release, e.g. "nemotron-bubble.1.2.0".
    pub(crate) release: String,
    /// Release date from the CHANGELOG, e.g. "2026-12-05".
    pub(crate) date: Option<String>,
    pub(crate) url: String,
    pub(crate) sha256: String,
    pub(crate) size: u64,
}

impl AvailableUpdate {
    /// The page of the release on GitHub, where the release notes are rendered.
    pub(crate) fn release_page(&self) -> String {
        format!("https://github.com/{REPOSITORY}/releases/tag/{}", self.release)
    }
}

/// What the updater is doing right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum UpdateActivity {
    #[default]
    Idle,
    Checking,
    Downloading {
        done: u64,
        total: u64,
    },
    Cancelling,
    Installing,
}

/// Everything the UI shows about app updates.
#[derive(Clone, Debug, Default)]
pub(crate) struct UpdateState {
    pub(crate) activity: UpdateActivity,
    /// The result of the last successful check, None while up to date or unchecked.
    pub(crate) available: Option<AvailableUpdate>,
    /// Local time of the last successful check, e.g. "14:02".
    pub(crate) checked_at: Option<String>,
    /// When the last check started (successful or not), for the hourly schedule.
    pub(crate) last_attempt: Option<Instant>,
    pub(crate) check_error: Option<String>,
    pub(crate) install_error: Option<String>,
    /// The update panel is open over the application window.
    pub(crate) panel_open: bool,
}

/// The update state together with the user's choice to skip the available version.
#[derive(Clone, Debug, Default)]
pub(crate) struct UpdateView {
    pub(crate) state: UpdateState,
    /// The available version was skipped (or an even newer one).
    pub(crate) skipped: bool,
}

impl UpdateView {
    /// The available update unless it was skipped: what the badges announce.
    pub(crate) fn pending(&self) -> Option<&AvailableUpdate> {
        self.state.available.as_ref().filter(|_| !self.skipped)
    }
}

// --------------------------------------------------------------------------
// updater.json
// --------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UpdaterManifest {
    projects: HashMap<String, ProjectUpdateInfo>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ProjectUpdateInfo {
    version: String,
    release: String,
    date: String,
    assets: HashMap<String, String>,
    sha256: HashMap<String, String>,
    size: HashMap<String, u64>,
}

/// The version of this build.
fn current_version() -> Version {
    Version::parse(APP_VERSION).expect("Cargo.toml holds a SemVer version")
}

/// True for a download URL of a release asset of this repository.
fn is_release_asset_url(url: &str) -> bool {
    let prefix = format!("https://github.com/{REPOSITORY}/releases/download/");
    url.strip_prefix(&prefix).is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/'))
            && !rest.contains("..")
    })
}

/// True for a release tag that may be put into a URL as it is.
fn is_plain_tag(tag: &str) -> bool {
    !tag.is_empty() && tag.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// The update offered by `json` (the contents of updater.json) for a user on
/// `current`. Ok(None): up to date. Pre-release versions are never offered.
fn update_from_manifest(json: &str, current: &Version) -> Result<Option<AvailableUpdate>> {
    let manifest: UpdaterManifest = serde_json::from_str(json).context("updater.json is not valid")?;
    let info = manifest
        .projects
        .get(PROJECT_KEY)
        .ok_or_else(|| anyhow!("updater.json has no entry for {PROJECT_KEY}"))?;
    let version = Version::parse(info.version.trim())
        .with_context(|| format!("updater.json names an invalid version '{}'", info.version))?;
    if !version.pre.is_empty() || version <= *current {
        return Ok(None);
    }
    let url = info
        .assets
        .get(RUNTIME_ID)
        .ok_or_else(|| anyhow!("version {version} has no download for {RUNTIME_ID}"))?;
    if !is_release_asset_url(url) {
        bail!("the download address of version {version} is not a release of {REPOSITORY}");
    }
    let sha256 = info.sha256.get(RUNTIME_ID).map(|hash| hash.trim().to_ascii_lowercase()).unwrap_or_default();
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("version {version} has no valid SHA-256 for {RUNTIME_ID}");
    }
    let size = info.size.get(RUNTIME_ID).copied().unwrap_or(0);
    if size == 0 {
        bail!("version {version} has no size for {RUNTIME_ID}");
    }
    if !is_plain_tag(&info.release) {
        bail!("version {version} names an invalid release '{}'", info.release);
    }
    let date = Some(info.date.trim().to_string()).filter(|date| !date.is_empty());
    Ok(Some(AvailableUpdate {
        version,
        release: info.release.clone(),
        date,
        url: url.clone(),
        sha256,
        size,
    }))
}

/// True when `skipped` (the saved setting) covers `version`: the user skipped
/// it or a newer one.
fn is_skipped(version: &Version, skipped: &str) -> bool {
    Version::parse(skipped.trim()).is_ok_and(|skipped| skipped >= *version)
}

/// For tests: a local file read instead of updater.json from GitHub. The
/// download address in it must still be a release of this repository, and the
/// archive is still verified with its SHA-256.
const MANIFEST_OVERRIDE_VARIABLE: &str = "NEMOTRON_UPDATE_MANIFEST";

fn fetch_update() -> Result<Option<AvailableUpdate>> {
    if let Some(path) = env::var_os(MANIFEST_OVERRIDE_VARIABLE).filter(|path| !path.is_empty()) {
        let path = PathBuf::from(path);
        log_info(&format!("Update check reads {} ({MANIFEST_OVERRIDE_VARIABLE})", path.display()));
        let json = read_text_file(&path).with_context(|| format!("could not read {}", path.display()))?;
        return update_from_manifest(&json, &current_version());
    }
    let url = format!("https://github.com/{REPOSITORY}/releases/download/{UPDATER_TAG}/{UPDATER_ASSET}");
    let json = http_agent()
        .get(&url)
        .call()
        .with_context(|| format!("could not reach GitHub ({url})"))?
        .body_mut()
        .read_to_string()
        .context("could not read updater.json from GitHub")?;
    update_from_manifest(&json, &current_version())
}

// --------------------------------------------------------------------------
// State helpers
// --------------------------------------------------------------------------

fn with_update<R>(app: &Arc<AppState>, change: impl FnOnce(&mut UpdateState) -> R) -> R {
    let mut state = app.update.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    change(&mut state)
}

pub(crate) fn update_view(app: &Arc<AppState>) -> UpdateView {
    let state = with_update(app, |state| state.clone());
    let skipped_setting = settings_snapshot(app).skipped_update;
    let skipped = state
        .available
        .as_ref()
        .is_some_and(|update| is_skipped(&update.version, &skipped_setting));
    UpdateView { state, skipped }
}

/// The version the badges announce, None when there is nothing to announce.
pub(crate) fn pending_update_version(app: &Arc<AppState>) -> Option<String> {
    update_view(app).pending().map(|update| update.version.to_string())
}

/// Repaint the bubble (badge, tooltips, tray) and the application window.
fn repaint_all(app: &Arc<AppState>) {
    update_ui(app, |_| {});
}

fn set_status(app: &Arc<AppState>, status: String) {
    update_ui(app, |ui| ui.status = status);
}

// --------------------------------------------------------------------------
// Checking
// --------------------------------------------------------------------------

/// Look for a new version in the background. `manual` checks (Check now) report
/// their result in the status line, automatic ones only a newly found version.
pub(crate) fn check_for_app_update(app: &Arc<AppState>, manual: bool) {
    let started = with_update(app, |state| {
        if state.activity != UpdateActivity::Idle {
            return false;
        }
        state.activity = UpdateActivity::Checking;
        state.last_attempt = Some(Instant::now());
        state.check_error = None;
        true
    });
    if !started {
        return;
    }
    if manual {
        set_status(app, "Checking GitHub for a new version...".to_string());
    } else {
        repaint_all(app);
    }

    let app = app.clone();
    thread::spawn(move || {
        let announced_before = pending_update_version(&app);
        let result = fetch_update();
        with_update(&app, |state| {
            state.activity = UpdateActivity::Idle;
            match &result {
                Ok(update) => {
                    state.available = update.clone();
                    state.checked_at = Some(Local::now().format("%H:%M").to_string());
                }
                Err(err) => state.check_error = Some(format!("{err:#}")),
            }
        });
        match result {
            Ok(Some(update)) => {
                log_info(&format!("Update check: version {} is available ({})", update.version, update.release));
                let announced = pending_update_version(&app);
                if manual || (announced.is_some() && announced != announced_before) {
                    let status = if announced.is_some() {
                        format!("Nemotron Bubble {} is available.", update.version)
                    } else {
                        format!("Version {} is available but skipped.", update.version)
                    };
                    set_status(&app, status);
                } else {
                    repaint_all(&app);
                }
            }
            Ok(None) => {
                log_info(&format!("Update check: {APP_VERSION} is up to date"));
                if manual {
                    set_status(&app, "Nemotron Bubble is up to date.".to_string());
                } else {
                    repaint_all(&app);
                }
            }
            Err(err) => {
                log_error(&format!("Update check failed: {err:#}"));
                if manual {
                    set_status(&app, "The update check failed.".to_string());
                } else {
                    repaint_all(&app);
                }
            }
        }
    });
}

/// Timer tick (UI thread): start an automatic check when the setting is on and
/// the last check is an hour ago, or none ran since the start.
pub(crate) fn update_timer_tick(app: &Arc<AppState>) {
    if !settings_snapshot(app).check_updates_automatically {
        return;
    }
    let due = with_update(app, |state| {
        state.activity == UpdateActivity::Idle
            && state.last_attempt.is_none_or(|last| last.elapsed() >= CHECK_INTERVAL)
    });
    if due {
        check_for_app_update(app, false);
    }
}

// --------------------------------------------------------------------------
// The update panel
// --------------------------------------------------------------------------

/// Open the application window with the update panel (bubble badge, footer, tray).
pub(crate) fn open_update_panel(app: &Arc<AppState>, origin: WindowOrigin) {
    if with_update(app, |state| state.available.is_none()) {
        return;
    }
    with_update(app, |state| state.panel_open = true);
    show_application_window(origin);
    repaint_all(app);
}

pub(crate) fn close_update_panel(app: &Arc<AppState>) {
    with_update(app, |state| {
        state.panel_open = false;
        // An error is shown once. The next attempt starts clean.
        if state.activity == UpdateActivity::Idle {
            state.install_error = None;
        }
    });
    repaint_all(app);
}

/// "Skip this version": no badges until an even newer version appears.
pub(crate) fn skip_update(app: &Arc<AppState>) {
    let Some(update) = with_update(app, |state| state.available.clone()) else {
        return;
    };
    let settings = {
        let mut settings = app.settings.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        settings.skipped_update = update.version.to_string();
        settings.clone()
    };
    save_settings(&settings);
    log_info(&format!("Version {} skipped", update.version));
    with_update(app, |state| {
        state.panel_open = false;
        state.install_error = None;
    });
    set_status(app, format!("Version {} skipped. A newer version is offered again.", update.version));
}

/// Open the release notes of the available update in the browser.
pub(crate) fn open_release_notes(app: &Arc<AppState>) {
    let Some(update) = with_update(app, |state| state.available.clone()) else {
        return;
    };
    let page = update.release_page();
    let operation = wide_null("open");
    let file = wide_null(&page);
    let result = unsafe { ShellExecuteW(null_mut(), operation.as_ptr(), file.as_ptr(), null(), null(), SW_SHOWNORMAL) };
    // ShellExecuteW reports success with a value greater than 32.
    if (result as isize) <= 32 {
        log_error(&format!("Could not open {page} (error {})", result as isize));
        set_status(app, "The browser could not be opened.".to_string());
    }
}

// --------------------------------------------------------------------------
// Downloading and installing
// --------------------------------------------------------------------------

/// "Update now": download, verify and install the available update in the
/// background, then restart. Refused while dictating or downloading a model.
pub(crate) fn start_app_update(app: &Arc<AppState>) {
    if app.recording.load(Ordering::SeqCst) {
        set_status(app, "Stop the dictation before updating.".to_string());
        return;
    }
    if downloads_snapshot(app).iter().any(ModelDownloadState::busy) {
        set_status(app, "Wait until the model download has finished.".to_string());
        return;
    }
    let update = with_update(app, |state| {
        if state.activity != UpdateActivity::Idle {
            return None;
        }
        let update = state.available.clone()?;
        state.activity = UpdateActivity::Downloading {
            done: 0,
            total: update.size,
        };
        state.install_error = None;
        Some(update)
    });
    let Some(update) = update else {
        return;
    };
    app.update_cancel.store(false, Ordering::SeqCst);
    set_status(app, format!("Downloading version {}...", update.version));
    log_info(&format!("Updating to {} from {}", update.version, update.url));

    let app = app.clone();
    thread::spawn(move || {
        let result = download_and_install(&app, &update);
        with_update(&app, |state| state.activity = UpdateActivity::Idle);
        match result {
            Ok(Some(exe)) => {
                log_info(&format!("Version {} installed, restarting", update.version));
                let _ = RESTART_EXE.set(exe);
                set_status(&app, "Restarting...".to_string());
                // Quit on the UI thread. The new exe is started once the window is gone.
                unsafe {
                    let hwnd = hwnd_from_app(&app);
                    if hwnd != null_mut() {
                        PostMessageW(hwnd, WM_COMMAND, IDM_QUIT, 0);
                    }
                }
            }
            Ok(None) => {
                log_info("Update cancelled");
                set_status(&app, "Update cancelled. It resumes the next time.".to_string());
            }
            Err(err) => {
                log_error(&format!("Update to {} failed: {err:#}", update.version));
                with_update(&app, |state| state.install_error = Some(format!("{err:#}")));
                set_status(&app, "The update failed.".to_string());
            }
        }
    });
}

/// Ask a running update download to stop. The partial file is kept for resuming.
pub(crate) fn cancel_app_update(app: &Arc<AppState>) {
    let cancelled = with_update(app, |state| {
        if matches!(state.activity, UpdateActivity::Downloading { .. }) {
            state.activity = UpdateActivity::Cancelling;
            true
        } else {
            false
        }
    });
    if cancelled {
        app.update_cancel.store(true, Ordering::SeqCst);
        repaint_all(app);
    }
}

/// File name of the downloaded archive of `version`.
fn archive_name(version: &Version) -> String {
    format!("nemotron-bubble.{version}.{RUNTIME_ID}.tgz")
}

/// Returns the exe to start after quitting, or None when cancelled.
fn download_and_install(app: &Arc<AppState>, update: &AvailableUpdate) -> Result<Option<PathBuf>> {
    let root = app_root_dir();
    let folder = root.join(UPDATE_FOLDER);
    fs::create_dir_all(&folder).with_context(|| format!("could not create {}", folder.display()))?;
    let archive = folder.join(archive_name(&update.version));
    let part = folder.join(format!("{}.part", archive_name(&update.version)));

    let mut done = 0u64;
    let mut last_report = Instant::now();
    let completed = download_verified(
        &update.url,
        &part,
        update.size,
        &update.sha256,
        &archive_name(&update.version),
        &|| app.update_cancel.load(Ordering::SeqCst),
        &mut |bytes| {
            done = done.saturating_add_signed(bytes);
            if last_report.elapsed() >= PROGRESS_INTERVAL {
                last_report = Instant::now();
                let reported = with_update(app, |state| {
                    if matches!(state.activity, UpdateActivity::Downloading { .. }) {
                        state.activity = UpdateActivity::Downloading {
                            done,
                            total: update.size,
                        };
                        true
                    } else {
                        false // keep showing "Cancelling..."
                    }
                });
                if reported {
                    repaint_all(app);
                }
            }
        },
    )?;
    if !completed {
        return Ok(None);
    }
    replace_file(&part, &archive)?;

    with_update(app, |state| state.activity = UpdateActivity::Installing);
    set_status(app, format!("Installing version {}...", update.version));
    let staging = folder.join(update.version.to_string());
    if staging.exists() {
        fs::remove_dir_all(&staging).with_context(|| format!("could not clear {}", staging.display()))?;
    }
    fs::create_dir_all(&staging).with_context(|| format!("could not create {}", staging.display()))?;
    let files = extract_archive(&archive, &staging)?;

    // A dictation started during the download ends before the app restarts.
    while app.recording.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(500));
    }
    let exe_name = env::current_exe()
        .ok()
        .and_then(|exe| exe.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| ARCHIVE_EXE.to_string());
    let exe = install_files(&staging, &root, &files, &exe_name)?;

    let _ = fs::remove_file(&archive);
    let _ = fs::remove_dir_all(&staging);
    Ok(Some(exe))
}

/// Unpack the release archive into `staging` and return the names of its files.
/// The archive holds plain files only, no folders. Anything else is refused,
/// so no entry can land outside `staging`.
fn extract_archive(archive: &Path, staging: &Path) -> Result<Vec<String>> {
    let file = fs::File::open(archive).with_context(|| format!("could not open {}", archive.display()))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut names = Vec::new();
    for entry in tar.entries().context("the update archive cannot be read")? {
        let mut entry = entry.context("the update archive is damaged")?;
        let path = entry.path().context("the update archive holds an invalid name")?.into_owned();
        let parts: Vec<_> = path
            .components()
            .filter(|part| !matches!(part, std::path::Component::CurDir))
            .collect();
        if parts.is_empty() {
            continue; // the archive's root folder "./"
        }
        let kind = entry.header().entry_type();
        let name = match parts.as_slice() {
            [std::path::Component::Normal(name)] if kind.is_file() => name.to_string_lossy().into_owned(),
            _ => bail!("the update archive holds an unexpected entry: {}", path.display()),
        };
        let target = staging.join(&name);
        entry
            .unpack(&target)
            .with_context(|| format!("could not unpack {name}"))?;
        names.push(name);
    }
    if !names.iter().any(|name| name.eq_ignore_ascii_case(ARCHIVE_EXE)) {
        bail!("the update archive holds no {ARCHIVE_EXE}");
    }
    Ok(names)
}

/// A free name for the replaced file `target`: `<name>.old`, or `<name>.old-2`
/// and up when an older one is still locked (e.g. by an exe that is still exiting).
fn old_path(target: &Path) -> PathBuf {
    let name = target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let mut counter = 1;
    loop {
        let candidate = if counter == 1 {
            target.with_file_name(format!("{name}{OLD_SUFFIX}"))
        } else {
            target.with_file_name(format!("{name}{OLD_SUFFIX}-{counter}"))
        };
        if !candidate.exists() || fs::remove_file(&candidate).is_ok() {
            return candidate;
        }
        counter += 1;
    }
}

/// Move the unpacked `files` from `staging` into `root`. Installed files are
/// renamed to `.old` first, which also works for the running exe and its DLL.
/// The archive's exe replaces this exe under its current name (`exe_name`).
/// On an error everything is put back. Returns the path of the new exe.
fn install_files(staging: &Path, root: &Path, files: &[String], exe_name: &str) -> Result<PathBuf> {
    let target_of = |name: &str| {
        if name.eq_ignore_ascii_case(ARCHIVE_EXE) {
            root.join(exe_name)
        } else {
            root.join(name)
        }
    };
    // (installed target, where the file it replaced went)
    let mut installed: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    for name in files {
        let target = target_of(name);
        let result = (|| -> Result<Option<PathBuf>> {
            let old = if target.exists() {
                let old = old_path(&target);
                fs::rename(&target, &old).with_context(|| format!("could not move {} aside", target.display()))?;
                Some(old)
            } else {
                None
            };
            if let Err(err) = fs::rename(staging.join(name), &target) {
                if let Some(old) = &old {
                    let _ = fs::rename(old, &target);
                }
                return Err(anyhow!(err).context(format!("could not install {}", target.display())));
            }
            Ok(old)
        })();
        match result {
            Ok(old) => installed.push((target, old)),
            Err(err) => {
                for (target, old) in installed.iter().rev() {
                    let _ = fs::remove_file(target);
                    if let Some(old) = old {
                        let _ = fs::rename(old, target);
                    }
                }
                return Err(err);
            }
        }
    }
    Ok(target_of(ARCHIVE_EXE))
}

/// After the app has shut down: start the exe of an installed update.
pub(crate) fn restart_after_update() {
    let Some(exe) = RESTART_EXE.get() else {
        return;
    };
    let mut command = std::process::Command::new(exe);
    if let Some(dir) = exe.parent() {
        command.current_dir(dir);
    }
    match command.spawn() {
        Ok(_) => log_info(&format!("Started the updated {}", exe.display())),
        Err(err) => log_error(&format!("Could not start the updated {}: {err}", exe.display())),
    }
}

/// True for a file name an update left behind: `<name>.old` or `<name>.old-<n>`.
fn is_leftover_name(name: &str) -> bool {
    match name.rfind(OLD_SUFFIX) {
        Some(index) if index > 0 => {
            let rest = &name[index + OLD_SUFFIX.len()..];
            rest.is_empty() || rest.strip_prefix('-').is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        }
        _ => false,
    }
}

/// True for a download in the update folder that is still worth resuming: the
/// partial archive of a version newer than `current`.
fn is_resumable_download(name: &str, current: &Version) -> bool {
    name.strip_prefix("nemotron-bubble.")
        .and_then(|rest| rest.strip_suffix(&format!(".{RUNTIME_ID}.tgz.part")))
        .and_then(|version| Version::parse(version).ok())
        .is_some_and(|version| version > *current)
}

/// Remove what an update left behind in `root`: the replaced `.old` files and
/// everything in the update folder except a resumable download.
fn remove_leftovers_in(root: &Path, current: &Version) -> bool {
    let mut complete = true;
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_file() && is_leftover_name(&name) {
                match fs::remove_file(entry.path()) {
                    Ok(()) => log_info(&format!("Removed {name} of the previous version")),
                    Err(_) => complete = false,
                }
            }
        }
    }
    let folder = root.join(UPDATE_FOLDER);
    if let Ok(entries) = fs::read_dir(&folder) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_resumable_download(&name, current) {
                continue;
            }
            let path = entry.path();
            let _ = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
        }
        // Only removed when nothing is left in it.
        let _ = fs::remove_dir(&folder);
    }
    complete
}

/// At startup: remove what an update left behind. The previous exe may still be
/// exiting, so locked files are tried again for a while in the background.
pub(crate) fn remove_update_leftovers() {
    thread::spawn(|| {
        let root = app_root_dir();
        let current = current_version();
        for _ in 0..30 {
            if remove_leftovers_in(&root, &current) {
                return;
            }
            thread::sleep(Duration::from_secs(1));
        }
        log_error("Some files of the previous version could not be removed.");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    const HASH: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn manifest(version: &str) -> String {
        format!(
            r##"{{ "generated": "2026-12-05T10:00:00Z", "projects": {{ "nemotron-bubble": {{
                "version": "{version}", "release": "nemotron-bubble.{version}", "date": "2026-12-05",
                "changelog": "Changes of {version}",
                "assets": {{ "win-x64": "https://github.com/LittleGitPhoenix/nemotron-bubble/releases/download/nemotron-bubble.{version}/nemotron-bubble.{version}.win-x64.tgz" }},
                "sha256": {{ "win-x64": "{HASH}" }},
                "size": {{ "win-x64": 1234 }} }} }} }}"##
        )
    }

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn the_current_version_parses() {
        let _ = current_version();
    }

    #[test]
    fn a_newer_version_is_offered() {
        let update = update_from_manifest(&manifest("1.2.0"), &version("1.1.0")).unwrap().unwrap();
        assert_eq!(update.version, version("1.2.0"));
        assert_eq!(update.release, "nemotron-bubble.1.2.0");
        assert_eq!(update.date.as_deref(), Some("2026-12-05"));
        assert_eq!(update.sha256, HASH);
        assert_eq!(update.size, 1234);
        assert_eq!(
            update.release_page(),
            "https://github.com/LittleGitPhoenix/nemotron-bubble/releases/tag/nemotron-bubble.1.2.0"
        );
    }

    #[test]
    fn the_same_or_an_older_version_is_not_offered() {
        assert!(update_from_manifest(&manifest("1.1.0"), &version("1.1.0")).unwrap().is_none());
        assert!(update_from_manifest(&manifest("1.0.0"), &version("1.1.0")).unwrap().is_none());
    }

    #[test]
    fn a_pre_release_is_never_offered_but_a_pre_release_user_gets_the_stable_version() {
        assert!(update_from_manifest(&manifest("1.3.0-beta.1"), &version("1.1.0")).unwrap().is_none());
        assert!(update_from_manifest(&manifest("1.1.0"), &version("1.1.0-beta.1")).unwrap().is_some());
    }

    #[test]
    fn a_download_outside_the_repository_is_refused() {
        let json = manifest("1.2.0").replace("https://github.com/LittleGitPhoenix", "https://example.com/LittleGitPhoenix");
        assert!(update_from_manifest(&json, &version("1.1.0")).is_err());
        let json = manifest("1.2.0").replace("nemotron-bubble.1.2.0/", "../other/");
        assert!(update_from_manifest(&json, &version("1.1.0")).is_err());
    }

    #[test]
    fn an_entry_without_hash_or_size_is_refused() {
        let json = manifest("1.2.0").replace(HASH, "abc");
        assert!(update_from_manifest(&json, &version("1.1.0")).is_err());
        let json = manifest("1.2.0").replace("\"win-x64\": 1234", "\"win-x64\": 0");
        assert!(update_from_manifest(&json, &version("1.1.0")).is_err());
        let json = manifest("1.2.0").replace("nemotron-bubble\": {", "other\": {");
        assert!(update_from_manifest(&json, &version("1.1.0")).is_err());
    }

    #[test]
    fn a_skipped_version_stays_skipped_until_a_newer_one_appears() {
        assert!(is_skipped(&version("1.2.0"), "1.2.0"));
        assert!(is_skipped(&version("1.2.0"), "1.3.0"));
        assert!(!is_skipped(&version("1.2.1"), "1.2.0"));
        assert!(!is_skipped(&version("1.2.0"), ""));
        assert!(!is_skipped(&version("1.2.0"), "garbage"));
    }

    #[test]
    fn only_update_leftovers_are_recognized() {
        assert!(is_leftover_name("nemotron-bubble.exe.old"));
        assert!(is_leftover_name("DirectML.dll.old-2"));
        assert!(!is_leftover_name(".old"));
        assert!(!is_leftover_name("nemotron-bubble.exe"));
        assert!(!is_leftover_name("notes.old-x"));
        assert!(!is_leftover_name("folder.older"));
    }

    #[test]
    fn only_newer_partial_downloads_are_kept() {
        let current = version("1.1.0");
        assert!(is_resumable_download("nemotron-bubble.1.2.0.win-x64.tgz.part", &current));
        assert!(!is_resumable_download("nemotron-bubble.1.1.0.win-x64.tgz.part", &current));
        assert!(!is_resumable_download("nemotron-bubble.1.2.0.win-x64.tgz", &current));
    }

    /// Writes a .tgz like the release workflow (`tar -czf archive -C dir .`).
    fn write_archive(path: &Path, files: &[(&str, &[u8])]) {
        let gzip = flate2::write::GzEncoder::new(fs::File::create(path).unwrap(), flate2::Compression::default());
        let mut builder = tar::Builder::new(gzip);
        let mut root = tar::Header::new_gnu();
        root.set_entry_type(tar::EntryType::Directory);
        root.set_mode(0o755);
        root.set_size(0);
        builder.append_data(&mut root, "./", std::io::empty()).unwrap();
        for (name, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            builder.append_data(&mut header, format!("./{name}"), *content).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap().flush().unwrap();
    }

    #[test]
    fn the_release_archive_is_unpacked() {
        let dir = TestDir::new("update-extract");
        let archive = dir.path().join("update.tgz");
        write_archive(&archive, &[("nemotron-bubble.exe", b"new exe"), ("DirectML.dll", b"new dll")]);
        let staging = dir.path().join("staging");
        fs::create_dir_all(&staging).unwrap();

        let names = extract_archive(&archive, &staging).unwrap();
        assert_eq!(names, ["nemotron-bubble.exe", "DirectML.dll"]);
        assert_eq!(fs::read(staging.join("DirectML.dll")).unwrap(), b"new dll");
    }

    #[test]
    fn an_archive_with_folders_or_without_exe_is_refused() {
        let dir = TestDir::new("update-extract-bad");
        let staging = dir.path().join("staging");
        fs::create_dir_all(&staging).unwrap();

        let nested = dir.path().join("nested.tgz");
        write_archive(&nested, &[("nemotron-bubble.exe", b"exe"), ("sub/file.txt", b"x")]);
        assert!(extract_archive(&nested, &staging).is_err());

        let no_exe = dir.path().join("no-exe.tgz");
        write_archive(&no_exe, &[("README.md", b"readme")]);
        assert!(extract_archive(&no_exe, &staging).is_err());
    }

    /// Writes a .tgz whose only entry has the raw `name` and `kind`. tar::Builder
    /// refuses to write names like "../x", so the header is filled in directly.
    fn write_hostile_archive(path: &Path, name: &str, kind: tar::EntryType) {
        let gzip = flate2::write::GzEncoder::new(fs::File::create(path).unwrap(), flate2::Compression::default());
        let mut builder = tar::Builder::new(gzip);
        let content: &[u8] = if kind.is_file() { b"payload" } else { b"" };
        let mut header = tar::Header::new_old();
        header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
        header.set_entry_type(kind);
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        if !kind.is_file() {
            header.as_old_mut().linkname[..14].copy_from_slice(b"../outside.txt");
        }
        header.set_cksum();
        builder.append(&header, content).unwrap();
        builder.into_inner().unwrap().finish().unwrap().flush().unwrap();
    }

    #[test]
    fn hostile_archive_entries_are_refused_before_anything_is_written() {
        let cases = [
            ("../evil.exe", tar::EntryType::Regular),
            ("/evil.exe", tar::EntryType::Regular),
            ("C:/evil.exe", tar::EntryType::Regular),
            ("sub\\evil.exe", tar::EntryType::Regular),
            ("sub/evil.exe", tar::EntryType::Regular),
            ("evil.exe", tar::EntryType::Symlink),
            ("evil.exe", tar::EntryType::Link),
            ("evil", tar::EntryType::Directory),
        ];
        for (index, (name, kind)) in cases.into_iter().enumerate() {
            let dir = TestDir::new(&format!("update-hostile-{index}"));
            let archive = dir.path().join("hostile.tgz");
            write_hostile_archive(&archive, name, kind);
            let staging = dir.path().join("staging");
            fs::create_dir_all(&staging).unwrap();

            let result = extract_archive(&archive, &staging);
            assert!(result.is_err(), "{name} ({kind:?}) was accepted");
            assert!(!dir.path().join("evil.exe").exists(), "{name} ({kind:?}) escaped the staging folder");
            assert_eq!(fs::read_dir(&staging).unwrap().count(), 0, "{name} ({kind:?}) was unpacked");
        }
    }

    /// Opens `path` without sharing, so it can neither be deleted nor renamed,
    /// like the files of an exe that is still exiting.
    fn lock(path: &Path) -> fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new().read(true).share_mode(0).open(path).unwrap()
    }

    #[test]
    fn a_locked_old_file_makes_room_under_the_next_free_name() {
        let dir = TestDir::new("update-locked-old");
        let target = dir.path().join("nemotron-bubble.exe");
        fs::write(&target, "exe").unwrap();
        let old = dir.path().join("nemotron-bubble.exe.old");
        fs::write(&old, "older exe").unwrap();
        let _held = lock(&old);

        assert_eq!(old_path(&target), dir.path().join("nemotron-bubble.exe.old-2"));
        assert!(old.exists());
    }

    #[test]
    fn a_locked_leftover_is_removed_once_it_is_released() {
        let dir = TestDir::new("update-locked-leftover");
        let old = dir.path().join("nemotron-bubble.exe.old");
        fs::write(&old, "previous exe").unwrap();
        let held = lock(&old);

        assert!(!remove_leftovers_in(dir.path(), &version("1.1.0")), "a locked file must make the caller retry");
        assert!(old.exists());
        drop(held);
        assert!(remove_leftovers_in(dir.path(), &version("1.1.0")));
        assert!(!old.exists());
    }

    /// tests/data/updater.json was written by the "Update updater.json" step of
    /// release.yml (run locally with gh stubbed out). Regenerate it when that step
    /// changes what it writes.
    #[test]
    fn the_file_written_by_the_release_workflow_is_understood() {
        let json = include_str!("../tests/data/updater.json");
        let update = update_from_manifest(json, &version("1.1.0")).unwrap().expect("1.2.0 is offered");
        assert_eq!(update.version, version("1.2.0"));
        assert_eq!(update.release, "nemotron-bubble.1.2.0");
        assert_eq!(update.date.as_deref(), Some("2026-12-05"));
        assert_eq!(update.size, 18_886_659);
        assert_eq!(update.sha256, "1eb88d8db34e2ec8e0c0417c20be2d67d9cdd900147bdde9e564d3cc85fe99a4");
        assert!(update.url.ends_with(&format!("/nemotron-bubble.1.2.0/{}", archive_name(&update.version))));
    }

    /// The names the app looks for must be the ones release.yml writes.
    #[test]
    fn the_release_workflow_writes_the_names_the_app_reads() {
        let workflow = include_str!("../.github/workflows/release.yml");
        for expected in [
            format!("$updaterTag = '{UPDATER_TAG}'"),
            format!("--pattern '{UPDATER_ASSET}'"),
            format!("$data['projects']['{PROJECT_KEY}']"),
            format!("assets    = [ordered]@{{ '{RUNTIME_ID}' ="),
            format!("sha256    = [ordered]@{{ '{RUNTIME_ID}' ="),
            format!("size      = [ordered]@{{ '{RUNTIME_ID}' ="),
            "version   = $Env:VERSION".to_string(),
            "release   = $Env:TAG".to_string(),
            "date      = $date".to_string(),
            format!("archive-name=nemotron-bubble.$version.{RUNTIME_ID}.tgz"),
        ] {
            assert!(workflow.contains(&expected), "release.yml no longer contains: {expected}");
        }
    }

    #[test]
    fn installing_moves_the_old_files_aside_and_keeps_the_exe_name() {
        let dir = TestDir::new("update-install");
        let (root, staging) = (dir.path().join("app"), dir.path().join("staging"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::write(root.join("bubble.exe"), "old exe").unwrap();
        fs::write(root.join("DirectML.dll"), "old dll").unwrap();
        fs::write(root.join("DirectML.dll.old"), "older dll").unwrap();
        fs::write(staging.join("nemotron-bubble.exe"), "new exe").unwrap();
        fs::write(staging.join("DirectML.dll"), "new dll").unwrap();
        fs::write(staging.join("LICENSE"), "license").unwrap();
        let files = ["nemotron-bubble.exe", "DirectML.dll", "LICENSE"].map(String::from);

        let exe = install_files(&staging, &root, &files, "bubble.exe").unwrap();
        assert_eq!(exe, root.join("bubble.exe"));
        assert_eq!(fs::read_to_string(root.join("bubble.exe")).unwrap(), "new exe");
        assert_eq!(fs::read_to_string(root.join("bubble.exe.old")).unwrap(), "old exe");
        assert_eq!(fs::read_to_string(root.join("DirectML.dll")).unwrap(), "new dll");
        assert_eq!(fs::read_to_string(root.join("DirectML.dll.old")).unwrap(), "old dll");
        assert_eq!(fs::read_to_string(root.join("LICENSE")).unwrap(), "license");

        assert!(remove_leftovers_in(&root, &version("1.1.0")));
        assert!(!root.join("bubble.exe.old").exists());
        assert!(!root.join("DirectML.dll.old").exists());
        assert!(root.join("bubble.exe").exists());
    }

    #[test]
    fn a_failed_installation_restores_the_previous_files() {
        let dir = TestDir::new("update-rollback");
        let (root, staging) = (dir.path().join("app"), dir.path().join("staging"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::write(root.join("nemotron-bubble.exe"), "old exe").unwrap();
        fs::write(staging.join("nemotron-bubble.exe"), "new exe").unwrap();
        // "missing.dll" is listed but not unpacked, so its installation fails.
        let files = ["nemotron-bubble.exe", "missing.dll"].map(String::from);

        assert!(install_files(&staging, &root, &files, "nemotron-bubble.exe").is_err());
        assert_eq!(fs::read_to_string(root.join("nemotron-bubble.exe")).unwrap(), "old exe");
        assert!(!root.join("nemotron-bubble.exe.old").exists());
    }

    #[test]
    fn the_update_folder_keeps_only_a_newer_partial_download() {
        let dir = TestDir::new("update-folder");
        let folder = dir.path().join(UPDATE_FOLDER);
        fs::create_dir_all(folder.join("1.2.0")).unwrap();
        fs::write(folder.join("nemotron-bubble.1.2.0.win-x64.tgz.part"), "part").unwrap();
        fs::write(folder.join("nemotron-bubble.1.1.0.win-x64.tgz"), "old").unwrap();

        assert!(remove_leftovers_in(dir.path(), &version("1.1.0")));
        assert!(folder.join("nemotron-bubble.1.2.0.win-x64.tgz.part").exists());
        assert!(!folder.join("1.2.0").exists());
        assert!(!folder.join("nemotron-bubble.1.1.0.win-x64.tgz").exists());

        assert!(remove_leftovers_in(dir.path(), &version("1.2.0")));
        assert!(!folder.exists());
    }
}
