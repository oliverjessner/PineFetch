use super::*;

pub(super) fn legacy_history_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "Data directory unavailable")?;
    fs::create_dir_all(&dir).map_err(|e| format!("Data dir create failed: {e}"))?;
    Ok(dir.join("history.json"))
}

pub(super) fn load_legacy_history_json(app: &AppHandle) -> Vec<HistoryEntry> {
    if let Ok(path) = legacy_history_path(app) {
        if let Ok(raw) = fs::read_to_string(path) {
            if let Ok(history) = serde_json::from_str::<Vec<HistoryEntry>>(&raw) {
                return history.into_iter().map(normalize_history_entry).collect();
            }
        }
    }
    Vec::new()
}

pub(super) fn normalize_history_entry(mut entry: HistoryEntry) -> HistoryEntry {
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

pub(super) fn count_history_entries_in_db(state: &AppState) -> Result<u64, String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM history_entries", [], |row| row.get(0))
        .map_err(|e| format!("History read failed: {e}"))?;
    Ok(count.max(0) as u64)
}

pub(super) fn list_history_page_from_db(
    state: &AppState,
    limit: u32,
    offset: u32,
) -> Result<HistoryPage, String> {
    search_history_page_from_db(state, limit, offset, None, None, None)
}

pub(super) fn search_history_page_from_db(
    state: &AppState,
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
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
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

#[cfg(test)]
pub(super) fn list_history_entries_from_db(state: &AppState) -> Result<Vec<HistoryEntry>, String> {
    Ok(list_history_page_from_db(state, u32::MAX, 0)?.entries)
}

pub(super) fn get_history_stats_from_db(state: &AppState) -> Result<HistoryStats, String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
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

pub(super) fn insert_history_entry_in_db(
    state: &AppState,
    entry: &HistoryEntry,
) -> Result<(), String> {
    let entry = normalize_history_entry(entry.clone());
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
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

pub(super) fn insert_captions_in_db(
    state: &AppState,
    history_entry_id: &str,
    captions: &[SavedCaption],
) -> Result<(), String> {
    if captions.is_empty() {
        return Ok(());
    }

    let mut conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    let transaction = conn
        .transaction()
        .map_err(|e| format!("Caption transaction failed: {e}"))?;
    {
        let mut statement = transaction
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
    transaction
        .commit()
        .map_err(|e| format!("Caption transaction failed: {e}"))?;
    Ok(())
}

pub(super) fn store_transcription_for_history_entry(
    state: &AppState,
    job: &DownloadJob,
    history_entry_id: &str,
    transcript_path: &str,
    language: &str,
) -> Result<(), String> {
    let text = fs::read_to_string(transcript_path)
        .map_err(|e| format!("Transcript file could not be read: {e}"))?;
    let transcription_type = if job.transcribe_timestamps {
        "text with timestamps"
    } else {
        "text"
    };
    insert_transcription_in_db(state, history_entry_id, &text, transcription_type, language)
}

pub(super) fn insert_transcription_in_db(
    state: &AppState,
    history_entry_id: &str,
    text: &str,
    transcription_type: &str,
    language: &str,
) -> Result<(), String> {
    let language = language.trim().to_ascii_lowercase();
    if language.is_empty() {
        return Err("Transcription language is required".to_string());
    }
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
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

pub(super) fn delete_history_entry_from_db(state: &AppState, id: &str) -> Result<(), String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    conn.execute("DELETE FROM history_entries WHERE id = ?1", params![id])
        .map_err(|e| format!("History delete failed: {e}"))?;
    Ok(())
}

pub(super) fn clear_history_entries_in_db(state: &AppState) -> Result<(), String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    conn.execute("DELETE FROM history_entries", [])
        .map_err(|e| format!("History clear failed: {e}"))?;
    Ok(())
}

pub(super) fn migrate_legacy_history_json(app: &AppHandle, state: &AppState) -> Result<(), String> {
    if count_history_entries_in_db(state)? > 0 {
        return Ok(());
    }

    for entry in load_legacy_history_json(app) {
        insert_history_entry_in_db(state, &entry)?;
    }
    Ok(())
}
