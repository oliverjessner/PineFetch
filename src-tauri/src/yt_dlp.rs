use crate::models::DownloadJob;
use crate::models::DownloadProgress;
use crate::models::InfoFormat;
use crate::models::InfoResponse;
use crate::platform::caption_platform;
use crate::platform::site_format_sort;

pub(crate) const PROGRESS_PATTERN: &str =
    r"\[download\]\s+([\d\.]+)%.*?at\s+([^\s]+).*?ETA\s+([^\s]+)";

pub(crate) fn build_download_args(
    job: &DownloadJob,
    output_template: String,
    ffmpeg_location: Option<&str>,
    deno_path: Option<&str>,
) -> Result<Vec<String>, String> {
    let save_captions = caption_platform(&job.url, job.save_captions).is_some();
    let mut args = vec![
        "--no-playlist".to_string(),
        "--newline".to_string(),
        "--progress".to_string(),
        "--no-color".to_string(),
        "--print".to_string(),
        "after_move:filepath".to_string(),
        "--print".to_string(),
        "after_video:filepath".to_string(),
        "--print".to_string(),
        "after_move:pinefetch_metadata:%(.{filepath,title,uploader,thumbnail,upload_date,timestamp,duration})j".to_string(),
        "-f".to_string(),
        job.format.clone(),
        "-o".to_string(),
        output_template,
    ];

    if save_captions {
        args.push("--print".to_string());
        args.push(
            "after_move:pinefetch_caption:%(.{filepath,description,alt_title,title})j".to_string(),
        );
    }

    if job.save_thumbnails {
        args.push("--write-thumbnail".to_string());
    }

    let needs_ffmpeg = job.extract_audio
        || job.transcribe_text
        || job.format.contains('+')
        || job.cut_start_time.is_some();
    if let Some(location) = ffmpeg_location {
        args.push("--ffmpeg-location".to_string());
        args.push(location.to_string());
    } else if needs_ffmpeg {
        return Err(
            "ffmpeg and ffprobe not found. Install ffmpeg (or make sure it is in the same directory as yt-dlp) and try again."
                .to_string(),
        );
    }

    if let Some(deno) = deno_path {
        args.push("--js-runtimes".to_string());
        args.push(format!("deno:{deno}"));
    }

    if job.extract_audio {
        args.push("--extract-audio".to_string());
        if let Some(fmt) = job.audio_format.as_ref() {
            args.push("--audio-format".to_string());
            args.push(fmt.to_string());
        }
    }

    if let Some(format_sort) = site_format_sort(&job.url) {
        args.push("--format-sort".to_string());
        args.push(format_sort.to_string());
    }

    args.push(job.url.clone());

    Ok(args)
}

pub(super) fn parse_download_metadata_line(line: &str) -> Option<(String, InfoResponse)> {
    let json = line.strip_prefix("pinefetch_metadata:")?;
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let path = value.get("filepath")?.as_str()?.trim().to_string();
    if path.is_empty() {
        return None;
    }
    let string = |key: &str| {
        value
            .get(key)
            .and_then(|item| item.as_str())
            .map(str::to_string)
    };
    Some((
        path,
        InfoResponse {
            title: string("title"),
            uploader: string("uploader"),
            duration: value.get("duration").and_then(json_value_to_i64),
            thumbnail: string("thumbnail"),
            upload_date: string("upload_date"),
            timestamp: value.get("timestamp").and_then(json_value_to_i64),
            formats: None,
            description: None,
            id: None,
        },
    ))
}

