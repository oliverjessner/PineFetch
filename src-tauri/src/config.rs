use super::*;

pub(super) fn normalize_app_config(mut config: AppConfig) -> AppConfig {
    config.selected_preset_key = Some(normalize_download_preset_key(
        config.selected_preset_key.as_deref(),
    ));
    config.faster_whisper_model = normalize_faster_whisper_model(&config.faster_whisper_model);
    config
}

pub(super) fn normalize_faster_whisper_model(model: &str) -> String {
    let model = model.trim();
    FASTER_WHISPER_MODELS
        .iter()
        .find(|candidate| **candidate == model)
        .copied()
        .unwrap_or(DEFAULT_FASTER_WHISPER_MODEL)
        .to_string()
}

#[tauri::command]
pub(super) fn get_config(state: State<AppState>) -> Result<AppConfig, String> {
    let cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
    Ok(cfg.clone())
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct ConfigPatch {
    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    yt_dlp_path: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    default_output_dir: Option<Option<String>>,
    selected_preset_key: Option<String>,
    faster_whisper_model: Option<String>,
    download_video_with_transcript: Option<bool>,
    #[serde(alias = "save_instagram_captions")]
    save_captions: Option<bool>,
    save_thumbnails: Option<bool>,
    magic_import_enabled: Option<bool>,
    cut_at_timestamp_enabled: Option<bool>,
    notifications_enabled: Option<bool>,
}

fn deserialize_nullable_patch<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

fn apply_config_patch(config: &mut AppConfig, changes: ConfigPatch) {
    if let Some(value) = changes.yt_dlp_path {
        config.yt_dlp_path = value;
    }
    if let Some(value) = changes.default_output_dir {
        config.default_output_dir = value;
    }
    if let Some(value) = changes.selected_preset_key {
        config.selected_preset_key = Some(value);
    }
    if let Some(value) = changes.faster_whisper_model {
        config.faster_whisper_model = value;
    }
    if let Some(value) = changes.download_video_with_transcript {
        config.download_video_with_transcript = value;
    }
    if let Some(value) = changes.save_captions {
        config.save_captions = value;
    }
    if let Some(value) = changes.save_thumbnails {
        config.save_thumbnails = value;
    }
    if let Some(value) = changes.magic_import_enabled {
        config.magic_import_enabled = value;
    }
    if let Some(value) = changes.cut_at_timestamp_enabled {
        config.cut_at_timestamp_enabled = value;
    }
    if let Some(value) = changes.notifications_enabled {
        config.notifications_enabled = value;
    }
}

#[tauri::command]
pub(super) fn patch_config(
    state: State<AppState>,
    changes: ConfigPatch,
) -> Result<AppConfig, String> {
    update_config(state.inner(), |config| apply_config_patch(config, changes))
}

pub(super) fn update_config(
    state: &AppState,
    change: impl FnOnce(&mut AppConfig),
) -> Result<AppConfig, String> {
    let mut current = state.config.lock().map_err(|_| "Config lock poisoned")?;
    let mut next = current.clone();
    change(&mut next);
    let next = normalize_app_config(next);
    save_config_to_db(state, &next)?;
    *current = next.clone();
    Ok(next)
}

#[tauri::command]
pub(super) fn set_selected_preset_key(
    state: State<AppState>,
    preset_key: String,
) -> Result<AppConfig, String> {
    let selected_preset_key = normalize_download_preset_key(Some(&preset_key));
    update_config(state.inner(), |cfg| {
        cfg.selected_preset_key = Some(selected_preset_key)
    })
}

#[tauri::command]
pub(super) fn set_save_captions(
    state: State<AppState>,
    enabled: bool,
) -> Result<AppConfig, String> {
    update_config(state.inner(), |cfg| cfg.save_captions = enabled)
}

#[tauri::command]
pub(super) fn cache_last_download_url(state: State<AppState>, url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    update_config(state.inner(), |cfg| {
        cfg.last_download_url = Some(trimmed.to_string())
    })?;
    Ok(())
}

pub(super) fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|_| "Config directory unavailable")?;
    fs::create_dir_all(&dir).map_err(|e| format!("Config dir create failed: {e}"))?;
    Ok(dir.join("config.json"))
}

pub(super) fn load_legacy_config_json(app: &AppHandle) -> Option<AppConfig> {
    if let Ok(path) = config_path(app) {
        if let Ok(raw) = fs::read_to_string(path) {
            if let Ok(cfg) = serde_json::from_str::<AppConfig>(&raw) {
                return Some(normalize_app_config(cfg));
            }
        }
    }
    None
}

pub(super) fn get_app_config_from_conn(conn: &Connection) -> rusqlite::Result<AppConfig> {
    conn.query_row(
        "SELECT yt_dlp_path, default_output_dir, selected_preset_key, faster_whisper_model, download_video_with_transcript, magic_import_enabled, cut_at_timestamp_enabled, last_download_url, notifications_enabled, save_captions, save_thumbnails
         FROM app_config
         WHERE id = 1",
        [],
        |row| {
            Ok(normalize_app_config(AppConfig {
                yt_dlp_path: row.get(0)?,
                default_output_dir: row.get(1)?,
                selected_preset_key: row.get(2)?,
                faster_whisper_model: row.get(3)?,
                download_video_with_transcript: row.get::<_, i64>(4)? != 0,
                magic_import_enabled: row.get::<_, i64>(5)? != 0,
                cut_at_timestamp_enabled: row.get::<_, i64>(6)? != 0,
                last_download_url: row.get(7)?,
                notifications_enabled: row.get::<_, i64>(8)? != 0,
                save_captions: row.get::<_, i64>(9)? != 0,
                save_thumbnails: row.get::<_, i64>(10)? != 0,
            }))
        },
    )
}

