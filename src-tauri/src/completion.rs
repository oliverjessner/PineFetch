//! A small completion receipt, not a persisted queue or automatic retry system.
use crate::files::current_timestamp_millis;
use crate::files::file_size_bytes_from_path;
use crate::files::sync_output;
use crate::hashing::sha256_from_path;
use crate::history_rules::filename_from_path;
use crate::history_rules::hydrate_history_metadata;
use crate::history_rules::medium_for_job;
use crate::history_rules::source_from_url;
use crate::models::DownloadJob;
use crate::models::HistoryEntry;
use crate::models::InfoResponse;
use crate::models::SavedCaption;
use crate::platform::detect_platform;
use std::fs;
use std::path::Path;

pub(crate) use crate::completion_store::{begin, known_output, CompletionPoint};

fn require_output(path: Option<&str>) -> Result<&str, String> {
    let path = path.ok_or("Required output file is missing; completion not committed")?;
    let metadata = fs::metadata(path)
        .map_err(|e| format!("Required output file unavailable: {e}; completion not committed"))?;
    if !metadata.is_file() {
        return Err("Required output is not a regular file; completion not committed".into());
    }
    Ok(path)
}

#[cfg(test)]
pub(super) fn complete(
    state: &crate::database::Database,
    job: &DownloadJob,
    path: Option<&str>,
    info: Option<&InfoResponse>,
    captions: &[SavedCaption],
    language: Option<&str>,
) -> Result<(), String> {
    complete_with_hook(state, job, path, info, captions, language, |_| Ok(()))
}

#[cfg(test)]
pub(super) fn complete_with_hook(
    state: &crate::database::Database,
    job: &DownloadJob,
    path: Option<&str>,
    info: Option<&InfoResponse>,
    captions: &[SavedCaption],
    language: Option<&str>,
    hook: impl FnMut(CompletionPoint) -> Result<(), String>,
) -> Result<(), String> {
    complete_files_with_hook(
        state,
        job,
        CompletionFiles {
            path,
            retained_media_path: None,
            info,
            captions,
            subtitles: &[],
            language,
        },
        hook,
    )
}

pub(crate) struct CompletionFiles<'a> {
    pub(crate) path: Option<&'a str>,
    pub(crate) retained_media_path: Option<&'a str>,
    pub(crate) info: Option<&'a InfoResponse>,
    pub(crate) captions: &'a [SavedCaption],
    pub(crate) subtitles: &'a [crate::subtitles::SavedSubtitle],
    pub(crate) language: Option<&'a str>,
}

pub(super) fn complete_files_with_hook(
    state: &crate::database::Database,
    job: &DownloadJob,
    files: CompletionFiles<'_>,
    hook: impl FnMut(CompletionPoint) -> Result<(), String>,
) -> Result<(), String> {
    let CompletionFiles {
        path,
        retained_media_path,
        info,
        captions,
        subtitles,
        language,
    } = files;
    let result = (|| -> Result<(), String> {
        let path = require_output(path)?;
        output_ready(state, job, Some(path))?;
        // File reads/hashing and external processing are outside writer locks.
        sync_output(Path::new(path))?;
        let retained_media_path = match (
            job.transcribe_text && job.download_video_with_transcript,
            retained_media_path,
        ) {
            (true, Some(media)) => {
                let media = fs::canonicalize(require_output(Some(media))?)
                    .map_err(|e| format!("Required retained video unavailable: {e}"))?;
                crate::ocr_source::validate(&media)?;
                sync_output(&media)?;
                Some(
                    media
                        .into_os_string()
                        .into_string()
                        .map_err(|_| "The retained video path is not valid Unicode")?,
                )
            }
            (true, None) => return Err("Required retained video path is missing".into()),
            (false, Some(_)) => return Err("Unexpected retained video for this job".into()),
            (false, None) => None,
        };
        let entry = prepare_history_entry(job, Some(path), info)?;
        let transcript = if job.transcribe_text {
            let language = language
                .filter(|s| !s.trim().is_empty())
                .ok_or("Required transcript language is missing")?;
            Some((
                fs::read_to_string(path)
                    .map_err(|e| format!("Required transcript read failed: {e}"))?,
                language,
            ))
        } else {
            None
        };
        for caption in captions {
            require_output(Some(&caption.media_path))?;
            if let Some(caption_path) = &caption.caption_path {
                let text = fs::read_to_string(caption_path)
                    .map_err(|e| format!("Produced caption read failed: {e}"))?;
                if text != caption.text {
                    return Err("Produced caption changed before persistence".into());
                }
            }
        }
        for subtitle in subtitles {
            if crate::subtitles::read_text(Path::new(&subtitle.path))? != subtitle.text {
                return Err("Produced subtitles changed before persistence".into());
            }
        }
        let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
        let creator = info.and_then(|info| {
            crate::history_rules::source_from_url(&job.url)
                .and_then(|source| info.creator.identity(&source))
        });
        crate::completion_store::commit(
            &conn,
            crate::completion_store::CompletionData {
                creator: creator.as_deref(),
                job,
                path,
                entry: &entry,
                captions,
                subtitles,
                transcript: transcript.as_ref().map(|(text, language)| {
                    (text.as_str(), *language, retained_media_path.as_deref())
                }),
            },
            hook,
        )
    })();
    result.map_err(|e| {
        let preserved = path.is_some_and(|p| Path::new(p).is_file());
        format!(
            "Required completion persistence failed: {e}. Output file {}.",
            if preserved {
                "preserved"
            } else {
                "unavailable"
            }
        )
    })
}

// Used by the worker and by file-backed regression tests; errors retain paths.
#[cfg(test)]
pub(super) fn apply_result(
    event: &mut crate::models::DownloadStateEvent,
    result: Result<(), String>,
) {
    event.apply_result(result);
}

pub(crate) fn prepare_history_entry(
    job: &DownloadJob,
    output_path: Option<&str>,
    info: Option<&InfoResponse>,
) -> Result<HistoryEntry, String> {
    let filename = filename_from_path(output_path);
    let metadata = hydrate_history_metadata(job, filename.as_deref(), info);
    let file_size_bytes = file_size_bytes_from_path(output_path);
    let sha256 = sha256_from_path(output_path)?;
    let now = current_timestamp_millis();
    let history_entry_id = job.id.clone();
    let entry = HistoryEntry {
        id: history_entry_id.clone(),
        url: job.url.clone(),
        title: metadata.title,
        uploader: metadata.uploader,
        filename,
        thumbnail: metadata.thumbnail,
        upload_date: metadata.upload_date,
        timestamp: metadata.timestamp,
        duration_seconds: metadata.duration_seconds,
        file_size_bytes,
        sha256,
        medium: Some(medium_for_job(job).to_string()),
        source: source_from_url(&job.url),
        platform: detect_platform(&job.url),
        output_path: output_path.map(|s| s.to_string()),
        pinefetch_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        created_at: now,
        completed_at: Some(now),
    };

    Ok(entry)
}

pub(crate) fn output_ready(
    database: &crate::database::Database,
    job: &DownloadJob,
    path: Option<&str>,
) -> Result<(), String> {
    let path = require_output(path)?;
    crate::completion_store::output_ready(database, job, path)
}
