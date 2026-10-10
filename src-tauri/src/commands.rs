use crate::browser_import::restart_link_dump_server_internal;
use crate::browser_import::snapshot_link_dump_server_status;
use crate::config::update_config;
use crate::config_rules::apply_config_patch;
use crate::config_rules::ConfigPatch;
use crate::events::emit_queue_status;
use crate::files::canonical_existing_local_path;
use crate::history::clear_history_entries_in_db;
use crate::history::delete_history_entry_from_db;
use crate::history::get_history_caption_from_db;
use crate::history::get_history_details_from_db;
use crate::history::get_history_stats_from_db;
use crate::history::get_history_transcript_from_db;
use crate::history::search_history_page_from_db;
use crate::link_dump_store::create_link_dump_secret_in_db;
use crate::link_dump_store::delete_link_dump_secret_in_db;
use crate::link_dump_store::get_link_dump_settings;
use crate::link_dump_store::list_link_dump_secrets;
use crate::link_dump_store::revoke_link_dump_secret_in_db;
use crate::link_dump_store::update_link_dump_settings_in_db;
use crate::metadata::load_info_with_yt_dlp;
use crate::models::AppConfig;
use crate::models::DownloadJob;
use crate::models::DownloadRequest;
use crate::models::GeneratedLinkDumpSecret;
use crate::models::HistoryCaptionContent;
use crate::models::HistoryDetails;
use crate::models::HistoryPage;
use crate::models::HistoryStats;
use crate::models::HistoryTranscriptContent;
use crate::models::InfoResponse;
use crate::models::InstalledYtDlpVersion;
use crate::models::LinkDumpOverview;
use crate::models::LinkDumpSecretView;
use crate::models::LinkDumpServerStatus;
use crate::models::LinkDumpSettingsPatch;
use crate::models::QueueStatus;
use crate::models::TxtImportFile;
use crate::presets::normalize_download_preset_key;
use crate::presets::DownloadPreset;
use crate::presets::DOWNLOAD_PRESETS;
use crate::queue::set_queue_paused;
use crate::queue::snapshot_queue_status;
use crate::runtime::resolve_deno_executable;
use crate::runtime::resolve_yt_dlp;
use crate::state::AppState;
use crate::url_rules::validate_download_url;
use crate::worker::enqueue_download_request;
use crate::worker::ensure_worker;
use std::fs;
use tauri::AppHandle;
use tauri::Manager;
use tauri::State;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub(crate) fn get_download_presets() -> Vec<DownloadPreset> {
    DOWNLOAD_PRESETS.to_vec()
}

#[tauri::command]
pub(crate) async fn pick_output_dir(app: AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |path| {
        let _ = tx.send(
            path.and_then(|p| p.into_path().ok())
                .map(|p| p.to_string_lossy().to_string()),
        );
    });
    tauri::async_runtime::spawn_blocking(move || rx.recv())
        .await
        .map_err(|_| "Dialog task failed".to_string())?
        .map_err(|_| "Dialog closed".to_string())
}

#[tauri::command]
pub(crate) async fn pick_txt_file(app: AppHandle) -> Result<Option<TxtImportFile>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .add_filter("Text", &["txt"])
        .pick_file(move |path| {
            let _ = tx.send(path.and_then(|p| p.into_path().ok()));
        });

    let selected_path = tauri::async_runtime::spawn_blocking(move || rx.recv())
        .await
        .map_err(|_| "Dialog task failed".to_string())?
        .map_err(|_| "Dialog closed".to_string())?;

    let Some(path) = selected_path else {
        return Ok(None);
    };

    let content =
        fs::read_to_string(&path).map_err(|e| format!("TXT file could not be read: {e}"))?;

    Ok(Some(TxtImportFile {
        path: path.to_string_lossy().to_string(),
        content,
    }))
}

#[tauri::command]
pub(crate) fn open_folder(app: AppHandle, path: String) -> Result<(), String> {
    let path =
        canonical_existing_local_path(&path)?.ok_or_else(|| "Path does not exist".to_string())?;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| format!("Open folder failed: {e}"))
}

#[tauri::command]
pub(crate) fn open_file_path(app: AppHandle, path: String) -> Result<bool, String> {
    let Some(path) = canonical_existing_local_path(&path)? else {
        return Ok(false);
    };
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| format!("Open file failed: {e}"))?;
    Ok(true)
}

