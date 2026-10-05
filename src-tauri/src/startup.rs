//! Desktop startup paths and legacy-import wiring; repositories receive paths.
use crate::config::import_legacy_config;
use crate::database;
use crate::history::import_legacy_history;
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|_| "Config directory unavailable")?;
    fs::create_dir_all(&dir).map_err(|e| format!("Config dir create failed: {e}"))?;
    Ok(dir.join("config.json"))
}

pub(super) fn migrate_legacy_config_json(app: &AppHandle, conn: &Connection) -> Result<(), String> {
    import_legacy_config(conn, &config_path(app)?)
}

fn legacy_history_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "Data directory unavailable")?;
    fs::create_dir_all(&dir).map_err(|e| format!("Data dir create failed: {e}"))?;
    Ok(dir.join("history.json"))
}

pub(super) fn migrate_legacy_history_json(
    app: &AppHandle,
    state: &crate::database::Database,
) -> Result<(), String> {
    let path = legacy_history_path(app)?;
    let conn = state.lock().map_err(|_| "SQLite lock poisoned")?;
    import_legacy_history(&conn, &path)
}

fn link_dump_db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "Data directory unavailable")?;
    fs::create_dir_all(&dir).map_err(|e| format!("Data dir create failed: {e}"))?;
    Ok(dir.join("pinefetch.sqlite"))
}

pub(crate) fn open_link_dump_db(app: &AppHandle) -> Result<Connection, String> {
    let path = link_dump_db_path(app)?;
    database::open(&path)
}
