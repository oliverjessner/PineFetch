//! Restart and failure cases use isolated profiles and never start media tools.
use crate::download_rules::{prepare_download_job, request_from_preset, DownloadOptions};
use crate::models::AppConfig;
use crate::presets::download_preset_for_key;
use crate::test_support::TempRoot;
use crate::{queue, startup};

#[test]
fn integration_waiting_queue_and_pause_survive_restart_with_all_job_options() {
    let root = TempRoot::new("queue-restart");
    let data = root.path("data");
    let config = root.path("config");
    let state = startup::load_from_dirs(&data, &config).unwrap();
    queue::set_queue_paused(&state.queue, true).unwrap();
    let jobs: Vec<_> = (0..100)
        .map(|index| {
            let mut job = prepare_download_job(
                request_from_preset(
                    download_preset_for_key(Some("text_timestamps")),
                    format!("https://example.com/video/{index}"),
                    false,
                ),
                DownloadOptions::from(&AppConfig::default()),
                root.path("Grüße 🌲").to_string_lossy().into_owned(),
                format!("waiting-{index}"),
            );
            job.download_video_with_transcript = true;
            job.save_creator_subtitles = true;
            job.cut_start_time = Some(index as f64 + 0.25);
            job.filename_suffix = Some(format!("_t{index}"));
            job
        })
        .collect();
    let expected = serde_json::to_value(&jobs).unwrap();
    queue::enqueue_jobs(&state.queue, jobs).unwrap();
    drop(state);
    let reopened = startup::load_from_dirs(&data, &config).unwrap();
    let restored: Vec<_> = reopened
        .queue
        .data
        .lock()
        .unwrap()
        .pending
        .iter()
        .cloned()
        .collect();
    assert_eq!(serde_json::to_value(restored).unwrap(), expected);
    assert!(
        queue::snapshot_queue_status(&reopened.queue)
            .unwrap()
            .paused
    );
}

fn queued_job(root: &TempRoot, id: &str) -> crate::models::DownloadJob {
    prepare_download_job(
        request_from_preset(
            download_preset_for_key(Some("best")),
            format!("https://example.com/{id}"),
            false,
        ),
        DownloadOptions::from(&AppConfig::default()),
        root.root().to_string_lossy().into_owned(),
        id.into(),
    )
}

#[test]
fn integration_auto_start_and_removals_are_saved_before_acknowledgement() {
    let root = TempRoot::new("queue-modes");
    let data = root.path("data");
    let config = root.path("config");
    let state = startup::load_from_dirs(&data, &config).unwrap();
    queue::set_queue_auto_start(&state.queue, false).unwrap();
    queue::enqueue_jobs(
        &state.queue,
        vec![
            queued_job(&root, "one"),
            queued_job(&root, "two"),
            queued_job(&root, "three"),
        ],
    )
    .unwrap();
    assert_eq!(queue::remove_waiting_at(&state.queue, 2).unwrap().id, "two");
    assert!(queue::remove_queued_job(&state.queue, "one").unwrap());
    drop(state);
    let state = startup::load_from_dirs(&data, &config).unwrap();
    let status = queue::snapshot_queue_status(&state.queue).unwrap();
    assert!(!status.auto_start);
    assert!(!status.paused);
    assert!(!status.worker_running);
    assert_eq!(
        state.queue.data.lock().unwrap().pending.front().unwrap().id,
        "three"
    );
    queue::set_queue_paused(&state.queue, true).unwrap();
    queue::set_queue_auto_start(&state.queue, true).unwrap();
    drop(state);
    let state = startup::load_from_dirs(&data, &config).unwrap();
    let status = queue::snapshot_queue_status(&state.queue).unwrap();
    assert!(status.auto_start && status.paused);
    assert!(!queue::claim_worker_start(&state.queue).unwrap());
}

