//! Runtime lookup failures use explicit search paths, never the developer's PATH.
use crate::download_rules::{prepare_download_job, request_from_preset, DownloadOptions};
use crate::models::AppConfig;
use crate::presets::download_preset_for_key;
use crate::runtime::{resolve_output_dir, resolve_yt_dlp_in_paths};
use crate::test_support::TempRoot;
use crate::yt_dlp::build_download_args;

#[test]
fn integration_missing_runtime_and_stale_setting_fail_without_using_ambient_tools() {
    let root = TempRoot::new("runtime-lookup");
    let search = root.path("isolated search path");
    std::fs::create_dir(&search).unwrap();
    let configured = root.path("configured yt-dlp");
    let configured = configured.to_str().unwrap();
    assert!(resolve_yt_dlp_in_paths(None, None)
        .unwrap_err()
        .contains("yt-dlp"));
    assert!(resolve_yt_dlp_in_paths(Some(configured), Some(search.as_os_str())).is_err());
    let discovered = search.join(format!("yt-dlp{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&discovered, b"lookup fixture, not executed").unwrap();
    assert_eq!(
        resolve_yt_dlp_in_paths(Some(configured), Some(search.as_os_str())).unwrap(),
        discovered.to_str().unwrap()
    );
    std::fs::write(configured, b"explicit runtime path").unwrap();
    assert_eq!(
        resolve_yt_dlp_in_paths(Some(configured), Some(search.as_os_str())).unwrap(),
        configured
    );
}

#[test]
fn missing_ffmpeg_rejects_each_operation_that_requires_postprocessing() {
    for preset in ["audio_mp3", "audio_opus", "text", "text_timestamps"] {
        let job = prepare_download_job(
            request_from_preset(
                download_preset_for_key(Some(preset)),
                "https://example.com/clip".into(),
                false,
            ),
            DownloadOptions::from(&AppConfig::default()),
            "/synthetic".into(),
            preset.into(),
        );
        let error = build_download_args(&job, "clip.%(ext)s".into(), None, None).unwrap_err();
        assert!(error.contains("ffmpeg") && error.contains("ffprobe"));
    }
    let mut request = request_from_preset(
        download_preset_for_key(None),
        "https://example.com/clip".into(),
        false,
    );
    for cut in [false, true] {
        request.format = if cut { "best" } else { "bestvideo+bestaudio" }.into();
        request.cut_at_timestamp_enabled = cut;
        request.cut_start_time = cut.then_some(42.0);
        let job = prepare_download_job(
            request.clone(),
            DownloadOptions::from(&AppConfig::default()),
            "/synthetic".into(),
            "job".into(),
        );
        assert!(build_download_args(&job, "clip".into(), None, None).is_err());
    }
}

#[test]
fn missing_output_setting_and_empty_explicit_directory_are_controlled_errors() {
    let state = crate::state::AppState::new(
        AppConfig::default(),
        rusqlite::Connection::open_in_memory().unwrap(),
    );
    assert!(resolve_output_dir(&state.config, None).is_err());
    assert!(resolve_output_dir(&state.config, Some(" ".into())).is_err());
    assert_eq!(
        resolve_output_dir(&state.config, Some("/synthetic/Grüße 🌲".into())).unwrap(),
        "/synthetic/Grüße 🌲"
    );
}
