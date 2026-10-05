use crate::database;
use crate::hashing::bytes_to_hex;
use crate::models::GeneratedLinkDumpSecret;
use crate::models::LinkDumpSecretView;
use crate::models::LinkDumpSettings;
use crate::models::LinkDumpSettingsPatch;
use crate::models::ValidSecretResult;
use crate::video_urls::normalize_link_dump_host;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rusqlite::params;
use rusqlite::Connection;
use sha2::Digest;
use sha2::Sha256;
use uuid::Uuid;

pub(super) fn get_link_dump_settings_from_conn(
    conn: &Connection,
) -> rusqlite::Result<LinkDumpSettings> {
    conn.query_row(
        "SELECT server_enabled, host, port, created_at, updated_at FROM link_dump_settings WHERE id = 1",
        [],
        |row| {
            Ok(LinkDumpSettings {
                server_enabled: row.get::<_, i64>(0)? != 0,
                host: row.get(1)?,
                port: row.get::<_, i64>(2)? as u16,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        },
    )
}

pub(super) fn get_link_dump_settings(
    state: &crate::database::Database,
) -> Result<LinkDumpSettings, String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    get_link_dump_settings_from_conn(&conn)
        .map_err(|e| format!("Link Dump settings read failed: {e}"))
}

pub(super) fn update_link_dump_settings_in_db(
    state: &crate::database::Database,
    patch: LinkDumpSettingsPatch,
) -> Result<LinkDumpSettings, String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let transaction = database::write_transaction(&conn)
        .map_err(|e| format!("Link Dump settings transaction failed: {e}"))?;
    let mut current = get_link_dump_settings_from_conn(&transaction)
        .map_err(|e| format!("Link Dump settings read failed: {e}"))?;
    if let Some(enabled) = patch.server_enabled {
        current.server_enabled = enabled;
    }
    if let Some(host) = patch.host {
        let trimmed = host.trim();
        if !trimmed.is_empty() {
            current.host = normalize_link_dump_host(trimmed);
        }
    }
    if let Some(port) = patch.port {
        if port == 0 {
            return Err("Port must be between 1 and 65535".to_string());
        }
        current.port = port;
    }

    transaction
        .execute(
            "UPDATE link_dump_settings
         SET server_enabled = ?1, host = ?2, port = ?3, updated_at = datetime('now')
         WHERE id = 1",
            params![
                if current.server_enabled { 1 } else { 0 },
                current.host,
                i64::from(current.port)
            ],
        )
        .map_err(|e| format!("Link Dump settings update failed: {e}"))?;
    let settings = get_link_dump_settings_from_conn(&transaction)
        .map_err(|e| format!("Link Dump settings read failed: {e}"))?;
    transaction
        .commit()
        .map_err(|e| format!("Link Dump settings commit failed: {e}"))?;
    Ok(settings)
}

pub(super) fn list_link_dump_secrets_from_conn(
    conn: &Connection,
) -> rusqlite::Result<Vec<LinkDumpSecretView>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, created_at, last_used_at, revoked_at, deleted_at
         FROM link_dump_secrets
         WHERE deleted_at IS NULL
         ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let revoked_at: Option<String> = row.get(4)?;
        let deleted_at: Option<String> = row.get(5)?;
        let status = if deleted_at.is_some() {
            "deleted"
        } else if revoked_at.is_some() {
            "revoked"
        } else {
            "active"
        };
        Ok(LinkDumpSecretView {
            id: row.get(0)?,
            name: row.get(1)?,
            created_at: row.get(2)?,
            last_used_at: row.get(3)?,
            revoked_at,
            deleted_at,
            status: status.to_string(),
        })
    })?;

    rows.collect()
}

pub(super) fn list_link_dump_secrets(
    state: &crate::database::Database,
) -> Result<Vec<LinkDumpSecretView>, String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    list_link_dump_secrets_from_conn(&conn)
        .map_err(|e| format!("Link Dump secrets read failed: {e}"))
}

pub(super) fn next_link_dump_secret_name(conn: &Connection) -> rusqlite::Result<String> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM link_dump_secrets WHERE deleted_at IS NULL",
        [],
        |row| row.get(0),
    )?;
    Ok(format!("Link Dump Connection {}", count + 1))
}

pub(super) fn generate_link_dump_secret_value() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("Secret generation failed: {e}"))?;
    Ok(format!("pfld_{}", URL_SAFE_NO_PAD.encode(bytes)))
}

