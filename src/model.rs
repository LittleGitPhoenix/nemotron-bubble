//! Speech model: finding, checking and loading Nemotron, GPU probing, languages and model switching.

use crate::*;

/// Switch the speech model: unload the current one (freeing its RAM) and, with
/// preloading on, load the new one in the background right away.
pub(crate) fn select_model(app: &Arc<AppState>, choice: ModelChoice) {
    if app.recording.load(Ordering::SeqCst) {
        update_ui(app, |ui| {
            ui.status = "Stop dictation before switching models.".to_string();
        });
        return;
    }
    if !model_choice_available(&settings_snapshot(app), choice) {
        log_info(
            "Predefined needs NEMOTRON_MODEL_DIR or \"model_dir\" in config\\settings.json \
             (restart the app after setting it).",
        );
        update_ui(app, |ui| {
            ui.status = "No model folder is set for Predefined. See the log.".to_string();
        });
        return;
    }
    let settings = {
        let mut settings = app
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if settings.model == choice {
            return;
        }
        settings.model = choice;
        settings.clone()
    };
    save_settings(&settings);
    log_info(&format!("Speech model changed to {}", choice.label()));

    let _ = app.engine_tx.send(EngineCommand::Unload);
    if settings.preload_model {
        let _ = app.engine_tx.send(EngineCommand::Preload);
    }
    update_ui(app, |ui| {
        ui.status = if settings.preload_model {
            format!("Switching to {} model...", choice.label())
        } else {
            format!("{} model selected. Loads on first dictation.", choice.label())
        };
    });
}

/// Switch between GPU and CPU processing. The device is fixed when a model is
/// loaded, so this unloads it and (with preloading on) loads it again.
pub(crate) fn select_compute(app: &Arc<AppState>, choice: ComputeChoice) {
    if app.recording.load(Ordering::SeqCst) {
        update_ui(app, |ui| {
            ui.status = "Stop dictation before switching processing.".to_string();
        });
        return;
    }
    let settings = {
        let mut settings = app
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if settings.compute == choice {
            return;
        }
        settings.compute = choice;
        settings.clone()
    };
    save_settings(&settings);
    log_info(&format!("Processing changed to {}", choice.label()));

    let _ = app.engine_tx.send(EngineCommand::Unload);
    if settings.preload_model {
        let _ = app.engine_tx.send(EngineCommand::Preload);
    }
    update_ui(app, |ui| {
        ui.status = if settings.preload_model {
            format!("Reloading the model for processing: {}...", choice.label())
        } else {
            format!("Processing: {}. Applies on the next dictation.", choice.label())
        };
    });
}

/// The language only matters for the multilingual model, so the Language
/// controls are greyed out when the English model is selected. (Predefined may
/// point at either model, so it keeps them enabled.)
pub(crate) fn language_selectable(settings: &AppSettings) -> bool {
    settings.model != ModelChoice::English
}

/// Set the spoken language for the multilingual model. Applied immediately to a
/// loaded model (no reload) and saved for future loads.
pub(crate) fn select_language(app: &Arc<AppState>, code: &'static str) {
    if app.recording.load(Ordering::SeqCst) {
        update_ui(app, |ui| {
            ui.status = "Stop dictation before changing the language.".to_string();
        });
        return;
    }
    let settings = {
        let mut settings = app
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !language_selectable(&settings) {
            drop(settings);
            update_ui(app, |ui| {
                ui.status = "The language applies to the multilingual model only.".to_string();
            });
            return;
        }
        if normalize_language(&settings.language) == code {
            return;
        }
        settings.language = code.to_string();
        settings.clone()
    };
    save_settings(&settings);
    let _ = app.engine_tx.send(EngineCommand::SetLanguage(code.to_string()));
    update_ui(app, |ui| {
        ui.status = format!("Language set to {}.", language_label(code));
    });
}

pub(crate) fn report_model_error(err: &anyhow::Error) {
    let message = format!("{err:#}");
    log_error(&format!("Nemotron model failed to load: {message}"));
    notify_user(
        "Nemotron Bubble: speech model failed to load",
        &format!(
            "{message}\n\nYou can choose the speech model in the application window.\n\n\
             Log file: {}",
            log_path().display()
        ),
        MB_ICONERROR,
    );
}

/// The process-wide ONNX Runtime environment (parakeet-rs >= 0.4 requires one).
/// Created on first use and shared by every model load, e.g. when switching models.
pub(crate) fn ort_environment() -> Result<Environment> {
    static ENV: OnceCell<Environment> = OnceCell::new();
    ENV.get_or_try_init(|| parakeet_rs::ort::init().build())
        .cloned()
        .map_err(|err| anyhow!("failed to initialize ONNX Runtime: {err}"))
}

pub(crate) fn model_status_label(model: &Nemotron, language: &str, on_gpu: bool) -> String {
    let model_part = match model.mode() {
        NemotronMode::EnglishOnly => "Loaded: English".to_string(),
        NemotronMode::Multilingual if normalize_language(language) == "auto" => {
            "Loaded: Multilingual (detects language)".to_string()
        }
        NemotronMode::Multilingual => format!("Loaded: Multilingual ({})", language_label(language)),
    };
    format!("{model_part} · {}", if on_gpu { "GPU" } else { "CPU" })
}

