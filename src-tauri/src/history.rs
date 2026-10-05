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
    search_history_page_from_db(state, limit, offset, None, None, None)
}

pub(super) fn search_history_page_from_db(
    state: &crate::database::Database,
    limit: u32,
    offset: u32,
    query: Option<&str>,
    search_field: Option<&str>,
    source: Option<&str>,
) -> Result<HistoryPage, String> {
    let pattern = history_search_pattern(query);
    let search_field = match search_field
        .map(str::trim)
        .filter(|field| !field.is_empty())
    {
        None | Some("title") => "title",
        Some("description") => "description",
        Some("user") => "user",
        Some(field) => return Err(format!("Unsupported history search field: {field}")),
    };
    let source = source
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .map(str::to_ascii_lowercase);
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM history_entries AS history
             WHERE (?1 IS NULL
                OR (?2 = 'title' AND history.title LIKE ?1 ESCAPE '\\' COLLATE NOCASE)
                OR (?2 = 'user' AND history.uploader LIKE ?1 ESCAPE '\\' COLLATE NOCASE)
                OR (?2 = 'description' AND EXISTS (
                    SELECT 1 FROM captions
                    WHERE captions.history_entry_id = history.id
                      AND captions.text LIKE ?1 ESCAPE '\\' COLLATE NOCASE
                )))
               AND (?3 IS NULL OR history.source = ?3 COLLATE NOCASE)",
            params![pattern, search_field, source],
            |row| row.get(0),
        )
        .map_err(|e| format!("History read failed: {e}"))?;
    let mut stmt = conn
        .prepare(
            "SELECT id, url, title, uploader, filename, thumbnail, upload_date, timestamp, duration_seconds, file_size_bytes, sha256, medium, source, platform, output_path, pinefetch_version, created_at, completed_at
             FROM history_entries AS history
             WHERE (?1 IS NULL
                OR (?2 = 'title' AND history.title LIKE ?1 ESCAPE '\\' COLLATE NOCASE)
                OR (?2 = 'user' AND history.uploader LIKE ?1 ESCAPE '\\' COLLATE NOCASE)
                OR (?2 = 'description' AND EXISTS (
                    SELECT 1 FROM captions
                    WHERE captions.history_entry_id = history.id
                      AND captions.text LIKE ?1 ESCAPE '\\' COLLATE NOCASE
                )))
               AND (?3 IS NULL OR history.source = ?3 COLLATE NOCASE)
             ORDER BY COALESCE(completed_at, created_at) DESC, created_at DESC, id DESC
             LIMIT ?4 OFFSET ?5",
        )
        .map_err(|e| format!("History read failed: {e}"))?;

    let rows = stmt
        .query_map(
            params![
                pattern,
                search_field,
                source,
                i64::from(limit),
                i64::from(offset)
            ],
            |row| {
                let created_at: i64 = row.get(16)?;
                let completed_at: Option<i64> = row.get(17)?;
                Ok(HistoryEntry {
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
                })
            },
        )
        .map_err(|e| format!("History read failed: {e}"))?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(normalize_history_entry(
            row.map_err(|e| format!("History read failed: {e}"))?,
        ));
    }

    let loaded_count = u64::from(offset).saturating_add(entries.len() as u64);
    Ok(HistoryPage {
        entries,
        has_more: loaded_count < total.max(0) as u64,
    })
}

pub(super) fn history_search_pattern(query: Option<&str>) -> Option<String> {
    let query = query.map(str::trim).filter(|query| !query.is_empty())?;
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    Some(format!("%{escaped}%"))
}

fn path_extension(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(str::to_ascii_lowercase)
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
            |row| {
                let created_at: i64 = row.get(16)?;
                let completed_at: Option<i64> = row.get(17)?;
                Ok(HistoryEntry {
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
                })
            },
        )
        .optional()
        .map_err(|e| format!("History details read failed: {e}"))?;
    let Some(entry) = entry.map(normalize_history_entry) else {
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
            let caption_path: String = row.get(1)?;
            let created_at: i64 = row.get(3)?;
            Ok(HistoryCaptionSummary {
                media_path: row.get(0)?,
                format: path_extension(&caption_path),
                file_available: Path::new(&caption_path).is_file(),
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
            let caption_path: String = row.get(1)?;
            let created_at: i64 = row.get(4)?;
            Ok(HistoryCaptionContent {
                media_path: row.get(0)?,
                format: path_extension(&caption_path),
                file_available: Path::new(&caption_path).is_file(),
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

pub(super) fn get_history_stats_from_db(
    state: &crate::database::Database,
) -> Result<HistoryStats, String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let mut stats = conn
        .query_row(
            "SELECT
            COUNT(*),
            COALESCE(SUM(duration_seconds), 0),
            COALESCE(SUM(file_size_bytes), 0)
         FROM history_entries",
            [],
            |row| {
                let video_count: i64 = row.get(0)?;
                let total_duration_seconds: i64 = row.get(1)?;
                let total_file_size_bytes: i64 = row.get(2)?;
                Ok(HistoryStats {
                    video_count: video_count.max(0) as u64,
                    total_duration_seconds: total_duration_seconds.max(0) as u64,
                    total_file_size_bytes: total_file_size_bytes.max(0) as u64,
                    source_counts: Vec::new(),
                })
            },
        )
        .map_err(|e| format!("History stats read failed: {e}"))?;

    let mut stmt = conn
        .prepare("SELECT source, url, platform FROM history_entries")
        .map_err(|e| format!("History stats read failed: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| format!("History stats read failed: {e}"))?;
    let mut counts = BTreeMap::<String, u64>::new();
    for row in rows {
        let (source, url, platform) = row.map_err(|e| format!("History stats read failed: {e}"))?;
        let source = trim_optional_string(source)
            .map(|value| value.to_ascii_lowercase())
            .or_else(|| source_from_url(&url))
            .or_else(|| trim_optional_string(platform).map(|value| value.to_ascii_lowercase()))
            .unwrap_or_else(|| "unknown".to_string());
        *counts.entry(source).or_default() += 1;
    }
    stats.source_counts = counts
        .into_iter()
        .map(|(source, count)| HistorySourceCount { source, count })
        .collect();
    stats
        .source_counts
        .sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.source.cmp(&b.source)));
    Ok(stats)
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
            entry.pinefetch_version,
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
    insert_transcription_in_conn(&conn, history_entry_id, text, transcription_type, language)
}

pub(super) fn insert_transcription_in_conn(
    conn: &Connection,
    history_entry_id: &str,
    text: &str,
    transcription_type: &str,
    language: &str,
) -> Result<(), String> {
    let language = language.trim().to_ascii_lowercase();
    if language.is_empty() {
        return Err("Transcription language is required".to_string());
    }
    conn.execute(
        "INSERT INTO transcriptions (id, history_entry_id, text, \"type\", language)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            Uuid::new_v4().to_string(),
            history_entry_id,
            text,
            transcription_type,
            language,
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
    tx.execute("DELETE FROM history_entries WHERE id = ?1", params![id])
        .map_err(|e| format!("History delete failed: {e}"))?;
    tx.commit()
        .map_err(|e| format!("History deletion commit failed: {e}"))
}

pub(super) fn clear_history_entries_in_db(state: &crate::database::Database) -> Result<(), String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    tx.execute("DELETE FROM history_entries", [])
        .map_err(|e| format!("History clear failed: {e}"))?;
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
