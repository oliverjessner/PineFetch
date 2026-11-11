//! Transcript revisions own separate summary and conversation records.
//! Neither table depends on the other; summary regeneration cannot modify chat.
use crate::database::{self, Database};
use crate::hashing::sha256_hex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub(crate) const SCHEMA_SQL: &str = r#"
CREATE TABLE history_ai_summaries (
    history_entry_id TEXT NOT NULL,
    transcript_hash TEXT NOT NULL CHECK(length(transcript_hash)=64),
    generation_id TEXT NOT NULL,
    quick_summary TEXT NOT NULL CHECK(length(quick_summary)>0 AND length(quick_summary)<=32768),
    key_takeaways_json TEXT NOT NULL CHECK(length(key_takeaways_json)<=65536 AND json_valid(key_takeaways_json) AND json_type(key_takeaways_json)='array'),
    metadata_json TEXT NOT NULL CHECK(length(metadata_json)<=16384 AND json_valid(metadata_json) AND json_type(metadata_json)='object'),
    created_at INTEGER NOT NULL CHECK(created_at>=0),
    updated_at INTEGER NOT NULL CHECK(updated_at>=created_at),
    PRIMARY KEY(history_entry_id,transcript_hash),
    FOREIGN KEY(history_entry_id) REFERENCES history_entries(id) ON DELETE CASCADE
);
CREATE TABLE history_ai_chat (
    id TEXT PRIMARY KEY NOT NULL,
    history_entry_id TEXT NOT NULL,
    transcript_hash TEXT NOT NULL CHECK(length(transcript_hash)=64),
    sequence INTEGER NOT NULL CHECK(sequence>0),
    question TEXT NOT NULL CHECK(length(question)>0 AND length(question)<=16000),
    answer TEXT NOT NULL CHECK(length(answer)>0 AND length(answer)<=65536),
    metadata_json TEXT NOT NULL CHECK(length(metadata_json)<=16384 AND json_valid(metadata_json) AND json_type(metadata_json)='object'),
    created_at INTEGER NOT NULL CHECK(created_at>=0),
    UNIQUE(history_entry_id,transcript_hash,sequence),
    FOREIGN KEY(history_entry_id) REFERENCES history_entries(id) ON DELETE CASCADE
);
"#;

const MAX_TRANSCRIPT_BYTES: usize = 16 * 1024 * 1024;
const STALE_TRANSCRIPT: &str =
    "The saved transcript changed during this request. Reopen AI Video Summary and try again.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SummaryContent {
    pub(crate) quick_summary: String,
    pub(crate) key_takeaways: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct GenerationMetadata {
    pub(crate) provider: String,
    pub(crate) model: Option<String>,
    pub(crate) cli_version: String,
    pub(crate) context_strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) transcript_hash: String,
    pub(crate) generation_id: String,
    pub(crate) quick_summary: String,
    pub(crate) key_takeaways: Vec<String>,
    pub(crate) metadata: GenerationMetadata,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ChatMessage {
    pub(crate) id: String,
    pub(crate) role: String,
    pub(crate) content: String,
    pub(crate) created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct VideoAiSnapshot {
    pub(crate) transcript_available: bool,
    pub(crate) transcript_hash: Option<String>,
    pub(crate) summary: Option<Summary>,
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) invalidated: bool,
    pub(crate) summary_error: Option<String>,
    pub(crate) chat_error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct TranscriptSnapshot {
    pub(crate) history_entry_id: String,
    pub(crate) transcript_hash: String,
    pub(crate) text: String,
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) chat_revision: i64,
}

pub(crate) fn snapshot(db: &Database, id: &str) -> Result<VideoAiSnapshot, String> {
    db.read_snapshot(|conn| {
        let text = saved_transcript(conn, id)?;
        let transcript_hash = text.as_ref().map(|text| sha256_hex(text.as_bytes()));
        let (summary, summary_error, messages, chat_error) = match &transcript_hash {
            Some(hash) => {
                let (summary, summary_error) = match read_summary(conn, id, hash) {
                    Ok(summary) => (summary, None),
                    Err(error) => (None, Some(error)),
                };
                let (messages, chat_error) = match read_messages(conn, id, hash) {
                    Ok(messages) => (messages, None),
                    Err(error) => (Vec::new(), Some(error)),
                };
                (summary, summary_error, messages, chat_error)
            }
            None => (None, None, Vec::new(), None),
        };
        let invalidated = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM history_ai_summaries WHERE history_entry_id=?1 AND (?2 IS NULL OR transcript_hash!=?2))
                 OR EXISTS(SELECT 1 FROM history_ai_chat WHERE history_entry_id=?1 AND (?2 IS NULL OR transcript_hash!=?2))",
            params![id, transcript_hash], |row| row.get(0),
        ).map_err(|e| format!("Video AI revision read failed: {e}"))?;
        Ok(VideoAiSnapshot {
            transcript_available: transcript_hash.is_some(),
            transcript_hash,
            summary,
            messages,
            invalidated,
            summary_error,
            chat_error,
        })
    })
}

