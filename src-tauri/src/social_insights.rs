//! Explicit Fetchary requests and atomic snapshots. Reads never initiate network work.
use crate::database::{self, Database};
use crate::process::{self, ProcessState};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

const MAX_BYTES: usize = 262144;
pub(crate) const METRICS: &[&str] = &[
    "views",
    "likes",
    "comments",
    "shares",
    "saves",
    "followers",
    "following",
    "posts",
    "subscribers",
];

#[derive(Default)]
pub(crate) struct SocialInsightsState {
    active: Mutex<HashSet<String>>,
}
struct RequestGuard<'a> {
    state: &'a SocialInsightsState,
    id: String,
}
impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.active.lock() {
            active.remove(&self.id);
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SocialInsightsSnapshot {
    pub(crate) history_entry_id: String,
    pub(crate) source_url: String,
    pub(crate) fetched_at: u64,
    pub(crate) fetchary_version: String,
    pub(crate) data: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    fetchary_version: String,
    data: Value,
}

pub(crate) struct SocialInsightsTools {
    pub(crate) node: PathBuf,
    pub(crate) script: PathBuf,
}

fn source_url(db: &Database, id: &str) -> Result<String, String> {
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    conn.query_row("SELECT url FROM history_entries WHERE id=?1", [id], |r| {
        r.get(0)
    })
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "This history entry no longer exists.".into())
}

pub(crate) fn get(db: &Database, id: &str) -> Result<Option<SocialInsightsSnapshot>, String> {
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let stored: Option<(String, u64, String, String)> = conn.query_row(
        "SELECT s.source_url,s.fetched_at,s.fetchary_version,s.payload_json FROM history_social_insights s JOIN history_entries h ON h.id=s.history_entry_id AND h.url=s.source_url WHERE s.history_entry_id=?1", [id],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|e| e.to_string())?;
    stored
        .map(|(source_url, fetched_at, fetchary_version, json)| {
            let data = serde_json::from_str(&json)
                .map_err(|_| "Saved Social Insights could not be read.")?;
            validate_data(&fetchary_version, &data)?;
            Ok(SocialInsightsSnapshot {
                history_entry_id: id.into(),
                source_url,
                fetched_at,
                fetchary_version,
                data,
            })
        })
        .transpose()
}

fn validate(reply: &Reply) -> Result<(), String> {
    validate_data(&reply.fetchary_version, &reply.data)
}

pub(crate) fn validate_data(version: &str, payload: &Value) -> Result<(), String> {
    let valid = || -> Option<()> {
        if version.is_empty() || version.len() > 64 {
            return None;
        }
        let data = payload.as_object()?;
        let status = data.get("status")?.as_str()?;
        if ![
            "available",
            "partial",
            "unavailable",
            "unsupported",
            "login_required",
            "blocked",
            "extraction_error",
        ]
        .contains(&status)
        {
            return None;
        }
        data.get("url")?.as_str()?;
        data.get("kind")?.as_str()?;
        let metrics = data.get("metrics")?.as_object()?;
        let score = metrics.get("score");
        if metrics.len() != METRICS.len() + usize::from(score.is_some()) {
            return None;
        }
        for name in METRICS {
            let value = metrics.get(*name)?;
            if !value.is_null()
                && value
                    .as_u64()
                    .is_none_or(|count| count > 9_007_199_254_740_991)
            {
                return None;
            }
        }
        // Fetchary 2.0.7 adds Reddit's signed score. Older snapshots do not
        // contain this field; ordinary metric counts remain nonnegative.
        if score.is_some_and(|value| {
            !value.is_null()
                && value
                    .as_i64()
                    .is_none_or(|score| score.unsigned_abs() > 9_007_199_254_740_991)
        }) {
            return None;
        }
        data.get("sources")?.as_object()?;
        data.get("unavailable")?.as_object()?;
        Some(())
    };
    valid().ok_or_else(|| {
        "Fetchary returned invalid Social Insights. Previous data was preserved.".into()
    })
}