pub(super) fn load_config_from_db(conn: &Connection) -> Result<AppConfig, String> {
    get_app_config_from_conn(conn).map_err(|e| format!("Config read failed: {e}"))
}

pub(super) fn upsert_app_config_in_conn(
    conn: &Connection,
    config: &AppConfig,
) -> rusqlite::Result<()> {
    let config = normalize_app_config(config.clone());
    conn.execute(
        "INSERT INTO app_config (
            id,
            yt_dlp_path,
            default_output_dir,
            selected_preset_key,
            faster_whisper_model,
            download_video_with_transcript,
            magic_import_enabled,
            cut_at_timestamp_enabled,
            last_download_url,
            notifications_enabled,
            save_captions,
            save_thumbnails,
            created_at,
            updated_at
        ) VALUES (
            1,
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            ?8,
            ?9,
            ?10,
            ?11,
            datetime('now'),
            datetime('now')
        )
        ON CONFLICT(id) DO UPDATE SET
            yt_dlp_path = excluded.yt_dlp_path,
            default_output_dir = excluded.default_output_dir,
            selected_preset_key = excluded.selected_preset_key,
            faster_whisper_model = excluded.faster_whisper_model,
            download_video_with_transcript = excluded.download_video_with_transcript,
            magic_import_enabled = excluded.magic_import_enabled,
            cut_at_timestamp_enabled = excluded.cut_at_timestamp_enabled,
            last_download_url = excluded.last_download_url,
            notifications_enabled = excluded.notifications_enabled,
            save_captions = excluded.save_captions,
            save_thumbnails = excluded.save_thumbnails,
            updated_at = datetime('now')",
        params![
            config.yt_dlp_path,
            config.default_output_dir,
            config.selected_preset_key,
            config.faster_whisper_model,
            if config.download_video_with_transcript {
                1
            } else {
                0
            },
            if config.magic_import_enabled { 1 } else { 0 },
            if config.cut_at_timestamp_enabled {
                1
            } else {
                0
            },
            config.last_download_url,
            config.notifications_enabled,
            config.save_captions,
            config.save_thumbnails,
        ],
    )?;
    Ok(())
}

pub(super) fn save_config_to_db(state: &AppState, config: &AppConfig) -> Result<(), String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    upsert_app_config_in_conn(&conn, config).map_err(|e| format!("Config write failed: {e}"))
}

pub(super) fn migrate_legacy_config_json(app: &AppHandle, conn: &Connection) -> Result<(), String> {
    let already_migrated: bool = conn
        .query_row(
            "SELECT legacy_config_json_migrated FROM app_config WHERE id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| format!("Config migration check failed: {e}"))?
        != 0;

    if already_migrated {
        return Ok(());
    }

    if let Some(config) = load_legacy_config_json(app) {
        upsert_app_config_in_conn(conn, &config)
            .map_err(|e| format!("Config migration failed: {e}"))?;
    }

    conn.execute(
        "UPDATE app_config SET legacy_config_json_migrated = 1 WHERE id = 1",
        [],
    )
    .map_err(|e| format!("Config migration marker failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_patch_preserves_newer_url_and_preset_and_clears_nullable_path() {
        let conn = Connection::open_in_memory().unwrap();
        run_link_dump_migrations(&conn).unwrap();
        let state = AppState::new(
            AppConfig {
                yt_dlp_path: Some("/old/yt-dlp".to_string()),
                default_output_dir: Some("/old/downloads".to_string()),
                selected_preset_key: Some("audio_mp3".to_string()),
                last_download_url: Some("https://example.com/new".to_string()),
                ..AppConfig::default()
            },
            conn,
        );
        let changes: ConfigPatch = serde_json::from_value(json!({
            "yt_dlp_path": null,
            "notifications_enabled": true
        }))
        .unwrap();

        let updated = update_config(&state, |config| apply_config_patch(config, changes)).unwrap();
        let persisted = load_config_from_db(&state.db.lock().unwrap()).unwrap();

        assert_eq!(updated.yt_dlp_path, None);
        assert_eq!(
            updated.default_output_dir.as_deref(),
            Some("/old/downloads")
        );
        assert_eq!(updated.selected_preset_key.as_deref(), Some("audio_mp3"));
        assert_eq!(
            updated.last_download_url.as_deref(),
            Some("https://example.com/new")
        );
        assert!(updated.notifications_enabled);
        assert_eq!(persisted.last_download_url, updated.last_download_url);
        assert_eq!(persisted.selected_preset_key, updated.selected_preset_key);
        assert_eq!(persisted.yt_dlp_path, None);

        let clear_output: ConfigPatch =
            serde_json::from_value(json!({ "default_output_dir": null })).unwrap();
        let updated =
            update_config(&state, |config| apply_config_patch(config, clear_output)).unwrap();
        assert_eq!(updated.default_output_dir, None);
        assert_eq!(
            updated.last_download_url.as_deref(),
            Some("https://example.com/new")
        );
    }
}
