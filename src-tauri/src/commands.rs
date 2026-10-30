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
use crate::history_query::{read_page, HistoryFilter};
use crate::link_dump_store::create_link_dump_secret_in_db;
use crate::link_dump_store::delete_link_dump_secret_in_db;
use crate::link_dump_store::get_link_dump_settings;
use crate::link_dump_store::list_link_dump_secrets;
use crate::link_dump_store::revoke_link_dump_secret_in_db;
use crate::link_dump_store::update_link_dump_settings_in_db;
use crate::metadata::load_info_with_yt_dlp;
use crate::models::AppConfig;
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
    let url = crate::url_rules::normalize_video_input(
        &url,
        state
            .config
            .lock()
            .map_err(|_| "Config lock poisoned")?
            .id_import_enabled,
    );
    validate_download_url(&url).map_err(|error| error.to_string())?;
    let yt_dlp = resolve_yt_dlp(&app, &state.config)?;
    let deno = resolve_deno_executable(&app);

    let app_handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let browser = state
            .config
            .lock()
            .map_err(|_| "Config lock poisoned")?
            .youtube_cookie_browser
            .clone();
        load_info_with_yt_dlp(yt_dlp, deno, url, &state.processes, browser.as_deref())
    })
    .await
    .map_err(|e| format!("Info task failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn check_youtube_cookie_access(
    app: AppHandle,
    state: State<'_, AppState>,
    browser: String,
) -> Result<crate::cookie_sync::BrowserAccessReport, crate::cookie_sync::BrowserAccessError> {
    let yt_dlp = resolve_yt_dlp(&app, &state.config)
        .map_err(|_| crate::cookie_sync::BrowserAccessError::YtDlpUnavailable)?;
    let deno = resolve_deno_executable(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        crate::cookie_sync::check_access(&yt_dlp, deno.as_deref(), &browser, &state.processes)
    })
    .await
    .map_err(|_| crate::cookie_sync::BrowserAccessError::CheckFailed)?
}

#[tauri::command]
pub(crate) fn get_yt_dlp_installed_version(
    app: AppHandle,
    state: State<AppState>,
    path: Option<String>,
) -> Result<InstalledYtDlpVersion, String> {
    crate::runtime::installed_yt_dlp_version(app, &state.config, path, &state.processes)
}

