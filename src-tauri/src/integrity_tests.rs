//! All fixtures are synthetic, file-backed and confined to a fresh temp root.
use super::*;
use database::{MigrationPoint, SCHEMA_VERSION};
use rusqlite::{OpenFlags, Transaction, TransactionBehavior};
use std::sync::Barrier;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("pinefetch-integrity-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join(".synthetic-fixture"), b"no user data").unwrap();
        Self(root)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn db(&self) -> Connection {
        database::open(&self.path("pinefetch.sqlite3")).unwrap()
    }
    fn backups(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".pre-schema-")
                    && p.extension().is_some_and(|e| e == "sqlite3")
            })
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn version(conn: &Connection) -> i64 {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}
fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn schema(conn: &Connection) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare("SELECT name,sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn entry(id: &str) -> HistoryEntry {
    serde_json::from_value(
        json!({"id":id,"url":"https://www.youtube.com/watch?v=synthetic","title":"Grüße 🌲",
        "created_at":1700000000000_u64,"completed_at":1700000000100_u64}),
    )
    .unwrap()
}

fn job(id: &str, root: &Path, transcript: bool) -> DownloadJob {
    DownloadJob {
        id: id.into(),
        url: entry(id).url,
        format: "best".into(),
        output_dir: root.to_string_lossy().into_owned(),
        extract_audio: false,
        audio_format: None,
        transcribe_text: transcript,
        transcribe_timestamps: transcript,
        faster_whisper_model: "base".into(),
        download_video_with_transcript: false,
        save_captions: false,
        save_thumbnails: false,
        title: Some("Grüße 🌲".into()),
        uploader: None,
        thumbnail: None,
        upload_date: None,
        timestamp: Some(1714560000),
        duration_seconds: Some(12),
        cut_start_time: None,
        filename_suffix: None,
    }
}

fn app_state(conn: Connection) -> AppState {
    AppState::new(load_config_from_db(&conn).unwrap(), conn)
}

const RELEASE_SCHEMAS: &[(&str, &str)] = &[
    ("v1.4.3", include_str!("../test-fixtures/schema/v1.4.3.sql")),
    ("v1.4.5", include_str!("../test-fixtures/schema/v1.4.5.sql")),
    ("v1.5.0", include_str!("../test-fixtures/schema/v1.5.0.sql")),
    ("v1.6.0", include_str!("../test-fixtures/schema/v1.6.0.sql")),
    ("v1.7.2", include_str!("../test-fixtures/schema/v1.7.2.sql")),
    ("v1.8.0", include_str!("../test-fixtures/schema/v1.8.0.sql")),
    ("v1.9.1", include_str!("../test-fixtures/schema/v1.9.1.sql")),
    ("v2.1.0", include_str!("../test-fixtures/schema/v2.1.0.sql")),
    ("v2.2.0", include_str!("../test-fixtures/schema/v2.2.0.sql")),
];

// Schema-only reproduction of the reported upgraded database. All rows below
// are synthetic; this is not a fixture copied from a user's database.
fn reported_upgraded_layout(conn: &Connection) {
    let sql = RELEASE_SCHEMAS.last().unwrap().1.replace(
        "pinefetch_version TEXT,",
        "pinefetch_version TEXT NOT NULL DEFAULT '2.1.0',",
    );
    conn.execute_batch(&sql).unwrap();
    conn.execute_batch(
        "ALTER TABLE app_config ADD COLUMN save_instagram_captions INTEGER NOT NULL DEFAULT 0;
         UPDATE app_config SET save_instagram_captions=1,save_captions=0;",
    )
    .unwrap();
    populate_legacy(conn);
}

#[test]
fn reported_upgraded_columns_preserve_settings_history_constraints_and_reopen() {
    let f = Fixture::new();
    let path = f.path("pinefetch.sqlite3");
    let conn = Connection::open(&path).unwrap();
    reported_upgraded_layout(&conn);
    let before = schema(&conn);
    drop(conn);

    let conn = database::open(&path).unwrap();
    assert_eq!(version(&conn), SCHEMA_VERSION);
    assert_eq!(count(&conn, "history_entries"), 2);
    assert!(!load_config_from_db(&conn).unwrap().save_captions);
    assert_eq!(
        conn.query_row("SELECT save_instagram_captions FROM app_config", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT pinefetch_version FROM history_entries WHERE id='old-1'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "2.1.0"
    );
    let backup =
        Connection::open_with_flags(&f.backups()[0], OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(version(&backup), 0);
    assert_eq!(schema(&backup), before);
    assert_eq!(count(&backup, "history_entries"), 2);
    assert_eq!(count(&conn, "transcriptions"), 1);
    assert_eq!(count(&conn, "captions"), 1);
    drop(backup);

    // Older JSON entries have no app version. Retain this layout's own
    // historical default while preserving NULL in normal nullable layouts.
    insert_history_entry_in_conn(&conn, &entry("imported")).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT pinefetch_version FROM history_entries WHERE id='imported'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "2.1.0"
    );
    let mut explicit = entry("explicit");
    explicit.pinefetch_version = Some("1.4.3".into());
    insert_history_entry_in_conn(&conn, &explicit).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT pinefetch_version FROM history_entries WHERE id='explicit'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "1.4.3"
    );
    let migrated_schema = schema(&conn);
    drop(conn);
    let bytes = fs::read(&path).unwrap();
    let reopened = database::open(&path).unwrap();
    assert_eq!(schema(&reopened), migrated_schema);
    assert_eq!(count(&reopened, "history_entries"), 4);
    drop(reopened);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(f.backups().len(), 1);
}

#[test]
fn reported_upgrade_failure_rolls_back_and_retry_preserves_original_rows() {
    let f = Fixture::new();
    let path = f.path("pinefetch.sqlite3");
    let conn = Connection::open(&path).unwrap();
    reported_upgraded_layout(&conn);
    let before = schema(&conn);
    drop(conn);
    let bytes = fs::read(&path).unwrap();
    let error = database::open_with_hook(&path, |point| {
        if point == MigrationPoint::Receipts {
            Err("synthetic interrupted upgrade".into())
        } else {
            Ok(())
        }
    })
    .err()
    .unwrap();
    assert!(error.contains("synthetic interrupted upgrade"));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let original = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(schema(&original), before);
    assert_eq!(version(&original), 0);
    assert_eq!(count(&original, "history_entries"), 2);
    drop(original);
    let reopened = database::open(&path).unwrap();
    assert_eq!(version(&reopened), SCHEMA_VERSION);
    assert_eq!(count(&reopened, "transcriptions"), 1);
    assert_eq!(count(&reopened, "captions"), 1);
    assert_eq!(count(&reopened, "history_entries"), 2);
}

#[test]
fn reported_column_compatibility_still_rejects_unrelated_definitions() {
    for (history_definition, caption_definition) in [
        (
            "TEXT NOT NULL DEFAULT 'other'",
            "INTEGER NOT NULL DEFAULT 0",
        ),
        ("TEXT DEFAULT '2.1.0'", "INTEGER NOT NULL DEFAULT 0"),
        ("TEXT NOT NULL DEFAULT '2.1.0'", "TEXT NOT NULL DEFAULT 0"),
        (
            "TEXT NOT NULL DEFAULT '2.1.0'",
            "INTEGER NOT NULL DEFAULT 1",
        ),
    ] {
        let f = Fixture::new();
        let path = f.path("pinefetch.sqlite3");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(&RELEASE_SCHEMAS.last().unwrap().1.replace(
            "pinefetch_version TEXT,",
            &format!("pinefetch_version {history_definition},"),
        ))
        .unwrap();
        conn.execute_batch(&format!(
            "ALTER TABLE app_config ADD COLUMN save_instagram_captions {caption_definition};"
        ))
        .unwrap();
        let before = schema(&conn);
        drop(conn);
        let bytes = fs::read(&path).unwrap();
        assert!(database::open(&path)
            .unwrap_err()
            .contains("database preserved"));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let reopened =
            Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(schema(&reopened), before);
        assert_eq!(version(&reopened), 0);
        assert!(f.backups().is_empty());
    }
}

