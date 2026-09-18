use super::*;

#[tauri::command]
pub(super) fn get_queue_status(state: State<AppState>) -> Result<QueueStatus, String> {
    snapshot_queue_status(state.inner())
}

#[tauri::command]
pub(super) fn get_queue(state: State<AppState>) -> Result<Vec<DownloadJob>, String> {
    let queue = state.queue.lock().map_err(|_| "Queue lock poisoned")?;
    Ok(queue.iter().cloned().collect())
}

#[tauri::command]
pub(super) fn set_queue_auto_start(
    app: AppHandle,
    state: State<AppState>,
    enabled: bool,
) -> Result<QueueStatus, String> {
    {
        let mut auto_start = state
            .queue_auto_start
            .lock()
            .map_err(|_| "Queue auto-start lock poisoned")?;
        *auto_start = enabled;
    }

    if enabled {
        ensure_worker(&app, state.inner())?;
    }
    emit_queue_status(&app, state.inner());

    snapshot_queue_status(state.inner())
}

#[tauri::command]
pub(super) fn start_queue(app: AppHandle, state: State<AppState>) -> Result<QueueStatus, String> {
    set_queue_paused(state.inner(), false)?;
    ensure_worker(&app, state.inner())?;
    emit_queue_status(&app, state.inner());
    snapshot_queue_status(state.inner())
}

#[tauri::command]
pub(super) fn pause_queue(app: AppHandle, state: State<AppState>) -> Result<QueueStatus, String> {
    set_queue_paused(state.inner(), true)?;
    emit_queue_status(&app, state.inner());
    snapshot_queue_status(state.inner())
}

#[tauri::command]
pub(super) fn resume_queue(app: AppHandle, state: State<AppState>) -> Result<QueueStatus, String> {
    set_queue_paused(state.inner(), false)?;
    ensure_worker(&app, state.inner())?;
    emit_queue_status(&app, state.inner());
    snapshot_queue_status(state.inner())
}

pub(super) fn set_queue_paused(state: &AppState, paused: bool) -> Result<(), String> {
    let mut value = state
        .queue_paused
        .lock()
        .map_err(|_| "Queue pause lock poisoned")?;
    *value = paused;
    Ok(())
}

#[tauri::command]
pub(super) fn enqueue_download(
    app: AppHandle,
    state: State<AppState>,
    request: DownloadRequest,
) -> Result<String, String> {
    enqueue_download_request(&app, state.inner(), request)
}

pub(super) fn enqueue_download_request(
    app: &AppHandle,
    state: &AppState,
    request: DownloadRequest,
) -> Result<String, String> {
    let job = build_download_job(state, request)?;
    let id = job.id.clone();
    enqueue_download_jobs(app, state, vec![job])?;
    Ok(id)
}

pub(super) fn build_download_job(
    state: &AppState,
    request: DownloadRequest,
) -> Result<DownloadJob, String> {
    if !is_valid_url(&request.url) {
        return Err("URL must start with http:// or https://".to_string());
    }

    let output_dir = resolve_output_dir(state, request.output_dir.clone())?;
    let cut_start_time = resolve_cut_start_time(
        request.cut_at_timestamp_enabled,
        request.cut_start_time,
        &request.url,
    );
    let (faster_whisper_model, download_video_with_transcript, save_captions, save_thumbnails) = {
        let cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
        (
            normalize_faster_whisper_model(&cfg.faster_whisper_model),
            cfg.download_video_with_transcript,
            cfg.save_captions,
            cfg.save_thumbnails,
        )
    };
    let id = Uuid::new_v4().to_string();
    Ok(DownloadJob {
        id: id.clone(),
        url: request.url,
        format: request.format,
        output_dir,
        extract_audio: request.extract_audio,
        audio_format: request.audio_format,
        transcribe_text: request.transcribe_text,
        transcribe_timestamps: request.transcribe_timestamps,
        faster_whisper_model,
        download_video_with_transcript,
        save_captions,
        save_thumbnails,
        title: request.title,
        uploader: request.uploader,
        thumbnail: request.thumbnail,
        upload_date: request.upload_date,
        timestamp: request.timestamp,
        duration_seconds: request.duration_seconds,
        cut_start_time,
        filename_suffix: normalize_filename_suffix(request.filename_suffix.as_deref()),
    })
}