#[test]
fn integration_queue_write_failures_preserve_saved_and_in_memory_jobs_and_modes() {
    let root = TempRoot::new("queue-write-failure");
    let state = startup::load_from_dirs(&root.path("data"), &root.path("config")).unwrap();
    queue::enqueue_jobs(&state.queue, vec![queued_job(&root, "one")]).unwrap();
    let saved: String = state
        .db
        .lock()
        .unwrap()
        .query_row("SELECT snapshot_json FROM queue_state", [], |row| {
            row.get(0)
        })
        .unwrap();
    state
        .db
        .lock()
        .unwrap()
        .execute_batch("PRAGMA query_only=ON")
        .unwrap();
    assert!(queue::enqueue_jobs(&state.queue, vec![queued_job(&root, "two")]).is_err());
    assert!(queue::remove_queued_job(&state.queue, "one").is_err());
    assert!(queue::set_queue_paused(&state.queue, true).is_err());
    assert!(queue::set_queue_auto_start(&state.queue, false).is_err());
    // Cancelling an active/unknown ID must not require a queue database write.
    assert!(!queue::remove_queued_job(&state.queue, "active").unwrap());
    assert!(queue::claim_worker_start(&state.queue).unwrap());
    assert!(queue::next_worker_job(&state.queue, &state.processes.current_job_id).is_err());
    assert!(state.processes.current_job_id.lock().unwrap().is_none());
    let data = state.queue.data.lock().unwrap();
    assert_eq!(data.pending.len(), 1);
    assert!(data.active.is_none());
    assert!(data.auto_start);
    assert!(!data.paused);
    drop(data);
    assert_eq!(
        state
            .db
            .lock()
            .unwrap()
            .query_row::<String, _, _>("SELECT snapshot_json FROM queue_state", [], |row| row
                .get(0))
            .unwrap(),
        saved
    );
}

#[test]
fn integration_queue_crash_probe() {
    let Ok(path) = std::env::var("PINEFETCH_QUEUE_CRASH_PROFILE") else {
        return;
    };
    let root = std::path::PathBuf::from(path);
    let state = startup::load_from_dirs(&root.join("data"), &root.join("config")).unwrap();
    queue::claim_worker_start(&state.queue).unwrap();
    let job = queue::next_worker_job(&state.queue, &state.processes.current_job_id)
        .unwrap()
        .0
        .unwrap();
    crate::completion::begin(&state.db, &job).unwrap();
    let output = root.join("produced.mp4");
    std::fs::write(&output, b"produced output must survive").unwrap();
    crate::completion::output_ready(&state.db, &job, output.to_str()).unwrap();
    if std::env::var("PINEFETCH_QUEUE_CRASH_MODE").as_deref() == Ok("complete") {
        crate::completion::complete(&state.db, &job, output.to_str(), None, &[], None).unwrap();
    }
    // Exit without destructors: no shutdown callback or queue-finalization write.
    std::process::exit(91);
}

