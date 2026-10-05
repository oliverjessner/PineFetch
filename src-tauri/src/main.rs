use crate::browser_import::start_link_dump_server_from_settings;
use crate::browser_import::stop_link_dump_server;
use crate::state::AppState;
use crate::worker::stop_active_download_on_exit;
use tauri::Manager;

#[cfg(test)]
mod architecture_tests;
mod browser_import;
mod cli;
mod commands;
mod completion;
mod completion_store;
mod config;
mod config_rules;
mod database;
mod download;
mod download_rules;
mod events;
mod files;
mod hashing;
mod history;
mod history_rules;
#[cfg(test)]
mod integrity_tests;
mod link_dump_store;
mod metadata;
mod models;
mod platform;
mod presets;
mod process;
mod queue;
mod runtime;
mod startup;
mod state;
#[cfg(test)]
#[path = "regression_tests.rs"]
mod tests;
mod video_urls;
mod worker;
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
            let state = app.state::<AppState>();
            let _ = start_link_dump_server_from_settings(app.handle(), state.inner());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            cli::initialize_cli,
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
            commands::get_queue_status,
            commands::get_queue,
            commands::set_queue_auto_start,
            commands::start_queue,
            commands::pause_queue,
            commands::resume_queue,
            commands::enqueue_download,
            commands::cancel_download,
            commands::get_history,
            commands::get_history_stats,
            commands::get_history_details,
            commands::get_history_transcript,
            commands::get_history_caption,
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
            stop_link_dump_server(state.inner());
            if let Some(server) = app_handle.try_state::<cli::CliServer>() {
                server.stop();
            }
        }
    });
}
