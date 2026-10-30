use crate::database;
use crate::files::current_timestamp_millis;
use crate::files::read_legacy_json;
use crate::hashing::sha256_hex;
use crate::history_rules::normalize_history_entry;
use crate::history_rules::source_from_url;
use crate::history_rules::trim_optional_string;
use crate::models::HistoryCaptionContent;
use crate::models::HistoryCaptionSummary;
use crate::models::HistoryDetails;
use crate::models::HistoryEntry;
use crate::models::HistoryPage;
use crate::models::HistorySourceCount;
use crate::models::HistoryStats;
use crate::models::HistoryTranscriptContent;
use crate::models::HistoryTranscriptSummary;
use crate::models::SavedCaption;
use rusqlite::params;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::Path;
use uuid::Uuid;

pub(super) fn millis_to_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

pub(super) fn i64_to_millis(value: i64) -> u64 {
    if value < 0 {
        0
    } else {
        value as u64
    }
}

pub(super) fn optional_i64_to_millis(value: Option<i64>) -> Option<u64> {
    value.map(i64_to_millis)
}

#[cfg(test)]
pub(super) fn count_history_entries_in_db(
    state: &crate::database::Database,
) -> Result<u64, String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM history_entries", [], |row| row.get(0))
        .map_err(|e| format!("History read failed: {e}"))?;
    Ok(count.max(0) as u64)
}

pub(super) fn list_history_page_from_db(
    state: &crate::database::Database,
    limit: u32,
    offset: u32,
) -> Result<HistoryPage, String> {
    search_history_page_from_db(state, limit, offset, None, None, None, false)
}

pub(super) fn search_history_page_from_db(
    state: &crate::database::Database,
    limit: u32,
    offset: u32,
    query: Option<&str>,
    search_field: Option<&str>,
    source: Option<&str>,
    exact_user: bool,
) -> Result<HistoryPage, String> {
    crate::history_query::read_page(
        state,
        limit,
        offset,
        crate::history_query::HistoryFilter::new(query, search_field, source, exact_user)?,
        None,
    )
}

pub(super) fn history_search_key(query: Option<&str>) -> Option<String> {
    let query = query
        .map(crate::unicode_search::trim_query)
        .filter(|query| !query.is_empty())?;
    Some(crate::unicode_search::key(query))
}

fn path_extension(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(str::to_ascii_lowercase)
}

pub(crate) fn local_history_media_path(
    state: &crate::database::Database,
    id: &str,
) -> Result<std::path::PathBuf, String> {
    let path = crate::ocr_store::saved_path(state, id)?;
    let path = std::fs::canonicalize(path).map_err(|_| "The saved video is unavailable")?;
    crate::ocr_source::validate(&path)?;
    Ok(path)
}

pub(super) fn get_history_details_from_db(
    state: &crate::database::Database,
    id: &str,
) -> Result<Option<HistoryDetails>, String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let entry = conn
        .query_row(
            "SELECT id, url, title, uploader, filename, thumbnail, upload_date, timestamp, duration_seconds, file_size_bytes, sha256, medium, source, platform, output_path, pinefetch_version, created_at, completed_at
             FROM history_entries WHERE id = ?1",
            params![id],
            map_history_entry,
        )
        .optional()
        .map_err(|e| format!("History details read failed: {e}"))?;
    let Some(entry) = entry else {
        return Ok(None);
    };

    let output_file_available = entry
        .output_path
        .as_deref()
        .is_some_and(|path| Path::new(path).is_file());
    let file_extension = entry
        .output_path
        .as_deref()
        .or(entry.filename.as_deref())
        .and_then(path_extension);
    let transcript = conn
        .query_row(
            "SELECT \"type\", language FROM transcriptions WHERE history_entry_id = ?1",
            params![id],
            |row| {
                Ok(HistoryTranscriptSummary {
                    transcription_type: row.get(0)?,
                    language: trim_optional_string(row.get(1)?),
                    file_available: output_file_available,
                })
            },
        )
        .optional()
        .map_err(|e| format!("History details read failed: {e}"))?;

    let mut statement = conn
        .prepare(
            "SELECT media_path, caption_path, sha256, created_at
             FROM captions WHERE history_entry_id = ?1 ORDER BY created_at, media_path",
        )
        .map_err(|e| format!("History details read failed: {e}"))?;
    let rows = statement
        .query_map(params![id], |row| {
            let caption_path: Option<String> = row.get(1)?;
            let created_at: i64 = row.get(3)?;
            Ok(HistoryCaptionSummary {
                media_path: row.get(0)?,
                format: caption_path.as_deref().and_then(path_extension),
                file_available: caption_path
                    .as_deref()
                    .is_some_and(|path| Path::new(path).is_file()),
                caption_path,
                sha256: row.get(2)?,
                created_at: i64_to_millis(created_at),
            })
        })
        .map_err(|e| format!("History details read failed: {e}"))?;
    let captions = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| format!("History details read failed: {e}"))?;

    Ok(Some(HistoryDetails {
        entry,
        output_file_available,
        file_extension,
        transcript,
        captions,
        subtitles: crate::subtitles::summaries(&conn, id)?,
    }))
}