pub(crate) fn transcript(db: &Database, id: &str) -> Result<TranscriptSnapshot, String> {
    transcript_snapshot(db, id, true)
}

pub(crate) fn transcript_for_summary(
    db: &Database,
    id: &str,
) -> Result<TranscriptSnapshot, String> {
    transcript_snapshot(db, id, false)
}

fn transcript_snapshot(
    db: &Database,
    id: &str,
    with_chat: bool,
) -> Result<TranscriptSnapshot, String> {
    db.read_snapshot(|conn| {
        let text = saved_transcript(conn, id)?.ok_or(
            "No saved transcript is available. Create a transcript before using video AI.",
        )?;
        let transcript_hash = sha256_hex(text.as_bytes());
        Ok(TranscriptSnapshot {
            history_entry_id: id.to_owned(),
            messages: if with_chat {
                read_messages(conn, id, &transcript_hash)?
            } else {
                Vec::new()
            },
            chat_revision: if with_chat {
                chat_revision(conn, id, &transcript_hash)?
            } else {
                0
            },
            transcript_hash,
            text,
        })
    })
}

// Callers receive owned transcript/chat data and release SQLite before invoking
// Codex. A writer reservation rechecks the live transcript before publication.
pub(crate) fn save_summary(
    db: &Database,
    source: &TranscriptSnapshot,
    content: &SummaryContent,
    metadata: &GenerationMetadata,
) -> Result<Summary, String> {
    validate_summary(content)?;
    let takeaways = serde_json::to_string(&content.key_takeaways).map_err(|e| e.to_string())?;
    if takeaways.len() > 65536 {
        return Err("The generated key takeaways are too large to save.".into());
    }
    let metadata_json = metadata_json(metadata)?;
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    verify_revision(&tx, source)?;
    let now = now_ms()?;
    tx.execute(
        "INSERT INTO history_ai_summaries(history_entry_id,transcript_hash,generation_id,quick_summary,key_takeaways_json,metadata_json,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?7)
         ON CONFLICT(history_entry_id,transcript_hash) DO UPDATE SET
             generation_id=excluded.generation_id,quick_summary=excluded.quick_summary,
             key_takeaways_json=excluded.key_takeaways_json,metadata_json=excluded.metadata_json,
             updated_at=MAX(history_ai_summaries.created_at,excluded.updated_at)",
        params![source.history_entry_id, source.transcript_hash, Uuid::new_v4().to_string(),
            content.quick_summary, takeaways, metadata_json, now],
    ).map_err(|e| format!("Video summary save failed: {e}"))?;
    let summary = read_summary(&tx, &source.history_entry_id, &source.transcript_hash)?
        .ok_or("Saved video summary is missing")?;
    tx.commit()
        .map_err(|e| format!("Video summary commit failed: {e}"))?;
    Ok(summary)
}

pub(crate) fn append_exchange(
    db: &Database,
    source: &TranscriptSnapshot,
    question: &str,
    answer: &str,
    metadata: &GenerationMetadata,
) -> Result<Vec<ChatMessage>, String> {
    validate_text(question, 16000, "Question")?;
    validate_text(answer, 65536, "Answer")?;
    let metadata_json = metadata_json(metadata)?;
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    verify_revision(&tx, source)?;
    if chat_revision(&tx, &source.history_entry_id, &source.transcript_hash)?
        != source.chat_revision
    {
        return Err(
            "This video's conversation changed during the request. Try your question again.".into(),
        );
    }
    let sequence = source
        .chat_revision
        .checked_add(1)
        .ok_or("Video chat revision exhausted")?;
    tx.execute(
        "INSERT INTO history_ai_chat(id,history_entry_id,transcript_hash,sequence,question,answer,metadata_json,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![Uuid::new_v4().to_string(),source.history_entry_id,source.transcript_hash,
            sequence,question,answer,metadata_json,now_ms()?],
    ).map_err(|e| format!("Video chat save failed: {e}"))?;
    let messages = read_messages(&tx, &source.history_entry_id, &source.transcript_hash)?;
    tx.commit()
        .map_err(|e| format!("Video chat commit failed: {e}"))?;
    Ok(messages)
}

