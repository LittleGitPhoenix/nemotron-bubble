//! Model downloads from Hugging Face: the state shown on the Model tab, the
//! update check, and downloading with resume and SHA-256 verification.
//!
//! Each model folder gets a `model-info.json` that records the upstream version
//! (date of its last change) and the SHA-256 of every file, so later update
//! checks only hash files whose size or modification time changed.

use crate::*;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

/// The Hugging Face repository that hosts the ONNX exports parakeet-rs loads.
pub(crate) const HF_REPO: &str = "altunenes/parakeet-rs";
/// The files a model consists of (all of them are downloaded and verified).
pub(crate) const MODEL_FILES: [&str; 4] = ["encoder.onnx", "encoder.onnx.data", "decoder_joint.onnx", "tokenizer.model"];
/// The models the Model tab can download, in display order.
pub(crate) const DOWNLOADABLE_MODELS: [ModelChoice; 2] = [ModelChoice::English, ModelChoice::Multilingual];
const MANIFEST_NAME: &str = "model-info.json";
// How often a running download or verification repaints its progress.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// Folder of a model inside the Hugging Face repository.
fn repo_folder(choice: ModelChoice) -> &'static str {
    match choice {
        ModelChoice::Multilingual => "nemotron-3.5-asr-streaming-0.6b-onnx",
        _ => "nemotron-speech-streaming-en-0.6b",
    }
}

/// Where a model that is not installed yet is downloaded to: `<exe dir>\models\<folder>`.
fn download_dir(choice: ModelChoice) -> PathBuf {
    app_root_dir().join("models").join(choice.folder_name().unwrap_or("nemotron"))
}

/// One model file: as published upstream, or as found on disk (with its
/// modification time, so an unchanged file need not be hashed again).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ModelFile {
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
    pub(crate) modified: u64,
}

/// Contents of `model-info.json` in a model folder.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ModelManifest {
    pub(crate) repo: String,
    pub(crate) folder: String,
    /// Date of the upstream change these files match, e.g. "2026-10-22". None
    /// when the files were hashed but match no known upstream version.
    pub(crate) date: Option<String>,
    pub(crate) commit: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) files: Vec<ModelFile>,
}

/// The current upstream version of a model.
#[derive(Clone, Debug)]
pub(crate) struct RemoteModel {
    pub(crate) date: String,
    pub(crate) commit: String,
    pub(crate) title: String,
    pub(crate) files: Vec<ModelFile>,
}

impl RemoteModel {
    pub(crate) fn total_size(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
}

/// What is happening with a model right now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum DownloadActivity {
    #[default]
    Idle,
    Checking,
    Verifying { percent: u32 },
    Downloading { done: u64, total: u64 },
    Cancelling,
    Installing,
}

/// Everything the Model tab shows about one downloadable model.
#[derive(Clone, Debug, Default)]
pub(crate) struct ModelDownloadState {
    pub(crate) activity: DownloadActivity,
    /// Folder the model was found in (None: not installed).
    pub(crate) installed_dir: Option<PathBuf>,
    pub(crate) installed_bytes: u64,
    /// Bytes of an interrupted download that the next download resumes from.
    pub(crate) partial_bytes: u64,
    /// Upstream date of the installed files, from model-info.json.
    pub(crate) local_date: Option<String>,
    /// Upstream version, known after an update check or a download.
    pub(crate) remote: Option<RemoteModel>,
    /// Result of the last update check (None: not checked yet).
    pub(crate) up_to_date: Option<bool>,
    pub(crate) error: Option<String>,
}

impl ModelDownloadState {
    pub(crate) fn busy(&self) -> bool {
        self.activity != DownloadActivity::Idle
    }
}

fn with_download<R>(app: &Arc<AppState>, index: usize, update: impl FnOnce(&mut ModelDownloadState) -> R) -> R {
    let mut downloads = app.downloads.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    update(&mut downloads[index])
}

