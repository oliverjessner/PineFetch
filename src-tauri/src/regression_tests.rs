use crate::browser_import::build_link_dump_download_request;
use crate::browser_import::is_link_dump_endpoint;
use crate::config::load_config_from_db;
use crate::config::upsert_app_config_in_conn;
use crate::config_rules::normalize_app_config;
use crate::database::initialize as run_link_dump_migrations;
use crate::download::build_timestamp_cut_output_path;
use crate::download::preserve_unique_cut_output;
use crate::download::related_existing_output_paths;
use crate::download::select_existing_output_path;
use crate::download::write_caption_sidecar;
use crate::download_rules::build_output_template;
use crate::download_rules::effective_download_job;
use crate::download_rules::extract_url_start_timestamp;
use crate::download_rules::format_timestamp_filename_suffix;
use crate::download_rules::format_yt_dlp_timestamp;
use crate::download_rules::normalize_filename_suffix;
use crate::download_rules::resolve_cut_start_time;
use crate::files::canonical_existing_local_path;
use crate::files::TemporaryTranscriptionAudio;
use crate::hashing::sha256_from_path;
use crate::hashing::sha256_hex;
use crate::history::clear_history_entries_in_db;
use crate::history::delete_history_entry_from_db;
use crate::history::get_history_caption_from_db;
use crate::history::get_history_details_from_db;
use crate::history::get_history_stats_from_db;
use crate::history::get_history_transcript_from_db;
use crate::history::insert_captions_in_db;
use crate::history::insert_history_entry_in_db;
use crate::history::insert_transcription_in_db;
use crate::history::list_history_entries_from_db;
use crate::history::list_history_page_from_db;
use crate::history::search_history_page_from_db;
use crate::history_rules::medium_for_job;
use crate::history_rules::source_from_url;
use crate::link_dump_store::create_link_dump_secret_in_db;
use crate::link_dump_store::delete_link_dump_secret_in_db;
use crate::link_dump_store::get_link_dump_settings;
use crate::link_dump_store::hash_link_dump_secret;
use crate::link_dump_store::list_link_dump_secrets;
use crate::link_dump_store::revoke_link_dump_secret_in_db;
use crate::link_dump_store::validate_link_dump_secret;
use crate::models::AppConfig;
use crate::models::DownloadJob;
use crate::models::HistoryEntry;
use crate::models::HistorySourceCount;
use crate::models::HistoryTranscriptSummary;
use crate::models::LinkDumpQueueSummary;
use crate::models::SavedCaption;
use crate::models::DEFAULT_FASTER_WHISPER_MODEL;
use crate::platform::caption_platform;
use crate::platform::detect_platform;
use crate::platform::site_format_sort;
use crate::platform::TIKTOK_FORMAT_SORT;
use crate::presets::download_preset_for_key;
use crate::presets::DEFAULT_DOWNLOAD_PRESET_KEY;
use crate::process::run_command_output;
use crate::process::terminate_child_process_tree;
use crate::process::ProcessError;
use crate::queue::insert_unique_video_jobs;
use crate::queue::next_worker_job;
use crate::queue::set_queue_paused;
use crate::queue::snapshot_queue_status;
use crate::queue::QueueRunSummary;
use crate::runtime::ffmpeg_tool_name;
use crate::runtime::ffprobe_tool_name;
use crate::runtime::normalize_ffmpeg_location;
use crate::state::AppState;
#[cfg(unix)]
use crate::test_support::{DescendantCleanup, FakeProcess, TEST_TIMEOUT};
use crate::video_urls::normalize_video_url;
use crate::video_urls::normalize_youtube_url;
use crate::worker::build_download_job;
use crate::worker::stop_active_download_on_exit;
use crate::yt_dlp::parse_caption_line;
use crate::yt_dlp::parse_download_metadata_line;
use crate::yt_dlp::parse_yt_dlp_filepath;
use rusqlite::params;
use rusqlite::Connection;
use serde_json::json;
use std::collections::VecDeque;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
#[cfg(unix)]
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::time::Instant;
use uuid::Uuid;

fn link_dump_test_state_with_config(config: AppConfig) -> AppState {
    let conn = Connection::open_in_memory().unwrap();
    run_link_dump_migrations(&conn).unwrap();
    upsert_app_config_in_conn(&conn, &config).unwrap();
    AppState::new(config, conn)
}

fn link_dump_test_state() -> AppState {
    link_dump_test_state_with_config(AppConfig::default())
}

#[test]
fn rejects_remote_open_targets() {
    let err = canonical_existing_local_path("https://example.com").unwrap_err();
    assert!(err.contains("local filesystem"));
}

#[test]
fn canonicalizes_existing_local_paths() {
    let temp_dir = std::env::temp_dir();
    let canonical = fs::canonicalize(&temp_dir).unwrap();

    let resolved = canonical_existing_local_path(temp_dir.to_string_lossy().as_ref())
        .unwrap()
        .unwrap();

    assert_eq!(resolved, canonical.to_string_lossy());
}

