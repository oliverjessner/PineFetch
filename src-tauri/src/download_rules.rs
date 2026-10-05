use crate::config_rules::normalize_faster_whisper_model;
use crate::models::DownloadJob;
use crate::models::DownloadRequest;
use crate::presets::download_preset_for_key;
use crate::presets::DownloadPreset;
use crate::presets::DEFAULT_DOWNLOAD_PRESET_KEY;
use std::path::PathBuf;

pub(crate) fn resolve_cut_start_time(
    cut_at_timestamp_enabled: bool,
    requested_cut_start_time: Option<f64>,
    url: &str,
) -> Option<f64> {
    if !cut_at_timestamp_enabled {
        return None;
    }

    requested_cut_start_time
        .and_then(normalize_positive_timestamp)
        .or_else(|| extract_url_start_timestamp(url))
}

pub(crate) fn extract_url_start_timestamp(raw_url: &str) -> Option<f64> {
    let parsed = url::Url::parse(raw_url).ok()?;

    for (name, value) in parsed.query_pairs() {
        if is_start_timestamp_param(name.as_ref()) {
            if let Some(seconds) = parse_timestamp_value(value.as_ref()) {
                return Some(seconds);
            }
        }
    }

    if let Some(fragment) = parsed.fragment() {
        for (name, value) in url::form_urlencoded::parse(fragment.as_bytes()) {
            if is_start_timestamp_param(name.as_ref()) {
                if let Some(seconds) = parse_timestamp_value(value.as_ref()) {
                    return Some(seconds);
                }
            }
        }

        parse_timestamp_value(fragment)
    } else {
        None
    }
}

pub(crate) fn is_start_timestamp_param(name: &str) -> bool {
    matches!(name, "t" | "start" | "start_time" | "time_continue")
}

pub(crate) fn parse_timestamp_value(raw: &str) -> Option<f64> {
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }

    if let Ok(seconds) = value.parse::<f64>() {
        return normalize_positive_timestamp(seconds);
    }

    if value.contains(':') {
        return parse_colon_timestamp(&value);
    }

    parse_unit_timestamp(&value)
}

pub(crate) fn parse_colon_timestamp(value: &str) -> Option<f64> {
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }

    let mut total = 0.0;
    for part in parts {
        if part.is_empty() {
            return None;
        }
        let value = part.parse::<f64>().ok()?;
        if !value.is_finite() || value < 0.0 {
            return None;
        }
        total = total * 60.0 + value;
    }

    normalize_positive_timestamp(total)
}

pub(crate) fn parse_unit_timestamp(value: &str) -> Option<f64> {
    let mut number = String::new();
    let mut total = 0.0;
    let mut saw_unit = false;

    for character in value.chars() {
        if character.is_ascii_digit() || character == '.' {
            number.push(character);
            continue;
        }

        let multiplier = match character {
            'h' => 3600.0,
            'm' => 60.0,
            's' => 1.0,
            _ => return None,
        };
        if number.is_empty() {
            return None;
        }
        let amount = number.parse::<f64>().ok()?;
        if !amount.is_finite() || amount < 0.0 {
            return None;
        }
        total += amount * multiplier;
        number.clear();
        saw_unit = true;
    }

    if !saw_unit || !number.is_empty() {
        return None;
    }

    normalize_positive_timestamp(total)
}

pub(crate) fn normalize_positive_timestamp(seconds: f64) -> Option<f64> {
    if seconds.is_finite() && seconds > 0.0 {
        Some(seconds)
    } else {
        None
    }
}

pub(crate) fn format_yt_dlp_timestamp(seconds: f64) -> String {
    let mut formatted = format!("{seconds:.3}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
}

pub(crate) fn effective_download_job(job: &DownloadJob) -> DownloadJob {
    let mut download_job = job.clone();
    if job.transcribe_text && job.download_video_with_transcript {
        download_job.format = download_preset_for_key(Some(DEFAULT_DOWNLOAD_PRESET_KEY))
            .format
            .to_string();
        download_job.extract_audio = false;
        download_job.audio_format = None;
    }
    download_job
}

pub(crate) fn format_timestamp_filename_suffix(seconds: f64) -> String {
    let mut timestamp = format_yt_dlp_timestamp(seconds);
    timestamp = timestamp
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();
    format!("_t{timestamp}")
}

pub(crate) fn build_output_template(output_dir: &str, filename_suffix: Option<&str>) -> String {
    let mut path = PathBuf::from(output_dir);
    // Use title, but fallback to uploader and id for platforms where title might be missing or duplicate
    // %(title)s - video title
    // %(uploader)s - uploader name
    // %(id)s - unique video ID (ensures uniqueness for Instagram posts from same creator)
    let suffix = filename_suffix.unwrap_or("");
    path.push(format!("%(title)s - %(uploader)s - %(id)s{suffix}.%(ext)s"));
    path.to_string_lossy().to_string()
}

pub(crate) fn normalize_filename_suffix(raw: Option<&str>) -> Option<String> {
    let suffix = raw?.trim();
    if suffix.is_empty()
        || suffix.len() > 32
        || !suffix.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
    {
        return None;
    }

    Some(suffix.to_string())
}

// Snapshot only the options a job needs while holding the configuration lock.
pub(crate) struct DownloadOptions {
    faster_whisper_model: String,
    download_video_with_transcript: bool,
    save_captions: bool,
    save_thumbnails: bool,
}

impl From<&crate::models::AppConfig> for DownloadOptions {
    fn from(config: &crate::models::AppConfig) -> Self {
        Self {
            faster_whisper_model: normalize_faster_whisper_model(&config.faster_whisper_model),
            download_video_with_transcript: config.download_video_with_transcript,
            save_captions: config.save_captions,
            save_thumbnails: config.save_thumbnails,
        }
    }
}

pub(crate) fn prepare_download_job(
    request: DownloadRequest,
    options: DownloadOptions,
    output_dir: String,
    id: String,
) -> DownloadJob {
    let cut_start_time = resolve_cut_start_time(
        request.cut_at_timestamp_enabled,
        request.cut_start_time,
        &request.url,
    );
    DownloadJob {
        id,
        url: request.url,
        format: request.format,
        output_dir,
        extract_audio: request.extract_audio,
        audio_format: request.audio_format,
        transcribe_text: request.transcribe_text,
        transcribe_timestamps: request.transcribe_timestamps,
        faster_whisper_model: options.faster_whisper_model,
        download_video_with_transcript: options.download_video_with_transcript,
        save_captions: options.save_captions,
        save_thumbnails: options.save_thumbnails,
        title: request.title,
        uploader: request.uploader,
        thumbnail: request.thumbnail,
        upload_date: request.upload_date,
        timestamp: request.timestamp,
        duration_seconds: request.duration_seconds,
        cut_start_time,
        filename_suffix: normalize_filename_suffix(request.filename_suffix.as_deref()),
    }
}

pub(crate) fn request_from_preset(
    preset: &DownloadPreset,
    url: String,
    cut_at_timestamp_enabled: bool,
) -> DownloadRequest {
    DownloadRequest {
        url,
        format: preset.format.into(),
        output_dir: None,
        extract_audio: preset.extract_audio,
        audio_format: preset.audio_format.map(str::to_string),
        transcribe_text: preset.transcribe_text,
        transcribe_timestamps: preset.transcribe_timestamps,
        cut_at_timestamp_enabled,
        cut_start_time: None,
        filename_suffix: preset.filename_suffix.map(str::to_string),
        title: None,
        uploader: None,
        thumbnail: None,
        upload_date: None,
        timestamp: None,
        duration_seconds: None,
    }
}
