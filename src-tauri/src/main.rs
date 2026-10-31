use crate::browser_import::start_link_dump_server_from_settings;
use crate::browser_import::stop_link_dump_server;
use crate::state::AppState;
use crate::worker::stop_active_download_on_exit;
use tauri::Manager;

mod accounts;
#[cfg(test)]
mod application_failure_tests;
#[cfg(test)]
mod architecture_tests;
mod browser_import;
mod cli;
mod commands;
mod completion;
mod completion_store;
mod config;
mod config_rules;
mod cookie_sync;
mod creator_identity;
mod database;
#[cfg(test)]
mod db_contention_tests;
#[cfg(target_os = "macos")]
mod dock_menu;
mod download;
mod download_rules;
mod events;
mod files;
mod hashing;
mod history;
#[cfg(test)]
mod history_archive_tests;
mod history_media;
mod history_query;
mod history_rules;
#[cfg(test)]
mod http_failure_tests;
#[cfg(test)]
mod integrity_tests;
mod keyword_evidence;
mod keywords;
mod link_dump_store;
mod media_tools;
mod metadata;
mod models;
mod navigation;
mod ocr;
mod ocr_preview;
mod ocr_rules;
mod ocr_source;
mod ocr_store;
#[cfg(test)]
mod ocr_tests;
#[cfg(test)]
mod persistence_failure_tests;
mod platform;
mod playback_cache;
mod presets;
mod process;
#[cfg(test)]
mod process_failure_tests;
mod queue;
#[cfg(test)]
mod queue_failure_tests;
#[cfg(test)]
mod queue_persistence_tests;
mod queue_store;
#[cfg(test)]
mod reuse_tests;
mod runtime;
#[cfg(test)]
mod runtime_failure_tests;
mod social_insights;
mod startup;
mod state;
mod subtitles;
mod tags;
#[cfg(test)]
mod tags_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
#[path = "regression_tests.rs"]
mod tests;
mod unicode_search;
mod url_rules;
mod video_urls;
mod worker;
mod youtube_playlist;
mod yt_dlp;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match cli::parse(&args) {
        Ok(Some(cli::CliCommand::Help)) => {
            print!("{}", cli::HELP);
            return;
        }
        Ok(Some(cli::CliCommand::Version)) => {
            println!("PineFetch {}", cli::VERSION);
            return;
        }
        Ok(command) => command,
        Err(err) => {
            eprintln!("PineFetch: {err}");
            std::process::exit(2);
        }
    };
    let context = tauri::generate_context!();
    if let Some(command) = command {
        match cli::run(command, context.config()) {
            Ok(output) => println!("{output}"),
            Err(err) => {
                eprintln!("PineFetch: {err}");
                std::process::exit(1);
            }
        }
        return;
    }
    // Tauri panics when setup returns an error. On macOS that panic crosses
    // an Objective-C callback and aborts instead of reporting a normal error.
    let state = match startup::load_for_identifier(&context.config().identifier) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("PineFetch could not start: {err}");
            std::process::exit(1);
        }
    };
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            app.manage(state);
            app.manage(navigation::NavigationState::default());
            #[cfg(target_os = "macos")]
            if let Err(err) = dock_menu::install(app) {
                eprintln!("Could not install Dock menu: {err}");
            }
            let state = app.state::<AppState>();
            if crate::playback_cache::start_maintenance(app.handle().clone()).is_err() {
                eprintln!("Playback cache maintenance could not be started.");
            }
            let _ = start_link_dump_server_from_settings(app.handle(), state.inner());
            if queue::is_queue_auto_start_enabled(&state.queue)? {
                worker::ensure_worker(app.handle(), state.inner())?;
            }
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if webview.label() != "main"
                || !matches!(payload.event(), tauri::webview::PageLoadEvent::Started)
            {
                return;
            }
            let Some(state) = webview.try_state::<AppState>() else {
                return;
            };
            let generation = state
                .history_media
                .generation
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1;
            let app = webview.app_handle().clone();
            // Unload IPC is not guaranteed. Revoke references from the old page
            // natively, without blocking the UI behind a running conversion.
            tauri::async_runtime::spawn_blocking(move || {
                if let Ok(mut cache) = app.state::<AppState>().history_media.cache.lock() {
                    cache.release_before(generation);
                }
            });
        })
        .invoke_handler(tauri::generate_handler![
            cli::initialize_cli,
            navigation::get_navigation_request,
            commands::get_config,
            commands::get_download_presets,
            commands::patch_config,
            commands::set_selected_preset_key,
            commands::set_save_captions,
            commands::cache_last_download_url,
            commands::pick_output_dir,
            commands::pick_txt_file,
            commands::open_folder,
            commands::open_file_path,
            commands::open_external_url,
            commands::read_clipboard_text,
            commands::load_info,
            commands::get_yt_dlp_installed_version,
            commands::check_youtube_cookie_access,
            commands::get_queue_status,
            commands::get_queue,
            commands::retry_interrupted_download,
            commands::set_queue_auto_start,
            commands::start_queue,
            commands::pause_queue,
            commands::resume_queue,
            commands::enqueue_download,
            commands::enqueue_playlist,
            commands::cancel_download,
            commands::get_history,
            commands::list_tags,
            commands::get_tag_choices,
            commands::get_tag,
            commands::create_tag,
            commands::update_tag,
            commands::delete_tag,
            commands::get_history_tags,
            commands::get_history_tags_batch,
            commands::add_history_tag,
            commands::remove_history_tag,
            commands::get_tag_content,
            commands::get_tag_accounts,
            commands::get_history_stats,
            commands::get_history_account,
            commands::load_history_account_insights,
            commands::load_history_account_videos,
            commands::get_history_details,
            commands::get_history_media,
            commands::release_history_media,
            commands::get_history_keywords,
            commands::get_history_keyword_occurrences,
            commands::get_history_keyword_source,
            commands::get_history_social_insights,
            commands::load_history_social_insights,
            commands::get_history_transcript,
            commands::get_history_caption,
            commands::get_history_subtitle,
            commands::start_history_ocr,
            commands::cancel_history_ocr,
            commands::get_history_ocr,
            commands::get_history_ocr_text,
            commands::get_history_ocr_progress,
            commands::get_history_ocr_frame,
            commands::delete_history_ocr_frame_text,
            commands::preview_history_ocr_duplicates,
            commands::remove_history_ocr_duplicates,
            commands::remove_history_entry,
            commands::clear_history,
            commands::get_link_dump_overview,
            commands::update_link_dump_settings,
            commands::create_link_dump_secret,
            commands::revoke_link_dump_secret,
            commands::delete_link_dump_secret,
            commands::restart_link_dump_server,
        ])
        .build(context);
    let app = match app {
        Ok(app) => app,
        Err(err) => {
            eprintln!("PineFetch could not start: {err}");
            std::process::exit(1);
        }
    };
    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            let state = app_handle.state::<AppState>();
            stop_active_download_on_exit(state.inner());
            state.ocr.stop();
            stop_link_dump_server(state.inner());
            if let Some(server) = app_handle.try_state::<cli::CliServer>() {
                server.stop();
            }
        }
    });
}
