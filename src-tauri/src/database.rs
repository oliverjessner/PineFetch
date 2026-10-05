use super::*;
use rusqlite::{
    backup::{Backup, StepResult},
    OpenFlags, Transaction, TransactionBehavior,
};

pub(super) const SCHEMA_VERSION: i64 = 2;
const LOCK_TIMEOUT: Duration = Duration::from_secs(3);
// The reported upgraded layout retains this stronger, compatible constraint.
const LEGACY_HISTORY_VERSION: &str = "2.1.0";
const LEGACY_HISTORY_VERSION_SQL: &str = "'2.1.0'";

pub(super) fn legacy_history_version_default(
    conn: &Connection,
) -> rusqlite::Result<Option<&'static str>> {
    let retained: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('history_entries')
         WHERE name='pinefetch_version' AND UPPER(type)='TEXT' AND \"notnull\"=1
           AND pk=0 AND dflt_value=?1)",
        [LEGACY_HISTORY_VERSION_SQL],
        |row| row.get(0),
    )?;
    Ok(retained.then_some(LEGACY_HISTORY_VERSION))
}

// Version 1 adopts the known pre-versioning schemas; version 2 adds only
// import receipts and the minimal processing/output/commit receipt.
const RECEIPTS_SQL: &str = r#"
CREATE TABLE legacy_imports (
    name TEXT PRIMARY KEY NOT NULL,
    completed_at INTEGER NOT NULL
);
CREATE TABLE job_completions (
    job_id TEXT PRIMARY KEY NOT NULL,
    url TEXT NOT NULL,
    output_path TEXT,
    state TEXT NOT NULL CHECK (state IN ('processing', 'output_ready', 'complete')),
    history_entry_id TEXT UNIQUE,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (history_entry_id) REFERENCES history_entries(id) ON DELETE SET NULL
);
"#;

pub(super) fn open(path: &Path) -> Result<Connection, String> {
    let conn = open_uninitialized(path)?;
    initialize(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub(super) fn open_with_hook(
    path: &Path,
    hook: impl FnMut(MigrationPoint) -> Result<(), String>,
) -> Result<Connection, String> {
    let conn = open_uninitialized(path)?;
    initialize_with_hook(&conn, hook)?;
    Ok(conn)
}

fn open_uninitialized(path: &Path) -> Result<Connection, String> {
    // Never turn an unreadable/corrupt existing file into a new database.
    match fs::metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err("SQLite path is not a regular file".into());
            }
            let probe = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| format!("SQLite compatibility read failed: {e}"))?;
            probe
                .busy_timeout(LOCK_TIMEOUT)
                .map_err(|e| e.to_string())?;
            match probe.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)) {
                Ok(version) => {
                    supported_version(version)?;
                }
                Err(rusqlite::Error::SqliteFailure(error, _))
                    if matches!(
                        error.extended_code,
                        rusqlite::ffi::SQLITE_READONLY_ROLLBACK
                            | rusqlite::ffi::SQLITE_READONLY_RECOVERY
                    ) =>
                {
                    // SQLite must roll back spilled, uncommitted pages before
                    // even reading user_version. Permit its native pager
                    // recovery, then check the recovered committed version
                    // before any application pragma/migration/data write.
                    eprintln!(
                        "SQLite interrupted-write recovery required before compatibility check"
                    );
                }
                Err(error) => return Err(format!("SQLite compatibility read failed: {error}")),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("SQLite path could not be inspected: {e}")),
    }
    Connection::open(path).map_err(|e| format!("SQLite open failed: {e}"))
}

fn check_version(conn: &Connection) -> Result<i64, String> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| format!("SQLite schema version could not be read: {e}"))?;
    supported_version(version)
}

fn supported_version(version: i64) -> Result<i64, String> {
    if !(0..=SCHEMA_VERSION).contains(&version) {
        return Err(format!("Unsupported SQLite schema version {version}; this app supports 0..={SCHEMA_VERSION}. Database preserved."));
    }
    Ok(version)
}