pub(super) fn enqueue_download_jobs(
    app: &AppHandle,
    state: &AppState,
    jobs: Vec<DownloadJob>,
) -> Result<Vec<String>, String> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }

    let ids = jobs.iter().map(|job| job.id.clone()).collect::<Vec<_>>();

    {
        let mut queue = state.queue.lock().map_err(|_| "Queue lock poisoned")?;
        queue.extend(jobs);
    }

    emit_queue(app, state)?;
    if is_queue_auto_start_enabled(state)? {
        ensure_worker(app, state)?;
    }
    Ok(ids)
}

#[tauri::command]
pub(super) fn cancel_download(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<(), String> {
    let removed = {
        let mut queue = state.queue.lock().map_err(|_| "Queue lock poisoned")?;
        let before = queue.len();
        queue.retain(|job| job.id != id);
        before != queue.len()
    };

    if removed {
        emit_queue(&app, &state)?;
        emit_state(
            &app,
            DownloadStateEvent {
                id,
                state: "cancelled".to_string(),
                exit_code: None,
                error: None,
                output_path: None,
            },
        );
        return Ok(());
    }

    {
        let current = state
            .current_job_id
            .lock()
            .map_err(|_| "Current job lock poisoned")?;
        if current.as_deref() != Some(&id) {
            return Err("Job not found in queue".to_string());
        }
        let mut cancel = state
            .cancel_requested
            .lock()
            .map_err(|_| "Cancel lock poisoned")?;
        *cancel = Some(id.clone());
    }

    let child = {
        let child_guard = state
            .current_child
            .lock()
            .map_err(|_| "Child lock poisoned")?;
        child_guard.clone()
    };

    if let Some(child) = child {
        let kill_result = child
            .lock()
            .map_err(|_| "Child lock poisoned".to_string())
            .and_then(|mut process| {
                terminate_child_process_tree(&mut process).map_err(|err| err.to_string())
            });
        if let Err(err) = kill_result {
            if let Ok(mut cancel) = state.cancel_requested.lock() {
                if cancel.as_deref() == Some(&id) {
                    *cancel = None;
                }
            }
            return Err(format!("Could not cancel process: {err}"));
        }
    }

    emit_state(
        &app,
        DownloadStateEvent {
            id,
            state: "cancelling".to_string(),
            exit_code: None,
            error: None,
            output_path: None,
        },
    );
    Ok(())
}

pub(super) fn stop_active_download_on_exit(state: &AppState) {
    state.shutting_down.store(true, Ordering::SeqCst);
    if let Ok(mut paused) = state.queue_paused.lock() {
        *paused = true;
    }
    if let Ok(current) = state.current_job_id.lock() {
        if let Some(id) = current.as_ref() {
            if let Ok(mut cancel) = state.cancel_requested.lock() {
                *cancel = Some(id.clone());
            }
        }
    }
    let child = state
        .current_child
        .lock()
        .ok()
        .and_then(|slot| slot.clone());
    if let Some(child) = child {
        if let Ok(mut process) = child.lock() {
            let _ = terminate_child_process_tree(&mut process);
            let _ = process.wait();
        }
    }
    let utility_children = state
        .utility_children
        .lock()
        .map(|mut children| std::mem::take(&mut *children))
        .unwrap_or_default();
    for child in utility_children {
        if let Ok(mut process) = child.lock() {
            let _ = terminate_child_process_tree(&mut process);
            let _ = process.wait();
        }
    }
    let worker = state
        .worker_handle
        .lock()
        .ok()
        .and_then(|mut slot| slot.take());
    let deadline = Instant::now() + Duration::from_secs(5);
    if let Some(worker) = worker {
        while !worker.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        if worker.is_finished() {
            let _ = worker.join();
        } else {
            eprintln!("PineFetch queue worker did not stop within five seconds");
        }
    } else {
        while state
            .worker_running
            .lock()
            .map(|running| *running)
            .unwrap_or(false)
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(50));
        }
    }
}

pub(super) fn job_cancel_requested(state: &AppState, id: &str) -> bool {
    state
        .cancel_requested
        .lock()
        .map(|requested| requested.as_deref() == Some(id))
        .unwrap_or(false)
}

pub(super) fn finish_active_job(
    app: &AppHandle,
    state: &AppState,
    event: DownloadStateEvent,
) -> bool {
    finish_active_job_with_side_effect(app, state, event, || None)
}