pub(crate) fn downloads_snapshot(app: &Arc<AppState>) -> Vec<ModelDownloadState> {
    app.downloads.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

fn any_download_busy(app: &Arc<AppState>) -> bool {
    app.downloads
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .any(ModelDownloadState::busy)
}

/// Repaint the settings window (safe from any thread).
fn repaint_settings(app: &Arc<AppState>) {
    unsafe {
        let hwnd = settings_hwnd_from_app(app);
        if hwnd != null_mut() {
            PostMessageW(hwnd, WM_UI_UPDATE, 0, 0);
        }
    }
}

fn set_activity(app: &Arc<AppState>, index: usize, activity: DownloadActivity) {
    with_download(app, index, |state| state.activity = activity);
    repaint_settings(app);
}

fn set_status(app: &Arc<AppState>, status: String) {
    update_ui(app, |ui| ui.status = status);
}

fn file_modified(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|age| age.as_secs())
        .unwrap_or(0)
}

fn read_manifest(dir: &Path) -> Option<ModelManifest> {
    let json = fs::read_to_string(dir.join(MANIFEST_NAME)).ok()?;
    serde_json::from_str(&json).ok()
}

fn write_manifest(dir: &Path, manifest: &ModelManifest) {
    let result = serde_json::to_string_pretty(manifest)
        .map_err(anyhow::Error::from)
        .and_then(|json| fs::write(dir.join(MANIFEST_NAME), json).map_err(anyhow::Error::from));
    if let Err(err) = result {
        log_error(&format!("Could not write {}: {err:#}", dir.join(MANIFEST_NAME).display()));
    }
}

/// True when the manifest describes every model file exactly as it is on disk
/// (same size and modification time), so its hashes and date can be trusted.
fn manifest_matches_disk(manifest: &ModelManifest, dir: &Path) -> bool {
    MODEL_FILES.iter().all(|name| {
        let Ok(meta) = fs::metadata(dir.join(name)) else {
            return false;
        };
        manifest
            .files
            .iter()
            .any(|file| file.name == *name && file.size == meta.len() && file.modified == file_modified(&meta))
    })
}

/// Read the disk: where each model is installed, its size, an interrupted
/// download, and the version recorded in model-info.json.
pub(crate) fn refresh_local_model_state(app: &Arc<AppState>) {
    for (index, choice) in DOWNLOADABLE_MODELS.iter().enumerate() {
        let dir = find_model_dir(*choice, "").ok();
        let installed_bytes = dir
            .as_ref()
            .map(|dir| {
                MODEL_FILES
                    .iter()
                    .filter_map(|name| fs::metadata(dir.join(name)).ok())
                    .map(|meta| meta.len())
                    .sum()
            })
            .unwrap_or(0);
        let local_date = dir
            .as_ref()
            .and_then(|dir| read_manifest(dir).filter(|manifest| manifest_matches_disk(manifest, dir)))
            .and_then(|manifest| manifest.date);
        let part_dir = dir.clone().unwrap_or_else(|| download_dir(*choice));
        let partial_bytes = MODEL_FILES
            .iter()
            .filter_map(|name| fs::metadata(part_dir.join(format!("{name}.part"))).ok())
            .map(|meta| meta.len())
            .sum();
        with_download(app, index, |state| {
            state.installed_dir = dir;
            state.installed_bytes = installed_bytes;
            state.partial_bytes = partial_bytes;
            state.local_date = local_date;
        });
    }
    repaint_settings(app);
}

fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .user_agent(format!("nemotron-bubble/{APP_VERSION}"))
        .build()
        .into()
}