/// Point the multilingual model at `language` (a LANGUAGES code; "auto" lets
/// the model detect it). Takes effect from the next audio chunk, no reload needed.
pub(crate) fn apply_language(model: &mut Nemotron, language: &str) -> Result<()> {
    let code = normalize_language(language);
    model
        .set_target_lang(code)
        .map_err(|err| anyhow!("failed to set language '{code}': {err}"))?;
    log_info(&format!("Multilingual language: {} ({code})", language_label(code)));
    Ok(())
}

/// Check that DirectML can open a GPU, by registering it (with "fail if
/// unavailable") on a throwaway session builder. parakeet-rs itself registers
/// DirectML with a silent CPU fallback, so without this check "GPU" could quietly
/// run on the CPU.
pub(crate) fn probe_directml(environment: &Environment) -> Result<()> {
    use parakeet_rs::ort::ep::DirectML;
    use parakeet_rs::ort::session::Session;
    let builder = Session::builder(environment).map_err(|err| anyhow!("{err}"))?;
    builder
        .with_execution_providers([DirectML::default().build().error_on_failure()])
        .map_err(|err| anyhow!("{err}"))?;
    Ok(())
}

pub(crate) fn load_with_device(model_dir: &std::path::Path, on_gpu: bool) -> Result<Nemotron> {
    let mut exec_config = ExecutionConfig::new().with_environment(ort_environment()?);
    if on_gpu {
        // DirectML requires memory patterns off and sequential execution.
        exec_config = exec_config
            .with_execution_provider(ExecutionProvider::DirectML)
            .with_custom_configure(|builder| {
                Ok(builder
                    .with_memory_pattern(false)?
                    .with_parallel_execution(false)?)
            });
    }
    Nemotron::from_pretrained(model_dir, Some(exec_config))
        .map_err(|err| anyhow!("failed to load {}: {err}", model_dir.display()))
}

/// A loaded model plus where it came from and where it runs.
pub(crate) struct LoadedModel {
    pub(crate) model: Nemotron,
    pub(crate) dir: PathBuf,
    pub(crate) on_gpu: bool,
}

pub(crate) fn load_nemotron(settings: &AppSettings) -> Result<LoadedModel> {
    let choice = settings.model;
    let model_dir = find_model_dir(choice, &settings.model_dir)?;
    log_info(&format!(
        "Loading Nemotron ({} selected, processing: {}) from {}",
        choice.label(),
        settings.compute.label(),
        model_dir.display()
    ));
    check_model_files(&model_dir)?;

    let started = Instant::now();
    let gpu_usable = match settings.compute {
        ComputeChoice::Cpu => false,
        ComputeChoice::Gpu => {
            probe_directml(&ort_environment()?).map_err(|err| {
                anyhow!(
                    "GPU processing was selected, but no usable GPU was found (DirectML: {err}).\n\n\
                     Choose Auto or CPU under Processing in the settings."
                )
            })?;
            true
        }
        ComputeChoice::Auto => match probe_directml(&ort_environment()?) {
            Ok(()) => true,
            Err(err) => {
                log_info(&format!("No usable GPU (DirectML: {err}); using the CPU."));
                false
            }
        },
    };
    let (mut model, on_gpu) = match load_with_device(&model_dir, gpu_usable) {
        Ok(model) => (model, gpu_usable),
        Err(err) if gpu_usable && settings.compute == ComputeChoice::Auto => {
            log_error(&format!("Loading on the GPU failed, retrying on the CPU: {err:#}"));
            (load_with_device(&model_dir, false)?, false)
        }
        Err(err) => return Err(err),
    };

    let mode = match model.mode() {
        NemotronMode::EnglishOnly => "English-only",
        NemotronMode::Multilingual => "Multilingual",
    };
    if model.mode() == NemotronMode::Multilingual {
        apply_language(&mut model, &settings.language)?;
    }
    log_info(&format!(
        "Nemotron loaded ({mode}, on the {}) in {:.1}s",
        if on_gpu { "GPU via DirectML" } else { "CPU" },
        started.elapsed().as_secs_f32()
    ));
    let mismatch = matches!(
        (choice, model.mode()),
        (ModelChoice::English, NemotronMode::Multilingual)
            | (ModelChoice::Multilingual, NemotronMode::EnglishOnly)
    );
    if mismatch {
        log_error(&format!(
            "{} was selected, but {} contains the {mode} model.",
            choice.label(),
            model_dir.display()
        ));
    }

    Ok(LoadedModel {
        model,
        dir: model_dir,
        on_gpu,
    })
}

