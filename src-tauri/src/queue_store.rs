//! One atomic queue snapshot; completion receipts decide recovery, never filenames.
use crate::database::{self, Database};
use crate::models::DownloadJob;
use crate::queue::{QueueData, RecoveredDownload};
use rusqlite::OptionalExtension;
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::Arc;

pub(crate) const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
pub(crate) struct QueueStore {
    db: Arc<Database>,
    // OS-owned locks disappear after a crash; the empty lock file may remain.
    _profile_lock: Option<File>,
}

impl QueueStore {
    pub(crate) fn open(db: Arc<Database>) -> Result<Self, String> {
        let path = db
            .lock()
            .map_err(|_| "SQLite lock poisoned")?
            .path()
            .filter(|path| !path.is_empty())
            .map(str::to_owned);
        let profile_lock = if let Some(path) = path {
            let mut options = OpenOptions::new();
            options.read(true).write(true).create(true).truncate(false);
            #[cfg(unix)]
            options.mode(0o600);
            let file = options
                .open(Path::new(&path).with_extension("queue.lock"))
                .map_err(|error| format!("Queue profile lock unavailable: {error}"))?;
            file.try_lock().map_err(|error| format!(
                "This queue profile is already open or cannot be locked: {error}. Close the other PineFetch instance before reopening it."
            ))?;
            Some(file)
        } else {
            None
        };
        Ok(Self {
            db,
            _profile_lock: profile_lock,
        })
    }

    pub(crate) fn load(&self) -> Result<QueueData, String> {
        let conn = self.db.lock().map_err(|_| "SQLite lock poisoned")?;
        let (revision, length): (i64, i64) = conn
            .query_row(
                "SELECT revision,length(CAST(snapshot_json AS BLOB)) FROM queue_state WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| format!("Queue snapshot read failed: {error}"))?;
        if !(0..=MAX_SNAPSHOT_BYTES as i64).contains(&length) {
            return Err("Saved queue exceeds its supported size; data preserved".into());
        }
        let json: String = conn
            .query_row(
                "SELECT snapshot_json FROM queue_state WHERE id=1",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("Queue snapshot read failed: {error}"))?;
        let mut data: QueueData = serde_json::from_str(&json)
            .map_err(|error| format!("Saved queue is invalid; data preserved: {error}"))?;
        data.revision = revision;
        validate(&data)?;
        let mut changed = false;
        let mut pending = std::collections::VecDeque::new();
        let mut interrupted = Vec::new();
        for job in data.pending.drain(..) {
            match receipt(&conn, &job)? {
                Some((state, _)) if state == "complete" => changed = true,
                Some((_, path)) => {
                    interrupted.push(recovered(job, path));
                    changed = true;
                }
                None => pending.push_back(job),
            }
        }
        data.pending = pending;
        if let Some(job) = data.active.take() {
            let status = receipt(&conn, &job)?;
            if status.as_ref().is_none_or(|(state, _)| state != "complete") {
                interrupted.push(recovered(job, status.and_then(|(_, path)| path)));
            }
            changed = true;
        }
        for mut entry in data.interrupted.drain(..) {
            if let Some((state, path)) = receipt(&conn, &entry.job)? {
                if state == "complete" {
                    changed = true;
                    continue;
                }
                if path.is_some() && entry.output_path != path {
                    entry.output_path = path;
                    changed = true;
                }
            }
            interrupted.push(entry);
        }
        data.interrupted = interrupted;
        drop(conn);
        if changed {
            self.save(&mut data)?;
        }
        Ok(data)
    }

    pub(crate) fn save(&self, data: &mut QueueData) -> Result<(), String> {
        validate(data)?;
        let next_revision = data
            .revision
            .checked_add(1)
            .ok_or("Queue revision exhausted")?;
        let json = serde_json::to_string(data)
            .map_err(|error| format!("Queue serialization failed: {error}"))?;
        if json.len() > MAX_SNAPSHOT_BYTES {
            return Err("Queue is too large to save; existing queue preserved".into());
        }
        let conn = self.db.lock().map_err(|_| "SQLite lock poisoned")?;
        let tx = database::write_transaction(&conn)?;
        let changed = tx
            .execute(
                "UPDATE queue_state SET snapshot_json=?1,revision=?2 WHERE id=1 AND revision=?3",
                rusqlite::params![json, next_revision, data.revision],
            )
            .map_err(|error| format!("Queue save failed; existing queue preserved: {error}"))?;
        if changed != 1 {
            return Err("Queue changed outside this session; restart PineFetch to reload it. Saved jobs preserved.".into());
        }
        tx.commit()
            .map_err(|error| format!("Queue save commit failed: {error}"))?;
        data.revision = next_revision;
        Ok(())
    }
}

fn receipt(
    conn: &rusqlite::Connection,
    job: &DownloadJob,
) -> Result<Option<(String, Option<String>)>, String> {
    let row: Option<(String, String, Option<String>)> = conn
        .query_row(
            "SELECT url,state,output_path FROM job_completions WHERE job_id=?1",
            [&job.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| format!("Queue recovery receipt read failed: {error}"))?;
    match row {
        Some((url, state, path)) if url == job.url => Ok(Some((state, path))),
        Some(_) => {
            Err("Saved queue job conflicts with its completion identity; data preserved".into())
        }
        None => Ok(None),
    }
}

pub(crate) fn recovered(job: DownloadJob, output_path: Option<String>) -> RecoveredDownload {
    RecoveredDownload {
        job, output_path,
        error: "Interrupted when PineFetch closed. Existing output files are preserved. Check them before using Queue again.".into(),
    }
}

fn validate(data: &QueueData) -> Result<(), String> {
    if data.format_version != 1 || data.revision < 0 {
        return Err("Unsupported saved queue format; data preserved".into());
    }
    let mut identities = HashSet::new();
    for job in data
        .pending
        .iter()
        .chain(data.active.iter())
        .chain(data.interrupted.iter().map(|entry| &entry.job))
    {
        if job.id.trim().is_empty() || !identities.insert(&job.id) {
            return Err(
                "Saved queue has missing or duplicate job identities; data preserved".into(),
            );
        }
        crate::url_rules::validate_download_url(&job.url)
            .map_err(|_| "Saved queue contains an invalid URL; data preserved")?;
        if job.output_dir.trim().is_empty() {
            return Err("Saved queue contains an empty destination; data preserved".into());
        }
    }
    Ok(())
}