fn saved_transcript(conn: &Connection, id: &str) -> Result<Option<String>, String> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM history_entries WHERE id=?1)",
            [id],
            |row| row.get(0),
        )
        .map_err(|e| format!("Video AI History read failed: {e}"))?;
    if !exists {
        return Err("This History item no longer exists.".into());
    }
    let length: Option<i64> = conn
        .query_row(
            "SELECT length(CAST(text AS BLOB)) FROM transcriptions WHERE history_entry_id=?1",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Video AI transcript read failed: {e}"))?;
    if length.is_some_and(|length| length > MAX_TRANSCRIPT_BYTES as i64) {
        return Err(
            "This transcript exceeds the 16 MiB video AI limit. The saved transcript is preserved."
                .into(),
        );
    }
    let text =
        crate::history::get_history_transcript_in_conn(conn, id)?.map(|transcript| transcript.text);
    Ok(text.filter(|text| !text.trim().is_empty()))
}

fn verify_revision(conn: &Connection, source: &TranscriptSnapshot) -> Result<(), String> {
    let text = saved_transcript(conn, &source.history_entry_id)?.ok_or(STALE_TRANSCRIPT)?;
    if sha256_hex(text.as_bytes()) != source.transcript_hash {
        return Err(STALE_TRANSCRIPT.into());
    }
    Ok(())
}

fn read_summary(conn: &Connection, id: &str, hash: &str) -> Result<Option<Summary>, String> {
    let row: Option<(String, String, String, String, i64, i64)> = conn.query_row(
        "SELECT generation_id,quick_summary,key_takeaways_json,metadata_json,created_at,updated_at
         FROM history_ai_summaries WHERE history_entry_id=?1 AND transcript_hash=?2",
        params![id, hash], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?)),
    ).optional().map_err(|e| format!("Video summary read failed: {e}"))?;
    row.map(
        |(generation_id, quick_summary, takeaways, metadata, created_at, updated_at)| {
            let content = SummaryContent {
                quick_summary,
                key_takeaways: serde_json::from_str(&takeaways)
                    .map_err(|_| "Saved key takeaways are invalid; data preserved.")?,
            };
            validate_saved_summary(&content)?;
            Ok(Summary {
                transcript_hash: hash.to_owned(),
                generation_id,
                quick_summary: content.quick_summary,
                key_takeaways: content.key_takeaways,
                metadata: serde_json::from_str(&metadata)
                    .map_err(|_| "Saved summary metadata is invalid; data preserved.")?,
                created_at,
                updated_at,
            })
        },
    )
    .transpose()
}

fn read_messages(conn: &Connection, id: &str, hash: &str) -> Result<Vec<ChatMessage>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id,question,answer,created_at FROM history_ai_chat
         WHERE history_entry_id=?1 AND transcript_hash=?2 ORDER BY sequence",
        )
        .map_err(|e| format!("Video chat read failed: {e}"))?;
    let rows = stmt
        .query_map(params![id, hash], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| format!("Video chat read failed: {e}"))?;
    let mut messages = Vec::new();
    for row in rows {
        let (id, question, answer, created_at) =
            row.map_err(|e| format!("Video chat read failed: {e}"))?;
        messages.push(ChatMessage {
            id: format!("{id}:user"),
            role: "user".into(),
            content: question,
            created_at,
        });
        messages.push(ChatMessage {
            id: format!("{id}:assistant"),
            role: "assistant".into(),
            content: answer,
            created_at,
        });
    }
    Ok(messages)
}

fn chat_revision(conn: &Connection, id: &str, hash: &str) -> Result<i64, String> {
    conn.query_row("SELECT COALESCE(MAX(sequence),0) FROM history_ai_chat WHERE history_entry_id=?1 AND transcript_hash=?2",params![id,hash],|row| row.get(0))
        .map_err(|e| format!("Video chat revision read failed: {e}"))
}

