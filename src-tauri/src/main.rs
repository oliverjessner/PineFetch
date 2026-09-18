mod browser_import;
mod cli;
mod config;
mod download;
mod history;
mod platform;
mod presets;
mod queue;

use browser_import::*;
use config::*;
use download::*;
use history::*;
#[cfg(test)]
use platform::TIKTOK_FORMAT_SORT;
use platform::{detect_platform, site_format_sort};
use presets::{
    download_preset_for_key, normalize_download_preset_key, DownloadPreset,
    DEFAULT_DOWNLOAD_PRESET_KEY, DOWNLOAD_PRESETS,
};
use queue::*;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tauri::{AppHandle, ClipboardManager, Manager, State};
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

fn default_magic_import_enabled() -> bool {
    true
}

fn default_cut_at_timestamp_enabled() -> bool {
    true
}

fn default_faster_whisper_model() -> String {
    DEFAULT_FASTER_WHISPER_MODEL.to_string()
}

const LEGACY_CONFIG_MIGRATION_KEY: &str = "legacy_config_json_migrated";
const DEFAULT_FASTER_WHISPER_MODEL: &str = "base";
const FASTER_WHISPER_MODELS: [&str; 4] = ["base", "small", "medium", "large-v3"];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    yt_dlp_path: Option<String>,
    default_output_dir: Option<String>,
    #[serde(default)]
    selected_preset_key: Option<String>,
    #[serde(default = "default_faster_whisper_model")]
    faster_whisper_model: String,
    #[serde(default)]
    download_video_with_transcript: bool,
    #[serde(default)]
    save_instagram_captions: bool,
    #[serde(default = "default_magic_import_enabled")]
    magic_import_enabled: bool,
    #[serde(default = "default_cut_at_timestamp_enabled")]
    cut_at_timestamp_enabled: bool,
    #[serde(default)]
    last_download_url: Option<String>,
    #[serde(default)]
    notifications_enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            yt_dlp_path: None,
            default_output_dir: None,
            selected_preset_key: Some(DEFAULT_DOWNLOAD_PRESET_KEY.to_string()),
            faster_whisper_model: default_faster_whisper_model(),
            download_video_with_transcript: false,
            save_instagram_captions: false,
            magic_import_enabled: default_magic_import_enabled(),
            cut_at_timestamp_enabled: default_cut_at_timestamp_enabled(),
            last_download_url: None,
            notifications_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DownloadRequest {
    url: String,
    format: String,
    output_dir: Option<String>,
    extract_audio: bool,
    audio_format: Option<String>,
    transcribe_text: bool,
    #[serde(default)]
    transcribe_timestamps: bool,
    #[serde(default = "default_cut_at_timestamp_enabled")]
    cut_at_timestamp_enabled: bool,
    #[serde(default)]
    cut_start_time: Option<f64>,
    #[serde(default)]
    filename_suffix: Option<String>,
    title: Option<String>,
    #[serde(default)]
    uploader: Option<String>,
    thumbnail: Option<String>,
    #[serde(default)]
    upload_date: Option<String>,
    #[serde(default)]
    timestamp: Option<i64>,
    #[serde(default)]
    duration_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DownloadJob {
    id: String,
    url: String,
    format: String,
    output_dir: String,
    extract_audio: bool,
    audio_format: Option<String>,
    transcribe_text: bool,
    transcribe_timestamps: bool,
    #[serde(default = "default_faster_whisper_model")]
    faster_whisper_model: String,
    #[serde(default)]
    download_video_with_transcript: bool,
    #[serde(default)]
    save_instagram_captions: bool,
    title: Option<String>,
    uploader: Option<String>,
    thumbnail: Option<String>,
    upload_date: Option<String>,
    timestamp: Option<i64>,
    duration_seconds: Option<i64>,
    cut_start_time: Option<f64>,
    filename_suffix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DownloadProgress {
    id: String,
    percent: Option<f32>,
    speed: Option<String>,
    eta: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DownloadStateEvent {
    id: String,
    state: String,
    exit_code: Option<i32>,
    error: Option<String>,
    output_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogEvent {
    id: String,
    line: String,
    is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InfoFormat {
    format_id: Option<String>,
    ext: Option<String>,
    vcodec: Option<String>,
    acodec: Option<String>,
    height: Option<i64>,
    width: Option<i64>,
    fps: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InfoResponse {
    title: Option<String>,
    uploader: Option<String>,
    duration: Option<i64>,
    thumbnail: Option<String>,
    upload_date: Option<String>,
    timestamp: Option<i64>,
    formats: Option<Vec<InfoFormat>>,
    description: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryEntry {
    id: String,
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    uploader: Option<String>,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    thumbnail: Option<String>,
    #[serde(default)]
    upload_date: Option<String>,
    #[serde(default)]
    timestamp: Option<i64>,
    #[serde(default)]
    duration_seconds: Option<i64>,
    #[serde(default)]
    file_size_bytes: Option<i64>,
    #[serde(default)]
    medium: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    output_path: Option<String>,
    created_at: u64,
    #[serde(default)]
    completed_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct HistoryPage {
    entries: Vec<HistoryEntry>,
    has_more: bool,
}

#[derive(Debug, Clone, Serialize)]
struct HistoryStats {
    video_count: u64,
    total_duration_seconds: u64,
    total_file_size_bytes: u64,
    source_counts: Vec<HistorySourceCount>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct HistorySourceCount {
    source: String,
    count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledYtDlpVersion {
    version: String,
    path: String,
}

#[derive(Debug, Clone, Serialize)]
struct QueueStatus {
    auto_start: bool,
    worker_running: bool,
    paused: bool,
}

#[derive(Debug, Clone, Serialize)]
struct TxtImportFile {
    path: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LinkDumpSettings {
    server_enabled: bool,
    host: String,
    port: u16,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct LinkDumpSecretView {
    id: String,
    name: String,
    created_at: String,
    last_used_at: Option<String>,
    revoked_at: Option<String>,
    deleted_at: Option<String>,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
struct GeneratedLinkDumpSecret {
    secret: String,
    connection: LinkDumpSecretView,
}

#[derive(Debug, Clone, Serialize)]
struct LinkDumpServerStatus {
    status: String,
    url: String,
    error_message: Option<String>,
}

impl Default for LinkDumpServerStatus {
    fn default() -> Self {
        Self {
            status: "stopped".to_string(),
            url: format!(
                "http://{}:{}",
                LINK_DUMP_DEFAULT_HOST, LINK_DUMP_DEFAULT_PORT
            ),
            error_message: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct LinkDumpOverview {
    settings: LinkDumpSettings,
    secrets: Vec<LinkDumpSecretView>,
    server_status: LinkDumpServerStatus,
}

#[derive(Debug, Clone, Deserialize)]
struct LinkDumpSettingsPatch {
    server_enabled: Option<bool>,
    host: Option<String>,
    port: Option<u16>,
}

#[derive(Debug, Clone)]
struct ValidSecretResult {
    id: String,
    #[allow(dead_code)]
    name: String,
}

#[derive(Debug, Clone)]
struct NormalizedVideoUrl {
    url: String,
    key: String,
    thumbnail: Option<String>,
}

#[derive(Debug, Clone)]
struct LinkDumpQueueSummary {
    received: usize,
    added: usize,
    skipped: usize,
    invalid: usize,
}

#[derive(Debug)]
struct LinkDumpServerRuntime {
    status: LinkDumpServerStatus,
    shutdown: Option<Arc<AtomicBool>>,
    handle: Option<JoinHandle<()>>,
}

struct ActiveConnectionPermit(Arc<AtomicUsize>);

impl Drop for ActiveConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Default for LinkDumpServerRuntime {
    fn default() -> Self {
        Self {
            status: LinkDumpServerStatus::default(),
            shutdown: None,
            handle: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct AddVideoLinkRequestBody {
    url: Option<String>,
    secret: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AddVideoLinksRequestBody {
    urls: Option<Vec<String>>,
    secret: Option<String>,
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

#[derive(Debug, Clone)]
struct DownloadRunResult {
    exit_code: i32,
    output_path: Option<String>,
    error: Option<String>,
    info: Option<InfoResponse>,
}

#[derive(Debug)]
struct TemporaryTranscriptionAudio {
    path: PathBuf,
}

impl Drop for TemporaryTranscriptionAudio {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

const FASTER_WHISPER_TRANSCRIBE_SNIPPET: &str = r#"
import sys
from pathlib import Path

try:
    from faster_whisper import WhisperModel
except Exception as exc:
    print(f"Failed to import faster_whisper: {exc}", file=sys.stderr)
    raise

audio_path = sys.argv[1]
output_path = Path(sys.argv[2])
model_name = sys.argv[3] if len(sys.argv) > 3 and sys.argv[3] else "base"
include_timestamps = len(sys.argv) > 4 and sys.argv[4] == "1"

def format_timestamp(seconds):
    total_seconds = max(0, int(round(seconds)))
    hours, remainder = divmod(total_seconds, 3600)
    minutes, seconds = divmod(remainder, 60)
    return f"{hours:02d}:{minutes:02d}:{seconds:02d}"

model = WhisperModel(model_name, compute_type="int8")
segments, _ = model.transcribe(audio_path, beam_size=5)
lines = []
for segment in segments:
    text = segment.text.strip()
    if text:
        if include_timestamps:
            start = format_timestamp(segment.start)
            end = format_timestamp(segment.end)
            lines.append(f"[{start} → {end}] {text}")
        else:
            lines.append(text)

content = "\n".join(lines).strip()
if content:
    content += "\n"
output_path.write_text(content, encoding="utf-8")
print(str(output_path))
"#;

const LINK_DUMP_DEFAULT_HOST: &str = "127.0.0.1";
const LINK_DUMP_DEFAULT_PORT: u16 = 2255;
const LINK_DUMP_MAX_BATCH_SIZE: usize = 500;
const LINK_DUMP_MAX_BODY_BYTES: usize = 1024 * 1024;
const LINK_DUMP_MAX_CONNECTIONS: usize = 8;
const INFO_TIMEOUT: Duration = Duration::from_secs(90);
const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

struct AppState {
    config: Mutex<AppConfig>,
    db: Mutex<Connection>,
    link_dump_server: Mutex<LinkDumpServerRuntime>,
    queue: Mutex<VecDeque<DownloadJob>>,
    queue_auto_start: Mutex<bool>,
    queue_paused: Mutex<bool>,
    worker_running: Mutex<bool>,
    worker_handle: Mutex<Option<JoinHandle<()>>>,
    current_job_id: Mutex<Option<String>>,
    active_video_key: Mutex<Option<String>>,
    current_child: Mutex<Option<Arc<Mutex<Child>>>>,
    utility_children: Mutex<Vec<Arc<Mutex<Child>>>>,
    cancel_requested: Mutex<Option<String>>,
    shutting_down: AtomicBool,
}

impl AppState {
    fn new(config: AppConfig, db: Connection) -> Self {
        Self {
            config: Mutex::new(config),
            db: Mutex::new(db),
            link_dump_server: Mutex::new(LinkDumpServerRuntime::default()),
            queue: Mutex::new(VecDeque::new()),
            queue_auto_start: Mutex::new(true),
            queue_paused: Mutex::new(false),
            worker_running: Mutex::new(false),
            worker_handle: Mutex::new(None),
            current_job_id: Mutex::new(None),
            active_video_key: Mutex::new(None),
            current_child: Mutex::new(None),
            utility_children: Mutex::new(Vec::new()),
            cancel_requested: Mutex::new(None),
            shutting_down: AtomicBool::new(false),
        }
    }
}

#[tauri::command]
fn get_download_presets() -> Vec<DownloadPreset> {
    DOWNLOAD_PRESETS.to_vec()
}

#[tauri::command]
async fn pick_output_dir() -> Result<Option<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    tauri::api::dialog::FileDialogBuilder::new().pick_folder(move |path| {
        let _ = tx.send(path.map(|p| p.to_string_lossy().to_string()));
    });
    tauri::async_runtime::spawn_blocking(move || rx.recv())
        .await
        .map_err(|_| "Dialog task failed".to_string())?
        .map_err(|_| "Dialog closed".to_string())
}

#[tauri::command]
async fn pick_txt_file() -> Result<Option<TxtImportFile>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    tauri::api::dialog::FileDialogBuilder::new()
        .add_filter("Text", &["txt"])
        .pick_file(move |path| {
            let _ = tx.send(path);
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
fn open_folder(app: AppHandle, path: String) -> Result<(), String> {
    let path =
        canonical_existing_local_path(&path)?.ok_or_else(|| "Path does not exist".to_string())?;
    tauri::api::shell::open(&app.shell_scope(), path, None)
        .map_err(|e| format!("Open folder failed: {e}"))
}

#[tauri::command]
fn open_file_path(app: AppHandle, path: String) -> Result<bool, String> {
    let Some(path) = canonical_existing_local_path(&path)? else {
        return Ok(false);
    };
    tauri::api::shell::open(&app.shell_scope(), path, None)
        .map_err(|e| format!("Open file failed: {e}"))?;
    Ok(true)
}

#[tauri::command]
fn read_clipboard_text(app: AppHandle) -> Result<Option<String>, String> {
    app.clipboard_manager()
        .read_text()
        .map_err(|e| format!("Clipboard read failed: {e}"))
}

#[tauri::command]
async fn load_info(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> Result<InfoResponse, String> {
    if !is_valid_url(&url) {
        return Err("URL must start with http:// or https://".to_string());
    }
    let yt_dlp = resolve_yt_dlp(&app, &state)?;
    let deno = resolve_deno_executable(&app);

    let app_handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        load_info_with_yt_dlp(yt_dlp, deno, url, state.inner())
    })
    .await
    .map_err(|e| format!("Info task failed: {e}"))?
}

fn load_info_with_yt_dlp(
    yt_dlp: String,
    deno: Option<String>,
    url: String,
    state: &AppState,
) -> Result<InfoResponse, String> {
    let mut command = Command::new(&yt_dlp);
    command.args(["--dump-json", "--no-playlist", "--no-warnings"]);
    if let Some(deno) = deno {
        command.arg("--js-runtimes");
        command.arg(format!("deno:{deno}"));
    }

    command.arg(&url);
    let output = run_command_output(command, None, Some(state), Some(INFO_TIMEOUT))
        .map_err(|e| format!("Failed to run yt-dlp: {e}"))?;

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("yt-dlp exited {code}: {stderr}"));
    }

    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("Invalid JSON from yt-dlp: {e}"))?;

    let formats = value.get("formats").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .map(|f| InfoFormat {
                format_id: f
                    .get("format_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                ext: f.get("ext").and_then(|v| v.as_str()).map(|s| s.to_string()),
                vcodec: f
                    .get("vcodec")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                acodec: f
                    .get("acodec")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                height: f.get("height").and_then(|v| v.as_i64()),
                width: f.get("width").and_then(|v| v.as_i64()),
                fps: f.get("fps").and_then(|v| v.as_f64()),
            })
            .collect::<Vec<_>>()
    });

    Ok(InfoResponse {
        title: value
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        uploader: value
            .get("uploader")
            .or_else(|| value.get("uploader_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        duration: value.get("duration").and_then(json_value_to_i64),
        thumbnail: value
            .get("thumbnail")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        upload_date: value
            .get("upload_date")
            .or_else(|| value.get("release_date"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        timestamp: value
            .get("timestamp")
            .or_else(|| value.get("release_timestamp"))
            .and_then(json_value_to_i64),
        formats,
        description: value
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        id: value
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

fn register_current_child(state: &AppState, child: &Arc<Mutex<Child>>) -> Result<(), String> {
    let mut slot = state
        .current_child
        .lock()
        .map_err(|_| "Child lock poisoned")?;
    *slot = Some(child.clone());
    // Cancel may have been requested between process creation and registration.
    let current = state
        .current_job_id
        .lock()
        .map_err(|_| "Current job lock poisoned")?;
    let cancelled = state.shutting_down.load(Ordering::SeqCst)
        || current.is_some()
            && state
                .cancel_requested
                .lock()
                .map_err(|_| "Cancel lock poisoned")?
                .as_deref()
                == current.as_deref();
    if cancelled {
        if let Ok(mut process) = child.lock() {
            let _ = terminate_child_process_tree(&mut process);
        }
    }
    Ok(())
}

fn register_utility_child(state: &AppState, child: &Arc<Mutex<Child>>) -> Result<(), String> {
    let mut children = state
        .utility_children
        .lock()
        .map_err(|_| "Utility child lock poisoned")?;
    if state.shutting_down.load(Ordering::SeqCst) {
        return Err("Application is shutting down".to_string());
    }
    children.push(child.clone());
    Ok(())
}

fn clear_utility_child(state: &AppState, child: &Arc<Mutex<Child>>) {
    if let Ok(mut children) = state.utility_children.lock() {
        children.retain(|active| !Arc::ptr_eq(active, child));
    }
}

fn clear_current_child(state: &AppState, child: &Arc<Mutex<Child>>) {
    if let Ok(mut slot) = state.current_child.lock() {
        if slot
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, child))
        {
            *slot = None;
        }
    }
}

fn configure_child_process_group(command: &mut Command) {
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(not(unix))]
    let _ = command;
}

fn terminate_child_process_tree(process: &mut Child) -> std::io::Result<()> {
    // A running child is its own Unix process-group leader. Only signal the
    // group while that leader is still alive, so the PGID cannot be reused.
    if process.try_wait()?.is_some() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        let group_id = i32::try_from(process.id())
            .map_err(|_| std::io::Error::other("Child process ID is out of range"))?;
        if unsafe { libc::kill(-group_id, libc::SIGKILL) } == 0 {
            return Ok(());
        }
        let group_error = std::io::Error::last_os_error();
        let _ = process.kill();
        return Err(group_error);
    }
    #[cfg(not(unix))]
    process.kill()
}

fn run_command_output(
    mut command: Command,
    active_state: Option<&AppState>,
    utility_state: Option<&AppState>,
    timeout: Option<Duration>,
) -> Result<Output, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_child_process_group(&mut command);
    let mut child = command.spawn().map_err(|err| err.to_string())?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child = Arc::new(Mutex::new(child));
    if let Some(state) = active_state {
        if let Err(err) = register_current_child(state, &child) {
            if let Ok(mut process) = child.lock() {
                let _ = terminate_child_process_tree(&mut process);
                let _ = process.wait();
            }
            return Err(err);
        }
    }
    if let Some(state) = utility_state {
        if let Err(err) = register_utility_child(state, &child) {
            if let Ok(mut process) = child.lock() {
                let _ = terminate_child_process_tree(&mut process);
                let _ = process.wait();
            }
            return Err(err);
        }
    }

    let (output_sender, output_receiver) = std::sync::mpsc::channel();
    let stdout_sender = output_sender.clone();
    thread::spawn(move || {
        let mut output = Vec::new();
        if let Some(mut stdout) = stdout {
            let _ = stdout.read_to_end(&mut output);
        }
        let _ = stdout_sender.send((true, output));
    });
    thread::spawn(move || {
        let mut output = Vec::new();
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_end(&mut output);
        }
        let _ = output_sender.send((false, output));
    });

    let started = Instant::now();
    let mut failure = None;
    let status = loop {
        let result = child
            .lock()
            .map_err(|_| "Child lock poisoned".to_string())
            .and_then(|mut process| process.try_wait().map_err(|err| err.to_string()));
        match result {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(err) => {
                failure = Some(err);
                break None;
            }
        }
        if timeout.is_some_and(|limit| started.elapsed() >= limit) {
            failure = Some("process timed out".to_string());
            break None;
        }
        thread::sleep(Duration::from_millis(100));
    };
    if status.is_none() {
        if let Ok(mut process) = child.lock() {
            let _ = terminate_child_process_tree(&mut process);
            let _ = process.wait();
        }
    }
    if let Some(state) = active_state {
        clear_current_child(state, &child);
    }
    if let Some(state) = utility_state {
        clear_utility_child(state, &child);
    }
    if let Some(err) = failure {
        // A descendant may still own a pipe after the direct child exits.
        // Do not let that keep a timed-out metadata request blocked here.
        return Err(err);
    }
    let mut drain_deadline = Instant::now() + OUTPUT_DRAIN_TIMEOUT;
    if let Some(limit) = timeout.and_then(|limit| started.checked_add(limit)) {
        drain_deadline = drain_deadline.min(limit);
    }
    let mut stdout = None;
    let mut stderr = None;
    for _ in 0..2 {
        let remaining = drain_deadline.saturating_duration_since(Instant::now());
        let (is_stdout, output) = output_receiver
            .recv_timeout(remaining)
            .map_err(|_| "Process output did not close after exit".to_string())?;
        if is_stdout {
            stdout = Some(output);
        } else {
            stderr = Some(output);
        }
    }
    Ok(Output {
        status: status.ok_or_else(|| "Process status unavailable".to_string())?,
        stdout: stdout.ok_or_else(|| "Process stdout missing".to_string())?,
        stderr: stderr.ok_or_else(|| "Process stderr missing".to_string())?,
    })
}

fn json_value_to_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| {
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .map(|value| value.trunc() as i64)
        })
        .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
}

#[tauri::command]
fn get_yt_dlp_installed_version(
    app: AppHandle,
    state: State<AppState>,
    path: Option<String>,
) -> Result<InstalledYtDlpVersion, String> {
    let yt_dlp = resolve_yt_dlp_for_version(&app, &state, path)?;
    let mut command = Command::new(&yt_dlp);
    command.arg("--version");
    let output = run_command_output(
        command,
        None,
        Some(state.inner()),
        Some(Duration::from_secs(15)),
    )
    .map_err(|e| format!("Failed to run yt-dlp: {e}"))?;

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let details = if stderr.is_empty() {
            "no stderr".to_string()
        } else {
            stderr
        };
        return Err(format!("yt-dlp exited {code}: {details}"));
    }

    let version = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .unwrap_or("")
        .to_string();

    if version.is_empty() {
        return Err("yt-dlp returned an empty version".to_string());
    }

    Ok(InstalledYtDlpVersion {
        version,
        path: yt_dlp,
    })
}

#[tauri::command]
fn get_history(
    state: State<AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
    query: Option<String>,
) -> Result<HistoryPage, String> {
    let limit = limit.unwrap_or(20).clamp(1, 100);
    let offset = offset.unwrap_or(0);
    search_history_page_from_db(state.inner(), limit, offset, query.as_deref())
}

#[tauri::command]
fn get_history_stats(state: State<AppState>) -> Result<HistoryStats, String> {
    get_history_stats_from_db(state.inner())
}

fn source_from_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.domain()?.trim_end_matches('.').to_ascii_lowercase();

    let canonical_short_domain = match host.as_str() {
        "youtu.be" => Some("youtube"),
        "fb.watch" => Some("facebook"),
        "instagr.am" => Some("instagram"),
        "lnkd.in" => Some("linkedin"),
        "t.co" => Some("x"),
        _ => None,
    };
    if let Some(source) = canonical_short_domain {
        return Some(source.to_string());
    }

    let labels = host
        .split('.')
        .filter(|label| !label.is_empty())
        .collect::<Vec<_>>();
    if labels.is_empty() {
        return None;
    }
    if labels.len() == 1 {
        return Some(labels[0].to_string());
    }

    let common_second_level_tld = matches!(
        labels[labels.len() - 2],
        "ac" | "co" | "com" | "edu" | "gov" | "net" | "org"
    );
    let source_index = if labels.len() >= 3
        && labels.last().is_some_and(|tld| tld.len() == 2)
        && common_second_level_tld
    {
        labels.len() - 3
    } else {
        labels.len() - 2
    };

    Some(labels[source_index].to_string())
}

fn current_timestamp_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn filename_from_path(path: Option<&str>) -> Option<String> {
    let filename = Path::new(path?)
        .file_name()
        .and_then(|value| value.to_str())?
        .trim();
    if filename.is_empty() {
        None
    } else {
        Some(filename.to_string())
    }
}

fn title_from_filename(filename: Option<&str>) -> Option<String> {
    let stem = Path::new(filename?)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(filename?)
        .trim();
    if stem.is_empty() {
        None
    } else {
        Some(stem.to_string())
    }
}

fn trim_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Debug)]
struct HydratedHistoryMetadata {
    title: Option<String>,
    uploader: Option<String>,
    thumbnail: Option<String>,
    upload_date: Option<String>,
    timestamp: Option<i64>,
    duration_seconds: Option<i64>,
}

fn hydrate_history_metadata(
    job: &DownloadJob,
    filename: Option<&str>,
    info: Option<&InfoResponse>,
) -> HydratedHistoryMetadata {
    let mut title = trim_optional_string(job.title.clone());
    let mut uploader = trim_optional_string(job.uploader.clone());
    let mut thumbnail = trim_optional_string(job.thumbnail.clone());
    let mut upload_date = trim_optional_string(job.upload_date.clone());
    let mut timestamp = job.timestamp;
    let mut duration_seconds = job.duration_seconds;

    if let Some(info) = info {
        if title.is_none() {
            title = trim_optional_string(info.title.clone());
        }
        if uploader.is_none() {
            uploader = trim_optional_string(info.uploader.clone());
        }
        if thumbnail.is_none() {
            thumbnail = trim_optional_string(info.thumbnail.clone());
        }
        if upload_date.is_none() {
            upload_date = trim_optional_string(info.upload_date.clone());
        }
        if timestamp.is_none() {
            timestamp = info.timestamp;
        }
        if duration_seconds.is_none() {
            duration_seconds = info.duration;
        }
    }

    if title.is_none() {
        title = title_from_filename(filename);
    }

    HydratedHistoryMetadata {
        title,
        uploader,
        thumbnail,
        upload_date,
        timestamp,
        duration_seconds,
    }
}

fn medium_for_job(job: &DownloadJob) -> &'static str {
    if job.transcribe_text {
        "transcript"
    } else if job.extract_audio {
        "audio"
    } else {
        "video"
    }
}

fn file_size_bytes_from_path(path: Option<&str>) -> Option<i64> {
    let size = fs::metadata(path?).ok()?.len();
    i64::try_from(size).ok()
}

fn add_history_entry_on_success(
    app: &AppHandle,
    state: &AppState,
    job: &DownloadJob,
    output_path: Option<&str>,
    info: Option<&InfoResponse>,
) -> Result<String, String> {
    let filename = filename_from_path(output_path);
    let metadata = hydrate_history_metadata(job, filename.as_deref(), info);
    let file_size_bytes = file_size_bytes_from_path(output_path);
    let now = current_timestamp_millis();
    let history_entry_id = Uuid::new_v4().to_string();
    let entry = HistoryEntry {
        id: history_entry_id.clone(),
        url: job.url.clone(),
        title: metadata.title,
        uploader: metadata.uploader,
        filename,
        thumbnail: metadata.thumbnail,
        upload_date: metadata.upload_date,
        timestamp: metadata.timestamp,
        duration_seconds: metadata.duration_seconds,
        file_size_bytes,
        medium: Some(medium_for_job(job).to_string()),
        source: source_from_url(&job.url),
        platform: detect_platform(&job.url),
        output_path: output_path.map(|s| s.to_string()),
        created_at: now,
        completed_at: Some(now),
    };

    insert_history_entry_in_db(state, &entry)?;
    let _ = app.emit_all("history:changed", ());
    Ok(history_entry_id)
}

#[tauri::command]
fn remove_history_entry(state: State<AppState>, id: String) -> Result<(), String> {
    delete_history_entry_from_db(state.inner(), &id)?;
    Ok(())
}

#[tauri::command]
fn clear_history(state: State<AppState>) -> Result<(), String> {
    clear_history_entries_in_db(state.inner())?;
    Ok(())
}

fn is_valid_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn resolve_cut_start_time(
    cut_at_timestamp_enabled: bool,
    requested_cut_start_time: Option<f64>,
    url: &str,
) -> Option<f64> {
    if !cut_at_timestamp_enabled {
        return None;
    }

    requested_cut_start_time
        .and_then(normalize_positive_timestamp)
        .or_else(|| extract_url_start_timestamp(url))
}

fn extract_url_start_timestamp(raw_url: &str) -> Option<f64> {
    let parsed = url::Url::parse(raw_url).ok()?;

    for (name, value) in parsed.query_pairs() {
        if is_start_timestamp_param(name.as_ref()) {
            if let Some(seconds) = parse_timestamp_value(value.as_ref()) {
                return Some(seconds);
            }
        }
    }

    if let Some(fragment) = parsed.fragment() {
        for (name, value) in url::form_urlencoded::parse(fragment.as_bytes()) {
            if is_start_timestamp_param(name.as_ref()) {
                if let Some(seconds) = parse_timestamp_value(value.as_ref()) {
                    return Some(seconds);
                }
            }
        }

        parse_timestamp_value(fragment)
    } else {
        None
    }
}

fn is_start_timestamp_param(name: &str) -> bool {
    matches!(name, "t" | "start" | "start_time" | "time_continue")
}

fn parse_timestamp_value(raw: &str) -> Option<f64> {
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }

    if let Ok(seconds) = value.parse::<f64>() {
        return normalize_positive_timestamp(seconds);
    }

    if value.contains(':') {
        return parse_colon_timestamp(&value);
    }

    parse_unit_timestamp(&value)
}

fn parse_colon_timestamp(value: &str) -> Option<f64> {
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }

    let mut total = 0.0;
    for part in parts {
        if part.is_empty() {
            return None;
        }
        let value = part.parse::<f64>().ok()?;
        if !value.is_finite() || value < 0.0 {
            return None;
        }
        total = total * 60.0 + value;
    }

    normalize_positive_timestamp(total)
}

fn parse_unit_timestamp(value: &str) -> Option<f64> {
    let mut number = String::new();
    let mut total = 0.0;
    let mut saw_unit = false;

    for character in value.chars() {
        if character.is_ascii_digit() || character == '.' {
            number.push(character);
            continue;
        }

        let multiplier = match character {
            'h' => 3600.0,
            'm' => 60.0,
            's' => 1.0,
            _ => return None,
        };
        if number.is_empty() {
            return None;
        }
        let amount = number.parse::<f64>().ok()?;
        if !amount.is_finite() || amount < 0.0 {
            return None;
        }
        total += amount * multiplier;
        number.clear();
        saw_unit = true;
    }

    if !saw_unit || !number.is_empty() {
        return None;
    }

    normalize_positive_timestamp(total)
}

fn normalize_positive_timestamp(seconds: f64) -> Option<f64> {
    if seconds.is_finite() && seconds > 0.0 {
        Some(seconds)
    } else {
        None
    }
}

fn format_yt_dlp_timestamp(seconds: f64) -> String {
    let mut formatted = format!("{seconds:.3}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
}

fn canonical_existing_local_path(raw: &str) -> Result<Option<String>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.contains('\0')
        || trimmed.contains("://")
        || trimmed.starts_with("mailto:")
        || trimmed.starts_with("tel:")
    {
        return Err("Only local filesystem paths can be opened".to_string());
    }

    match fs::canonicalize(trimmed) {
        Ok(path) => Ok(Some(path.to_string_lossy().to_string())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("Path could not be opened: {err}")),
    }
}

fn resolve_output_dir(state: &AppState, requested: Option<String>) -> Result<String, String> {
    if let Some(dir) = requested {
        if dir.trim().is_empty() {
            return Err("Output directory is empty".to_string());
        }
        return Ok(dir);
    }
    let cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
    cfg.default_output_dir
        .clone()
        .ok_or_else(|| "Default output directory not set".to_string())
}

fn resolve_yt_dlp(_app: &AppHandle, state: &AppState) -> Result<String, String> {
    let cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
    if let Some(path) = cfg.yt_dlp_path.as_ref() {
        if Path::new(path).exists() {
            return Ok(path.clone());
        }
    }

    if let Some(path) = find_in_path("yt-dlp") {
        return Ok(path);
    }

    Err("yt-dlp not found. Set its path in Settings.".to_string())
}

fn resolve_yt_dlp_for_version(
    app: &AppHandle,
    state: &AppState,
    path: Option<String>,
) -> Result<String, String> {
    if let Some(raw) = path {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            if Path::new(trimmed).exists() {
                return Ok(trimmed.to_string());
            }
            return Err(format!("yt-dlp path not found: {trimmed}"));
        }
    }
    resolve_yt_dlp(app, state)
}

fn find_in_path(binary: &str) -> Option<String> {
    let paths = std::env::var_os("PATH")?;
    let splitter = if cfg!(windows) { ';' } else { ':' };
    for path in paths.to_string_lossy().split(splitter) {
        let candidate = Path::new(path).join(if cfg!(windows) {
            format!("{binary}.exe")
        } else {
            binary.to_string()
        });
        if candidate.exists() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

fn link_dump_db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = tauri::api::path::app_data_dir(&app.config()).ok_or("Data directory unavailable")?;
    fs::create_dir_all(&dir).map_err(|e| format!("Data dir create failed: {e}"))?;
    Ok(dir.join("pinefetch.sqlite"))
}

fn open_link_dump_db(app: &AppHandle) -> Result<Connection, String> {
    let path = link_dump_db_path(app)?;
    let conn = Connection::open(path).map_err(|e| format!("SQLite open failed: {e}"))?;
    run_link_dump_migrations(&conn)
        .map_err(|e| format!("Link Dump SQLite migration failed: {e}"))?;
    println!("SQLite migration completed");
    Ok(conn)
}

fn run_link_dump_migrations(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
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
            save_instagram_captions INTEGER NOT NULL DEFAULT 0,
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
        "#,
    )?;

    ensure_app_config_notifications_enabled_column(conn)?;
    ensure_app_config_faster_whisper_model_column(conn)?;
    ensure_app_config_download_video_with_transcript_column(conn)?;
    ensure_app_config_save_instagram_captions_column(conn)?;
    ensure_history_entries_timestamp_column(conn)?;
    ensure_history_entries_duration_seconds_column(conn)?;
    ensure_history_entries_file_size_bytes_column(conn)?;
    ensure_history_entries_text_column(conn, "uploader")?;
    ensure_history_entries_text_column(conn, "medium")?;
    ensure_history_entries_text_column(conn, "source")?;
    backfill_history_sources(conn)?;
    Ok(())
}

fn ensure_app_config_notifications_enabled_column(conn: &Connection) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('app_config') WHERE name = 'notifications_enabled')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute(
            "ALTER TABLE app_config ADD COLUMN notifications_enabled INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

fn ensure_app_config_faster_whisper_model_column(conn: &Connection) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('app_config') WHERE name = 'faster_whisper_model')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute(
            "ALTER TABLE app_config ADD COLUMN faster_whisper_model TEXT NOT NULL DEFAULT 'base'",
            [],
        )?;
    }
    Ok(())
}

fn ensure_app_config_download_video_with_transcript_column(
    conn: &Connection,
) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('app_config') WHERE name = 'download_video_with_transcript')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute(
            "ALTER TABLE app_config ADD COLUMN download_video_with_transcript INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

fn ensure_app_config_save_instagram_captions_column(conn: &Connection) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('app_config') WHERE name = 'save_instagram_captions')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute(
            "ALTER TABLE app_config ADD COLUMN save_instagram_captions INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

fn backfill_history_sources(conn: &Connection) -> rusqlite::Result<()> {
    let entries = {
        let mut stmt = conn.prepare(
            "SELECT id, url FROM history_entries WHERE source IS NULL OR TRIM(source) = ''",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    for (id, url) in entries {
        if let Some(source) = source_from_url(&url) {
            conn.execute(
                "UPDATE history_entries SET source = ?1 WHERE id = ?2",
                params![source, id],
            )?;
        }
    }

    Ok(())
}

fn ensure_history_entries_timestamp_column(conn: &Connection) -> rusqlite::Result<()> {
    ensure_history_entries_integer_column(conn, "timestamp")
}

fn ensure_history_entries_duration_seconds_column(conn: &Connection) -> rusqlite::Result<()> {
    ensure_history_entries_integer_column(conn, "duration_seconds")
}

fn ensure_history_entries_file_size_bytes_column(conn: &Connection) -> rusqlite::Result<()> {
    ensure_history_entries_integer_column(conn, "file_size_bytes")
}

fn ensure_history_entries_integer_column(
    conn: &Connection,
    column_name: &str,
) -> rusqlite::Result<()> {
    ensure_history_entries_column(conn, column_name, "INTEGER")
}

fn ensure_history_entries_text_column(
    conn: &Connection,
    column_name: &str,
) -> rusqlite::Result<()> {
    ensure_history_entries_column(conn, column_name, "TEXT")
}

fn ensure_history_entries_column(
    conn: &Connection,
    column_name: &str,
    column_type: &str,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(history_entries)")?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        if column? == column_name {
            return Ok(());
        }
    }
    drop(stmt);

    conn.execute(
        &format!("ALTER TABLE history_entries ADD COLUMN {column_name} {column_type}"),
        [],
    )?;
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match cli::parse(&args) {
        Ok(Some(cli::CliCommand::Help)) => {
            print!("{}", cli::HELP);
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
    tauri::Builder::default()
        .setup(|app| {
            let db = open_link_dump_db(&app.handle())?;
            migrate_legacy_config_json(&app.handle(), &db)?;
            let config = load_config_from_db(&db)?;
            let state = AppState::new(config, db);
            migrate_legacy_history_json(&app.handle(), &state)?;
            app.manage(state);
            let state = app.state::<AppState>();
            let _ = start_link_dump_server_from_settings(&app.handle(), state.inner());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            cli::initialize_cli,
            get_config,
            get_download_presets,
            patch_config,
            set_selected_preset_key,
            set_save_instagram_captions,
            cache_last_download_url,
            pick_output_dir,
            pick_txt_file,
            open_folder,
            open_file_path,
            read_clipboard_text,
            load_info,
            get_yt_dlp_installed_version,
            get_queue_status,
            get_queue,
            set_queue_auto_start,
            start_queue,
            pause_queue,
            resume_queue,
            enqueue_download,
            cancel_download,
            get_history,
            get_history_stats,
            remove_history_entry,
            clear_history,
            get_link_dump_overview,
            update_link_dump_settings,
            create_link_dump_secret,
            revoke_link_dump_secret,
            delete_link_dump_secret,
            restart_link_dump_server
        ])
        .build(context)
        .expect("error while building tauri application")
        .run(|app_handle, event| {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn link_dump_test_state_with_config(config: AppConfig) -> AppState {
        let conn = Connection::open_in_memory().unwrap();
        run_link_dump_migrations(&conn).unwrap();
        AppState::new(config, conn)
    }

    fn link_dump_test_state() -> AppState {
        link_dump_test_state_with_config(AppConfig::default())
    }

    #[test]
    fn rejects_remote_open_targets() {
        let err = canonical_existing_local_path("https://example.com").unwrap_err();
        assert!(err.contains("local filesystem"));
    }

    #[test]
    fn canonicalizes_existing_local_paths() {
        let temp_dir = std::env::temp_dir();
        let canonical = fs::canonicalize(&temp_dir).unwrap();

        let resolved = canonical_existing_local_path(temp_dir.to_string_lossy().as_ref())
            .unwrap()
            .unwrap();

        assert_eq!(resolved, canonical.to_string_lossy());
    }

    #[cfg(unix)]
    fn write_fake_ffmpeg_tool(path: &Path, exit_code: i32) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, format!("#!/bin/sh\nexit {exit_code}\n")).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn normalizes_only_usable_ffmpeg_locations() {
        let dir = std::env::temp_dir().join(format!("pinefetch-ffmpeg-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        write_fake_ffmpeg_tool(&dir.join(ffmpeg_tool_name()), 0);
        write_fake_ffmpeg_tool(&dir.join(ffprobe_tool_name()), 0);

        assert_eq!(
            normalize_ffmpeg_location(&dir).as_deref(),
            Some(dir.to_string_lossy().as_ref())
        );

        write_fake_ffmpeg_tool(&dir.join(ffprobe_tool_name()), 1);

        assert!(normalize_ffmpeg_location(&dir).is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_numeric_query_timestamp() {
        let timestamp =
            extract_url_start_timestamp("https://youtu.be/-PrRmxn2U-w?si=gsvuw4hTglBy5UQd&t=61");

        assert_eq!(timestamp, Some(61.0));
    }

    #[test]
    fn extracts_compact_query_timestamp() {
        let timestamp = extract_url_start_timestamp("https://example.com/watch?v=abc&t=1h2m3s");

        assert_eq!(timestamp, Some(3723.0));
    }

    #[test]
    fn extracts_fragment_timestamp() {
        let timestamp = extract_url_start_timestamp("https://example.com/video#t=01:02");

        assert_eq!(timestamp, Some(62.0));
    }

    #[test]
    fn uses_requested_cut_start_time_when_provided() {
        let timestamp = resolve_cut_start_time(true, Some(61.0), "https://example.com/video");

        assert_eq!(timestamp, Some(61.0));
    }

    #[test]
    fn disables_requested_cut_start_time() {
        let timestamp = resolve_cut_start_time(false, Some(61.0), "https://example.com/video?t=62");

        assert_eq!(timestamp, None);
    }

    #[test]
    fn ignores_invalid_or_zero_timestamp() {
        assert_eq!(
            extract_url_start_timestamp("https://example.com/video?t=abc"),
            None
        );
        assert_eq!(
            extract_url_start_timestamp("https://example.com/video?t=0"),
            None
        );
    }

    #[test]
    fn formats_yt_dlp_timestamp_without_extra_zeroes() {
        assert_eq!(format_yt_dlp_timestamp(61.0), "61");
        assert_eq!(format_yt_dlp_timestamp(61.5), "61.5");
    }

    #[test]
    fn appends_filename_suffix_before_extension() {
        let template = build_output_template("/tmp/pinefetch", Some("__max"));

        assert!(template.ends_with("%(title)s - %(uploader)s - %(id)s__max.%(ext)s"));
    }

    #[test]
    fn saves_instagram_caption_as_separate_utf8_text_file() {
        let directory = std::env::temp_dir().join(format!("pinefetch-caption-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let media_path = directory.join("post.mp4");
        fs::write(&media_path, b"video").unwrap();
        let line = format!(
            "pinefetch_caption:{}",
            serde_json::json!({
                "filepath": media_path,
                "description": "Grüße aus Wien 👋\n#urlaub"
            })
        );
        let (path, caption) = parse_instagram_caption_line(&line).unwrap();
        let caption_path = write_instagram_caption_sidecar(Path::new(&path), &caption).unwrap();

        assert_eq!(caption_path, directory.join("post.caption.txt"));
        assert_eq!(
            fs::read_to_string(&caption_path).unwrap(),
            "Grüße aus Wien 👋\n#urlaub"
        );
        assert!(parse_yt_dlp_filepath(&line).is_none());
        assert!(parse_instagram_caption_line(
            "pinefetch_caption:{\"filepath\":\"x\",\"description\":\"\"}"
        )
        .is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn parses_yt_dlp_filepath_output() {
        assert_eq!(
            parse_yt_dlp_filepath("/tmp/pinefetch/video.mp4").as_deref(),
            Some("/tmp/pinefetch/video.mp4")
        );
        assert_eq!(
            parse_yt_dlp_filepath("[download] Destination: /tmp/pinefetch/video.f398.mp4")
                .as_deref(),
            Some("/tmp/pinefetch/video.f398.mp4")
        );
        assert_eq!(
            parse_yt_dlp_filepath("[Merger] Merging formats into \"/tmp/pinefetch/video.webm\"")
                .as_deref(),
            Some("/tmp/pinefetch/video.webm")
        );
        assert_eq!(parse_yt_dlp_filepath("[download] 100% of 1MiB"), None);
        assert_eq!(parse_yt_dlp_filepath("NA"), None);
        assert_eq!(parse_yt_dlp_filepath("https://example.com/video"), None);
    }

    #[test]
    fn selects_last_existing_output_path() {
        let dir = std::env::temp_dir().join(format!("pinefetch-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let first_path = dir.join("first.mp4");
        let final_path = dir.join("final.mp3");
        fs::write(&first_path, b"first").unwrap();
        fs::write(&final_path, b"final").unwrap();

        let candidates = vec![
            first_path.to_string_lossy().to_string(),
            dir.join("missing.webm").to_string_lossy().to_string(),
            final_path.to_string_lossy().to_string(),
        ];
        let expected = final_path.to_string_lossy().to_string();

        assert_eq!(
            select_existing_output_path(&candidates).as_deref(),
            Some(expected.as_str())
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn selects_largest_format_part_when_final_output_is_missing() {
        let dir = std::env::temp_dir().join(format!("pinefetch-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let audio_part = dir.join("Example - Uploader - id__max.f251.webm");
        let video_part = dir.join("Example - Uploader - id__max.f398.mp4");
        fs::write(&audio_part, b"audio").unwrap();
        fs::write(&video_part, b"larger video part").unwrap();

        let candidates = vec![
            audio_part.to_string_lossy().to_string(),
            video_part.to_string_lossy().to_string(),
        ];
        let expected = video_part.to_string_lossy().to_string();

        assert_eq!(
            select_existing_output_path(&candidates).as_deref(),
            Some(expected.as_str())
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_related_format_parts_from_expected_output_path() {
        let dir = std::env::temp_dir().join(format!("pinefetch-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let expected = dir.join("Example - Uploader - id__max.webm");
        let video_part = dir.join("Example - Uploader - id__max.f398.mp4");
        fs::write(&video_part, b"video").unwrap();

        let related = related_existing_output_paths(&expected);

        assert_eq!(related, vec![video_part.to_string_lossy().to_string()]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_unsafe_filename_suffix() {
        assert_eq!(
            normalize_filename_suffix(Some("__max")),
            Some("__max".to_string())
        );
        assert_eq!(normalize_filename_suffix(Some("../max")), None);
        assert_eq!(normalize_filename_suffix(Some("")), None);
    }

    #[test]
    fn normalizes_unknown_selected_preset_to_best() {
        let config = normalize_app_config(AppConfig {
            selected_preset_key: Some("missing".to_string()),
            ..AppConfig::default()
        });

        assert_eq!(
            config.selected_preset_key.as_deref(),
            Some(DEFAULT_DOWNLOAD_PRESET_KEY)
        );
    }

    #[test]
    fn normalizes_unknown_faster_whisper_model_to_base() {
        let config = normalize_app_config(AppConfig {
            faster_whisper_model: "unknown".to_string(),
            ..AppConfig::default()
        });

        assert_eq!(config.faster_whisper_model, DEFAULT_FASTER_WHISPER_MODEL);
    }

    #[test]
    fn app_config_defaults_are_loaded_from_sqlite() {
        let conn = Connection::open_in_memory().unwrap();
        run_link_dump_migrations(&conn).unwrap();

        let config = load_config_from_db(&conn).unwrap();

        assert_eq!(
            config.selected_preset_key.as_deref(),
            Some(DEFAULT_DOWNLOAD_PRESET_KEY)
        );
        assert_eq!(config.faster_whisper_model, DEFAULT_FASTER_WHISPER_MODEL);
        assert!(!config.download_video_with_transcript);
        assert!(!config.save_instagram_captions);
        assert!(config.magic_import_enabled);
        assert!(config.cut_at_timestamp_enabled);
        assert!(config.yt_dlp_path.is_none());
        assert!(config.default_output_dir.is_none());
        assert!(config.last_download_url.is_none());
    }

    #[test]
    fn app_config_is_stored_in_sqlite() {
        let conn = Connection::open_in_memory().unwrap();
        run_link_dump_migrations(&conn).unwrap();
        let config = AppConfig {
            yt_dlp_path: Some("/opt/pinefetch/yt-dlp".to_string()),
            default_output_dir: Some("/Users/example/Downloads".to_string()),
            selected_preset_key: Some("audio_mp3".to_string()),
            faster_whisper_model: "medium".to_string(),
            download_video_with_transcript: true,
            save_instagram_captions: true,
            magic_import_enabled: false,
            cut_at_timestamp_enabled: false,
            last_download_url: Some("https://example.com/watch".to_string()),
            notifications_enabled: true,
        };

        upsert_app_config_in_conn(&conn, &config).unwrap();
        let loaded = load_config_from_db(&conn).unwrap();

        assert_eq!(loaded.yt_dlp_path.as_deref(), Some("/opt/pinefetch/yt-dlp"));
        assert_eq!(
            loaded.default_output_dir.as_deref(),
            Some("/Users/example/Downloads")
        );
        assert_eq!(loaded.selected_preset_key.as_deref(), Some("audio_mp3"));
        assert_eq!(loaded.faster_whisper_model, "medium");
        assert!(loaded.download_video_with_transcript);
        assert!(loaded.save_instagram_captions);
        assert!(loaded.notifications_enabled);
        assert!(!loaded.magic_import_enabled);
        assert!(!loaded.cut_at_timestamp_enabled);
        assert_eq!(
            loaded.last_download_url.as_deref(),
            Some("https://example.com/watch")
        );
    }

    #[test]
    fn link_dump_request_uses_selected_preset() {
        let state = link_dump_test_state_with_config(AppConfig {
            selected_preset_key: Some("audio_mp3".to_string()),
            ..AppConfig::default()
        });
        let normalized = normalize_youtube_url("https://youtu.be/abc123").unwrap();

        let request = build_link_dump_download_request(&state, &normalized).unwrap();

        assert_eq!(request.url, "https://www.youtube.com/watch?v=abc123");
        assert_eq!(request.format, "ba/b");
        assert!(request.extract_audio);
        assert_eq!(request.audio_format.as_deref(), Some("mp3"));
        assert!(!request.transcribe_text);
        assert!(!request.transcribe_timestamps);
        assert_eq!(request.filename_suffix, None);
    }

    #[test]
    fn browser_import_deduplicates_while_inserting_into_queue() {
        let state = link_dump_test_state_with_config(AppConfig {
            default_output_dir: Some(std::env::temp_dir().to_string_lossy().into_owned()),
            ..AppConfig::default()
        });
        let normalized = normalize_youtube_url("https://youtu.be/abc123").unwrap();
        let request = build_link_dump_download_request(&state, &normalized).unwrap();
        let first = build_download_job(&state, request.clone()).unwrap();
        let second = build_download_job(&state, request).unwrap();
        let mut queue = VecDeque::from([first]);
        let mut summary = LinkDumpQueueSummary {
            received: 2,
            added: 0,
            skipped: 0,
            invalid: 0,
        };

        let added = insert_unique_video_jobs(
            &mut queue,
            None,
            vec![(normalized.key.clone(), second)],
            &mut summary,
        );

        assert_eq!(added, 0);
        assert_eq!(summary.skipped, 1);
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn browser_import_skips_a_link_while_its_download_is_active() {
        let state = link_dump_test_state_with_config(AppConfig {
            default_output_dir: Some(std::env::temp_dir().to_string_lossy().into_owned()),
            ..AppConfig::default()
        });
        let normalized = normalize_youtube_url("https://youtu.be/abc123").unwrap();
        let request = build_link_dump_download_request(&state, &normalized).unwrap();
        let first = build_download_job(&state, request.clone()).unwrap();
        let second = build_download_job(&state, request).unwrap();
        state.queue.lock().unwrap().push_back(first);

        let (active, paused) = next_worker_job(&state).unwrap();
        assert!(active.is_some());
        assert!(!paused);
        assert!(state.queue.lock().unwrap().is_empty());

        let mut summary = LinkDumpQueueSummary {
            received: 1,
            added: 0,
            skipped: 0,
            invalid: 0,
        };
        let mut queue = state.queue.lock().unwrap();
        let active_key = state.active_video_key.lock().unwrap();
        let added = insert_unique_video_jobs(
            &mut queue,
            active_key.as_deref(),
            vec![(normalized.key, second)],
            &mut summary,
        );

        assert_eq!(added, 0);
        assert_eq!(summary.skipped, 1);
        assert!(queue.is_empty());
    }

    #[test]
    fn timestamped_text_preset_enables_segment_timestamps() {
        let preset = download_preset_for_key(Some("text_timestamps"));

        assert_eq!(preset.format, "ba/b");
        assert!(preset.extract_audio);
        assert_eq!(preset.audio_format, Some("mp3"));
        assert!(preset.transcribe_text);
        assert!(preset.transcribe_timestamps);
        assert_eq!(preset.filename_suffix, Some("_timestamps"));
    }

    #[test]
    fn appends_timestamp_suffix_to_cut_output_path() {
        let path = build_timestamp_cut_output_path(
            Path::new("/tmp/Title - Uploader - id_best.webm"),
            13.0,
        )
        .unwrap();

        assert_eq!(
            path.to_string_lossy(),
            "/tmp/Title - Uploader - id_best_t13.webm"
        );
    }

    #[test]
    fn sanitizes_decimal_timestamp_suffix() {
        assert_eq!(format_timestamp_filename_suffix(13.5), "_t13_5");
    }

    #[test]
    fn timestamp_cut_keeps_an_existing_output_file() {
        let dir = std::env::temp_dir().join(format!("pinefetch-cut-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("video.webm");
        let temp = dir.join("cut.webm");
        let existing = build_timestamp_cut_output_path(&input, 13.0).unwrap();
        fs::write(&input, b"original").unwrap();
        fs::write(&temp, b"new cut").unwrap();
        fs::write(&existing, b"older cut").unwrap();

        let output = preserve_unique_cut_output(&temp, &input, 13.0).unwrap();

        assert_ne!(output, existing);
        assert_eq!(fs::read(&existing).unwrap(), b"older cut");
        assert_eq!(fs::read(&output).unwrap(), b"new cut");
        assert_eq!(fs::read(&input).unwrap(), b"original");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn platform_detection_requires_a_domain_boundary() {
        assert_eq!(
            detect_platform("https://m.youtube.com/watch?v=123"),
            Some("youtube".to_string())
        );
        assert_eq!(
            detect_platform("https://www.instagram.com/p/abc/"),
            Some("instagram".to_string())
        );
        assert_eq!(detect_platform("https://fakeinstagram.com/p/abc/"), None);
        assert_eq!(detect_platform("https://notyoutube.com/watch?v=123"), None);
        assert_eq!(
            detect_platform("https://pretiktok.com/@user/video/123"),
            None
        );
    }

    #[test]
    fn download_metadata_parser_keeps_fields_from_first_yt_dlp_run() {
        let line = r#"pinefetch_metadata:{"filepath":"/tmp/video.mp4","title":"Clip","uploader":"Creator","duration":13.7,"thumbnail":"https://example.com/thumb.jpg"}"#;
        let (path, info) = parse_download_metadata_line(line).unwrap();
        assert_eq!(path, "/tmp/video.mp4");
        assert_eq!(info.title.as_deref(), Some("Clip"));
        assert_eq!(info.uploader.as_deref(), Some("Creator"));
        assert_eq!(info.duration, Some(13));
    }

    #[cfg(unix)]
    #[test]
    fn timed_out_child_is_stopped_promptly() {
        let mut command = Command::new("sleep");
        command.arg("5");
        let started = Instant::now();

        let result = run_command_output(command, None, None, Some(Duration::from_millis(100)));

        assert_eq!(result.unwrap_err(), "process timed out");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn inherited_output_pipe_cannot_block_after_parent_exits() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 4 & exit 0"]);
        let started = Instant::now();

        let result = run_command_output(command, None, None, Some(Duration::from_secs(10)));

        assert_eq!(
            result.unwrap_err(),
            "Process output did not close after exit"
        );
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_stops_a_child_and_its_process_group() {
        let marker = std::env::temp_dir().join(format!("pinefetch-process-{}.txt", Uuid::new_v4()));
        let mut command = Command::new("sh");
        command
            .args(["-c", "(sleep 1; printf alive > \"$1\") & wait", "sh"])
            .arg(&marker);
        configure_child_process_group(&mut command);
        let mut child = command.spawn().unwrap();
        let pid = i32::try_from(child.id()).unwrap();
        assert_eq!(unsafe { libc::getpgid(pid) }, pid);

        thread::sleep(Duration::from_millis(100));
        terminate_child_process_tree(&mut child).unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success());
        thread::sleep(Duration::from_millis(1100));
        assert!(!marker.exists());
    }

    #[cfg(unix)]
    #[test]
    fn app_exit_stops_registered_utility_child() {
        let state = Arc::new(link_dump_test_state());
        let worker_state = state.clone();
        let handle = thread::spawn(move || {
            let mut command = Command::new("sleep");
            command.arg("5");
            run_command_output(command, None, Some(&worker_state), None)
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while state.utility_children.lock().unwrap().is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(state.utility_children.lock().unwrap().len(), 1);

        stop_active_download_on_exit(&state);

        let output = handle.join().unwrap().unwrap();
        assert!(!output.status.success());
        assert!(state.utility_children.lock().unwrap().is_empty());
    }

    #[test]
    fn extracts_source_from_url_without_subdomain_or_tld() {
        assert_eq!(
            source_from_url("https://www.youtube.com/watch?v=abc123").as_deref(),
            Some("youtube")
        );
        assert_eq!(
            source_from_url("https://media.linkedin.com/posts/123").as_deref(),
            Some("linkedin")
        );
        assert_eq!(
            source_from_url("https://video.example.co.uk/watch/123").as_deref(),
            Some("example")
        );
        assert_eq!(
            source_from_url("https://youtu.be/abc123").as_deref(),
            Some("youtube")
        );
        assert_eq!(source_from_url("not-a-url"), None);
    }

    #[test]
    fn prefers_h264_for_tiktok_downloads() {
        assert_eq!(
            site_format_sort("https://www.tiktok.com/@afd_fraktionbb/video/7608588575982144790"),
            Some(TIKTOK_FORMAT_SORT)
        );
        assert_eq!(
            site_format_sort("https://vm.tiktok.com/ZMexample/"),
            Some(TIKTOK_FORMAT_SORT)
        );
        assert_eq!(site_format_sort("https://exampletiktok.com/video/1"), None);
        assert_eq!(
            site_format_sort("https://www.youtube.com/watch?v=abc123"),
            None
        );
    }

    #[test]
    fn derives_medium_from_download_job() {
        let mut job = DownloadJob {
            id: "job-1".to_string(),
            url: "https://example.com/video".to_string(),
            format: "best".to_string(),
            output_dir: "/tmp".to_string(),
            extract_audio: false,
            audio_format: None,
            transcribe_text: false,
            transcribe_timestamps: false,
            faster_whisper_model: DEFAULT_FASTER_WHISPER_MODEL.to_string(),
            download_video_with_transcript: false,
            save_instagram_captions: false,
            title: None,
            uploader: None,
            thumbnail: None,
            upload_date: None,
            timestamp: None,
            duration_seconds: None,
            cut_start_time: None,
            filename_suffix: None,
        };

        assert_eq!(medium_for_job(&job), "video");
        job.extract_audio = true;
        job.audio_format = Some("mp3".to_string());
        assert_eq!(medium_for_job(&job), "audio");
        job.transcribe_text = true;
        assert_eq!(medium_for_job(&job), "transcript");

        let audio_download_job = effective_download_job(&job);
        assert!(audio_download_job.extract_audio);
        assert_eq!(audio_download_job.audio_format.as_deref(), Some("mp3"));

        job.download_video_with_transcript = true;
        let download_job = effective_download_job(&job);
        assert_eq!(
            download_job.format,
            download_preset_for_key(Some(DEFAULT_DOWNLOAD_PRESET_KEY)).format
        );
        assert!(!download_job.extract_audio);
        assert!(download_job.audio_format.is_none());
    }

    #[test]
    fn removes_temporary_transcription_audio_when_dropped() {
        let dir = std::env::temp_dir().join(format!("pinefetch-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("transcription.wav");
        fs::write(&path, b"temporary audio").unwrap();

        {
            let _temporary_audio = TemporaryTranscriptionAudio { path: path.clone() };
            assert!(path.exists());
        }

        assert!(!path.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn link_dump_migration_creates_default_settings() {
        let state = link_dump_test_state();
        let settings = get_link_dump_settings(&state).unwrap();

        assert!(settings.server_enabled);
        assert_eq!(settings.host, "127.0.0.1");
        assert_eq!(settings.port, 2255);

        let conn = state.db.lock().unwrap();
        let secret_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM link_dump_secrets", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(secret_count, 0);
    }

    #[test]
    fn link_dump_migration_does_not_create_request_log() {
        let state = link_dump_test_state();
        let conn = state.db.lock().unwrap();
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'link_dump_request_log'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(table_count, 0);
    }

    #[test]
    fn link_dump_migration_adds_history_metadata_columns_to_existing_table() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE history_entries (
                id TEXT PRIMARY KEY,
                url TEXT NOT NULL,
                title TEXT,
                filename TEXT,
                thumbnail TEXT,
                upload_date TEXT,
                platform TEXT,
                output_path TEXT,
                created_at INTEGER NOT NULL,
                completed_at INTEGER
            );

            INSERT INTO history_entries (
                id, url, title, filename, thumbnail, upload_date, platform,
                output_path, created_at, completed_at
            ) VALUES (
                'legacy-1', 'https://media.linkedin.com/posts/123', 'Legacy',
                NULL, NULL, NULL, NULL, NULL, 1700000000000, 1700000000100
            );
            "#,
        )
        .unwrap();

        run_link_dump_migrations(&conn).unwrap();
        for column_name in ["timestamp", "duration_seconds", "file_size_bytes"] {
            let column_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('history_entries') WHERE name = ?1 AND type = 'INTEGER'",
                    params![column_name],
                    |row| row.get(0),
                )
                .unwrap();

            assert_eq!(column_count, 1, "missing INTEGER column {column_name}");
        }
        for column_name in ["uploader", "medium", "source"] {
            let column_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('history_entries') WHERE name = ?1 AND type = 'TEXT'",
                    params![column_name],
                    |row| row.get(0),
                )
                .unwrap();

            assert_eq!(column_count, 1, "missing TEXT column {column_name}");
        }

        let source: Option<String> = conn
            .query_row(
                "SELECT source FROM history_entries WHERE id = 'legacy-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source.as_deref(), Some("linkedin"));
    }

    #[test]
    fn link_dump_migration_adds_settings_to_existing_config() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE app_config (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                yt_dlp_path TEXT,
                default_output_dir TEXT,
                selected_preset_key TEXT,
                magic_import_enabled INTEGER NOT NULL DEFAULT 1,
                cut_at_timestamp_enabled INTEGER NOT NULL DEFAULT 1,
                last_download_url TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            INSERT INTO app_config (
                id, selected_preset_key, magic_import_enabled,
                cut_at_timestamp_enabled, created_at, updated_at
            ) VALUES (1, 'text', 1, 1, datetime('now'), datetime('now'));
            "#,
        )
        .unwrap();

        run_link_dump_migrations(&conn).unwrap();
        let config = load_config_from_db(&conn).unwrap();

        assert_eq!(config.faster_whisper_model, DEFAULT_FASTER_WHISPER_MODEL);
        assert!(!config.download_video_with_transcript);
        assert!(!config.notifications_enabled);
        assert!(!config.save_instagram_captions);
        // Re-running migrations preserves the user's choice.
        let config = AppConfig {
            notifications_enabled: true,
            save_instagram_captions: true,
            ..config
        };
        upsert_app_config_in_conn(&conn, &config).unwrap();
        run_link_dump_migrations(&conn).unwrap();
        assert!(load_config_from_db(&conn).unwrap().notifications_enabled);
        assert!(load_config_from_db(&conn).unwrap().save_instagram_captions);
    }

    #[test]
    fn queue_notification_requires_multiple_successful_downloads_and_opt_in() {
        for (started, succeeded, enabled, expected) in [
            (0, 0, true, false),
            (1, 1, true, false),
            (2, 2, false, false),
            (2, 2, true, true),
            (5, 5, true, true),
            (2, 1, true, false),
            (3, 2, true, false),
            (2, 0, true, false),
        ] {
            let summary = QueueRunSummary { started, succeeded };
            assert_eq!(summary.should_notify(enabled), expected);
        }
    }

    #[test]
    fn history_entries_are_stored_in_sqlite_with_file_metadata() {
        let state = link_dump_test_state();
        let entry = HistoryEntry {
            id: "history-1".to_string(),
            url: "https://www.youtube.com/watch?v=abc123".to_string(),
            title: Some("Example title".to_string()),
            uploader: Some("Example uploader".to_string()),
            filename: Some("Example title - Uploader - abc123.mp4".to_string()),
            thumbnail: Some("https://i.ytimg.com/vi/abc123/mqdefault.jpg".to_string()),
            upload_date: Some("20240501".to_string()),
            timestamp: Some(1_714_560_000),
            duration_seconds: Some(754),
            file_size_bytes: Some(42_000_000),
            medium: Some("video".to_string()),
            source: Some("youtube".to_string()),
            platform: Some("youtube".to_string()),
            output_path: Some("/tmp/Example title - Uploader - abc123.mp4".to_string()),
            created_at: 1_700_000_000_000,
            completed_at: Some(1_700_000_000_100),
        };

        insert_history_entry_in_db(&state, &entry).unwrap();
        let entries = list_history_entries_from_db(&state).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "history-1");
        assert_eq!(entries[0].title.as_deref(), Some("Example title"));
        assert_eq!(entries[0].uploader.as_deref(), Some("Example uploader"));
        assert_eq!(
            entries[0].filename.as_deref(),
            Some("Example title - Uploader - abc123.mp4")
        );
        assert_eq!(entries[0].url, "https://www.youtube.com/watch?v=abc123");
        assert_eq!(
            entries[0].thumbnail.as_deref(),
            Some("https://i.ytimg.com/vi/abc123/mqdefault.jpg")
        );
        assert_eq!(entries[0].upload_date.as_deref(), Some("20240501"));
        assert_eq!(entries[0].timestamp, Some(1_714_560_000));
        assert_eq!(entries[0].duration_seconds, Some(754));
        assert_eq!(entries[0].file_size_bytes, Some(42_000_000));
        assert_eq!(entries[0].medium.as_deref(), Some("video"));
        assert_eq!(entries[0].source.as_deref(), Some("youtube"));

        let stats = get_history_stats_from_db(&state).unwrap();
        assert_eq!(stats.video_count, 1);
        assert_eq!(stats.total_duration_seconds, 754);
        assert_eq!(stats.total_file_size_bytes, 42_000_000);
        assert_eq!(
            stats.source_counts,
            vec![HistorySourceCount {
                source: "youtube".to_string(),
                count: 1,
            }]
        );
    }

    #[test]
    fn history_stats_count_sources_across_all_entries_with_fallbacks() {
        let state = link_dump_test_state();
        {
            let conn = state.db.lock().unwrap();
            for index in 0..22 {
                conn.execute(
                    "INSERT INTO history_entries (id, url, created_at) VALUES (?1, ?2, ?3)",
                    params![
                        format!("youtube-{index}"),
                        format!("https://youtu.be/video-{index}"),
                        index
                    ],
                )
                .unwrap();
            }
            conn.execute_batch(
                "INSERT INTO history_entries (id, url, source, created_at) VALUES
                    ('instagram-1', 'https://instagram.com/p/1', 'Instagram', 23),
                    ('instagram-2', 'https://instagram.com/p/2', 'instagram', 24),
                    ('instagram-3', 'https://instagram.com/p/3', '  INSTAGRAM  ', 25);
                 INSERT INTO history_entries (id, url, platform, created_at) VALUES
                    ('tiktok-1', 'invalid-url', 'TikTok', 26),
                    ('unknown-1', 'invalid-url', NULL, 27);",
            )
            .unwrap();
        }

        let stats = get_history_stats_from_db(&state).unwrap();
        assert_eq!(stats.video_count, 27);
        assert_eq!(
            stats.source_counts,
            vec![
                HistorySourceCount {
                    source: "youtube".to_string(),
                    count: 22,
                },
                HistorySourceCount {
                    source: "instagram".to_string(),
                    count: 3,
                },
                HistorySourceCount {
                    source: "tiktok".to_string(),
                    count: 1,
                },
                HistorySourceCount {
                    source: "unknown".to_string(),
                    count: 1,
                },
            ]
        );
        assert_eq!(
            serde_json::to_value(&stats).unwrap()["source_counts"][0],
            json!({"source": "youtube", "count": 22})
        );
    }

    #[test]
    fn transcriptions_store_text_type_and_history_foreign_key() {
        let state = link_dump_test_state();
        {
            let conn = state.db.lock().unwrap();
            conn.execute_batch(
                r#"
                INSERT INTO history_entries (id, url, created_at)
                VALUES
                    ('history-text', 'https://example.com/text', 1),
                    ('history-timestamps', 'https://example.com/timestamps', 2),
                    ('history-invalid', 'https://example.com/invalid', 3);
                "#,
            )
            .unwrap();
        }

        insert_transcription_in_db(&state, "history-text", "Plain transcript", "text").unwrap();
        insert_transcription_in_db(
            &state,
            "history-timestamps",
            "[00:00:01 → 00:00:02] Timestamped transcript",
            "text with timestamps",
        )
        .unwrap();

        let invalid =
            insert_transcription_in_db(&state, "history-invalid", "Invalid transcript", "invalid");
        assert!(invalid.is_err());

        {
            let conn = state.db.lock().unwrap();
            let stored: (String, String, String) = conn
                .query_row(
                    "SELECT history_entry_id, text, \"type\" FROM transcriptions WHERE history_entry_id = 'history-timestamps'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(stored.0, "history-timestamps");
            assert_eq!(stored.1, "[00:00:01 → 00:00:02] Timestamped transcript");
            assert_eq!(stored.2, "text with timestamps");
        }

        delete_history_entry_from_db(&state, "history-timestamps").unwrap();
        let conn = state.db.lock().unwrap();
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 1);
    }

    #[test]
    fn history_entries_are_paged_newest_first() {
        let state = link_dump_test_state();

        for index in 0..55 {
            let timestamp = 1_700_000_000_000 + index;
            let entry = HistoryEntry {
                id: format!("history-{index:02}"),
                url: format!("https://example.com/video/{index}"),
                title: Some(format!("Example {index}")),
                uploader: None,
                filename: Some(format!("example-{index}.mp4")),
                thumbnail: None,
                upload_date: None,
                timestamp: None,
                duration_seconds: None,
                file_size_bytes: None,
                medium: Some("video".to_string()),
                source: Some("example".to_string()),
                platform: Some("example".to_string()),
                output_path: None,
                created_at: timestamp,
                completed_at: Some(timestamp),
            };
            insert_history_entry_in_db(&state, &entry).unwrap();
        }

        let first_page = list_history_page_from_db(&state, 20, 0).unwrap();
        let second_page = list_history_page_from_db(&state, 20, 20).unwrap();

        assert_eq!(first_page.entries.len(), 20);
        assert!(first_page.has_more);
        assert_eq!(first_page.entries[0].id, "history-54");
        assert_eq!(first_page.entries[19].id, "history-35");
        assert_eq!(second_page.entries.len(), 20);
        assert!(second_page.has_more);
        assert_eq!(second_page.entries[0].id, "history-34");
        assert_eq!(second_page.entries[19].id, "history-15");
    }

    #[test]
    fn history_search_filters_all_entries_before_pagination() {
        let state = link_dump_test_state();
        for index in 0..35 {
            let is_match = index % 3 == 0;
            insert_history_entry_in_db(
                &state,
                &HistoryEntry {
                    id: format!("history-{index:02}"),
                    url: format!("https://example.com/{index}"),
                    title: Some(if is_match && index % 2 == 0 {
                        format!("Pine needle {index}")
                    } else {
                        format!("Other {index}")
                    }),
                    uploader: None,
                    filename: None,
                    thumbnail: None,
                    upload_date: None,
                    timestamp: None,
                    duration_seconds: None,
                    file_size_bytes: None,
                    medium: None,
                    source: Some(if is_match && index % 2 != 0 {
                        "PINE source".to_string()
                    } else {
                        "other source".to_string()
                    }),
                    platform: None,
                    output_path: None,
                    created_at: index,
                    completed_at: None,
                },
            )
            .unwrap();
        }

        let first = search_history_page_from_db(&state, 5, 0, Some(" pine ")).unwrap();
        let second = search_history_page_from_db(&state, 5, 5, Some("pine")).unwrap();
        assert_eq!(first.entries.len(), 5);
        assert!(first.has_more);
        assert_eq!(first.entries[0].id, "history-33");
        assert_eq!(second.entries[0].id, "history-18");
        assert!(second.has_more);
        let final_page = search_history_page_from_db(&state, 5, 10, Some("pine")).unwrap();
        assert_eq!(final_page.entries.len(), 2);
        assert!(!final_page.has_more);

        let empty_query = search_history_page_from_db(&state, 5, 0, Some("  ")).unwrap();
        assert_eq!(empty_query.entries[0].id, "history-34");
    }

    #[test]
    fn history_search_treats_sql_wildcards_as_plain_text() {
        let state = link_dump_test_state();
        for (id, title) in [("literal", "100%_done"), ("other", "100ABdone")] {
            insert_history_entry_in_db(
                &state,
                &HistoryEntry {
                    id: id.to_string(),
                    url: format!("https://example.com/{id}"),
                    title: Some(title.to_string()),
                    uploader: None,
                    filename: None,
                    thumbnail: None,
                    upload_date: None,
                    timestamp: None,
                    duration_seconds: None,
                    file_size_bytes: None,
                    medium: None,
                    source: None,
                    platform: None,
                    output_path: None,
                    created_at: 1,
                    completed_at: None,
                },
            )
            .unwrap();
        }
        let result = search_history_page_from_db(&state, 20, 0, Some("%_done")).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].id, "literal");
    }

    #[test]
    fn paused_queue_keeps_pending_jobs_until_resumed() {
        let state = link_dump_test_state();
        let job = DownloadJob {
            id: "pending".to_string(),
            url: "https://example.com/video".to_string(),
            format: "best".to_string(),
            output_dir: "/tmp".to_string(),
            extract_audio: false,
            audio_format: None,
            transcribe_text: false,
            transcribe_timestamps: false,
            faster_whisper_model: "base".to_string(),
            download_video_with_transcript: false,
            save_instagram_captions: false,
            title: None,
            uploader: None,
            thumbnail: None,
            upload_date: None,
            timestamp: None,
            duration_seconds: None,
            cut_start_time: None,
            filename_suffix: None,
        };
        state.queue.lock().unwrap().push_back(job);
        *state.worker_running.lock().unwrap() = true;

        set_queue_paused(&state, true).unwrap();
        let (next, paused) = next_worker_job(&state).unwrap();
        assert!(next.is_none());
        assert!(paused);
        assert_eq!(state.queue.lock().unwrap().len(), 1);
        assert!(!snapshot_queue_status(&state).unwrap().worker_running);
        assert!(snapshot_queue_status(&state).unwrap().paused);

        set_queue_paused(&state, false).unwrap();
        let (next, paused) = next_worker_job(&state).unwrap();
        assert_eq!(next.unwrap().id, "pending");
        assert!(!paused);
        assert!(state.queue.lock().unwrap().is_empty());
    }

    #[test]
    fn link_dump_secret_is_hashed_and_validates() {
        let state = link_dump_test_state();
        let generated =
            create_link_dump_secret_in_db(&state, Some("Chrome Extension on MacBook".to_string()))
                .unwrap();

        assert!(generated.secret.starts_with("pfld_"));
        assert_eq!(generated.connection.status, "active");

        {
            let conn = state.db.lock().unwrap();
            let stored_hash: String = conn
                .query_row(
                    "SELECT secret_hash FROM link_dump_secrets WHERE id = ?1",
                    params![generated.connection.id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_ne!(stored_hash, generated.secret);
            assert_eq!(stored_hash, hash_link_dump_secret(&generated.secret));
        }

        let valid = validate_link_dump_secret(&state, Some(&generated.secret))
            .unwrap()
            .unwrap();
        assert_eq!(valid.id, generated.connection.id);

        let conn = state.db.lock().unwrap();
        let last_used_at: Option<String> = conn
            .query_row(
                "SELECT last_used_at FROM link_dump_secrets WHERE id = ?1",
                params![generated.connection.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(last_used_at.is_some());
    }

    #[test]
    fn link_dump_secret_rejects_wrong_revoked_and_deleted_values() {
        let state = link_dump_test_state();
        let generated =
            create_link_dump_secret_in_db(&state, Some("Profile A".to_string())).unwrap();

        assert!(validate_link_dump_secret(&state, Some("pfld_wrong"))
            .unwrap()
            .is_none());

        revoke_link_dump_secret_in_db(&state, &generated.connection.id).unwrap();
        assert!(validate_link_dump_secret(&state, Some(&generated.secret))
            .unwrap()
            .is_none());

        let second = create_link_dump_secret_in_db(&state, Some("Profile B".to_string())).unwrap();
        delete_link_dump_secret_in_db(&state, &second.connection.id).unwrap();
        assert!(validate_link_dump_secret(&state, Some(&second.secret))
            .unwrap()
            .is_none());
    }

    #[test]
    fn link_dump_secret_list_hides_deleted_connections() {
        let state = link_dump_test_state();
        let first = create_link_dump_secret_in_db(&state, Some("Profile A".to_string())).unwrap();
        let second = create_link_dump_secret_in_db(&state, Some("Profile B".to_string())).unwrap();

        delete_link_dump_secret_in_db(&state, &second.connection.id).unwrap();

        let secrets = list_link_dump_secrets(&state).unwrap();
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0].id, first.connection.id);
        assert_eq!(secrets[0].status, "active");

        let conn = state.db.lock().unwrap();
        let deleted_at: Option<String> = conn
            .query_row(
                "SELECT deleted_at FROM link_dump_secrets WHERE id = ?1",
                params![second.connection.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(deleted_at.is_some());
    }

    #[test]
    fn normalizes_youtube_watch_url() {
        let normalized =
            normalize_youtube_url("https://www.youtube.com/watch?v=abc123&t=42s&feature=share")
                .unwrap();

        assert_eq!(normalized.url, "https://www.youtube.com/watch?v=abc123");
        assert_eq!(normalized.key, "youtube:abc123");
        assert_eq!(
            normalized.thumbnail.as_deref(),
            Some("https://i.ytimg.com/vi/abc123/mqdefault.jpg")
        );
    }

    #[test]
    fn normalizes_youtu_be_url() {
        let normalized = normalize_youtube_url("https://youtu.be/def456?si=tracking").unwrap();

        assert_eq!(normalized.url, "https://www.youtube.com/watch?v=def456");
        assert_eq!(normalized.key, "youtube:def456");
    }

    #[test]
    fn rejects_non_youtube_domains() {
        assert!(normalize_youtube_url("https://example.com/watch?v=abc123").is_none());
        assert!(normalize_youtube_url("https://youtube.example.com/watch?v=abc123").is_none());
    }

    #[test]
    fn normalizes_shorts_and_live_urls() {
        assert_eq!(
            normalize_youtube_url("https://www.youtube.com/shorts/short1")
                .unwrap()
                .url,
            "https://www.youtube.com/watch?v=short1"
        );
        assert_eq!(
            normalize_youtube_url("https://www.youtube.com/live/live99")
                .unwrap()
                .url,
            "https://www.youtube.com/watch?v=live99"
        );
    }

    #[test]
    fn normalizes_tiktok_video_and_short_urls() {
        let video = normalize_video_url(
            "https://www.tiktok.com/@creator.name/video/7412345678901234567?is_from_webapp=1",
        )
        .unwrap();
        assert_eq!(
            video.url,
            "https://www.tiktok.com/@creator.name/video/7412345678901234567"
        );
        assert_eq!(video.key, "tiktok:7412345678901234567");
        assert_eq!(video.thumbnail, None);

        let short = normalize_video_url("https://vm.tiktok.com/ZMexample/?share=1").unwrap();
        assert_eq!(short.url, "https://vm.tiktok.com/ZMexample/");
        assert_eq!(short.key, "tiktok-short:ZMexample");
    }

    #[test]
    fn normalizes_instagram_post_reel_and_tv_urls() {
        let reel = normalize_video_url(
            "https://www.instagram.com/reel/ABC_def-123/?utm_source=ig_web_copy_link",
        )
        .unwrap();
        assert_eq!(reel.url, "https://www.instagram.com/reel/ABC_def-123/");
        assert_eq!(reel.key, "instagram:ABC_def-123");

        assert_eq!(
            normalize_video_url("https://instagram.com/creator/p/PostCode9/")
                .unwrap()
                .url,
            "https://www.instagram.com/p/PostCode9/"
        );
        assert!(normalize_video_url("https://www.instagram.com/tv/TvCode1/").is_some());
    }

    #[test]
    fn video_normalizer_rejects_unsupported_or_spoofed_domains() {
        assert!(normalize_video_url("https://example.com/video/123456").is_none());
        assert!(normalize_video_url(
            "https://tiktok.example.com/@creator/video/7412345678901234567"
        )
        .is_none());
        assert!(normalize_video_url("https://instagram.com.example.org/reel/ABC123/").is_none());
    }

    #[test]
    fn link_dump_exposes_video_endpoints_only() {
        assert!(is_link_dump_endpoint("/addVideoLinkToQueue/"));
        assert!(is_link_dump_endpoint("/addVideoLinksToQueue"));
        assert!(!is_link_dump_endpoint("/addYoutubeLinkToQueue/"));
        assert!(!is_link_dump_endpoint("/addYoutubeLinksToQueue/"));
    }
}