/// Ask Hugging Face for the current files of a model: size, SHA-256 and the
/// newest change among them, which serves as the model's version.
fn fetch_remote_model(choice: ModelChoice) -> Result<RemoteModel> {
    let folder = repo_folder(choice);
    let url = format!("https://huggingface.co/api/models/{HF_REPO}/tree/main/{folder}?expand=true");
    let json = http_agent()
        .get(&url)
        .call()
        .with_context(|| format!("could not reach Hugging Face ({url})"))?
        .body_mut()
        .read_to_string()
        .context("could not read the file list from Hugging Face")?;
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&json).context("Hugging Face returned an unexpected file list")?;

    let mut files = Vec::new();
    let mut newest: Option<(String, String, String)> = None; // (date, commit, title)
    for name in MODEL_FILES {
        let path = format!("{folder}/{name}");
        let entry = entries
            .iter()
            .find(|entry| entry["path"] == path.as_str())
            .ok_or_else(|| anyhow!("{name} is missing in the Hugging Face repository"))?;
        let sha256 = entry["lfs"]["oid"]
            .as_str()
            .ok_or_else(|| anyhow!("Hugging Face lists no SHA-256 for {name}"))?;
        files.push(ModelFile {
            name: name.to_string(),
            size: entry["size"].as_u64().unwrap_or(0),
            sha256: sha256.to_string(),
            modified: 0,
        });
        let commit = &entry["lastCommit"];
        let date = commit["date"].as_str().unwrap_or_default().to_string();
        if newest.as_ref().is_none_or(|(newest_date, _, _)| date > *newest_date) {
            newest = Some((
                date,
                commit["id"].as_str().unwrap_or_default().to_string(),
                commit["title"].as_str().unwrap_or_default().to_string(),
            ));
        }
    }
    let (date, commit, title) = newest.unwrap_or_default();
    Ok(RemoteModel {
        date: date.chars().take(10).collect(),
        commit,
        title,
        files,
    })
}