pub(super) fn get_history_transcript_from_db(
    state: &crate::database::Database,
    id: &str,
) -> Result<Option<HistoryTranscriptContent>, String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    conn.query_row(
        "SELECT transcription.text, transcription.\"type\", transcription.language, history.output_path
         FROM transcriptions AS transcription
         JOIN history_entries AS history ON history.id = transcription.history_entry_id
         WHERE transcription.history_entry_id = ?1",
        params![id],
        |row| {
            let output_path: Option<String> = row.get(3)?;
            Ok(HistoryTranscriptContent {
                text: row.get(0)?,
                transcription_type: row.get(1)?,
                language: trim_optional_string(row.get(2)?),
                file_available: output_path
                    .as_deref()
                    .is_some_and(|path| Path::new(path).is_file()),
            })
        },
    )
    .optional()
    .map_err(|e| format!("Transcript read failed: {e}"))
}

pub(super) fn get_history_caption_from_db(
    state: &crate::database::Database,
    id: &str,
    media_path: &str,
) -> Result<Option<HistoryCaptionContent>, String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    conn.query_row(
        "SELECT media_path, caption_path, text, sha256, created_at
         FROM captions WHERE history_entry_id = ?1 AND media_path = ?2",
        params![id, media_path],
        |row| {
            let caption_path: Option<String> = row.get(1)?;
            let created_at: i64 = row.get(4)?;
            Ok(HistoryCaptionContent {
                media_path: row.get(0)?,
                format: caption_path.as_deref().and_then(path_extension),
                file_available: caption_path
                    .as_deref()
                    .is_some_and(|path| Path::new(path).is_file()),
                caption_path,
                text: row.get(2)?,
                sha256: row.get(3)?,
                created_at: i64_to_millis(created_at),
            })
        },
    )
    .optional()
    .map_err(|e| format!("Caption read failed: {e}"))
}

#[cfg(test)]
pub(super) fn list_history_entries_from_db(
    state: &crate::database::Database,
) -> Result<Vec<HistoryEntry>, String> {
    Ok(list_history_page_from_db(state, u32::MAX, 0)?.entries)
}

struct StatsSnapshot {
    totals: HistoryStats,
    groups: Vec<(Option<String>, u64)>,
    legacy: Vec<(String, Option<String>)>,
}

