//! Terminal commands and a private local connection to the desktop process.
//! History is deliberately exposed only through read operations.
use crate::download_rules::request_from_preset;
use crate::events::emit_queue;
use crate::events::emit_state;
use crate::history::get_history_stats_from_db;
use crate::history::list_history_page_from_db;
use crate::models::DownloadJob;
use crate::models::DownloadRequest;
use crate::models::DownloadState;
use crate::models::DownloadStateEvent;
use crate::models::HistoryStats;
use crate::presets::download_preset_for_key;
use crate::state::AppState;
use crate::url_rules::validate_download_url;
use crate::worker::enqueue_download_request;
use serde::Deserialize;
use serde::Serialize;
use std::fs;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::net::SocketAddrV4;
use std::net::TcpListener;
use std::net::TcpStream;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::AppHandle;
use tauri::Manager;
use uuid::Uuid;

pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const HELP: &str = concat!(
    "PineFetch CLI ",
    env!("CARGO_PKG_VERSION"),
    "

Usage:
  PineFetch queue add --link <URL> [--preset <PRESET>]
  PineFetch queue list
  PineFetch queue remove <NUMBER>
  PineFetch history list
  PineFetch stats
  PineFetch --help
  PineFetch --version

Presets: best (default), max, mp3, opus, text, 'text with timestamps'

PineFetch opens automatically when needed. Downloads follow the app's
saved settings and auto-start mode. Queue numbers are one-based and refer
to waiting downloads at the time the command runs, excluding active jobs.
History lists the latest 25 entries and is read-only.
"
);
const MAX_MESSAGE_BYTES: u64 = 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CliCommand {
    Help,
    Version,
    QueueAdd { link: String, preset: String },
    QueueList,
    QueueRemove { number: usize },
    HistoryList,
    Stats,
}

pub(crate) fn parse(args: &[String]) -> Result<Option<CliCommand>, String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => Ok(None),
        ["--help" | "-h" | "help"] => Ok(Some(CliCommand::Help)),
        ["--version" | "-V" | "version"] => Ok(Some(CliCommand::Version)),
        ["queue", "list"] => Ok(Some(CliCommand::QueueList)),
        ["history", "list"] => Ok(Some(CliCommand::HistoryList)),
        ["stats"] => Ok(Some(CliCommand::Stats)),
        ["queue", "remove", number] => {
            let number = number
                .parse::<usize>()
                .map_err(|_| "Queue number must be a positive integer")?;
            if number == 0 {
                return Err("Queue numbering starts at 1".into());
            }
            Ok(Some(CliCommand::QueueRemove { number }))
        }
        ["queue", "add", options @ ..] => {
            let mut link = None;
            let mut preset = None;
            let mut options = options.iter();
            while let Some(flag) = options.next() {
                let value = options
                    .next()
                    .ok_or_else(|| format!("Missing value for {flag}"))?;
                match *flag {
                    "--link" if link.is_none() => link = Some((*value).to_string()),
                    "--preset" if preset.is_none() => preset = Some((*value).to_string()),
                    _ => return Err(format!("Unknown or repeated option: {flag}")),
                }
            }
            let link = link.ok_or("queue add requires --link <URL>")?;
            validate_link(&link)?;
            let preset = preset.unwrap_or_else(|| "best".into());
            preset_key(&preset)?;
            Ok(Some(CliCommand::QueueAdd { link, preset }))
        }
        _ => Err("Unknown command. Run PineFetch --help. History is read-only.".into()),
    }
}

fn validate_link(link: &str) -> Result<(), String> {
    validate_download_url(link).map_err(|_| "Link must be a valid http:// or https:// URL".into())
}

fn preset_key(preset: &str) -> Result<&'static str, String> {
    match preset {
        "best" => Ok("best"),
        "max" => Ok("1080"),
        "mp3" => Ok("audio_mp3"),
        "opus" => Ok("audio_opus"),
        "text" => Ok("text"),
        "text with timestamps" => Ok("text_timestamps"),
        _ => Err(format!(
            "Unknown preset '{preset}'. Use best, max, mp3, opus, text, or 'text with timestamps'."
        )),
    }
}

