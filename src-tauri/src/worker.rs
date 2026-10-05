use crate::completion;
use crate::download::run_download_job;
use crate::download::run_faster_whisper_transcription;
use crate::download_rules::is_valid_url;
use crate::download_rules::{prepare_download_job, DownloadOptions};
use crate::events::{emit_log, emit_queue, emit_queue_status, emit_state};
use crate::models::DownloadJob;
use crate::models::DownloadRequest;
use crate::models::DownloadState;
use crate::models::DownloadStateEvent;
use crate::models::InfoResponse;
use crate::models::LogEvent;
use crate::models::SavedCaption;
use crate::process::terminate_child_process_tree;
use crate::queue::is_queue_auto_start_enabled;
use crate::queue::next_worker_job;
use crate::queue::QueueRunSummary;
use crate::runtime::resolve_output_dir;
use crate::state::AppState;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use std::time::Instant;
use tauri::AppHandle;
use tauri::Emitter;
use tauri::Manager;
#[cfg(not(target_os = "macos"))]
use tauri_plugin_notification::NotificationExt;
use uuid::Uuid;

pub(super) fn enqueue_download_request(
    app: &AppHandle,
    state: &AppState,
    request: DownloadRequest,
) -> Result<String, String> {
    let job = build_download_job(&state.config, request)?;
    let id = job.id.clone();
    enqueue_download_jobs(app, state, vec![job])?;
    Ok(id)
}

pub(super) fn build_download_job(
    state: &crate::config::ConfigState,
    request: DownloadRequest,
) -> Result<DownloadJob, String> {
    if !is_valid_url(&request.url) {
        return Err("URL must start with http:// or https://".to_string());
    }
    let output_dir = resolve_output_dir(state, request.output_dir.clone())?;
    let options = {
        let config = state.lock().map_err(|_| "Config lock poisoned")?;
        DownloadOptions::from(&*config)
    };
    Ok(prepare_download_job(
        request,
        options,
        output_dir,
        Uuid::new_v4().to_string(),
    ))
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

    crate::queue::enqueue_jobs(&state.queue, jobs)?;

    emit_queue(app, &state.queue)?;
    if is_queue_auto_start_enabled(&state.queue)? {
        ensure_worker(app, state)?;
    }
    Ok(ids)
}

pub(super) fn stop_active_download_on_exit(state: &AppState) {
    state.processes.shutting_down.store(true, Ordering::SeqCst);
    if let Ok(mut paused) = state.queue.paused.lock() {
        *paused = true;
    }
    if let Ok(current) = state.processes.current_job_id.lock() {
        if let Some(id) = current.as_ref() {
            if let Ok(mut cancel) = state.processes.cancel_requested.lock() {
                *cancel = Some(id.clone());
            }
        }
    }
    let child = state
        .processes
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
        .processes
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
        .queue
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
            .queue
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
        .processes
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
    finish_active_job_with_side_effect(app, state, event, || Ok(()))
}