fn read_stats(conn: &Connection) -> Result<StatsSnapshot, String> {
    let totals = conn.query_row("SELECT COUNT(*),COALESCE(SUM(duration_seconds),0),COALESCE(SUM(file_size_bytes),0) FROM history_entries", [], |row| {
        Ok(HistoryStats {
            video_count: row.get::<_, i64>(0)?.max(0) as u64,
            total_duration_seconds: row.get::<_, i64>(1)?.max(0) as u64,
            total_file_size_bytes: row.get::<_, i64>(2)?.max(0) as u64,
            source_counts: Vec::new(),
        })
    }).map_err(|e| format!("History stats read failed: {e}"))?;
    let groups = {
        let mut stmt = conn
            .prepare("SELECT source,COUNT(*) FROM history_entries GROUP BY source COLLATE NOCASE")
            .map_err(|e| format!("History stats read failed: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, i64>(1)?.max(0) as u64,
                ))
            })
            .map_err(|e| format!("History stats read failed: {e}"))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| format!("History stats read failed: {e}"))?
    };
    // Copy only the legacy blank-source projection. Modern archives retain the
    // covering source-index path, without loading their URLs or other metadata.
    let blank_sources: Vec<_> = groups
        .iter()
        .filter(|(source, _)| {
            source
                .as_deref()
                .is_none_or(|source| source.trim().is_empty())
        })
        .map(|(source, _)| source.clone())
        .collect();
    let mut legacy = Vec::new();
    for sources in blank_sources.chunks(128) {
        let predicate = (1..=sources.len())
            .map(|n| format!("source IS ?{n} COLLATE NOCASE"))
            .collect::<Vec<_>>()
            .join(" OR ");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT url,platform FROM history_entries WHERE {predicate}"
            ))
            .map_err(|e| format!("History stats read failed: {e}"))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(sources), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|e| format!("History stats read failed: {e}"))?;
        for row in rows {
            legacy.push(row.map_err(|e| format!("History stats read failed: {e}"))?);
        }
    }
    Ok(StatsSnapshot {
        totals,
        groups,
        legacy,
    })
}

impl StatsSnapshot {
    fn finish(mut self) -> HistoryStats {
        #[cfg(test)]
        crate::db_contention_tests::analysis_started();
        let mut counts = BTreeMap::<String, u64>::new();
        for (source, count) in self.groups {
            if let Some(source) = trim_optional_string(source) {
                *counts.entry(source.to_ascii_lowercase()).or_default() += count;
            }
        }
        for (url, platform) in self.legacy {
            let source = source_from_url(&url)
                .or_else(|| trim_optional_string(platform).map(|value| value.to_ascii_lowercase()))
                .unwrap_or_else(|| "unknown".into());
            *counts.entry(source).or_default() += 1;
        }
        self.totals.source_counts = counts
            .into_iter()
            .map(|(source, count)| HistorySourceCount { source, count })
            .collect();
        self.totals
            .source_counts
            .sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.source.cmp(&b.source)));
        self.totals
    }
}

pub(super) fn get_history_stats_from_db(
    state: &crate::database::Database,
) -> Result<HistoryStats, String> {
    state.analyze(|| Ok(state.read_snapshot(read_stats)?.finish()))
}

#[cfg(test)]
pub(super) fn insert_history_entry_in_db(
    state: &crate::database::Database,
    entry: &HistoryEntry,
) -> Result<(), String> {
    let entry = normalize_history_entry(entry.clone());
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    insert_history_entry_in_conn(&conn, &entry)
}