pub(super) fn finish_active_job_with_side_effect(
    app: &AppHandle,
    state: &AppState,
    mut event: DownloadStateEvent,
    after_success: impl FnOnce() -> Option<String>,
) -> bool {
    if let Ok(mut current) = state.current_job_id.lock() {
        if let Ok(mut cancel) = state.cancel_requested.lock() {
            if cancel.as_deref() == Some(event.id.as_str()) {
                *cancel = None;
                event.state = "cancelled".to_string();
                event.error = None;
                event.output_path = None;
            }
        }
        if event.state == "success" {
            event.error = after_success();
        }
        *current = None;
        if let Ok(mut active_key) = state.active_video_key.lock() {
            *active_key = None;
        }
    }
    let succeeded = event.state == "success";
    emit_state(app, event);
    succeeded
}

pub(super) fn snapshot_queue_status(state: &AppState) -> Result<QueueStatus, String> {
    let auto_start = *state
        .queue_auto_start
        .lock()
        .map_err(|_| "Queue auto-start lock poisoned")?;
    let worker_running = *state
        .worker_running
        .lock()
        .map_err(|_| "Worker lock poisoned")?;
    let paused = *state
        .queue_paused
        .lock()
        .map_err(|_| "Queue pause lock poisoned")?;

    Ok(QueueStatus {
        auto_start,
        worker_running,
        paused,
    })
}

pub(super) fn emit_queue_status(app: &AppHandle, state: &AppState) {
    if let Ok(status) = snapshot_queue_status(state) {
        let _ = app.emit("queue:status", status);
    }
}

pub(super) fn is_queue_auto_start_enabled(state: &AppState) -> Result<bool, String> {
    let auto_start = state
        .queue_auto_start
        .lock()
        .map_err(|_| "Queue auto-start lock poisoned")?;
    Ok(*auto_start)
}

#[derive(Default)]
pub(super) struct QueueRunSummary {
    pub(super) started: usize,
    pub(super) succeeded: usize,
}

impl QueueRunSummary {
    pub(super) fn should_notify(&self, enabled: bool) -> bool {
        enabled && self.started > 1 && self.succeeded == self.started
    }
}

pub(super) fn notify_queue_completed(app: &AppHandle, state: &AppState, summary: &QueueRunSummary) {
    let enabled = state
        .config
        .lock()
        .map(|config| config.notifications_enabled)
        .unwrap_or(false);
    if !summary.should_notify(enabled) {
        return;
    }
    let body = format!("All {} downloads finished successfully.", summary.succeeded);
    if let Err(err) = show_queue_notification(app, &body) {
        emit_log(
            app,
            LogEvent {
                id: String::new(),
                line: format!("[notification] {err}"),
                is_error: true,
            },
        );
    }
}

pub(super) fn emit_history_warning(app: &AppHandle, job_id: &str, warning: &str) {
    emit_log(
        app,
        LogEvent {
            id: job_id.to_string(),
            line: format!("[history] {warning}"),
            is_error: true,
        },
    );
}

fn store_captions_with_warning(
    app: &AppHandle,
    state: &AppState,
    job_id: &str,
    history_entry_id: &str,
    captions: &[SavedCaption],
) -> Option<String> {
    insert_captions_in_db(state, history_entry_id, captions)
        .err()
        .map(|err| {
            let warning = format!("Caption save failed: {err}");
            emit_history_warning(app, job_id, &warning);
            warning
        })
}

pub(super) fn show_queue_notification(app: &AppHandle, body: &str) -> Result<(), String> {
    let icon = app
        .path()
        .resolve("icons/icon.png", tauri::path::BaseDirectory::Resource)
        .ok()
        .filter(|path| path.is_file())
        .ok_or("Bundled notification icon unavailable")?;
    let icon = icon.to_string_lossy();
    let title = "PineFetch — Queue complete";

    #[cfg(target_os = "macos")]
    {
        // Tauri's notification wrapper ignores custom icons on macOS. Use the
        // native app_icon option, retaining Tauri's delivery identity in dev.
        let identifier = if cfg!(feature = "custom-protocol") {
            app.config().identifier.clone()
        } else {
            "com.apple.Terminal".to_string()
        };
        match mac_notification_sys::set_application(&identifier) {
            Ok(()) => {}
            Err(mac_notification_sys::error::Error::Application(
                mac_notification_sys::error::ApplicationError::AlreadySet(_),
            )) => {}
            Err(err) => return Err(err.to_string()),
        }
        mac_notification_sys::Notification::new()
            .title(title)
            .message(body)
            .app_icon(&icon)
            .send()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_notification::NotificationExt;
        app.notification()
            .builder()
            .title(title)
            .body(body)
            .icon(icon.to_string())
            .show()
            .map_err(|err| err.to_string())
    }
}