/// SHA-256 of a file as lowercase hex, reporting each chunk's length to `progress`.
fn hash_file(path: &Path, mut progress: impl FnMut(u64)) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("could not open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        progress(read as u64);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// The hashes of an installed model: from model-info.json when it still
/// matches the disk, otherwise computed (shown as "Verifying" on the Model tab).
fn local_manifest(app: &Arc<AppState>, index: usize, dir: &Path) -> Result<ModelManifest> {
    if let Some(manifest) = read_manifest(dir).filter(|manifest| manifest_matches_disk(manifest, dir)) {
        return Ok(manifest);
    }
    let total: u64 = MODEL_FILES
        .iter()
        .filter_map(|name| fs::metadata(dir.join(name)).ok())
        .map(|meta| meta.len())
        .sum::<u64>()
        .max(1);
    let mut done = 0u64;
    let mut last_report = Instant::now();
    set_activity(app, index, DownloadActivity::Verifying { percent: 0 });
    let mut files = Vec::new();
    for name in MODEL_FILES {
        let path = dir.join(name);
        let meta = fs::metadata(&path).with_context(|| format!("{name} is missing"))?;
        let sha256 = hash_file(&path, |read| {
            done += read;
            if last_report.elapsed() >= PROGRESS_INTERVAL {
                last_report = Instant::now();
                set_activity(app, index, DownloadActivity::Verifying { percent: (done * 100 / total) as u32 });
            }
        })?;
        files.push(ModelFile {
            name: name.to_string(),
            size: meta.len(),
            sha256,
            modified: file_modified(&meta),
        });
    }
    // Same contents as recorded (only the timestamps changed, e.g. after
    // copying the folder): the recorded version still applies.
    let previous = read_manifest(dir).filter(|previous| {
        files.iter().all(|file| {
            previous
                .files
                .iter()
                .any(|old| old.name == file.name && old.sha256 == file.sha256)
        })
    });
    Ok(ModelManifest {
        repo: HF_REPO.to_string(),
        folder: dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
        date: previous.as_ref().and_then(|previous| previous.date.clone()),
        commit: previous.as_ref().and_then(|previous| previous.commit.clone()),
        title: previous.and_then(|previous| previous.title),
        files,
    })
}

/// "Check for updates": fetch the upstream version of both models and compare
/// the installed files with it. Runs in the background.
pub(crate) fn check_for_model_updates(app: &Arc<AppState>) {
    if any_download_busy(app) {
        return;
    }
    for index in 0..DOWNLOADABLE_MODELS.len() {
        with_download(app, index, |state| {
            state.activity = DownloadActivity::Checking;
            state.error = None;
        });
    }
    repaint_settings(app);
    set_status(app, "Checking Hugging Face for model updates...".to_string());

    let app = app.clone();
    thread::spawn(move || {
        let mut updates = Vec::new();
        for (index, choice) in DOWNLOADABLE_MODELS.iter().enumerate() {
            match check_model(&app, index, *choice) {
                Ok(true) => updates.push(choice.label()),
                Ok(false) => {}
                Err(err) => {
                    log_error(&format!("Update check for the {} model failed: {err:#}", choice.label()));
                    with_download(&app, index, |state| state.error = Some(format!("{err:#}")));
                }
            }
            set_activity(&app, index, DownloadActivity::Idle);
        }
        refresh_local_model_state(&app);
        set_status(
            &app,
            if updates.is_empty() {
                "Model update check finished.".to_string()
            } else {
                format!("Update available for: {}.", updates.join(", "))
            },
        );
    });
}

/// Check one model. Returns true when an update is available.
fn check_model(app: &Arc<AppState>, index: usize, choice: ModelChoice) -> Result<bool> {
    let remote = fetch_remote_model(choice)?;
    log_info(&format!(
        "{} model upstream: {} ({}) \"{}\"",
        choice.label(),
        remote.date,
        remote.commit.chars().take(8).collect::<String>(),
        remote.title
    ));
    with_download(app, index, |state| state.remote = Some(remote.clone()));
    let Some(dir) = with_download(app, index, |state| state.installed_dir.clone()) else {
        return Ok(false);
    };
    let mut manifest = local_manifest(app, index, &dir)?;
    let up_to_date = remote.files.iter().all(|wanted| {
        manifest
            .files
            .iter()
            .any(|file| file.name == wanted.name && file.sha256 == wanted.sha256)
    });
    if up_to_date {
        manifest.date = Some(remote.date.clone());
        manifest.commit = Some(remote.commit.clone());
        manifest.title = Some(remote.title.clone());
    }
    write_manifest(&dir, &manifest);
    log_info(&format!(
        "{} model in {}: {}",
        choice.label(),
        dir.display(),
        if up_to_date { "up to date" } else { "update available" }
    ));
    with_download(app, index, |state| state.up_to_date = Some(up_to_date));
    Ok(!up_to_date)
}

/// Download (or update, or resume) a model in the background.
pub(crate) fn start_model_download(app: &Arc<AppState>, choice: ModelChoice) {
    let Some(index) = DOWNLOADABLE_MODELS.iter().position(|model| *model == choice) else {
        return;
    };
    if any_download_busy(app) {
        return;
    }
    app.download_cancel.store(false, Ordering::SeqCst);
    with_download(app, index, |state| {
        state.activity = DownloadActivity::Downloading { done: 0, total: 0 };
        state.error = None;
    });
    repaint_settings(app);
    set_status(app, format!("Downloading the {} model...", choice.label()));

    let app = app.clone();
    thread::spawn(move || {
        match download_model(&app, index, choice) {
            Ok(Some(backup)) => {
                log_info(&format!("{} model downloaded", choice.label()));
                set_status(
                    &app,
                    match backup {
                        Some(backup) => format!(
                            "The {} model is updated. The previous version is kept in {}.",
                            choice.label(),
                            backup.display()
                        ),
                        None => format!("The {} model is downloaded.", choice.label()),
                    },
                );
            }
            Ok(None) => {
                log_info(&format!("{} model download cancelled", choice.label()));
                set_status(&app, "Download cancelled. Resume continues where it stopped.".to_string());
            }
            Err(err) => {
                log_error(&format!("{} model download failed: {err:#}", choice.label()));
                with_download(&app, index, |state| state.error = Some(format!("{err:#}")));
                set_status(&app, format!("The {} model download failed.", choice.label()));
            }
        }
        set_activity(&app, index, DownloadActivity::Idle);
        refresh_local_model_state(&app);
    });
}

/// Ask a running download to stop. Its partial files are kept for resuming.
pub(crate) fn cancel_model_download(app: &Arc<AppState>, choice: ModelChoice) {
    let Some(index) = DOWNLOADABLE_MODELS.iter().position(|model| *model == choice) else {
        return;
    };
    let downloading = with_download(app, index, |state| {
        matches!(state.activity, DownloadActivity::Downloading { .. })
    });
    if downloading {
        app.download_cancel.store(true, Ordering::SeqCst);
        set_activity(app, index, DownloadActivity::Cancelling);
    }
}

/// Returns Ok(None) when cancelled, otherwise the backup folder of the
/// previously installed version (None for a first download).
fn download_model(app: &Arc<AppState>, index: usize, choice: ModelChoice) -> Result<Option<Option<PathBuf>>> {
    let remote = fetch_remote_model(choice)?;
    with_download(app, index, |state| state.remote = Some(remote.clone()));
    // Update in place where the model was found, else download next to the exe.
    let dir = with_download(app, index, |state| state.installed_dir.clone()).unwrap_or_else(|| download_dir(choice));
    fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    log_info(&format!(
        "Downloading the {} model ({}) into {}",
        choice.label(),
        remote.date,
        dir.display()
    ));

    // Keep files that already match upstream, e.g. the tokenizer when only the encoder changed.
    let local = read_manifest(&dir).filter(|manifest| manifest_matches_disk(manifest, &dir));
    let needed: Vec<&ModelFile> = remote
        .files
        .iter()
        .filter(|wanted| {
            !local.as_ref().is_some_and(|manifest| {
                manifest
                    .files
                    .iter()
                    .any(|file| file.name == wanted.name && file.sha256 == wanted.sha256)
            })
        })
        .collect();

    let mut progress = Progress {
        app,
        index,
        done: 0,
        total: needed.iter().map(|file| file.size).sum(),
        last_report: Instant::now(),
    };
    for file in &needed {
        if !download_file(&mut progress, &dir, repo_folder(choice), file)? {
            return Ok(None);
        }
    }

    // Install: keep the installed version as a backup, then swap the verified
    // .part files in. The loaded model may keep its files open, so wait for a
    // running dictation to end and unload it first.
    set_activity(app, index, DownloadActivity::Installing);
    let backup = if has_model_files(&dir) && !needed.is_empty() {
        Some(backup_model(&dir)?)
    } else {
        None
    };
    while app.recording.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(500));
    }
    let active = settings_snapshot(app).model == choice;
    if active {
        let _ = app.engine_tx.send(EngineCommand::Unload);
    }
    for file in &needed {
        replace_file(&dir.join(format!("{}.part", file.name)), &dir.join(&file.name))?;
    }

    let files = remote
        .files
        .iter()
        .map(|file| {
            let modified = fs::metadata(dir.join(&file.name)).map(|meta| file_modified(&meta)).unwrap_or(0);
            ModelFile {
                modified,
                ..file.clone()
            }
        })
        .collect();
    write_manifest(
        &dir,
        &ModelManifest {
            repo: HF_REPO.to_string(),
            folder: repo_folder(choice).to_string(),
            date: Some(remote.date.clone()),
            commit: Some(remote.commit.clone()),
            title: Some(remote.title.clone()),
            files,
        },
    );
    with_download(app, index, |state| state.up_to_date = Some(true));

    // The model in use was replaced (or installed for the first time): load it.
    if active && settings_snapshot(app).preload_model {
        let _ = app.engine_tx.send(EngineCommand::Preload);
    }
    Ok(Some(backup))
}