#[test]
fn integration_crashed_jobs_are_interrupted_and_completed_receipts_are_never_requeued() {
    for mode in ["output_ready", "complete"] {
        let root = TempRoot::new("queue-process-crash");
        let data = root.path("data");
        let config = root.path("config");
        let state = startup::load_from_dirs(&data, &config).unwrap();
        queue::set_queue_auto_start(&state.queue, false).unwrap();
        queue::enqueue_jobs(
            &state.queue,
            vec![queued_job(&root, "active"), queued_job(&root, "waiting")],
        )
        .unwrap();
        drop(state);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "queue_persistence_tests::integration_queue_crash_probe",
                "--nocapture",
            ])
            .env("PINEFETCH_QUEUE_CRASH_PROFILE", root.root())
            .env("PINEFETCH_QUEUE_CRASH_MODE", mode)
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(91),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        let state = startup::load_from_dirs(&data, &config).unwrap();
        {
            let data = state.queue.data.lock().unwrap();
            assert_eq!(data.pending.len(), 1);
            assert_eq!(data.pending[0].id, "waiting");
            assert!(data.active.is_none());
            assert!(!data.worker_running);
            assert_eq!(data.interrupted.len(), usize::from(mode != "complete"));
            if let Some(entry) = data.interrupted.first() {
                assert_eq!(entry.job.id, "active");
                assert_eq!(
                    entry.output_path.as_deref(),
                    root.path("produced.mp4").to_str()
                );
            }
        }
        assert_eq!(
            std::fs::read(root.path("produced.mp4")).unwrap(),
            b"produced output must survive"
        );
        if mode == "complete" {
            assert_eq!(
                state
                    .db
                    .lock()
                    .unwrap()
                    .query_row::<i64, _, _>("SELECT COUNT(*) FROM history_entries", [], |row| row
                        .get(0))
                    .unwrap(),
                1
            );
        } else {
            state
                .db
                .lock()
                .unwrap()
                .execute_batch("PRAGMA query_only=ON")
                .unwrap();
            assert!(queue::retry_interrupted(&state.queue, "active").is_err());
            assert_eq!(state.queue.data.lock().unwrap().interrupted.len(), 1);
            state
                .db
                .lock()
                .unwrap()
                .execute_batch("PRAGMA query_only=OFF")
                .unwrap();
            let before = state.queue.data.lock().unwrap().interrupted[0].job.clone();
            let new = queue::retry_interrupted(&state.queue, "active").unwrap();
            assert_ne!(new.id, before.id);
            let mut expected = before;
            expected.id = new.id.clone();
            assert_eq!(
                serde_json::to_value(&new).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
            assert!(queue::retry_interrupted(&state.queue, "active").is_err());
            drop(state);
            let state = startup::load_from_dirs(&data, &config).unwrap();
            assert!(state.queue.data.lock().unwrap().interrupted.is_empty());
            assert_eq!(
                state.queue.data.lock().unwrap().pending.back().unwrap().id,
                new.id
            );
        }
    }
}

#[test]
fn integration_shutdown_preserves_the_chosen_mode_and_holds_unfinished_jobs_for_review() {
    let root = TempRoot::new("queue-shutdown");
    let data = root.path("data");
    let config = root.path("config");
    let state = startup::load_from_dirs(&data, &config).unwrap();
    queue::enqueue_jobs(
        &state.queue,
        vec![queued_job(&root, "one"), queued_job(&root, "two")],
    )
    .unwrap();
    queue::claim_worker_start(&state.queue).unwrap();
    let job = queue::next_worker_job(&state.queue, &state.processes.current_job_id)
        .unwrap()
        .0
        .unwrap();
    crate::completion::begin(&state.db, &job).unwrap();
    crate::worker::stop_active_download_on_exit(&state);
    let event = crate::models::DownloadStateEvent {
        id: job.id.clone(),
        state: crate::models::DownloadState::Success,
        exit_code: Some(0),
        error: None,
        output_path: None,
    };
    let event = queue::finalize_active_job_event(&state.processes, &state.queue, event, || {
        panic!("shutdown cancellation must win")
    });
    assert_eq!(event.state, crate::models::DownloadState::Cancelled);
    drop(state);
    let state = startup::load_from_dirs(&data, &config).unwrap();
    let restored = state.queue.data.lock().unwrap();
    assert!(restored.auto_start);
    assert!(!restored.paused);
    assert!(!restored.stopping);
    assert_eq!(restored.pending[0].id, "two");
    assert_eq!(restored.interrupted[0].job.id, "one");
}