pub(super) fn insert_history_entry_in_conn(
    conn: &Connection,
    entry: &HistoryEntry,
) -> Result<(), String> {
    let legacy_version = if entry.pinefetch_version.is_none() {
        database::legacy_history_version_default(conn)
            .map_err(|e| format!("History version default read failed: {e}"))?
    } else {
        None
    };
    conn.execute(
        "INSERT INTO history_entries (
            id,
            url,
            title,
            uploader,
            filename,
            thumbnail,
            upload_date,
            timestamp,
            duration_seconds,
            file_size_bytes,
            sha256,
            medium,
            source,
            platform,
            output_path,
            pinefetch_version,
            created_at,
            completed_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
        ON CONFLICT(id) DO UPDATE SET
            url = excluded.url,
            title = excluded.title,
            uploader = excluded.uploader,
            filename = excluded.filename,
            thumbnail = excluded.thumbnail,
            upload_date = excluded.upload_date,
            timestamp = excluded.timestamp,
            duration_seconds = excluded.duration_seconds,
            file_size_bytes = excluded.file_size_bytes,
            sha256 = excluded.sha256,
            medium = excluded.medium,
            source = excluded.source,
            platform = excluded.platform,
            output_path = excluded.output_path,
            pinefetch_version = excluded.pinefetch_version,
            created_at = excluded.created_at,
            completed_at = excluded.completed_at",
        params![
            entry.id,
            entry.url,
            entry.title,
            entry.uploader,
            entry.filename,
            entry.thumbnail,
            entry.upload_date,
            entry.timestamp,
            entry.duration_seconds,
            entry.file_size_bytes,
            entry.sha256,
            entry.medium,
            entry.source,
            entry.platform,
            entry.output_path,
            entry.pinefetch_version.as_deref().or(legacy_version),
            millis_to_i64(entry.created_at),
            entry.completed_at.map(millis_to_i64),
        ],
    )
    .map_err(|e| format!("History insert failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
pub(super) fn insert_captions_in_db(
    state: &crate::database::Database,
    history_entry_id: &str,
    captions: &[SavedCaption],
) -> Result<(), String> {
    if captions.is_empty() {
        return Ok(());
    }

    let mut conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let transaction = conn
        .transaction()
        .map_err(|e| format!("Caption transaction failed: {e}"))?;
    insert_captions_in_conn(&transaction, history_entry_id, captions)?;
    transaction
        .commit()
        .map_err(|e| format!("Caption transaction failed: {e}"))?;
    Ok(())
}

pub(super) fn insert_captions_in_conn(
    conn: &Connection,
    history_entry_id: &str,
    captions: &[SavedCaption],
) -> Result<(), String> {
    {
        let mut statement = conn
            .prepare(
                "INSERT INTO captions (
                    history_entry_id, media_path, caption_path, text, sha256, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(history_entry_id, media_path) DO UPDATE SET
                    caption_path = excluded.caption_path,
                    text = excluded.text,
                    sha256 = excluded.sha256",
            )
            .map_err(|e| format!("Caption insert failed: {e}"))?;
        for caption in captions {
            statement
                .execute(params![
                    history_entry_id,
                    caption.media_path,
                    caption.caption_path,
                    caption.text,
                    sha256_hex(caption.text.as_bytes()),
                    millis_to_i64(current_timestamp_millis()),
                ])
                .map_err(|e| format!("Caption insert failed: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn insert_transcription_in_db(
    state: &crate::database::Database,
    history_entry_id: &str,
    text: &str,
    transcription_type: &str,
    language: &str,
) -> Result<(), String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    insert_transcription_in_conn(
        &conn,
        history_entry_id,
        text,
        transcription_type,
        language,
        None,
    )
}

pub(super) fn insert_transcription_in_conn(
    conn: &Connection,
    history_entry_id: &str,
    text: &str,
    transcription_type: &str,
    language: &str,
    media_path: Option<&str>,
) -> Result<(), String> {
    let language = language.trim().to_ascii_lowercase();
    if language.is_empty() {
        return Err("Transcription language is required".to_string());
    }
    conn.execute(
        "INSERT INTO transcriptions (id, history_entry_id, text, \"type\", language, media_path, media_path_known)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
        params![
            Uuid::new_v4().to_string(),
            history_entry_id,
            text,
            transcription_type,
            language,
            media_path,
        ],
    )
    .map_err(|e| format!("Transcription insert failed: {e}"))?;
    Ok(())
}

pub(super) fn delete_history_entry_from_db(
    state: &crate::database::Database,
    id: &str,
) -> Result<(), String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    struct AccountKeys {
        source: Option<String>,
        uploader: Option<String>,
        creator_source: Option<String>,
        creator_id: Option<String>,
        url: String,
    }
    let target: Option<AccountKeys> = tx
        .query_row(
            "SELECT source,uploader,creator_source,creator_id,url FROM history_entries WHERE id=?1",
            [id],
            |r| {
                Ok(AccountKeys {
                    source: r.get(0)?,
                    uploader: r.get(1)?,
                    creator_source: r.get(2)?,
                    creator_id: r.get(3)?,
                    url: r.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM history_entries WHERE id = ?1", params![id])
        .map_err(|e| format!("History delete failed: {e}"))?;
    if let Some(AccountKeys {
        source,
        uploader,
        creator_source,
        creator_id,
        url,
    }) = target
    {
        // Check only the deleted entry's keys through matching membership indexes.
        if let (Some(source), Some(identity)) = (creator_source, creator_id) {
            tx.execute("DELETE FROM creator_account_insights WHERE source=?1 AND creator_id=?2 AND NOT EXISTS (SELECT 1 FROM history_entries WHERE creator_source=?1 AND creator_id=?2)",params![source,identity]).map_err(|e| format!("Account cleanup failed: {e}"))?;
        }
        if let Some(platform) = source
            .as_deref()
            .filter(|source| !source.trim().is_empty())
            .map(crate::creator_identity::canonical_source)
            .or_else(|| crate::history_rules::source_from_url(&url))
        {
            tx.execute(
                "DELETE FROM creator_account_insights WHERE source=?1 AND creator_id=?2",
                params![platform, format!("entry:{id}")],
            )
            .map_err(|e| e.to_string())?;
        }
        if let (Some(source), Some(uploader)) = (source, uploader) {
            let source = source.trim().to_ascii_lowercase();
            tx.execute("DELETE FROM history_account_insights WHERE source=?1 AND uploader=?2 COLLATE BINARY AND NOT EXISTS (SELECT 1 FROM history_entries WHERE source=?1 COLLATE NOCASE AND uploader=?2 COLLATE BINARY) AND NOT EXISTS (SELECT 1 FROM history_entries WHERE source IS NULL AND uploader=?2 COLLATE BINARY)",params![source,uploader]).map_err(|e|format!("Account cleanup failed: {e}"))?;
        }
    }
    tx.commit()
        .map_err(|e| format!("History deletion commit failed: {e}"))
}

pub(super) fn clear_history_entries_in_db(state: &crate::database::Database) -> Result<(), String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    tx.execute("DELETE FROM history_entries", [])
        .map_err(|e| format!("History clear failed: {e}"))?;
    tx.execute("DELETE FROM history_account_insights", [])
        .map_err(|e| format!("Account cleanup failed: {e}"))?;
    tx.execute("DELETE FROM creator_account_insights", [])
        .map_err(|e| format!("Account cleanup failed: {e}"))?;
    tx.commit()
        .map_err(|e| format!("History deletion commit failed: {e}"))
}

pub(super) fn import_legacy_history(conn: &Connection, path: &Path) -> Result<(), String> {
    import_legacy_history_with_hook(conn, path, || Ok(()))
}

pub(super) fn import_legacy_history_with_hook(
    conn: &Connection,
    path: &Path,
    mut after_entry: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let imported: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM legacy_imports WHERE name='history_json')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| format!("History import marker read failed: {e}"))?;
    if imported {
        return Ok(());
    }
    let entries = read_legacy_json::<Vec<HistoryEntry>>(path)?;
    let tx = database::write_transaction(conn)
        .map_err(|e| format!("History import transaction failed: {e}"))?;
    let imported: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM legacy_imports WHERE name='history_json')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if imported {
        return Ok(());
    }
    let mut ids = HashSet::new();
    for entry in entries.into_iter().flatten() {
        if entry.id.trim().is_empty() || !ids.insert(entry.id.clone()) {
            return Err(
                "Legacy history has missing/duplicate IDs; import rolled back, original preserved"
                    .into(),
            );
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT url FROM history_entries WHERE id=?1",
                [&entry.id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        match existing {
            Some(url) if url != entry.url => return Err("Legacy history ID conflicts with an existing entry; import rolled back, original preserved".into()),
            Some(_) => {}, // stable imported ID; retain possibly newer DB metadata
            None => {
                let entry = normalize_history_entry(entry);
                insert_history_entry_in_conn(&tx, &entry)?;
            },
        }
        after_entry()?;
    }
    tx.execute(
        "INSERT INTO legacy_imports(name, completed_at) VALUES ('history_json', ?1)",
        [millis_to_i64(current_timestamp_millis())],
    )
    .map_err(|e| format!("History import marker failed: {e}"))?;
    tx.commit()
        .map_err(|e| format!("History import commit failed: {e}"))
}

pub(super) fn map_history_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let created_at: i64 = row.get(16)?;
    let completed_at: Option<i64> = row.get(17)?;
    Ok(normalize_history_entry(HistoryEntry {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        uploader: row.get(3)?,
        filename: row.get(4)?,
        thumbnail: row.get(5)?,
        upload_date: row.get(6)?,
        timestamp: row.get(7)?,
        duration_seconds: row.get(8)?,
        file_size_bytes: row.get(9)?,
        sha256: row.get(10)?,
        medium: row.get(11)?,
        source: row.get(12)?,
        platform: row.get(13)?,
        output_path: row.get(14)?,
        pinefetch_version: row.get(15)?,
        created_at: i64_to_millis(created_at),
        completed_at: optional_i64_to_millis(completed_at),
    }))
}