/// Catch the most common broken-download cases (missing weights file, or a file
/// left truncated by an interrupted download — the script skips files that
/// already exist) with a readable message instead of an opaque ONNX error.
pub(crate) fn check_model_files(model_dir: &std::path::Path) -> Result<()> {
    const MIN_SIZES: &[(&str, u64)] = &[
        ("encoder.onnx", 1_000_000),
        ("encoder.onnx.data", 2_000_000_000),
        ("decoder_joint.onnx", 10_000_000),
        ("tokenizer.model", 100_000),
    ];
    let mut problems = Vec::new();
    for (name, min) in MIN_SIZES {
        let path = model_dir.join(name);
        match fs::metadata(&path) {
            Ok(meta) => {
                log_info(&format!("  {name}: {} bytes", meta.len()));
                if meta.len() < *min {
                    problems.push(format!(
                        "{name} is only {} bytes — the download looks incomplete",
                        meta.len()
                    ));
                }
            }
            Err(_) => problems.push(format!("{name} is missing")),
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "Model files in {} are not usable:\n  - {}\nDownload the model again on the Model tab of the application window.",
            model_dir.display(),
            problems.join("\n  - ")
        ))
    }
}

/// The Predefined folders, in priority order: NEMOTRON_MODEL_DIR, then
/// `model_dir` from settings.json. Empty when neither is set.
pub(crate) fn predefined_model_dirs(configured_dir: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(path) = env::var("NEMOTRON_MODEL_DIR") {
        if !path.trim().is_empty() {
            dirs.push(PathBuf::from(path.trim()));
        }
    }
    if !configured_dir.trim().is_empty() {
        dirs.push(PathBuf::from(configured_dir.trim()));
    }
    dirs
}

/// Whether `choice` can be selected. Only Predefined can be unavailable: it
/// needs NEMOTRON_MODEL_DIR or `model_dir`. Both are fixed while the app runs
/// (settings are read once at startup, and a process keeps its environment), so
/// the UI can grey it out for the whole session.
pub(crate) fn model_choice_available(settings: &AppSettings, choice: ModelChoice) -> bool {
    choice != ModelChoice::Predefined || !predefined_model_dirs(&settings.model_dir).is_empty()
}

/// The model choice for a first start (or an unknown saved value): Predefined
/// when a predefined folder is set, otherwise the installed model (English
/// first). Falls back to English, whose error message then explains the download.
pub(crate) fn detect_model_choice(configured_dir: &str) -> ModelChoice {
    let choice = if !predefined_model_dirs(configured_dir).is_empty() {
        ModelChoice::Predefined
    } else if find_model_dir(ModelChoice::English, configured_dir).is_ok() {
        ModelChoice::English
    } else if find_model_dir(ModelChoice::Multilingual, configured_dir).is_ok() {
        ModelChoice::Multilingual
    } else {
        ModelChoice::English
    };
    log_info(&format!("No speech model chosen yet; selected {}", choice.label()));
    choice
}

pub(crate) fn has_model_files(dir: &std::path::Path) -> bool {
    dir.join("encoder.onnx").exists()
        && dir.join("decoder_joint.onnx").exists()
        && dir.join("tokenizer.model").exists()
}

pub(crate) fn find_model_dir(choice: ModelChoice, configured_dir: &str) -> Result<PathBuf> {
    let Some(folder_name) = choice.folder_name() else {
        // Predefined: only the configured folders, never the standard ones.
        let dirs = predefined_model_dirs(configured_dir);
        if dirs.is_empty() {
            return Err(anyhow!(
                "No predefined model folder is set.\n\nSet the NEMOTRON_MODEL_DIR environment \
                 variable or \"model_dir\" in {}, or choose English or Multilingual in the settings.",
                settings_path().display()
            ));
        }
        return dirs.iter().find(|dir| has_model_files(dir)).cloned().ok_or_else(|| {
            anyhow!(
                "The predefined model folder does not contain the Nemotron ONNX files:\n{}",
                dirs.iter()
                    .map(|p| format!("  - {}", p.display()))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        });
    };

    let mut candidates = Vec::<PathBuf>::new();
    let mut search_roots = Vec::<PathBuf>::new();
    if let Ok(cwd) = env::current_dir() {
        search_roots.push(cwd);
    }
    // Walk up from the executable so the model is found no matter the working
    // directory. This matters for "Start with Windows", where Windows launches us
    // with the working directory set to System32 — and for the cargo layout, where
    // the exe sits in target\release\ but the model is in the project root.
    if let Ok(exe) = env::current_exe() {
        let mut dir = exe.parent();
        for _ in 0..5 {
            let Some(d) = dir else { break };
            search_roots.push(d.to_path_buf());
            dir = d.parent();
        }
    }
    for root in &search_roots {
        let candidate = root.join("models").join(folder_name);
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }

    if let Some(found) = candidates.iter().find(|dir| has_model_files(dir)) {
        return Ok(found.clone());
    }

    let hint = if choice == ModelChoice::Multilingual {
        "The multilingual model is not installed. Download it on the Model tab of the \
         application window, or pick another model there.\n\n"
    } else {
        "The English model is not installed. Download it on the Model tab of the \
         application window, or pick another model there.\n\n"
    };
    Err(anyhow!(
        "{hint}Expected Nemotron ONNX files in one of these folders:\n{}",
        candidates
            .iter()
            .map(|p| format!("  - {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

