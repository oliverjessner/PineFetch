//! Completion and saved-data failures use real SQLite and isolated synthetic files.
use crate::completion;
use crate::config::{load_config_from_db, update_config};
use crate::database;
use crate::history::{
    get_history_caption_from_db, get_history_details_from_db, get_history_transcript_from_db,
    list_history_page_from_db,
};
use crate::models::{DownloadJob, DownloadState, DownloadStateEvent, SavedCaption};
use crate::state::AppState;
use crate::test_support::TempRoot;
use rusqlite::{params, Connection};
use std::fs;

fn state(root: &TempRoot) -> AppState {
    let conn = database::open(&root.path("pinefetch.sqlite3")).unwrap();
    AppState::new(load_config_from_db(&conn).unwrap(), conn)
}

fn job(root: &TempRoot, id: &str, transcript: bool) -> DownloadJob {
    DownloadJob {
        id: id.into(),
        url: "https://www.youtube.com/watch?v=synthetic".into(),
        format: "best".into(),
        output_dir: root.root().to_string_lossy().into_owned(),
        extract_audio: false,
        audio_format: None,
        transcribe_text: transcript,
        transcribe_timestamps: transcript,
        faster_whisper_model: "base".into(),
        download_video_with_transcript: false,
        save_captions: false,
        save_thumbnails: false,
        title: Some("Synthetic Grüße 🌲".into()),
        uploader: None,
        thumbnail: None,
        upload_date: None,
        timestamp: None,
        duration_seconds: None,
        cut_start_time: None,
        filename_suffix: None,
    }
}