pub(super) fn parse_caption_line(line: &str, reddit: bool) -> Option<(String, String)> {
    let json = line.strip_prefix("pinefetch_caption:")?;
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let path = value.get("filepath")?.as_str()?.trim();
    if path.is_empty() {
        return None;
    }
    let description = value
        .get("description")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    if reddit {
        let title = value
            .get("alt_title")
            .and_then(|item| item.as_str())
            .filter(|title| !title.trim().is_empty())
            .or_else(|| value.get("title").and_then(|item| item.as_str()))
            .unwrap_or("")
            .trim();
        let body = description.trim();
        let caption = match (title.is_empty(), body.is_empty()) {
            (false, false) => format!("{title}\n\n{body}"),
            (false, true) => title.to_string(),
            (true, false) => body.to_string(),
            (true, true) => return None,
        };
        return Some((path.to_string(), caption));
    }
    if description.trim().is_empty() {
        return None;
    }
    Some((path.to_string(), description.to_string()))
}

pub(super) fn parse_yt_dlp_filepath(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("pinefetch_caption:") {
        return None;
    }

    for prefix in [
        "[download] Destination:",
        "[ExtractAudio] Destination:",
        "[Metadata] Writing metadata to:",
    ] {
        if let Some(candidate) = trimmed.strip_prefix(prefix) {
            return normalize_filepath_candidate(candidate);
        }
    }

    if let Some(candidate) = trimmed.strip_prefix("[Merger] Merging formats into ") {
        return normalize_filepath_candidate(candidate);
    }

    if let Some(candidate) = trimmed
        .strip_prefix("[download] ")
        .and_then(|value| value.strip_suffix(" has already been downloaded"))
    {
        return normalize_filepath_candidate(candidate);
    }

    if trimmed.starts_with('[') {
        return None;
    }

    normalize_filepath_candidate(trimmed)
}

pub(super) fn normalize_filepath_candidate(raw: &str) -> Option<String> {
    let mut candidate = raw.trim();
    if candidate.len() >= 2 {
        let bytes = candidate.as_bytes();
        if (bytes[0] == b'"' && bytes[candidate.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[candidate.len() - 1] == b'\'')
        {
            candidate = &candidate[1..candidate.len() - 1];
        }
    }
    let candidate = candidate.trim();
    if matches!(candidate, "" | "NA" | "N/A" | "None" | "null") {
        return None;
    }
    if candidate.starts_with("http://") || candidate.starts_with("https://") {
        return None;
    }
    Some(candidate.to_string())
}

pub(crate) fn parse_progress_line(
    line: &str,
    pattern: &regex::Regex,
    id: &str,
) -> Option<DownloadProgress> {
    let caps = pattern.captures(line)?;
    Some(DownloadProgress {
        id: id.to_string(),
        percent: caps.get(1).and_then(|m| m.as_str().parse::<f32>().ok()),
        speed: caps.get(2).map(|m| m.as_str().to_string()),
        eta: caps.get(3).map(|m| m.as_str().to_string()),
    })
}

pub(crate) fn json_value_to_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| {
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .map(|value| value.trunc() as i64)
        })
        .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
}

pub(crate) fn parse_info_json(raw: &str) -> Result<InfoResponse, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("Invalid JSON from yt-dlp: {e}"))?;

    let formats = value.get("formats").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .map(|f| InfoFormat {
                format_id: f
                    .get("format_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                ext: f.get("ext").and_then(|v| v.as_str()).map(|s| s.to_string()),
                vcodec: f
                    .get("vcodec")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                acodec: f
                    .get("acodec")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                height: f.get("height").and_then(|v| v.as_i64()),
                width: f.get("width").and_then(|v| v.as_i64()),
                fps: f.get("fps").and_then(|v| v.as_f64()),
            })
            .collect::<Vec<_>>()
    });

    Ok(InfoResponse {
        title: value
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        uploader: value
            .get("uploader")
            .or_else(|| value.get("uploader_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        duration: value.get("duration").and_then(json_value_to_i64),
        thumbnail: value
            .get("thumbnail")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        upload_date: value
            .get("upload_date")
            .or_else(|| value.get("release_date"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        timestamp: value
            .get("timestamp")
            .or_else(|| value.get("release_timestamp"))
            .and_then(json_value_to_i64),
        formats,
        description: value
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        id: value
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}