pub(super) fn initialize(conn: &Connection) -> Result<(), String> {
    initialize_with_hook(conn, |_| Ok(()))
}

// A long-running older instance can outlive another process's schema upgrade.
// Recheck compatibility under every application writer reservation as well.
pub(super) fn write_transaction(conn: &Connection) -> Result<Transaction<'_>, String> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|e| format!("SQLite writer lock failed (3 second limit): {e}"))?;
    let version = check_version(&tx)?;
    if version != SCHEMA_VERSION {
        return Err(format!("SQLite schema {version} requires initialization to supported schema {SCHEMA_VERSION}; write refused, database preserved"));
    }
    Ok(tx)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MigrationPoint {
    Compatible,
    BeforeLock,
    Locked,
    BackedUp,
    Adopted,
    Receipts,
}

pub(super) fn initialize_with_hook(
    conn: &Connection,
    mut hook: impl FnMut(MigrationPoint) -> Result<(), String>,
) -> Result<(), String> {
    conn.busy_timeout(LOCK_TIMEOUT)
        .map_err(|e| format!("SQLite timeout setup failed: {e}"))?;
    let initial = check_version(conn)?;
    hook(MigrationPoint::Compatible)?;
    // SQLite ignores foreign_keys changes inside a transaction.
    conn.pragma_update(None, "foreign_keys", true)
        .map_err(|e| format!("SQLite foreign key setup failed: {e}"))?;
    if initial == SCHEMA_VERSION {
        return validate_layout(conn, initial);
    }
    hook(MigrationPoint::BeforeLock)?;
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|e| format!("SQLite migration writer lock failed (3 second limit): {e}"))?;
    hook(MigrationPoint::Locked)?;
    // Another process may have completed the upgrade while we waited.
    let version = check_version(&tx)?;
    validate_layout(&tx, version)?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    let has_tables: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%')", [], |r| r.get(0)
    ).map_err(|e| format!("SQLite schema inspection failed: {e}"))?;
    if has_tables {
        check_integrity(&tx)?;
        migration_backup(conn, version)?;
        hook(MigrationPoint::BackedUp)?;
    }
    // Entire upgrade section commits together. No partial baseline on failure.
    if version == 0 {
        adopt_legacy_layout(&tx).map_err(|e| format!("SQLite migration 1 failed: {e}"))?;
        tx.pragma_update(None, "user_version", 1)
            .map_err(|e| e.to_string())?;
        hook(MigrationPoint::Adopted)?;
    }
    if version < 2 {
        tx.execute_batch(RECEIPTS_SQL)
            .map_err(|e| format!("SQLite migration 2 failed: {e}"))?;
        tx.pragma_update(None, "user_version", 2)
            .map_err(|e| e.to_string())?;
        hook(MigrationPoint::Receipts)?;
    }
    if check_version(&tx)? != SCHEMA_VERSION {
        return Err(
            "SQLite upgrade has no step for the supported schema version; rolled back".into(),
        );
    }
    check_integrity(&tx)?;
    validate_layout(&tx, SCHEMA_VERSION)?;
    tx.commit()
        .map_err(|e| format!("SQLite migration commit failed: {e}"))
}

fn check_integrity(conn: &Connection) -> Result<(), String> {
    let check: String = conn
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(|e| format!("SQLite integrity check failed: {e}"))?;
    if check != "ok" {
        return Err("SQLite integrity check found corruption; database preserved".into());
    }
    let mut stmt = conn
        .prepare("PRAGMA foreign_key_check")
        .map_err(|e| e.to_string())?;
    if stmt
        .query([])
        .map_err(|e| e.to_string())?
        .next()
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Err("SQLite contains orphaned relationships; database preserved".into());
    }
    Ok(())
}

