//! Resolve YouTube playlists into ordinary, independently queued video jobs.
use crate::download_rules::{prepare_download_job, DownloadOptions};
use crate::models::{AppConfig, DownloadJob, DownloadRequest, InfoResponse};
use crate::process::{run_command_output, ProcessState};
use crate::url_rules::{
    domain_matches, normalized_url_host, parse_http_url, validate_download_url,
};
use serde::Serialize;
use std::process::Command;
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct PlaylistEntry {
    pub(crate) url: String,
    pub(crate) info: InfoResponse,
}

#[derive(Debug)]
pub(crate) struct Playlist {
    pub(crate) title: Option<String>,
    pub(crate) entries: Vec<PlaylistEntry>,
    pub(crate) skipped_count: usize,
}

#[derive(Serialize)]
pub(crate) struct PlaylistQueueResult {
    pub(crate) title: Option<String>,
    pub(crate) jobs: Vec<DownloadJob>,
    pub(crate) skipped_count: usize,
}

pub(crate) fn normalize_playlist_url(input: &str) -> Option<String> {
    validate_download_url(input).ok()?;
    let url = parse_http_url(input)?;
    let host = normalized_url_host(&url)?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| !matches!(port, 80 | 443))
    {
        return None;
    }
    let youtube = domain_matches(&host, "youtube.com");
    let short = host == "youtu.be";
    if !(youtube && matches!(url.path(), "/playlist" | "/watch")
        || short
            && url.path().starts_with('/')
            && url.path()[1..].split('/').count() == 1
            && url.path().len() > 1)
    {
        return None;
    }
    let (_, list) = url.query_pairs().find(|(name, _)| name == "list")?;
    if list.is_empty()
        || !list
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return None;
    }
    Some(format!("https://www.youtube.com/playlist?list={list}"))
}

pub(crate) fn playlist_args(url: &str, deno: Option<&str>) -> Vec<String> {
    let mut args = [
        "--ignore-config",
        "--flat-playlist",
        "--yes-playlist",
        "--dump-single-json",
        "--skip-download",
        "--no-warnings",
        "--ignore-errors",
        "--socket-timeout",
        "20",
        "--retries",
        "2",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    if let Some(deno) = deno {
        args.extend(["--js-runtimes".into(), format!("deno:{deno}")]);
    }
    args.push(url.into());
    args
}

pub(crate) fn parse_playlist(raw: &str) -> Result<Playlist, String> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|error| format!("Invalid playlist response from yt-dlp: {error}"))?;
    let entries = value["entries"]
        .as_array()
        .ok_or("YouTube did not return a playlist.")?;
    let mut playlist = Playlist {
        title: value["title"].as_str().map(str::to_string),
        entries: Vec::with_capacity(entries.len()),
        skipped_count: 0,
    };
    for entry in entries {
        let id = entry["id"].as_str().filter(|id| {
            id.len() == 11
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        });
        let Some(id) = id else {
            playlist.skipped_count += 1;
            continue;
        };
        if matches!(
            entry["availability"].as_str(),
            Some("private" | "unavailable")
        ) || matches!(
            entry["title"].as_str(),
            Some("[Private video]" | "[Deleted video]")
        ) {
            playlist.skipped_count += 1;
            continue;
        }
        let mut info = crate::yt_dlp::parse_info_json(&entry.to_string())?;
        if info.uploader.is_none() {
            info.uploader = entry["channel"].as_str().map(str::to_string);
        }
        if info.thumbnail.is_none() {
            info.thumbnail = entry["thumbnails"].as_array().and_then(|images| {
                images
                    .iter()
                    .rev()
                    .find_map(|image| image["url"].as_str().map(str::to_string))
            });
        }
        playlist.entries.push(PlaylistEntry {
            url: format!("https://www.youtube.com/watch?v={id}"),
            info,
        });
    }
    Ok(playlist)
}