/// Keep the installed version of a model next to it as `<folder>_<version>`,
/// e.g. `models\nemotron_2026-10-22` ("unknown" when no version is recorded).
/// To go back, point Predefined at it or rename the folders.
///
/// The model files are hard-linked, which is instant. Files the update replaces
/// then live on in the backup only (new files are renamed over the old names,
/// never written into), and unchanged files stay shared between both folders. Where hard links are not
/// supported (e.g. FAT drives) they are copied. model-info.json is always copied,
/// because it is rewritten in place after the update.
fn backup_model(dir: &Path) -> Result<PathBuf> {
    let version = read_manifest(dir)
        .and_then(|manifest| manifest.date)
        .unwrap_or_else(|| "unknown".to_string());
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".to_string());
    let parent = dir.parent().unwrap_or(dir);
    let mut backup = parent.join(format!("{name}_{version}"));
    let mut counter = 2;
    while backup.exists() {
        backup = parent.join(format!("{name}_{version}-{counter}"));
        counter += 1;
    }
    fs::create_dir_all(&backup).with_context(|| format!("could not create the backup folder {}", backup.display()))?;

    for file in MODEL_FILES {
        let (source, target) = (dir.join(file), backup.join(file));
        if !source.exists() {
            continue;
        }
        if fs::hard_link(&source, &target).is_err() {
            fs::copy(&source, &target).with_context(|| format!("could not back up {}", source.display()))?;
        }
    }
    let manifest = dir.join(MANIFEST_NAME);
    if manifest.exists() {
        fs::copy(&manifest, backup.join(MANIFEST_NAME))
            .with_context(|| format!("could not back up {}", manifest.display()))?;
    }
    log_info(&format!("Previous model version kept in {}", backup.display()));
    Ok(backup)
}