#[cfg(unix)]
fn write_fake_ffmpeg_tool(path: &Path, exit_code: i32) {
    fs::write(path, format!("#!/bin/sh\nexit {exit_code}\n")).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
#[test]
fn normalizes_only_usable_ffmpeg_locations() {
    let dir = std::env::temp_dir().join(format!("pinefetch-ffmpeg-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    write_fake_ffmpeg_tool(&dir.join(ffmpeg_tool_name()), 0);
    write_fake_ffmpeg_tool(&dir.join(ffprobe_tool_name()), 0);

    assert_eq!(
        normalize_ffmpeg_location(&dir).as_deref(),
        Some(dir.to_string_lossy().as_ref())
    );

    write_fake_ffmpeg_tool(&dir.join(ffprobe_tool_name()), 1);

    assert!(normalize_ffmpeg_location(&dir).is_none());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn extracts_numeric_query_timestamp() {
    let timestamp =
        extract_url_start_timestamp("https://youtu.be/-PrRmxn2U-w?si=gsvuw4hTglBy5UQd&t=61");

    assert_eq!(timestamp, Some(61.0));
}

#[test]
fn extracts_compact_query_timestamp() {
    let timestamp = extract_url_start_timestamp("https://example.com/watch?v=abc&t=1h2m3s");

    assert_eq!(timestamp, Some(3723.0));
}

#[test]
fn extracts_fragment_timestamp() {
    let timestamp = extract_url_start_timestamp("https://example.com/video#t=01:02");

    assert_eq!(timestamp, Some(62.0));
}

#[test]
fn uses_requested_cut_start_time_when_provided() {
    let timestamp = resolve_cut_start_time(true, Some(61.0), "https://example.com/video");

    assert_eq!(timestamp, Some(61.0));
}

#[test]
fn disables_requested_cut_start_time() {
    let timestamp = resolve_cut_start_time(false, Some(61.0), "https://example.com/video?t=62");

    assert_eq!(timestamp, None);
}

#[test]
fn ignores_invalid_or_zero_timestamp() {
    assert_eq!(
        extract_url_start_timestamp("https://example.com/video?t=abc"),
        None
    );
    assert_eq!(
        extract_url_start_timestamp("https://example.com/video?t=0"),
        None
    );
}

#[test]
fn formats_yt_dlp_timestamp_without_extra_zeroes() {
    assert_eq!(format_yt_dlp_timestamp(61.0), "61");
    assert_eq!(format_yt_dlp_timestamp(61.5), "61.5");
}

#[test]
fn appends_filename_suffix_before_extension() {
    let template = build_output_template("/tmp/pinefetch", Some("__max"));

    assert!(template.ends_with("%(title)s - %(uploader)s - %(id)s__max.%(ext)s"));
}

#[test]
fn saves_caption_as_separate_utf8_text_file() {
    let directory = std::env::temp_dir().join(format!("pinefetch-caption-{}", Uuid::new_v4()));
    fs::create_dir_all(&directory).unwrap();
    let media_path = directory.join("post.mp4");
    fs::write(&media_path, b"video").unwrap();
    let line = format!(
        "pinefetch_caption:{}",
        serde_json::json!({
            "filepath": media_path,
            "description": "Grüße aus Wien 👋\n#urlaub"
        })
    );
    let (path, caption) = parse_caption_line(&line, false).unwrap();
    let caption_path = write_caption_sidecar(Path::new(&path), &caption).unwrap();

    assert_eq!(caption_path, directory.join("post.caption.txt"));
    assert_eq!(
        fs::read_to_string(&caption_path).unwrap(),
        "Grüße aus Wien 👋\n#urlaub"
    );
    assert!(parse_yt_dlp_filepath(&line).is_none());
    assert!(parse_caption_line(
        "pinefetch_caption:{\"filepath\":\"x\",\"description\":\"\"}",
        false
    )
    .is_none());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn saves_reddit_title_and_optional_selftext_as_caption() {
    let with_body = r#"pinefetch_caption:{"filepath":"post.mp4","title":"Short title","alt_title":"Full Reddit title","description":"First line\nSecond line"}"#;
    assert_eq!(
        parse_caption_line(with_body, true),
        Some((
            "post.mp4".to_string(),
            "Full Reddit title\n\nFirst line\nSecond line".to_string()
        ))
    );

    let title_only =
        r#"pinefetch_caption:{"filepath":"post.mp4","title":"Reddit title","description":null}"#;
    assert_eq!(
        parse_caption_line(title_only, true),
        Some(("post.mp4".to_string(), "Reddit title".to_string()))
    );
    assert_eq!(parse_caption_line(title_only, false), None);
}

#[test]
fn captures_post_text_only_for_enabled_caption_platforms() {
    for (url, platform) in [
        ("https://www.youtube.com/watch?v=abc123", "youtube"),
        ("https://www.tiktok.com/@user/video/123456789", "tiktok"),
        ("https://www.instagram.com/reel/ABC123/", "instagram"),
        ("https://www.facebook.com/watch/?v=123456", "facebook"),
        ("https://fb.watch/ABC123/", "facebook"),
        ("https://x.com/creator/status/123456789", "x"),
        ("https://twitter.com/creator/status/123456789", "x"),
        (
            "https://www.reddit.com/r/videos/comments/abc123/post/",
            "reddit",
        ),
        ("https://old.reddit.com/comments/abc123", "reddit"),
        ("https://redd.it/abc123", "reddit"),
    ] {
        assert_eq!(caption_platform(url, true).as_deref(), Some(platform));
        assert_eq!(caption_platform(url, false), None);
    }
    assert_eq!(
        caption_platform("https://www.twitch.tv/videos/123456", true),
        None
    );
    assert_eq!(
        caption_platform(
            "https://reddit.com.example.org/r/videos/comments/abc123",
            true
        ),
        None
    );
}

#[test]
fn parses_yt_dlp_filepath_output() {
    assert_eq!(
        parse_yt_dlp_filepath("/tmp/pinefetch/video.mp4").as_deref(),
        Some("/tmp/pinefetch/video.mp4")
    );
    assert_eq!(
        parse_yt_dlp_filepath("[download] Destination: /tmp/pinefetch/video.f398.mp4").as_deref(),
        Some("/tmp/pinefetch/video.f398.mp4")
    );
    assert_eq!(
        parse_yt_dlp_filepath("[Merger] Merging formats into \"/tmp/pinefetch/video.webm\"")
            .as_deref(),
        Some("/tmp/pinefetch/video.webm")
    );
    assert_eq!(parse_yt_dlp_filepath("[download] 100% of 1MiB"), None);
    assert_eq!(parse_yt_dlp_filepath("NA"), None);
    assert_eq!(parse_yt_dlp_filepath("https://example.com/video"), None);
}

#[test]
fn selects_last_existing_output_path() {
    let dir = std::env::temp_dir().join(format!("pinefetch-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let first_path = dir.join("first.mp4");
    let final_path = dir.join("final.mp3");
    fs::write(&first_path, b"first").unwrap();
    fs::write(&final_path, b"final").unwrap();

    let candidates = vec![
        first_path.to_string_lossy().to_string(),
        dir.join("missing.webm").to_string_lossy().to_string(),
        final_path.to_string_lossy().to_string(),
    ];
    let expected = final_path.to_string_lossy().to_string();

    assert_eq!(
        select_existing_output_path(&candidates).as_deref(),
        Some(expected.as_str())
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn selects_largest_format_part_when_final_output_is_missing() {
    let dir = std::env::temp_dir().join(format!("pinefetch-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let audio_part = dir.join("Example - Uploader - id__max.f251.webm");
    let video_part = dir.join("Example - Uploader - id__max.f398.mp4");
    fs::write(&audio_part, b"audio").unwrap();
    fs::write(&video_part, b"larger video part").unwrap();

    let candidates = vec![
        audio_part.to_string_lossy().to_string(),
        video_part.to_string_lossy().to_string(),
    ];
    let expected = video_part.to_string_lossy().to_string();

    assert_eq!(
        select_existing_output_path(&candidates).as_deref(),
        Some(expected.as_str())
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn finds_related_format_parts_from_expected_output_path() {
    let dir = std::env::temp_dir().join(format!("pinefetch-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let expected = dir.join("Example - Uploader - id__max.webm");
    let video_part = dir.join("Example - Uploader - id__max.f398.mp4");
    fs::write(&video_part, b"video").unwrap();

    let related = related_existing_output_paths(&expected);

    assert_eq!(related, vec![video_part.to_string_lossy().to_string()]);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rejects_unsafe_filename_suffix() {
    assert_eq!(
        normalize_filename_suffix(Some("__max")),
        Some("__max".to_string())
    );
    assert_eq!(normalize_filename_suffix(Some("../max")), None);
    assert_eq!(normalize_filename_suffix(Some("")), None);
}

#[test]
fn normalizes_unknown_selected_preset_to_best() {
    let config = normalize_app_config(AppConfig {
        selected_preset_key: Some("missing".to_string()),
        ..AppConfig::default()
    });

    assert_eq!(
        config.selected_preset_key.as_deref(),
        Some(DEFAULT_DOWNLOAD_PRESET_KEY)
    );
}

#[test]
fn normalizes_unknown_faster_whisper_model_to_base() {
    let config = normalize_app_config(AppConfig {
        faster_whisper_model: "unknown".to_string(),
        ..AppConfig::default()
    });

    assert_eq!(config.faster_whisper_model, DEFAULT_FASTER_WHISPER_MODEL);
}

#[test]
fn app_config_defaults_are_loaded_from_sqlite() {
    let conn = Connection::open_in_memory().unwrap();
    run_link_dump_migrations(&conn).unwrap();

    let config = load_config_from_db(&conn).unwrap();

    assert_eq!(
        config.selected_preset_key.as_deref(),
        Some(DEFAULT_DOWNLOAD_PRESET_KEY)
    );
    assert_eq!(config.faster_whisper_model, DEFAULT_FASTER_WHISPER_MODEL);
    assert!(!config.download_video_with_transcript);
    assert!(!config.save_captions);
    assert!(!config.save_thumbnails);
    assert!(config.magic_import_enabled);
    assert!(config.cut_at_timestamp_enabled);
    assert!(config.yt_dlp_path.is_none());
    assert!(config.default_output_dir.is_none());
    assert!(config.last_download_url.is_none());
}

#[test]
fn app_config_is_stored_in_sqlite() {
    let conn = Connection::open_in_memory().unwrap();
    run_link_dump_migrations(&conn).unwrap();
    let config = AppConfig {
        yt_dlp_path: Some("/opt/pinefetch/yt-dlp".to_string()),
        default_output_dir: Some("/Users/example/Downloads".to_string()),
        selected_preset_key: Some("audio_mp3".to_string()),
        faster_whisper_model: "medium".to_string(),
        download_video_with_transcript: true,
        save_captions: true,
        save_thumbnails: true,
        magic_import_enabled: false,
        cut_at_timestamp_enabled: false,
        last_download_url: Some("https://example.com/watch".to_string()),
        notifications_enabled: true,
    };

    upsert_app_config_in_conn(&conn, &config).unwrap();
    let loaded = load_config_from_db(&conn).unwrap();

    assert_eq!(loaded.yt_dlp_path.as_deref(), Some("/opt/pinefetch/yt-dlp"));
    assert_eq!(
        loaded.default_output_dir.as_deref(),
        Some("/Users/example/Downloads")
    );
    assert_eq!(loaded.selected_preset_key.as_deref(), Some("audio_mp3"));
    assert_eq!(loaded.faster_whisper_model, "medium");
    assert!(loaded.download_video_with_transcript);
    assert!(loaded.save_captions);
    assert!(loaded.save_thumbnails);
    assert!(loaded.notifications_enabled);
    assert!(!loaded.magic_import_enabled);
    assert!(!loaded.cut_at_timestamp_enabled);
    assert_eq!(
        loaded.last_download_url.as_deref(),
        Some("https://example.com/watch")
    );
}

#[test]
fn link_dump_request_uses_selected_preset() {
    let state = link_dump_test_state_with_config(AppConfig {
        selected_preset_key: Some("audio_mp3".to_string()),
        ..AppConfig::default()
    });
    let normalized = normalize_youtube_url("https://youtu.be/abc123").unwrap();

    let request = build_link_dump_download_request(&state.config, &normalized).unwrap();

    assert_eq!(request.url, "https://www.youtube.com/watch?v=abc123");
    assert_eq!(request.format, "ba/b");
    assert!(request.extract_audio);
    assert_eq!(request.audio_format.as_deref(), Some("mp3"));
    assert!(!request.transcribe_text);
    assert!(!request.transcribe_timestamps);
    assert_eq!(request.filename_suffix, None);
}

#[test]
fn browser_import_deduplicates_while_inserting_into_queue() {
    let state = link_dump_test_state_with_config(AppConfig {
        default_output_dir: Some(std::env::temp_dir().to_string_lossy().into_owned()),
        ..AppConfig::default()
    });
    let normalized = normalize_youtube_url("https://youtu.be/abc123").unwrap();
    let request = build_link_dump_download_request(&state.config, &normalized).unwrap();
    let first = build_download_job(&state.config, request.clone()).unwrap();
    let second = build_download_job(&state.config, request).unwrap();
    let mut queue = VecDeque::from([first]);
    let mut summary = LinkDumpQueueSummary {
        received: 2,
        added: 0,
        skipped: 0,
        invalid: 0,
    };

    let added = insert_unique_video_jobs(
        &mut queue,
        None,
        vec![(normalized.key.clone(), second)],
        &mut summary,
    );

    assert_eq!(added, 0);
    assert_eq!(summary.skipped, 1);
    assert_eq!(queue.len(), 1);
}

#[test]
fn browser_import_skips_a_link_while_its_download_is_active() {
    let state = link_dump_test_state_with_config(AppConfig {
        default_output_dir: Some(std::env::temp_dir().to_string_lossy().into_owned()),
        ..AppConfig::default()
    });
    let normalized = normalize_youtube_url("https://youtu.be/abc123").unwrap();
    let request = build_link_dump_download_request(&state.config, &normalized).unwrap();
    let first = build_download_job(&state.config, request.clone()).unwrap();
    let second = build_download_job(&state.config, request).unwrap();
    state.queue.pending.lock().unwrap().push_back(first);

    let (active, paused) = next_worker_job(&state.queue, &state.processes.current_job_id).unwrap();
    assert!(active.is_some());
    assert!(!paused);
    assert!(state.queue.pending.lock().unwrap().is_empty());

    let mut summary = LinkDumpQueueSummary {
        received: 1,
        added: 0,
        skipped: 0,
        invalid: 0,
    };
    let mut queue = state.queue.pending.lock().unwrap();
    let active_key = state.queue.active_video_key.lock().unwrap();
    let added = insert_unique_video_jobs(
        &mut queue,
        active_key.as_deref(),
        vec![(normalized.key, second)],
        &mut summary,
    );

    assert_eq!(added, 0);
    assert_eq!(summary.skipped, 1);
    assert!(queue.is_empty());
}

#[test]
fn timestamped_text_preset_enables_segment_timestamps() {
    let preset = download_preset_for_key(Some("text_timestamps"));

    assert_eq!(preset.format, "ba/b");
    assert!(preset.extract_audio);
    assert_eq!(preset.audio_format, Some("mp3"));
    assert!(preset.transcribe_text);
    assert!(preset.transcribe_timestamps);
    assert_eq!(preset.filename_suffix, Some("_timestamps"));
}

#[test]
fn appends_timestamp_suffix_to_cut_output_path() {
    let path =
        build_timestamp_cut_output_path(Path::new("/tmp/Title - Uploader - id_best.webm"), 13.0)
            .unwrap();

    assert_eq!(
        path.to_string_lossy(),
        "/tmp/Title - Uploader - id_best_t13.webm"
    );
}

#[test]
fn sanitizes_decimal_timestamp_suffix() {
    assert_eq!(format_timestamp_filename_suffix(13.5), "_t13_5");
}

#[test]
fn timestamp_cut_keeps_an_existing_output_file() {
    let dir = std::env::temp_dir().join(format!("pinefetch-cut-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("video.webm");
    let temp = dir.join("cut.webm");
    let existing = build_timestamp_cut_output_path(&input, 13.0).unwrap();
    fs::write(&input, b"original").unwrap();
    fs::write(&temp, b"new cut").unwrap();
    fs::write(&existing, b"older cut").unwrap();

    let output = preserve_unique_cut_output(&temp, &input, 13.0).unwrap();

    assert_ne!(output, existing);
    assert_eq!(fs::read(&existing).unwrap(), b"older cut");
    assert_eq!(fs::read(&output).unwrap(), b"new cut");
    assert_eq!(fs::read(&input).unwrap(), b"original");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn platform_detection_requires_a_domain_boundary() {
    assert_eq!(
        detect_platform("https://m.youtube.com/watch?v=123"),
        Some("youtube".to_string())
    );
    assert_eq!(
        detect_platform("https://www.instagram.com/p/abc/"),
        Some("instagram".to_string())
    );
    assert_eq!(detect_platform("https://fakeinstagram.com/p/abc/"), None);
    assert_eq!(detect_platform("https://notyoutube.com/watch?v=123"), None);
    assert_eq!(
        detect_platform("https://pretiktok.com/@user/video/123"),
        None
    );
}

#[test]
fn download_metadata_parser_keeps_fields_from_first_yt_dlp_run() {
    let line = r#"pinefetch_metadata:{"filepath":"/tmp/video.mp4","title":"Clip","uploader":"Creator","duration":13.7,"thumbnail":"https://example.com/thumb.jpg"}"#;
    let (path, info) = parse_download_metadata_line(line).unwrap();
    assert_eq!(path, "/tmp/video.mp4");
    assert_eq!(info.title.as_deref(), Some("Clip"));
    assert_eq!(info.uploader.as_deref(), Some("Creator"));
    assert_eq!(info.duration, Some(13));
}

#[cfg(unix)]
#[test]
fn timed_out_child_is_stopped_promptly() {
    let fake = FakeProcess::new("hang");
    let started = Instant::now();
    let result = run_command_output(fake.command(), None, None, Some(Duration::from_secs(1)));
    let error = result.unwrap_err();
    assert!(matches!(error, ProcessError::TimedOut));
    assert_eq!(error.to_string(), "process timed out");
    assert!(started.elapsed() < TEST_TIMEOUT);
}

#[cfg(unix)]
#[test]
fn inherited_output_pipe_cannot_block_after_parent_exits() {
    let fake = FakeProcess::new("spawn-child");
    let (mut command, control) = fake.controlled_command();
    command.arg("--parent-exits");
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        let _ = sender.send(run_command_output(command, None, None, Some(TEST_TIMEOUT)));
    });
    let ready = control.wait_ready();
    let _cleanup = DescendantCleanup::new(&ready);
    let started = Instant::now();
    ready.release();
    let result = receiver.recv_timeout(TEST_TIMEOUT).unwrap();
    handle.join().unwrap();
    let error = result.unwrap_err();
    assert!(matches!(error, ProcessError::OutputDrain));
    assert_eq!(error.to_string(), "Process output did not close after exit");
    assert!(started.elapsed() < TEST_TIMEOUT);
}

#[cfg(unix)]
#[test]
fn cancellation_stops_a_child_and_its_process_group() {
    let fake = FakeProcess::new("spawn-child");
    let (command, control) = fake.controlled_command();
    let state = Arc::new(crate::process::ProcessState::default());
    let worker_state = state.clone();
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        let _ = sender.send(run_command_output(
            command,
            Some(&worker_state),
            None,
            Some(TEST_TIMEOUT),
        ));
    });
    let ready = control.wait_ready();
    let _cleanup = DescendantCleanup::new(&ready);
    let deadline = Instant::now() + TEST_TIMEOUT;
    let child = loop {
        if let Some(child) = state.current_child.lock().unwrap().clone() {
            break child;
        }
        assert!(Instant::now() < deadline, "child was not registered");
        thread::yield_now();
    };
    let pid = i32::try_from(ready.pid).unwrap();
    let descendant = i32::try_from(ready.child_pid).unwrap();
    assert_eq!(ready.pid, child.lock().unwrap().id());
    assert_eq!(unsafe { libc::getpgid(pid) }, pid);
    assert_eq!(unsafe { libc::getpgid(descendant) }, pid);
    terminate_child_process_tree(&mut child.lock().unwrap()).unwrap();
    let output = receiver.recv_timeout(TEST_TIMEOUT).unwrap().unwrap();
    handle.join().unwrap();
    assert!(!output.status.success());
    assert!(child.lock().unwrap().try_wait().unwrap().is_some());
    crate::test_support::assert_process_stopped(ready.child_pid);
}

#[cfg(unix)]
#[test]
fn app_exit_stops_registered_utility_child() {
    let fake = FakeProcess::new("hang");
    let (command, control) = fake.controlled_command();
    let state = Arc::new(link_dump_test_state());
    let worker_state = state.clone();
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        let _ = sender.send(run_command_output(
            command,
            None,
            Some(&worker_state.processes),
            Some(TEST_TIMEOUT),
        ));
    });
    let ready = control.wait_ready();
    let deadline = Instant::now() + TEST_TIMEOUT;
    let child = loop {
        if let Some(child) = state
            .processes
            .utility_children
            .lock()
            .unwrap()
            .first()
            .cloned()
        {
            break child;
        }
        assert!(
            Instant::now() < deadline,
            "utility child was not registered"
        );
        thread::yield_now();
    };
    assert_eq!(child.lock().unwrap().id(), ready.pid);
    stop_active_download_on_exit(&state);
    let output = receiver.recv_timeout(TEST_TIMEOUT).unwrap().unwrap();
    handle.join().unwrap();
    assert!(!output.status.success());
    assert!(child.lock().unwrap().try_wait().unwrap().is_some());
    assert!(state.processes.utility_children.lock().unwrap().is_empty());
}

#[test]
fn extracts_source_from_url_without_subdomain_or_tld() {
    assert_eq!(
        source_from_url("https://www.youtube.com/watch?v=abc123").as_deref(),
        Some("youtube")
    );
    assert_eq!(
        source_from_url("https://media.linkedin.com/posts/123").as_deref(),
        Some("linkedin")
    );
    assert_eq!(
        source_from_url("https://video.example.co.uk/watch/123").as_deref(),
        Some("example")
    );
    assert_eq!(
        source_from_url("https://youtu.be/abc123").as_deref(),
        Some("youtube")
    );
    assert_eq!(source_from_url("not-a-url"), None);
}

#[test]
fn prefers_h264_for_tiktok_downloads() {
    assert_eq!(
        site_format_sort("https://www.tiktok.com/@afd_fraktionbb/video/7608588575982144790"),
        Some(TIKTOK_FORMAT_SORT)
    );
    assert_eq!(
        site_format_sort("https://vm.tiktok.com/ZMexample/"),
        Some(TIKTOK_FORMAT_SORT)
    );
    assert_eq!(site_format_sort("https://exampletiktok.com/video/1"), None);
    assert_eq!(
        site_format_sort("https://www.youtube.com/watch?v=abc123"),
        None
    );
}

#[test]
fn derives_medium_from_download_job() {
    let mut job = DownloadJob {
        id: "job-1".to_string(),
        url: "https://example.com/video".to_string(),
        format: "best".to_string(),
        output_dir: "/tmp".to_string(),
        extract_audio: false,
        audio_format: None,
        transcribe_text: false,
        transcribe_timestamps: false,
        faster_whisper_model: DEFAULT_FASTER_WHISPER_MODEL.to_string(),
        download_video_with_transcript: false,
        save_captions: false,
        save_thumbnails: false,
        title: None,
        uploader: None,
        thumbnail: None,
        upload_date: None,
        timestamp: None,
        duration_seconds: None,
        cut_start_time: None,
        filename_suffix: None,
    };

    assert_eq!(medium_for_job(&job), "video");
    job.extract_audio = true;
    job.audio_format = Some("mp3".to_string());
    assert_eq!(medium_for_job(&job), "audio");
    job.transcribe_text = true;
    assert_eq!(medium_for_job(&job), "transcript");

    let audio_download_job = effective_download_job(&job);
    assert!(audio_download_job.extract_audio);
    assert_eq!(audio_download_job.audio_format.as_deref(), Some("mp3"));

    job.download_video_with_transcript = true;
    let download_job = effective_download_job(&job);
    assert_eq!(
        download_job.format,
        download_preset_for_key(Some(DEFAULT_DOWNLOAD_PRESET_KEY)).format
    );
    assert!(!download_job.extract_audio);
    assert!(download_job.audio_format.is_none());
}

#[test]
fn removes_temporary_transcription_audio_when_dropped() {
    let dir = std::env::temp_dir().join(format!("pinefetch-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("transcription.wav");
    {
        let _temporary_audio = TemporaryTranscriptionAudio::create_at(path.clone()).unwrap();
        fs::write(&path, b"temporary audio").unwrap();
        assert!(path.exists());
    }

    assert!(!path.exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn link_dump_migration_creates_default_settings() {
    let state = link_dump_test_state();
    let settings = get_link_dump_settings(&state.db).unwrap();

    assert!(settings.server_enabled);
    assert_eq!(settings.host, "127.0.0.1");
    assert_eq!(settings.port, 2255);

    let conn = state.db.lock().unwrap();
    let secret_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM link_dump_secrets", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(secret_count, 0);
}

#[test]
fn link_dump_migration_does_not_create_request_log() {
    let state = link_dump_test_state();
    let conn = state.db.lock().unwrap();
    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'link_dump_request_log'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(table_count, 0);
}

#[test]
fn link_dump_migration_does_not_create_app_meta() {
    let state = link_dump_test_state();
    let conn = state.db.lock().unwrap();
    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'app_meta'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let migrated: i64 = conn
        .query_row(
            "SELECT legacy_config_json_migrated FROM app_config WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(table_count, 0);
    assert_eq!(migrated, 0);
}

#[test]
fn link_dump_migration_adds_history_and_transcription_metadata_columns() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE history_entries (
            id TEXT PRIMARY KEY,
            url TEXT NOT NULL,
            title TEXT,
            filename TEXT,
            thumbnail TEXT,
            upload_date TEXT,
            platform TEXT,
            output_path TEXT,
            created_at INTEGER NOT NULL,
            completed_at INTEGER
        );

        CREATE TABLE captions (
            history_entry_id TEXT NOT NULL,
            media_path TEXT NOT NULL,
            caption_path TEXT NOT NULL,
            text TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (history_entry_id, media_path),
            FOREIGN KEY (history_entry_id) REFERENCES history_entries(id) ON DELETE CASCADE
        );

        CREATE TABLE transcriptions (
            id TEXT PRIMARY KEY,
            history_entry_id TEXT NOT NULL UNIQUE,
            text TEXT NOT NULL,
            "type" TEXT NOT NULL CHECK ("type" IN ('text', 'text with timestamps')),
            FOREIGN KEY (history_entry_id) REFERENCES history_entries(id) ON DELETE CASCADE
        );

        INSERT INTO history_entries (
            id, url, title, filename, thumbnail, upload_date, platform,
            output_path, created_at, completed_at
        ) VALUES (
            'legacy-1', 'https://media.linkedin.com/posts/123', 'Legacy',
            NULL, NULL, NULL, NULL, NULL, 1700000000000, 1700000000100
        );
        "#,
    )
    .unwrap();

    run_link_dump_migrations(&conn).unwrap();
    for column_name in ["timestamp", "duration_seconds", "file_size_bytes"] {
        let column_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('history_entries') WHERE name = ?1 AND type = 'INTEGER'",
                params![column_name],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(column_count, 1, "missing INTEGER column {column_name}");
    }
    for column_name in [
        "uploader",
        "medium",
        "source",
        "pinefetch_version",
        "sha256",
    ] {
        let column_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('history_entries') WHERE name = ?1 AND type = 'TEXT'",
                params![column_name],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(column_count, 1, "missing TEXT column {column_name}");
    }

    let transcription_language_column: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('transcriptions') WHERE name = 'language' AND type = 'TEXT'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(transcription_language_column, 1);

    let source: Option<String> = conn
        .query_row(
            "SELECT source FROM history_entries WHERE id = 'legacy-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(source.as_deref(), Some("linkedin"));
    let pinefetch_version: Option<String> = conn
        .query_row(
            "SELECT pinefetch_version FROM history_entries WHERE id = 'legacy-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pinefetch_version, None);
    let caption_sha256_columns: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('captions') WHERE name = 'sha256' AND type = 'TEXT'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(caption_sha256_columns, 1);
}

#[test]
fn instagram_caption_migration_preserves_legacy_history_and_enforces_foreign_key() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE history_entries (
            id TEXT PRIMARY KEY,
            url TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            completed_at INTEGER
        );
        INSERT INTO history_entries (id, url, created_at)
        VALUES ('legacy-instagram', 'https://www.instagram.com/p/example/', 1);",
    )
    .unwrap();

    run_link_dump_migrations(&conn).unwrap();
    conn.execute(
        "INSERT INTO captions (
            history_entry_id, media_path, caption_path, text, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            "legacy-instagram",
            "/tmp/legacy.mp4",
            "/tmp/legacy.txt",
            "Alte Beschreibung",
            2,
        ],
    )
    .unwrap();
    assert!(conn
        .execute(
            "INSERT INTO captions (
                history_entry_id, media_path, caption_path, text, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params!["missing", "/tmp/missing.mp4", "/tmp/missing.txt", "x", 3],
        )
        .is_err());
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM captions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn link_dump_migration_adds_settings_to_existing_config() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE app_config (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            yt_dlp_path TEXT,
            default_output_dir TEXT,
            selected_preset_key TEXT,
            save_instagram_captions INTEGER NOT NULL DEFAULT 0,
            magic_import_enabled INTEGER NOT NULL DEFAULT 1,
            cut_at_timestamp_enabled INTEGER NOT NULL DEFAULT 1,
            last_download_url TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        INSERT INTO app_config (
            id, selected_preset_key, save_instagram_captions, magic_import_enabled,
            cut_at_timestamp_enabled, created_at, updated_at
        ) VALUES (1, 'text', 1, 1, 1, datetime('now'), datetime('now'));

        CREATE TABLE app_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        INSERT INTO app_meta (key, value)
        VALUES ('legacy_config_json_migrated', '1');
        "#,
    )
    .unwrap();

    run_link_dump_migrations(&conn).unwrap();
    let config = load_config_from_db(&conn).unwrap();

    assert_eq!(config.faster_whisper_model, DEFAULT_FASTER_WHISPER_MODEL);
    assert!(!config.download_video_with_transcript);
    assert!(!config.notifications_enabled);
    assert!(config.save_captions);
    assert!(!config.save_thumbnails);
    let migrated: i64 = conn
        .query_row(
            "SELECT legacy_config_json_migrated FROM app_config WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(migrated, 1);
    // Re-running migrations preserves the user's choice.
    let config = AppConfig {
        notifications_enabled: true,
        save_captions: false,
        ..config
    };
    upsert_app_config_in_conn(&conn, &config).unwrap();
    run_link_dump_migrations(&conn).unwrap();
    assert!(load_config_from_db(&conn).unwrap().notifications_enabled);
    assert!(!load_config_from_db(&conn).unwrap().save_captions);
}

#[test]
fn queue_notification_requires_multiple_successful_downloads_and_opt_in() {
    for (started, succeeded, enabled, expected) in [
        (0, 0, true, false),
        (1, 1, true, false),
        (2, 2, false, false),
        (2, 2, true, true),
        (5, 5, true, true),
        (2, 1, true, false),
        (3, 2, true, false),
        (2, 0, true, false),
    ] {
        let summary = QueueRunSummary { started, succeeded };
        assert_eq!(summary.should_notify(enabled), expected);
    }
}

#[test]
fn history_entries_are_stored_in_sqlite_with_file_metadata() {
    let state = link_dump_test_state();
    let entry = HistoryEntry {
        id: "history-1".to_string(),
        url: "https://www.youtube.com/watch?v=abc123".to_string(),
        title: Some("Example title".to_string()),
        uploader: Some("Example uploader".to_string()),
        filename: Some("Example title - Uploader - abc123.mp4".to_string()),
        thumbnail: Some("https://i.ytimg.com/vi/abc123/mqdefault.jpg".to_string()),
        upload_date: Some("20240501".to_string()),
        timestamp: Some(1_714_560_000),
        duration_seconds: Some(754),
        file_size_bytes: Some(42_000_000),
        sha256: Some("a".repeat(64)),
        medium: Some("video".to_string()),
        source: Some("youtube".to_string()),
        platform: Some("youtube".to_string()),
        output_path: Some("/tmp/Example title - Uploader - abc123.mp4".to_string()),
        pinefetch_version: Some("2.2.0".to_string()),
        created_at: 1_700_000_000_000,
        completed_at: Some(1_700_000_000_100),
    };

    insert_history_entry_in_db(&state.db, &entry).unwrap();
    let entries = list_history_entries_from_db(&state.db).unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "history-1");
    assert_eq!(entries[0].pinefetch_version.as_deref(), Some("2.2.0"));
    assert_eq!(entries[0].title.as_deref(), Some("Example title"));
    assert_eq!(entries[0].uploader.as_deref(), Some("Example uploader"));
    assert_eq!(
        entries[0].filename.as_deref(),
        Some("Example title - Uploader - abc123.mp4")
    );
    assert_eq!(entries[0].url, "https://www.youtube.com/watch?v=abc123");
    assert_eq!(
        entries[0].thumbnail.as_deref(),
        Some("https://i.ytimg.com/vi/abc123/mqdefault.jpg")
    );
    assert_eq!(entries[0].upload_date.as_deref(), Some("20240501"));
    assert_eq!(entries[0].timestamp, Some(1_714_560_000));
    assert_eq!(entries[0].duration_seconds, Some(754));
    assert_eq!(entries[0].file_size_bytes, Some(42_000_000));
    assert_eq!(
        entries[0].sha256.as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
    assert_eq!(entries[0].medium.as_deref(), Some("video"));
    assert_eq!(entries[0].source.as_deref(), Some("youtube"));

    let stats = get_history_stats_from_db(&state.db).unwrap();
    assert_eq!(stats.video_count, 1);
    assert_eq!(stats.total_duration_seconds, 754);
    assert_eq!(stats.total_file_size_bytes, 42_000_000);
    assert_eq!(
        stats.source_counts,
        vec![HistorySourceCount {
            source: "youtube".to_string(),
            count: 1,
        }]
    );
}

#[test]
fn captions_store_unicode_newlines_and_upsert_without_duplication() {
    let state = link_dump_test_state();
    {
        let conn = state.db.lock().unwrap();
        conn.execute(
            "INSERT INTO history_entries (id, url, created_at) VALUES (?1, ?2, ?3)",
            params!["instagram-1", "https://www.instagram.com/p/example/", 1],
        )
        .unwrap();
    }

    let original = SavedCaption {
        media_path: "/tmp/post.mp4".to_string(),
        caption_path: "/tmp/post.txt".to_string(),
        text: "Grüße 🌲\nZweite Zeile".to_string(),
    };
    insert_captions_in_db(&state.db, "instagram-1", std::slice::from_ref(&original)).unwrap();
    insert_captions_in_db(&state.db, "instagram-1", &[original]).unwrap();
    insert_captions_in_db(
        &state.db,
        "instagram-1",
        &[SavedCaption {
            media_path: "/tmp/post.mp4".to_string(),
            caption_path: "/tmp/post-updated.txt".to_string(),
            text: "Aktualisiert ✨\nMehr Text".to_string(),
        }],
    )
    .unwrap();
    insert_captions_in_db(
        &state.db,
        "instagram-1",
        &[SavedCaption {
            media_path: "/tmp/post-second.jpg".to_string(),
            caption_path: "/tmp/post-second.txt".to_string(),
            text: "Zweites Medium 🖼️".to_string(),
        }],
    )
    .unwrap();
    insert_history_entry_in_db(
        &state.db,
        &HistoryEntry {
            id: "instagram-1".to_string(),
            url: "https://www.instagram.com/p/example/".to_string(),
            title: Some("Updated post".to_string()),
            uploader: None,
            filename: None,
            thumbnail: None,
            upload_date: None,
            timestamp: None,
            duration_seconds: None,
            file_size_bytes: None,
            sha256: None,
            medium: Some("video".to_string()),
            source: Some("instagram".to_string()),
            platform: Some("instagram".to_string()),
            output_path: None,
            pinefetch_version: None,
            created_at: 1,
            completed_at: Some(2),
        },
    )
    .unwrap();

    let conn = state.db.lock().unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM captions", [], |row| row.get(0))
        .unwrap();
    let (caption_path, text, caption_sha256): (String, String, String) = conn
        .query_row(
            "SELECT caption_path, text, sha256 FROM captions WHERE history_entry_id = 'instagram-1' AND media_path = '/tmp/post.mp4'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let (second_text, second_sha256): (String, String) = conn
        .query_row(
            "SELECT text, sha256 FROM captions WHERE history_entry_id = 'instagram-1' AND media_path = '/tmp/post-second.jpg'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(caption_path, "/tmp/post-updated.txt");
    assert_eq!(text, "Aktualisiert ✨\nMehr Text");
    assert_eq!(
        caption_sha256,
        "a5f15944d0a0d218da37ffe97bc765f80dc385b47b34efdb0d7af260c9de0156"
    );
    assert_eq!(second_text, "Zweites Medium 🖼️");
    assert_eq!(
        second_sha256,
        "a9f434c157b8912f06759505c3aebfbd94fa6600fc1bb8197d8a208120d437c7"
    );
}

#[test]
fn computes_sha256_for_text_and_files() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );

    let path = std::env::temp_dir().join(format!("pinefetch-sha256-{}", Uuid::new_v4()));
    fs::write(&path, b"PineFetch\n").unwrap();
    assert_eq!(
        sha256_from_path(path.to_str()).unwrap().as_deref(),
        Some("21ddd0703e1fbcde4671e62b15d51386e6f6b22d3af0756e79538a66611ed32e")
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn captions_follow_history_delete_and_clear() {
    let state = link_dump_test_state();
    {
        let conn = state.db.lock().unwrap();
        conn.execute_batch(
            "INSERT INTO history_entries (id, url, created_at) VALUES
                ('instagram-1', 'https://instagram.com/p/one/', 1),
                ('instagram-2', 'https://instagram.com/p/two/', 2);",
        )
        .unwrap();
    }
    for id in ["instagram-1", "instagram-2"] {
        insert_captions_in_db(
            &state.db,
            id,
            &[SavedCaption {
                media_path: format!("/tmp/{id}.mp4"),
                caption_path: format!("/tmp/{id}.txt"),
                text: id.to_string(),
            }],
        )
        .unwrap();
    }

    delete_history_entry_from_db(&state.db, "instagram-1").unwrap();
    {
        let conn = state.db.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM captions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
    clear_history_entries_in_db(&state.db).unwrap();
    let conn = state.db.lock().unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM captions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn history_stats_count_sources_across_all_entries_with_fallbacks() {
    let state = link_dump_test_state();
    {
        let conn = state.db.lock().unwrap();
        for index in 0..22 {
            conn.execute(
                "INSERT INTO history_entries (id, url, created_at) VALUES (?1, ?2, ?3)",
                params![
                    format!("youtube-{index}"),
                    format!("https://youtu.be/video-{index}"),
                    index
                ],
            )
            .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO history_entries (id, url, source, created_at) VALUES
                ('instagram-1', 'https://instagram.com/p/1', 'Instagram', 23),
                ('instagram-2', 'https://instagram.com/p/2', 'instagram', 24),
                ('instagram-3', 'https://instagram.com/p/3', '  INSTAGRAM  ', 25);
             INSERT INTO history_entries (id, url, platform, created_at) VALUES
                ('tiktok-1', 'invalid-url', 'TikTok', 26),
                ('unknown-1', 'invalid-url', NULL, 27);",
        )
        .unwrap();
    }

    let stats = get_history_stats_from_db(&state.db).unwrap();
    assert_eq!(stats.video_count, 27);
    assert_eq!(
        stats.source_counts,
        vec![
            HistorySourceCount {
                source: "youtube".to_string(),
                count: 22,
            },
            HistorySourceCount {
                source: "instagram".to_string(),
                count: 3,
            },
            HistorySourceCount {
                source: "tiktok".to_string(),
                count: 1,
            },
            HistorySourceCount {
                source: "unknown".to_string(),
                count: 1,
            },
        ]
    );
    assert_eq!(
        serde_json::to_value(&stats).unwrap()["source_counts"][0],
        json!({"source": "youtube", "count": 22})
    );
}

#[test]
fn transcriptions_store_text_type_and_history_foreign_key() {
    let state = link_dump_test_state();
    {
        let conn = state.db.lock().unwrap();
        conn.execute_batch(
            r#"
            INSERT INTO history_entries (id, url, created_at)
            VALUES
                ('history-text', 'https://example.com/text', 1),
                ('history-timestamps', 'https://example.com/timestamps', 2),
                ('history-invalid', 'https://example.com/invalid', 3);
            "#,
        )
        .unwrap();
    }

    insert_transcription_in_db(&state.db, "history-text", "Plain transcript", "text", "EN")
        .unwrap();
    insert_transcription_in_db(
        &state.db,
        "history-timestamps",
        "[00:00:01 → 00:00:02] Timestamped transcript",
        "text with timestamps",
        "de",
    )
    .unwrap();

    let invalid = insert_transcription_in_db(
        &state.db,
        "history-invalid",
        "Invalid transcript",
        "invalid",
        "en",
    );
    assert!(invalid.is_err());
    let missing_language = insert_transcription_in_db(
        &state.db,
        "history-invalid",
        "Missing language",
        "text",
        "  ",
    );
    assert!(missing_language.is_err());

    {
        let conn = state.db.lock().unwrap();
        let stored: (String, String, String, String) = conn
            .query_row(
                "SELECT history_entry_id, text, \"type\", language FROM transcriptions WHERE history_entry_id = 'history-timestamps'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(stored.0, "history-timestamps");
        assert_eq!(stored.1, "[00:00:01 → 00:00:02] Timestamped transcript");
        assert_eq!(stored.2, "text with timestamps");
        assert_eq!(stored.3, "de");

        let normalized_language: String = conn
            .query_row(
                "SELECT language FROM transcriptions WHERE history_entry_id = 'history-text'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(normalized_language, "en");
    }

    delete_history_entry_from_db(&state.db, "history-timestamps").unwrap();
    let conn = state.db.lock().unwrap();
    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(remaining, 1);
}

#[test]
fn history_details_load_metadata_and_text_content_separately() {
    let state = link_dump_test_state();
    let directory =
        std::env::temp_dir().join(format!("pinefetch-history-details-{}", Uuid::new_v4()));
    fs::create_dir_all(&directory).unwrap();
    let transcript_path = directory.join("example.txt");
    let existing_caption_path = directory.join("example.caption.txt");
    let missing_caption_path = directory.join("missing.caption.txt");
    fs::write(&transcript_path, "Transcript text").unwrap();
    fs::write(&existing_caption_path, "First caption").unwrap();

    insert_history_entry_in_db(
        &state.db,
        &HistoryEntry {
            id: "history-details".to_string(),
            url: "https://www.youtube.com/watch?v=details".to_string(),
            title: Some("Detailed video".to_string()),
            uploader: Some("Creator".to_string()),
            filename: Some("example.txt".to_string()),
            thumbnail: None,
            upload_date: Some("20260919".to_string()),
            timestamp: Some(1_789_761_600),
            duration_seconds: Some(90),
            file_size_bytes: Some(15),
            sha256: Some("a".repeat(64)),
            medium: Some("transcript".to_string()),
            source: Some("youtube".to_string()),
            platform: Some("YouTube".to_string()),
            output_path: Some(transcript_path.to_string_lossy().into_owned()),
            pinefetch_version: Some("2.1.1".to_string()),
            created_at: 10,
            completed_at: Some(20),
        },
    )
    .unwrap();
    insert_transcription_in_db(
        &state.db,
        "history-details",
        "Transcript text",
        "text",
        "EN",
    )
    .unwrap();
    insert_captions_in_db(
        &state.db,
        "history-details",
        &[
            SavedCaption {
                media_path: directory.join("example.mp4").to_string_lossy().into_owned(),
                caption_path: existing_caption_path.to_string_lossy().into_owned(),
                text: "First caption".to_string(),
            },
            SavedCaption {
                media_path: directory.join("missing.mp4").to_string_lossy().into_owned(),
                caption_path: missing_caption_path.to_string_lossy().into_owned(),
                text: "Stored caption".to_string(),
            },
        ],
    )
    .unwrap();

    let details = get_history_details_from_db(&state.db, "history-details")
        .unwrap()
        .unwrap();
    assert_eq!(details.entry.title.as_deref(), Some("Detailed video"));
    assert!(details.output_file_available);
    assert_eq!(details.file_extension.as_deref(), Some("txt"));
    assert_eq!(
        details.transcript,
        Some(HistoryTranscriptSummary {
            transcription_type: "text".to_string(),
            language: Some("en".to_string()),
            file_available: true,
        })
    );
    assert_eq!(details.captions.len(), 2);
    assert!(details.captions[0].file_available);
    assert!(!details.captions[1].file_available);

    let transcript = get_history_transcript_from_db(&state.db, "history-details")
        .unwrap()
        .unwrap();
    assert_eq!(transcript.text, "Transcript text");
    assert_eq!(transcript.language.as_deref(), Some("en"));

    let caption = get_history_caption_from_db(
        &state.db,
        "history-details",
        &details.captions[1].media_path,
    )
    .unwrap()
    .unwrap();
    assert_eq!(caption.text, "Stored caption");
    assert!(!caption.file_available);
    assert_eq!(caption.format.as_deref(), Some("txt"));
    assert!(get_history_details_from_db(&state.db, "missing")
        .unwrap()
        .is_none());

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn history_details_report_only_available_content_sections() {
    let state = link_dump_test_state();
    let entry = |id: &str| HistoryEntry {
        id: id.to_string(),
        url: format!("https://example.com/{id}"),
        title: Some(id.to_string()),
        uploader: None,
        filename: None,
        thumbnail: None,
        upload_date: None,
        timestamp: None,
        duration_seconds: None,
        file_size_bytes: None,
        sha256: None,
        medium: None,
        source: Some("example".to_string()),
        platform: None,
        output_path: None,
        pinefetch_version: None,
        created_at: 1,
        completed_at: Some(2),
    };

    for id in ["transcript-only", "captions-only", "overview-only"] {
        insert_history_entry_in_db(&state.db, &entry(id)).unwrap();
    }
    insert_transcription_in_db(&state.db, "transcript-only", "Transcript", "text", "en").unwrap();
    insert_captions_in_db(
        &state.db,
        "captions-only",
        &[SavedCaption {
            media_path: "/tmp/captions-only.mp4".to_string(),
            caption_path: "/tmp/captions-only.caption.txt".to_string(),
            text: "Caption".to_string(),
        }],
    )
    .unwrap();

    let transcript_only = get_history_details_from_db(&state.db, "transcript-only")
        .unwrap()
        .unwrap();
    assert!(transcript_only.transcript.is_some());
    assert!(transcript_only.captions.is_empty());

    let captions_only = get_history_details_from_db(&state.db, "captions-only")
        .unwrap()
        .unwrap();
    assert!(captions_only.transcript.is_none());
    assert_eq!(captions_only.captions.len(), 1);

    let overview_only = get_history_details_from_db(&state.db, "overview-only")
        .unwrap()
        .unwrap();
    assert!(overview_only.transcript.is_none());
    assert!(overview_only.captions.is_empty());
}

#[test]
fn history_entries_are_paged_newest_first() {
    let state = link_dump_test_state();

    for index in 0..55 {
        let timestamp = 1_700_000_000_000 + index;
        let entry = HistoryEntry {
            id: format!("history-{index:02}"),
            url: format!("https://example.com/video/{index}"),
            title: Some(format!("Example {index}")),
            uploader: None,
            filename: Some(format!("example-{index}.mp4")),
            thumbnail: None,
            upload_date: None,
            timestamp: None,
            duration_seconds: None,
            file_size_bytes: None,
            sha256: None,
            medium: Some("video".to_string()),
            source: Some("example".to_string()),
            platform: Some("example".to_string()),
            output_path: None,
            pinefetch_version: None,
            created_at: timestamp,
            completed_at: Some(timestamp),
        };
        insert_history_entry_in_db(&state.db, &entry).unwrap();
    }

    let first_page = list_history_page_from_db(&state.db, 20, 0).unwrap();
    let second_page = list_history_page_from_db(&state.db, 20, 20).unwrap();

    assert_eq!(first_page.entries.len(), 20);
    assert!(first_page.has_more);
    assert_eq!(first_page.entries[0].id, "history-54");
    assert_eq!(first_page.entries[19].id, "history-35");
    assert_eq!(second_page.entries.len(), 20);
    assert!(second_page.has_more);
    assert_eq!(second_page.entries[0].id, "history-34");
    assert_eq!(second_page.entries[19].id, "history-15");
}

#[test]
fn history_search_filters_all_entries_before_pagination() {
    let state = link_dump_test_state();
    for index in 0..35 {
        let is_match = index % 3 == 0;
        insert_history_entry_in_db(
            &state.db,
            &HistoryEntry {
                id: format!("history-{index:02}"),
                url: format!("https://example.com/{index}"),
                title: Some(if is_match {
                    format!("Pine needle {index}")
                } else {
                    format!("Other {index}")
                }),
                uploader: None,
                filename: None,
                thumbnail: None,
                upload_date: None,
                timestamp: None,
                duration_seconds: None,
                file_size_bytes: None,
                sha256: None,
                medium: None,
                source: Some("other source".to_string()),
                platform: None,
                output_path: None,
                pinefetch_version: None,
                created_at: index,
                completed_at: None,
            },
        )
        .unwrap();
    }

    let first =
        search_history_page_from_db(&state.db, 5, 0, Some(" pine "), Some("title"), None, false)
            .unwrap();
    let second =
        search_history_page_from_db(&state.db, 5, 5, Some("pine"), Some("title"), None, false)
            .unwrap();
    assert_eq!(first.entries.len(), 5);
    assert!(first.has_more);
    assert_eq!(first.entries[0].id, "history-33");
    assert_eq!(second.entries[0].id, "history-18");
    assert!(second.has_more);
    let final_page =
        search_history_page_from_db(&state.db, 5, 10, Some("pine"), Some("title"), None, false)
            .unwrap();
    assert_eq!(final_page.entries.len(), 2);
    assert!(!final_page.has_more);

    let empty_query =
        search_history_page_from_db(&state.db, 5, 0, Some("  "), Some("title"), None, false)
            .unwrap();
    assert_eq!(empty_query.entries[0].id, "history-34");
}

#[test]
fn history_search_treats_sql_wildcards_as_plain_text() {
    let state = link_dump_test_state();
    for (id, title) in [
        ("literal", r"100%_done C:\notes don't"),
        ("other", "100ABdone C:Xnotes dont"),
    ] {
        insert_history_entry_in_db(
            &state.db,
            &HistoryEntry {
                id: id.to_string(),
                url: format!("https://example.com/{id}"),
                title: Some(title.to_string()),
                uploader: None,
                filename: None,
                thumbnail: None,
                upload_date: None,
                timestamp: None,
                duration_seconds: None,
                file_size_bytes: None,
                sha256: None,
                medium: None,
                source: None,
                platform: None,
                output_path: None,
                pinefetch_version: None,
                created_at: 1,
                completed_at: None,
            },
        )
        .unwrap();
        insert_transcription_in_db(&state.db, id, title, "text", "en").unwrap();
    }
    for field in ["title", "transcript"] {
        for query in ["%_done", r"C:\notes", "don't"] {
            let result = search_history_page_from_db(
                &state.db,
                20,
                0,
                Some(query),
                Some(field),
                None,
                false,
            )
            .unwrap();
            assert_eq!(result.entries.len(), 1, "{field}: {query}");
            assert_eq!(result.entries[0].id, "literal");
        }
    }
}

#[test]
fn history_search_matches_stored_transcripts_before_pagination() {
    let state = link_dump_test_state();
    for index in 0..35 {
        let id = format!("history-{index:02}");
        let source = if index % 2 == 0 { "youtube" } else { "tiktok" };
        {
            let conn = state.db.lock().unwrap();
            conn.execute(
                "INSERT INTO history_entries (id, url, title, source, created_at)
                 VALUES (?1, ?2, 'Pine needle in the title', ?3, ?4)",
                params![id, format!("https://example.com/{index}"), source, index],
            )
            .unwrap();
        }
        if index == 34 {
            // A title/caption match without a transcript must not match this field.
            insert_captions_in_db(
                &state.db,
                &id,
                &[SavedCaption {
                    media_path: "/synthetic/post.mp4".to_string(),
                    caption_path: "/synthetic/post.caption.txt".to_string(),
                    text: "Pine needle in the caption".to_string(),
                }],
            )
            .unwrap();
            continue;
        }
        let timestamped = index % 2 != 0;
        let text = match (index % 3 == 0, timestamped) {
            (true, false) => "A pine needle in the stored transcript",
            (true, true) => "[00:00:01 → 00:00:02] A pine needle in the stored transcript",
            (false, _) => "An unrelated stored transcript",
        };
        let kind = if timestamped {
            "text with timestamps"
        } else {
            "text"
        };
        insert_transcription_in_db(&state.db, &id, text, kind, "en").unwrap();
    }

    let search = |offset, source| {
        search_history_page_from_db(
            &state.db,
            5,
            offset,
            Some(" PINE "),
            Some("transcript"),
            source,
            false,
        )
        .unwrap()
    };
    let first = search(0, None);
    assert_eq!(first.entries.len(), 5);
    assert_eq!(first.entries[0].id, "history-33");
    assert!(first.has_more);
    let second = search(5, None);
    assert_eq!(second.entries.len(), 5);
    assert_eq!(second.entries[0].id, "history-18");
    assert!(second.has_more);
    let last = search(10, None);
    assert_eq!(last.entries.len(), 2);
    assert_eq!(last.entries[0].id, "history-03");
    assert!(!last.has_more);

    let by_source = search(0, Some(" YOUTUBE "));
    assert_eq!(by_source.entries.len(), 5);
    assert!(by_source.has_more);
    assert!(by_source
        .entries
        .iter()
        .all(|entry| entry.source.as_deref() == Some("youtube")));
    let last_by_source = search(5, Some("youtube"));
    assert_eq!(last_by_source.entries.len(), 1);
    assert_eq!(last_by_source.entries[0].id, "history-00");
    assert!(!last_by_source.has_more);

    let no_match = search_history_page_from_db(
        &state.db,
        5,
        0,
        Some("missing phrase"),
        Some("transcript"),
        None,
        false,
    )
    .unwrap();
    assert!(no_match.entries.is_empty());
    assert!(!no_match.has_more);
    let empty_query =
        search_history_page_from_db(&state.db, 5, 0, Some("  "), Some("transcript"), None, false)
            .unwrap();
    assert_eq!(empty_query.entries[0].id, "history-34");
}

#[test]
fn history_search_exact_creator_keeps_platform_and_pagination_scoped() {
    let state = link_dump_test_state();
    {
        let conn = state.db.lock().unwrap();
        for index in 0..25 {
            conn.execute(
                "INSERT INTO history_entries (id, url, title, uploader, source, created_at)
                 VALUES (?1, ?2, 'Blender tutorial', 'Blender', 'youtube', ?3)",
                params![
                    format!("creator-{index:02}"),
                    format!("https://example.com/{index}"),
                    index
                ],
            )
            .unwrap();
        }
        for (id, uploader, source) in [
            ("similar-name", Some("Blender Guru"), "youtube"),
            ("different-case", Some("blender"), "youtube"),
            ("other-platform", Some("Blender"), "tiktok"),
            ("missing-name", None, "youtube"),
            ("literal-name", Some(r"100%_done C:\notes don't"), "youtube"),
        ] {
            conn.execute(
                "INSERT INTO history_entries (id, url, title, uploader, source, created_at)
                 VALUES (?1, ?2, 'Blender tutorial', ?3, ?4, 100)",
                params![id, format!("https://example.com/{id}"), uploader, source],
            )
            .unwrap();
        }
    }
    let search = |offset, query, exact_user| {
        search_history_page_from_db(
            &state.db,
            20,
            offset,
            Some(query),
            Some("user"),
            Some(" YOUTUBE "),
            exact_user,
        )
        .unwrap()
    };
    let first = search(0, "Blender", true);
    assert_eq!(first.entries.len(), 20);
    assert_eq!(first.entries[0].id, "creator-24");
    assert!(first.has_more);
    let last = search(20, "Blender", true);
    assert_eq!(last.entries.len(), 5);
    assert_eq!(last.entries[0].id, "creator-04");
    assert!(!last.has_more);
    assert!(first.entries.iter().chain(&last.entries).all(|entry| {
        entry.uploader.as_deref() == Some("Blender") && entry.source.as_deref() == Some("youtube")
    }));

    let manual = search(0, "Blender", false);
    assert!(manual
        .entries
        .iter()
        .any(|entry| entry.id == "similar-name"));
    assert!(manual
        .entries
        .iter()
        .any(|entry| entry.id == "different-case"));
    assert!(!manual
        .entries
        .iter()
        .any(|entry| entry.id == "other-platform"));
    assert!(search(0, "missing", true).entries.is_empty());
    let literal = search(0, r"100%_done C:\notes don't", true);
    assert_eq!(literal.entries.len(), 1);
    assert_eq!(literal.entries[0].id, "literal-name");
    assert!(!literal.has_more);
}

#[test]
fn history_search_filters_by_field_and_source() {
    let state = link_dump_test_state();
    for (id, title, uploader, source, created_at) in [
        ("instagram", "Mountain view", "alice", "instagram", 3),
        ("youtube", "City walk", "bob", "youtube", 2),
        ("tiktok", "Morning coffee", "alice", "tiktok", 1),
    ] {
        insert_history_entry_in_db(
            &state.db,
            &HistoryEntry {
                id: id.to_string(),
                url: format!("https://example.com/{id}"),
                title: Some(title.to_string()),
                uploader: Some(uploader.to_string()),
                filename: None,
                thumbnail: None,
                upload_date: None,
                timestamp: None,
                duration_seconds: None,
                file_size_bytes: None,
                sha256: None,
                medium: Some("video".to_string()),
                source: Some(source.to_string()),
                platform: None,
                output_path: None,
                pinefetch_version: None,
                created_at,
                completed_at: None,
            },
        )
        .unwrap();
    }
    insert_captions_in_db(
        &state.db,
        "instagram",
        &[SavedCaption {
            media_path: "/tmp/instagram.mp4".to_string(),
            caption_path: "/tmp/instagram.caption.txt".to_string(),
            text: "Golden sunset above the lake".to_string(),
        }],
    )
    .unwrap();

    let by_user =
        search_history_page_from_db(&state.db, 20, 0, Some("alice"), Some("user"), None, false)
            .unwrap();
    assert_eq!(by_user.entries.len(), 2);

    let by_description = search_history_page_from_db(
        &state.db,
        20,
        0,
        Some("sunset"),
        Some("description"),
        None,
        false,
    )
    .unwrap();
    assert_eq!(by_description.entries.len(), 1);
    assert_eq!(by_description.entries[0].id, "instagram");

    let by_source = search_history_page_from_db(
        &state.db,
        20,
        0,
        Some("alice"),
        Some("user"),
        Some("tiktok"),
        false,
    )
    .unwrap();
    assert_eq!(by_source.entries.len(), 1);
    assert_eq!(by_source.entries[0].id, "tiktok");

    let invalid_field =
        search_history_page_from_db(&state.db, 20, 0, Some("alice"), Some("source"), None, false);
    assert!(invalid_field.is_err());
}

#[test]
fn paused_queue_keeps_pending_jobs_until_resumed() {
    let state = link_dump_test_state();
    let job = DownloadJob {
        id: "pending".to_string(),
        url: "https://example.com/video".to_string(),
        format: "best".to_string(),
        output_dir: "/tmp".to_string(),
        extract_audio: false,
        audio_format: None,
        transcribe_text: false,
        transcribe_timestamps: false,
        faster_whisper_model: "base".to_string(),
        download_video_with_transcript: false,
        save_captions: false,
        save_thumbnails: false,
        title: None,
        uploader: None,
        thumbnail: None,
        upload_date: None,
        timestamp: None,
        duration_seconds: None,
        cut_start_time: None,
        filename_suffix: None,
    };
    state.queue.pending.lock().unwrap().push_back(job);
    *state.queue.worker_running.lock().unwrap() = true;

    set_queue_paused(&state.queue, true).unwrap();
    let (next, paused) = next_worker_job(&state.queue, &state.processes.current_job_id).unwrap();
    assert!(next.is_none());
    assert!(paused);
    assert_eq!(state.queue.pending.lock().unwrap().len(), 1);
    assert!(!snapshot_queue_status(&state.queue).unwrap().worker_running);
    assert!(snapshot_queue_status(&state.queue).unwrap().paused);

    set_queue_paused(&state.queue, false).unwrap();
    let (next, paused) = next_worker_job(&state.queue, &state.processes.current_job_id).unwrap();
    assert_eq!(next.unwrap().id, "pending");
    assert!(!paused);
    assert!(state.queue.pending.lock().unwrap().is_empty());
}

#[test]
fn link_dump_secret_is_hashed_and_validates() {
    let state = link_dump_test_state();
    let generated =
        create_link_dump_secret_in_db(&state.db, Some("Chrome Extension on MacBook".to_string()))
            .unwrap();

    assert!(generated.secret.starts_with("pfld_"));
    assert_eq!(generated.connection.status, "active");

    {
        let conn = state.db.lock().unwrap();
        let stored_hash: String = conn
            .query_row(
                "SELECT secret_hash FROM link_dump_secrets WHERE id = ?1",
                params![generated.connection.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_ne!(stored_hash, generated.secret);
        assert_eq!(stored_hash, hash_link_dump_secret(&generated.secret));
    }

    let valid = validate_link_dump_secret(&state.db, Some(&generated.secret))
        .unwrap()
        .unwrap();
    assert_eq!(valid.id, generated.connection.id);

    let conn = state.db.lock().unwrap();
    let last_used_at: Option<String> = conn
        .query_row(
            "SELECT last_used_at FROM link_dump_secrets WHERE id = ?1",
            params![generated.connection.id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(last_used_at.is_some());
}

#[test]
fn link_dump_secret_rejects_wrong_revoked_and_deleted_values() {
    let state = link_dump_test_state();
    let generated =
        create_link_dump_secret_in_db(&state.db, Some("Profile A".to_string())).unwrap();

    assert!(validate_link_dump_secret(&state.db, Some("pfld_wrong"))
        .unwrap()
        .is_none());

    revoke_link_dump_secret_in_db(&state.db, &generated.connection.id).unwrap();
    assert!(
        validate_link_dump_secret(&state.db, Some(&generated.secret))
            .unwrap()
            .is_none()
    );

    let second = create_link_dump_secret_in_db(&state.db, Some("Profile B".to_string())).unwrap();
    delete_link_dump_secret_in_db(&state.db, &second.connection.id).unwrap();
    assert!(validate_link_dump_secret(&state.db, Some(&second.secret))
        .unwrap()
        .is_none());
}

#[test]
fn link_dump_secret_list_hides_deleted_connections() {
    let state = link_dump_test_state();
    let first = create_link_dump_secret_in_db(&state.db, Some("Profile A".to_string())).unwrap();
    let second = create_link_dump_secret_in_db(&state.db, Some("Profile B".to_string())).unwrap();

    delete_link_dump_secret_in_db(&state.db, &second.connection.id).unwrap();

    let secrets = list_link_dump_secrets(&state.db).unwrap();
    assert_eq!(secrets.len(), 1);
    assert_eq!(secrets[0].id, first.connection.id);
    assert_eq!(secrets[0].status, "active");

    let conn = state.db.lock().unwrap();
    let deleted_at: Option<String> = conn
        .query_row(
            "SELECT deleted_at FROM link_dump_secrets WHERE id = ?1",
            params![second.connection.id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(deleted_at.is_some());
}

#[test]
fn normalizes_youtube_watch_url() {
    let normalized =
        normalize_youtube_url("https://www.youtube.com/watch?v=abc123&t=42s&feature=share")
            .unwrap();

    assert_eq!(normalized.url, "https://www.youtube.com/watch?v=abc123");
    assert_eq!(normalized.key, "youtube:abc123");
    assert_eq!(
        normalized.thumbnail.as_deref(),
        Some("https://i.ytimg.com/vi/abc123/mqdefault.jpg")
    );
}

#[test]
fn normalizes_youtu_be_url() {
    let normalized = normalize_youtube_url("https://youtu.be/def456?si=tracking").unwrap();

    assert_eq!(normalized.url, "https://www.youtube.com/watch?v=def456");
    assert_eq!(normalized.key, "youtube:def456");
}

#[test]
fn rejects_non_youtube_domains() {
    assert!(normalize_youtube_url("https://example.com/watch?v=abc123").is_none());
    assert!(normalize_youtube_url("https://youtube.example.com/watch?v=abc123").is_none());
}

#[test]
fn normalizes_shorts_and_live_urls() {
    assert_eq!(
        normalize_youtube_url("https://www.youtube.com/shorts/short1")
            .unwrap()
            .url,
        "https://www.youtube.com/watch?v=short1"
    );
    assert_eq!(
        normalize_youtube_url("https://www.youtube.com/live/live99")
            .unwrap()
            .url,
        "https://www.youtube.com/watch?v=live99"
    );
}

#[test]
fn normalizes_tiktok_video_and_short_urls() {
    let video = normalize_video_url(
        "https://www.tiktok.com/@creator.name/video/7412345678901234567?is_from_webapp=1",
    )
    .unwrap();
    assert_eq!(
        video.url,
        "https://www.tiktok.com/@creator.name/video/7412345678901234567"
    );
    assert_eq!(video.key, "tiktok:7412345678901234567");
    assert_eq!(video.thumbnail, None);

    let short = normalize_video_url("https://vm.tiktok.com/ZMexample/?share=1").unwrap();
    assert_eq!(short.url, "https://vm.tiktok.com/ZMexample/");
    assert_eq!(short.key, "tiktok-short:ZMexample");
}

#[test]
fn normalizes_instagram_post_reel_and_tv_urls() {
    let reel = normalize_video_url(
        "https://www.instagram.com/reel/ABC_def-123/?utm_source=ig_web_copy_link",
    )
    .unwrap();
    assert_eq!(reel.url, "https://www.instagram.com/reel/ABC_def-123/");
    assert_eq!(reel.key, "instagram:ABC_def-123");

    assert_eq!(
        normalize_video_url("https://instagram.com/creator/p/PostCode9/")
            .unwrap()
            .url,
        "https://www.instagram.com/p/PostCode9/"
    );
    assert!(normalize_video_url("https://www.instagram.com/tv/TvCode1/").is_some());
}

#[test]
fn normalizes_facebook_video_reel_and_short_urls() {
    let watch = normalize_video_url("https://m.facebook.com/watch/?v=3676516585958356&ref=sharing")
        .unwrap();
    let reel = normalize_video_url("https://www.facebook.com/reel/3676516585958356/").unwrap();
    let video =
        normalize_video_url("https://facebook.com/creator/videos/3676516585958356/").unwrap();
    assert_eq!(
        watch.url,
        "https://www.facebook.com/watch/?v=3676516585958356"
    );
    assert_eq!(watch.key, "facebook:3676516585958356");
    assert_eq!(watch.key, reel.key);
    assert_eq!(watch.key, video.key);

    let short = normalize_video_url("https://fb.watch/AbC123xy/?mibextid=abc").unwrap();
    assert_eq!(short.url, "https://fb.watch/AbC123xy/");
    assert_eq!(short.key, "facebook-short:AbC123xy");

    let share =
        normalize_video_url("https://www.facebook.com/share/r/AbC123xy/?mibextid=abc").unwrap();
    assert_eq!(share.url, "https://www.facebook.com/share/r/AbC123xy/");

    let post =
        normalize_video_url("https://www.facebook.com/creator/posts/3676516585958356/?ref=share")
            .unwrap();
    assert_eq!(
        post.url,
        "https://www.facebook.com/creator/posts/3676516585958356/"
    );
    assert_eq!(post.key, "facebook-post:3676516585958356");
}

#[test]
fn normalizes_x_and_twitter_status_urls() {
    let x = normalize_video_url("https://x.com/creator/status/1821234567890123456/video/1?s=20")
        .unwrap();
    let twitter =
        normalize_video_url("https://mobile.twitter.com/creator/status/1821234567890123456")
            .unwrap();
    assert_eq!(x.url, "https://x.com/creator/status/1821234567890123456");
    assert_eq!(x.key, "x:1821234567890123456");
    assert_eq!(x.key, twitter.key);
    assert_eq!(
        normalize_video_url("https://x.com/i/web/status/1821234567890123456")
            .unwrap()
            .key,
        x.key
    );
}

#[test]
fn normalizes_reddit_post_and_short_urls() {
    let post = normalize_video_url(
        "https://www.reddit.com/r/videos/comments/124pp33/example/?utm_source=share",
    )
    .unwrap();
    let short = normalize_video_url("https://redd.it/124pp33").unwrap();
    assert_eq!(post.url, "https://www.reddit.com/comments/124pp33/");
    assert_eq!(post.key, "reddit:124pp33");
    assert_eq!(post.key, short.key);
    assert_eq!(
        normalize_video_url("https://old.reddit.com/user/creator/comments/124pp33/example/")
            .unwrap()
            .key,
        post.key
    );
}

#[test]
fn video_normalizer_rejects_unsupported_or_spoofed_domains() {
    assert!(normalize_video_url("https://example.com/video/123456").is_none());
    assert!(
        normalize_video_url("https://tiktok.example.com/@creator/video/7412345678901234567")
            .is_none()
    );
    assert!(normalize_video_url("https://instagram.com.example.org/reel/ABC123/").is_none());
    assert!(
        normalize_video_url("https://facebook.com.example.org/watch/?v=3676516585958356").is_none()
    );
    assert!(
        normalize_video_url("https://x.com.example.org/creator/status/1821234567890123456")
            .is_none()
    );
    assert!(normalize_video_url("https://reddit.com.example.org/comments/124pp33/").is_none());
    assert!(
        normalize_video_url("https://www.facebook.com/profile.php?id=3676516585958356").is_none()
    );
    assert!(normalize_video_url("https://x.com/creator").is_none());
    assert!(normalize_video_url("https://www.reddit.com/r/videos/").is_none());
}

#[test]
fn link_dump_exposes_video_endpoints_only() {
    assert!(is_link_dump_endpoint("/addVideoLinkToQueue/"));
    assert!(is_link_dump_endpoint("/addVideoLinksToQueue"));
    assert!(!is_link_dump_endpoint("/addYoutubeLinkToQueue/"));
    assert!(!is_link_dump_endpoint("/addYoutubeLinksToQueue/"));
}
