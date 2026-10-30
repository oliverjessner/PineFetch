//! Short, guarded SQLite operations. Result fields survive every failed attempt.
use crate::database::{self, Database};
use crate::files::current_timestamp_millis;
use crate::models::{
    OcrContent, OcrDuplicatePreview, OcrDuplicateRemoval, OcrFrameSummary, OcrResult, OcrSelection,
};
use crate::ocr_rules::{self, OcrBlock, OcrFrame, OcrSegment};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeMap;

pub(crate) fn saved_path(db: &Database, id: &str) -> Result<String, String> {
    let stored: Option<(Option<String>, bool, Option<String>, bool)> = {
        let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
        conn.query_row(
            "SELECT history.output_path,
             COALESCE(history.medium='transcript',0) OR transcription.history_entry_id IS NOT NULL,
             transcription.media_path, COALESCE(transcription.media_path_known,0)
             FROM history_entries AS history
             LEFT JOIN transcriptions AS transcription ON transcription.history_entry_id=history.id
             WHERE history.id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
    };
    let (path, is_transcript, media_path, media_path_known) =
        stored.ok_or("This history entry no longer exists.")?;
    if media_path_known {
        // An explicit missing/unavailable source must never select a nearby file.
        let media_path = media_path.ok_or(crate::ocr_source::NO_VIDEO)?;
        crate::ocr_source::validate(std::path::Path::new(&media_path))?;
        return Ok(media_path);
    }
    let path = path
        .filter(|p| !p.trim().is_empty())
        .ok_or(crate::ocr_source::NO_VIDEO)?;
    crate::ocr_source::resolve(std::path::Path::new(&path), is_transcript)?
        .into_os_string()
        .into_string()
        .map_err(|_| "The saved video path is not valid Unicode.".into())
}

pub(crate) fn begin(db: &Database, id: &str, attempt: &str) -> Result<(), String> {
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM history_entries WHERE id=?1)",
            [id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err("This history entry no longer exists.".into());
    }
    tx.execute("INSERT INTO history_ocr (history_entry_id,last_attempt_id,last_attempt_status,last_attempt_at)
        VALUES (?1,?2,'running',?3) ON CONFLICT(history_entry_id) DO UPDATE SET
        last_attempt_id=excluded.last_attempt_id,last_attempt_status='running',last_attempt_at=excluded.last_attempt_at,last_error=NULL",
        params![id, attempt, current_timestamp_millis() as i64]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub(crate) fn finish(
    db: &Database,
    id: &str,
    attempt: &str,
    result: &OcrResult,
) -> Result<(), String> {
    let mut bytes = result
        .settings_json
        .len()
        .saturating_add(result.engine_version.len());
    for frame in &result.frames {
        if frame.full_text != ocr_rules::frame_text(&frame.blocks) || frame.full_text.is_empty() {
            return Err("OCR frame text does not match its original blocks.".into());
        }
        bytes = bytes.saturating_add(ocr_rules::frame_storage_bytes(frame)?);
    }
    if bytes > ocr_rules::MAX_RESULT_BYTES {
        return Err("OCR result exceeds the 32 MiB storage limit.".into());
    }
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    let changed = tx
        .execute(
            "UPDATE history_ocr SET text=NULL,segments_json=NULL,result_format='frames',result_revision=?3,source_sha256=?4,
        engine='apple_vision',engine_version=?5,settings_json=?6,result_created_at=?7,
        last_attempt_status='completed',last_error=NULL
        WHERE history_entry_id=?1 AND last_attempt_id=?2 AND last_attempt_status='running'",
            params![
                id,
                attempt,
                uuid::Uuid::new_v4().to_string(),
                result.source_sha256,
                result.engine_version,
                result.settings_json,
                current_timestamp_millis() as i64
            ],
        )
        .map_err(|e| format!("OCR result could not be saved: {e}"))?;
    if changed != 1 {
        return Err("OCR attempt is no longer current or its history entry was deleted.".into());
    }
    tx.execute(
        "DELETE FROM history_ocr_frames WHERE history_entry_id=?1",
        [id],
    )
    .map_err(|e| e.to_string())?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO history_ocr_frames (id,history_entry_id,timestamp_ms,frame_key,full_text,blocks_json)
             VALUES (?1,?2,?3,?4,?5,?6)").map_err(|e| e.to_string())?;
        for frame in &result.frames {
            insert_frame(&mut insert, id, frame)?;
        }
    }
    tx.commit()
        .map_err(|e| format!("OCR result could not be committed: {e}"))
}

pub(crate) fn fail(
    db: &Database,
    id: &str,
    attempt: &str,
    status: &str,
    error: &str,
) -> Result<(), String> {
    if !["error", "cancelled", "interrupted"].contains(&status) {
        return Err("Invalid OCR failure status.".into());
    }
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    tx.execute(
        "UPDATE history_ocr SET last_attempt_status=?3,last_error=?4
        WHERE history_entry_id=?1 AND last_attempt_id=?2 AND last_attempt_status='running'",
        params![id, attempt, status, error],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub(crate) fn interrupt_orphans(db: &Database) -> Result<(), String> {
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let running: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM history_ocr WHERE last_attempt_status='running')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !running {
        return Ok(());
    }
    let tx = database::write_transaction(&conn)?;
    tx.execute("UPDATE history_ocr SET last_attempt_status='interrupted',last_error='The app stopped before OCR finished.' WHERE last_attempt_status='running'", [])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub(crate) fn get(db: &Database, id: &str) -> Result<Option<OcrContent>, String> {
    page(db, id, 100, 0, "", None)
}

fn read(conn: &Connection, id: &str) -> Result<Option<OcrContent>, String> {
    conn.query_row("SELECT history_entry_id,text,result_format,result_revision,source_sha256,engine,engine_version,settings_json,
        result_created_at,last_attempt_id,last_attempt_status,last_attempt_at,last_error FROM history_ocr WHERE history_entry_id=?1", [id], |r| {
        Ok(OcrContent { history_entry_id:r.get(0)?,text:r.get(1)?,result_format:r.get(2)?,result_revision:r.get(3)?,
            frames:Vec::new(),total_count:0,matched_count:0,offset:0,has_more:false,
            source_sha256:r.get(4)?,engine:r.get(5)?,engine_version:r.get(6)?,settings_json:r.get(7)?,
            result_created_at:r.get(8)?,last_attempt_id:r.get(9)?,last_attempt_status:r.get(10)?,last_attempt_at:r.get(11)?,last_error:r.get(12)? })
    }).optional().map_err(|e| format!("OCR could not be loaded: {e}"))
}

fn validate_revision(content: &OcrContent, revision: Option<&str>) -> Result<(), String> {
    if revision.is_some_and(|revision| content.result_revision.as_deref() != Some(revision)) {
        return Err("OCR results changed. Reload the detected text and try again.".into());
    }
    Ok(())
}

pub(crate) fn page(
    db: &Database,
    id: &str,
    limit: u64,
    offset: u64,
    search: &str,
    revision: Option<&str>,
) -> Result<Option<OcrContent>, String> {
    if !(1..=100).contains(&limit) || offset > i64::MAX as u64 || search.len() > 4096 {
        return Err("Invalid OCR page request.".into());
    }
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    // A read snapshot keeps count, page and revision consistent across processes.
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let content = read_page(
        &tx,
        id,
        limit,
        offset,
        crate::unicode_search::trim_query(search),
        revision,
    )?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(content)
}

fn read_page(
    conn: &Connection,
    id: &str,
    limit: u64,
    offset: u64,
    search: &str,
    revision: Option<&str>,
) -> Result<Option<OcrContent>, String> {
    let Some(mut content) = read(conn, id)? else {
        return Ok(None);
    };
    validate_revision(&content, revision)?;
    content.total_count = conn
        .query_row(
            "SELECT COUNT(*) FROM history_ocr_frames WHERE history_entry_id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    content.matched_count = conn.query_row(
        "SELECT COUNT(*) FROM history_ocr_frames WHERE history_entry_id=?1 AND (?2='' OR pinefetch_contains(full_text,?2))",
        params![id,search], |r| r.get(0)).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id,timestamp_ms,frame_key,full_text FROM history_ocr_frames
         WHERE history_entry_id=?1 AND (?2='' OR pinefetch_contains(full_text,?2))
         ORDER BY timestamp_ms,id LIMIT ?3 OFFSET ?4",
        )
        .map_err(|e| e.to_string())?;
    content.frames = stmt
        .query_map(params![id, search, limit, offset], |r| {
            Ok(OcrFrameSummary {
                id: r.get(0)?,
                timestamp_ms: r.get(1)?,
                frame_key: r.get(2)?,
                full_text: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    content.offset = offset;
    content.has_more = offset.saturating_add(content.frames.len() as u64) < content.matched_count;
    Ok(Some(content))
}

fn selection_time(conn: &Connection, id: &str, selection: &OcrSelection) -> Result<u64, String> {
    let content = read(conn, id)?.ok_or("No saved OCR result is available.")?;
    validate_revision(&content, Some(&selection.result_revision))?;
    conn.query_row(
        "SELECT timestamp_ms FROM history_ocr_frames WHERE history_entry_id=?1 AND id=?2",
        params![id, selection.frame_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "This OCR frame is no longer available.".into())
}

pub(crate) fn selected_time(
    db: &Database,
    id: &str,
    selection: &OcrSelection,
) -> Result<u64, String> {
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let time = selection_time(&tx, id, selection)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(time)
}

pub(crate) fn export_text(
    db: &Database,
    id: &str,
    revision: Option<&str>,
) -> Result<String, String> {
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let content = read(&tx, id)?.ok_or("No saved OCR result is available.")?;
    validate_revision(&content, revision)?;
    let mut text = String::new();
    {
        let mut stmt = tx.prepare("SELECT timestamp_ms,full_text FROM history_ocr_frames WHERE history_entry_id=?1 ORDER BY timestamp_ms,id").map_err(|e| e.to_string())?;
        let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&format!(
                "[{}]\n{}",
                ocr_rules::timestamp(row.get(0).map_err(|e| e.to_string())?),
                row.get::<_, String>(1).map_err(|e| e.to_string())?
            ));
        }
    }
    if let Some(legacy) = content.text.filter(|text| !text.is_empty()) {
        if !text.is_empty() {
            text.push_str("\n\n[Unmapped legacy text]\n");
        }
        text.push_str(&legacy);
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(text)
}

fn editable_result(conn: &Connection, id: &str) -> Result<OcrContent, String> {
    let content = read(conn, id)?.ok_or("No saved OCR result is available.")?;
    if content.last_attempt_status == "running" {
        return Err("Wait for the OCR analysis to finish before removing detected text.".into());
    }
    if content.result_revision.is_none() {
        return Err("No saved OCR result is available.".into());
    }
    Ok(content)
}

fn duplicate_ids(conn: &Connection, id: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn.prepare("SELECT id,timestamp_ms,frame_key,full_text FROM history_ocr_frames WHERE history_entry_id=?1 ORDER BY timestamp_ms,id")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
    let mut run = ocr_rules::DuplicateRun::default();
    let mut duplicates = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let text = row
            .get_ref(3)
            .map_err(|e| e.to_string())?
            .as_str()
            .map_err(|e| e.to_string())?;
        if run.push(
            row.get(1).map_err(|e| e.to_string())?,
            row.get(2).map_err(|e| e.to_string())?,
            text,
        ) {
            duplicates.push(row.get(0).map_err(|e| e.to_string())?);
        }
    }
    Ok(duplicates)
}

pub(crate) fn preview_duplicates(db: &Database, id: &str) -> Result<OcrDuplicatePreview, String> {
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let content = editable_result(&tx, id)?;
    let duplicate_count = duplicate_ids(&tx, id)?.len() as u64;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(OcrDuplicatePreview {
        duplicate_count,
        result_revision: content.result_revision.unwrap(),
    })
}

pub(crate) fn remove_duplicates(
    db: &Database,
    id: &str,
    revision: &str,
) -> Result<OcrDuplicateRemoval, String> {
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    // BEGIN IMMEDIATE serializes other app instances and analysis starts/saves.
    let tx = database::write_transaction(&conn)?;
    let content = editable_result(&tx, id)?;
    validate_revision(&content, Some(revision))?;
    let duplicates = duplicate_ids(&tx, id)?;
    let mut removed_count = 0;
    if !duplicates.is_empty() {
        let mut delete = tx
            .prepare("DELETE FROM history_ocr_frames WHERE history_entry_id=?1 AND id=?2")
            .map_err(|e| e.to_string())?;
        for frame_id in duplicates {
            removed_count += delete
                .execute(params![id, frame_id])
                .map_err(|e| e.to_string())? as u64;
        }
        tx.execute("UPDATE history_ocr SET result_revision=?2 WHERE history_entry_id=?1 AND result_revision=?3",
            params![id, uuid::Uuid::new_v4().to_string(), revision]).map_err(|e| e.to_string())?;
    }
    let content =
        read_page(&tx, id, 100, 0, "", None)?.ok_or("The OCR result is no longer available.")?;
    tx.commit()
        .map_err(|e| format!("OCR duplicate removal could not be committed: {e}"))?;
    Ok(OcrDuplicateRemoval {
        removed_count,
        content,
    })
}

pub(crate) fn delete_frame(
    db: &Database,
    id: &str,
    selection: &OcrSelection,
) -> Result<OcrContent, String> {
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    let content = read(&tx, id)?.ok_or("No saved OCR result is available.")?;
    if content.last_attempt_status == "running" {
        return Err("Wait for the OCR analysis to finish before deleting detected text.".into());
    }
    selection_time(&tx, id, selection)?;
    tx.execute(
        "DELETE FROM history_ocr_frames WHERE history_entry_id=?1 AND id=?2",
        params![id, selection.frame_id],
    )
    .map_err(|e| format!("OCR frame text could not be deleted: {e}"))?;
    tx.execute(
        "UPDATE history_ocr SET result_revision=?2 WHERE history_entry_id=?1",
        params![id, uuid::Uuid::new_v4().to_string()],
    )
    .map_err(|e| format!("OCR text could not be deleted: {e}"))?;
    let updated =
        read_page(&tx, id, 100, 0, "", None)?.ok_or("The OCR result is no longer available.")?;
    tx.commit()
        .map_err(|e| format!("OCR deletion could not be committed: {e}"))?;
    Ok(updated)
}

fn insert_frame(
    stmt: &mut rusqlite::Statement<'_>,
    id: &str,
    frame: &OcrFrame,
) -> Result<String, String> {
    let blocks = serde_json::to_string(&frame.blocks).map_err(|e| e.to_string())?;
    stmt.execute(params![
        frame.id,
        id,
        frame.timestamp_ms,
        frame.frame_key,
        frame.full_text,
        blocks
    ])
    .map_err(|e| format!("OCR frame could not be saved: {e}"))?;
    Ok(blocks)
}

// Called only by additive migration 4, under its backup and single transaction.
// Index legacy timestamps by segment reference, not expanded copies of text.
// Expanded legacy rows may exceed the old compressed size; migrate one frame at
// a time without materializing a full-video JSON or expanded export in memory.
pub(crate) fn migrate_frames(conn: &Connection) -> Result<(), String> {
    let mut source = conn.prepare("SELECT history_entry_id,segments_json,text,result_created_at FROM history_ocr ORDER BY history_entry_id").map_err(|e| e.to_string())?;
    let mut rows = source.query([]).map_err(|e| e.to_string())?;
    let mut insert = conn.prepare("INSERT INTO history_ocr_frames (id,history_entry_id,timestamp_ms,frame_key,full_text,blocks_json) VALUES (?1,?2,?3,?4,?5,?6)").map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: String = row.get(0).map_err(|e| e.to_string())?;
        let json: Option<String> = row.get(1).map_err(|e| e.to_string())?;
        let legacy_text: Option<String> = row.get(2).map_err(|e| e.to_string())?;
        let created: Option<u64> = row.get(3).map_err(|e| e.to_string())?;
        if json.is_none() && created.is_none() && legacy_text.is_none() {
            continue;
        }
        let segments: Vec<OcrSegment> = json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| format!("Invalid legacy OCR segments for {id}: {e}"))?
            .unwrap_or_default();
        let mut times: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
        let mut unmapped = Vec::new();
        for (index, segment) in segments.iter().enumerate() {
            if segment.text.is_empty() {
                continue;
            }
            if segment.observed_at_ms.is_empty() {
                unmapped.push(segment.text.as_str());
            }
            for time in &segment.observed_at_ms {
                // A repeated timestamp in one run represents the same stored
                // observation. Equal texts from distinct segments remain intact.
                let indexes = times.entry(*time).or_default();
                if !indexes.contains(&index) {
                    indexes.push(index);
                }
            }
        }
        let expected = times.len() as u64;
        for (time, indexes) in times {
            let mut blocks: Vec<_> = indexes
                .into_iter()
                .map(|index| {
                    let segment = &segments[index];
                    OcrBlock {
                        text: segment.text.clone(),
                        bounds: None,
                        confidence: None,
                        metadata_source: "legacy_segment".into(),
                        legacy_segment_bounds: segment.bounds.clone(),
                    }
                })
                .collect();
            ocr_rules::order_blocks(&mut blocks);
            let frame = OcrFrame {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp_ms: time,
                frame_key: None,
                full_text: ocr_rules::frame_text(&blocks),
                blocks,
            };
            let expected_json = insert_frame(&mut insert, &id, &frame)?;
            // Verify the exact stored payload before retiring its source JSON.
            // Reparsing JSON floats can change legacy coordinates by one ULP
            // even when SQLite preserved every byte of the serialized value.
            let (text, json): (String, String) = conn
                .query_row(
                    "SELECT full_text,blocks_json FROM history_ocr_frames WHERE id=?1",
                    [&frame.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|e| e.to_string())?;
            if text != frame.full_text || json != expected_json {
                return Err("Migrated OCR frame verification failed.".into());
            }
        }
        let actual: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM history_ocr_frames WHERE history_entry_id=?1",
                [&id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if actual != expected {
            return Err("Migrated OCR frame count verification failed.".into());
        }
        let unstructured = if !unmapped.is_empty() {
            Some(unmapped.join("\n"))
        } else if json.is_none()
            || segments.is_empty() && legacy_text.as_ref().is_some_and(|s| !s.is_empty())
        {
            legacy_text
        } else {
            None
        };
        let format = if json.is_some() && (expected > 0 || unstructured.is_none()) {
            "frames"
        } else {
            "legacy_text"
        };
        conn.execute("UPDATE history_ocr SET segments_json=NULL,text=?2,result_format=?3,result_revision=?4 WHERE history_entry_id=?1",
            params![id,unstructured,format,uuid::Uuid::new_v4().to_string()]).map_err(|e| e.to_string())?;
    }
    Ok(())
}
