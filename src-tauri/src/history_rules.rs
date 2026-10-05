use crate::models::DownloadJob;
use crate::models::HistoryEntry;
use crate::models::InfoResponse;
use crate::platform::detect_platform;
use std::path::Path;

#[derive(Debug)]
pub(crate) struct HydratedHistoryMetadata {
    pub(crate) title: Option<String>,
    pub(crate) uploader: Option<String>,
    pub(crate) thumbnail: Option<String>,
    pub(crate) upload_date: Option<String>,
    pub(crate) timestamp: Option<i64>,
    pub(crate) duration_seconds: Option<i64>,
}

pub(crate) fn hydrate_history_metadata(
    job: &DownloadJob,
    filename: Option<&str>,
    info: Option<&InfoResponse>,
) -> HydratedHistoryMetadata {
    let mut title = trim_optional_string(job.title.clone());
    let mut uploader = trim_optional_string(job.uploader.clone());
    let mut thumbnail = trim_optional_string(job.thumbnail.clone());
    let mut upload_date = trim_optional_string(job.upload_date.clone());
    let mut timestamp = job.timestamp;
    let mut duration_seconds = job.duration_seconds;

    if let Some(info) = info {
        if title.is_none() {
            title = trim_optional_string(info.title.clone());
        }
        if uploader.is_none() {
            uploader = trim_optional_string(info.uploader.clone());
        }
        if thumbnail.is_none() {
            thumbnail = trim_optional_string(info.thumbnail.clone());
        }
        if upload_date.is_none() {
            upload_date = trim_optional_string(info.upload_date.clone());
        }
        if timestamp.is_none() {
            timestamp = info.timestamp;
        }
        if duration_seconds.is_none() {
            duration_seconds = info.duration;
        }
    }

    if title.is_none() {
        title = title_from_filename(filename);
    }

    HydratedHistoryMetadata {
        title,
        uploader,
        thumbnail,
        upload_date,
        timestamp,
        duration_seconds,
    }
}

pub(crate) fn medium_for_job(job: &DownloadJob) -> &'static str {
    if job.transcribe_text {
        "transcript"
    } else if job.extract_audio {
        "audio"
    } else {
        "video"
    }
}

pub(crate) fn source_from_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.domain()?.trim_end_matches('.').to_ascii_lowercase();

    let canonical_short_domain = match host.as_str() {
        "youtu.be" => Some("youtube"),
        "fb.watch" => Some("facebook"),
        "instagr.am" => Some("instagram"),
        "lnkd.in" => Some("linkedin"),
        "t.co" => Some("x"),
        _ => None,
    };
    if let Some(source) = canonical_short_domain {
        return Some(source.to_string());
    }

    let labels = host
        .split('.')
        .filter(|label| !label.is_empty())
        .collect::<Vec<_>>();
    if labels.is_empty() {
        return None;
    }
    if labels.len() == 1 {
        return Some(labels[0].to_string());
    }

    let common_second_level_tld = matches!(
        labels[labels.len() - 2],
        "ac" | "co" | "com" | "edu" | "gov" | "net" | "org"
    );
    let source_index = if labels.len() >= 3
        && labels.last().is_some_and(|tld| tld.len() == 2)
        && common_second_level_tld
    {
        labels.len() - 3
    } else {
        labels.len() - 2
    };

    Some(labels[source_index].to_string())
}

pub(crate) fn filename_from_path(path: Option<&str>) -> Option<String> {
    let filename = Path::new(path?)
        .file_name()
        .and_then(|value| value.to_str())?
        .trim();
    if filename.is_empty() {
        None
    } else {
        Some(filename.to_string())
    }
}

pub(crate) fn title_from_filename(filename: Option<&str>) -> Option<String> {
    let stem = Path::new(filename?)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(filename?)
        .trim();
    if stem.is_empty() {
        None
    } else {
        Some(stem.to_string())
    }
}

pub(crate) fn trim_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn normalize_history_entry(mut entry: HistoryEntry) -> HistoryEntry {
    entry.title = trim_optional_string(entry.title);
    entry.uploader = trim_optional_string(entry.uploader);
    entry.filename = trim_optional_string(entry.filename)
        .or_else(|| filename_from_path(entry.output_path.as_deref()));
    entry.thumbnail = trim_optional_string(entry.thumbnail);
    entry.upload_date = trim_optional_string(entry.upload_date);
    entry.timestamp = entry.timestamp.filter(|timestamp| *timestamp >= 0);
    entry.duration_seconds = entry.duration_seconds.filter(|duration| *duration >= 0);
    entry.file_size_bytes = entry.file_size_bytes.filter(|size| *size >= 0);
    entry.sha256 = trim_optional_string(entry.sha256)
        .map(|hash| hash.to_ascii_lowercase())
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    entry.medium = trim_optional_string(entry.medium)
        .map(|medium| medium.to_ascii_lowercase())
        .filter(|medium| matches!(medium.as_str(), "video" | "audio" | "transcript"));
    entry.source = trim_optional_string(entry.source)
        .map(|source| source.to_ascii_lowercase())
        .or_else(|| source_from_url(&entry.url));
    entry.platform = trim_optional_string(entry.platform).or_else(|| detect_platform(&entry.url));
    entry.output_path = trim_optional_string(entry.output_path);
    entry.pinefetch_version = trim_optional_string(entry.pinefetch_version);
    if entry.title.is_none() {
        entry.title = title_from_filename(entry.filename.as_deref());
    }
    entry
}