#[tauri::command]
pub(crate) fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    validate_download_url(&url).map_err(|e| e.to_string())?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("Open URL failed: {e}"))
}

#[tauri::command]
pub(crate) fn read_clipboard_text(app: AppHandle) -> Result<Option<String>, String> {
    app.clipboard()
        .read_text()
        .map(Some)
        .map_err(|e| format!("Clipboard read failed: {e}"))
}

#[tauri::command]
pub(crate) async fn load_info(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> Result<InfoResponse, String> {
    validate_download_url(&url).map_err(|error| error.to_string())?;
    let yt_dlp = resolve_yt_dlp(&app, &state.config)?;
    let deno = resolve_deno_executable(&app);

    let app_handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        load_info_with_yt_dlp(yt_dlp, deno, url, &state.processes)
    })
    .await
    .map_err(|e| format!("Info task failed: {e}"))?
}

#[tauri::command]
pub(crate) fn get_yt_dlp_installed_version(
    app: AppHandle,
    state: State<AppState>,
    path: Option<String>,
) -> Result<InstalledYtDlpVersion, String> {
    crate::runtime::installed_yt_dlp_version(app, &state.config, path, &state.processes)
}

#[tauri::command]
pub(crate) fn get_history(
    state: State<AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
    query: Option<String>,
    search_field: Option<String>,
    source: Option<String>,
) -> Result<HistoryPage, String> {
    let limit = limit.unwrap_or(20).clamp(1, 100);
    let offset = offset.unwrap_or(0);
    search_history_page_from_db(
        &state.db,
        limit,
        offset,
        query.as_deref(),
        search_field.as_deref(),
        source.as_deref(),
    )
}

#[tauri::command]
pub(crate) fn get_history_stats(state: State<AppState>) -> Result<HistoryStats, String> {
    get_history_stats_from_db(&state.db)
}

#[tauri::command]
pub(crate) fn get_history_details(
    state: State<AppState>,
    id: String,
) -> Result<Option<HistoryDetails>, String> {
    get_history_details_from_db(&state.db, &id)
}

#[tauri::command]
pub(crate) fn get_history_transcript(
    state: State<AppState>,
    id: String,
) -> Result<Option<HistoryTranscriptContent>, String> {
    get_history_transcript_from_db(&state.db, &id)
}

#[tauri::command]
pub(crate) fn get_history_caption(
    state: State<AppState>,
    id: String,
    media_path: String,
) -> Result<Option<HistoryCaptionContent>, String> {
    get_history_caption_from_db(&state.db, &id, &media_path)
}

