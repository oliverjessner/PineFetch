//! Creator membership never derives from mutable display names or handles.
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

pub(crate) const SCHEMA_SQL: &str = "
ALTER TABLE history_entries ADD COLUMN creator_source TEXT;
ALTER TABLE history_entries ADD COLUMN creator_id TEXT;
CREATE INDEX idx_history_identity_order ON history_entries(creator_source,creator_id,COALESCE(completed_at,created_at) DESC,created_at DESC,id DESC);
CREATE TABLE creator_account_insights (
 source TEXT NOT NULL,
 creator_id TEXT NOT NULL,
 profile_url TEXT NOT NULL,
 fetched_at INTEGER NOT NULL CHECK(fetched_at>=0),
 fetchary_version TEXT NOT NULL,
 payload_json TEXT NOT NULL CHECK(length(payload_json)<=262144),
 PRIMARY KEY(source,creator_id)
);
";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Metadata {
    pub(crate) channel_id: Option<String>,
    pub(crate) uploader_id: Option<String>,
}
impl Metadata {
    pub(crate) fn parse(data: &serde_json::Value) -> Self {
        let string = |key: &str| match &data[key] {
            serde_json::Value::String(value) => Some(value.clone()),
            serde_json::Value::Number(value) => Some(value.to_string()),
            _ => None,
        };
        Self {
            channel_id: string("channel_id"),
            uploader_id: string("uploader_id"),
        }
    }
    pub(crate) fn identity(&self, platform: &str) -> Option<String> {
        let source = canonical_source(platform);
        if source == "youtube" {
            return self
                .channel_id
                .as_ref()
                .filter(|id| {
                    id.starts_with("UC")
                        && id.len() == 24
                        && id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                })
                .map(|id| format!("channel:{id}"));
        }
        if !matches!(
            source.as_str(),
            "tiktok" | "instagram" | "x" | "facebook" | "twitch" | "vimeo"
        ) {
            return None;
        }
        // Some extractors expose handles in these fields. Accept only the
        // numeric user ID; missing/ambiguous metadata stays entry-scoped.
        let numeric = |id: &&String| {
            !id.is_empty()
                && id.len() <= 32
                && id.bytes().all(|b| b.is_ascii_digit())
                && id.bytes().any(|b| b != b'0')
        };
        let id = if source == "x" {
            self.channel_id
                .as_ref()
                .filter(numeric)
                .or_else(|| self.uploader_id.as_ref().filter(numeric))
        } else {
            self.uploader_id.as_ref().filter(numeric)
        }?;
        Some(format!("user:{}", id.trim_start_matches('0')))
    }
}
pub(crate) fn canonical_source(source: &str) -> String {
    match source.trim().to_ascii_lowercase().as_str() {
        "twitter" => "x".into(),
        source => source.into(),
    }
}
pub(crate) fn read(
    conn: &rusqlite::Connection,
    id: &str,
    source: &str,
) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT creator_id FROM history_entries WHERE id=?1 AND creator_source=?2",
        params![id, canonical_source(source)],
        |r| r.get(0),
    )
    .optional()
    .map(Option::flatten)
    .map_err(|e| e.to_string())
}
pub(crate) fn store(
    conn: &rusqlite::Connection,
    id: &str,
    source: &str,
    identity: &str,
) -> Result<(), String> {
    let changed = conn.execute("UPDATE history_entries SET creator_source=?1,creator_id=?2 WHERE id=?3 AND (creator_id IS NULL OR (creator_source=?1 AND creator_id=?2))", params![canonical_source(source), identity, id]).map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("The creator identity changed. Previous account data was preserved.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{self, Database};
    use crate::test_support::TempRoot;
    fn db() -> Database {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        database::initialize(&conn).unwrap();
        conn.execute_batch("INSERT INTO history_entries(id,url,source,uploader,created_at) VALUES('one','https://youtube.com/watch?v=abcdefghijk','youtube','Same',1),('two','https://youtube.com/watch?v=lmnopqrstuv','youtube','Renamed',2),('three','https://youtube.com/watch?v=xyzabcdefgh','youtube','Same',3),('legacy','https://youtube.com/watch?v=defghijklmn','youtube','Same',4),('platform','https://tiktok.com/@same/video/1234567890','tiktok','Same',5);").unwrap();
        store(&conn, "one", "youtube", "channel:UCaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        store(&conn, "two", "youtube", "channel:UCaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        store(
            &conn,
            "three",
            "youtube",
            "channel:UCbbbbbbbbbbbbbbbbbbbbbb",
        )
        .unwrap();
        store(
            &conn,
            "platform",
            "tiktok",
            "channel:UCaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap();
        Database::new(conn)
    }
    #[test]
    fn membership_uses_platform_ids_across_renames_and_isolates_missing_ids() {
        let db = db();
        let page = serde_json::to_value(crate::accounts::get(&db, "one", 0).unwrap()).unwrap();
        assert_eq!(page["identity_verified"], true);
        assert_eq!(page["saved"]["total_count"], 2);
        let saved = crate::history_query::read_page(
            &db,
            20,
            0,
            crate::accounts::history_filter(&db, "one").unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(saved.total_count, 2);
        let page = serde_json::to_value(crate::accounts::get(&db, "three", 0).unwrap()).unwrap();
        assert_eq!(page["saved"]["total_count"], 1);
        let page = serde_json::to_value(crate::accounts::get(&db, "legacy", 0).unwrap()).unwrap();
        assert_eq!(page["identity_verified"], false);
        assert_eq!(page["saved"]["total_count"], 1);
        assert!(store(&db.lock().unwrap(), "one", "youtube", "channel:different").is_err());
    }
    #[test]
    fn handles_and_ambiguous_metadata_never_become_stable_ids() {
        let metadata = Metadata::parse(
            &serde_json::json!({"uploader_id":"mutable-handle","channel_id":"UCaaaaaaaaaaaaaaaaaaaaaa"}),
        );
        assert_eq!(
            metadata.identity("youtube").as_deref(),
            Some("channel:UCaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert!(metadata.identity("tiktok").is_none());
        let metadata = Metadata::parse(&serde_json::json!({"uploader_id":12345}));
        assert_eq!(
            metadata.identity("instagram").as_deref(),
            Some("user:12345")
        );
        assert_eq!(metadata.identity("twitter"), metadata.identity("x"));
        assert!(Metadata::parse(&serde_json::json!({"uploader_id":"000"}))
            .identity("instagram")
            .is_none());
    }
    #[test]
    fn deletion_only_checks_target_keys_and_keeps_shared_insights_until_last_member() {
        let db = db();
        db.lock().unwrap().execute_batch("INSERT INTO creator_account_insights VALUES('youtube','channel:UCaaaaaaaaaaaaaaaaaaaaaa','https://youtube.com/@same',1,'2.0.4','{}'),('youtube','orphan','https://youtube.com/@orphan',1,'2.0.4','{}'); INSERT INTO history_account_insights VALUES('youtube','Same','https://youtube.com/@same',1,'2.0.4','{}'),('youtube','Unrelated','https://youtube.com/@other',1,'2.0.4','{}');").unwrap();
        crate::history::delete_history_entry_from_db(&db, "one").unwrap();
        assert_eq!(
            db.lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM creator_account_insights", [], |r| r
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            2
        );
        crate::history::delete_history_entry_from_db(&db, "two").unwrap();
        assert_eq!(
            db.lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM creator_account_insights", [], |r| r
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            1
        );
        assert_eq!(
            db.lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM history_account_insights", [], |r| r
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            2
        );
        db.lock().unwrap().execute_batch("CREATE TRIGGER cleanup_failure BEFORE DELETE ON history_account_insights BEGIN SELECT RAISE(ABORT,'synthetic cleanup error'); END;").unwrap();
        crate::history::delete_history_entry_from_db(&db, "three").unwrap();
        assert!(crate::history::delete_history_entry_from_db(&db, "legacy").is_err());
        assert!(crate::accounts::get(&db, "legacy", 0).is_ok());
        db.lock()
            .unwrap()
            .execute_batch("DROP TRIGGER cleanup_failure;")
            .unwrap();
        crate::history::delete_history_entry_from_db(&db, "legacy").unwrap();
        assert_eq!(
            db.lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM history_account_insights", [], |r| r
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            1
        );
        crate::history::clear_history_entries_in_db(&db).unwrap();
        assert_eq!(
            db.lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM creator_account_insights", [], |r| r
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            0
        );
    }
    #[test]
    fn current_schema_rejects_an_identity_index_with_the_wrong_order() {
        let root = TempRoot::new("creator-index-validation");
        let path = root.path("archive.sqlite");
        let conn = database::open(&path).unwrap();
        conn.execute_batch("DROP INDEX idx_history_identity_order; CREATE INDEX idx_history_identity_order ON history_entries(creator_source,creator_id,created_at ASC,id ASC);").unwrap();
        drop(conn);
        let before = std::fs::read(&path).unwrap();
        assert!(database::open(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn integration_schema14_preserves_legacy_data_backs_up_rolls_back_and_reopens() {
        let root = TempRoot::new("creator-schema14");
        let path = root.path("archive.sqlite");
        let conn = database::open(&path).unwrap();
        database::restore_schema13_creator_fixture(&conn);
        conn.execute_batch("INSERT INTO history_entries(id,url,source,uploader,created_at) VALUES('old','https://youtube.com/watch?v=abcdefghijk','youtube','Same',1); INSERT INTO history_account_insights VALUES('youtube','Same','https://youtube.com/@same',1,'2.0.4','{}');").unwrap();
        drop(conn);
        let before = std::fs::read(&path).unwrap();
        assert!(database::open_with_hook(&path, |point| {
            if point == database::MigrationPoint::CreatorIdentities {
                Err("synthetic identity migration failure".into())
            } else {
                Ok(())
            }
        })
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let conn = database::open(&path).unwrap();
        assert!(read(&conn, "old", "youtube").unwrap().is_none());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM history_account_insights", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            1
        );
        store(&conn, "old", "youtube", "channel:UCaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        drop(conn);
        let conn = database::open(&path).unwrap();
        assert!(read(&conn, "old", "youtube").unwrap().is_some());
        assert!(std::fs::read_dir(root.root())
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains("pre-schema-13")));
    }
}