/// Progress of one model download across its files.
struct Progress<'a> {
    app: &'a Arc<AppState>,
    index: usize,
    done: u64,
    total: u64,
    last_report: Instant,
}

impl Progress<'_> {
    fn advance(&mut self, bytes: u64) {
        self.done += bytes;
        if self.last_report.elapsed() >= PROGRESS_INTERVAL {
            self.last_report = Instant::now();
            self.report();
        }
    }

    fn report(&self) {
        if self.app.download_cancel.load(Ordering::SeqCst) {
            return; // keep showing "Cancelling..."
        }
        set_activity(
            self.app,
            self.index,
            DownloadActivity::Downloading {
                done: self.done,
                total: self.total,
            },
        );
    }
}

/// Download one file into `<name>.part` and verify its SHA-256. An existing
/// `.part` file is resumed. Returns Ok(false) when cancelled.
fn download_file(progress: &mut Progress, dir: &Path, folder: &str, file: &ModelFile) -> Result<bool> {
    let part = dir.join(format!("{}.part", file.name));
    let mut hasher = Sha256::new();
    let mut have = 0u64;

    // Resume: hash what an earlier, interrupted download already stored.
    if let Ok(meta) = fs::metadata(&part) {
        if meta.len() <= file.size {
            let mut existing = fs::File::open(&part)?;
            let mut buffer = vec![0u8; 1 << 20];
            loop {
                let read = existing.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
                have += read as u64;
                progress.advance(read as u64);
            }
        }
    }

    if have < file.size {
        let url = format!("https://huggingface.co/{HF_REPO}/resolve/main/{folder}/{}", file.name);
        let mut request = http_agent().get(&url);
        if have > 0 {
            request = request.header("Range", format!("bytes={have}-"));
        }
        let response = request
            .call()
            .with_context(|| format!("could not download {}", file.name))?;
        let resumed = have > 0 && response.status().as_u16() == 206;
        let mut out = if resumed {
            fs::OpenOptions::new().append(true).open(&part)?
        } else {
            // Fresh start (or the server ignored the range): drop what was counted.
            progress.done -= have;
            have = 0;
            hasher = Sha256::new();
            fs::File::create(&part).with_context(|| format!("could not create {}", part.display()))?
        };
        let mut reader = response.into_body().into_reader();
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            if progress.app.download_cancel.load(Ordering::SeqCst) {
                out.flush()?;
                return Ok(false);
            }
            let read = reader
                .read(&mut buffer)
                .with_context(|| format!("the download of {} was interrupted", file.name))?;
            if read == 0 {
                break;
            }
            out.write_all(&buffer[..read])
                .with_context(|| format!("could not write {} (disk full?)", part.display()))?;
            hasher.update(&buffer[..read]);
            have += read as u64;
            progress.advance(read as u64);
        }
        out.flush()?;
    }

    if have != file.size {
        bail!("{}: received {have} of {} bytes", file.name, file.size);
    }
    let sha256 = format!("{:x}", hasher.finalize());
    if sha256 != file.sha256 {
        let _ = fs::remove_file(&part);
        bail!("{} is damaged (SHA-256 does not match). Download it again.", file.name);
    }
    Ok(true)
}

/// Replace `target` with `source`. Retries for a while, because a model that
/// was just unloaded may still hold its files open for a moment.
fn replace_file(source: &Path, target: &Path) -> Result<()> {
    let mut attempt = 0;
    loop {
        match fs::rename(source, target) {
            Ok(()) => return Ok(()),
            Err(err) if attempt < 40 => {
                attempt += 1;
                if attempt == 1 {
                    log_info(&format!("{} is in use, waiting: {err}", target.display()));
                }
                thread::sleep(Duration::from_millis(250));
            }
            Err(err) => {
                return Err(anyhow!(err).context(format!("could not replace {}", target.display())));
            }
        }
    }
}