pub(super) fn ensure_worker(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let paused = state
        .queue_paused
        .lock()
        .map_err(|_| "Queue pause lock poisoned")?;
    if *paused {
        return Ok(());
    }
    let mut running = state
        .worker_running
        .lock()
        .map_err(|_| "Worker lock poisoned")?;
    if *running {
        return Ok(());
    }
    *running = true;
    drop(running);
    drop(paused);
    emit_queue_status(app, state);

    let app_handle = app.clone();

    let handle = thread::spawn(move || {
        let mut summary = QueueRunSummary::default();
        loop {
            let state_handle = app_handle.state::<AppState>();
            let (job_opt, paused) = match next_worker_job(&state_handle) {
                Ok(next) => next,
                Err(_) => break,
            };

            let job = match job_opt {
                Some(job) => job,
                None => {
                    if !paused {
                        notify_queue_completed(&app_handle, &state_handle, &summary);
                    }
                    let _ = emit_queue(&app_handle, &state_handle);
                    emit_queue_status(&app_handle, &state_handle);
                    break;
                }
            };

            // The waiting queue no longer includes this active job. Publish the
            // new snapshot before its state changes so counts stay accurate.
            let _ = emit_queue(&app_handle, &state_handle);

            summary.started += 1;

            emit_state(
                &app_handle,
                DownloadStateEvent {
                    id: job.id.clone(),
                    state: "downloading".to_string(),
                    exit_code: None,
                    error: None,
                    output_path: None,
                },
            );

            let result = run_download_job(&app_handle, &state_handle, &job);

            match result {
                Ok(run_result) => {
                    if job_cancel_requested(&state_handle, &job.id) {
                        finish_active_job(
                            &app_handle,
                            &state_handle,
                            DownloadStateEvent {
                                id: job.id.clone(),
                                state: "cancelled".to_string(),
                                exit_code: Some(run_result.exit_code),
                                error: None,
                                output_path: None,
                            },
                        );
                    } else if run_result.exit_code != 0 {
                        finish_active_job(
                            &app_handle,
                            &state_handle,
                            DownloadStateEvent {
                                id: job.id.clone(),
                                state: "error".to_string(),
                                exit_code: Some(run_result.exit_code),
                                error: Some(run_result.error.unwrap_or_else(|| {
                                    format!("yt-dlp failed (exit code {})", run_result.exit_code)
                                })),
                                output_path: None,
                            },
                        );
                    } else if job.transcribe_text {
                        if job.download_video_with_transcript {
                            if let Some(video_path) = run_result.output_path.as_deref() {
                                emit_log(
                                    &app_handle,
                                    LogEvent {
                                        id: job.id.clone(),
                                        line: format!("[video] saved: {video_path}"),
                                        is_error: false,
                                    },
                                );
                            }
                        }
                        emit_state(
                            &app_handle,
                            DownloadStateEvent {
                                id: job.id.clone(),
                                state: "transcribing".to_string(),
                                exit_code: Some(run_result.exit_code),
                                error: None,
                                output_path: if job.download_video_with_transcript {
                                    run_result.output_path.clone()
                                } else {
                                    None
                                },
                            },
                        );

                        match run_faster_whisper_transcription(
                            &app_handle,
                            &state_handle,
                            &job,
                            run_result.output_path.as_deref(),
                        ) {
                            Ok(transcript_path) => {
                                emit_log(
                                    &app_handle,
                                    LogEvent {
                                        id: job.id.clone(),
                                        line: format!("[transcript] saved: {transcript_path}"),
                                        is_error: false,
                                    },
                                );
                                if finish_active_job_with_side_effect(
                                    &app_handle,
                                    &state_handle,
                                    DownloadStateEvent {
                                        id: job.id.clone(),
                                        state: "success".to_string(),
                                        exit_code: Some(run_result.exit_code),
                                        error: None,
                                        output_path: Some(transcript_path.clone()),
                                    },
                                    || match add_history_entry_on_success(
                                        &app_handle,
                                        &state_handle,
                                        &job,
                                        Some(&transcript_path),
                                        run_result.info.as_ref(),
                                    ) {
                                        Ok(history_entry_id) => {
                                            let mut warnings = Vec::new();
                                            if let Err(err) = store_transcription_for_history_entry(
                                                &state_handle,
                                                &job,
                                                &history_entry_id,
                                                &transcript_path,
                                            ) {
                                                let warning =
                                                    format!("Transcript index save failed: {err}");
                                                emit_history_warning(
                                                    &app_handle,
                                                    &job.id,
                                                    &warning,
                                                );
                                                warnings.push(warning);
                                            }
                                            if let Some(warning) = store_captions_with_warning(
                                                &app_handle,
                                                &state_handle,
                                                &job.id,
                                                &history_entry_id,
                                                &run_result.captions,
                                            ) {
                                                warnings.push(warning);
                                            }
                                            (!warnings.is_empty()).then(|| warnings.join("; "))
                                        }
                                        Err(err) => {
                                            let warning = format!("History save failed: {err}");
                                            emit_history_warning(&app_handle, &job.id, &warning);
                                            Some(warning)
                                        }
                                    },
                                ) {
                                    summary.succeeded += 1;
                                }
                            }
                            Err(err) => {
                                finish_active_job(
                                    &app_handle,
                                    &state_handle,
                                    DownloadStateEvent {
                                        id: job.id.clone(),
                                        state: "error".to_string(),
                                        exit_code: Some(run_result.exit_code),
                                        error: Some(err),
                                        output_path: None,
                                    },
                                );
                            }
                        }
                    } else {
                        if finish_active_job_with_side_effect(
                            &app_handle,
                            &state_handle,
                            DownloadStateEvent {
                                id: job.id.clone(),
                                state: "success".to_string(),
                                exit_code: Some(run_result.exit_code),
                                error: None,
                                output_path: run_result.output_path.clone(),
                            },
                            || match add_history_entry_on_success(
                                &app_handle,
                                &state_handle,
                                &job,
                                run_result.output_path.as_deref(),
                                run_result.info.as_ref(),
                            ) {
                                Ok(history_entry_id) => store_captions_with_warning(
                                    &app_handle,
                                    &state_handle,
                                    &job.id,
                                    &history_entry_id,
                                    &run_result.captions,
                                ),
                                Err(err) => {
                                    let warning = format!("History save failed: {err}");
                                    emit_history_warning(&app_handle, &job.id, &warning);
                                    Some(warning)
                                }
                            },
                        ) {
                            summary.succeeded += 1;
                        }
                    }
                }
                Err(err) => {
                    finish_active_job(
                        &app_handle,
                        &state_handle,
                        DownloadStateEvent {
                            id: job.id.clone(),
                            state: "error".to_string(),
                            exit_code: None,
                            error: Some(err),
                            output_path: None,
                        },
                    );
                }
            }

            let _ = emit_queue(&app_handle, &state_handle);
        }
    });
    if let Ok(mut slot) = state.worker_handle.lock() {
        if let Some(previous) = slot.replace(handle) {
            if previous.is_finished() {
                let _ = previous.join();
            }
        }
    }

    Ok(())
}

