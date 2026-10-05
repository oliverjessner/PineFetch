//! Contract fixtures are synthetic and shared with the browser-side tests.
use crate::database;
use crate::database::Database;
use crate::download_rules::{prepare_download_job, request_from_preset, DownloadOptions};
use crate::history::{get_history_details_from_db, list_history_page_from_db};
use crate::media_tools::{
    build_cut_args, build_transcription_args, build_transcription_audio_args,
    parse_transcription_line, TranscriptionLine,
};
use crate::models::AppConfig;
use crate::platform::detect_platform;
use crate::presets::download_preset_for_key;
use crate::url_rules::validate_download_url;
use crate::video_urls::normalize_video_url;
use crate::yt_dlp::{
    build_download_args, build_filename_probe_args, build_info_args, parse_caption_line,
    parse_download_metadata_line, parse_info_json, parse_yt_dlp_filepath,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

fn url_cases() -> Value {
    serde_json::from_str(include_str!("../../test/fixtures/url-contracts.json")).unwrap()
}

#[test]
fn shared_url_validation_covers_gui_cli_and_import_boundaries() {
    for case in url_cases()["validation"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let valid = case["valid"].as_bool().unwrap();
        assert_eq!(validate_download_url(input).is_ok(), valid, "{input:?}");
        let args = ["queue", "add", "--link", input].map(str::to_string);
        assert_eq!(crate::cli::parse(&args).is_ok(), valid, "CLI: {input:?}");
    }
}

#[test]
fn shared_platform_and_timestamp_vectors_match_backend_rules() {
    for case in url_cases()["platforms"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        assert_eq!(
            detect_platform(input).as_deref(),
            case["platform"].as_str(),
            "{input}"
        );
    }
    for case in url_cases()["timestamps"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        assert_eq!(
            crate::download_rules::extract_url_start_timestamp(input),
            case["seconds"].as_f64(),
            "{input}"
        );
    }
}

#[test]
fn shared_import_vectors_preserve_link_dump_canonicalization() {
    for case in url_cases()["imports"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let actual =
            normalize_video_url(input).map(|value| json!({"url": value.url, "key": value.key}));
        assert_eq!(actual.unwrap_or(Value::Null), case["backend"], "{input}");
    }
}

#[test]
fn malformed_requests_are_rejected_by_job_creation_before_output_resolution() {
    let config = crate::config::ConfigState::new(
        AppConfig::default(),
        std::sync::Arc::new(Database::new(Connection::open_in_memory().unwrap())),
    );
    for case in url_cases()["validation"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["valid"] == false)
    {
        let request = request_from_preset(
            download_preset_for_key(None),
            case["input"].as_str().unwrap().into(),
            true,
        );
        assert_eq!(
            crate::worker::build_download_job(&config, request).unwrap_err(),
            "URL must start with http:// or https://"
        );
    }
}

#[test]
fn download_and_filename_probe_share_audio_deno_and_platform_flags() {
    let request = request_from_preset(
        download_preset_for_key(Some("audio_opus")),
        "https://www.tiktok.com/@creator/video/123456789".into(),
        false,
    );
    let job = prepare_download_job(
        request,
        DownloadOptions::from(&AppConfig::default()),
        "/synthetic/Grüße 🌲".into(),
        "fixed-id".into(),
    );
    let expected_tail = vec![
        "--js-runtimes",
        "deno:/synthetic/Deno tools",
        "--extract-audio",
        "--audio-format",
        "opus",
        "--format-sort",
        "vcodec:h264",
        job.url.as_str(),
    ];
    let download = build_download_args(
        &job,
        "/synthetic/Clip file.%(ext)s".into(),
        Some("/synthetic/FFmpeg tools"),
        Some("/synthetic/Deno tools"),
    )
    .unwrap();
    let probe = build_filename_probe_args(
        &job,
        "/synthetic/Clip file.%(ext)s",
        Some("/synthetic/Deno tools"),
    );
    assert_eq!(
        &download[download.len() - expected_tail.len()..],
        expected_tail
    );
    assert_eq!(&probe[probe.len() - expected_tail.len()..], expected_tail);
    assert_eq!(
        probe[..9],
        [
            "--simulate",
            "--no-playlist",
            "--no-warnings",
            "--print",
            "filename",
            "-f",
            "ba/b",
            "-o",
            "/synthetic/Clip file.%(ext)s"
        ]
    );
    assert_eq!(
        build_info_args(&job.url, Some("/synthetic/Deno tools")),
        [
            "--dump-json",
            "--no-playlist",
            "--no-warnings",
            "--js-runtimes",
            "deno:/synthetic/Deno tools",
            &job.url
        ]
    );
    let plain = prepare_download_job(
        request_from_preset(
            download_preset_for_key(Some("best")),
            "https://example.com/video".into(),
            false,
        ),
        DownloadOptions::from(&AppConfig::default()),
        "/synthetic".into(),
        "other".into(),
    );
    let probe = build_filename_probe_args(&plain, "out", None);
    assert!(!probe.iter().any(|arg| matches!(
        arg.as_str(),
        "--audio-format" | "--extract-audio" | "--js-runtimes" | "--format-sort"
    )));
}

#[test]
fn ffmpeg_and_whisper_argument_builders_preserve_paths_and_modes() {
    let input = "/synthetic/Grüße 🌲; $(echo ignored).mp4";
    let output = "/synthetic/Output file.wav";
    assert_eq!(
        build_cut_args(input, output, "12.5"),
        [
            "-hide_banner",
            "-y",
            "-ss",
            "12.5",
            "-i",
            input,
            "-map",
            "0",
            "-c",
            "copy",
            "-avoid_negative_ts",
            "make_zero",
            output
        ]
    );
    assert_eq!(
        build_transcription_audio_args(input, output),
        [
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            input,
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
            output
        ]
    );
    for (timestamps, expected) in [(false, "0"), (true, "1")] {
        let args =
            build_transcription_args(std::path::Path::new(input), output, "small", timestamps);
        assert_eq!(args[0], "-c");
        assert!(args[1]
            .to_str()
            .unwrap()
            .contains("from faster_whisper import WhisperModel"));
        assert_eq!(
            args[2..],
            [input, output, "small", expected].map(std::ffi::OsStr::new)
        );
    }
}

#[test]
fn transcription_parser_distinguishes_language_metadata_and_unchanged_log_lines() {
    assert_eq!(
        parse_transcription_line("pinefetch_language: DE \t"),
        TranscriptionLine::Language(Some("de".into()))
    );
    assert_eq!(
        parse_transcription_line("pinefetch_language:"),
        TranscriptionLine::Language(None)
    );
    assert_eq!(
        parse_transcription_line("pinefetch_language: \t"),
        TranscriptionLine::Language(None)
    );
    for line in [
        "",
        "Grüße 🌲",
        "/synthetic/Clip file.txt",
        "pinefetch_unknown: value",
        " pinefetch_language: de",
    ] {
        assert_eq!(parse_transcription_line(line), TranscriptionLine::Log(line));
    }
}

#[test]
fn metadata_caption_and_path_parsers_preserve_missing_invalid_and_unknown_fields() {
    let line = r#"pinefetch_metadata:{"filepath":" /synthetic/Grüße 🌲 clip.mp4 ","title":"🌲","duration":"42","extra":{"unknown":true}}"#;
    let (path, info) = parse_download_metadata_line(line).unwrap();
    assert_eq!(path, "/synthetic/Grüße 🌲 clip.mp4");
    assert_eq!(info.title.as_deref(), Some("🌲"));
    assert_eq!(info.duration, Some(42));
    assert!(info.uploader.is_none());
    for line in [
        "",
        "not JSON",
        "pinefetch_metadata:{}",
        "pinefetch_metadata:{\"filepath\":\" \"}",
        "pinefetch_metadata:{\"filepath\":42}",
    ] {
        assert!(parse_download_metadata_line(line).is_none(), "{line}");
    }
    assert!(parse_info_json("").is_err());
    let info = parse_info_json(
        r#"{"uploader":null,"uploader_id":"fallback","release_date":"20261005","extra":true}"#,
    )
    .unwrap();
    assert!(info.uploader.is_none()); // A present null retains the existing alias precedence.
    assert_eq!(info.upload_date.as_deref(), Some("20261005"));
    assert!(parse_info_json("{}").unwrap().title.is_none());
    assert_eq!(
        parse_yt_dlp_filepath(r#"[Merger] Merging formats into "/synthetic/Grüße 🌲 clip.mp4""#)
            .as_deref(),
        Some("/synthetic/Grüße 🌲 clip.mp4")
    );
    for line in ["", "[warning] ignored", "https://example.com/video", "null"] {
        assert!(parse_yt_dlp_filepath(line).is_none());
    }
    assert_eq!(
        parse_caption_line(
            r#"pinefetch_caption:{"filepath":"clip file.mp4","description":"Grüße\n🌲","unknown":7}"#,
            false
        ),
        Some(("clip file.mp4".into(), "Grüße\n🌲".into()))
    );
    assert!(parse_caption_line("pinefetch_caption:{}", false).is_none());
}

#[test]
fn history_list_and_detail_share_all_column_mapping_and_normalization() {
    let conn = Connection::open_in_memory().unwrap();
    database::initialize(&conn).unwrap();
    conn.execute("INSERT INTO history_entries(id,url,title,uploader,filename,thumbnail,upload_date,timestamp,duration_seconds,file_size_bytes,sha256,medium,source,platform,output_path,pinefetch_version,created_at,completed_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)", params!["mapped", "https://www.youtube.com/watch?v=abc123", " 🌲 Title ", " Creator ", " clip.mp4 ", " https://example.com/thumb.jpg ", "20261005", 12, 33, 777, "AB".repeat(32), " AUDIO ", Option::<String>::None, Option::<String>::None, "/synthetic/clip.mp4", "2.2.1", -5, Option::<i64>::None]).unwrap();
    let db = Database::new(conn);
    let listed = list_history_page_from_db(&db, 25, 0)
        .unwrap()
        .entries
        .remove(0);
    let details = get_history_details_from_db(&db, "mapped").unwrap().unwrap();
    let expected = json!({"id":"mapped","url":"https://www.youtube.com/watch?v=abc123","title":"🌲 Title","uploader":"Creator","filename":"clip.mp4","thumbnail":"https://example.com/thumb.jpg","upload_date":"20261005","timestamp":12,"duration_seconds":33,"file_size_bytes":777,"sha256":"ab".repeat(32),"medium":"audio","source":"youtube","platform":"youtube","output_path":"/synthetic/clip.mp4","pinefetch_version":"2.2.1","created_at":0,"completed_at":null});
    assert_eq!(serde_json::to_value(listed).unwrap(), expected);
    assert_eq!(serde_json::to_value(details.entry).unwrap(), expected);
    assert!(get_history_details_from_db(&db, "absent")
        .unwrap()
        .is_none());
    db.lock()
        .unwrap()
        .execute(
            "UPDATE history_entries SET duration_seconds='invalid' WHERE id='mapped'",
            [],
        )
        .unwrap();
    assert!(list_history_page_from_db(&db, 25, 0)
        .unwrap_err()
        .starts_with("History read failed:"));
    assert!(get_history_details_from_db(&db, "mapped")
        .unwrap_err()
        .starts_with("History details read failed:"));
}
