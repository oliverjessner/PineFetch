//! Offline smoke: real child, parsers, temporary output, SQLite and final state.
use crate::completion;
use crate::database::{self, Database};
use crate::download_rules::{prepare_download_job, request_from_preset, DownloadOptions};
use crate::history::get_history_details_from_db;
use crate::models::{AppConfig, DownloadState, DownloadStateEvent};
use crate::presets::download_preset_for_key;
use crate::process::{run_command_output, ProcessState};
use crate::queue::{enqueue_jobs, finalize_active_job_event, next_worker_job, QueueState};
use crate::test_support::{FakeProcess, TempRoot, TEST_TIMEOUT};
use crate::yt_dlp::{parse_download_metadata_line, parse_progress_line, PROGRESS_PATTERN};

#[test]
fn integration_fake_download_commits_history_only_after_output_and_required_persistence() {
    let pattern = regex::Regex::new(PROGRESS_PATTERN).unwrap();
    for fail_history in [false, true] {
        let root = TempRoot::new("application-smoke");
        let path = root.path("Grüße 🌲 clip.mp4");
        let connection = rusqlite::Connection::open(root.path("pinefetch.sqlite")).unwrap();
        database::initialize(&connection).unwrap();
        let db = Database::new(connection);
        let job = prepare_download_job(
            request_from_preset(
                download_preset_for_key(None),
                "https://example.com/clip".into(),
                false,
            ),
            DownloadOptions::from(&AppConfig::default()),
            root.root().to_string_lossy().into_owned(),
            "smoke-job".into(),
        );
        let queue = QueueState::default();
        let processes = ProcessState::default();
        enqueue_jobs(&queue, vec![job]).unwrap();
        let job = next_worker_job(&queue, &processes.current_job_id)
            .unwrap()
            .0
            .unwrap();
        completion::begin(&db, &job).unwrap();
        let fake = FakeProcess::new("success");
        let mut command = fake.command();
        command.arg("--output").arg(&path);
        let output =
            run_command_output(command, Some(&processes), None, Some(TEST_TIMEOUT)).unwrap();
        assert!(output.status.success());
        assert!(path.is_file());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(
            stdout
                .lines()
                .find_map(|line| parse_progress_line(line, &pattern, &job.id))
                .unwrap()
                .percent,
            Some(42.0)
        );
        let (parsed_path, info) = stdout
            .lines()
            .find_map(parse_download_metadata_line)
            .unwrap();
        assert_eq!(std::path::Path::new(&parsed_path), path);
        assert_eq!(info.title.as_deref(), Some("Offline clip"));
        if fail_history {
            db.lock().unwrap().execute_batch("CREATE TRIGGER fail_smoke_history BEFORE INSERT ON history_entries BEGIN SELECT RAISE(ABORT, 'injected history failure'); END;").unwrap();
        }
        let result = finalize_active_job_event(
            &processes,
            &queue,
            DownloadStateEvent {
                id: job.id.clone(),
                state: DownloadState::Success,
                exit_code: Some(0),
                error: None,
                output_path: Some(parsed_path.clone()),
            },
            || completion::complete(&db, &job, Some(&parsed_path), Some(&info), &[], None),
        );
        assert_eq!(
            result.state,
            if fail_history {
                DownloadState::Error
            } else {
                DownloadState::Success
            }
        );
        assert!(processes.current_job_id.lock().unwrap().is_none());
        assert!(
            path.is_file(),
            "persisting history must never delete the user's output"
        );
        let details = get_history_details_from_db(&db, &job.id).unwrap();
        if fail_history {
            assert!(details.is_none());
            assert!(result.error.as_deref().unwrap().contains("preserved"));
            db.lock()
                .unwrap()
                .execute_batch("DROP TRIGGER fail_smoke_history")
                .unwrap();
            completion::complete(&db, &job, Some(&parsed_path), Some(&info), &[], None).unwrap();
        } else {
            let entry = details.unwrap().entry;
            assert_eq!(entry.title.as_deref(), Some("Offline clip"));
            assert_eq!(
                entry.file_size_bytes,
                Some(i64::try_from(std::fs::metadata(&path).unwrap().len()).unwrap())
            );
            assert!(entry.sha256.is_some());
        }
        assert!(get_history_details_from_db(&db, &job.id).unwrap().is_some());
        drop(db);
        let reopened = rusqlite::Connection::open(root.path("pinefetch.sqlite")).unwrap();
        database::initialize(&reopened).unwrap();
        assert_eq!(
            reopened
                .query_row(
                    "SELECT COUNT(*) FROM history_entries WHERE id='smoke-job'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
    }
}