fn assert_uncommitted(conn: &Connection, id: &str, expected_receipt: &str) {
    let counts: (i64, i64, i64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM history_entries),
                    (SELECT COUNT(*) FROM captions),
                    (SELECT COUNT(*) FROM transcriptions)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(counts, (0, 0, 0));
    let receipt: (String, Option<String>) = conn
        .query_row(
            "SELECT state,history_entry_id FROM job_completions WHERE job_id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(receipt, (expected_receipt.into(), None));
}

fn failed_completion_is_an_error(
    job: &DownloadJob,
    path: Option<&str>,
    result: Result<(), String>,
) {
    assert!(result.is_err());
    let mut event = DownloadStateEvent {
        id: job.id.clone(),
        state: DownloadState::Success,
        exit_code: Some(0),
        error: None,
        output_path: path.map(str::to_owned),
    };
    event.apply_result(result);
    assert_eq!(event.state, DownloadState::Error);
    assert!(event.error.is_some());
    assert_eq!(event.output_path.as_deref(), path);
}

#[test]
fn integration_a_successful_process_without_a_required_output_cannot_commit_completion() {
    let root = TempRoot::new("completion-no-output");
    let state = state(&root);
    let job = job(&root, "no-output", false);
    completion::begin(&state.db, &job).unwrap();
    failed_completion_is_an_error(
        &job,
        None,
        completion::complete(&state.db, &job, None, None, &[], None),
    );
    assert_uncommitted(&state.db.lock().unwrap(), &job.id, "processing");
}

#[test]
fn integration_a_directory_in_place_of_the_required_output_is_preserved_and_rejected() {
    let root = TempRoot::new("completion-directory");
    let state = state(&root);
    let job = job(&root, "directory-output", false);
    let output = root.path("Grüße 🌲 supposed media.mp4");
    fs::create_dir(&output).unwrap();
    let unrelated = output.join("existing-file.txt");
    fs::write(&unrelated, "existing synthetic data").unwrap();
    completion::begin(&state.db, &job).unwrap();
    failed_completion_is_an_error(
        &job,
        output.to_str(),
        completion::complete(&state.db, &job, output.to_str(), None, &[], None),
    );
    assert_uncommitted(&state.db.lock().unwrap(), &job.id, "processing");
    assert_eq!(
        fs::read_to_string(unrelated).unwrap(),
        "existing synthetic data"
    );
}

#[test]
fn integration_output_disappearing_after_its_receipt_is_not_successful_and_can_retry_after_reopen()
{
    let root = TempRoot::new("completion-disappeared");
    let initial = state(&root);
    let job = job(&root, "disappeared-output", false);
    let output = root.path("Grüße 🌲 media file.mp4");
    fs::write(&output, b"synthetic media").unwrap();
    completion::begin(&initial.db, &job).unwrap();
    completion::output_ready(&initial.db, &job, output.to_str()).unwrap();
    fs::remove_file(&output).unwrap();
    failed_completion_is_an_error(
        &job,
        output.to_str(),
        completion::complete(&initial.db, &job, output.to_str(), None, &[], None),
    );
    assert_uncommitted(&initial.db.lock().unwrap(), &job.id, "output_ready");
    drop(initial);

    let reopened = state(&root);
    assert_uncommitted(&reopened.db.lock().unwrap(), &job.id, "output_ready");
    fs::write(&output, b"synthetic media").unwrap();
    completion::complete(&reopened.db, &job, output.to_str(), None, &[], None).unwrap();
    assert_eq!(
        list_history_page_from_db(&reopened.db, 25, 0)
            .unwrap()
            .entries
            .len(),
        1
    );
}

#[test]
fn integration_requested_optional_captions_and_thumbnail_can_be_absent_without_failing_the_media() {
    let root = TempRoot::new("completion-optional-metadata");
    let state = state(&root);
    let mut job = job(&root, "optional-absent", false);
    job.save_captions = true;
    job.save_thumbnails = true;
    let output = root.path("Grüße 🌲 media.mp4");
    fs::write(&output, b"synthetic media").unwrap();
    completion::begin(&state.db, &job).unwrap();
    let result = completion::complete(&state.db, &job, output.to_str(), None, &[], None);
    let mut event = DownloadStateEvent {
        id: job.id.clone(),
        state: DownloadState::Success,
        exit_code: Some(0),
        error: None,
        output_path: Some(output.to_str().unwrap().into()),
    };
    event.apply_result(result);
    assert_eq!(event.state, DownloadState::Success);
    assert!(event.error.is_none());
    let details = get_history_details_from_db(&state.db, &job.id)
        .unwrap()
        .unwrap();
    assert!(details.output_file_available);
    assert!(details.entry.thumbnail.is_none());
    assert!(details.captions.is_empty());
    assert!(details.transcript.is_none());
    assert_eq!(fs::read(output).unwrap(), b"synthetic media");
}

#[test]
fn integration_produced_captions_must_remain_readable_unchanged_and_attached_to_existing_media() {
    for failure in [
        "missing-caption",
        "caption-is-directory",
        "changed-caption",
        "invalid-utf8-caption",
        "missing-caption-media",
    ] {
        let root = TempRoot::new(failure);
        let initial = state(&root);
        let job = job(&root, "produced-caption", false);
        let output = root.path("primary media.mp4");
        let media = root.path("Grüße 🌲 caption media.mp4");
        let sidecar = root.path("Grüße 🌲 caption media.caption.txt");
        fs::write(&output, b"primary synthetic media").unwrap();
        fs::write(&media, b"caption synthetic media").unwrap();
        fs::write(&sidecar, "Caption Grüße 🌲\nSecond line").unwrap();
        let caption = SavedCaption {
            media_path: media.to_str().unwrap().into(),
            caption_path: sidecar.to_str().unwrap().into(),
            text: "Caption Grüße 🌲\nSecond line".into(),
        };
        completion::begin(&initial.db, &job).unwrap();
        match failure {
            "missing-caption" => fs::remove_file(&sidecar).unwrap(),
            "caption-is-directory" => {
                fs::remove_file(&sidecar).unwrap();
                fs::create_dir(&sidecar).unwrap();
            }
            "changed-caption" => fs::write(&sidecar, "changed after processing").unwrap(),
            "invalid-utf8-caption" => fs::write(&sidecar, [0xff, 0xfe]).unwrap(),
            "missing-caption-media" => fs::remove_file(&media).unwrap(),
            _ => unreachable!(),
        }
        failed_completion_is_an_error(
            &job,
            output.to_str(),
            completion::complete(
                &initial.db,
                &job,
                output.to_str(),
                None,
                std::slice::from_ref(&caption),
                None,
            ),
        );
        assert_uncommitted(&initial.db.lock().unwrap(), &job.id, "output_ready");
        assert_eq!(fs::read(&output).unwrap(), b"primary synthetic media");
        drop(initial);

        let reopened = state(&root);
        assert_uncommitted(&reopened.db.lock().unwrap(), &job.id, "output_ready");
        if sidecar.is_dir() {
            fs::remove_dir(&sidecar).unwrap();
        }
        fs::write(&sidecar, &caption.text).unwrap();
        fs::write(&media, b"caption synthetic media").unwrap();
        completion::complete(
            &reopened.db,
            &job,
            output.to_str(),
            None,
            std::slice::from_ref(&caption),
            None,
        )
        .unwrap();
        let saved = get_history_caption_from_db(&reopened.db, &job.id, &caption.media_path)
            .unwrap()
            .unwrap();
        assert_eq!(saved.text, caption.text);
        assert!(saved.file_available);
    }
}

#[test]
fn integration_an_unreadable_required_transcript_preserves_the_output_and_can_retry_after_repair() {
    let root = TempRoot::new("completion-invalid-transcript");
    let initial = state(&root);
    let job = job(&root, "invalid-transcript", true);
    let output = root.path("Grüße 🌲 transcript.txt");
    fs::write(&output, [0xff, 0xfe]).unwrap();
    completion::begin(&initial.db, &job).unwrap();
    let result = completion::complete(&initial.db, &job, output.to_str(), None, &[], Some("de"));
    assert!(result.as_ref().unwrap_err().contains("transcript"));
    failed_completion_is_an_error(&job, output.to_str(), result);
    assert_uncommitted(&initial.db.lock().unwrap(), &job.id, "output_ready");
    assert_eq!(fs::read(&output).unwrap(), [0xff, 0xfe]);
    drop(initial);

    let reopened = state(&root);
    assert_uncommitted(&reopened.db.lock().unwrap(), &job.id, "output_ready");
    fs::write(&output, "Transcript Grüße 🌲\nSecond line").unwrap();
    completion::complete(&reopened.db, &job, output.to_str(), None, &[], Some("DE")).unwrap();
    let saved = get_history_transcript_from_db(&reopened.db, &job.id)
        .unwrap()
        .unwrap();
    assert_eq!(saved.text, "Transcript Grüße 🌲\nSecond line");
    assert_eq!(saved.language.as_deref(), Some("de"));
    assert!(saved.file_available);
}

#[test]
fn integration_failed_config_write_keeps_cached_and_persisted_settings_and_allows_retry_after_reopen(
) {
    let root = TempRoot::new("config-write-failed");
    let initial = state(&root);
    update_config(&initial.config, |config| {
        config.last_download_url = Some("https://example.com/preserved".into());
        config.selected_preset_key = Some("audio_mp3".into());
    })
    .unwrap();
    let before = serde_json::to_value(initial.config.lock().unwrap().clone()).unwrap();
    initial
        .db
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER fail_config BEFORE UPDATE ON app_config
         BEGIN SELECT RAISE(ABORT,'synthetic config write failure'); END;",
        )
        .unwrap();
    let result = update_config(&initial.config, |config| {
        config.save_captions = true;
        config.notifications_enabled = true;
    });
    assert!(result.unwrap_err().contains("Config write"));
    assert_eq!(
        serde_json::to_value(initial.config.lock().unwrap().clone()).unwrap(),
        before
    );
    assert_eq!(
        serde_json::to_value(load_config_from_db(&initial.db.lock().unwrap()).unwrap()).unwrap(),
        before
    );
    // Remove our fault injection before the production schema validator reopens it.
    initial
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_config")
        .unwrap();
    drop(initial);

    let reopened = state(&root);
    assert_eq!(
        serde_json::to_value(reopened.config.lock().unwrap().clone()).unwrap(),
        before
    );
    let updated = update_config(&reopened.config, |config| config.save_captions = true).unwrap();
    assert!(updated.save_captions);
    assert_eq!(updated.selected_preset_key.as_deref(), Some("audio_mp3"));
    assert_eq!(
        updated.last_download_url.as_deref(),
        Some("https://example.com/preserved")
    );
    assert_eq!(
        serde_json::to_value(load_config_from_db(&reopened.db.lock().unwrap()).unwrap()).unwrap(),
        serde_json::to_value(updated).unwrap()
    );
}

