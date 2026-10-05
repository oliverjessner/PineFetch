//! State transitions and concurrency without a desktop or external runtime.
use crate::download_rules::{prepare_download_job, request_from_preset, DownloadOptions};
use crate::models::{AppConfig, DownloadJob, DownloadState, DownloadStateEvent};
use crate::presets::download_preset_for_key;
use crate::process::ProcessState;
use crate::queue::{
    claim_worker_start, enqueue_jobs, finalize_active_job_event, is_queue_auto_start_enabled,
    next_worker_job, remove_queued_job, set_queue_paused, snapshot_queue_status, QueueState,
};
use crate::worker::request_active_job_cancellation;
use std::cell::Cell;
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

fn job(id: &str) -> DownloadJob {
    prepare_download_job(
        request_from_preset(
            download_preset_for_key(None),
            "https://example.com/clip".into(),
            false,
        ),
        DownloadOptions::from(&AppConfig::default()),
        "/synthetic/Output files".into(),
        id.into(),
    )
}

fn event(id: &str, state: DownloadState) -> DownloadStateEvent {
    DownloadStateEvent {
        id: id.into(),
        state,
        exit_code: Some(0),
        error: None,
        output_path: Some("/synthetic/Clip file.mp4".into()),
    }
}

#[test]
fn completed_and_failed_jobs_release_active_state_and_allow_the_next_fifo_job() {
    for terminal in [DownloadState::Success, DownloadState::Error] {
        let queue = QueueState::default();
        let processes = ProcessState::default();
        enqueue_jobs(&queue, vec![job("one"), job("two")]).unwrap();
        assert!(claim_worker_start(&queue).unwrap());
        let (first, paused) = next_worker_job(&queue, &processes.current_job_id).unwrap();
        assert!(!paused);
        assert_eq!(first.unwrap().id, "one");
        assert_eq!(
            processes.current_job_id.lock().unwrap().as_deref(),
            Some("one")
        );
        let committed = Cell::new(false);
        let result = finalize_active_job_event(&processes, &queue, event("one", terminal), || {
            committed.set(true);
            Ok(())
        });
        assert_eq!(result.state, terminal);
        assert_eq!(committed.get(), terminal == DownloadState::Success);
        assert!(processes.current_job_id.lock().unwrap().is_none());
        assert!(queue.active_video_key.lock().unwrap().is_none());
        assert_eq!(
            next_worker_job(&queue, &processes.current_job_id)
                .unwrap()
                .0
                .unwrap()
                .id,
            "two"
        );
        finalize_active_job_event(
            &processes,
            &queue,
            event("two", DownloadState::Success),
            || Ok(()),
        );
        assert!(next_worker_job(&queue, &processes.current_job_id)
            .unwrap()
            .0
            .is_none());
        assert!(!snapshot_queue_status(&queue).unwrap().worker_running);
    }
}

#[test]
fn cancelled_waiting_jobs_are_removed_once_without_changing_the_active_job() {
    let queue = QueueState::default();
    let processes = ProcessState::default();
    enqueue_jobs(&queue, vec![job("active"), job("waiting"), job("next")]).unwrap();
    next_worker_job(&queue, &processes.current_job_id).unwrap();
    assert!(remove_queued_job(&queue, "waiting").unwrap());
    assert!(!remove_queued_job(&queue, "waiting").unwrap());
    assert!(!remove_queued_job(&queue, "unknown").unwrap());
    assert!(!remove_queued_job(&queue, "active").unwrap());
    assert_eq!(
        processes.current_job_id.lock().unwrap().as_deref(),
        Some("active")
    );
    assert_eq!(queue.pending.lock().unwrap().front().unwrap().id, "next");
}