pub(super) fn hash_link_dump_secret(secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    bytes_to_hex(&hasher.finalize())
}

pub(super) fn constant_time_eq_str(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let max_len = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();

    for index in 0..max_len {
        let left_byte = left.get(index).copied().unwrap_or(0);
        let right_byte = right.get(index).copied().unwrap_or(0);
        diff |= usize::from(left_byte ^ right_byte);
    }

    diff == 0
}

pub(super) fn create_link_dump_secret_in_db(
    state: &crate::database::Database,
    name: Option<String>,
) -> Result<GeneratedLinkDumpSecret, String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let transaction = database::write_transaction(&conn)
        .map_err(|e| format!("Link Dump secret transaction failed: {e}"))?;
    let clean_name = name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .map(Ok)
        .unwrap_or_else(|| next_link_dump_secret_name(&transaction))
        .map_err(|e| format!("Link Dump name generation failed: {e}"))?;

    let secret = generate_link_dump_secret_value()?;
    let secret_hash = hash_link_dump_secret(&secret);
    let id = Uuid::new_v4().to_string();
    transaction
        .execute(
            "INSERT INTO link_dump_secrets (id, name, secret_hash, created_at)
         VALUES (?1, ?2, ?3, datetime('now'))",
            params![id, clean_name, secret_hash],
        )
        .map_err(|e| format!("Link Dump secret create failed: {e}"))?;

    let connection = transaction
        .query_row(
            "SELECT id, name, created_at, last_used_at, revoked_at, deleted_at
             FROM link_dump_secrets
             WHERE id = ?1",
            params![id],
            |row| {
                Ok(LinkDumpSecretView {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                    last_used_at: row.get(3)?,
                    revoked_at: row.get(4)?,
                    deleted_at: row.get(5)?,
                    status: "active".to_string(),
                })
            },
        )
        .map_err(|e| format!("Link Dump secret read failed: {e}"))?;

    transaction
        .commit()
        .map_err(|e| format!("Link Dump secret commit failed: {e}"))?;
    Ok(GeneratedLinkDumpSecret { secret, connection })
}

pub(super) fn revoke_link_dump_secret_in_db(
    state: &crate::database::Database,
    id: &str,
) -> Result<(), String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    tx.execute(
        "UPDATE link_dump_secrets
         SET revoked_at = COALESCE(revoked_at, datetime('now'))
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
    )
    .map_err(|e| format!("Link Dump secret revoke failed: {e}"))?;
    tx.commit()
        .map_err(|e| format!("Link Dump secret commit failed: {e}"))
}

pub(super) fn delete_link_dump_secret_in_db(
    state: &crate::database::Database,
    id: &str,
) -> Result<(), String> {
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    tx.execute(
        "UPDATE link_dump_secrets
         SET deleted_at = COALESCE(deleted_at, datetime('now'))
         WHERE id = ?1",
        params![id],
    )
    .map_err(|e| format!("Link Dump secret delete failed: {e}"))?;
    tx.commit()
        .map_err(|e| format!("Link Dump secret commit failed: {e}"))
}

pub(super) fn validate_link_dump_secret(
    state: &crate::database::Database,
    secret: Option<&str>,
) -> Result<Option<ValidSecretResult>, String> {
    let Some(secret) = secret.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let candidate_hash = hash_link_dump_secret(secret);
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, secret_hash
             FROM link_dump_secrets
             WHERE revoked_at IS NULL AND deleted_at IS NULL",
        )
        .map_err(|e| format!("Link Dump secret validation failed: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| format!("Link Dump secret validation failed: {e}"))?;

    let mut matched: Option<ValidSecretResult> = None;
    for row in rows {
        let (id, name, stored_hash) =
            row.map_err(|e| format!("Link Dump secret validation failed: {e}"))?;
        if constant_time_eq_str(&candidate_hash, &stored_hash) {
            matched = Some(ValidSecretResult { id, name });
        }
    }

    if let Some(valid) = matched.as_ref() {
        let tx = database::write_transaction(&conn)?;
        let updated = tx.execute(
            "UPDATE link_dump_secrets SET last_used_at = datetime('now') WHERE id = ?1 AND revoked_at IS NULL AND deleted_at IS NULL",
            params![valid.id],
        )
        .map_err(|e| format!("Link Dump secret last-used update failed: {e}"))?;
        if updated != 1 {
            return Ok(None);
        }
        tx.commit()
            .map_err(|e| format!("Link Dump secret last-used commit failed: {e}"))?;
    }

    Ok(matched)
}