pub(crate) fn validate_summary(content: &SummaryContent) -> Result<(), String> {
    validate_saved_summary(content)?;
    if !(3..=8).contains(&content.key_takeaways.len()) {
        return Err("New Key Takeaways must contain between 3 and 8 items.".into());
    }
    Ok(())
}

// Existing summaries used wider item limits. Continue reading them unchanged;
// the stricter generation contract applies only to newly saved results.
fn validate_saved_summary(content: &SummaryContent) -> Result<(), String> {
    validate_text(&content.quick_summary, 32768, "Quick Summary")?;
    if content.key_takeaways.is_empty() || content.key_takeaways.len() > 20 {
        return Err("Key Takeaways must contain between 1 and 20 items.".into());
    }
    for takeaway in &content.key_takeaways {
        validate_text(takeaway, 8192, "Key Takeaway")?;
    }
    Ok(())
}

fn validate_text(value: &str, max: usize, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{label} is empty or exceeds the supported size."));
    }
    Ok(())
}

fn metadata_json(metadata: &GenerationMetadata) -> Result<String, String> {
    validate_text(&metadata.provider, 128, "AI provider")?;
    validate_text(&metadata.cli_version, 256, "Codex version")?;
    validate_text(&metadata.context_strategy, 4096, "Context strategy")?;
    if let Some(model) = &metadata.model {
        validate_text(model, 256, "AI model")?;
    }
    let json = serde_json::to_string(metadata).map_err(|e| e.to_string())?;
    if json.len() > 16384 {
        return Err("AI generation metadata is too large to save.".into());
    }
    Ok(json)
}