pub(crate) fn load_playlist(
    yt_dlp: &str,
    deno: Option<&str>,
    url: &str,
    state: &ProcessState,
    browser: Option<&str>,
) -> Result<Playlist, String> {
    let url = normalize_playlist_url(url).ok_or("Enter a valid YouTube playlist link.")?;
    let mut command = Command::new(yt_dlp);
    crate::yt_dlp::apply_browser_cookies(&mut command, &url, browser);
    command.args(playlist_args(&url, deno));
    let output = run_command_output(command, None, Some(state), Some(Duration::from_secs(180)))
        .map_err(|error| format!("Could not load the YouTube playlist: {error}"))?;
    let raw = String::from_utf8_lossy(&output.stdout);
    // --ignore-errors may report an unavailable playlist with a zero exit code.
    if !output.status.success() || raw.trim().is_empty() || raw.trim() == "null" {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Could not load the YouTube playlist: {}",
            if error.trim().is_empty() {
                "The playlist is unavailable."
            } else {
                error.trim()
            }
        ));
    }
    parse_playlist(&raw)
}

pub(crate) fn build_jobs(
    request: DownloadRequest,
    playlist: Playlist,
    config: &AppConfig,
    output_dir: String,
) -> PlaylistQueueResult {
    let jobs = playlist
        .entries
        .into_iter()
        .map(|entry| {
            let mut request = request.clone();
            request.url = entry.url;
            request.cut_start_time = None;
            request.title = entry.info.title;
            request.uploader = entry.info.uploader;
            request.thumbnail = entry.info.thumbnail;
            request.upload_date = entry.info.upload_date;
            request.timestamp = entry.info.timestamp;
            request.duration_seconds = entry.info.duration;
            prepare_download_job(
                request,
                DownloadOptions::from(config),
                output_dir.clone(),
                uuid::Uuid::new_v4().to_string(),
            )
        })
        .collect();
    PlaylistQueueResult {
        title: playlist.title,
        jobs,
        skipped_count: playlist.skipped_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_urls_share_frontend_boundaries_and_drop_per_video_state() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../../test/fixtures/youtube-playlists.json"))
                .unwrap();
        for case in cases["valid"].as_array().unwrap() {
            assert_eq!(
                normalize_playlist_url(case[0].as_str().unwrap()),
                Some(format!(
                    "https://www.youtube.com/playlist?list={}",
                    case[1].as_str().unwrap()
                ))
            );
        }
        for input in cases["invalid"].as_array().unwrap() {
            assert_eq!(normalize_playlist_url(input.as_str().unwrap()), None);
        }
    }

    #[test]
    fn playlist_entries_keep_order_metadata_and_duplicate_occurrences_but_skip_unavailable_entries()
    {
        let raw = serde_json::json!({"_type":"playlist", "title":"Test playlist", "entries":[
            {"id":"abcdefghijk", "title":"First", "duration":12.5, "channel":"Creator", "thumbnails":[{"url":"small"},{"url":"large"}], "url":"https://evil.example/ignored"},
            null,
            {"id":"invalid"},
            {"id":"private1234", "availability":"private", "title":"Private"},
            {"id":"deleted1234", "title":"[Deleted video]"},
            {"id":"lmnopqrstuv", "title":"Second", "uploader":"Other", "thumbnail":"direct", "duration":42},
            {"id":"abcdefghijk", "title":"Repeated"}
        ]});
        let playlist = parse_playlist(&raw.to_string()).unwrap();
        assert_eq!(playlist.skipped_count, 4);
        assert_eq!(playlist.entries.len(), 3);
        assert_eq!(
            playlist.entries[0].url,
            "https://www.youtube.com/watch?v=abcdefghijk"
        );
        assert_eq!(
            playlist.entries[0].info.uploader.as_deref(),
            Some("Creator")
        );
        assert_eq!(playlist.entries[0].info.thumbnail.as_deref(), Some("large"));
        assert_eq!(playlist.entries[0].info.duration, Some(12));
        assert_eq!(
            playlist.entries[1].info.thumbnail.as_deref(),
            Some("direct")
        );
        assert_eq!(playlist.entries[2].url, playlist.entries[0].url);
        assert!(parse_playlist("not JSON").is_err());
        assert!(parse_playlist(r#"{"id":"video"}"#).is_err());
        assert!(parse_playlist(r#"{"entries":[]}"#)
            .unwrap()
            .entries
            .is_empty());
    }

    #[test]
    fn playlist_jobs_keep_shared_options_and_enter_the_queue_as_one_ordered_batch() {
        let config = AppConfig {
            save_thumbnails: true,
            save_captions: true,
            faster_whisper_model: "medium".into(),
            ..AppConfig::default()
        };
        let mut request = crate::download_rules::request_from_preset(
            crate::presets::download_preset_for_key(Some("text_timestamps")),
            "https://youtube.com/watch?v=abcdefghijk&list=PL_test&t=90".into(),
            true,
        );
        request.cut_start_time = Some(90.0);
        request.title = Some("Must not leak into each video".into());
        let playlist = parse_playlist(r#"{"title":"Two videos","entries":[{"id":"abcdefghijk","title":"First"},{"id":"lmnopqrstuv","title":"Second"}]}"#).unwrap();
        let result = build_jobs(request, playlist, &config, "/synthetic/output".into());
        assert_eq!(result.title.as_deref(), Some("Two videos"));
        for job in &result.jobs {
            assert_eq!(job.output_dir, "/synthetic/output");
            assert_eq!(job.format, "ba/b");
            assert!(
                job.transcribe_text
                    && job.transcribe_timestamps
                    && job.save_thumbnails
                    && job.save_captions
            );
            assert_eq!(job.faster_whisper_model, "medium");
            assert_eq!(job.cut_start_time, None);
        }
        assert_ne!(result.jobs[0].id, result.jobs[1].id);
        let ids = result
            .jobs
            .iter()
            .map(|job| job.id.clone())
            .collect::<Vec<_>>();
        let queue = crate::queue::QueueState::default();
        crate::queue::enqueue_jobs(&queue, result.jobs).unwrap();
        let queued = queue.pending.lock().unwrap();
        assert_eq!(
            queued.iter().map(|job| job.id.clone()).collect::<Vec<_>>(),
            ids
        );
        assert_eq!(queued[0].title.as_deref(), Some("First"));
        assert_eq!(queued[1].title.as_deref(), Some("Second"));
        assert!(!playlist_args(
            "https://youtube.com/playlist?list=PL_test",
            Some("/synthetic/deno")
        )
        .iter()
        .any(|arg| arg == "--no-playlist"));
    }

    #[cfg(unix)]
    #[test]
    fn integration_playlist_resolution_uses_flat_extraction_and_rejects_failures_before_queueing() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        let root = crate::test_support::TempRoot::new("playlist-resolution");
        let executable = root.path("yt-dlp");
        fs::write(&executable, "#!/bin/sh\nset -eu\n[ \"$1\" = '--ignore-config' ]\n[ \"$2\" = '--flat-playlist' ]\n[ \"$3\" = '--yes-playlist' ]\nprintf '%s\\n' '{\"entries\":[{\"id\":\"abcdefghijk\",\"title\":\"Resolved\"}]}'\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let processes = ProcessState::default();
        let playlist = load_playlist(
            executable.to_str().unwrap(),
            None,
            "https://youtube.com/playlist?list=PL_test",
            &processes,
            None,
        )
        .unwrap();
        assert_eq!(playlist.entries.len(), 1);
        assert!(processes.utility_children.lock().unwrap().is_empty());
        fs::write(
            &executable,
            "#!/bin/sh\necho 'private playlist' >&2\nexit 1\n",
        )
        .unwrap();
        assert!(load_playlist(
            executable.to_str().unwrap(),
            None,
            "https://youtube.com/playlist?list=PL_test",
            &processes,
            None
        )
        .unwrap_err()
        .contains("private playlist"));
        assert!(processes.utility_children.lock().unwrap().is_empty());
        fs::write(
            &executable,
            "#!/bin/sh\necho 'private playlist' >&2\necho null\nexit 0\n",
        )
        .unwrap();
        assert!(load_playlist(
            executable.to_str().unwrap(),
            None,
            "https://youtube.com/playlist?list=PL_test",
            &processes,
            None
        )
        .unwrap_err()
        .contains("private playlist"));
    }
}