fn build_request(state: &AppState, link: &str, preset: &str) -> Result<DownloadRequest, String> {
    validate_link(link)?;
    let preset = download_preset_for_key(Some(preset_key(preset)?));
    let config = state.config.lock().map_err(|_| "Config lock poisoned")?;
    Ok(request_from_preset(
        preset,
        link.trim().to_string(),
        config.cut_at_timestamp_enabled,
    ))
}

// Remove under the same lock used by the worker: never cancel an active job
// because a queue index changed between resolving it and removing it.
fn remove_waiting_job(state: &AppState, number: usize) -> Result<DownloadJob, String> {
    let mut queue = state
        .queue
        .pending
        .lock()
        .map_err(|_| "Queue lock poisoned")?;
    number
        .checked_sub(1)
        .and_then(|index| queue.remove(index))
        .ok_or_else(|| format!("No waiting queue item at position {number}"))
}

fn terminal_text(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn job_preset(job: &DownloadJob) -> &str {
    if job.transcribe_timestamps {
        "text with timestamps"
    } else if job.transcribe_text {
        "text"
    } else if job.extract_audio {
        job.audio_format.as_deref().unwrap_or("audio")
    } else if job.format == download_preset_for_key(Some("1080")).format {
        "max"
    } else {
        "best"
    }
}

fn list_queue(state: &AppState) -> Result<String, String> {
    let queue = state
        .queue
        .pending
        .lock()
        .map_err(|_| "Queue lock poisoned")?;
    if queue.is_empty() {
        return Ok("No waiting downloads.".into());
    }
    let mut output = String::from("#\tPRESET\tLINK");
    for (index, job) in queue.iter().enumerate() {
        output.push_str(&format!(
            "\n{}\t{}\t{}",
            index + 1,
            job_preset(job),
            terminal_text(&job.url)
        ));
    }
    Ok(output)
}

fn list_history(state: &AppState) -> Result<String, String> {
    let page = list_history_page_from_db(&state.db, 25, 0)?;
    if page.entries.is_empty() {
        return Ok("History is empty.".into());
    }
    let mut output = String::from("#\tTITLE\tLINK");
    for (index, entry) in page.entries.iter().enumerate() {
        let title = entry
            .title
            .as_deref()
            .or(entry.filename.as_deref())
            .unwrap_or("-");
        output.push_str(&format!(
            "\n{}\t{}\t{}",
            index + 1,
            terminal_text(title),
            terminal_text(&entry.url)
        ));
    }
    Ok(output)
}

fn format_size(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= 1024.0 && index < units.len() - 1 {
        value /= 1024.0;
        index += 1;
    }
    let precision = if index == 0 || value >= 100.0 {
        0
    } else if value >= 10.0 {
        1
    } else {
        2
    };
    format!("{value:.precision$} {}", units[index])
}

fn format_stats(stats: &HistoryStats) -> String {
    let seconds = stats.total_duration_seconds;
    let duration = if seconds >= 3600 {
        format!("{}h {:02}m", seconds / 3600, seconds / 60 % 60)
    } else {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    };
    format!(
        "Downloaded videos: {}\nTotal data: {}\nTotal runtime: {}",
        stats.video_count,
        format_size(stats.total_file_size_bytes),
        duration
    )
}

fn execute(app: &AppHandle, command: CliCommand) -> Result<String, String> {
    let state = app.state::<AppState>();
    match command {
        CliCommand::Help => Ok(HELP.into()),
        CliCommand::Version => Ok(format!("PineFetch {VERSION}")),
        CliCommand::QueueAdd { link, preset } => {
            let request = build_request(&state, &link, &preset)?;
            let id = enqueue_download_request(app, &state, request)?;
            Ok(format!(
                "Added {} with preset {preset} (job {id}).",
                terminal_text(&link)
            ))
        }
        CliCommand::QueueList => list_queue(&state),
        CliCommand::QueueRemove { number } => {
            let job = remove_waiting_job(&state, number)?;
            emit_queue(app, &state.queue)?;
            emit_state(
                app,
                DownloadStateEvent {
                    id: job.id,
                    state: DownloadState::Cancelled,
                    exit_code: None,
                    error: None,
                    output_path: None,
                },
            );
            Ok(format!(
                "Removed queue item {number}: {}",
                terminal_text(&job.url)
            ))
        }
        CliCommand::HistoryList => list_history(&state),
        CliCommand::Stats => Ok(format_stats(&get_history_stats_from_db(&state.db)?)),
    }
}

#[derive(Serialize, Deserialize)]
struct Endpoint {
    port: u16,
    token: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    token: String,
    command: CliCommand,
}

fn endpoint_path(config: &tauri::Config) -> Result<PathBuf, String> {
    let dir = dirs::data_dir()
        .ok_or("App data directory unavailable")?
        .join(&config.identifier);
    Ok(dir.join("cli-endpoint.json"))
}

fn read_message<T: serde::de::DeserializeOwned>(stream: &mut TcpStream) -> Result<T, String> {
    let mut reader = BufReader::new(stream.take(MAX_MESSAGE_BYTES + 1));
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("Could not read CLI response: {e}"))?;
    if line.len() as u64 > MAX_MESSAGE_BYTES || !line.ends_with('\n') {
        return Err("Invalid or oversized CLI message".into());
    }
    serde_json::from_str(&line).map_err(|_| "Invalid CLI message".into())
}

