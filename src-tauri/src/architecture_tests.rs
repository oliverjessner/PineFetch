//! Boundary regressions: no AppHandle, desktop, SQLite or live downloads.
use crate::download_rules::{build_output_template, prepare_download_job, DownloadOptions};
use crate::models::{AppConfig, DownloadJob, DownloadRequest, DownloadState, DownloadStateEvent};
use crate::process::{run_command_output, ProcessError, ProcessState};
use crate::queue::{
    enqueue_jobs, finalize_active_job_event, next_worker_job, remove_queued_job, set_queue_paused,
    snapshot_queue_status, QueueState,
};
use crate::yt_dlp::{build_download_args, parse_info_json, parse_progress_line};
use serde_json::json;
use std::cell::Cell;
use std::process::Command;
use std::sync::Mutex;

fn request() -> DownloadRequest {
    serde_json::from_value(json!({
        "url": "https://www.youtube.com/watch?v=synthetic&t=42",
        "format": "best", "extract_audio": false, "transcribe_text": false
    }))
    .unwrap()
}

fn job(id: &str) -> DownloadJob {
    prepare_download_job(
        request(),
        DownloadOptions::from(&AppConfig::default()),
        "/synthetic/Grüße 🌲".into(),
        id.into(),
    )
}

fn event(status: DownloadState) -> DownloadStateEvent {
    DownloadStateEvent {
        id: "one".into(),
        state: status,
        exit_code: Some(0),
        error: None,
        output_path: Some("/synthetic/output.mp4".into()),
    }
}

#[test]
fn job_preparation_uses_explicit_configuration_and_retains_request_metadata() {
    let mut req = request();
    req.title = Some("Grüße 🌲".into());
    req.cut_start_time = Some(12.5);
    req.filename_suffix = Some("__max".into());
    let config = AppConfig {
        faster_whisper_model: "small".into(),
        download_video_with_transcript: true,
        save_captions: true,
        save_thumbnails: true,
        ..AppConfig::default()
    };
    let prepared = prepare_download_job(
        req,
        DownloadOptions::from(&config),
        "/synthetic/explicit output".into(),
        "stable-id".into(),
    );
    assert_eq!(prepared.id, "stable-id");
    assert_eq!(prepared.output_dir, "/synthetic/explicit output");
    assert_eq!(prepared.title.as_deref(), Some("Grüße 🌲"));
    assert_eq!(prepared.faster_whisper_model, "small");
    assert_eq!(prepared.cut_start_time, Some(12.5));
    assert_eq!(prepared.filename_suffix.as_deref(), Some("__max"));
    assert!(
        prepared.download_video_with_transcript
            && prepared.save_captions
            && prepared.save_thumbnails
    );
    let fallback = job("other");
    assert_eq!(fallback.cut_start_time, Some(42.0));
    let mut disabled = request();
    disabled.cut_at_timestamp_enabled = false;
    assert_eq!(
        prepare_download_job(
            disabled,
            DownloadOptions::from(&config),
            "/synthetic".into(),
            "no-cut".into()
        )
        .cut_start_time,
        None
    );
}

#[test]
fn download_arguments_keep_paths_and_urls_as_separate_arguments() {
    let mut prepared = job("one");
    prepared.extract_audio = true;
    prepared.audio_format = Some("mp3".into());
    prepared.save_thumbnails = true;
    let template = build_output_template(&prepared.output_dir, None);
    let args = build_download_args(
        &prepared,
        template.clone(),
        Some("/synthetic/FFmpeg tools"),
        Some("/synthetic/Deno runtime"),
    )
    .unwrap();
    let value_after = |flag| {
        args.windows(2)
            .find(|pair| pair[0] == flag)
            .map(|pair| pair[1].as_str())
    };
    assert_eq!(value_after("-o"), Some(template.as_str()));
    assert_eq!(
        value_after("--ffmpeg-location"),
        Some("/synthetic/FFmpeg tools")
    );
    assert_eq!(
        value_after("--js-runtimes"),
        Some("deno:/synthetic/Deno runtime")
    );
    assert_eq!(value_after("--audio-format"), Some("mp3"));
    assert_eq!(args.last().unwrap(), &prepared.url);
    assert!(args.iter().any(|arg| arg == "--write-thumbnail"));
    assert!(args.iter().any(|arg| arg == "--extract-audio"));
    assert!(build_download_args(&prepared, template, None, None)
        .unwrap_err()
        .starts_with("ffmpeg and ffprobe not found."));
}