// Preserve the existing flat IPC filters/offset while adding an optional cursor.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn get_history(
    state: State<'_, AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
    query: Option<String>,
    search_field: Option<String>,
    source: Option<String>,
    exact_user: Option<bool>,
    cursor: Option<String>,
    creator_entry_id: Option<String>,
    tag_id: Option<String>,
) -> Result<HistoryPage, String> {
    let limit = limit.unwrap_or(20).clamp(1, 100);
    let offset = offset.unwrap_or(0);
    let db = state.db.clone();
    let filter = HistoryFilter::new(
        query.as_deref(),
        search_field.as_deref(),
        source.as_deref(),
        exact_user.unwrap_or(false),
    )?;
    tauri::async_runtime::spawn_blocking(move || {
        let filter = if let Some(id) = creator_entry_id {
            crate::accounts::history_filter(&db, &id)?
        } else {
            filter
        };
        let filter = filter.with_tag(tag_id.as_deref())?;
        read_page(&db, limit, offset, filter, cursor.as_deref())
    })
    .await
    .map_err(|e| format!("History request failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn get_history_stats(state: State<'_, AppState>) -> Result<HistoryStats, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || get_history_stats_from_db(&db))
        .await
        .map_err(|e| format!("History statistics request failed: {e}"))?
}

// The asset protocol starts with an empty scope. Authorize only the exact
// video or playback derivative resolved from this History ID, never a caller path.
#[tauri::command]
pub(crate) async fn get_history_media(
    app: AppHandle,
    id: String,
    compatible: Option<bool>,
) -> Result<crate::playback_cache::PlaybackSource, String> {
    let generation = app
        .state::<AppState>()
        .history_media
        .generation
        .load(std::sync::atomic::Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let source = crate::history::local_history_media_path(&state.db, &id)?;
        let identity = crate::ocr::fingerprint(&source)?;
        let directory = app
            .path()
            .app_cache_dir()
            .map_err(|e| format!("Playback cache unavailable: {e}"))?
            .join("history-media");
        let publish = |path: std::path::PathBuf,
                       cache: &mut crate::playback_cache::PlaybackCache|
         -> Result<crate::playback_cache::PlaybackSource, String> {
            if crate::history::local_history_media_path(&state.db, &id)? != source
                || crate::ocr::fingerprint(&source)? != identity
            {
                return Err(
                    "The saved video changed while preparing playback. Reopen the tab.".into(),
                );
            }
            let path =
                fs::canonicalize(path).map_err(|e| format!("Playback file unavailable: {e}"))?;
            let value = path
                .to_str()
                .ok_or("The saved video path is not valid Unicode")?
                .to_owned();
            if generation
                != state
                    .history_media
                    .generation
                    .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err("The playback page was closed. Reopen the History entry.".into());
            }
            app.asset_protocol_scope()
                .allow_file(&path)
                .map_err(|_| "The saved video could not be opened")?;
            let lease_id = if path != source {
                Some(cache.acquire(&path, generation)?)
            } else {
                None
            };
            Ok(crate::playback_cache::PlaybackSource {
                path: value,
                lease_id,
            })
        };
        {
            let mut cache = state
                .history_media
                .cache
                .lock()
                .map_err(|_| "Playback lock poisoned")?;
            if let Some(path) = crate::history_media::cached(&directory, &source)? {
                return publish(path, &mut cache);
            }
            if !compatible.unwrap_or(false) {
                return publish(source.clone(), &mut cache);
            }
        }
        // A long conversion never holds the cache/lease mutex. Other History
        // players can open their originals, release references or finish loading.
        let _conversion = state
            .history_media
            .conversion
            .lock()
            .map_err(|_| "Playback lock poisoned")?;
        let configured = state
            .config
            .lock()
            .map_err(|_| "Config lock poisoned")?
            .yt_dlp_path
            .clone();
        let tools =
            crate::runtime::resolve_ffmpeg_location(&app, configured.as_deref().unwrap_or(""))
                .ok_or("FFmpeg is required to prepare this video for local playback.")?;
        let prepared = crate::history_media::prepare(
            &directory,
            &source,
            &std::path::Path::new(&tools).join(crate::runtime::ffmpeg_tool_name()),
            &state.processes,
            &state.history_media.cache,
        )?;
        let mut cache = state
            .history_media
            .cache
            .lock()
            .map_err(|_| "Playback lock poisoned")?;
        publish(prepared.path.clone(), &mut cache)
    })
    .await
    .map_err(|e| format!("Local playback failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn release_history_media(app: AppHandle, lease_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .history_media
            .cache
            .lock()
            .map_err(|_| "Playback lock poisoned")?
            .release(&lease_id);
        Ok(())
    })
    .await
    .map_err(|_| "Playback preview could not be released")?
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
pub(crate) fn get_history_subtitle(
    state: State<AppState>,
    id: String,
    path: String,
) -> Result<Option<crate::subtitles::SubtitleContent>, String> {
    crate::subtitles::content(
        &*state.db.lock().map_err(|_| "SQLite lock poisoned")?,
        &id,
        &path,
    )
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
pub(crate) fn get_history_ocr(
    state: State<AppState>,
    id: String,
    limit: Option<u64>,
    offset: Option<u64>,
    search: Option<String>,
    result_revision: Option<String>,
) -> Result<Option<crate::models::OcrContent>, String> {
    crate::ocr_store::page(
        &state.db,
        &id,
        limit.unwrap_or(100),
        offset.unwrap_or(0),
        search.as_deref().unwrap_or(""),
        result_revision.as_deref(),
    )
}

#[tauri::command]
pub(crate) fn get_history_social_insights(
    state: State<AppState>,
    id: String,
) -> Result<Option<crate::social_insights::SocialInsightsSnapshot>, String> {
    crate::social_insights::get(&state.db, &id)
}

#[tauri::command]
pub(crate) async fn get_history_keywords(
    app: AppHandle,
    id: String,
) -> Result<crate::keywords::HistoryKeywords, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::keywords::mine(&app.state::<AppState>().db, &id)
    })
    .await
    .map_err(|e| format!("Keyword mining failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn load_history_social_insights(
    app: AppHandle,
    id: String,
) -> Result<crate::social_insights::SocialInsightsSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let tools = crate::runtime::resolve_social_insights_tools(&app)?;
        crate::social_insights::fetch(
            &state.db,
            &state.social_insights,
            &state.processes,
            &id,
            &tools,
        )
    })
    .await
    .map_err(|e| format!("Social Insights request failed: {e}"))?
}