pub(super) fn finish_active_job_with_side_effect(
    app: &AppHandle,
    state: &AppState,
    event: DownloadStateEvent,
    after_success: impl FnOnce() -> Result<(), String>,
) -> bool {
    let id = event.id.clone();
    let event =
        crate::queue::finalize_active_job_event(&state.processes, &state.queue, event, || {
            let result = after_success();
            if let Err(error) = &result {
                emit_history_warning(app, &id, error);
            }
            result
        });
    let succeeded = event.state == DownloadState::Success;
    emit_state(app, event);
    succeeded
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

fn persist_job_completion(
    app: &AppHandle,
    state: &AppState,
    job: &DownloadJob,
    path: Option<&str>,
    info: Option<&InfoResponse>,
    captions: &[SavedCaption],
    language: Option<&str>,
) -> Result<(), String> {
    completion::complete(&state.db, job, path, info, captions, language)?;
    let _ = app.emit(crate::events::HISTORY_CHANGED, ());
    Ok(())
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
        .queue
        .paused
        .lock()
        .map_err(|_| "Queue pause lock poisoned")?;
    if *paused {
        return Ok(());
    }
    let mut running = state
        .queue
        .worker_running
        .lock()
        .map_err(|_| "Worker lock poisoned")?;
    if *running {
        return Ok(());
    }
    *running = true;
    drop(running);
    drop(paused);
    emit_queue_status(app, &state.queue);

    let app_handle = app.clone();

    let handle = thread::spawn(move || {
        let mut summary = QueueRunSummary::default();
        loop {
            let state_handle = app_handle.state::<AppState>();
            let (job_opt, paused) = match next_worker_job(
                &state_handle.queue,
                &state_handle.processes.current_job_id,
            ) {
                Ok(next) => next,
                Err(_) => break,
            };

            let job = match job_opt {
                Some(job) => job,
                None => {
                    if !paused {
                        notify_queue_completed(&app_handle, &state_handle, &summary);
                    }
                    let _ = emit_queue(&app_handle, &state_handle.queue);
                    emit_queue_status(&app_handle, &state_handle.queue);
                    break;
                }
            };

            // The waiting queue no longer includes this active job. Publish the
            // new snapshot before its state changes so counts stay accurate.
            let _ = emit_queue(&app_handle, &state_handle.queue);

            summary.started += 1;

            emit_state(
                &app_handle,
                DownloadStateEvent {
                    id: job.id.clone(),
                    state: DownloadState::Downloading,
                    exit_code: None,
                    error: None,
                    output_path: None,
                },
            );

            let result = completion::begin(&state_handle.db, &job)
                .and_then(|()| run_download_job(&app_handle, &state_handle, &job));

            match result {
                Ok(run_result) => {
                    if job_cancel_requested(&state_handle, &job.id) {
                        finish_active_job(
                            &app_handle,
                            &state_handle,
                            DownloadStateEvent {
                                id: job.id.clone(),
                                state: DownloadState::Cancelled,
                                exit_code: Some(run_result.exit_code),
                                error: run_result
                                    .output_path
                                    .as_ref()
                                    .map(|_| "Cancelled; produced output file preserved".into()),
                                output_path: run_result.output_path.clone(),
                            },
                        );
                    } else if run_result.exit_code != 0 {
                        finish_active_job(
                            &app_handle,
                            &state_handle,
                            DownloadStateEvent {
                                id: job.id.clone(),
                                state: DownloadState::Error,
                                exit_code: Some(run_result.exit_code),
                                error: Some(format!(
                                    "{}. Existing output files preserved.",
                                    run_result.error.unwrap_or_else(|| {
                                        format!(
                                            "yt-dlp failed (exit code {})",
                                            run_result.exit_code
                                        )
                                    })
                                )),
                                output_path: run_result.output_path.clone(),
                            },
                        );
                    } else if let Err(err) = completion::output_ready(
                        &state_handle.db,
                        &job,
                        run_result.output_path.as_deref(),
                    ) {
                        finish_active_job(&app_handle, &state_handle, DownloadStateEvent {
                            id: job.id.clone(), state: DownloadState::Error, exit_code: Some(run_result.exit_code),
                            error: Some(format!("Output completion receipt failed: {err}. Existing output file preserved.")),
                            output_path: run_result.output_path.clone(),
                        });
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
                                state: DownloadState::Transcribing,
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
                            Ok(transcription) => {
                                let transcript_path = transcription.transcript_path;
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
                                        state: DownloadState::Success,
                                        exit_code: Some(run_result.exit_code),
                                        error: None,
                                        output_path: Some(transcript_path.clone()),
                                    },
                                    || {
                                        persist_job_completion(
                                            &app_handle,
                                            &state_handle,
                                            &job,
                                            Some(&transcript_path),
                                            run_result.info.as_ref(),
                                            &run_result.captions,
                                            Some(&transcription.language),
                                        )
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
                                        state: DownloadState::Error,
                                        exit_code: Some(run_result.exit_code),
                                        error: Some(format!("Transcription failed: {err}. Existing media/output files preserved.")),
                                        output_path: run_result.output_path.clone(),
                                    },
                                );
                            }
                        }
                    } else if finish_active_job_with_side_effect(
                        &app_handle,
                        &state_handle,
                        DownloadStateEvent {
                            id: job.id.clone(),
                            state: DownloadState::Success,
                            exit_code: Some(run_result.exit_code),
                            error: None,
                            output_path: run_result.output_path.clone(),
                        },
                        || {
                            persist_job_completion(
                                &app_handle,
                                &state_handle,
                                &job,
                                run_result.output_path.as_deref(),
                                run_result.info.as_ref(),
                                &run_result.captions,
                                None,
                            )
                        },
                    ) {
                        summary.succeeded += 1;
                    }
                }
                Err(err) => {
                    let (output_path, receipt_error) =
                        match completion::known_output(&state_handle.db, &job.id) {
                            Ok(path) => (path, String::new()),
                            Err(read_error) => {
                                (None, format!(" Output location unavailable: {read_error}."))
                            }
                        };
                    finish_active_job(
                        &app_handle,
                        &state_handle,
                        DownloadStateEvent {
                            id: job.id.clone(),
                            state: DownloadState::Error,
                            exit_code: None,
                            error: Some(format!("Processing failed: {err}. Existing output files preserved.{receipt_error}")),
                            output_path,
                        },
                    );
                }
            }

            let _ = emit_queue(&app_handle, &state_handle.queue);
        }
    });
    if let Ok(mut slot) = state.queue.worker_handle.lock() {
        if let Some(previous) = slot.replace(handle) {
            if previous.is_finished() {
                let _ = previous.join();
            }
        }
    }

    Ok(())
}

pub(crate) fn cancel_download_job(
    app: AppHandle,
    state: &AppState,
    id: String,
) -> Result<(), String> {
    let removed = crate::queue::remove_queued_job(&state.queue, &id)?;

    if removed {
        emit_queue(&app, &state.queue)?;
        emit_state(
            &app,
            DownloadStateEvent {
                id,
                state: DownloadState::Cancelled,
                exit_code: None,
                error: None,
                output_path: None,
            },
        );
        return Ok(());
    }

    {
        let current = state
            .processes
            .current_job_id
            .lock()
            .map_err(|_| "Current job lock poisoned")?;
        if current.as_deref() != Some(&id) {
            return Err("Job not found in queue".to_string());
        }
        let mut cancel = state
            .processes
            .cancel_requested
            .lock()
            .map_err(|_| "Cancel lock poisoned")?;
        *cancel = Some(id.clone());
    }

    let child = {
        let child_guard = state
            .processes
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
            if let Ok(mut cancel) = state.processes.cancel_requested.lock() {
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
            state: DownloadState::Cancelling,
            exit_code: None,
            error: None,
            output_path: None,
        },
    );
    Ok(())
}