#[test]
fn persistence_startup_imports_before_managing_state_and_reopens_idempotently() {
    let f = Fixture::new();
    let data_dir = f.path("data");
    let config_dir = f.path("config");
    fs::create_dir(&data_dir).unwrap();
    fs::create_dir(&config_dir).unwrap();
    let config_path = config_dir.join("config.json");
    let history_path = data_dir.join("history.json");
    let config = json!({"yt_dlp_path":null,"default_output_dir":"/synthetic/output",
        "save_captions":true,"selected_preset_key":"audio_mp3"})
    .to_string();
    let history = serde_json::to_string(&vec![entry("startup-entry")]).unwrap();
    fs::write(&config_path, &config).unwrap();
    fs::write(&history_path, &history).unwrap();
    let state = startup::load_from_dirs(&data_dir, &config_dir).unwrap();
    assert_eq!(
        state.config.lock().unwrap().default_output_dir.as_deref(),
        Some("/synthetic/output")
    );
    assert!(state.config.lock().unwrap().save_captions);
    assert_eq!(count(&state.db.lock().unwrap(), "history_entries"), 1);
    assert_eq!(fs::read_to_string(&config_path).unwrap(), config);
    assert_eq!(fs::read_to_string(&history_path).unwrap(), history);
    // Receipts prevent re-reading obsolete files, including malformed ones.
    drop(state);
    let bytes = fs::read(data_dir.join("pinefetch.sqlite")).unwrap();
    fs::write(&config_path, "obsolete invalid config").unwrap();
    fs::write(&history_path, "obsolete invalid history").unwrap();
    let state = startup::load_from_dirs(&data_dir, &config_dir).unwrap();
    assert_eq!(count(&state.db.lock().unwrap(), "history_entries"), 1);
    drop(state);
    assert_eq!(fs::read(data_dir.join("pinefetch.sqlite")).unwrap(), bytes);
}