#[tauri::command]
pub(crate) fn get_history_ocr_text(
    state: State<AppState>,
    id: String,
    result_revision: String,
) -> Result<String, String> {
    crate::ocr_store::export_text(&state.db, &id, Some(&result_revision))
}

#[tauri::command]
pub(crate) fn get_history_ocr_progress(
    state: State<AppState>,
    id: String,
) -> Result<Option<crate::models::OcrProgress>, String> {
    state.ocr.progress(&id)
}

#[tauri::command]
pub(crate) async fn get_history_ocr_frame(
    app: AppHandle,
    id: String,
    selection: crate::models::OcrSelection,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let configured = state
            .config
            .lock()
            .map_err(|_| "Config lock poisoned")?
            .yt_dlp_path
            .clone();
        let directory =
            crate::runtime::resolve_ffmpeg_location(&app, configured.as_deref().unwrap_or(""))
                .ok_or("FFmpeg and ffprobe are required for frame previews.")?;
        let directory = std::path::Path::new(&directory);
        state.ocr_preview.frame(
            &state.db,
            &state.processes,
            &id,
            &selection,
            &directory.join(crate::runtime::ffmpeg_tool_name()),
            &directory.join(crate::runtime::ffprobe_tool_name()),
        )
    })
    .await
    .map_err(|e| format!("Frame preview failed: {e}"))?
}

#[tauri::command]
pub(crate) fn delete_history_ocr_frame_text(
    state: State<AppState>,
    id: String,
    selection: crate::models::OcrSelection,
) -> Result<crate::models::OcrContent, String> {
    crate::ocr_store::delete_frame(&state.db, &id, &selection)
}

#[tauri::command]
pub(crate) async fn preview_history_ocr_duplicates(
    app: AppHandle,
    id: String,
) -> Result<crate::models::OcrDuplicatePreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::ocr_store::preview_duplicates(&app.state::<AppState>().db, &id)
    })
    .await
    .map_err(|e| format!("OCR duplicate preview failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn remove_history_ocr_duplicates(
    app: AppHandle,
    id: String,
    result_revision: String,
) -> Result<crate::models::OcrDuplicateRemoval, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::ocr_store::remove_duplicates(&app.state::<AppState>().db, &id, &result_revision)
    })
    .await
    .map_err(|e| format!("OCR duplicate removal failed: {e}"))?
}

#[tauri::command]
pub(crate) fn cancel_history_ocr(
    state: State<AppState>,
    id: String,
    attempt_id: String,
) -> Result<(), String> {
    state.ocr.cancel(&state.db, &id, &attempt_id)
}