pub(crate) fn save(
    db: &Database,
    snapshot: SocialInsightsSnapshot,
) -> Result<SocialInsightsSnapshot, String> {
    validate_data(&snapshot.fetchary_version, &snapshot.data)?;
    let json = serde_json::to_string(&snapshot.data).map_err(|e| e.to_string())?;
    if json.len() > MAX_BYTES {
        return Err("Social Insights exceed the storage limit.".into());
    }
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    let current: Option<String> = tx
        .query_row(
            "SELECT url FROM history_entries WHERE id=?1",
            [&snapshot.history_entry_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if current.as_deref() != Some(&snapshot.source_url) {
        return Err(
            "The history entry changed or no longer exists. Social Insights were not saved.".into(),
        );
    }
    tx.execute("INSERT INTO history_social_insights(history_entry_id,source_url,fetched_at,fetchary_version,payload_json)
        VALUES (?1,?2,?3,?4,?5) ON CONFLICT(history_entry_id) DO UPDATE SET
        source_url=excluded.source_url,fetched_at=excluded.fetched_at,fetchary_version=excluded.fetchary_version,payload_json=excluded.payload_json",
        params![snapshot.history_entry_id,snapshot.source_url,snapshot.fetched_at,snapshot.fetchary_version,json]).map_err(|e| format!("Social Insights could not be saved: {e}"))?;
    tx.commit()
        .map_err(|e| format!("Social Insights could not be committed: {e}"))?;
    Ok(snapshot)
}

pub(crate) fn fetch(
    db: &Database,
    state: &SocialInsightsState,
    processes: &ProcessState,
    id: &str,
    tools: &SocialInsightsTools,
) -> Result<SocialInsightsSnapshot, String> {
    {
        let mut active = state
            .active
            .lock()
            .map_err(|_| "Social Insights lock poisoned")?;
        if !active.insert(id.into()) {
            return Err("Social Insights are already loading for this video.".into());
        }
    }
    let _guard = RequestGuard {
        state,
        id: id.into(),
    };
    let url = source_url(db, id)?;
    let (version, data) = fetch_url(processes, &url, tools)?;
    save(
        db,
        SocialInsightsSnapshot {
            history_entry_id: id.into(),
            source_url: url,
            fetched_at: crate::files::current_timestamp_millis(),
            fetchary_version: version,
            data,
        },
    )
}

pub(crate) fn fetch_url(
    processes: &ProcessState,
    url: &str,
    tools: &SocialInsightsTools,
) -> Result<(String, Value), String> {
    let parsed = url::Url::parse(url).map_err(|_| "A public video URL is required.")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("A public video URL is required.".into());
    }
    let mut command = Command::new(&tools.node);
    command
        .arg(&tools.script)
        .arg(url)
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH");
    let output = process::run_command_output(
        command,
        None,
        Some(processes),
        Some(Duration::from_secs(90)),
    )
    .map_err(String::from)?;
    if !output.status.success() {
        if String::from_utf8_lossy(&output.stderr)
            .contains("The bundled Social Insights browser runtime is unavailable.")
        {
            return Err("The bundled Social Insights browser runtime is unavailable. Rebuild or reinstall PineFetch.".into());
        }
        return Err(
            "Social Insights could not be fetched. Check your connection and try again.".into(),
        );
    }
    if output.stdout.len() > MAX_BYTES {
        return Err("Fetchary returned oversized Social Insights.".into());
    }
    let reply: Reply = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Fetchary returned invalid Social Insights.")?;
    validate(&reply)?;
    match reply.data["status"].as_str() {
        Some("blocked") => {
            return Err("The platform blocked the Social Insights request. Try again later.".into())
        }
        Some("login_required") => {
            return Err("This platform requires sign-in to expose Social Insights.".into())
        }
        Some("extraction_error") => {
            return Err("The platform's Social Insights could not be extracted.".into())
        }
        _ => {}
    }
    Ok((reply.fetchary_version, reply.data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::MigrationPoint;
    use crate::test_support::TempRoot;
    use rusqlite::Connection;
    use std::fs;

    fn db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        database::initialize(&conn).unwrap();
        conn.execute("INSERT INTO history_entries(id,url,created_at) VALUES ('video','https://www.youtube.com/watch?v=AbC123',1)", []).unwrap();
        Database::new(conn)
    }
    fn snapshot(count: u64) -> SocialInsightsSnapshot {
        let mut metrics = serde_json::Map::new();
        for name in METRICS {
            metrics.insert((*name).into(), Value::Null);
        }
        metrics.insert("views".into(), count.into());
        metrics.insert("likes".into(), 0.into());
        SocialInsightsSnapshot {
            history_entry_id: "video".into(),
            source_url: "https://www.youtube.com/watch?v=AbC123".into(),
            fetched_at: count,
            fetchary_version: "2.0.4".into(),
            data: serde_json::json!({"platform":"youtube","kind":"video","url":"https://www.youtube.com/watch?v=AbC123","status":"partial","metrics":metrics,"sources":{},"unavailable":{"shares":"unsupported"}}),
        }
    }
    #[test]
    fn snapshots_are_explicit_atomic_and_cascade_without_altering_history_or_ocr() {
        let db = db();
        assert!(get(&db, "video").unwrap().is_none());
        assert!(get(&db, "other").unwrap().is_none());
        save(&db, snapshot(123)).unwrap();
        assert_eq!(
            get(&db, "video").unwrap().unwrap().data["metrics"]["likes"],
            0
        );
        db.lock().unwrap().execute_batch("CREATE TRIGGER reject_insights BEFORE UPDATE ON history_social_insights BEGIN SELECT RAISE(ABORT,'synthetic SQLite failure'); END;").unwrap();
        assert!(save(&db, snapshot(456)).is_err());
        assert_eq!(get(&db, "video").unwrap().unwrap().fetched_at, 123);
        db.lock()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_insights")
            .unwrap();
        save(&db, snapshot(789)).unwrap();
        assert_eq!(
            get(&db, "video").unwrap().unwrap().data["metrics"]["views"],
            789
        );
        db.lock()
            .unwrap()
            .execute(
                "UPDATE history_entries SET url='https://example.com/changed' WHERE id='video'",
                [],
            )
            .unwrap();
        assert!(save(&db, snapshot(999)).unwrap_err().contains("changed"));
        db.lock()
            .unwrap()
            .execute("DELETE FROM history_entries WHERE id='video'", [])
            .unwrap();
        assert!(get(&db, "video").unwrap().is_none());
        assert!(save(&db, snapshot(999)).is_err());
    }
    #[test]
    fn integration_saved_social_snapshot_survives_reopen_without_a_request() {
        let root = TempRoot::new("social-offline-reopen");
        let path = root.path("history.sqlite");
        let conn = database::open(&path).unwrap();
        conn.execute("INSERT INTO history_entries(id,url,created_at) VALUES ('video','https://www.youtube.com/watch?v=AbC123',1)", []).unwrap();
        let db = Database::new(conn);
        save(&db, snapshot(123)).unwrap();
        drop(db);
        let db = Database::new(database::open(&path).unwrap());
        let saved = get(&db, "video").unwrap().unwrap();
        assert_eq!(saved.fetched_at, 123);
        assert_eq!(saved.data["metrics"]["views"], 123);
        assert_eq!(saved.data["metrics"]["likes"], 0);
        assert!(saved.data["metrics"]["shares"].is_null());
        db.lock()
            .unwrap()
            .execute("UPDATE history_social_insights SET payload_json='{}'", [])
            .unwrap();
        assert!(get(&db, "video").is_err());
    }

    #[test]
    fn invalid_metrics_statuses_and_versions_are_rejected() {
        let valid = snapshot(1);
        let mut reply = Reply {
            fetchary_version: valid.fetchary_version,
            data: valid.data,
        };
        validate(&reply).unwrap();
        for value in [
            serde_json::json!(-1),
            serde_json::json!(1.25),
            serde_json::json!("123"),
            serde_json::json!(9_007_199_254_740_992_u64),
        ] {
            reply.data["metrics"]["views"] = value;
            assert!(validate(&reply).is_err());
        }
        reply.data["metrics"]["views"] = Value::Null;
        validate(&reply).unwrap();
        reply.data["status"] = "invented".into();
        assert!(validate(&reply).is_err());
    }
    #[test]
    fn optional_fetchary_score_round_trips_while_legacy_and_count_rules_remain_valid() {
        let db = db();
        validate_data("2.0.4", &snapshot(1).data).unwrap();
        for score in [
            Value::Null,
            0.into(),
            (-3).into(),
            9_007_199_254_740_991_i64.into(),
            (-9_007_199_254_740_991_i64).into(),
        ] {
            let mut value = snapshot(123);
            value.fetchary_version = "2.0.7".into();
            value.data["metrics"]["score"] = score.clone();
            save(&db, value).unwrap();
            assert_eq!(
                get(&db, "video").unwrap().unwrap().data["metrics"]["score"],
                score
            );
        }
        for score in [
            serde_json::json!(1.25),
            serde_json::json!("3"),
            serde_json::json!(9_007_199_254_740_992_i64),
            serde_json::json!(-9_007_199_254_740_992_i64),
        ] {
            let mut value = snapshot(456);
            value.data["metrics"]["score"] = score;
            assert!(save(&db, value).is_err());
            assert_eq!(get(&db, "video").unwrap().unwrap().fetched_at, 123);
        }
        let mut invalid = snapshot(456);
        invalid.data["metrics"]["unknown"] = Value::Null;
        assert!(save(&db, invalid).is_err());
        let mut invalid = snapshot(456);
        invalid.data["metrics"]["score"] = 0.into();
        invalid.data["metrics"]["likes"] = (-1).into();
        assert!(save(&db, invalid).is_err());
        let mut invalid = snapshot(456);
        invalid.data["metrics"]
            .as_object_mut()
            .unwrap()
            .remove("views");
        invalid.data["metrics"]["score"] = 0.into();
        assert!(save(&db, invalid).is_err());
    }
    #[test]
    fn integration_schema_four_upgrade_backs_up_preserves_results_and_rolls_back_on_failure() {
        let root = TempRoot::new("social-migration");
        let path = root.path("history.sqlite");
        let conn = database::open(&path).unwrap();
        conn.execute_batch("DROP TABLE history_account_insights; DROP INDEX idx_history_account_lookup; DROP TABLE history_social_insights; ALTER TABLE app_config DROP COLUMN youtube_cookie_browser; PRAGMA user_version=4; INSERT INTO history_entries(id,url,title,created_at) VALUES ('video','https://example.com/video','Original title',123);").unwrap();
        drop(conn);
        let before = fs::read(&path).unwrap();
        assert!(database::open_with_hook(&path, |point| {
            if point == MigrationPoint::SocialInsights {
                Err("synthetic migration interruption".into())
            } else {
                Ok(())
            }
        })
        .is_err());
        assert_eq!(before, fs::read(&path).unwrap());
        let conn = database::open(&path).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            database::SCHEMA_VERSION
        );
        assert_eq!(
            conn.query_row("SELECT title FROM history_entries", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "Original title"
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM history_social_insights", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(conn);
        database::open(&path).unwrap();
        assert!(fs::read_dir(root.root())
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().contains("pre-schema-4")));
    }
    #[cfg(unix)]
    #[test]
    fn integration_explicit_fetch_uses_the_library_adapter_and_failed_requests_keep_saved_data() {
        use std::os::unix::fs::PermissionsExt;
        let root = TempRoot::new("social-process");
        let node = root.path("node");
        let db = db();
        let state = SocialInsightsState::default();
        let tools = SocialInsightsTools {
            node: node.clone(),
            script: root.path("adapter.cjs"),
        };
        let mut payload = snapshot(123);
        payload.fetchary_version = "2.0.7".into();
        payload.data["metrics"]["score"] = Value::Null;
        let reply =
            serde_json::json!({"fetchary_version":payload.fetchary_version,"data":payload.data});
        fs::write(&node, format!("#!/bin/sh\nprintf '%s' '{}'", reply)).unwrap();
        fs::set_permissions(&node, fs::Permissions::from_mode(0o700)).unwrap();
        let saved = fetch(&db, &state, &ProcessState::default(), "video", &tools).unwrap();
        assert_eq!(saved.data["metrics"]["views"], 123);
        state.active.lock().unwrap().insert("video".into());
        assert!(
            fetch(&db, &state, &ProcessState::default(), "video", &tools)
                .unwrap_err()
                .contains("already loading")
        );
        state.active.lock().unwrap().clear();
        fs::write(&node, "#!/bin/sh\nprintf '%s' 'The bundled Social Insights browser runtime is unavailable. Rebuild or reinstall PineFetch.' >&2\nexit 1\n").unwrap();
        assert!(
            fetch(&db, &state, &ProcessState::default(), "video", &tools)
                .unwrap_err()
                .contains("Rebuild or reinstall")
        );
        assert_eq!(
            get(&db, "video").unwrap().unwrap().fetched_at,
            saved.fetched_at
        );
        for body in [
            "exit 1",
            "printf 'not json'",
            "printf '{}'",
            "printf '%0262145d' 0",
        ] {
            fs::write(&node, format!("#!/bin/sh\n{body}")).unwrap();
            assert!(fetch(&db, &state, &ProcessState::default(), "video", &tools).is_err());
            assert!(state.active.lock().unwrap().is_empty());
            assert_eq!(
                get(&db, "video").unwrap().unwrap().fetched_at,
                saved.fetched_at
            );
        }
    }
}