fn write_message<T: Serialize>(stream: &mut TcpStream, message: &T) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(message).map_err(|e| e.to_string())?;
    if bytes.len() as u64 >= MAX_MESSAGE_BYTES {
        return Err("CLI response too large".into());
    }
    bytes.push(b'\n');
    stream
        .write_all(&bytes)
        .map_err(|e| format!("Could not write CLI message: {e}"))
}

fn configure_stream(stream: &TcpStream) -> Result<(), String> {
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())
}

fn authorize(request: Request, token: &str) -> Result<CliCommand, String> {
    if request.token != token {
        return Err("CLI authentication failed".into());
    }
    Ok(request.command)
}

pub(crate) struct CliServer {
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
    path: PathBuf,
    token: String,
}

impl CliServer {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut handle) = self.thread.lock() {
            if let Some(handle) = handle.take() {
                let _ = handle.join();
            }
        }
        // An older instance must not delete a newer instance's endpoint.
        if read_endpoint(&self.path).is_ok_and(|endpoint| endpoint.token == self.token) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

// Publish the endpoint only after the frontend subscribes to queue events, so
// the first command that launches the app cannot lose its UI updates.
#[tauri::command]
pub(crate) fn initialize_cli(app: AppHandle) -> Result<(), String> {
    if app.try_state::<CliServer>().is_none() {
        app.manage(start_server(&app)?);
    }
    Ok(())
}

fn start_server(app: &AppHandle) -> Result<CliServer, String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let endpoint = Endpoint {
        port: listener.local_addr().map_err(|e| e.to_string())?.port(),
        token: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
    };
    let path = endpoint_path(app.config())?;
    fs::create_dir_all(path.parent().ok_or("Invalid CLI endpoint path")?)
        .map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let write_result = (|| -> Result<(), String> {
        let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut file, &endpoint).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&temporary, &path).map_err(|e| e.to_string())
    })();
    if let Err(err) = write_result {
        let _ = fs::remove_file(temporary);
        return Err(err);
    }

    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let token = endpoint.token.clone();
    let app = app.clone();
    let handle = thread::spawn(move || {
        while !worker_stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    if configure_stream(&stream).is_err() {
                        continue;
                    }
                    let response = read_message::<Request>(&mut stream)
                        .and_then(|request| authorize(request, &token))
                        .and_then(|command| execute(&app, command));
                    let _ = write_message(&mut stream, &response);
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(25))
                }
                Err(_) => break,
            }
        }
    });
    Ok(CliServer {
        stop,
        thread: Mutex::new(Some(handle)),
        path,
        token: endpoint.token,
    })
}

