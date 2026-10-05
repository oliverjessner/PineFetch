-- Synthetic empty schema extracted from v1.8.0 src-tauri/src/main.rs.
PRAGMA foreign_keys = ON;

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
            magic_import_enabled INTEGER NOT NULL DEFAULT 1,
            cut_at_timestamp_enabled INTEGER NOT NULL DEFAULT 1,
            last_download_url TEXT,
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

        CREATE TABLE IF NOT EXISTS app_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
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

        DROP TABLE IF EXISTS link_dump_request_log;

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
            medium TEXT,
            source TEXT,
            platform TEXT,
            output_path TEXT,
            created_at INTEGER NOT NULL,
            completed_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS transcriptions (
            id TEXT PRIMARY KEY,
            history_entry_id TEXT NOT NULL UNIQUE,
            text TEXT NOT NULL,
            "type" TEXT NOT NULL CHECK ("type" IN ('text', 'text with timestamps')),
            FOREIGN KEY (history_entry_id) REFERENCES history_entries(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_link_dump_secrets_active
            ON link_dump_secrets(revoked_at, deleted_at);

        CREATE INDEX IF NOT EXISTS idx_history_entries_completed_at
            ON history_entries(completed_at, created_at);