#[test]
fn integration_queue_profile_has_one_owner_and_rejects_stale_snapshot_writers() {
    let root = TempRoot::new("queue-owner");
    let data = root.path("data");
    let config = root.path("config");
    let state = startup::load_from_dirs(&data, &config).unwrap();
    assert!(startup::load_from_dirs(&data, &config)
        .err()
        .unwrap()
        .contains("already open"));
    queue::enqueue_jobs(&state.queue, vec![queued_job(&root, "one")]).unwrap();
    // External writes cannot be silently overwritten by an old in-memory revision.
    state
        .db
        .lock()
        .unwrap()
        .execute("UPDATE queue_state SET revision=revision+1", [])
        .unwrap();
    assert!(
        queue::enqueue_jobs(&state.queue, vec![queued_job(&root, "two")])
            .unwrap_err()
            .contains("changed outside")
    );
    assert_eq!(state.queue.data.lock().unwrap().pending.len(), 1);
    drop(state);
    let state = startup::load_from_dirs(&data, &config).unwrap();
    assert_eq!(state.queue.data.lock().unwrap().pending[0].id, "one");
}

#[test]
fn integration_corrupt_or_missing_queue_snapshots_are_refused_without_reinitializing_jobs() {
    for json in [
        Some("not json"),
        Some(
            r#"{"format_version":99,"pending":[],"active":null,"interrupted":[],"auto_start":true,"paused":false}"#,
        ),
        None,
    ] {
        let root = TempRoot::new("queue-corruption");
        let data = root.path("data");
        let config = root.path("config");
        let state = startup::load_from_dirs(&data, &config).unwrap();
        queue::enqueue_jobs(&state.queue, vec![queued_job(&root, "one")]).unwrap();
        if let Some(json) = json {
            state
                .db
                .lock()
                .unwrap()
                .execute("UPDATE queue_state SET snapshot_json=?1", [json])
                .unwrap();
        } else {
            state
                .db
                .lock()
                .unwrap()
                .execute("DELETE FROM queue_state", [])
                .unwrap();
        }
        drop(state);
        let before = std::fs::read(data.join("pinefetch.sqlite")).unwrap();
        assert!(startup::load_from_dirs(&data, &config).is_err());
        assert_eq!(
            std::fs::read(data.join("pinefetch.sqlite")).unwrap(),
            before
        );
    }
}

#[test]
fn integration_queue_migration_preserves_settings_and_rolls_back_the_whole_step() {
    let root = TempRoot::new("queue-migration");
    let path = root.path("old.sqlite");
    let conn = crate::database::open(&path).unwrap();
    crate::database::restore_schema11_queue_fixture(&conn);
    conn.execute_batch("PRAGMA user_version=11; UPDATE app_config SET selected_preset_key='text'; INSERT INTO history_entries(id,url,created_at) VALUES ('old','https://example.com/old',1);").unwrap();
    assert!(crate::database::initialize_with_hook(&conn, |point| {
        if point == crate::database::MigrationPoint::Queue {
            Err("synthetic queue migration failure".into())
        } else {
            Ok(())
        }
    })
    .unwrap_err()
    .contains("synthetic queue migration failure"));
    assert_eq!(
        conn.pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        11
    );
    assert_eq!(
        conn.query_row::<i64, _, _>(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='queue_state'",
            [],
            |row| row.get(0)
        )
        .unwrap(),
        0
    );
    crate::database::initialize(&conn).unwrap();
    assert_eq!(
        conn.pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        crate::database::SCHEMA_VERSION
    );
    assert_eq!(
        conn.query_row::<String, _, _>("SELECT selected_preset_key FROM app_config", [], |row| row
            .get(0))
            .unwrap(),
        "text"
    );
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM history_entries", [], |row| row.get(0))
            .unwrap(),
        1
    );
    assert!(std::fs::read_dir(root.root()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("pre-schema-11")));
}