#[tauri::command]
pub(crate) fn remove_history_entry(state: State<AppState>, id: String) -> Result<(), String> {
    delete_history_entry_from_db(&state.db, &id)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn clear_history(state: State<AppState>) -> Result<(), String> {
    clear_history_entries_in_db(&state.db)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn get_queue_status(state: State<AppState>) -> Result<QueueStatus, String> {
    snapshot_queue_status(&state.queue)
}

#[tauri::command]
pub(crate) fn get_queue(state: State<AppState>) -> Result<Vec<DownloadJob>, String> {
    let queue = state
        .queue
        .pending
        .lock()
        .map_err(|_| "Queue lock poisoned")?;
    Ok(queue.iter().cloned().collect())
}

#[tauri::command]
pub(crate) fn set_queue_auto_start(
    app: AppHandle,
    state: State<AppState>,
    enabled: bool,
) -> Result<QueueStatus, String> {
    {
        let mut auto_start = state
            .queue
            .auto_start
            .lock()
            .map_err(|_| "Queue auto-start lock poisoned")?;
        *auto_start = enabled;
    }

    if enabled {
        ensure_worker(&app, state.inner())?;
    }
    emit_queue_status(&app, &state.queue);

    snapshot_queue_status(&state.queue)
}

#[tauri::command]
pub(crate) fn start_queue(app: AppHandle, state: State<AppState>) -> Result<QueueStatus, String> {
    set_queue_paused(&state.queue, false)?;
    ensure_worker(&app, state.inner())?;
    emit_queue_status(&app, &state.queue);
    snapshot_queue_status(&state.queue)
}

#[tauri::command]
pub(crate) fn pause_queue(app: AppHandle, state: State<AppState>) -> Result<QueueStatus, String> {
    set_queue_paused(&state.queue, true)?;
    emit_queue_status(&app, &state.queue);
    snapshot_queue_status(&state.queue)
}

#[tauri::command]
pub(crate) fn resume_queue(app: AppHandle, state: State<AppState>) -> Result<QueueStatus, String> {
    set_queue_paused(&state.queue, false)?;
    ensure_worker(&app, state.inner())?;
    emit_queue_status(&app, &state.queue);
    snapshot_queue_status(&state.queue)
}

#[tauri::command]
pub(crate) fn enqueue_download(
    app: AppHandle,
    state: State<AppState>,
    request: DownloadRequest,
) -> Result<String, String> {
    enqueue_download_request(&app, state.inner(), request)
}

#[tauri::command]
pub(crate) fn cancel_download(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<(), String> {
    crate::worker::cancel_download_job(app, state.inner(), id)
}

#[tauri::command]
pub(crate) fn get_config(state: State<AppState>) -> Result<AppConfig, String> {
    let cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
    Ok(cfg.clone())
}

#[tauri::command]
pub(crate) fn patch_config(
    state: State<AppState>,
    changes: ConfigPatch,
) -> Result<AppConfig, String> {
    update_config(&state.config, |config| apply_config_patch(config, changes))
}

#[tauri::command]
pub(crate) fn set_selected_preset_key(
    state: State<AppState>,
    preset_key: String,
) -> Result<AppConfig, String> {
    let selected_preset_key = normalize_download_preset_key(Some(&preset_key));
    update_config(&state.config, |cfg| {
        cfg.selected_preset_key = Some(selected_preset_key)
    })
}

#[tauri::command]
pub(crate) fn set_save_captions(
    state: State<AppState>,
    enabled: bool,
) -> Result<AppConfig, String> {
    update_config(&state.config, |cfg| cfg.save_captions = enabled)
}

#[tauri::command]
pub(crate) fn cache_last_download_url(state: State<AppState>, url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    update_config(&state.config, |cfg| {
        cfg.last_download_url = Some(trimmed.to_string())
    })?;
    Ok(())
}

#[tauri::command]
pub(crate) fn get_link_dump_overview(state: State<AppState>) -> Result<LinkDumpOverview, String> {
    Ok(LinkDumpOverview {
        settings: get_link_dump_settings(&state.db)?,
        secrets: list_link_dump_secrets(&state.db)?,
        server_status: snapshot_link_dump_server_status(state.inner()),
    })
}

#[tauri::command]
pub(crate) fn update_link_dump_settings(
    app: AppHandle,
    state: State<AppState>,
    patch: LinkDumpSettingsPatch,
) -> Result<LinkDumpOverview, String> {
    let settings = update_link_dump_settings_in_db(&state.db, patch)?;
    let server_status = restart_link_dump_server_internal(&app, state.inner())?;
    Ok(LinkDumpOverview {
        settings,
        secrets: list_link_dump_secrets(&state.db)?,
        server_status,
    })
}

#[tauri::command]
pub(crate) fn create_link_dump_secret(
    state: State<AppState>,
    name: Option<String>,
) -> Result<GeneratedLinkDumpSecret, String> {
    create_link_dump_secret_in_db(&state.db, name)
}

#[tauri::command]
pub(crate) fn revoke_link_dump_secret(
    state: State<AppState>,
    id: String,
) -> Result<Vec<LinkDumpSecretView>, String> {
    revoke_link_dump_secret_in_db(&state.db, &id)?;
    list_link_dump_secrets(&state.db)
}

#[tauri::command]
pub(crate) fn delete_link_dump_secret(
    state: State<AppState>,
    id: String,
) -> Result<Vec<LinkDumpSecretView>, String> {
    delete_link_dump_secret_in_db(&state.db, &id)?;
    list_link_dump_secrets(&state.db)
}

#[tauri::command]
pub(crate) fn restart_link_dump_server(
    app: AppHandle,
    state: State<AppState>,
) -> Result<LinkDumpServerStatus, String> {
    restart_link_dump_server_internal(&app, state.inner())
}