pub(super) fn next_worker_job(state: &AppState) -> Result<(Option<DownloadJob>, bool), String> {
    let pause_guard = state
        .queue_paused
        .lock()
        .map_err(|_| "Queue pause lock poisoned")?;
    let paused = *pause_guard;
    let mut queue = state.queue.lock().map_err(|_| "Queue lock poisoned")?;
    let job = if paused { None } else { queue.pop_front() };
    if let Some(job) = job.as_ref() {
        let mut current = state
            .current_job_id
            .lock()
            .map_err(|_| "Current job lock poisoned")?;
        *current = Some(job.id.clone());
        let mut active_key = state
            .active_video_key
            .lock()
            .map_err(|_| "Active video key lock poisoned")?;
        *active_key = normalize_video_url(&job.url).map(|normalized| normalized.key);
    }
    // Keep the queue locked until idle is published so an enqueue or resume
    // cannot miss starting a worker between the empty check and shutdown.
    if job.is_none() {
        let mut running = state
            .worker_running
            .lock()
            .map_err(|_| "Worker lock poisoned")?;
        *running = false;
    }
    Ok((job, paused))
}

pub(super) fn emit_queue(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let queue = state.queue.lock().map_err(|_| "Queue lock poisoned")?;
    app.emit("queue:update", queue.clone())
        .map_err(|e| format!("Emit queue failed: {e}"))
}

pub(super) fn emit_progress(app: &AppHandle, progress: DownloadProgress) {
    let _ = app.emit("download:progress", progress);
}

pub(super) fn emit_state(app: &AppHandle, state: DownloadStateEvent) {
    let _ = app.emit("download:state", state);
}

pub(super) fn emit_log(app: &AppHandle, log: LogEvent) {
    let _ = app.emit("download:log", log);
}