fn migration_backup(conn: &Connection, version: i64) -> Result<(), String> {
    let Some(path) = conn.path().filter(|path| !path.is_empty()) else {
        // Only existing unit-test in-memory connections have no backing file.
        return Ok(());
    };
    let path = Path::new(path);
    let name = path
        .file_name()
        .ok_or("SQLite file name unavailable")?
        .to_string_lossy();
    let backup_path = path.with_file_name(format!(
        "{name}.pre-schema-{version}-{}.sqlite3",
        Uuid::new_v4()
    ));
    let partial = backup_path.with_extension("partial");
    let result = (|| -> Result<(), String> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&partial).map_err(|e| e.to_string())?;
        // The migration connection holds BEGIN IMMEDIATE but has not written.
        // A dedicated read-only source sees committed pages including WAL;
        // writers cannot change them. Backing up the writing connection itself
        // would return SQLITE_LOCKED. No nested transactions are involved.
        let source = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
        source
            .busy_timeout(LOCK_TIMEOUT)
            .map_err(|e| e.to_string())?;
        source
            .pragma_update(None, "foreign_keys", true)
            .map_err(|e| e.to_string())?;
        let mut destination = Connection::open(&partial).map_err(|e| e.to_string())?;
        {
            let backup = Backup::new(&source, &mut destination).map_err(|e| e.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                match backup.step(128).map_err(|e| e.to_string())? {
                    StepResult::Done => break,
                    StepResult::More if Instant::now() < deadline => {}
                    _ => return Err("SQLite backup locked or exceeded 30 second limit".into()),
                }
            }
        }
        check_integrity(&destination)?;
        destination.close().map_err(|(_, e)| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        // No-clobber publication; a failed/aborted snapshot stays .partial.
        fs::hard_link(&partial, &backup_path).map_err(|e| e.to_string())?;
        fs::remove_file(&partial).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        fs::File::open(path.parent().ok_or("SQLite parent unavailable")?)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    result.map_err(|e| {
        format!("Required pre-migration SQLite backup failed: {e}. Upgrade not applied.")
    })
}

#[derive(Debug, PartialEq, Eq)]
struct Column {
    name: String,
    kind: String,
    required: bool,
    default: Option<String>,
    pk: i64,
}

fn columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<Column>> {
    let mut stmt =
        conn.prepare("SELECT name, type, \"notnull\", dflt_value, pk FROM pragma_table_info(?1)")?;
    let rows = stmt.query_map([table], |r| {
        Ok(Column {
            name: r.get(0)?,
            kind: r.get::<_, String>(1)?.to_ascii_uppercase(),
            required: r.get(2)?,
            default: r.get(3)?,
            pk: r.get(4)?,
        })
    })?;
    rows.collect()
}

fn foreign_keys(conn: &Connection, table: &str) -> rusqlite::Result<Vec<Vec<String>>> {
    let mut stmt = conn.prepare("SELECT \"table\", \"from\", \"to\", on_update, on_delete FROM pragma_foreign_key_list(?1) ORDER BY id, seq")?;
    let rows = stmt.query_map([table], |r| (0..5).map(|i| r.get(i)).collect())?;
    rows.collect()
}

fn indexes(
    conn: &Connection,
    table: &str,
) -> rusqlite::Result<BTreeMap<String, (bool, Vec<String>)>> {
    let mut stmt = conn.prepare("SELECT name, \"unique\" FROM pragma_index_list(?1)")?;
    let names = stmt
        .query_map([table], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    names
        .into_iter()
        .map(|(name, unique)| {
            let mut stmt = conn.prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?;
            let fields = stmt
                .query_map([&name], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((name, (unique, fields)))
        })
        .collect()
}

// Only schema metadata is inspected on ordinary starts. Full integrity/data
// checks are reserved for an actual upgrade.
fn validate_layout(conn: &Connection, version: i64) -> Result<(), String> {
    let reference = Connection::open_in_memory().map_err(|e| e.to_string())?;
    adopt_legacy_layout(&reference).map_err(|e| e.to_string())?;
    if version >= 2 {
        reference
            .execute_batch(RECEIPTS_SQL)
            .map_err(|e| e.to_string())?;
    }
    reference
        .execute_batch("CREATE TABLE app_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
        .map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT name, type, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' AND type != 'index'")
        .map_err(|e| format!("SQLite schema inspection failed: {e}"))?;
    let objects = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    let tables: HashSet<&str> = objects.iter().map(|(name, _, _)| name.as_str()).collect();
    if version > 0 {
        for name in [
            "app_config",
            "history_entries",
            "link_dump_settings",
            "link_dump_secrets",
            "transcriptions",
            "captions",
        ] {
            if !tables.contains(name) {
                return Err(format!(
                    "Inconsistent schema {version}: missing table {name}; database preserved"
                ));
            }
        }
        if version >= 2
            && (!tables.contains("legacy_imports") || !tables.contains("job_completions"))
        {
            return Err(
                "Inconsistent schema 2: missing completion/import receipts; database preserved"
                    .into(),
            );
        }
    } else if (tables.contains("captions") || tables.contains("transcriptions"))
        && !tables.contains("history_entries")
    {
        return Err(
            "Unknown legacy schema: metadata without history table; database preserved".into(),
        );
    }
    for (table, kind, sql) in objects {
        let fail = || {
            format!(
                "Unknown/inconsistent SQLite schema {version}: object {table}; database preserved"
            )
        };
        if kind != "table" {
            return Err(fail());
        }
        let expected = columns(&reference, &table).map_err(|e| e.to_string())?;
        if expected.is_empty() {
            return Err(fail());
        }
        let actual = columns(conn, &table).map_err(|e| e.to_string())?;
        let mut actual_names = HashSet::new();
        for column in &actual {
            // Existing code already prefers save_captions when both names
            // exist. Validate the inactive alias, but retain it and its value.
            let retained_caption_alias = table == "app_config"
                && column.name == "save_instagram_captions"
                && actual.iter().any(|c| c.name == "save_captions");
            let legacy_caption =
                version == 0 && table == "app_config" && column.name == "save_instagram_captions";
            let compare_name = if legacy_caption || retained_caption_alias {
                "save_captions"
            } else {
                &column.name
            };
            let Some(wanted) = expected.iter().find(|c| c.name == compare_name) else {
                return Err(fail());
            };
            let retained_history_version = table == "history_entries"
                && column.name == "pinefetch_version"
                && column.required
                && column.default.as_deref() == Some(LEGACY_HISTORY_VERSION_SQL)
                && !wanted.required
                && wanted.default.is_none();
            if column.kind != wanted.kind
                || column.pk != wanted.pk
                || (!retained_history_version
                    && (column.required != wanted.required || column.default != wanted.default))
            {
                return Err(format!("{} (column {})", fail(), column.name));
            }
            if !retained_caption_alias && !actual_names.insert(compare_name.to_string()) {
                return Err(fail());
            }
        }
        for column in &expected {
            // Historical upgrades added these nullable fields/settings. Accept
            // recognizable interrupted legacy subsets too, without rebuilding.
            let legacy_optional = version == 0
                && match table.as_str() {
                    "history_entries" => !matches!(
                        column.name.as_str(),
                        "id" | "url" | "created_at" | "completed_at"
                    ),
                    "app_config" => matches!(
                        column.name.as_str(),
                        "notifications_enabled"
                            | "faster_whisper_model"
                            | "download_video_with_transcript"
                            | "save_captions"
                            | "save_thumbnails"
                            | "legacy_config_json_migrated"
                    ),
                    "transcriptions" => column.name == "language",
                    "captions" => column.name == "sha256",
                    _ => false,
                };
            if !legacy_optional && !actual_names.contains(&column.name) {
                return Err(fail());
            }
        }
        if foreign_keys(conn, &table).map_err(|e| e.to_string())?
            != foreign_keys(&reference, &table).map_err(|e| e.to_string())?
        {
            return Err(format!("{} (foreign keys)", fail()));
        }
        let wanted_indexes = indexes(&reference, &table).map_err(|e| e.to_string())?;
        let actual_indexes = indexes(conn, &table).map_err(|e| e.to_string())?;
        for (name, definition) in &actual_indexes {
            if wanted_indexes.get(name) != Some(definition) {
                return Err(format!("{} (index {name})", fail()));
            }
        }
        for (name, (unique, fields)) in wanted_indexes {
            if (version > 0 || unique) && actual_indexes.get(&name) != Some(&(unique, fields)) {
                return Err(format!("{} (missing index {name})", fail()));
            }
        }
        let canonical: String = sql
            .chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect();
        if matches!(table.as_str(), "app_config" | "link_dump_settings")
            && !canonical.contains("check(id=1)")
        {
            return Err(format!("{} (singleton constraint)", fail()));
        }
        if table == "transcriptions"
            && !canonical.contains("check(\"type\"in('text','textwithtimestamps'))")
        {
            return Err(format!("{} (transcript type constraint)", fail()));
        }
        if table == "job_completions"
            && !canonical.contains("check(statein('processing','output_ready','complete'))")
        {
            return Err(format!("{} (completion state constraint)", fail()));
        }
        if version < SCHEMA_VERSION {
            // SQLite's historical TEXT PRIMARY KEY permits NULL. Refuse
            // conflicting legacy identities rather than guessing a new ID.
            for column in actual.iter().filter(|c| c.pk > 0) {
                let query = format!(
                    "SELECT EXISTS(SELECT 1 FROM \"{table}\" WHERE \"{}\" IS NULL)",
                    column.name
                );
                let bad: bool = conn
                    .query_row(&query, [], |r| r.get(0))
                    .map_err(|e| e.to_string())?;
                if bad {
                    return Err(format!("{} (null identity)", fail()));
                }
            }
        }
    }
    Ok(())
}

fn adopt_legacy_layout(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"

        CREATE TABLE IF NOT EXISTS link_dump_settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            server_enabled INTEGER NOT NULL DEFAULT 1,
            host TEXT NOT NULL DEFAULT '127.0.0.1',
            port INTEGER NOT NULL DEFAULT 2255,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        INSERT OR IGNORE INTO link_dump_settings (
            id,
            server_enabled,
            host,
            port,
            created_at,
            updated_at
        ) VALUES (
            1,
            1,
            '127.0.0.1',
            2255,
            datetime('now'),
            datetime('now')
        );

        CREATE TABLE IF NOT EXISTS app_config (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            yt_dlp_path TEXT,
            default_output_dir TEXT,
            selected_preset_key TEXT,
            faster_whisper_model TEXT NOT NULL DEFAULT 'base',
            download_video_with_transcript INTEGER NOT NULL DEFAULT 0,
            save_captions INTEGER NOT NULL DEFAULT 0,
            save_thumbnails INTEGER NOT NULL DEFAULT 0,
            magic_import_enabled INTEGER NOT NULL DEFAULT 1,
            cut_at_timestamp_enabled INTEGER NOT NULL DEFAULT 1,
            last_download_url TEXT,
            legacy_config_json_migrated INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        INSERT OR IGNORE INTO app_config (
            id,
            yt_dlp_path,
            default_output_dir,
            selected_preset_key,
            magic_import_enabled,
            cut_at_timestamp_enabled,
            last_download_url,
            created_at,
            updated_at
        ) VALUES (
            1,
            NULL,
            NULL,
            'best',
            1,
            1,
            NULL,
            datetime('now'),
            datetime('now')
        );

        CREATE TABLE IF NOT EXISTS link_dump_secrets (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            secret_hash TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL,
            last_used_at TEXT,
            revoked_at TEXT,
            deleted_at TEXT
        );


        CREATE TABLE IF NOT EXISTS history_entries (
            id TEXT PRIMARY KEY,
            url TEXT NOT NULL,
            title TEXT,
            uploader TEXT,
            filename TEXT,
            thumbnail TEXT,
            upload_date TEXT,
            timestamp INTEGER,
            duration_seconds INTEGER,
            file_size_bytes INTEGER,
            sha256 TEXT,
            medium TEXT,
            source TEXT,
            platform TEXT,
            output_path TEXT,
            pinefetch_version TEXT,
            created_at INTEGER NOT NULL,
            completed_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS transcriptions (
            id TEXT PRIMARY KEY,
            history_entry_id TEXT NOT NULL UNIQUE,
            text TEXT NOT NULL,
            "type" TEXT NOT NULL CHECK ("type" IN ('text', 'text with timestamps')),
            language TEXT,
            FOREIGN KEY (history_entry_id) REFERENCES history_entries(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS captions (
            history_entry_id TEXT NOT NULL,
            media_path TEXT NOT NULL,
            caption_path TEXT NOT NULL,
            text TEXT NOT NULL,
            sha256 TEXT,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (history_entry_id, media_path),
            FOREIGN KEY (history_entry_id) REFERENCES history_entries(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_link_dump_secrets_active
            ON link_dump_secrets(revoked_at, deleted_at);

        CREATE INDEX IF NOT EXISTS idx_history_entries_completed_at
            ON history_entries(completed_at, created_at);

        "#,
    )?;

    ensure_app_config_notifications_enabled_column(conn)?;
    ensure_app_config_faster_whisper_model_column(conn)?;
    ensure_app_config_download_video_with_transcript_column(conn)?;
    ensure_app_config_save_captions_column(conn)?;
    ensure_app_config_save_thumbnails_column(conn)?;
    ensure_app_config_legacy_migration_column(conn)?;
    for name in [
        "title",
        "filename",
        "thumbnail",
        "upload_date",
        "platform",
        "output_path",
    ] {
        ensure_history_entries_text_column(conn, name)?;
    }
    ensure_history_entries_timestamp_column(conn)?;
    ensure_history_entries_duration_seconds_column(conn)?;
    ensure_history_entries_file_size_bytes_column(conn)?;
    ensure_history_entries_text_column(conn, "sha256")?;
    ensure_history_entries_text_column(conn, "uploader")?;
    ensure_history_entries_text_column(conn, "medium")?;
    ensure_history_entries_text_column(conn, "source")?;
    ensure_history_entries_text_column(conn, "pinefetch_version")?;
    ensure_transcriptions_language_column(conn)?;
    ensure_captions_sha256_column(conn)?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_history_entries_sha256 ON history_entries(sha256);
         CREATE INDEX IF NOT EXISTS idx_captions_sha256 ON captions(sha256);",
    )?;
    backfill_history_sources(conn)?;
    Ok(())
}

fn ensure_transcriptions_language_column(conn: &Connection) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('transcriptions') WHERE name = 'language')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute("ALTER TABLE transcriptions ADD COLUMN language TEXT", [])?;
    }
    Ok(())
}

fn ensure_app_config_notifications_enabled_column(conn: &Connection) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('app_config') WHERE name = 'notifications_enabled')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute(
            "ALTER TABLE app_config ADD COLUMN notifications_enabled INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

fn ensure_app_config_faster_whisper_model_column(conn: &Connection) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('app_config') WHERE name = 'faster_whisper_model')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute(
            "ALTER TABLE app_config ADD COLUMN faster_whisper_model TEXT NOT NULL DEFAULT 'base'",
            [],
        )?;
    }
    Ok(())
}

fn ensure_app_config_download_video_with_transcript_column(
    conn: &Connection,
) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('app_config') WHERE name = 'download_video_with_transcript')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute(
            "ALTER TABLE app_config ADD COLUMN download_video_with_transcript INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

fn ensure_app_config_save_captions_column(conn: &Connection) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('app_config') WHERE name = 'save_captions')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        let has_instagram_column: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('app_config') WHERE name = 'save_instagram_captions')",
            [],
            |row| row.get(0),
        )?;
        if has_instagram_column {
            conn.execute(
                "ALTER TABLE app_config RENAME COLUMN save_instagram_captions TO save_captions",
                [],
            )?;
        } else {
            conn.execute(
                "ALTER TABLE app_config ADD COLUMN save_captions INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
    }
    Ok(())
}