fn read_endpoint(path: &Path) -> Result<Endpoint, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    serde_json::from_reader(file.take(4096)).map_err(|e| e.to_string())
}

fn connect(path: &Path) -> Result<(TcpStream, Endpoint), String> {
    let endpoint = read_endpoint(path)?;
    let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, endpoint.port));
    let stream = TcpStream::connect_timeout(&address, Duration::from_millis(250))
        .map_err(|e| e.to_string())?;
    configure_stream(&stream)?;
    Ok((stream, endpoint))
}

fn launch_desktop() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    if let Some(bundle) = exe
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
    {
        let status = Command::new("/usr/bin/open")
            .arg("-g")
            .arg(bundle)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("Could not open PineFetch: {e}"))?;
        return if status.success() {
            Ok(())
        } else {
            Err("Could not open PineFetch".into())
        };
    }
    let mut command = Command::new(exe);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not open PineFetch: {e}"))?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

// Coordinate simultaneous CLI launches without starting multiple desktop workers.
struct StartupLock(PathBuf);
impl Drop for StartupLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn connect_or_launch(
    path: &Path,
    launch: impl FnOnce() -> Result<(), String>,
) -> Result<(TcpStream, Endpoint), String> {
    if let Ok(connection) = connect(path) {
        return Ok(connection);
    }
    fs::create_dir_all(path.parent().ok_or("Invalid CLI endpoint path")?)
        .map_err(|e| e.to_string())?;
    let lock_path = path.with_extension("startup.lock");
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let mut launch = Some(launch);
    let mut lock = None;
    loop {
        if let Ok(connection) = connect(path) {
            return Ok(connection);
        }
        if lock.is_none() {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(_) => {
                    lock = Some(StartupLock(lock_path.clone()));
                    // Another launcher may have finished just before we took its lock.
                    if let Ok(connection) = connect(path) {
                        return Ok(connection);
                    }
                    launch.take().ok_or("PineFetch launch already attempted")?()?;
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = fs::metadata(&lock_path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|time| time.elapsed().ok())
                        .is_some_and(|age| age > Duration::from_secs(30));
                    if stale {
                        let _ = fs::remove_file(&lock_path);
                    }
                }
                Err(err) => return Err(format!("Could not coordinate PineFetch startup: {err}")),
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(
                "PineFetch did not become ready within 15 seconds. Open the app and try again."
                    .into(),
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn send_command(
    path: &Path,
    command: CliCommand,
    launch: impl FnOnce() -> Result<(), String>,
) -> Result<String, String> {
    let connection = connect_or_launch(path, launch)?;
    let (mut stream, endpoint) = connection;
    // Never retry after sending: a lost response must not duplicate a download
    // or remove the next item a second time.
    write_message(
        &mut stream,
        &Request {
            token: endpoint.token,
            command,
        },
    )?;
    read_message::<Result<String, String>>(&mut stream)?
}

pub(crate) fn run(command: CliCommand, config: &tauri::Config) -> Result<String, String> {
    send_command(&endpoint_path(config)?, command, launch_desktop)
}

#[cfg(test)]
mod tests {
    use super::authorize;
    use super::build_request;
    use super::configure_stream;
    use super::format_stats;
    use super::job_preset;
    use super::list_history;
    use super::list_queue;
    use super::parse;
    use super::read_message;
    use super::remove_waiting_job;
    use super::send_command;
    use super::terminal_text;
    use super::write_message;
    use super::CliCommand;
    use super::Endpoint;
    use super::Request;
    use super::HELP;
    use super::VERSION;
    use crate::database::initialize as run_link_dump_migrations;
    use crate::history::count_history_entries_in_db;
    use crate::history::get_history_stats_from_db;
    use crate::models::AppConfig;
    use crate::models::HistoryStats;
    use crate::presets::download_preset_for_key;
    use crate::state::AppState;
    use crate::worker::build_download_job;
    use rusqlite::params;
    use rusqlite::Connection;
    use serde_json::json;
    use std::fs;
    use std::net::Ipv4Addr;
    use std::net::TcpListener;
    use std::path::Path;
    use std::thread;
    use std::thread::JoinHandle;
    use uuid::Uuid;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    fn state() -> AppState {
        let db = Connection::open_in_memory().unwrap();
        run_link_dump_migrations(&db).unwrap();
        AppState::new(
            AppConfig {
                default_output_dir: Some(std::env::temp_dir().to_string_lossy().to_string()),
                ..AppConfig::default()
            },
            db,
        )
    }

    #[test]
    fn cli_link_dump_and_gui_requests_share_preset_jobs_and_configuration_defaults() {
        use crate::browser_import::build_link_dump_download_request;
        use crate::download_rules::prepare_download_job;
        use crate::download_rules::DownloadOptions;
        use crate::models::DownloadRequest;
        use crate::video_urls::normalize_video_url;

        let state = state();
        let url = "https://www.youtube.com/watch?v=abc123";
        let normalized = normalize_video_url(url).unwrap();
        for (name, key) in [
            ("best", "best"),
            ("max", "1080"),
            ("mp3", "audio_mp3"),
            ("opus", "audio_opus"),
            ("text", "text"),
            ("text with timestamps", "text_timestamps"),
        ] {
            for enabled in [false, true] {
                {
                    let mut config = state.config.lock().unwrap();
                    config.selected_preset_key = Some(key.into());
                    config.cut_at_timestamp_enabled = enabled;
                    config.faster_whisper_model = " medium ".into();
                    config.download_video_with_transcript = true;
                    config.save_captions = true;
                    config.save_thumbnails = true;
                }
                // GUI receives this exact preset DTO from get_download_presets.
                let mut gui_input =
                    serde_json::to_value(download_preset_for_key(Some(key))).unwrap();
                let fields = gui_input.as_object_mut().unwrap();
                fields.remove("key");
                fields.insert("url".into(), json!(url));
                fields.insert("cut_at_timestamp_enabled".into(), json!(enabled));
                let gui: DownloadRequest = serde_json::from_value(gui_input).unwrap();
                let cli = build_request(&state, url, name).unwrap();
                let imported =
                    build_link_dump_download_request(&state.config, &normalized).unwrap();
                assert_eq!(gui.cut_at_timestamp_enabled, enabled);
                assert_eq!(cli.cut_at_timestamp_enabled, enabled);
                assert_eq!(imported.cut_at_timestamp_enabled, enabled);
                assert_eq!(imported.thumbnail, normalized.thumbnail);
                let prepare = |request| {
                    let options = DownloadOptions::from(&*state.config.lock().unwrap());
                    prepare_download_job(
                        request,
                        options,
                        "/synthetic/Output files".into(),
                        "stable-id".into(),
                    )
                };
                let gui = serde_json::to_value(prepare(gui)).unwrap();
                let cli = serde_json::to_value(prepare(cli)).unwrap();
                let mut imported = serde_json::to_value(prepare(imported)).unwrap();
                // Link Dump alone supplies its known thumbnail; job rules are shared.
                imported["thumbnail"] = serde_json::Value::Null;
                assert_eq!(gui, cli, "CLI preset {name}");
                assert_eq!(gui, imported, "Link Dump preset {key}");
                assert_eq!(gui["faster_whisper_model"], "medium");
                assert_eq!(gui["download_video_with_transcript"], true);
                assert_eq!(gui["save_captions"], true);
                assert_eq!(gui["save_thumbnails"], true);
            }
        }
        state.config.lock().unwrap().selected_preset_key = Some("audio_mp3".into());
        assert_eq!(
            build_request(&state, url, "best").unwrap().format,
            download_preset_for_key(Some("best")).format
        );
        assert_eq!(
            build_link_dump_download_request(&state.config, &normalized)
                .unwrap()
                .format,
            download_preset_for_key(Some("audio_mp3")).format
        );
    }

    #[test]
    fn parses_all_commands_and_presets() {
        assert_eq!(parse(&[]).unwrap(), None);
        assert_eq!(parse(&args(&["--help"])).unwrap(), Some(CliCommand::Help));
        assert_eq!(
            parse(&args(&["--version"])).unwrap(),
            Some(CliCommand::Version)
        );
        for (words, command) in [
            (vec!["queue", "list"], CliCommand::QueueList),
            (
                vec!["queue", "remove", "1"],
                CliCommand::QueueRemove { number: 1 },
            ),
            (vec!["history", "list"], CliCommand::HistoryList),
            (vec!["stats"], CliCommand::Stats),
        ] {
            assert_eq!(parse(&args(&words)).unwrap(), Some(command));
        }
        for preset in ["best", "max", "mp3", "opus", "text", "text with timestamps"] {
            assert_eq!(
                parse(&args(&[
                    "queue",
                    "add",
                    "--preset",
                    preset,
                    "--link",
                    "https://example.com/video"
                ]))
                .unwrap(),
                Some(CliCommand::QueueAdd {
                    link: "https://example.com/video".into(),
                    preset: preset.into()
                })
            );
        }
        assert_eq!(
            parse(&args(&["queue", "add", "--link", "https://example.com"])).unwrap(),
            Some(CliCommand::QueueAdd {
                link: "https://example.com".into(),
                preset: "best".into()
            })
        );
    }

    #[test]
    fn help_and_version_use_the_cargo_package_version() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
        assert!(HELP.starts_with(&format!("PineFetch CLI {VERSION}\n")));
        assert!(HELP.contains("PineFetch --version"));
    }

    #[test]
    fn rejects_invalid_commands_without_starting_desktop() {
        for words in [
            vec!["history", "delete"],
            vec!["history", "clear"],
            vec!["history", "remove", "1"],
            vec!["queue", "remove", "0"],
            vec!["queue", "remove", "-1"],
            vec!["queue", "remove", "1.5"],
            vec!["queue", "list", "extra"],
            vec!["stats", "--delete"],
            vec!["queue", "add"],
            vec!["queue", "add", "--link"],
            vec!["queue", "add", "--link", "file:///tmp/video"],
            vec!["queue", "add", "--link", "https://"],
            vec![
                "queue",
                "add",
                "--link",
                "https://example.com",
                "--preset",
                "invalid",
            ],
            vec![
                "queue",
                "add",
                "--link",
                "https://example.com",
                "--link",
                "https://example.org",
            ],
        ] {
            assert!(parse(&args(&words)).is_err(), "{words:?}");
        }
    }

    #[test]
    fn protocol_cannot_express_history_mutations_and_requires_authentication() {
        for command in [
            "history_delete",
            "history_clear",
            "history_remove",
            "clear_history",
            "remove_history_entry",
        ] {
            assert!(serde_json::from_value::<CliCommand>(json!({"command":command})).is_err());
        }
        assert!(authorize(
            Request {
                token: "wrong".into(),
                command: CliCommand::HistoryList
            },
            "secret"
        )
        .is_err());
        assert_eq!(
            authorize(
                Request {
                    token: "secret".into(),
                    command: CliCommand::HistoryList
                },
                "secret"
            )
            .unwrap(),
            CliCommand::HistoryList
        );
    }

    #[test]
    fn presets_use_desktop_download_options_and_saved_settings() {
        let state = state();
        {
            let mut config = state.config.lock().unwrap();
            config.faster_whisper_model = "medium".into();
            config.download_video_with_transcript = true;
            config.cut_at_timestamp_enabled = false;
        }
        for (name, key) in [
            ("best", "best"),
            ("max", "1080"),
            ("mp3", "audio_mp3"),
            ("opus", "audio_opus"),
            ("text", "text"),
            ("text with timestamps", "text_timestamps"),
        ] {
            let expected = download_preset_for_key(Some(key));
            let job = build_download_job(
                &state.config,
                build_request(&state, "https://example.com/video?t=60", name).unwrap(),
            )
            .unwrap();
            assert_eq!(job.format, expected.format);
            assert_eq!(job.extract_audio, expected.extract_audio);
            assert_eq!(job.audio_format.as_deref(), expected.audio_format);
            assert_eq!(job.transcribe_text, expected.transcribe_text);
            assert_eq!(job.transcribe_timestamps, expected.transcribe_timestamps);
            assert_eq!(job.filename_suffix.as_deref(), expected.filename_suffix);
            assert_eq!(job.faster_whisper_model, "medium");
            assert!(job.download_video_with_transcript);
            assert!(job.cut_start_time.is_none());
            assert_eq!(job_preset(&job), name);
        }
    }

    #[test]
    fn removes_exactly_the_numbered_waiting_item_and_leaves_active_job() {
        let state = state();
        for index in 1..=3 {
            let job = build_download_job(
                &state.config,
                build_request(&state, &format!("https://example.com/{index}"), "best").unwrap(),
            )
            .unwrap();
            state.queue.pending.lock().unwrap().push_back(job);
        }
        *state.processes.current_job_id.lock().unwrap() = Some("active".into());
        assert!(list_queue(&state)
            .unwrap()
            .contains("1\tbest\thttps://example.com/1"));
        assert_eq!(
            remove_waiting_job(&state, 1).unwrap().url,
            "https://example.com/1"
        );
        assert_eq!(
            remove_waiting_job(&state, 2).unwrap().url,
            "https://example.com/3"
        );
        assert_eq!(
            state.queue.pending.lock().unwrap()[0].url,
            "https://example.com/2"
        );
        assert!(remove_waiting_job(&state, 0).is_err());
        assert!(remove_waiting_job(&state, 2).is_err());
        assert_eq!(
            state.processes.current_job_id.lock().unwrap().as_deref(),
            Some("active")
        );
    }

    #[test]
    fn history_and_stats_work_with_sqlite_writes_disabled() {
        let state = state();
        {
            let db = state.db.lock().unwrap();
            for index in 0..30 {
                db.execute("INSERT INTO history_entries (id,url,title,created_at,completed_at,duration_seconds,file_size_bytes) VALUES (?1,?2,?3,?4,?4,60,1024)",
                    params![format!("id-{index:02}"), format!("https://example.com/{index}"), format!("Video {index}"), index]).unwrap();
            }
            db.execute_batch("PRAGMA query_only = ON").unwrap();
        }
        let output = list_history(&state).unwrap();
        assert_eq!(output.lines().count(), 26);
        assert_eq!(
            output.lines().nth(1).unwrap(),
            "1\tVideo 29\thttps://example.com/29"
        );
        assert_eq!(
            output.lines().last().unwrap(),
            "25\tVideo 5\thttps://example.com/5"
        );
        assert_eq!(
            format_stats(&get_history_stats_from_db(&state.db).unwrap()),
            "Downloaded videos: 30\nTotal data: 30.0 KB\nTotal runtime: 30m 00s"
        );
        assert_eq!(count_history_entries_in_db(&state.db).unwrap(), 30);
    }

    #[test]
    fn empty_results_and_stats_match_the_history_panel() {
        let state = state();
        assert_eq!(list_queue(&state).unwrap(), "No waiting downloads.");
        assert_eq!(list_history(&state).unwrap(), "History is empty.");
        assert_eq!(
            format_stats(&get_history_stats_from_db(&state.db).unwrap()),
            "Downloaded videos: 0\nTotal data: 0 B\nTotal runtime: 0m 00s"
        );
        assert_eq!(
            format_stats(&HistoryStats {
                video_count: 2,
                total_duration_seconds: 3661,
                total_file_size_bytes: 1_048_576,
                source_counts: Vec::new(),
            }),
            "Downloaded videos: 2\nTotal data: 1.00 MB\nTotal runtime: 1h 01m"
        );
        assert_eq!(terminal_text("title\n\x1b[31m\tbad"), "title  [31m bad");
    }

    fn mock_endpoint(
        path: &Path,
        expected: CliCommand,
        result: Result<String, String>,
    ) -> JoinHandle<()> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let endpoint = Endpoint {
            port: listener.local_addr().unwrap().port(),
            token: "test-token".into(),
        };
        fs::write(path, serde_json::to_vec(&endpoint).unwrap()).unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            configure_stream(&stream).unwrap();
            let request: Request = read_message(&mut stream).unwrap();
            assert_eq!(authorize(request, "test-token").unwrap(), expected);
            write_message(&mut stream, &result).unwrap();
        })
    }

    #[cfg(unix)]
    #[test]
    fn integration_accepted_cli_stream_reads_fragmented_messages_in_blocking_mode() {
        use std::io::Write;
        use std::net::TcpStream;
        use std::os::fd::AsRawFd;
        use std::sync::mpsc;
        use std::time::Instant;

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let deadline = Instant::now() + crate::test_support::TEST_TIMEOUT;
        let mut accepted = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "CLI fixture connection did not arrive"
                    );
                    thread::yield_now();
                }
                Err(error) => panic!("CLI fixture accept failed: {error}"),
            }
        };
        // macOS can inherit the nonblocking listener flag; explicitly reproduce
        // that accepted-stream state on every Unix platform.
        accepted.set_nonblocking(true).unwrap();
        configure_stream(&accepted).unwrap();
        let flags = unsafe { libc::fcntl(accepted.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0);
        assert_eq!(
            flags & libc::O_NONBLOCK,
            0,
            "accepted CLI messages must wait for data so the I/O timeout applies"
        );
        assert_eq!(accepted.read_timeout().unwrap(), Some(super::IO_TIMEOUT));
        assert_eq!(accepted.write_timeout().unwrap(), Some(super::IO_TIMEOUT));
        configure_stream(&client).unwrap();

        let request = serde_json::to_vec(&Request {
            token: "synthetic-token".into(),
            command: CliCommand::Stats,
        })
        .unwrap();
        let (fragment_seen, wait_for_fragment) = mpsc::channel();
        let server = thread::spawn(move || {
            let mut first = [0];
            assert_eq!(accepted.peek(&mut first).unwrap(), 1);
            assert_eq!(first, [b'{']);
            fragment_seen.send(()).unwrap();
            let command = read_message::<Request>(&mut accepted)
                .and_then(|request| super::authorize(request, "synthetic-token"));
            write_message(&mut accepted, &command).unwrap();
            command
        });
        client.write_all(&request[..1]).unwrap();
        wait_for_fragment
            .recv_timeout(crate::test_support::TEST_TIMEOUT)
            .expect("server did not observe the first request fragment");
        client.write_all(&request[1..]).unwrap();
        client.write_all(b"\n").unwrap();
        let response: Result<CliCommand, String> = read_message(&mut client).unwrap();
        assert_eq!(response.unwrap(), CliCommand::Stats);
        assert_eq!(server.join().unwrap().unwrap(), CliCommand::Stats);
    }

    #[test]
    fn sends_to_running_app_without_launching_and_propagates_errors() {
        let path = std::env::temp_dir().join(format!("pinefetch-cli-{}.json", Uuid::new_v4()));
        let server = mock_endpoint(
            &path,
            CliCommand::QueueRemove { number: 9 },
            Err("No waiting queue item at position 9".into()),
        );
        let result = send_command(&path, CliCommand::QueueRemove { number: 9 }, || {
            panic!("must not launch")
        });
        assert_eq!(result.unwrap_err(), "No waiting queue item at position 9");
        server.join().unwrap();
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn launches_when_closed_then_sends_command_once() {
        let path = std::env::temp_dir().join(format!("pinefetch-cli-{}.json", Uuid::new_v4()));
        let mut server = None;
        let output = send_command(&path, CliCommand::Stats, || {
            server = Some(mock_endpoint(&path, CliCommand::Stats, Ok("stats".into())));
            Ok(())
        })
        .unwrap();
        assert_eq!(output, "stats");
        server.unwrap().join().unwrap();
        assert!(!path.with_extension("startup.lock").exists());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn launch_failure_is_reported_and_releases_startup_lock() {
        let path = std::env::temp_dir().join(format!("pinefetch-cli-{}.json", Uuid::new_v4()));
        assert_eq!(
            send_command(&path, CliCommand::Stats, || Err("launch failed".into())).unwrap_err(),
            "launch failed"
        );
        assert!(!path.with_extension("startup.lock").exists());
    }
}