#[test]
fn integration_completed_history_is_not_repeated_if_the_final_queue_save_fails() {
    let root = TempRoot::new("queue-completion-gap");
    let data = root.path("data");
    let config = root.path("config");
    let state = startup::load_from_dirs(&data, &config).unwrap();
    queue::enqueue_jobs(&state.queue, vec![queued_job(&root, "complete-once")]).unwrap();
    queue::claim_worker_start(&state.queue).unwrap();
    let job = queue::next_worker_job(&state.queue, &state.processes.current_job_id)
        .unwrap()
        .0
        .unwrap();
    crate::completion::begin(&state.db, &job).unwrap();
    let output = root.path("saved.mp4");
    std::fs::write(&output, b"saved exactly once").unwrap();
    state.db.lock().unwrap().execute_batch("CREATE TRIGGER fail_queue_save BEFORE UPDATE ON queue_state BEGIN SELECT RAISE(ABORT,'synthetic queue save failure'); END;").unwrap();
    let event = crate::models::DownloadStateEvent {
        id: job.id.clone(),
        state: crate::models::DownloadState::Success,
        exit_code: Some(0),
        error: None,
        output_path: Some(output.to_string_lossy().into_owned()),
    };
    let result = queue::finalize_active_job_event(&state.processes, &state.queue, event, || {
        crate::completion::complete(&state.db, &job, output.to_str(), None, &[], None)
    });
    assert_eq!(result.state, crate::models::DownloadState::Error);
    assert!(result
        .error
        .unwrap()
        .contains("synthetic queue save failure"));
    assert!(state.queue.data.lock().unwrap().stopping);
    state
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_queue_save")
        .unwrap();
    drop(state);
    let state = startup::load_from_dirs(&data, &config).unwrap();
    let restored = state.queue.data.lock().unwrap();
    assert!(
        restored.pending.is_empty() && restored.active.is_none() && restored.interrupted.is_empty()
    );
    assert_eq!(
        state
            .db
            .lock()
            .unwrap()
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM history_entries WHERE id='complete-once'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(std::fs::read(output).unwrap(), b"saved exactly once");
}

#[test]
fn integration_persistent_concurrent_enqueue_keeps_batches_contiguous_and_survives_restart() {
    let root = TempRoot::new("queue-concurrent");
    let data = root.path("data");
    let config = root.path("config");
    let state = std::sync::Arc::new(startup::load_from_dirs(&data, &config).unwrap());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(5));
    let mut threads = Vec::new();
    for batch in 0..4 {
        let jobs = (0..25)
            .map(|index| queued_job(&root, &format!("batch-{batch}-{index}")))
            .collect();
        let state = std::sync::Arc::clone(&state);
        let barrier = std::sync::Arc::clone(&barrier);
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            queue::enqueue_jobs(&state.queue, jobs).unwrap();
        }));
    }
    barrier.wait();
    for thread in threads {
        thread.join().unwrap();
    }
    let expected = serde_json::to_value(&state.queue.data.lock().unwrap().pending).unwrap();
    drop(state);
    let state = startup::load_from_dirs(&data, &config).unwrap();
    let jobs = state.queue.data.lock().unwrap();
    assert_eq!(jobs.pending.len(), 100);
    assert_eq!(serde_json::to_value(&jobs.pending).unwrap(), expected);
    for chunk in jobs.pending.as_slices().0.chunks(25) {
        let prefix = chunk[0].id.rsplit_once('-').unwrap().0;
        for (index, job) in chunk.iter().enumerate() {
            assert_eq!(job.id, format!("{prefix}-{index}"));
        }
    }
}

#[test]
fn integration_conflicting_completion_identity_cannot_discard_a_saved_queue_job() {
    let root = TempRoot::new("queue-receipt-identity");
    let data = root.path("data");
    let config = root.path("config");
    let state = startup::load_from_dirs(&data, &config).unwrap();
    queue::enqueue_jobs(&state.queue, vec![queued_job(&root, "one")]).unwrap();
    state.db.lock().unwrap().execute("INSERT INTO job_completions(job_id,url,state,updated_at) VALUES ('one','https://example.com/different','complete',1)",[]).unwrap();
    drop(state);
    let before = std::fs::read(data.join("pinefetch.sqlite")).unwrap();
    assert!(startup::load_from_dirs(&data, &config)
        .err()
        .unwrap()
        .contains("completion identity"));
    assert_eq!(
        std::fs::read(data.join("pinefetch.sqlite")).unwrap(),
        before
    );
}