#[test]
fn integration_corrupted_saved_config_returns_an_error_without_overwriting_it_or_the_cached_config()
{
    let root = TempRoot::new("corrupt-saved-config");
    let initial = state(&root);
    let before = serde_json::to_value(initial.config.lock().unwrap().clone()).unwrap();
    initial
        .db
        .lock()
        .unwrap()
        .execute(
            "UPDATE app_config SET save_captions=?1 WHERE id=1",
            [vec![0xff, 0xfe]],
        )
        .unwrap();
    assert!(load_config_from_db(&initial.db.lock().unwrap())
        .unwrap_err()
        .contains("Config read"));
    assert!(
        update_config(&initial.config, |config| config.notifications_enabled =
            true)
        .unwrap_err()
        .contains("Config read")
    );
    assert_eq!(
        serde_json::to_value(initial.config.lock().unwrap().clone()).unwrap(),
        before
    );
    drop(initial);

    let reopened = database::open(&root.path("pinefetch.sqlite3")).unwrap();
    assert!(load_config_from_db(&reopened).is_err());
    let stored: Vec<u8> = reopened
        .query_row(
            "SELECT save_captions FROM app_config WHERE id=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, [0xff, 0xfe]);
    reopened
        .execute("UPDATE app_config SET save_captions=0 WHERE id=1", [])
        .unwrap();
    let repaired = AppState::new(load_config_from_db(&reopened).unwrap(), reopened);
    assert!(
        update_config(&repaired.config, |config| config.notifications_enabled =
            true)
        .unwrap()
        .notifications_enabled
    );
}

#[test]
fn integration_corrupted_saved_caption_and_transcript_return_controlled_errors_without_mutating_history(
) {
    let root = TempRoot::new("corrupt-saved-content");
    let initial = state(&root);
    let job = job(&root, "saved-content", true);
    let output = root.path("Grüße 🌲 transcript.txt");
    let media = root.path("Grüße 🌲 media.mp4");
    let sidecar = root.path("Grüße 🌲 media.caption.txt");
    fs::write(&output, "Synthetic transcript").unwrap();
    fs::write(&media, b"synthetic media").unwrap();
    fs::write(&sidecar, "Synthetic caption").unwrap();
    let caption = SavedCaption {
        media_path: media.to_str().unwrap().into(),
        caption_path: sidecar.to_str().unwrap().into(),
        text: "Synthetic caption".into(),
    };
    completion::begin(&initial.db, &job).unwrap();
    completion::complete(
        &initial.db,
        &job,
        output.to_str(),
        None,
        std::slice::from_ref(&caption),
        Some("de"),
    )
    .unwrap();
    initial
        .db
        .lock()
        .unwrap()
        .execute_batch("UPDATE captions SET text=x'fffe'; UPDATE transcriptions SET text=x'fffe';")
        .unwrap();
    for _ in 0..2 {
        assert!(
            get_history_caption_from_db(&initial.db, &job.id, &caption.media_path)
                .unwrap_err()
                .contains("Caption read")
        );
        assert!(get_history_transcript_from_db(&initial.db, &job.id)
            .unwrap_err()
            .contains("Transcript read"));
    }
    assert_eq!(
        list_history_page_from_db(&initial.db, 25, 0)
            .unwrap()
            .entries
            .len(),
        1
    );
    drop(initial);

    let reopened = state(&root);
    let conn = reopened.db.lock().unwrap();
    let blobs: (Vec<u8>, Vec<u8>) = conn
        .query_row(
            "SELECT c.text,t.text FROM captions c JOIN transcriptions t
             ON t.history_entry_id=c.history_entry_id WHERE c.history_entry_id=?1",
            [&job.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(blobs, (vec![0xff, 0xfe], vec![0xff, 0xfe]));
    conn.execute("UPDATE captions SET text=?1", [&caption.text])
        .unwrap();
    conn.execute(
        "UPDATE transcriptions SET text=?1",
        ["Synthetic transcript"],
    )
    .unwrap();
    drop(conn);
    assert_eq!(
        get_history_caption_from_db(&reopened.db, &job.id, &caption.media_path)
            .unwrap()
            .unwrap()
            .text,
        caption.text
    );
    assert_eq!(
        get_history_transcript_from_db(&reopened.db, &job.id)
            .unwrap()
            .unwrap()
            .text,
        "Synthetic transcript"
    );
    assert_eq!(fs::read_to_string(output).unwrap(), "Synthetic transcript");
    assert_eq!(fs::read_to_string(sidecar).unwrap(), "Synthetic caption");
    let complete: String = reopened
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT state FROM job_completions WHERE job_id=?1",
            params![job.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(complete, "complete");
}