#[test]
fn persistence_startup_returns_errors_and_preserves_refused_inputs() {
    let f = Fixture::new();
    let config_dir = f.path("config");
    fs::create_dir(&config_dir).unwrap();
    fs::write(config_dir.join("config.json"), "synthetic malformed JSON").unwrap();
    let conn = f.db();
    conn.pragma_update(None, "user_version", 999).unwrap();
    drop(conn);
    fs::rename(f.path("pinefetch.sqlite3"), f.path("pinefetch.sqlite")).unwrap();
    let before = fs::read(f.path("pinefetch.sqlite")).unwrap();
    let error = startup::load_from_dirs(&f.0, &config_dir).err().unwrap();
    assert!(error.contains("999"));
    assert_eq!(fs::read(f.path("pinefetch.sqlite")).unwrap(), before);
    assert_eq!(
        fs::read_to_string(config_dir.join("config.json")).unwrap(),
        "synthetic malformed JSON"
    );
    assert!(f.backups().is_empty());

    let fresh_data = f.path("fresh");
    let error = startup::load_from_dirs(&fresh_data, &config_dir)
        .err()
        .unwrap();
    assert!(error.contains("Legacy JSON is invalid"));
    let conn = Connection::open_with_flags(
        fresh_data.join("pinefetch.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT legacy_config_json_migrated FROM app_config",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(count(&conn, "history_entries"), 0);
}

fn populate_legacy(conn: &Connection) {
    conn.execute("INSERT INTO history_entries(id,url,title,created_at,completed_at) VALUES ('old-1','https://www.youtube.com/watch?v=synthetic','Grüße 🌲',1700000000000, NULL)", []).unwrap();
    conn.execute("INSERT INTO history_entries(id,url,created_at,completed_at) VALUES ('old-2','https://www.youtube.com/watch?v=synthetic',1700000000200,1700000000300)", []).unwrap();
    conn.execute("INSERT INTO link_dump_secrets(id,name,secret_hash,created_at,last_used_at,revoked_at,deleted_at) VALUES ('secret-id','Synthetic',?1,'2020-01-01',NULL,'2020-01-02',NULL)", [sha256_hex(b"synthetic test secret")]).unwrap();
    let tables: HashSet<_> = schema(conn).into_iter().map(|(name, _)| name).collect();
    if tables.contains("app_config") {
        conn.execute("UPDATE app_config SET selected_preset_key='audio_mp3',default_output_dir='/synthetic/Grüße 🌲',last_download_url='https://example.com/newer' WHERE id=1", []).unwrap();
    }
    if tables.contains("app_meta") {
        conn.execute(
            "INSERT INTO app_meta(key,value) VALUES ('legacy_config_json_migrated','1')",
            [],
        )
        .unwrap();
    }
    if tables.contains("transcriptions") {
        conn.execute("INSERT INTO transcriptions(id,history_entry_id,text,\"type\") VALUES ('transcript-id','old-1','Grüße\nSecond line','text with timestamps')", []).unwrap();
    }
    if tables.contains("captions") {
        conn.execute("INSERT INTO captions(history_entry_id,media_path,caption_path,text,created_at) VALUES ('old-1','/synthetic/Grüße 🌲.mp4','/synthetic/Grüße 🌲.caption.txt','Caption\n🌲',1700000000400)", []).unwrap();
    }
}

#[test]
fn fresh_file_and_current_reopen_are_idempotent_without_backup_or_mutation() {
    let f = Fixture::new();
    let conn = f.db();
    assert_eq!(version(&conn), SCHEMA_VERSION);
    assert_eq!(count(&conn, "app_config"), 1);
    assert_eq!(count(&conn, "history_entries"), 0);
    assert_eq!(
        conn.pragma_query_value::<i64, _>(None, "foreign_keys", |r| r.get(0))
            .unwrap(),
        1
    );
    let original_schema = schema(&conn);
    conn.execute(
        "UPDATE app_config SET updated_at='synthetic unchanged' WHERE id=1",
        [],
    )
    .unwrap();
    drop(conn);
    let bytes = fs::read(f.path("pinefetch.sqlite3")).unwrap();
    let reopened = f.db();
    assert_eq!(schema(&reopened), original_schema);
    assert_eq!(
        reopened
            .query_row("SELECT updated_at FROM app_config", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "synthetic unchanged"
    );
    drop(reopened);
    assert_eq!(fs::read(f.path("pinefetch.sqlite3")).unwrap(), bytes);
    assert!(f.backups().is_empty());
}

#[test]
fn all_reconstructed_release_layouts_preserve_values_ids_nulls_and_relationships() {
    for (release, sql) in RELEASE_SCHEMAS {
        let f = Fixture::new();
        let conn = Connection::open(f.path("pinefetch.sqlite3")).unwrap();
        conn.execute_batch(sql).unwrap();
        populate_legacy(&conn);
        let old_schema = schema(&conn);
        let had_config = old_schema.iter().any(|(name, _)| name == "app_config");
        let had_transcript = old_schema.iter().any(|(name, _)| name == "transcriptions");
        let had_caption = old_schema.iter().any(|(name, _)| name == "captions");
        if *release == "v1.9.1" {
            conn.execute("UPDATE app_config SET save_instagram_captions=1", [])
                .unwrap();
        }
        database::initialize(&conn).unwrap_or_else(|e| panic!("{release}: {e}"));
        assert_eq!(version(&conn), SCHEMA_VERSION);
        assert_eq!(count(&conn, "history_entries"), 2);
        let row: (String, i64, Option<i64>, Option<String>, Option<String>) = conn.query_row(
            "SELECT title,created_at,completed_at,source,pinefetch_version FROM history_entries WHERE id='old-1'", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap();
        assert_eq!(
            row,
            (
                "Grüße 🌲".into(),
                1700000000000,
                None,
                Some("youtube".into()),
                None
            )
        );
        assert_eq!(
            conn.query_row(
                "SELECT secret_hash FROM link_dump_secrets WHERE id='secret-id'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            sha256_hex(b"synthetic test secret")
        );
        if had_config {
            let config = load_config_from_db(&conn).unwrap();
            assert_eq!(config.selected_preset_key.as_deref(), Some("audio_mp3"));
            assert_eq!(
                config.default_output_dir.as_deref(),
                Some("/synthetic/Grüße 🌲")
            );
            assert_eq!(
                config.last_download_url.as_deref(),
                Some("https://example.com/newer")
            );
        }
        if *release == "v1.9.1" {
            assert!(load_config_from_db(&conn).unwrap().save_captions);
        }
        if had_transcript {
            assert_eq!(
                conn.query_row(
                    "SELECT id,text,language FROM transcriptions WHERE history_entry_id='old-1'",
                    [],
                    |r| Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?
                    ))
                )
                .unwrap(),
                ("transcript-id".into(), "Grüße\nSecond line".into(), None)
            );
        }
        if had_caption {
            assert_eq!(
                conn.query_row(
                    "SELECT text,created_at,sha256 FROM captions WHERE history_entry_id='old-1'",
                    [],
                    |r| Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Option<String>>(2)?
                    ))
                )
                .unwrap(),
                ("Caption\n🌲".into(), 1700000000400, None)
            );
        }
        let backup =
            Connection::open_with_flags(&f.backups()[0], OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(version(&backup), 0);
        assert_eq!(schema(&backup), old_schema);
        assert_eq!(count(&backup, "history_entries"), 2);
        assert_eq!(
            backup
                .query_row("SELECT secret_hash FROM link_dump_secrets", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            sha256_hex(b"synthetic test secret")
        );
        database::initialize(&conn).unwrap();
        assert_eq!(f.backups().len(), 1);
    }
}

#[test]
fn version_one_upgrade_and_sql_failure_roll_back_entire_section_and_retry() {
    let f = Fixture::new();
    let conn = Connection::open(f.path("pinefetch.sqlite3")).unwrap();
    conn.execute_batch(RELEASE_SCHEMAS.last().unwrap().1)
        .unwrap();
    populate_legacy(&conn);
    let before = schema(&conn);
    let error = database::initialize_with_hook(&conn, |point| {
        if point == MigrationPoint::Adopted {
            conn.execute_batch("CREATE TABLE deliberately_partial(value TEXT); INSERT INTO no_such_table VALUES (1);")
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }).unwrap_err();
    assert!(error.contains("no_such_table"));
    assert_eq!(version(&conn), 0);
    assert_eq!(schema(&conn), before);
    assert_eq!(count(&conn, "history_entries"), 2);
    database::initialize(&conn).unwrap();
    assert_eq!(version(&conn), SCHEMA_VERSION);
    // Construct the version-1 baseline using the implemented adoption step,
    // then fail version 2. Version 1 is an internal baseline, not an old app.
    conn.execute_batch(
        "DROP TABLE job_completions; DROP TABLE legacy_imports; PRAGMA user_version=1;",
    )
    .unwrap();
    let before = schema(&conn);
    database::initialize_with_hook(&conn, |point| {
        if point == MigrationPoint::Receipts {
            return Err("controlled failure after migration 2".into());
        }
        Ok(())
    })
    .unwrap_err();
    assert_eq!(version(&conn), 1);
    assert_eq!(schema(&conn), before);
    database::initialize(&conn).unwrap();
    assert_eq!(version(&conn), SCHEMA_VERSION);
}

#[test]
fn future_corrupt_unknown_and_conflicting_layouts_are_preserved() {
    let f = Fixture::new();
    let conn = f.db();
    conn.execute(
        "INSERT INTO history_entries(id,url,created_at) VALUES ('keep','https://example.com',1)",
        [],
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 999).unwrap();
    drop(conn);
    let bytes = fs::read(f.path("pinefetch.sqlite3")).unwrap();
    let error = database::open(&f.path("pinefetch.sqlite3")).unwrap_err();
    assert!(error.contains("999") && error.contains("2"));
    assert_eq!(bytes, fs::read(f.path("pinefetch.sqlite3")).unwrap());
    assert!(f.backups().is_empty());
    let conn = Connection::open(f.path("unknown.sqlite3")).unwrap();
    conn.execute_batch("CREATE TABLE history_entries(id TEXT PRIMARY KEY,url TEXT NOT NULL,created_at INTEGER NOT NULL,completed_at INTEGER,alien TEXT); INSERT INTO history_entries VALUES ('keep','https://example.com',1,NULL,'retain');").unwrap();
    let before = schema(&conn);
    assert!(database::initialize(&conn)
        .unwrap_err()
        .contains("preserved"));
    assert_eq!(schema(&conn), before);
    assert_eq!(version(&conn), 0);
    let conn = Connection::open(f.path("conflict.sqlite3")).unwrap();
    conn.execute_batch(RELEASE_SCHEMAS.last().unwrap().1)
        .unwrap();
    conn.execute(
        "INSERT INTO history_entries(id,url,created_at) VALUES (NULL,'https://example.com',1)",
        [],
    )
    .unwrap();
    assert!(database::initialize(&conn)
        .unwrap_err()
        .contains("null identity"));
    assert_eq!(count(&conn, "history_entries"), 1);
    let corrupt = f.path("corrupt.sqlite3");
    fs::write(&corrupt, b"synthetic invalid SQLite bytes").unwrap();
    assert!(database::open(&corrupt).is_err());
    assert_eq!(
        fs::read(corrupt).unwrap(),
        b"synthetic invalid SQLite bytes"
    );
}

#[test]
fn wal_snapshot_restores_committed_contents_and_links_on_an_isolated_copy() {
    let f = Fixture::new();
    let conn = Connection::open(f.path("pinefetch.sqlite3")).unwrap();
    conn.execute_batch(RELEASE_SCHEMAS.last().unwrap().1)
        .unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    populate_legacy(&conn);
    assert!(fs::metadata(f.path("pinefetch.sqlite3-wal")).unwrap().len() > 32);
    database::initialize(&conn).unwrap();
    let backup_path = f.backups().pop().unwrap();
    let mut restored = Connection::open(f.path("restored synthetic.sqlite3")).unwrap();
    restored
        .restore(
            rusqlite::DatabaseName::Main,
            &backup_path,
            None::<fn(rusqlite::backup::Progress)>,
        )
        .unwrap();
    assert_eq!(version(&restored), 0);
    assert_eq!(restored.query_row("SELECT h.title,t.id,t.text,c.text,c.media_path FROM history_entries h JOIN transcriptions t ON t.history_entry_id=h.id JOIN captions c ON c.history_entry_id=h.id WHERE h.id='old-1'", [], |r| Ok((r.get::<_, String>(0)?,r.get::<_, String>(1)?,r.get::<_, String>(2)?,r.get::<_, String>(3)?,r.get::<_, String>(4)?))).unwrap(),
        ("Grüße 🌲".into(),"transcript-id".into(),"Grüße\nSecond line".into(),"Caption\n🌲".into(),"/synthetic/Grüße 🌲.mp4".into()));
    assert_eq!(
        restored
            .query_row("SELECT secret_hash FROM link_dump_secrets", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        sha256_hex(b"synthetic test secret")
    );
    assert_eq!(
        restored
            .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    database::initialize(&restored).unwrap();
    assert_eq!(count(&restored, "captions"), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(backup_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn simultaneous_migrations_recheck_version_after_writer_lock() {
    let f = Fixture::new();
    let conn = Connection::open(f.path("pinefetch.sqlite3")).unwrap();
    conn.execute_batch(RELEASE_SCHEMAS.last().unwrap().1)
        .unwrap();
    populate_legacy(&conn);
    drop(conn);
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = f.path("pinefetch.sqlite3");
            let barrier = barrier.clone();
            thread::spawn(move || {
                let conn = Connection::open(path).unwrap();
                // Both read unversioned before either is permitted to acquire the
                // migration lock; the loser must re-read inside its transaction.
                assert_eq!(version(&conn), 0);
                database::initialize_with_hook(&conn, |point| {
                    if point == MigrationPoint::BeforeLock {
                        barrier.wait();
                    }
                    Ok(())
                })
                .unwrap();
                assert_eq!(version(&conn), SCHEMA_VERSION);
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(f.backups().len(), 1);
    assert_eq!(count(&f.db(), "history_entries"), 2);
}

#[test]
fn held_writer_lock_times_out_without_schema_change_then_retries() {
    let f = Fixture::new();
    let owner = Connection::open(f.path("pinefetch.sqlite3")).unwrap();
    owner
        .execute_batch(RELEASE_SCHEMAS.last().unwrap().1)
        .unwrap();
    populate_legacy(&owner);
    let original_schema = schema(&owner);
    let original_bytes = fs::read(f.path("pinefetch.sqlite3")).unwrap();
    let tx = Transaction::new_unchecked(&owner, TransactionBehavior::Immediate).unwrap();
    let contender = Connection::open(f.path("pinefetch.sqlite3")).unwrap();
    let started = Instant::now();
    assert!(database::initialize(&contender)
        .unwrap_err()
        .contains("writer lock"));
    // Check the actual SQLite setting independently of runner scheduling.
    assert_eq!(
        contender
            .pragma_query_value::<i64, _>(None, "busy_timeout", |r| r.get(0))
            .unwrap(),
        3000
    );
    // Wall time includes scheduling and filesystem delays on shared CI hosts;
    // keep a generous watchdog while retaining the production three-second limit.
    assert!(started.elapsed() < Duration::from_secs(15));
    assert_eq!(version(&contender), 0);
    assert_eq!(schema(&contender), original_schema);
    assert_eq!(
        fs::read(f.path("pinefetch.sqlite3")).unwrap(),
        original_bytes
    );
    assert!(f.backups().is_empty());
    tx.rollback().unwrap();
    database::initialize(&contender).unwrap();
    assert_eq!(version(&contender), SCHEMA_VERSION);
    assert_eq!(count(&contender, "history_entries"), 2);
    assert_eq!(f.backups().len(), 1);
}

#[test]
fn config_json_missing_valid_alias_corrupt_unreadable_and_marker_failure() {
    let f = Fixture::new();
    let conn = f.db();
    let path = f.path("config.json");
    for invalid in ["{ broken", "[]", "{}", "{\"save_captions\":\"wrong\"}"] {
        fs::write(&path, invalid).unwrap();
        assert!(import_legacy_config(&conn, &path).is_err());
        assert_eq!(
            conn.query_row(
                "SELECT legacy_config_json_migrated FROM app_config",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(import_legacy_config(&conn, &path)
        .unwrap_err()
        .contains("read failed"));
    fs::remove_dir(&path).unwrap();
    let original = r#"{"yt_dlp_path":null,"default_output_dir":"/synthetic/Grüße 🌲","save_instagram_captions":true}"#;
    fs::write(&path, original).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_marker BEFORE UPDATE OF legacy_config_json_migrated ON app_config BEGIN SELECT RAISE(ABORT,'synthetic marker failure'); END;").unwrap();
    assert!(import_legacy_config(&conn, &path).is_err());
    assert_eq!(load_config_from_db(&conn).unwrap().default_output_dir, None);
    assert_eq!(
        conn.query_row(
            "SELECT legacy_config_json_migrated FROM app_config",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    conn.execute_batch("DROP TRIGGER fail_marker").unwrap();
    import_legacy_config(&conn, &path).unwrap();
    assert!(load_config_from_db(&conn).unwrap().save_captions);
    assert_eq!(fs::read_to_string(path).unwrap(), original);
    import_legacy_config(&conn, &f.path("no-longer-present.json")).unwrap();
    let absent = database::open(&f.path("absent.sqlite3")).unwrap();
    import_legacy_config(&absent, &f.path("missing.json")).unwrap();
    assert_eq!(
        absent
            .query_row(
                "SELECT legacy_config_json_migrated FROM app_config",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn history_json_import_is_atomic_retryable_idempotent_and_retains_existing_ids() {
    let f = Fixture::new();
    let conn = f.db();
    let path = f.path("history.json");
    for invalid in ["broken", "{}", "[{\"url\":\"https://example.com\"}]"] {
        fs::write(&path, invalid).unwrap();
        assert!(import_legacy_history(&conn, &path).is_err());
        assert_eq!(count(&conn, "legacy_imports"), 0);
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(import_legacy_history(&conn, &path).is_err());
    fs::remove_dir(&path).unwrap();
    let original = serde_json::to_vec(&vec![entry("existing"), entry("second")]).unwrap();
    fs::write(&path, &original).unwrap();
    let mut existing = entry("existing");
    existing.title = Some("Newer database title".into());
    insert_history_entry_in_conn(&conn, &existing).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_import BEFORE INSERT ON legacy_imports BEGIN SELECT RAISE(ABORT,'synthetic import failure'); END;").unwrap();
    assert!(import_legacy_history(&conn, &path).is_err());
    assert_eq!(count(&conn, "history_entries"), 1);
    assert_eq!(count(&conn, "legacy_imports"), 0);
    conn.execute_batch("DROP TRIGGER fail_import").unwrap();
    import_legacy_history(&conn, &path).unwrap();
    import_legacy_history(&conn, &path).unwrap();
    assert_eq!(count(&conn, "history_entries"), 2);
    assert_eq!(
        conn.query_row(
            "SELECT title FROM history_entries WHERE id='existing'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "Newer database title"
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    let other = database::open(&f.path("duplicate.sqlite3")).unwrap();
    fs::write(
        &path,
        serde_json::to_vec(&vec![entry("same"), entry("same")]).unwrap(),
    )
    .unwrap();
    assert!(import_legacy_history(&other, &path).is_err());
    assert_eq!(count(&other, "history_entries"), 0);
    fs::remove_file(&path).unwrap();
    import_legacy_history(&other, &path).unwrap();
    assert_eq!(count(&other, "legacy_imports"), 1);
}

#[test]
fn history_json_import_normalizes_new_metadata_before_search_and_preserves_existing_rows() {
    let f = Fixture::new();
    let conn = f.db();
    let path = f.path("history.json");
    let mut existing = entry("existing");
    existing.title = Some("  Newer database title  ".into());
    existing.duration_seconds = Some(-7);
    insert_history_entry_in_conn(&conn, &existing).unwrap();
    let original = serde_json::to_vec(&json!([
        {
            "id": "derived",
            "url": "https://www.youtube.com/watch?v=synthetic",
            "created_at": 1700000000000_u64,
            "completed_at": 1700000000100_u64,
            "title": "  ",
            "uploader": "  Synthetic creator  ",
            "filename": "  ",
            "thumbnail": "  ",
            "upload_date": "  ",
            "timestamp": -1,
            "duration_seconds": -2,
            "file_size_bytes": -3,
            "sha256": format!("  {}  ", "AB".repeat(32)),
            "medium": " VIDEO ",
            "output_path": "/synthetic/Grüße 🌲.mp4",
            "pinefetch_version": "  1.4.3  "
        },
        {
            "id": "invalid",
            "url": "https://www.youtube.com/watch?v=synthetic",
            "created_at": 1700000000200_u64,
            "title": "  Explicit title  ",
            "uploader": "  ",
            "filename": "  second.mp4  ",
            "thumbnail": "  https://example.com/synthetic.jpg  ",
            "upload_date": "  20200101  ",
            "timestamp": 0,
            "duration_seconds": 12,
            "file_size_bytes": 123,
            "sha256": "not-a-hash",
            "medium": "unknown",
            "source": " YoUTuBe ",
            "platform": " YouTube ",
            "output_path": "  ",
            "pinefetch_version": "  "
        },
        entry("existing")
    ]))
    .unwrap();
    fs::write(&path, &original).unwrap();
    import_legacy_history(&conn, &path).unwrap();
    // Read the persisted columns directly: history readers normalize returned
    // rows too, which would hide bad stored values and broken SQL filtering.
    let stored = |id| {
        conn.query_row(
            "SELECT title,uploader,filename,thumbnail,upload_date,timestamp,duration_seconds,file_size_bytes,sha256,medium,source,platform,output_path,pinefetch_version,created_at,completed_at FROM history_entries WHERE id=?1",
            [id],
            |r| Ok(json!({
                "title": r.get::<_, Option<String>>(0)?,
                "uploader": r.get::<_, Option<String>>(1)?,
                "filename": r.get::<_, Option<String>>(2)?,
                "thumbnail": r.get::<_, Option<String>>(3)?,
                "upload_date": r.get::<_, Option<String>>(4)?,
                "timestamp": r.get::<_, Option<i64>>(5)?,
                "duration_seconds": r.get::<_, Option<i64>>(6)?,
                "file_size_bytes": r.get::<_, Option<i64>>(7)?,
                "sha256": r.get::<_, Option<String>>(8)?,
                "medium": r.get::<_, Option<String>>(9)?,
                "source": r.get::<_, Option<String>>(10)?,
                "platform": r.get::<_, Option<String>>(11)?,
                "output_path": r.get::<_, Option<String>>(12)?,
                "pinefetch_version": r.get::<_, Option<String>>(13)?,
                "created_at": r.get::<_, i64>(14)?,
                "completed_at": r.get::<_, Option<i64>>(15)?
            })),
        ).unwrap()
    };
    assert_eq!(
        stored("derived"),
        json!({
            "title": "Grüße 🌲", "uploader": "Synthetic creator", "filename": "Grüße 🌲.mp4",
            "thumbnail": null, "upload_date": null, "timestamp": null,
            "duration_seconds": null, "file_size_bytes": null, "sha256": "ab".repeat(32),
            "medium": "video", "source": "youtube", "platform": "youtube",
            "output_path": "/synthetic/Grüße 🌲.mp4", "pinefetch_version": "1.4.3",
            "created_at": 1700000000000_i64, "completed_at": 1700000000100_i64
        })
    );
    assert_eq!(
        stored("invalid"),
        json!({
            "title": "Explicit title", "uploader": null, "filename": "second.mp4",
            "thumbnail": "https://example.com/synthetic.jpg", "upload_date": "20200101", "timestamp": 0,
            "duration_seconds": 12, "file_size_bytes": 123, "sha256": null,
            "medium": null, "source": "youtube", "platform": "YouTube",
            "output_path": null, "pinefetch_version": null,
            "created_at": 1700000000200_i64, "completed_at": null
        })
    );
    assert_eq!(stored("existing")["title"], "  Newer database title  ");
    assert_eq!(stored("existing")["duration_seconds"], -7);
    import_legacy_history(&conn, &path).unwrap();
    assert_eq!(count(&conn, "history_entries"), 3);
    assert_eq!(fs::read(path).unwrap(), original);
    let state = app_state(conn);
    let page =
        search_history_page_from_db(&state, 20, 0, Some("Grüße"), Some("title"), Some("youtube"))
            .unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].id, "derived");
}

#[test]
fn completion_db_failure_preserves_output_and_rolls_back_related_metadata_then_retries() {
    let f = Fixture::new();
    let state = app_state(f.db());
    let job = job("job-one", &f.0, true);
    let output = f.path("Grüße 🌲 transcript.txt");
    fs::write(&output, "Grüße\nTranscript").unwrap();
    let media = f.path("Grüße 🌲 video.mp4");
    fs::write(&media, b"synthetic video").unwrap();
    let caption = write_caption_sidecar(&media, "Caption 🌲").unwrap();
    let captions = vec![SavedCaption {
        media_path: media.to_string_lossy().into_owned(),
        caption_path: caption.to_string_lossy().into_owned(),
        text: "Caption 🌲".into(),
    }];
    completion::begin(&state, &job).unwrap();
    state.db.lock().unwrap().execute_batch("CREATE TRIGGER fail_caption BEFORE INSERT ON captions BEGIN SELECT RAISE(ABORT,'synthetic caption failure'); END;").unwrap();
    let result = completion::complete(&state, &job, output.to_str(), None, &captions, Some("de"));
    assert!(result
        .as_ref()
        .unwrap_err()
        .contains("Output file preserved"));
    let mut event = DownloadStateEvent {
        id: job.id.clone(),
        state: "success".into(),
        exit_code: Some(0),
        error: None,
        output_path: Some(output.to_string_lossy().into_owned()),
    };
    completion::apply_result(&mut event, result);
    assert_eq!(event.state, "error");
    assert!(event.output_path.is_some());
    {
        let conn = state.db.lock().unwrap();
        assert_eq!(count(&conn, "history_entries"), 0);
        assert_eq!(count(&conn, "transcriptions"), 0);
        assert_eq!(count(&conn, "captions"), 0);
        assert_eq!(
            conn.query_row("SELECT state FROM job_completions", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "output_ready"
        );
        conn.execute_batch("DROP TRIGGER fail_caption").unwrap();
    }
    assert_eq!(fs::read_to_string(&output).unwrap(), "Grüße\nTranscript");
    completion::complete(&state, &job, output.to_str(), None, &captions, Some("de")).unwrap();
    completion::complete(&state, &job, output.to_str(), None, &captions, Some("de")).unwrap();
    let conn = state.db.lock().unwrap();
    assert_eq!(count(&conn, "history_entries"), 1);
    assert_eq!(count(&conn, "transcriptions"), 1);
    assert_eq!(count(&conn, "captions"), 1);
    assert_eq!(conn.query_row("SELECT h.id,t.text,t.language,c.text,j.state FROM history_entries h JOIN transcriptions t ON t.history_entry_id=h.id JOIN captions c ON c.history_entry_id=h.id JOIN job_completions j ON j.history_entry_id=h.id",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?))).unwrap(),
        ("job-one".into(),"Grüße\nTranscript".into(),"de".into(),"Caption 🌲".into(),"complete".into()));
}

#[test]
fn optional_caption_missing_required_output_missing_repeated_url_and_deleted_history() {
    let f = Fixture::new();
    let state = app_state(f.db());
    let output = f.path("already existing Grüße 🌲.mp4");
    fs::write(&output, b"existing file").unwrap();
    let first = job("first", &f.0, false);
    completion::begin(&state, &first).unwrap();
    assert!(completion::complete(
        &state,
        &first,
        Some("/synthetic/missing-output"),
        None,
        &[],
        None
    )
    .is_err());
    assert_eq!(count(&state.db.lock().unwrap(), "history_entries"), 0);
    completion::complete(&state, &first, output.to_str(), None, &[], None).unwrap();
    let second = job("second", &f.0, false);
    completion::begin(&state, &second).unwrap();
    completion::complete(&state, &second, output.to_str(), None, &[], None).unwrap();
    assert_eq!(count(&state.db.lock().unwrap(), "history_entries"), 2);
    assert_eq!(fs::read(&output).unwrap(), b"existing file");
    delete_history_entry_from_db(&state, "first").unwrap();
    assert!(
        completion::complete(&state, &first, output.to_str(), None, &[], None)
            .unwrap_err()
            .contains("removed")
    );
    assert_eq!(count(&state.db.lock().unwrap(), "history_entries"), 1);
}

#[test]
fn controlled_failure_before_commit_retains_receipt_and_no_half_history() {
    let f = Fixture::new();
    let state = app_state(f.db());
    let job = job("hook-failure", &f.0, false);
    let output = f.path("video.mp4");
    fs::write(&output, b"video").unwrap();
    completion::begin(&state, &job).unwrap();
    assert!(completion::complete_with_hook(
        &state,
        &job,
        output.to_str(),
        None,
        &[],
        None,
        |point| {
            if point == completion::CompletionPoint::BeforeCommit {
                return Err("controlled pre-commit failure".into());
            }
            Ok(())
        }
    )
    .is_err());
    assert_eq!(count(&state.db.lock().unwrap(), "history_entries"), 0);
    completion::complete(&state, &job, output.to_str(), None, &[], None).unwrap();
}

#[test]
fn sqlite_full_error_rolls_back_without_filling_the_filesystem() {
    let f = Fixture::new();
    let conn = f.db();
    let pages: i64 = conn
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .unwrap();
    conn.pragma_update(None, "max_page_count", pages).unwrap();
    let tx = Transaction::new_unchecked(&conn, TransactionBehavior::Immediate).unwrap();
    let error=tx.execute("INSERT INTO history_entries(id,url,title,created_at) VALUES ('disk-full','https://example.com',zeroblob(1048576),1)",[]).unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DiskFull)
    );
    drop(tx);
    drop(conn);
    let reopened = f.db();
    assert_eq!(count(&reopened, "history_entries"), 0);
    assert_eq!(version(&reopened), SCHEMA_VERSION);
}

#[test]
fn owned_temporaries_preserve_foreign_and_preexisting_files_and_sidecar_targets() {
    let f = Fixture::new();
    let media = f.path("Grüße 🌲 video.mp4");
    fs::write(&media, b"existing media").unwrap();
    let caption = media.with_extension("caption.txt");
    fs::write(&caption, "existing caption").unwrap();
    let produced = write_caption_sidecar(&media, "new caption").unwrap();
    assert_ne!(produced, caption);
    assert_eq!(fs::read_to_string(caption).unwrap(), "existing caption");
    assert_eq!(fs::read_to_string(produced).unwrap(), "new caption");
    let owned_path = f.path("owned.tmp");
    let owned = OwnedTemporaryFile::create_at(owned_path.clone()).unwrap();
    fs::rename(&owned_path, f.path("moved-owned.tmp")).unwrap();
    fs::write(&owned_path, b"foreign replacement").unwrap();
    drop(owned);
    assert_eq!(fs::read(&owned_path).unwrap(), b"foreign replacement");
    assert!(OwnedTemporaryFile::create_at(owned_path).is_err());
    let clean_path = f.path("clean-owned.tmp");
    let clean = OwnedTemporaryFile::create_at(clean_path.clone()).unwrap();
    drop(clean);
    #[cfg(unix)]
    assert!(!clean_path.exists());
    assert_eq!(fs::read(media).unwrap(), b"existing media");
}

#[test]
fn configuration_patches_read_latest_values_across_connections() {
    let f = Fixture::new();
    let first = app_state(f.db());
    let second = app_state(f.db());
    update_config(&first, |cfg| {
        cfg.last_download_url = Some("https://example.com/newer".into())
    })
    .unwrap();
    update_config(&second, |cfg| cfg.save_captions = true).unwrap();
    let persisted = load_config_from_db(&first.db.lock().unwrap()).unwrap();
    assert_eq!(
        persisted.last_download_url.as_deref(),
        Some("https://example.com/newer")
    );
    assert!(persisted.save_captions);
}

#[test]
fn malformed_relationships_indexes_and_newer_version_after_lock_are_rejected() {
    let f = Fixture::new();
    let orphan = Connection::open(f.path("orphan.sqlite3")).unwrap();
    orphan
        .execute_batch(RELEASE_SCHEMAS.last().unwrap().1)
        .unwrap();
    orphan.execute_batch("PRAGMA foreign_keys=OFF; INSERT INTO transcriptions(id,history_entry_id,text,\"type\") VALUES ('orphan','missing','synthetic','text');").unwrap();
    assert!(database::initialize(&orphan)
        .unwrap_err()
        .contains("orphaned"));
    assert_eq!(count(&orphan, "transcriptions"), 1);
    assert_eq!(version(&orphan), 0);
    let indexes = Connection::open(f.path("unexpected-index.sqlite3")).unwrap();
    indexes
        .execute_batch(RELEASE_SCHEMAS.last().unwrap().1)
        .unwrap();
    indexes
        .execute_batch("CREATE UNIQUE INDEX unexpected_url_uniqueness ON history_entries(url);")
        .unwrap();
    assert!(database::initialize(&indexes)
        .unwrap_err()
        .contains("unexpected_url_uniqueness"));
    assert_eq!(version(&indexes), 0);
    let source = Connection::open(f.path("version-race.sqlite3")).unwrap();
    source
        .execute_batch(RELEASE_SCHEMAS.last().unwrap().1)
        .unwrap();
    let other = Connection::open(f.path("version-race.sqlite3")).unwrap();
    let before = schema(&source);
    let failure = database::initialize_with_hook(&source, |point| {
        if point == MigrationPoint::BeforeLock {
            other.pragma_update(None, "user_version", 99).unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(failure.contains("99"));
    assert_eq!(schema(&source), before);
    assert_eq!(version(&source), 99);
    assert!(f.backups().is_empty());
}

#[test]
fn completion_readonly_lock_and_disk_full_errors_preserve_files_and_retry_after_reopen() {
    let f = Fixture::new();
    let writable = app_state(f.db());
    let job = job("persistence-errors", &f.0, true);
    let output = f.path("Grüße 🌲 required transcript.txt");
    fs::write(&output, "x".repeat(1024 * 1024)).unwrap();
    completion::begin(&writable, &job).unwrap();
    completion::output_ready(&writable, &job, output.to_str()).unwrap();
    let read_only = Connection::open_with_flags(
        f.path("pinefetch.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    database::initialize(&read_only).unwrap();
    let read_only = app_state(read_only);
    assert!(
        completion::complete(&read_only, &job, output.to_str(), None, &[], Some("de"))
            .unwrap_err()
            .contains("preserved")
    );
    drop(read_only);
    assert!(
        completion::complete(&writable, &job, output.to_str(), None, &[], None)
            .unwrap_err()
            .contains("language")
    );
    let owner = f.db();
    let tx = Transaction::new_unchecked(&owner, TransactionBehavior::Immediate).unwrap();
    assert!(
        completion::complete(&writable, &job, output.to_str(), None, &[], Some("de"))
            .unwrap_err()
            .contains("locked")
    );
    tx.rollback().unwrap();
    drop(owner);
    {
        let conn = writable.db.lock().unwrap();
        let pages: i64 = conn
            .pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
        conn.pragma_update(None, "max_page_count", pages).unwrap();
    }
    assert!(
        completion::complete(&writable, &job, output.to_str(), None, &[], Some("de"))
            .unwrap_err()
            .contains("full")
    );
    assert_eq!(count(&writable.db.lock().unwrap(), "history_entries"), 0);
    assert_eq!(count(&writable.db.lock().unwrap(), "transcriptions"), 0);
    assert_eq!(fs::metadata(&output).unwrap().len(), 1024 * 1024);
    drop(writable);
    let reopened = app_state(f.db());
    completion::complete(&reopened, &job, output.to_str(), None, &[], Some("de")).unwrap();
    assert_eq!(count(&reopened.db.lock().unwrap(), "history_entries"), 1);
    assert_eq!(count(&reopened.db.lock().unwrap(), "transcriptions"), 1);
}

#[test]
fn simultaneous_completion_commits_only_one_history_and_preserves_identity_conflicts() {
    let f = Fixture::new();
    let output = f.path("Grüße 🌲 output.mp4");
    fs::write(&output, b"synthetic video").unwrap();
    let job = job("same-job", &f.0, false);
    let initial = app_state(f.db());
    completion::begin(&initial, &job).unwrap();
    drop(initial);
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let conn = f.db();
            let job = job.clone();
            let output = output.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let state = app_state(conn);
                barrier.wait();
                completion::complete(&state, &job, output.to_str(), None, &[], None).unwrap();
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let state = app_state(f.db());
    assert_eq!(count(&state.db.lock().unwrap(), "history_entries"), 1);
    let mut conflict = job.clone();
    conflict.url = "https://example.com/different".into();
    assert!(completion::complete(&state, &conflict, output.to_str(), None, &[], None).is_err());
    let original: String = state
        .db
        .lock()
        .unwrap()
        .query_row("SELECT url FROM history_entries", [], |r| r.get(0))
        .unwrap();
    assert_eq!(original, job.url);
    let mut collision = job.clone();
    collision.id = "legacy-id-collision".into();
    insert_history_entry_in_conn(&state.db.lock().unwrap(), &entry(&collision.id)).unwrap();
    completion::begin(&state, &collision).unwrap();
    assert!(
        completion::complete(&state, &collision, output.to_str(), None, &[], None)
            .unwrap_err()
            .contains("conflicts")
    );
    assert_eq!(count(&state.db.lock().unwrap(), "history_entries"), 2);
}

#[test]
fn json_import_conflicting_identity_and_concurrent_starts_never_merge_by_url() {
    let f = Fixture::new();
    let conn = f.db();
    let path = f.path("history.json");
    let mut conflict = entry("existing");
    conflict.url = "https://example.com/different".into();
    insert_history_entry_in_conn(&conn, &conflict).unwrap();
    fs::write(
        &path,
        serde_json::to_vec(&vec![entry("new-first"), entry("existing")]).unwrap(),
    )
    .unwrap();
    assert!(import_legacy_history(&conn, &path)
        .unwrap_err()
        .contains("conflicts"));
    assert_eq!(count(&conn, "history_entries"), 1);
    assert_eq!(count(&conn, "legacy_imports"), 0);
    fs::write(
        &path,
        serde_json::to_vec(&vec![entry("new-first"), entry("new-second")]).unwrap(),
    )
    .unwrap();
    let other = f.db();
    let barrier = Arc::new(Barrier::new(2));
    let child_barrier = barrier.clone();
    let child_path = path.clone();
    let child = thread::spawn(move || {
        child_barrier.wait();
        import_legacy_history(&other, &child_path).unwrap();
    });
    barrier.wait();
    import_legacy_history(&conn, &path).unwrap();
    child.join().unwrap();
    assert_eq!(count(&conn, "history_entries"), 3);
    assert_eq!(count(&conn, "legacy_imports"), 1);
}

#[test]
fn a_running_older_instance_refuses_every_write_after_another_process_upgrades() {
    let f = Fixture::new();
    let state = app_state(f.db());
    let output = f.path("existing output.mp4");
    fs::write(&output, b"synthetic existing output").unwrap();
    let job = job("running-old-instance", &f.0, false);
    completion::begin(&state, &job).unwrap();
    completion::output_ready(&state, &job, output.to_str()).unwrap();
    insert_history_entry_in_conn(&state.db.lock().unwrap(), &entry("existing")).unwrap();
    let secret = create_link_dump_secret_in_db(&state, Some("Synthetic".into())).unwrap();
    let other = f.db();
    other.pragma_update(None, "user_version", 99).unwrap();
    let before = schema(&other);
    assert!(update_config(&state, |cfg| cfg.save_captions = true)
        .unwrap_err()
        .contains("99"));
    assert!(update_link_dump_settings_in_db(
        &state,
        LinkDumpSettingsPatch {
            server_enabled: Some(false),
            host: None,
            port: None
        }
    )
    .is_err());
    assert!(completion::begin(&state, &job).is_err());
    assert!(completion::complete(&state, &job, output.to_str(), None, &[], None).is_err());
    assert!(delete_history_entry_from_db(&state, "existing").is_err());
    assert!(clear_history_entries_in_db(&state).is_err());
    assert!(create_link_dump_secret_in_db(&state, None).is_err());
    assert!(revoke_link_dump_secret_in_db(&state, &secret.connection.id).is_err());
    assert!(delete_link_dump_secret_in_db(&state, &secret.connection.id).is_err());
    assert!(validate_link_dump_secret(&state, Some(&secret.secret)).is_err());
    assert!(import_legacy_config(&other, &f.path("missing config.json")).is_err());
    assert!(import_legacy_history(&other, &f.path("missing history.json")).is_err());
    assert_eq!(version(&other), 99);
    assert_eq!(schema(&other), before);
    assert_eq!(count(&other, "history_entries"), 1);
    assert_eq!(count(&other, "link_dump_secrets"), 1);
    assert_eq!(count(&other, "legacy_imports"), 0);
    assert!(!load_config_from_db(&other).unwrap().save_captions);
    assert!(
        get_link_dump_settings_from_conn(&other)
            .unwrap()
            .server_enabled
    );
    assert_eq!(
        other
            .query_row(
                "SELECT last_used_at,revoked_at,deleted_at FROM link_dump_secrets",
                [],
                |r| Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?
                ))
            )
            .unwrap(),
        (None, None, None)
    );
    assert_eq!(fs::read(&output).unwrap(), b"synthetic existing output");
}

#[cfg(unix)]
#[test]
fn permissions_block_json_output_database_and_required_backup_without_mutation() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let conn = f.db();
    let json = f.path("config.json");
    fs::write(&json, b"{}").unwrap();
    fs::set_permissions(&json, fs::Permissions::from_mode(0o000)).unwrap();
    assert!(import_legacy_config(&conn, &json)
        .unwrap_err()
        .contains("read failed"));
    fs::set_permissions(&json, fs::Permissions::from_mode(0o600)).unwrap();
    let output_dir = f.path("read only output");
    fs::create_dir(&output_dir).unwrap();
    let media = output_dir.join("media.mp4");
    fs::write(&media, b"existing").unwrap();
    fs::set_permissions(&output_dir, fs::Permissions::from_mode(0o500)).unwrap();
    assert!(write_caption_sidecar(&media, "caption").is_err());
    assert_eq!(fs::read(&media).unwrap(), b"existing");
    assert!(database::open(&output_dir.join("new.sqlite3")).is_err());
    fs::set_permissions(&output_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let backup_dir = f.path("read only backup");
    fs::create_dir(&backup_dir).unwrap();
    let path = backup_dir.join("pinefetch.sqlite3");
    let legacy = Connection::open(&path).unwrap();
    legacy
        .execute_batch(RELEASE_SCHEMAS.last().unwrap().1)
        .unwrap();
    legacy
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    populate_legacy(&legacy);
    let original = schema(&legacy);
    fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o500)).unwrap();
    let failure = database::initialize(&legacy).unwrap_err();
    fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(failure.contains("backup failed"), "{failure}");
    assert_eq!(schema(&legacy), original);
    assert_eq!(version(&legacy), 0);
    database::initialize(&legacy).unwrap();
}

fn child_checkpoint() {
    println!("PINEFETCH_SYNTHETIC_CHECKPOINT");
    std::io::stdout().flush().unwrap();
    // Parent waits for the pipe handshake, then SIGKILLs us. No Drop handlers.
    let mut byte = [0];
    std::io::stdin().read_exact(&mut byte).unwrap();
    panic!("parent must terminate the test process");
}

#[test]
fn abrupt_child() {
    let Ok(root) = std::env::var("PINEFETCH_SYNTHETIC_CHILD_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    assert!(root.starts_with(std::env::temp_dir()));
    assert!(root.join(".synthetic-fixture").is_file());
    let mode = std::env::var("PINEFETCH_SYNTHETIC_CHILD_MODE").unwrap();
    let conn = Connection::open(root.join("pinefetch.sqlite3")).unwrap();
    conn.pragma_update(None, "cache_size", 1).unwrap();
    if mode == "future" {
        let tx = Transaction::new_unchecked(&conn, TransactionBehavior::Immediate).unwrap();
        tx.execute(
            "UPDATE history_entries SET title=zeroblob(2097152) WHERE id='old-2'",
            [],
        )
        .unwrap();
        child_checkpoint();
    } else if mode == "migration" {
        database::initialize_with_hook(&conn, |point| {
            if point == MigrationPoint::Adopted {
                // Force pager spill, so reopening genuinely needs hot-journal
                // recovery rather than merely discarding cached changes.
                conn.execute(
                    "UPDATE history_entries SET title=zeroblob(2097152) WHERE id='old-2'",
                    [],
                )
                .unwrap();
                child_checkpoint();
            }
            Ok(())
        })
        .unwrap();
    } else if mode == "import" {
        database::initialize(&conn).unwrap();
        import_legacy_history_with_hook(&conn, &root.join("history.json"), || {
            child_checkpoint();
            Ok(())
        })
        .unwrap();
    } else {
        database::initialize(&conn).unwrap();
        let state = app_state(conn);
        let job = job("crash-job", &root, true);
        let output = root.join("Grüße 🌲 transcript.txt");
        completion::begin(&state, &job).unwrap();
        completion::complete_with_hook(
            &state,
            &job,
            output.to_str(),
            None,
            &[],
            Some("de"),
            |point| {
                if point == completion::CompletionPoint::BeforeCommit {
                    child_checkpoint();
                }
                Ok(())
            },
        )
        .unwrap();
    }
}

#[cfg(unix)]
#[test]
fn abrupt_process_kill_rolls_back_migration_and_completion_and_allows_safe_retry() {
    use std::os::unix::process::ExitStatusExt;
    for mode in ["migration", "completion", "import", "future"] {
        let f = Fixture::new();
        let conn = Connection::open(f.path("pinefetch.sqlite3")).unwrap();
        conn.execute_batch(RELEASE_SCHEMAS.last().unwrap().1)
            .unwrap();
        populate_legacy(&conn);
        if mode != "migration" {
            database::initialize(&conn).unwrap();
        }
        if mode == "future" {
            conn.pragma_update(None, "user_version", 99).unwrap();
        }
        let mut imported = entry("legacy-crash-one");
        imported.title = Some("x".repeat(2 * 1024 * 1024));
        fs::write(
            f.path("history.json"),
            serde_json::to_vec(&vec![imported, entry("legacy-crash-two")]).unwrap(),
        )
        .unwrap();
        let before = schema(&conn);
        drop(conn);
        let committed_bytes = fs::read(f.path("pinefetch.sqlite3")).unwrap();
        let initial_backups = f.backups().len();
        let output = f.path("Grüße 🌲 transcript.txt");
        let transcript_text = format!(
            "Synthetic crash transcript 🌲{}",
            "x".repeat(2 * 1024 * 1024)
        );
        fs::write(&output, &transcript_text).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "integrity_tests::abrupt_child", "--nocapture"])
            .env("PINEFETCH_SYNTHETIC_CHILD_ROOT", &f.0)
            .env("PINEFETCH_SYNTHETIC_CHILD_MODE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pipe = child.stdout.take().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(pipe);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 0 {
                if line.contains("PINEFETCH_SYNTHETIC_CHECKPOINT") {
                    ready_tx.send(()).unwrap();
                    return;
                }
                line.clear();
            }
        });
        let ready = ready_rx.recv_timeout(Duration::from_secs(20));
        child.kill().unwrap();
        let status = child.wait().unwrap();
        reader.join().unwrap();
        ready.unwrap();
        assert_eq!(status.signal(), Some(9));
        let probe = Connection::open_with_flags(
            f.path("pinefetch.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let error = probe
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap_err();
        assert!(
            matches!(error, rusqlite::Error::SqliteFailure(e, _) if e.extended_code == rusqlite::ffi::SQLITE_READONLY_ROLLBACK)
        );
        drop(probe);
        if mode == "future" {
            assert!(database::open(&f.path("pinefetch.sqlite3"))
                .unwrap_err()
                .contains("99"));
            let conn = Connection::open_with_flags(
                f.path("pinefetch.sqlite3"),
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            assert_eq!(version(&conn), 99);
            assert_eq!(schema(&conn), before);
            assert_eq!(count(&conn, "history_entries"), 2);
            assert_eq!(
                conn.query_row(
                    "SELECT title FROM history_entries WHERE id='old-2'",
                    [],
                    |r| r.get::<_, Option<String>>(0)
                )
                .unwrap(),
                None
            );
            assert_eq!(f.backups().len(), initial_backups);
            assert_eq!(
                fs::read(f.path("pinefetch.sqlite3")).unwrap(),
                committed_bytes
            );
            continue;
        }
        let mut observed_recovered_state = false;
        let conn = database::open_with_hook(&f.path("pinefetch.sqlite3"), |point| {
            if point == MigrationPoint::Compatible {
                // The application's real open path must restore the committed
                // state before either metadata validation or further upgrades.
                let recovered = Connection::open_with_flags(
                    f.path("pinefetch.sqlite3"),
                    OpenFlags::SQLITE_OPEN_READ_ONLY,
                )
                .unwrap();
                assert_eq!(schema(&recovered), before);
                assert_eq!(
                    version(&recovered),
                    if mode == "migration" {
                        0
                    } else {
                        SCHEMA_VERSION
                    }
                );
                assert_eq!(count(&recovered, "history_entries"), 2);
                assert_eq!(
                    recovered
                        .query_row(
                            "SELECT title FROM history_entries WHERE id='old-2'",
                            [],
                            |r| r.get::<_, Option<String>>(0)
                        )
                        .unwrap(),
                    None
                );
                observed_recovered_state = true;
            }
            Ok(())
        })
        .unwrap();
        assert!(observed_recovered_state);
        assert_eq!(fs::read_to_string(&output).unwrap(), transcript_text);
        database::initialize(&conn).unwrap();
        if mode == "completion" {
            assert_eq!(
                conn.query_row(
                    "SELECT state FROM job_completions WHERE job_id='crash-job'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "output_ready"
            );
            assert_eq!(count(&conn, "transcriptions"), 1); // only the original legacy transcript
            let state = app_state(conn);
            let job = job("crash-job", &f.0, true);
            completion::complete(&state, &job, output.to_str(), None, &[], Some("de")).unwrap();
            completion::complete(&state, &job, output.to_str(), None, &[], Some("de")).unwrap();
            assert_eq!(count(&state.db.lock().unwrap(), "history_entries"), 3);
        } else if mode == "import" {
            assert_eq!(count(&conn, "legacy_imports"), 0);
            import_legacy_history(&conn, &f.path("history.json")).unwrap();
            import_legacy_history(&conn, &f.path("history.json")).unwrap();
            assert_eq!(count(&conn, "history_entries"), 4);
        }
    }
}