fn now_ms() -> Result<i64, String> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "System clock predates the Unix epoch")?
            .as_millis(),
    )
    .map_err(|_| "System clock exceeds supported range".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::MigrationPoint;
    use crate::test_support::TempRoot;
    use std::sync::{Arc, Barrier};

    fn fixture(conn: &Connection) {
        database::initialize(conn).unwrap();
        conn.execute_batch(
            "INSERT INTO history_entries(id,url,created_at) VALUES('one','https://example.com/one',1),('two','https://example.com/two',2),('empty','https://example.com/empty',3);
             INSERT INTO transcriptions(id,history_entry_id,text,\"type\") VALUES('t1','one','The first transcript states that the launch is on Tuesday. Engineers will inspect the capsule. Weather may delay the launch.','text'),('t2','two','The second transcript is about a different project.','text');",
        ).unwrap();
    }

    fn db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        fixture(&conn);
        Database::new(conn)
    }

    fn metadata() -> GenerationMetadata {
        GenerationMetadata {
            provider: "codex_chatgpt".into(),
            model: None,
            cli_version: "codex-cli 1.0.0".into(),
            context_strategy: "full_transcript".into(),
        }
    }

    fn summary(text: &str) -> SummaryContent {
        SummaryContent {
            quick_summary: text.into(),
            key_takeaways: vec![
                "The launch is on Tuesday.".into(),
                "Engineers will inspect the capsule.".into(),
                "Weather may delay the launch.".into(),
            ],
        }
    }

    #[test]
    fn chat_is_saved_without_a_summary_and_survives_summary_generation_and_regeneration() {
        let db = db();
        let source = transcript(&db, "one").unwrap();
        assert!(snapshot(&db, "one").unwrap().summary.is_none());
        let messages = append_exchange(
            &db,
            &source,
            "When is the launch?",
            "The transcript says Tuesday.",
            &metadata(),
        )
        .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
        let before = snapshot(&db, "one").unwrap();
        assert!(before.summary.is_none());
        let first = save_summary(&db, &source, &summary("A Tuesday launch."), &metadata()).unwrap();
        let second = save_summary(
            &db,
            &source,
            &summary("The project launches Tuesday."),
            &metadata(),
        )
        .unwrap();
        assert_ne!(first.generation_id, second.generation_id);
        assert_eq!(first.created_at, second.created_at);
        assert!(second.updated_at >= first.updated_at);
        assert_eq!(snapshot(&db, "one").unwrap().messages, before.messages);
        let source = transcript(&db, "one").unwrap();
        assert_eq!(source.messages, before.messages);
        let messages = append_exchange(
            &db,
            &source,
            "Where is the launch?",
            "The transcript does not say where the launch is.",
            &metadata(),
        )
        .unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(snapshot(&db, "one").unwrap().summary.unwrap(), second);
    }

    #[test]
    fn changed_or_removed_transcript_hides_old_results_and_refuses_stale_publication() {
        let db = db();
        let old = transcript(&db, "one").unwrap();
        save_summary(&db, &old, &summary("Old summary."), &metadata()).unwrap();
        append_exchange(&db, &old, "When?", "Tuesday.", &metadata()).unwrap();
        db.lock().unwrap().execute("UPDATE transcriptions SET text='The launch moved to Friday.' WHERE history_entry_id='one'", []).unwrap();
        let current = snapshot(&db, "one").unwrap();
        assert!(current.transcript_available && current.invalidated);
        assert_ne!(
            current.transcript_hash.as_deref(),
            Some(old.transcript_hash.as_str())
        );
        assert!(current.summary.is_none());
        assert!(current.messages.is_empty());
        assert!(
            save_summary(&db, &old, &summary("Stale summary."), &metadata())
                .unwrap_err()
                .contains("transcript changed")
        );
        assert!(append_exchange(&db, &old, "When?", "Tuesday.", &metadata())
            .unwrap_err()
            .contains("transcript changed"));
        let fresh = transcript(&db, "one").unwrap();
        assert_eq!(fresh.chat_revision, 0);
        append_exchange(&db, &fresh, "When?", "Friday.", &metadata()).unwrap();
        assert_eq!(snapshot(&db, "one").unwrap().messages[1].content, "Friday.");
        // Historical revisions remain available for provenance, but never leak
        // into the current conversation or get overwritten by a stale request.
        assert_eq!(
            db.lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM history_ai_chat", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
        db.lock()
            .unwrap()
            .execute(
                "DELETE FROM transcriptions WHERE history_entry_id='one'",
                [],
            )
            .unwrap();
        let missing = snapshot(&db, "one").unwrap();
        assert!(!missing.transcript_available);
        assert!(missing.summary.is_none() && missing.messages.is_empty() && missing.invalidated);
        assert!(append_exchange(&db, &fresh, "When?", "Friday.", &metadata()).is_err());
    }

    #[test]
    fn concurrent_chat_submissions_cannot_publish_answers_from_stale_conversation_context() {
        let db = Arc::new(db());
        let source = transcript(&db, "one").unwrap();
        let start = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let db = db.clone();
                let source = source.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    start.wait();
                    append_exchange(&db, &source, "When?", "Tuesday.", &metadata())
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert!(results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .all(|error| error.contains("conversation changed")));
        assert_eq!(snapshot(&db, "one").unwrap().messages.len(), 2);
    }

    #[test]
    fn summaries_and_chat_are_isolated_per_video_and_deleted_with_history() {
        let db = db();
        let one = transcript(&db, "one").unwrap();
        let two = transcript(&db, "two").unwrap();
        save_summary(&db, &one, &summary("One."), &metadata()).unwrap();
        append_exchange(&db, &one, "One?", "First video.", &metadata()).unwrap();
        append_exchange(&db, &two, "Two?", "Second video.", &metadata()).unwrap();
        assert!(snapshot(&db, "two").unwrap().summary.is_none());
        assert_eq!(
            snapshot(&db, "two").unwrap().messages[1].content,
            "Second video."
        );
        db.lock()
            .unwrap()
            .execute("DELETE FROM history_entries WHERE id='one'", [])
            .unwrap();
        assert!(snapshot(&db, "one").is_err());
        let conn = db.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM history_ai_summaries", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM history_ai_chat", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn missing_empty_and_oversized_transcripts_are_explicit_and_never_use_other_text() {
        let db = db();
        assert!(!snapshot(&db, "empty").unwrap().transcript_available);
        assert!(transcript(&db, "empty")
            .unwrap_err()
            .contains("No saved transcript"));
        assert!(transcript(&db, "missing")
            .unwrap_err()
            .contains("no longer exists"));
        db.lock().unwrap().execute("INSERT INTO transcriptions(id,history_entry_id,text,\"type\") VALUES('empty','empty','  \n\t','text')", []).unwrap();
        assert!(!snapshot(&db, "empty").unwrap().transcript_available);
        db.lock()
            .unwrap()
            .execute(
                "UPDATE transcriptions SET text=?1 WHERE history_entry_id='empty'",
                ["x".repeat(MAX_TRANSCRIPT_BYTES + 1)],
            )
            .unwrap();
        assert!(transcript(&db, "empty").unwrap_err().contains("16 MiB"));
    }

    #[test]
    fn invalid_summary_data_does_not_disable_chat_and_failed_writes_preserve_saved_data() {
        let db = db();
        let source = transcript(&db, "one").unwrap();
        let original = save_summary(&db, &source, &summary("Original."), &metadata()).unwrap();
        let invalid = SummaryContent {
            quick_summary: "Invalid.".into(),
            key_takeaways: vec![],
        };
        assert!(save_summary(&db, &source, &invalid, &metadata()).is_err());
        assert_eq!(snapshot(&db, "one").unwrap().summary.unwrap(), original);
        assert!(append_exchange(&db, &source, "Question", " \n", &metadata()).is_err());
        assert!(snapshot(&db, "one").unwrap().messages.is_empty());
        db.lock()
            .unwrap()
            .execute(
                "UPDATE history_ai_summaries SET key_takeaways_json='[]'",
                [],
            )
            .unwrap();
        let saved = snapshot(&db, "one").unwrap();
        assert!(saved.summary.is_none() && saved.summary_error.is_some());
        assert!(saved.chat_error.is_none());
        append_exchange(&db, &source, "When?", "Tuesday.", &metadata()).unwrap();
        assert_eq!(snapshot(&db, "one").unwrap().messages.len(), 2);
    }

    #[test]
    fn new_summary_writes_enforce_item_limits_and_keep_all_eight_items() {
        let db = db();
        let source = transcript(&db, "one").unwrap();
        let mut content = summary("A valid summary.");
        content.key_takeaways = (1..=8)
            .map(|index| format!("Distinct transcript insight {index}."))
            .collect();
        let original = save_summary(&db, &source, &content, &metadata()).unwrap();
        assert_eq!(original.key_takeaways, content.key_takeaways);
        for count in [0, 1, 2, 9, 12, 20] {
            content.key_takeaways = (1..=count)
                .map(|index| format!("Replacement insight {index}."))
                .collect();
            assert!(save_summary(&db, &source, &content, &metadata()).is_err());
            assert_eq!(snapshot(&db, "one").unwrap().summary.unwrap(), original);
        }
    }

    #[test]
    fn legacy_cached_summaries_remain_readable_without_changing_history_or_chat() {
        let db = db();
        let source = transcript(&db, "one").unwrap();
        let mut original =
            save_summary(&db, &source, &summary("Cached summary."), &metadata()).unwrap();
        let messages = append_exchange(&db, &source, "When?", "Tuesday.", &metadata()).unwrap();
        // The old decoder accepted 1–20 items, including generated 1–12 lists.
        for count in [1, 2, 9, 12, 20] {
            let takeaways = (1..=count)
                .map(|index| format!("Legacy cached insight {index}."))
                .collect::<Vec<_>>();
            db.lock()
                .unwrap()
                .execute(
                    "UPDATE history_ai_summaries SET key_takeaways_json=?1",
                    [serde_json::to_string(&takeaways).unwrap()],
                )
                .unwrap();
            original.key_takeaways = takeaways;
            let saved = snapshot(&db, "one").unwrap();
            assert_eq!(saved.summary.unwrap(), original);
            assert!(saved.summary_error.is_none());
            assert_eq!(saved.messages, messages);
        }
    }

    #[test]
    fn unreadable_chat_does_not_disable_summary_generation() {
        let db = db();
        let source = transcript(&db, "one").unwrap();
        append_exchange(&db, &source, "When?", "Tuesday.", &metadata()).unwrap();
        db.lock()
            .unwrap()
            .execute(
                "UPDATE history_ai_chat SET answer=CAST('unreadable chat value' AS BLOB)",
                [],
            )
            .unwrap();
        assert!(transcript(&db, "one").is_err());
        let source = transcript_for_summary(&db, "one").unwrap();
        assert!(source.messages.is_empty());
        save_summary(
            &db,
            &source,
            &summary("A fresh summary independent of chat."),
            &metadata(),
        )
        .unwrap();
        let saved = snapshot(&db, "one").unwrap();
        assert!(saved.summary.is_some() && saved.summary_error.is_none());
        assert!(saved.chat_error.is_some());
    }

    #[test]
    fn writes_respect_readonly_and_future_schema_guards_without_losing_saved_results() {
        let db = db();
        let source = transcript(&db, "one").unwrap();
        let saved_summary = save_summary(&db, &source, &summary("Original."), &metadata()).unwrap();
        db.lock()
            .unwrap()
            .pragma_update(None, "query_only", true)
            .unwrap();
        assert!(save_summary(&db, &source, &summary("Replacement."), &metadata()).is_err());
        assert!(append_exchange(&db, &source, "When?", "Tuesday.", &metadata()).is_err());
        let saved = snapshot(&db, "one").unwrap();
        assert_eq!(saved.summary.unwrap(), saved_summary);
        assert!(saved.messages.is_empty());
        {
            let conn = db.lock().unwrap();
            conn.pragma_update(None, "query_only", false).unwrap();
            conn.pragma_update(None, "user_version", database::SCHEMA_VERSION + 1)
                .unwrap();
        }
        assert!(
            save_summary(&db, &source, &summary("Replacement."), &metadata())
                .unwrap_err()
                .contains("Unsupported SQLite schema")
        );
        assert!(
            append_exchange(&db, &source, "When?", "Tuesday.", &metadata())
                .unwrap_err()
                .contains("Unsupported SQLite schema")
        );
        let saved = snapshot(&db, "one").unwrap();
        assert_eq!(saved.summary.unwrap(), saved_summary);
        assert!(saved.messages.is_empty());
    }

    #[test]
    fn integration_persisted_records_survive_reopen_without_duplicating_the_transcript() {
        let root = TempRoot::new("video-ai-persistence");
        let path = root.path("history.sqlite3");
        let conn = database::open(&path).unwrap();
        fixture(&conn);
        let db = Database::new(conn);
        let source = transcript(&db, "one").unwrap();
        let summary = save_summary(&db, &source, &summary("Stored summary."), &metadata()).unwrap();
        let messages = append_exchange(&db, &source, "When?", "Tuesday.", &metadata()).unwrap();
        drop(db);
        let db = Database::new(database::open(&path).unwrap());
        let saved = snapshot(&db, "one").unwrap();
        assert_eq!(saved.summary.unwrap(), summary);
        assert_eq!(saved.messages, messages);
        let conn = db.lock().unwrap();
        let schema: String = conn.query_row("SELECT group_concat(sql) FROM sqlite_master WHERE name IN ('history_ai_chat','history_ai_summaries')", [], |row| row.get(0)).unwrap();
        assert!(!schema.contains("transcript_text"));
        let copies: i64 = conn.query_row(
            "SELECT (SELECT COUNT(*) FROM history_ai_summaries WHERE quick_summary=?1 OR key_takeaways_json=?1 OR metadata_json=?1)
                  + (SELECT COUNT(*) FROM history_ai_chat WHERE question=?1 OR answer=?1 OR metadata_json=?1)",
            [&source.text], |row| row.get(0),
        ).unwrap();
        assert_eq!(copies, 0);
    }

    #[test]
    fn integration_migration_17_has_mandatory_backup_atomic_rollback_and_validated_layout() {
        let root = TempRoot::new("video-ai-migration");
        let path = root.path("history.sqlite3");
        let conn = database::open(&path).unwrap();
        fixture(&conn);
        database::restore_schema16_video_ai_fixture(&conn);
        let error = database::initialize_with_hook(&conn, |point| {
            if point == MigrationPoint::VideoAi {
                Err("synthetic video AI failure".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(error.contains("synthetic video AI failure"));
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            16
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'history_ai_%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        let backups: Vec<_> = std::fs::read_dir(root.root())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".pre-schema-16-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        let backup = Connection::open(&backups[0]).unwrap();
        assert_eq!(
            backup
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            16
        );
        assert_eq!(
            backup
                .query_row(
                    "SELECT text FROM transcriptions WHERE history_entry_id='one'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "The first transcript states that the launch is on Tuesday. Engineers will inspect the capsule. Weather may delay the launch."
        );
        database::initialize(&conn).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            17
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM history_entries", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            3
        );
        conn.execute_batch("DROP TABLE history_ai_summaries")
            .unwrap();
        assert!(database::initialize(&conn)
            .unwrap_err()
            .contains("missing video AI tables"));
    }
}
