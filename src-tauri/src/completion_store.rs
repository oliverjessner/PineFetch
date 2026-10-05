use crate::database;
use crate::files::current_timestamp_millis;
use crate::history::insert_captions_in_conn;
use crate::history::insert_history_entry_in_conn;
use crate::history::insert_transcription_in_conn;
use crate::history::millis_to_i64;
use crate::models::DownloadJob;
use crate::models::HistoryEntry;
use crate::models::SavedCaption;
use rusqlite::params;
use rusqlite::OptionalExtension;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CompletionPoint {
    History,
    Transcript,
    Captions,
    BeforeCommit,
}

pub(super) fn begin(state: &crate::database::Database, job: &DownloadJob) -> Result<(), String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn).map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO job_completions(job_id,url,state,updated_at) VALUES (?1,?2,'processing',?3) ON CONFLICT(job_id) DO NOTHING",
        params![job.id, job.url, millis_to_i64(current_timestamp_millis())]).map_err(|e| format!("Processing receipt failed: {e}"))?;
    let (url, status): (String, String) = tx
        .query_row(
            "SELECT url,state FROM job_completions WHERE job_id=?1",
            [&job.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    if url != job.url || status != "processing" {
        return Err("Job identity already has a different/completed receipt".into());
    }
    tx.commit()
        .map_err(|e| format!("Processing receipt commit failed: {e}"))
}

pub(super) fn known_output(
    state: &crate::database::Database,
    job_id: &str,
) -> Result<Option<String>, String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    conn.query_row(
        "SELECT output_path FROM job_completions WHERE job_id=?1",
        [job_id],
        |r| r.get(0),
    )
    .optional()
    .map(Option::flatten)
    .map_err(|e| format!("Output receipt read failed: {e}"))
}

pub(super) fn output_ready(
    state: &crate::database::Database,
    job: &DownloadJob,
    path: &str,
) -> Result<(), String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    let updated = tx.execute("UPDATE job_completions SET output_path=?1, state='output_ready', updated_at=?2 WHERE job_id=?3 AND url=?4 AND state IN ('processing','output_ready')",
        params![path, millis_to_i64(current_timestamp_millis()), job.id, job.url])
        .map_err(|e| format!("Output receipt failed: {e}"))?;
    if updated == 0 {
        let existing: Option<(String, String, String)> = tx
            .query_row(
                "SELECT url,output_path,state FROM job_completions WHERE job_id=?1",
                [&job.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if existing != Some((job.url.clone(), path.to_string(), "complete".into())) {
            return Err("Output receipt does not match the processing job".into());
        }
    }
    tx.commit()
        .map_err(|e| format!("Output receipt commit failed: {e}"))
}

pub(crate) struct CompletionData<'a> {
    pub(crate) job: &'a DownloadJob,
    pub(crate) path: &'a str,
    pub(crate) entry: &'a HistoryEntry,
    pub(crate) captions: &'a [SavedCaption],
    pub(crate) transcript: Option<(&'a str, &'a str)>,
}
pub(crate) fn commit(
    conn: &rusqlite::Connection,
    data: CompletionData<'_>,
    mut hook: impl FnMut(CompletionPoint) -> Result<(), String>,
) -> Result<(), String> {
    let CompletionData {
        job,
        path,
        entry,
        captions,
        ..
    } = data;
    let tx = database::write_transaction(conn)
        .map_err(|e| format!("Completion writer lock failed: {e}"))?;
    let (url, stored_path, status, history_id): (String, Option<String>, String, Option<String>) =
        tx.query_row(
            "SELECT url,output_path,state,history_entry_id FROM job_completions WHERE job_id=?1",
            [&job.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| format!("Completion receipt read failed: {e}"))?;
    if url != job.url || stored_path.as_deref() != Some(path) {
        return Err("Completion job identity/path conflict".into());
    }
    if status == "complete" {
        if history_id.as_deref() != Some(&job.id) {
            return Err(
                "Completed history was removed; receipt retained, no implicit recreation".into(),
            );
        }
        return Ok(());
    }
    let collision: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM history_entries WHERE id=?1)",
            [&job.id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if collision {
        return Err(
            "Completion ID conflicts with existing history; existing entry preserved".into(),
        );
    }
    insert_history_entry_in_conn(&tx, entry)?;
    hook(CompletionPoint::History)?;
    if let Some((text, language)) = data.transcript {
        insert_transcription_in_conn(
            &tx,
            &job.id,
            text,
            if job.transcribe_timestamps {
                "text with timestamps"
            } else {
                "text"
            },
            language,
        )?;
    }
    hook(CompletionPoint::Transcript)?;
    insert_captions_in_conn(&tx, &job.id, captions)?;
    hook(CompletionPoint::Captions)?;
    tx.execute("UPDATE job_completions SET state='complete',history_entry_id=?1,updated_at=?2 WHERE job_id=?1",
        params![job.id, millis_to_i64(current_timestamp_millis())]).map_err(|e| format!("Completion marker failed: {e}"))?;
    hook(CompletionPoint::BeforeCommit)?;
    tx.commit()
        .map_err(|e| format!("Completion commit failed: {e}"))
}
