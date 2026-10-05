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

pub(super) fn complete_with_hook(
    state: &crate::database::Database,
    job: &DownloadJob,
    path: Option<&str>,
    info: Option<&InfoResponse>,
    captions: &[SavedCaption],
    language: Option<&str>,
    hook: impl FnMut(CompletionPoint) -> Result<(), String>,
) -> Result<(), String> {
    let result = (|| -> Result<(), String> {
        let path = require_output(path)?;
        output_ready(state, job, Some(path))?;
        // File reads/hashing and external processing are outside writer locks.
        sync_output(Path::new(path))?;
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
            let text = fs::read_to_string(&caption.caption_path)
                .map_err(|e| format!("Produced caption read failed: {e}"))?;
            if text != caption.text {
                return Err("Produced caption changed before persistence".into());
            }
        }
        let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
        crate::completion_store::commit(
            &conn,
            crate::completion_store::CompletionData {
                job,
                path,
                entry: &entry,
                captions,
                transcript: transcript
                    .as_ref()
                    .map(|(text, language)| (text.as_str(), *language)),
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