#[tauri::command]
pub(crate) fn start_history_ocr(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<crate::models::OcrProgress, String> {
    use tauri::Emitter;
    let helper = crate::runtime::resolve_ocr_helper(&app)?;
    let configured = state
        .config
        .lock()
        .map_err(|_| "Config lock poisoned")?
        .yt_dlp_path
        .clone();
    let directory =
        crate::runtime::resolve_ffmpeg_location(&app, configured.as_deref().unwrap_or(""))
            .ok_or("FFmpeg and ffprobe are required for OCR.")?;
    let tools = crate::ocr::OcrTools {
        helper,
        ffmpeg: std::path::Path::new(&directory).join(crate::runtime::ffmpeg_tool_name()),
        ffprobe: std::path::Path::new(&directory).join(crate::runtime::ffprobe_tool_name()),
    };
    crate::ocr::start(
        state.db.clone(),
        state.ocr.clone(),
        id,
        tools,
        move |progress| {
            let _ = app.emit("history:ocr", progress);
        },
    )
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
pub(crate) fn get_queue(state: State<AppState>) -> Result<crate::queue::QueueSnapshot, String> {
    crate::queue::snapshot_queue(&state.queue)
}

#[tauri::command]
pub(crate) fn retry_interrupted_download(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<String, String> {
    let job = crate::queue::retry_interrupted(&state.queue, &id)?;
    crate::events::emit_queue(&app, &state.queue)?;
    if crate::queue::is_queue_auto_start_enabled(&state.queue)? {
        ensure_worker(&app, state.inner())?;
    }
    Ok(job.id)
}
#[tauri::command]
pub(crate) fn set_queue_auto_start(
    app: AppHandle,
    state: State<AppState>,
    enabled: bool,
) -> Result<QueueStatus, String> {
    crate::queue::set_queue_auto_start(&state.queue, enabled)?;

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
pub(crate) async fn enqueue_playlist(
    app: AppHandle,
    state: State<'_, AppState>,
    request: DownloadRequest,
) -> Result<crate::youtube_playlist::PlaylistQueueResult, String> {
    let url = crate::youtube_playlist::normalize_playlist_url(&request.url)
        .ok_or("Enter a valid YouTube playlist link.")?;
    let config = state
        .config
        .lock()
        .map_err(|_| "Config lock poisoned")?
        .clone();
    let output_dir = crate::runtime::resolve_output_dir(&state.config, request.output_dir.clone())?;
    let yt_dlp = resolve_yt_dlp(&app, &state.config)?;
    let deno = resolve_deno_executable(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let playlist = crate::youtube_playlist::load_playlist(
            &yt_dlp,
            deno.as_deref(),
            &url,
            &state.processes,
            config.youtube_cookie_browser.as_deref(),
        )?;
        let result = crate::youtube_playlist::build_jobs(request, playlist, &config, output_dir);
        crate::worker::enqueue_download_jobs(&app, &state, result.jobs.clone())?;
        Ok::<_, String>(result)
    })
    .await
    .map_err(|error| format!("Playlist task failed: {error}"))?
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

#[tauri::command]
pub(crate) async fn get_history_account(
    state: State<'_, AppState>,
    id: String,
    offset: Option<u32>,
    cursor: Option<String>,
) -> Result<crate::accounts::AccountPage, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::accounts::get_page(&db, &id, offset.unwrap_or(0), cursor.as_deref())
    })
    .await
    .map_err(|e| format!("Account request failed: {e}"))?
}
#[tauri::command]
pub(crate) async fn load_history_account_insights(
    app: AppHandle,
    id: String,
) -> Result<crate::accounts::AccountSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let yt_dlp = resolve_yt_dlp(&app, &state.config)?;
        let deno = resolve_deno_executable(&app);
        let tools = crate::runtime::resolve_social_insights_tools(&app)?;
        crate::accounts::fetch_insights(
            &state.db,
            &state.accounts,
            &state.processes,
            &id,
            &tools,
            &yt_dlp,
            deno.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Account request failed: {e}"))?
}
#[tauri::command]
pub(crate) async fn load_history_account_videos(
    app: AppHandle,
    id: String,
    offset: Option<u32>,
) -> Result<crate::accounts::ProfileVideos, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let yt_dlp = resolve_yt_dlp(&app, &state.config)?;
        let deno = resolve_deno_executable(&app);
        crate::accounts::fetch_videos(
            &state.db,
            &state.accounts,
            &state.processes,
            &id,
            offset.unwrap_or(0),
            &yt_dlp,
            deno.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Account videos request failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn get_history_keyword_occurrences(
    app: AppHandle,
    id: String,
    query: crate::keyword_evidence::OccurrenceQuery,
) -> Result<crate::keyword_evidence::OccurrencePage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::keyword_evidence::occurrences(&app.state::<AppState>().db, &id, query)
    })
    .await
    .map_err(|e| format!("Keyword occurrences failed: {e}"))?
}
#[tauri::command]
pub(crate) async fn get_history_keyword_source(
    app: AppHandle,
    id: String,
    source_key: String,
    revision: String,
) -> Result<crate::keywords::TextSource, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::keyword_evidence::source(&app.state::<AppState>().db, &id, &source_key, &revision)
    })
    .await
    .map_err(|e| format!("Keyword source failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn list_tags(
    state: State<'_, AppState>,
) -> Result<Vec<crate::tags::TagSummary>, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || crate::tags::list(&db))
        .await
        .map_err(|e| format!("Tag request failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn get_tag(
    state: State<'_, AppState>,
    id: String,
) -> Result<crate::tags::TagSummary, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || crate::tags::get(&db, &id))
        .await
        .map_err(|e| format!("Tag request failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn create_tag(
    state: State<'_, AppState>,
    tag: crate::tags::TagInput,
) -> Result<crate::tags::Tag, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || crate::tags::create(&db, tag))
        .await
        .map_err(|e| format!("Tag request failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn update_tag(
    state: State<'_, AppState>,
    id: String,
    tag: crate::tags::TagInput,
) -> Result<crate::tags::Tag, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || crate::tags::update(&db, &id, tag))
        .await
        .map_err(|e| format!("Tag request failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn delete_tag(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || crate::tags::delete(&db, &id))
        .await
        .map_err(|e| format!("Tag request failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn get_history_tags(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<crate::tags::Tag>, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::tags::batch(&db, &[id]).map(|mut entries| entries.remove(0).tags)
    })
    .await
    .map_err(|e| format!("Tag request failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn get_history_tags_batch(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<Vec<crate::tags::EntryTags>, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || crate::tags::batch(&db, &ids))
        .await
        .map_err(|e| format!("Tag request failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn add_history_tag(
    state: State<'_, AppState>,
    id: String,
    tag_id: String,
) -> Result<(), String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || crate::tags::assign(&db, &id, &tag_id, true))
        .await
        .map_err(|e| format!("Tag request failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn remove_history_tag(
    state: State<'_, AppState>,
    id: String,
    tag_id: String,
) -> Result<(), String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || crate::tags::assign(&db, &id, &tag_id, false))
        .await
        .map_err(|e| format!("Tag request failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn get_tag_accounts(
    state: State<'_, AppState>,
    id: String,
    query: Option<String>,
    limit: Option<u32>,
    cursor: Option<String>,
) -> Result<crate::tags::AccountPage, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::tags::accounts(
            &db,
            &id,
            query.as_deref().unwrap_or(""),
            limit.unwrap_or(20),
            cursor.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Tag request failed: {e}"))?
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn get_tag_content(
    state: State<'_, AppState>,
    id: String,
    query: Option<String>,
    search_field: Option<String>,
    source: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
    cursor: Option<String>,
) -> Result<HistoryPage, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        read_page(
            &db,
            limit.unwrap_or(20).clamp(1, 100),
            offset.unwrap_or(0),
            HistoryFilter::new(
                query.as_deref(),
                search_field.as_deref(),
                source.as_deref(),
                false,
            )?
            .with_tag(Some(&id))?,
            cursor.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Tag request failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn get_tag_choices(
    state: State<'_, AppState>,
) -> Result<Vec<crate::tags::Tag>, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || crate::tags::choices(&db))
        .await
        .map_err(|e| e.to_string())?
}
