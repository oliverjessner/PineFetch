//! Fallible persistence initialization runs before the native event loop.
use crate::config::{import_legacy_config, load_config_from_db};
use crate::database;
use crate::history::import_legacy_history;
use crate::AppState;
use std::fs;
use std::path::Path;

pub(super) fn load_for_identifier(identifier: &str) -> Result<AppState, String> {
    // These are the same dirs/identifier paths used by Tauri's desktop resolver.
    let data_dir = dirs::data_dir()
        .ok_or("Data directory unavailable")?
        .join(identifier);
    let config_dir = dirs::config_dir()
        .ok_or("Config directory unavailable")?
        .join(identifier);
    load_from_dirs(&data_dir, &config_dir)
}

pub(super) fn load_from_dirs(data_dir: &Path, config_dir: &Path) -> Result<AppState, String> {
    fs::create_dir_all(data_dir).map_err(|e| format!("Data dir create failed: {e}"))?;
    let db = database::open(&data_dir.join("pinefetch.sqlite"))?;
    fs::create_dir_all(config_dir).map_err(|e| format!("Config dir create failed: {e}"))?;
    import_legacy_config(&db, &config_dir.join("config.json"))?;
    let config = load_config_from_db(&db)?;
    import_legacy_history(&db, &data_dir.join("history.json"))?;
    Ok(AppState::new(config, db))
}