fn ensure_app_config_save_thumbnails_column(conn: &Connection) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('app_config') WHERE name = 'save_thumbnails')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute(
            "ALTER TABLE app_config ADD COLUMN save_thumbnails INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

fn ensure_app_config_legacy_migration_column(conn: &Connection) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('app_config') WHERE name = 'legacy_config_json_migrated')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute(
            "ALTER TABLE app_config ADD COLUMN legacy_config_json_migrated INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }

    let already_migrated: i64 = conn.query_row(
        "SELECT legacy_config_json_migrated FROM app_config WHERE id = 1",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if already_migrated != 0 {
        return Ok(());
    }

    let has_legacy_meta: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'app_meta')",
        [],
        |row| row.get(0),
    )?;
    if has_legacy_meta {
        let migrated: Option<String> = conn
            .query_row(
                "SELECT value FROM app_meta WHERE key = ?1",
                params![LEGACY_CONFIG_MIGRATION_KEY],
                |row| row.get(0),
            )
            .optional()?;
        if migrated.as_deref() == Some("1") {
            conn.execute(
                "UPDATE app_config SET legacy_config_json_migrated = 1 WHERE id = 1",
                [],
            )?;
        }
    }
    Ok(())
}

fn backfill_history_sources(conn: &Connection) -> rusqlite::Result<()> {
    let entries = {
        let mut stmt = conn.prepare(
            "SELECT id, url FROM history_entries WHERE source IS NULL OR TRIM(source) = ''",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    for (id, url) in entries {
        if let Some(source) = source_from_url(&url) {
            conn.execute(
                "UPDATE history_entries SET source = ?1 WHERE id = ?2",
                params![source, id],
            )?;
        }
    }

    Ok(())
}

fn ensure_history_entries_timestamp_column(conn: &Connection) -> rusqlite::Result<()> {
    ensure_history_entries_integer_column(conn, "timestamp")
}

fn ensure_history_entries_duration_seconds_column(conn: &Connection) -> rusqlite::Result<()> {
    ensure_history_entries_integer_column(conn, "duration_seconds")
}

fn ensure_history_entries_file_size_bytes_column(conn: &Connection) -> rusqlite::Result<()> {
    ensure_history_entries_integer_column(conn, "file_size_bytes")
}

fn ensure_history_entries_integer_column(
    conn: &Connection,
    column_name: &str,
) -> rusqlite::Result<()> {
    ensure_history_entries_column(conn, column_name, "INTEGER")
}

fn ensure_history_entries_text_column(
    conn: &Connection,
    column_name: &str,
) -> rusqlite::Result<()> {
    ensure_history_entries_column(conn, column_name, "TEXT")
}

fn ensure_captions_sha256_column(conn: &Connection) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('captions') WHERE name = 'sha256')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute("ALTER TABLE captions ADD COLUMN sha256 TEXT", [])?;
    }
    Ok(())
}

fn ensure_history_entries_column(
    conn: &Connection,
    column_name: &str,
    column_type: &str,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(history_entries)")?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        if column? == column_name {
            return Ok(());
        }
    }
    drop(stmt);

    conn.execute(
        &format!("ALTER TABLE history_entries ADD COLUMN {column_name} {column_type}"),
        [],
    )?;
    Ok(())
}