#[test]
fn repeated_active_cancellation_wins_over_success_and_never_runs_persistence() {
    let queue = QueueState::default();
    let processes = ProcessState::default();
    enqueue_jobs(&queue, vec![job("one"), job("next")]).unwrap();
    next_worker_job(&queue, &processes.current_job_id).unwrap();
    assert!(request_active_job_cancellation(&processes, "unknown").is_err());
    assert!(processes.cancel_requested.lock().unwrap().is_none());
    request_active_job_cancellation(&processes, "one").unwrap();
    request_active_job_cancellation(&processes, "one").unwrap();
    let result = finalize_active_job_event(
        &processes,
        &queue,
        event("one", DownloadState::Success),
        || panic!("cancelled completion must never commit"),
    );
    assert_eq!(result.state, DownloadState::Cancelled);
    assert_eq!(
        result.output_path.as_deref(),
        Some("/synthetic/Clip file.mp4")
    );
    assert!(processes.cancel_requested.lock().unwrap().is_none());
    assert!(request_active_job_cancellation(&processes, "one").is_err());
    assert_eq!(
        next_worker_job(&queue, &processes.current_job_id)
            .unwrap()
            .0
            .unwrap()
            .id,
        "next"
    );
}

#[test]
fn pause_and_repeated_resume_preserve_waiting_jobs_and_worker_ownership() {
    let queue = QueueState::default();
    let processes = ProcessState::default();
    enqueue_jobs(&queue, vec![job("one")]).unwrap();
    set_queue_paused(&queue, true).unwrap();
    assert!(!claim_worker_start(&queue).unwrap());
    let (next, paused) = next_worker_job(&queue, &processes.current_job_id).unwrap();
    assert!(next.is_none() && paused);
    assert_eq!(queue.pending.lock().unwrap().len(), 1);
    set_queue_paused(&queue, false).unwrap();
    set_queue_paused(&queue, false).unwrap();
    assert!(claim_worker_start(&queue).unwrap());
    assert!(!claim_worker_start(&queue).unwrap());
    assert_eq!(
        next_worker_job(&queue, &processes.current_job_id)
            .unwrap()
            .0
            .unwrap()
            .id,
        "one"
    );
}

#[test]
fn auto_start_setting_controls_automatic_start_while_manual_start_remains_available() {
    for enabled in [false, true] {
        let queue = QueueState::default();
        *queue.auto_start.lock().unwrap() = enabled;
        enqueue_jobs(&queue, vec![job("one")]).unwrap();
        if is_queue_auto_start_enabled(&queue).unwrap() {
            assert!(claim_worker_start(&queue).unwrap());
        }
        assert_eq!(
            snapshot_queue_status(&queue).unwrap().worker_running,
            enabled
        );
        assert_eq!(queue.pending.lock().unwrap().len(), 1);
        if !enabled {
            assert!(claim_worker_start(&queue).unwrap());
        }
    }
}

#[test]
fn simultaneous_start_requests_claim_exactly_one_worker() {
    let queue = Arc::new(QueueState::default());
    let barrier = Arc::new(Barrier::new(9));
    let (sender, receiver) = mpsc::channel();
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let queue = Arc::clone(&queue);
            let barrier = Arc::clone(&barrier);
            let sender = sender.clone();
            std::thread::spawn(move || {
                barrier.wait();
                sender.send(claim_worker_start(&queue).unwrap()).unwrap();
            })
        })
        .collect();
    barrier.wait();
    let claims: Vec<_> = (0..8)
        .map(|_| {
            receiver
                .recv_timeout(Duration::from_secs(10))
                .expect("worker start result")
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(claims.iter().filter(|claimed| **claimed).count(), 1);
    assert!(snapshot_queue_status(&queue).unwrap().worker_running);
}

#[test]
fn concurrent_enqueue_keeps_each_batch_contiguous_without_losing_jobs() {
    let queue = Arc::new(QueueState::default());
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|prefix| {
            let queue = Arc::clone(&queue);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                enqueue_jobs(
                    &queue,
                    vec![job(&format!("{prefix}1")), job(&format!("{prefix}2"))],
                )
                .unwrap();
            })
        })
        .collect();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }
    let ids: Vec<_> = queue
        .pending
        .lock()
        .unwrap()
        .iter()
        .map(|job| job.id.clone())
        .collect();
    assert!(ids == ["a1", "a2", "b1", "b2"] || ids == ["b1", "b2", "a1", "a2"]);
}