#[test]
fn metadata_and_progress_parsing_need_no_runtime() {
    let info = parse_info_json(r#"{"title":"Grüße 🌲","uploader_id":"synthetic","duration":"12","release_date":"20200101","release_timestamp":123.5,"formats":[{"format_id":"one","height":1080,"fps":30.0}]}"#).unwrap();
    assert_eq!(info.title.as_deref(), Some("Grüße 🌲"));
    assert_eq!(info.uploader.as_deref(), Some("synthetic"));
    assert_eq!(info.duration, Some(12));
    assert_eq!(info.upload_date.as_deref(), Some("20200101"));
    assert_eq!(info.timestamp, Some(123));
    assert_eq!(info.formats.unwrap()[0].height, Some(1080));
    assert!(parse_info_json("not JSON")
        .unwrap_err()
        .starts_with("Invalid JSON from yt-dlp:"));
    let pattern = regex::Regex::new(crate::yt_dlp::PROGRESS_PATTERN).unwrap();
    let progress = parse_progress_line(
        "[download] 42.5% of 1MiB at 2MiB/s ETA 00:01",
        &pattern,
        "one",
    )
    .unwrap();
    assert_eq!(progress.id, "one");
    assert_eq!(progress.percent, Some(42.5));
    assert_eq!(progress.speed.as_deref(), Some("2MiB/s"));
    assert_eq!(progress.eta.as_deref(), Some("00:01"));
    assert!(parse_progress_line("[Merger] merging formats", &pattern, "one").is_none());
}

#[test]
fn queue_pause_resume_and_idle_preserve_fifo_and_auto_start_defaults() {
    let queue = QueueState::default();
    let current = Mutex::new(None);
    enqueue_jobs(&queue, vec![job("one"), job("two")]).unwrap();
    assert!(snapshot_queue_status(&queue).unwrap().auto_start);
    *queue.worker_running.lock().unwrap() = true;
    set_queue_paused(&queue, true).unwrap();
    let (next, paused) = next_worker_job(&queue, &current).unwrap();
    assert!(next.is_none() && paused);
    assert_eq!(queue.pending.lock().unwrap().len(), 2);
    assert!(!snapshot_queue_status(&queue).unwrap().worker_running);
    set_queue_paused(&queue, false).unwrap();
    assert_eq!(
        next_worker_job(&queue, &current).unwrap().0.unwrap().id,
        "one"
    );
    assert_eq!(
        next_worker_job(&queue, &current).unwrap().0.unwrap().id,
        "two"
    );
    assert_eq!(current.lock().unwrap().as_deref(), Some("two"));
    assert!(next_worker_job(&queue, &current).unwrap().0.is_none());
}

#[test]
fn cancelling_a_waiting_job_preserves_the_other_jobs_and_their_order() {
    let queue = QueueState::default();
    enqueue_jobs(&queue, vec![job("one"), job("two"), job("three")]).unwrap();
    assert!(!remove_queued_job(&queue, "missing").unwrap());
    assert!(remove_queued_job(&queue, "two").unwrap());
    assert!(!remove_queued_job(&queue, "two").unwrap());
    assert_eq!(
        queue
            .pending
            .lock()
            .unwrap()
            .iter()
            .map(|job| job.id.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "three"]
    );
}

#[test]
fn cancellation_wins_before_persistence_and_retains_the_output_path() {
    let processes = ProcessState::default();
    let queue = QueueState::default();
    *processes.current_job_id.lock().unwrap() = Some("one".into());
    *processes.cancel_requested.lock().unwrap() = Some("one".into());
    *queue.active_video_key.lock().unwrap() = Some("youtube:synthetic".into());
    let called = Cell::new(false);
    let outcome =
        finalize_active_job_event(&processes, &queue, event(DownloadState::Success), || {
            called.set(true);
            Ok(())
        });
    assert_eq!(outcome.state, DownloadState::Cancelled);
    assert!(!called.get());
    assert_eq!(
        outcome.output_path.as_deref(),
        Some("/synthetic/output.mp4")
    );
    assert!(outcome.error.unwrap().contains("output file preserved"));
    assert!(processes.current_job_id.lock().unwrap().is_none());
    assert!(processes.cancel_requested.lock().unwrap().is_none());
    assert!(queue.active_video_key.lock().unwrap().is_none());
}

#[test]
fn finalization_reports_success_only_when_persistence_succeeds() {
    let processes = ProcessState::default();
    let queue = QueueState::default();
    let calls = Cell::new(0);
    let success =
        finalize_active_job_event(&processes, &queue, event(DownloadState::Success), || {
            calls.set(calls.get() + 1);
            Ok(())
        });
    assert_eq!(success.state, DownloadState::Success);
    assert_eq!(calls.get(), 1);
    let failure =
        finalize_active_job_event(&processes, &queue, event(DownloadState::Success), || {
            Err("synthetic persistence failure".into())
        });
    assert_eq!(failure.state, DownloadState::Error);
    assert_eq!(
        failure.output_path.as_deref(),
        Some("/synthetic/output.mp4")
    );
    assert_eq!(
        failure.error.as_deref(),
        Some("synthetic persistence failure")
    );
    let original_error =
        finalize_active_job_event(&processes, &queue, event(DownloadState::Error), || {
            calls.set(99);
            Ok(())
        });
    assert_eq!(original_error.state, DownloadState::Error);
    assert_eq!(calls.get(), 1);
}

#[test]
fn typed_download_states_preserve_the_wire_contract_and_reject_unknown_values() {
    for (status, wire) in [
        (DownloadState::Queued, "queued"),
        (DownloadState::Downloading, "downloading"),
        (DownloadState::Transcribing, "transcribing"),
        (DownloadState::Cancelling, "cancelling"),
        (DownloadState::Cancelled, "cancelled"),
        (DownloadState::Success, "success"),
        (DownloadState::Error, "error"),
    ] {
        assert_eq!(serde_json::to_value(event(status)).unwrap()["state"], wire);
        assert_eq!(
            serde_json::from_value::<DownloadState>(json!(wire)).unwrap(),
            status
        );
    }
    assert!(serde_json::from_value::<DownloadState>(json!("invalid")).is_err());
}

#[test]
fn process_start_errors_are_distinct_from_timeouts_without_parsing_messages() {
    let command = Command::new(format!(
        "/synthetic/pinefetch-missing-{}",
        uuid::Uuid::new_v4()
    ));
    assert!(matches!(
        run_command_output(command, None, None, None),
        Err(ProcessError::Spawn(_))
    ));
    assert_eq!(ProcessError::TimedOut.to_string(), "process timed out");
    assert_eq!(
        ProcessError::OutputDrain.to_string(),
        "Process output did not close after exit"
    );
}
