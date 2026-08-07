use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tauri::{AppHandle, ClipboardManager, Manager, State};
use uuid::Uuid;

fn default_magic_import_enabled() -> bool {
    true
}

fn default_cut_at_timestamp_enabled() -> bool {
    true
}

const LEGACY_CONFIG_MIGRATION_KEY: &str = "legacy_config_json_migrated";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    yt_dlp_path: Option<String>,
    default_output_dir: Option<String>,
    #[serde(default)]
    selected_preset_key: Option<String>,
    #[serde(default = "default_magic_import_enabled")]
    magic_import_enabled: bool,
    #[serde(default = "default_cut_at_timestamp_enabled")]
    cut_at_timestamp_enabled: bool,
    #[serde(default)]
    last_download_url: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            yt_dlp_path: None,
            default_output_dir: None,
            selected_preset_key: Some(DEFAULT_DOWNLOAD_PRESET_KEY.to_string()),
            magic_import_enabled: default_magic_import_enabled(),
            cut_at_timestamp_enabled: default_cut_at_timestamp_enabled(),
            last_download_url: None,
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
    #[serde(default = "default_cut_at_timestamp_enabled")]
    cut_at_timestamp_enabled: bool,
    #[serde(default)]
    cut_start_time: Option<f64>,
    #[serde(default)]
    filename_suffix: Option<String>,
    title: Option<String>,
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
    title: Option<String>,
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
struct NormalizedYoutubeUrl {
    url: String,
    key: String,
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
struct AddYoutubeLinkRequestBody {
    url: Option<String>,
    secret: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AddYoutubeLinksRequestBody {
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

model = WhisperModel(model_name, compute_type="int8")
segments, _ = model.transcribe(audio_path, beam_size=5)
lines = []
for segment in segments:
    text = segment.text.strip()
    if text:
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
const DEFAULT_DOWNLOAD_PRESET_KEY: &str = "best";

#[derive(Debug, Clone, Copy)]
struct DownloadPreset {
    key: &'static str,
    format: &'static str,
    extract_audio: bool,
    audio_format: Option<&'static str>,
    transcribe_text: bool,
    filename_suffix: Option<&'static str>,
}

const DOWNLOAD_PRESETS: &[DownloadPreset] = &[
    DownloadPreset {
        key: "best",
        format: "bestvideo+bestaudio/best",
        extract_audio: false,
        audio_format: None,
        transcribe_text: false,
        filename_suffix: Some("_best"),
    },
    DownloadPreset {
        key: "1080",
        format: "bv*[height<=1080]+ba/b[height<=1080]",
        extract_audio: false,
        audio_format: None,
        transcribe_text: false,
        filename_suffix: Some("__max"),
    },
    DownloadPreset {
        key: "audio_mp3",
        format: "ba/b",
        extract_audio: true,
        audio_format: Some("mp3"),
        transcribe_text: false,
        filename_suffix: None,
    },
    DownloadPreset {
        key: "audio_opus",
        format: "ba/b",
        extract_audio: true,
        audio_format: Some("opus"),
        transcribe_text: false,
        filename_suffix: None,
    },
    DownloadPreset {
        key: "text",
        format: "ba/b",
        extract_audio: true,
        audio_format: Some("mp3"),
        transcribe_text: true,
        filename_suffix: None,
    },
];

struct AppState {
    config: Mutex<AppConfig>,
    db: Mutex<Connection>,
    link_dump_server: Mutex<LinkDumpServerRuntime>,
    queue: Mutex<VecDeque<DownloadJob>>,
    queue_auto_start: Mutex<bool>,
    worker_running: Mutex<bool>,
    current_job_id: Mutex<Option<String>>,
    current_child: Mutex<Option<Arc<Mutex<Child>>>>,
    cancel_requested: Mutex<Option<String>>,
}

impl AppState {
    fn new(config: AppConfig, db: Connection) -> Self {
        Self {
            config: Mutex::new(config),
            db: Mutex::new(db),
            link_dump_server: Mutex::new(LinkDumpServerRuntime::default()),
            queue: Mutex::new(VecDeque::new()),
            queue_auto_start: Mutex::new(true),
            worker_running: Mutex::new(false),
            current_job_id: Mutex::new(None),
            current_child: Mutex::new(None),
            cancel_requested: Mutex::new(None),
        }
    }
}

fn download_preset_for_key(preset_key: Option<&str>) -> &'static DownloadPreset {
    let key = preset_key
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .unwrap_or(DEFAULT_DOWNLOAD_PRESET_KEY);

    DOWNLOAD_PRESETS
        .iter()
        .find(|preset| preset.key == key)
        .unwrap_or(&DOWNLOAD_PRESETS[0])
}

fn normalize_download_preset_key(preset_key: Option<&str>) -> String {
    download_preset_for_key(preset_key).key.to_string()
}

fn normalize_app_config(mut config: AppConfig) -> AppConfig {
    config.selected_preset_key = Some(normalize_download_preset_key(
        config.selected_preset_key.as_deref(),
    ));
    config
}

#[tauri::command]
fn get_config(state: State<AppState>) -> Result<AppConfig, String> {
    let cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
    Ok(cfg.clone())
}

#[tauri::command]
fn set_config(state: State<AppState>, config: AppConfig) -> Result<(), String> {
    let config = normalize_app_config(config);
    save_config_to_db(state.inner(), &config)?;
    {
        let mut cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
        *cfg = config.clone();
    }
    Ok(())
}

#[tauri::command]
fn set_selected_preset_key(
    state: State<AppState>,
    preset_key: String,
) -> Result<AppConfig, String> {
    let selected_preset_key = normalize_download_preset_key(Some(&preset_key));
    let next_config = {
        let cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
        let mut cfg = cfg.clone();
        cfg.selected_preset_key = Some(selected_preset_key);
        cfg.clone()
    };

    save_config_to_db(state.inner(), &next_config)?;
    {
        let mut cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
        *cfg = next_config.clone();
    }
    Ok(next_config)
}

#[tauri::command]
fn cache_last_download_url(state: State<AppState>, url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let next_config = {
        let cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
        let mut cfg = cfg.clone();
        cfg.last_download_url = Some(trimmed.to_string());
        cfg.clone()
    };

    save_config_to_db(state.inner(), &next_config)?;
    {
        let mut cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
        *cfg = next_config;
    }
    Ok(())
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

    tauri::async_runtime::spawn_blocking(move || load_info_with_yt_dlp(yt_dlp, deno, url))
        .await
        .map_err(|e| format!("Info task failed: {e}"))?
}

fn load_info_with_yt_dlp(
    yt_dlp: String,
    deno: Option<String>,
    url: String,
) -> Result<InfoResponse, String> {
    let mut command = Command::new(&yt_dlp);
    command.args(["--dump-json", "--no-playlist", "--no-warnings"]);
    if let Some(deno) = deno {
        command.arg("--js-runtimes");
        command.arg(format!("deno:{deno}"));
    }

    let output = command
        .arg(&url)
        .output()
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
    let output = Command::new(&yt_dlp)
        .arg("--version")
        .output()
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
fn get_queue_status(state: State<AppState>) -> Result<QueueStatus, String> {
    snapshot_queue_status(state.inner())
}

#[tauri::command]
fn set_queue_auto_start(
    app: AppHandle,
    state: State<AppState>,
    enabled: bool,
) -> Result<QueueStatus, String> {
    {
        let mut auto_start = state
            .queue_auto_start
            .lock()
            .map_err(|_| "Queue auto-start lock poisoned")?;
        *auto_start = enabled;
    }

    if enabled {
        ensure_worker(&app, state.inner())?;
    } else {
        emit_queue_status(&app, state.inner());
    }

    snapshot_queue_status(state.inner())
}

#[tauri::command]
fn start_queue(app: AppHandle, state: State<AppState>) -> Result<QueueStatus, String> {
    ensure_worker(&app, state.inner())?;
    snapshot_queue_status(state.inner())
}

#[tauri::command]
fn enqueue_download(
    app: AppHandle,
    state: State<AppState>,
    request: DownloadRequest,
) -> Result<String, String> {
    enqueue_download_request(&app, state.inner(), request)
}

fn enqueue_download_request(
    app: &AppHandle,
    state: &AppState,
    request: DownloadRequest,
) -> Result<String, String> {
    let job = build_download_job(state, request)?;
    let id = job.id.clone();
    enqueue_download_jobs(app, state, vec![job])?;
    Ok(id)
}

fn build_download_job(state: &AppState, request: DownloadRequest) -> Result<DownloadJob, String> {
    if !is_valid_url(&request.url) {
        return Err("URL must start with http:// or https://".to_string());
    }

    let output_dir = resolve_output_dir(state, request.output_dir.clone())?;
    let cut_start_time = resolve_cut_start_time(
        request.cut_at_timestamp_enabled,
        request.cut_start_time,
        &request.url,
    );
    let id = Uuid::new_v4().to_string();
    Ok(DownloadJob {
        id: id.clone(),
        url: request.url,
        format: request.format,
        output_dir,
        extract_audio: request.extract_audio,
        audio_format: request.audio_format,
        transcribe_text: request.transcribe_text,
        title: request.title,
        thumbnail: request.thumbnail,
        upload_date: request.upload_date,
        timestamp: request.timestamp,
        duration_seconds: request.duration_seconds,
        cut_start_time,
        filename_suffix: normalize_filename_suffix(request.filename_suffix.as_deref()),
    })
}

fn enqueue_download_jobs(
    app: &AppHandle,
    state: &AppState,
    jobs: Vec<DownloadJob>,
) -> Result<Vec<String>, String> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }

    let ids = jobs.iter().map(|job| job.id.clone()).collect::<Vec<_>>();

    {
        let mut queue = state.queue.lock().map_err(|_| "Queue lock poisoned")?;
        queue.extend(jobs);
    }

    emit_queue(app, state)?;
    if is_queue_auto_start_enabled(state)? {
        ensure_worker(app, state)?;
    }
    Ok(ids)
}

#[tauri::command]
fn cancel_download(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    let removed = {
        let mut queue = state.queue.lock().map_err(|_| "Queue lock poisoned")?;
        let before = queue.len();
        queue.retain(|job| job.id != id);
        before != queue.len()
    };

    if removed {
        emit_queue(&app, &state)?;
        emit_state(
            &app,
            DownloadStateEvent {
                id,
                state: "cancelled".to_string(),
                exit_code: None,
                error: None,
                output_path: None,
            },
        );
        return Ok(());
    }

    let is_current = {
        let current = state
            .current_job_id
            .lock()
            .map_err(|_| "Current job lock poisoned")?;
        current.as_deref() == Some(&id)
    };

    if !is_current {
        return Err("Job not found in queue".to_string());
    }

    {
        let mut cancel = state
            .cancel_requested
            .lock()
            .map_err(|_| "Cancel lock poisoned")?;
        *cancel = Some(id.clone());
    }

    let child = {
        let child_guard = state
            .current_child
            .lock()
            .map_err(|_| "Child lock poisoned")?;
        child_guard.clone()
    };

    if let Some(child) = child {
        if let Ok(mut guard) = child.lock() {
            let _ = guard.kill();
        }
    }

    emit_state(
        &app,
        DownloadStateEvent {
            id,
            state: "cancelling".to_string(),
            exit_code: None,
            error: None,
            output_path: None,
        },
    );
    Ok(())
}

#[tauri::command]
fn get_history(
    state: State<AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<HistoryPage, String> {
    let limit = limit.unwrap_or(50).clamp(1, 100);
    let offset = offset.unwrap_or(0);
    list_history_page_from_db(state.inner(), limit, offset)
}

#[tauri::command]
fn get_history_stats(state: State<AppState>) -> Result<HistoryStats, String> {
    get_history_stats_from_db(state.inner())
}

fn detect_platform(url: &str) -> Option<String> {
    if let Ok(parsed) = url::Url::parse(url) {
        let host = parsed.host_str().unwrap_or("").to_lowercase();
        let host = host.strip_prefix("www.").unwrap_or(&host);

        if host == "youtu.be" || host.ends_with("youtube.com") {
            return Some("youtube".to_string());
        }
        if host.ends_with("facebook.com") || host == "fb.watch" {
            return Some("facebook".to_string());
        }
        if host.ends_with("twitch.tv") {
            return Some("twitch".to_string());
        }
        if host == "x.com" || host.ends_with(".x.com") || host.ends_with("twitter.com") {
            return Some("x".to_string());
        }
        if host.ends_with("tiktok.com") {
            return Some("tiktok".to_string());
        }
        if host.ends_with("instagram.com") || host.ends_with("instagr.am") {
            return Some("instagram".to_string());
        }
    }
    None
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

fn hydrate_history_metadata(
    app: &AppHandle,
    state: &AppState,
    job: &DownloadJob,
    filename: Option<&str>,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
) {
    let mut title = trim_optional_string(job.title.clone());
    let mut thumbnail = trim_optional_string(job.thumbnail.clone());
    let mut upload_date = trim_optional_string(job.upload_date.clone());
    let mut timestamp = job.timestamp;
    let mut duration_seconds = job.duration_seconds;

    if title.is_none()
        || thumbnail.is_none()
        || upload_date.is_none()
        || timestamp.is_none()
        || duration_seconds.is_none()
    {
        if let Ok(yt_dlp) = resolve_yt_dlp(app, state) {
            let deno = resolve_deno_executable(app);
            if let Ok(info) = load_info_with_yt_dlp(yt_dlp, deno, job.url.clone()) {
                if title.is_none() {
                    title = trim_optional_string(info.title);
                }
                if thumbnail.is_none() {
                    thumbnail = trim_optional_string(info.thumbnail);
                }
                if upload_date.is_none() {
                    upload_date = trim_optional_string(info.upload_date);
                }
                if timestamp.is_none() {
                    timestamp = info.timestamp;
                }
                if duration_seconds.is_none() {
                    duration_seconds = info.duration;
                }
            }
        }
    }

    if title.is_none() {
        title = title_from_filename(filename);
    }

    (title, thumbnail, upload_date, timestamp, duration_seconds)
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
) {
    let filename = filename_from_path(output_path);
    let (title, thumbnail, upload_date, timestamp, duration_seconds) =
        hydrate_history_metadata(app, state, job, filename.as_deref());
    let file_size_bytes = file_size_bytes_from_path(output_path);
    let now = current_timestamp_millis();
    let entry = HistoryEntry {
        id: Uuid::new_v4().to_string(),
        url: job.url.clone(),
        title,
        filename,
        thumbnail,
        upload_date,
        timestamp,
        duration_seconds,
        file_size_bytes,
        platform: detect_platform(&job.url),
        output_path: output_path.map(|s| s.to_string()),
        created_at: now,
        completed_at: Some(now),
    };

    let _ = insert_history_entry_in_db(state, &entry);
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

fn snapshot_queue_status(state: &AppState) -> Result<QueueStatus, String> {
    let auto_start = *state
        .queue_auto_start
        .lock()
        .map_err(|_| "Queue auto-start lock poisoned")?;
    let worker_running = *state
        .worker_running
        .lock()
        .map_err(|_| "Worker lock poisoned")?;

    Ok(QueueStatus {
        auto_start,
        worker_running,
    })
}

fn emit_queue_status(app: &AppHandle, state: &AppState) {
    if let Ok(status) = snapshot_queue_status(state) {
        let _ = app.emit_all("queue:status", status);
    }
}

fn is_queue_auto_start_enabled(state: &AppState) -> Result<bool, String> {
    let auto_start = state
        .queue_auto_start
        .lock()
        .map_err(|_| "Queue auto-start lock poisoned")?;
    Ok(*auto_start)
}

fn ensure_worker(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let mut running = state
        .worker_running
        .lock()
        .map_err(|_| "Worker lock poisoned")?;
    if *running {
        return Ok(());
    }
    *running = true;
    drop(running);
    emit_queue_status(app, state);

    let app_handle = app.clone();

    thread::spawn(move || {
        loop {
            let state_handle = app_handle.state::<AppState>();
            let job_opt = {
                let mut queue = match state_handle.queue.lock() {
                    Ok(queue) => queue,
                    Err(_) => break,
                };
                queue.pop_front()
            };

            let job = match job_opt {
                Some(job) => job,
                None => {
                    if let Ok(mut running) = state_handle.worker_running.lock() {
                        *running = false;
                    }
                    let _ = emit_queue(&app_handle, &state_handle);
                    emit_queue_status(&app_handle, &state_handle);
                    break;
                }
            };

            if let Ok(mut current) = state_handle.current_job_id.lock() {
                *current = Some(job.id.clone());
            }

            emit_state(
                &app_handle,
                DownloadStateEvent {
                    id: job.id.clone(),
                    state: "downloading".to_string(),
                    exit_code: None,
                    error: None,
                    output_path: None,
                },
            );

            let result = run_download_job(&app_handle, &state_handle, &job);

            if let Ok(mut current) = state_handle.current_job_id.lock() {
                *current = None;
            }

            match result {
                Ok(run_result) => {
                    let cancelled = if let Ok(mut cancel) = state_handle.cancel_requested.lock() {
                        if cancel.as_deref() == Some(job.id.as_str()) {
                            *cancel = None;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if cancelled {
                        emit_state(
                            &app_handle,
                            DownloadStateEvent {
                                id: job.id.clone(),
                                state: "cancelled".to_string(),
                                exit_code: Some(run_result.exit_code),
                                error: None,
                                output_path: None,
                            },
                        );
                    } else if run_result.exit_code != 0 {
                        emit_state(
                            &app_handle,
                            DownloadStateEvent {
                                id: job.id.clone(),
                                state: "error".to_string(),
                                exit_code: Some(run_result.exit_code),
                                error: Some("yt-dlp exited with error".to_string()),
                                output_path: None,
                            },
                        );
                    } else if job.transcribe_text {
                        emit_state(
                            &app_handle,
                            DownloadStateEvent {
                                id: job.id.clone(),
                                state: "transcribing".to_string(),
                                exit_code: Some(run_result.exit_code),
                                error: None,
                                output_path: None,
                            },
                        );

                        match run_faster_whisper_transcription(
                            &app_handle,
                            &job,
                            run_result.output_path.as_deref(),
                        ) {
                            Ok(transcript_path) => {
                                emit_log(
                                    &app_handle,
                                    LogEvent {
                                        id: job.id.clone(),
                                        line: format!("[transcript] saved: {transcript_path}"),
                                        is_error: false,
                                    },
                                );
                                emit_state(
                                    &app_handle,
                                    DownloadStateEvent {
                                        id: job.id.clone(),
                                        state: "success".to_string(),
                                        exit_code: Some(run_result.exit_code),
                                        error: None,
                                        output_path: Some(transcript_path.clone()),
                                    },
                                );
                                // Add to history on success
                                add_history_entry_on_success(
                                    &app_handle,
                                    &state_handle,
                                    &job,
                                    run_result.output_path.as_deref(),
                                );
                            }
                            Err(err) => {
                                emit_state(
                                    &app_handle,
                                    DownloadStateEvent {
                                        id: job.id.clone(),
                                        state: "error".to_string(),
                                        exit_code: Some(run_result.exit_code),
                                        error: Some(err),
                                        output_path: None,
                                    },
                                );
                            }
                        }
                    } else {
                        emit_state(
                            &app_handle,
                            DownloadStateEvent {
                                id: job.id.clone(),
                                state: "success".to_string(),
                                exit_code: Some(run_result.exit_code),
                                error: None,
                                output_path: run_result.output_path.clone(),
                            },
                        );
                        // Add to history on success
                        add_history_entry_on_success(
                            &app_handle,
                            &state_handle,
                            &job,
                            run_result.output_path.as_deref(),
                        );
                    }
                }
                Err(err) => {
                    emit_state(
                        &app_handle,
                        DownloadStateEvent {
                            id: job.id.clone(),
                            state: "error".to_string(),
                            exit_code: None,
                            error: Some(err),
                            output_path: None,
                        },
                    );
                }
            }

            let _ = emit_queue(&app_handle, &state_handle);
        }
    });

    Ok(())
}

fn run_download_job(
    app: &AppHandle,
    state: &AppState,
    job: &DownloadJob,
) -> Result<DownloadRunResult, String> {
    let yt_dlp = resolve_yt_dlp(app, state)?;
    let ffmpeg_location = resolve_ffmpeg_location(app, &yt_dlp);
    let deno_path = resolve_deno_executable(app);
    let output_template = build_output_template(&job.output_dir, job.filename_suffix.as_deref());
    let output_template_for_fallback = output_template.clone();

    let mut args = vec![
        "--no-playlist".to_string(),
        "--newline".to_string(),
        "--progress".to_string(),
        "--no-color".to_string(),
        "--print".to_string(),
        "after_move:filepath".to_string(),
        "--print".to_string(),
        "after_video:filepath".to_string(),
        "-f".to_string(),
        job.format.clone(),
        "-o".to_string(),
        output_template,
    ];

    let needs_ffmpeg = job.extract_audio
        || job.transcribe_text
        || job.format.contains('+')
        || job.cut_start_time.is_some();
    if let Some(location) = ffmpeg_location.as_ref() {
        args.push("--ffmpeg-location".to_string());
        args.push(location.clone());
    } else if needs_ffmpeg {
        return Err(
            "ffmpeg and ffprobe not found. Install ffmpeg (or make sure it is in the same directory as yt-dlp) and try again."
                .to_string(),
        );
    }

    if let Some(deno) = deno_path.as_ref() {
        args.push("--js-runtimes".to_string());
        args.push(format!("deno:{deno}"));
    }

    if job.extract_audio {
        args.push("--extract-audio".to_string());
        if let Some(fmt) = job.audio_format.as_ref() {
            args.push("--audio-format".to_string());
            args.push(fmt.to_string());
        }
    }

    if let Some(cut_start_time) = job.cut_start_time {
        let cut_timestamp = format_yt_dlp_timestamp(cut_start_time);
        emit_log(
            app,
            LogEvent {
                id: job.id.clone(),
                line: format!("[cut] URL timestamp detected; downloading full file before local cut at {cut_timestamp}s"),
                is_error: false,
            },
        );
    }

    args.push(job.url.clone());

    let mut command = Command::new(&yt_dlp);
    command.args(args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let child = command.spawn().map_err(|e| format!("Spawn failed: {e}"))?;
    let child = Arc::new(Mutex::new(child));

    {
        let mut child_guard = state
            .current_child
            .lock()
            .map_err(|_| "Child lock poisoned")?;
        *child_guard = Some(child.clone());
    }

    let (stdout, stderr) = {
        let mut guard = child.lock().map_err(|_| "Child lock poisoned")?;
        (guard.stdout.take(), guard.stderr.take())
    };

    let progress_re = Regex::new(r"\[download\]\s+([\d\.]+)%.*?at\s+([^\s]+).*?ETA\s+([^\s]+)")
        .map_err(|e| format!("Regex error: {e}"))?;
    let output_path_capture: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let app_stdout = app.clone();
    let id_stdout = job.id.clone();
    let output_path_for_stdout = output_path_capture.clone();
    let handle_out = thread::spawn(move || {
        if let Some(out) = stdout {
            let reader = BufReader::new(out);
            for line in reader.lines().flatten() {
                emit_log(
                    &app_stdout,
                    LogEvent {
                        id: id_stdout.clone(),
                        line: line.clone(),
                        is_error: false,
                    },
                );

                if let Some(caps) = progress_re.captures(&line) {
                    let percent = caps.get(1).and_then(|m| m.as_str().parse::<f32>().ok());
                    let speed = caps.get(2).map(|m| m.as_str().to_string());
                    let eta = caps.get(3).map(|m| m.as_str().to_string());
                    emit_progress(
                        &app_stdout,
                        DownloadProgress {
                            id: id_stdout.clone(),
                            percent,
                            speed,
                            eta,
                        },
                    );
                }

                if let Some(path_line) = parse_yt_dlp_filepath(&line) {
                    if let Ok(mut slot) = output_path_for_stdout.lock() {
                        slot.push(path_line);
                    }
                }
            }
        }
    });

    let app_stderr = app.clone();
    let id_stderr = job.id.clone();
    let handle_err = thread::spawn(move || {
        if let Some(err) = stderr {
            let reader = BufReader::new(err);
            for line in reader.lines().flatten() {
                emit_log(
                    &app_stderr,
                    LogEvent {
                        id: id_stderr.clone(),
                        line,
                        is_error: true,
                    },
                );
            }
        }
    });

    let status = loop {
        let maybe_status = {
            let mut guard = child.lock().map_err(|_| "Child lock poisoned")?;
            guard.try_wait().map_err(|e| format!("Wait failed: {e}"))?
        };

        if let Some(status) = maybe_status {
            break status;
        }

        thread::sleep(Duration::from_millis(100));
    };
    {
        let mut child_guard = state
            .current_child
            .lock()
            .map_err(|_| "Child lock poisoned")?;
        *child_guard = None;
    }
    let _ = handle_out.join();
    let _ = handle_err.join();

    let mut output_path = output_path_capture
        .lock()
        .ok()
        .and_then(|guard| select_existing_output_path(&guard));

    if status.success() {
        if output_path.is_none() {
            output_path = resolve_existing_output_path_fallback(
                job,
                &yt_dlp,
                deno_path.as_deref(),
                &output_template_for_fallback,
            );
        }

        if let Some(cut_start_time) = job.cut_start_time {
            let trimmed_path = trim_downloaded_file(
                app,
                job,
                output_path.as_deref(),
                ffmpeg_location.as_deref(),
                cut_start_time,
            )?;
            output_path = Some(trimmed_path);
        }
    }

    Ok(DownloadRunResult {
        exit_code: status.code().unwrap_or(-1),
        output_path,
    })
}

fn trim_downloaded_file(
    app: &AppHandle,
    job: &DownloadJob,
    output_path: Option<&str>,
    ffmpeg_location: Option<&str>,
    cut_start_time: f64,
) -> Result<String, String> {
    let input_path = output_path
        .ok_or_else(|| "Could not determine downloaded file path for timestamp cut".to_string())?;
    let input_path = Path::new(input_path);
    if !input_path.exists() {
        return Err(format!(
            "Downloaded file not found for timestamp cut: {}",
            input_path.to_string_lossy()
        ));
    }

    let ffmpeg_location =
        ffmpeg_location.ok_or_else(|| "ffmpeg not available for timestamp cut".to_string())?;
    let ffmpeg_path = Path::new(ffmpeg_location).join(ffmpeg_tool_name());
    if !ffmpeg_path.exists() {
        return Err(format!(
            "ffmpeg executable not found for timestamp cut: {}",
            ffmpeg_path.to_string_lossy()
        ));
    }

    let cut_timestamp = format_yt_dlp_timestamp(cut_start_time);
    emit_progress(
        app,
        DownloadProgress {
            id: job.id.clone(),
            percent: Some(100.0),
            speed: Some("cutting".to_string()),
            eta: Some("-".to_string()),
        },
    );
    emit_log(
        app,
        LogEvent {
            id: job.id.clone(),
            line: format!("[cut] trimming local file from {cut_timestamp}s"),
            is_error: false,
        },
    );

    let final_path = build_timestamp_cut_output_path(input_path, cut_start_time)?;
    let temp_path = build_cut_sidecar_path(input_path, "cut")?;
    let backup_path = build_cut_sidecar_path(input_path, "original")?;

    let input_path_str = input_path.to_string_lossy().to_string();
    let temp_path_str = temp_path.to_string_lossy().to_string();

    let output = Command::new(&ffmpeg_path)
        .args([
            "-hide_banner",
            "-y",
            "-ss",
            cut_timestamp.as_str(),
            "-i",
            input_path_str.as_str(),
            "-map",
            "0",
            "-c",
            "copy",
            "-avoid_negative_ts",
            "make_zero",
            temp_path_str.as_str(),
        ])
        .output()
        .map_err(|e| format!("Failed to run ffmpeg timestamp cut: {e}"))?;

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        emit_log(
            app,
            LogEvent {
                id: job.id.clone(),
                line: format!("[ffmpeg] {line}"),
                is_error: false,
            },
        );
    }
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        emit_log(
            app,
            LogEvent {
                id: job.id.clone(),
                line: format!("[ffmpeg] {line}"),
                is_error: !output.status.success(),
            },
        );
    }

    if !output.status.success() {
        let _ = fs::remove_file(&temp_path);
        let code = output.status.code().unwrap_or(-1);
        return Err(format!("ffmpeg timestamp cut failed with exit code {code}"));
    }

    if !temp_path.exists() {
        return Err("ffmpeg finished but no cut file was created".to_string());
    }

    fs::rename(input_path, &backup_path)
        .map_err(|e| format!("Could not back up full file before cut replace: {e}"))?;
    if final_path.exists() {
        let _ = fs::remove_file(&final_path);
    }
    if let Err(err) = fs::rename(&temp_path, &final_path) {
        let _ = fs::rename(&backup_path, input_path);
        return Err(format!("Could not move cut file into final path: {err}"));
    }
    let _ = fs::remove_file(&backup_path);

    emit_log(
        app,
        LogEvent {
            id: job.id.clone(),
            line: format!("[cut] saved: {}", final_path.to_string_lossy()),
            is_error: false,
        },
    );

    Ok(final_path.to_string_lossy().to_string())
}

fn build_cut_sidecar_path(input_path: &Path, label: &str) -> Result<PathBuf, String> {
    let parent = input_path
        .parent()
        .ok_or_else(|| "Downloaded file has no parent directory".to_string())?;
    let extension = input_path
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("tmp");
    Ok(parent.join(format!(
        ".pinefetch-{label}-{}.{}",
        Uuid::new_v4(),
        extension
    )))
}

fn build_timestamp_cut_output_path(
    input_path: &Path,
    cut_start_time: f64,
) -> Result<PathBuf, String> {
    let parent = input_path
        .parent()
        .ok_or_else(|| "Downloaded file has no parent directory".to_string())?;
    let stem = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Downloaded file has no usable file name".to_string())?;
    let cut_suffix = format_timestamp_filename_suffix(cut_start_time);

    let mut filename = format!("{stem}{cut_suffix}");
    if let Some(extension) = input_path.extension().and_then(|value| value.to_str()) {
        if !extension.is_empty() {
            filename.push('.');
            filename.push_str(extension);
        }
    }

    Ok(parent.join(filename))
}

fn format_timestamp_filename_suffix(seconds: f64) -> String {
    let mut timestamp = format_yt_dlp_timestamp(seconds);
    timestamp = timestamp
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();
    format!("_t{timestamp}")
}

fn select_existing_output_path(candidates: &[String]) -> Option<String> {
    let existing = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            let path = Path::new(candidate.as_str());
            let metadata = path.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            Some((index, candidate, metadata.len(), is_format_part_path(path)))
        })
        .collect::<Vec<_>>();

    if let Some((_, candidate, _, _)) = existing.iter().rev().find(|(_, _, _, is_part)| !*is_part) {
        return Some((*candidate).clone());
    }

    existing
        .into_iter()
        .max_by(|left, right| left.2.cmp(&right.2).then_with(|| left.0.cmp(&right.0)))
        .map(|(_, candidate, _, _)| candidate.clone())
}

fn is_format_part_path(path: &Path) -> bool {
    path.file_stem()
        .and_then(|value| value.to_str())
        .and_then(|stem| stem.rsplit_once(".f"))
        .map(|(_, suffix)| !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()))
        .unwrap_or(false)
}

fn parse_yt_dlp_filepath(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    for prefix in [
        "[download] Destination:",
        "[ExtractAudio] Destination:",
        "[Metadata] Writing metadata to:",
    ] {
        if let Some(candidate) = trimmed.strip_prefix(prefix) {
            return normalize_filepath_candidate(candidate);
        }
    }

    if let Some(candidate) = trimmed.strip_prefix("[Merger] Merging formats into ") {
        return normalize_filepath_candidate(candidate);
    }

    if let Some(candidate) = trimmed
        .strip_prefix("[download] ")
        .and_then(|value| value.strip_suffix(" has already been downloaded"))
    {
        return normalize_filepath_candidate(candidate);
    }

    if trimmed.starts_with('[') {
        return None;
    }

    normalize_filepath_candidate(trimmed)
}

fn normalize_filepath_candidate(raw: &str) -> Option<String> {
    let mut candidate = raw.trim();
    if candidate.len() >= 2 {
        let bytes = candidate.as_bytes();
        if (bytes[0] == b'"' && bytes[candidate.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[candidate.len() - 1] == b'\'')
        {
            candidate = &candidate[1..candidate.len() - 1];
        }
    }
    let candidate = candidate.trim();
    if matches!(candidate, "" | "NA" | "N/A" | "None" | "null") {
        return None;
    }
    if candidate.starts_with("http://") || candidate.starts_with("https://") {
        return None;
    }
    Some(candidate.to_string())
}

fn resolve_existing_output_path_fallback(
    job: &DownloadJob,
    yt_dlp: &str,
    deno_path: Option<&str>,
    output_template: &str,
) -> Option<String> {
    let expected_path =
        probe_expected_output_filename(job, yt_dlp, deno_path, output_template).ok()??;
    let candidates = existing_output_candidates_from_expected(&expected_path, job);
    select_existing_output_path(&candidates)
}

fn probe_expected_output_filename(
    job: &DownloadJob,
    yt_dlp: &str,
    deno_path: Option<&str>,
    output_template: &str,
) -> Result<Option<String>, String> {
    let mut command = Command::new(yt_dlp);
    command.args([
        "--simulate",
        "--no-playlist",
        "--no-warnings",
        "--print",
        "filename",
        "-f",
    ]);
    command.arg(&job.format);
    command.arg("-o");
    command.arg(output_template);

    if let Some(deno) = deno_path {
        command.arg("--js-runtimes");
        command.arg(format!("deno:{deno}"));
    }

    if job.extract_audio {
        command.arg("--extract-audio");
        if let Some(fmt) = job.audio_format.as_ref() {
            command.arg("--audio-format");
            command.arg(fmt);
        }
    }

    command.arg(&job.url);

    let output = command
        .output()
        .map_err(|e| format!("Failed to probe output filename with yt-dlp: {e}"))?;

    if !output.status.success() {
        return Ok(None);
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find_map(parse_yt_dlp_filepath))
}

fn existing_output_candidates_from_expected(expected_path: &str, job: &DownloadJob) -> Vec<String> {
    let mut candidates = vec![expected_path.to_string()];
    let expected = Path::new(expected_path);

    if job.extract_audio {
        if let Some(audio_format) = job.audio_format.as_deref() {
            candidates.push(
                expected
                    .with_extension(audio_format)
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }

    if let Some(cut_start_time) = job.cut_start_time {
        for candidate in candidates.clone() {
            if let Ok(cut_path) =
                build_timestamp_cut_output_path(Path::new(&candidate), cut_start_time)
            {
                candidates.push(cut_path.to_string_lossy().to_string());
            }
        }
    }

    candidates.extend(related_existing_output_paths(expected));
    candidates.sort();
    candidates.dedup();
    candidates
}

fn related_existing_output_paths(expected: &Path) -> Vec<String> {
    let Some(parent) = expected.parent() else {
        return Vec::new();
    };
    let Some(expected_stem) = expected.file_stem().and_then(|value| value.to_str()) else {
        return Vec::new();
    };
    let format_part_prefix = format!("{expected_stem}.f");

    fs::read_dir(parent)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let stem = path.file_stem().and_then(|value| value.to_str())?;
            if stem == expected_stem || stem.starts_with(&format_part_prefix) {
                Some(path.to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect()
}

fn ffmpeg_tool_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

fn ffprobe_tool_name() -> &'static str {
    if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    }
}

fn ffmpeg_tool_is_usable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    Command::new(path)
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn has_usable_ffmpeg_tools_in_dir(dir: &Path) -> bool {
    ffmpeg_tool_is_usable(&dir.join(ffmpeg_tool_name()))
        && ffmpeg_tool_is_usable(&dir.join(ffprobe_tool_name()))
}

fn normalize_ffmpeg_location(path: &Path) -> Option<String> {
    if path.is_dir() {
        if has_usable_ffmpeg_tools_in_dir(path) {
            return Some(path.to_string_lossy().to_string());
        }
        return None;
    }

    if path.is_file() {
        if let Some(parent) = path.parent() {
            if has_usable_ffmpeg_tools_in_dir(parent) {
                return Some(parent.to_string_lossy().to_string());
            }
        }
    }

    None
}

fn resolve_bundled_ffmpeg_location(app: &AppHandle) -> Option<String> {
    for relative in [
        "ffmpeg-runtime/bin",
        "ffmpeg-runtime",
        "resources/ffmpeg-runtime/bin",
        "resources/ffmpeg-runtime",
    ] {
        if let Some(path) = app.path_resolver().resolve_resource(relative) {
            if let Some(location) = normalize_ffmpeg_location(&path) {
                return Some(location);
            }
        }
    }
    None
}

fn resolve_ffmpeg_location(app: &AppHandle, yt_dlp_path: &str) -> Option<String> {
    if let Ok(raw) = std::env::var("PINEFETCH_FFMPEG_LOCATION") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            if let Some(location) = normalize_ffmpeg_location(Path::new(trimmed)) {
                return Some(location);
            }
        }
    }

    if let Some(location) = resolve_bundled_ffmpeg_location(app) {
        return Some(location);
    }

    if let Some(location) = normalize_ffmpeg_location(Path::new(yt_dlp_path)) {
        return Some(location);
    }

    for candidate in ["/opt/homebrew/bin", "/usr/local/bin"] {
        if let Some(location) = normalize_ffmpeg_location(Path::new(candidate)) {
            return Some(location);
        }
    }

    if let Some(ffmpeg_path) = find_in_path("ffmpeg") {
        if let Some(location) = normalize_ffmpeg_location(Path::new(&ffmpeg_path)) {
            return Some(location);
        }
    }

    if let Some(ffprobe_path) = find_in_path("ffprobe") {
        if let Some(location) = normalize_ffmpeg_location(Path::new(&ffprobe_path)) {
            return Some(location);
        }
    }

    None
}

fn resolve_bundled_python(app: &AppHandle) -> Option<String> {
    #[cfg(target_os = "windows")]
    let candidates = vec![
        "whisper-runtime/Scripts/python.exe",
        "resources/whisper-runtime/Scripts/python.exe",
    ];

    #[cfg(not(target_os = "windows"))]
    let candidates = vec![
        "whisper-runtime/bin/python3.12",
        "whisper-runtime/bin/python3.11",
        "whisper-runtime/bin/python3.10",
        "whisper-runtime/bin/python3",
        "whisper-runtime/bin/python",
        "resources/whisper-runtime/bin/python3.12",
        "resources/whisper-runtime/bin/python3.11",
        "resources/whisper-runtime/bin/python3.10",
        "resources/whisper-runtime/bin/python3",
        "resources/whisper-runtime/bin/python",
    ];

    for relative in candidates {
        if let Some(path) = app.path_resolver().resolve_resource(relative) {
            if path.exists() {
                return Some(path.to_string_lossy().to_string());
            }
        }
    }

    None
}

fn resolve_bundled_deno(app: &AppHandle) -> Option<String> {
    #[cfg(target_os = "windows")]
    let candidates = vec![
        "deno-runtime/bin/deno.exe",
        "resources/deno-runtime/bin/deno.exe",
    ];

    #[cfg(not(target_os = "windows"))]
    let candidates = vec!["deno-runtime/bin/deno", "resources/deno-runtime/bin/deno"];

    for relative in candidates {
        if let Some(path) = app.path_resolver().resolve_resource(relative) {
            if path.exists() {
                return Some(path.to_string_lossy().to_string());
            }
        }
    }

    None
}

fn resolve_deno_executable(app: &AppHandle) -> Option<String> {
    if let Ok(raw) = std::env::var("PINEFETCH_DENO_PATH") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() && Path::new(trimmed).exists() {
            return Some(trimmed.to_string());
        }
    }

    if let Some(path) = resolve_bundled_deno(app) {
        return Some(path);
    }

    find_in_path("deno")
}

fn resolve_python_executable(app: &AppHandle) -> Option<String> {
    if let Ok(raw) = std::env::var("PINEFETCH_FASTER_WHISPER_PYTHON") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() && Path::new(trimmed).exists() {
            return Some(trimmed.to_string());
        }
    }

    if let Some(path) = resolve_bundled_python(app) {
        return Some(path);
    }

    for candidate in [
        "python3.12",
        "python3.11",
        "python3.10",
        "python3",
        "python",
    ] {
        if let Some(path) = find_in_path(candidate) {
            return Some(path);
        }
    }

    None
}

fn run_faster_whisper_transcription(
    app: &AppHandle,
    job: &DownloadJob,
    output_path: Option<&str>,
) -> Result<String, String> {
    let audio_path = output_path
        .ok_or_else(|| "Could not determine downloaded file path for transcription".to_string())?;
    if !Path::new(audio_path).exists() {
        return Err(format!(
            "Downloaded file not found for transcription: {audio_path}"
        ));
    }

    let python = resolve_python_executable(app).ok_or_else(|| {
    "No Python runtime found for faster-whisper (bundled runtime missing and no compatible Python in PATH)"
      .to_string()
  })?;
    emit_log(
        app,
        LogEvent {
            id: job.id.clone(),
            line: format!("[faster-whisper] using python: {python}"),
            is_error: false,
        },
    );

    let transcript_path = Path::new(audio_path).with_extension("txt");
    let transcript_path_str = transcript_path.to_string_lossy().to_string();
    let model_name = std::env::var("PINEFETCH_FASTER_WHISPER_MODEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "base".to_string());

    let mut command = Command::new(python);
    command
        .arg("-c")
        .arg(FASTER_WHISPER_TRANSCRIBE_SNIPPET)
        .arg(audio_path)
        .arg(&transcript_path_str)
        .arg(&model_name)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to start faster-whisper transcription: {e}"))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let app_stdout = app.clone();
    let job_id_stdout = job.id.clone();
    let handle_out = thread::spawn(move || {
        if let Some(out) = stdout {
            let reader = BufReader::new(out);
            for line in reader.lines().flatten() {
                emit_log(
                    &app_stdout,
                    LogEvent {
                        id: job_id_stdout.clone(),
                        line: format!("[faster-whisper] {line}"),
                        is_error: false,
                    },
                );
            }
        }
    });

    let app_stderr = app.clone();
    let job_id_stderr = job.id.clone();
    let handle_err = thread::spawn(move || {
        if let Some(err) = stderr {
            let reader = BufReader::new(err);
            for line in reader.lines().flatten() {
                emit_log(
                    &app_stderr,
                    LogEvent {
                        id: job_id_stderr.clone(),
                        line: format!("[faster-whisper] {line}"),
                        is_error: true,
                    },
                );
            }
        }
    });

    let status = child
        .wait()
        .map_err(|e| format!("Failed while waiting for faster-whisper: {e}"))?;
    let _ = handle_out.join();
    let _ = handle_err.join();

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        return Err(format!(
      "faster-whisper failed (exit code {code}). Ensure Python deps are installed (`pip install faster-whisper`)."
    ));
    }

    if !transcript_path.exists() {
        return Err("faster-whisper finished but no transcript file was created".to_string());
    }

    Ok(transcript_path_str)
}

fn build_output_template(output_dir: &str, filename_suffix: Option<&str>) -> String {
    let mut path = PathBuf::from(output_dir);
    // Use title, but fallback to uploader and id for platforms where title might be missing or duplicate
    // %(title)s - video title
    // %(uploader)s - uploader name
    // %(id)s - unique video ID (ensures uniqueness for Instagram posts from same creator)
    let suffix = filename_suffix.unwrap_or("");
    path.push(format!("%(title)s - %(uploader)s - %(id)s{suffix}.%(ext)s"));
    path.to_string_lossy().to_string()
}

fn normalize_filename_suffix(raw: Option<&str>) -> Option<String> {
    let suffix = raw?.trim();
    if suffix.is_empty()
        || suffix.len() > 32
        || !suffix.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
    {
        return None;
    }

    Some(suffix.to_string())
}

fn emit_queue(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let queue = state.queue.lock().map_err(|_| "Queue lock poisoned")?;
    app.emit_all("queue:update", queue.clone())
        .map_err(|e| format!("Emit queue failed: {e}"))
}

fn emit_progress(app: &AppHandle, progress: DownloadProgress) {
    let _ = app.emit_all("download:progress", progress);
}

fn emit_state(app: &AppHandle, state: DownloadStateEvent) {
    let _ = app.emit_all("download:state", state);
}

fn emit_log(app: &AppHandle, log: LogEvent) {
    let _ = app.emit_all("download:log", log);
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

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir =
        tauri::api::path::app_config_dir(&app.config()).ok_or("Config directory unavailable")?;
    fs::create_dir_all(&dir).map_err(|e| format!("Config dir create failed: {e}"))?;
    Ok(dir.join("config.json"))
}

fn legacy_history_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = tauri::api::path::app_data_dir(&app.config()).ok_or("Data directory unavailable")?;
    fs::create_dir_all(&dir).map_err(|e| format!("Data dir create failed: {e}"))?;
    Ok(dir.join("history.json"))
}

fn load_legacy_history_json(app: &AppHandle) -> Vec<HistoryEntry> {
    if let Ok(path) = legacy_history_path(app) {
        if let Ok(raw) = fs::read_to_string(path) {
            if let Ok(history) = serde_json::from_str::<Vec<HistoryEntry>>(&raw) {
                return history.into_iter().map(normalize_history_entry).collect();
            }
        }
    }
    Vec::new()
}

fn normalize_history_entry(mut entry: HistoryEntry) -> HistoryEntry {
    entry.title = trim_optional_string(entry.title);
    entry.filename = trim_optional_string(entry.filename)
        .or_else(|| filename_from_path(entry.output_path.as_deref()));
    entry.thumbnail = trim_optional_string(entry.thumbnail);
    entry.upload_date = trim_optional_string(entry.upload_date);
    entry.timestamp = entry.timestamp.filter(|timestamp| *timestamp >= 0);
    entry.duration_seconds = entry.duration_seconds.filter(|duration| *duration >= 0);
    entry.file_size_bytes = entry.file_size_bytes.filter(|size| *size >= 0);
    entry.platform = trim_optional_string(entry.platform).or_else(|| detect_platform(&entry.url));
    entry.output_path = trim_optional_string(entry.output_path);
    if entry.title.is_none() {
        entry.title = title_from_filename(entry.filename.as_deref());
    }
    entry
}

fn millis_to_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn i64_to_millis(value: i64) -> u64 {
    if value < 0 {
        0
    } else {
        value as u64
    }
}

fn optional_i64_to_millis(value: Option<i64>) -> Option<u64> {
    value.map(i64_to_millis)
}

fn count_history_entries_in_db(state: &AppState) -> Result<u64, String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM history_entries", [], |row| row.get(0))
        .map_err(|e| format!("History read failed: {e}"))?;
    Ok(count.max(0) as u64)
}

fn list_history_page_from_db(
    state: &AppState,
    limit: u32,
    offset: u32,
) -> Result<HistoryPage, String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM history_entries", [], |row| row.get(0))
        .map_err(|e| format!("History read failed: {e}"))?;
    let mut stmt = conn
        .prepare(
            "SELECT id, url, title, filename, thumbnail, upload_date, timestamp, duration_seconds, file_size_bytes, platform, output_path, created_at, completed_at
             FROM history_entries
             ORDER BY COALESCE(completed_at, created_at) DESC, created_at DESC, id DESC
             LIMIT ?1 OFFSET ?2",
        )
        .map_err(|e| format!("History read failed: {e}"))?;

    let rows = stmt
        .query_map(params![i64::from(limit), i64::from(offset)], |row| {
            let created_at: i64 = row.get(11)?;
            let completed_at: Option<i64> = row.get(12)?;
            Ok(HistoryEntry {
                id: row.get(0)?,
                url: row.get(1)?,
                title: row.get(2)?,
                filename: row.get(3)?,
                thumbnail: row.get(4)?,
                upload_date: row.get(5)?,
                timestamp: row.get(6)?,
                duration_seconds: row.get(7)?,
                file_size_bytes: row.get(8)?,
                platform: row.get(9)?,
                output_path: row.get(10)?,
                created_at: i64_to_millis(created_at),
                completed_at: optional_i64_to_millis(completed_at),
            })
        })
        .map_err(|e| format!("History read failed: {e}"))?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(normalize_history_entry(
            row.map_err(|e| format!("History read failed: {e}"))?,
        ));
    }

    let loaded_count = u64::from(offset).saturating_add(entries.len() as u64);
    Ok(HistoryPage {
        entries,
        has_more: loaded_count < total.max(0) as u64,
    })
}

fn list_history_entries_from_db(state: &AppState) -> Result<Vec<HistoryEntry>, String> {
    Ok(list_history_page_from_db(state, u32::MAX, 0)?.entries)
}

fn get_history_stats_from_db(state: &AppState) -> Result<HistoryStats, String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    conn.query_row(
        "SELECT
            COUNT(*),
            COALESCE(SUM(duration_seconds), 0),
            COALESCE(SUM(file_size_bytes), 0)
         FROM history_entries",
        [],
        |row| {
            let video_count: i64 = row.get(0)?;
            let total_duration_seconds: i64 = row.get(1)?;
            let total_file_size_bytes: i64 = row.get(2)?;
            Ok(HistoryStats {
                video_count: video_count.max(0) as u64,
                total_duration_seconds: total_duration_seconds.max(0) as u64,
                total_file_size_bytes: total_file_size_bytes.max(0) as u64,
            })
        },
    )
    .map_err(|e| format!("History stats read failed: {e}"))
}

fn insert_history_entry_in_db(state: &AppState, entry: &HistoryEntry) -> Result<(), String> {
    let entry = normalize_history_entry(entry.clone());
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    conn.execute(
        "INSERT OR REPLACE INTO history_entries (
            id,
            url,
            title,
            filename,
            thumbnail,
            upload_date,
            timestamp,
            duration_seconds,
            file_size_bytes,
            platform,
            output_path,
            created_at,
            completed_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            entry.id,
            entry.url,
            entry.title,
            entry.filename,
            entry.thumbnail,
            entry.upload_date,
            entry.timestamp,
            entry.duration_seconds,
            entry.file_size_bytes,
            entry.platform,
            entry.output_path,
            millis_to_i64(entry.created_at),
            entry.completed_at.map(millis_to_i64),
        ],
    )
    .map_err(|e| format!("History insert failed: {e}"))?;
    Ok(())
}

fn delete_history_entry_from_db(state: &AppState, id: &str) -> Result<(), String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    conn.execute("DELETE FROM history_entries WHERE id = ?1", params![id])
        .map_err(|e| format!("History delete failed: {e}"))?;
    Ok(())
}

fn clear_history_entries_in_db(state: &AppState) -> Result<(), String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    conn.execute("DELETE FROM history_entries", [])
        .map_err(|e| format!("History clear failed: {e}"))?;
    Ok(())
}

fn migrate_legacy_history_json(app: &AppHandle, state: &AppState) -> Result<(), String> {
    if count_history_entries_in_db(state)? > 0 {
        return Ok(());
    }

    for entry in load_legacy_history_json(app) {
        insert_history_entry_in_db(state, &entry)?;
    }
    Ok(())
}

fn load_legacy_config_json(app: &AppHandle) -> Option<AppConfig> {
    if let Ok(path) = config_path(app) {
        if let Ok(raw) = fs::read_to_string(path) {
            if let Ok(cfg) = serde_json::from_str::<AppConfig>(&raw) {
                return Some(normalize_app_config(cfg));
            }
        }
    }
    None
}

fn get_app_config_from_conn(conn: &Connection) -> rusqlite::Result<AppConfig> {
    conn.query_row(
        "SELECT yt_dlp_path, default_output_dir, selected_preset_key, magic_import_enabled, cut_at_timestamp_enabled, last_download_url
         FROM app_config
         WHERE id = 1",
        [],
        |row| {
            Ok(normalize_app_config(AppConfig {
                yt_dlp_path: row.get(0)?,
                default_output_dir: row.get(1)?,
                selected_preset_key: row.get(2)?,
                magic_import_enabled: row.get::<_, i64>(3)? != 0,
                cut_at_timestamp_enabled: row.get::<_, i64>(4)? != 0,
                last_download_url: row.get(5)?,
            }))
        },
    )
}

fn load_config_from_db(conn: &Connection) -> Result<AppConfig, String> {
    get_app_config_from_conn(conn).map_err(|e| format!("Config read failed: {e}"))
}

fn upsert_app_config_in_conn(conn: &Connection, config: &AppConfig) -> rusqlite::Result<()> {
    let config = normalize_app_config(config.clone());
    conn.execute(
        "INSERT INTO app_config (
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
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            datetime('now'),
            datetime('now')
        )
        ON CONFLICT(id) DO UPDATE SET
            yt_dlp_path = excluded.yt_dlp_path,
            default_output_dir = excluded.default_output_dir,
            selected_preset_key = excluded.selected_preset_key,
            magic_import_enabled = excluded.magic_import_enabled,
            cut_at_timestamp_enabled = excluded.cut_at_timestamp_enabled,
            last_download_url = excluded.last_download_url,
            updated_at = datetime('now')",
        params![
            config.yt_dlp_path,
            config.default_output_dir,
            config.selected_preset_key,
            if config.magic_import_enabled { 1 } else { 0 },
            if config.cut_at_timestamp_enabled {
                1
            } else {
                0
            },
            config.last_download_url,
        ],
    )?;
    Ok(())
}

fn save_config_to_db(state: &AppState, config: &AppConfig) -> Result<(), String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    upsert_app_config_in_conn(&conn, config).map_err(|e| format!("Config write failed: {e}"))
}

fn get_app_meta_value(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM app_meta WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
}

fn set_app_meta_value(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO app_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn migrate_legacy_config_json(app: &AppHandle, conn: &Connection) -> Result<(), String> {
    let already_migrated = get_app_meta_value(conn, LEGACY_CONFIG_MIGRATION_KEY)
        .map_err(|e| format!("Config migration check failed: {e}"))?
        .as_deref()
        == Some("1");

    if already_migrated {
        return Ok(());
    }

    if let Some(config) = load_legacy_config_json(app) {
        upsert_app_config_in_conn(conn, &config)
            .map_err(|e| format!("Config migration failed: {e}"))?;
    }

    set_app_meta_value(conn, LEGACY_CONFIG_MIGRATION_KEY, "1")
        .map_err(|e| format!("Config migration marker failed: {e}"))?;
    Ok(())
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
            filename TEXT,
            thumbnail TEXT,
            upload_date TEXT,
            timestamp INTEGER,
            duration_seconds INTEGER,
            file_size_bytes INTEGER,
            platform TEXT,
            output_path TEXT,
            created_at INTEGER NOT NULL,
            completed_at INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_link_dump_secrets_active
            ON link_dump_secrets(revoked_at, deleted_at);

        CREATE INDEX IF NOT EXISTS idx_history_entries_completed_at
            ON history_entries(completed_at, created_at);
        "#,
    )?;

    ensure_history_entries_timestamp_column(conn)?;
    ensure_history_entries_duration_seconds_column(conn)?;
    ensure_history_entries_file_size_bytes_column(conn)?;
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
    let mut stmt = conn.prepare("PRAGMA table_info(history_entries)")?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        if column? == column_name {
            return Ok(());
        }
    }
    drop(stmt);

    conn.execute(
        &format!("ALTER TABLE history_entries ADD COLUMN {column_name} INTEGER"),
        [],
    )?;
    Ok(())
}

fn get_link_dump_settings_from_conn(conn: &Connection) -> rusqlite::Result<LinkDumpSettings> {
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

fn get_link_dump_settings(state: &AppState) -> Result<LinkDumpSettings, String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    get_link_dump_settings_from_conn(&conn)
        .map_err(|e| format!("Link Dump settings read failed: {e}"))
}

fn update_link_dump_settings_in_db(
    state: &AppState,
    patch: LinkDumpSettingsPatch,
) -> Result<LinkDumpSettings, String> {
    let mut current = get_link_dump_settings(state)?;
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

    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    conn.execute(
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
    get_link_dump_settings_from_conn(&conn)
        .map_err(|e| format!("Link Dump settings read failed: {e}"))
}

fn list_link_dump_secrets_from_conn(
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

fn list_link_dump_secrets(state: &AppState) -> Result<Vec<LinkDumpSecretView>, String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    list_link_dump_secrets_from_conn(&conn)
        .map_err(|e| format!("Link Dump secrets read failed: {e}"))
}

fn next_link_dump_secret_name(conn: &Connection) -> rusqlite::Result<String> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM link_dump_secrets WHERE deleted_at IS NULL",
        [],
        |row| row.get(0),
    )?;
    Ok(format!("Link Dump Connection {}", count + 1))
}

fn generate_link_dump_secret_value() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("Secret generation failed: {e}"))?;
    Ok(format!("pfld_{}", URL_SAFE_NO_PAD.encode(bytes)))
}

fn hash_link_dump_secret(secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    bytes_to_hex(&hasher.finalize())
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn constant_time_eq_str(left: &str, right: &str) -> bool {
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

fn create_link_dump_secret_in_db(
    state: &AppState,
    name: Option<String>,
) -> Result<GeneratedLinkDumpSecret, String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    let clean_name = name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .map(Ok)
        .unwrap_or_else(|| next_link_dump_secret_name(&conn))
        .map_err(|e| format!("Link Dump name generation failed: {e}"))?;

    let secret = generate_link_dump_secret_value()?;
    let secret_hash = hash_link_dump_secret(&secret);
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO link_dump_secrets (id, name, secret_hash, created_at)
         VALUES (?1, ?2, ?3, datetime('now'))",
        params![id, clean_name, secret_hash],
    )
    .map_err(|e| format!("Link Dump secret create failed: {e}"))?;

    let connection = conn
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

    Ok(GeneratedLinkDumpSecret { secret, connection })
}

fn revoke_link_dump_secret_in_db(state: &AppState, id: &str) -> Result<(), String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    conn.execute(
        "UPDATE link_dump_secrets
         SET revoked_at = COALESCE(revoked_at, datetime('now'))
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
    )
    .map_err(|e| format!("Link Dump secret revoke failed: {e}"))?;
    Ok(())
}

fn delete_link_dump_secret_in_db(state: &AppState, id: &str) -> Result<(), String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    conn.execute(
        "UPDATE link_dump_secrets
         SET deleted_at = COALESCE(deleted_at, datetime('now'))
         WHERE id = ?1",
        params![id],
    )
    .map_err(|e| format!("Link Dump secret delete failed: {e}"))?;
    Ok(())
}

fn validate_link_dump_secret(
    state: &AppState,
    secret: Option<&str>,
) -> Result<Option<ValidSecretResult>, String> {
    let Some(secret) = secret.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let candidate_hash = hash_link_dump_secret(secret);
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
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
        conn.execute(
            "UPDATE link_dump_secrets SET last_used_at = datetime('now') WHERE id = ?1",
            params![valid.id],
        )
        .map_err(|e| format!("Link Dump secret last-used update failed: {e}"))?;
    }

    Ok(matched)
}

fn snapshot_link_dump_server_status(state: &AppState) -> LinkDumpServerStatus {
    state
        .link_dump_server
        .lock()
        .map(|runtime| runtime.status.clone())
        .unwrap_or_default()
}

fn emit_link_dump_server_status(app: &AppHandle, state: &AppState) {
    let _ = app.emit_all(
        "link-dump:server-status",
        snapshot_link_dump_server_status(state),
    );
}

#[tauri::command]
fn get_link_dump_overview(state: State<AppState>) -> Result<LinkDumpOverview, String> {
    Ok(LinkDumpOverview {
        settings: get_link_dump_settings(state.inner())?,
        secrets: list_link_dump_secrets(state.inner())?,
        server_status: snapshot_link_dump_server_status(state.inner()),
    })
}

#[tauri::command]
fn update_link_dump_settings(
    app: AppHandle,
    state: State<AppState>,
    patch: LinkDumpSettingsPatch,
) -> Result<LinkDumpOverview, String> {
    let settings = update_link_dump_settings_in_db(state.inner(), patch)?;
    let server_status = restart_link_dump_server_internal(&app, state.inner())?;
    Ok(LinkDumpOverview {
        settings,
        secrets: list_link_dump_secrets(state.inner())?,
        server_status,
    })
}

#[tauri::command]
fn create_link_dump_secret(
    state: State<AppState>,
    name: Option<String>,
) -> Result<GeneratedLinkDumpSecret, String> {
    create_link_dump_secret_in_db(state.inner(), name)
}

#[tauri::command]
fn revoke_link_dump_secret(
    state: State<AppState>,
    id: String,
) -> Result<Vec<LinkDumpSecretView>, String> {
    revoke_link_dump_secret_in_db(state.inner(), &id)?;
    list_link_dump_secrets(state.inner())
}

#[tauri::command]
fn delete_link_dump_secret(
    state: State<AppState>,
    id: String,
) -> Result<Vec<LinkDumpSecretView>, String> {
    delete_link_dump_secret_in_db(state.inner(), &id)?;
    list_link_dump_secrets(state.inner())
}

#[tauri::command]
fn restart_link_dump_server(
    app: AppHandle,
    state: State<AppState>,
) -> Result<LinkDumpServerStatus, String> {
    restart_link_dump_server_internal(&app, state.inner())
}

fn normalize_link_dump_host(host: &str) -> String {
    if host.trim() == "127.0.1" {
        return LINK_DUMP_DEFAULT_HOST.to_string();
    }
    host.trim().to_string()
}

fn is_allowed_link_dump_host(host: &str) -> bool {
    let normalized = normalize_link_dump_host(host);
    if normalized.eq_ignore_ascii_case("localhost") {
        return true;
    }
    normalized
        .parse::<std::net::IpAddr>()
        .map(|addr| addr.is_loopback())
        .unwrap_or(false)
}

fn link_dump_server_url(settings: &LinkDumpSettings) -> String {
    format!("http://{}:{}", settings.host, settings.port)
}

fn restart_link_dump_server_internal(
    app: &AppHandle,
    state: &AppState,
) -> Result<LinkDumpServerStatus, String> {
    stop_link_dump_server(state);
    start_link_dump_server_from_settings(app, state)
}

fn stop_link_dump_server(state: &AppState) {
    let handle = {
        let Ok(mut runtime) = state.link_dump_server.lock() else {
            return;
        };
        if let Some(shutdown) = runtime.shutdown.take() {
            shutdown.store(true, Ordering::SeqCst);
        }
        runtime.handle.take()
    };

    if let Some(handle) = handle {
        let _ = handle.join();
    }

    if let Ok(mut runtime) = state.link_dump_server.lock() {
        runtime.status.status = "stopped".to_string();
        runtime.status.error_message = None;
    }
}

fn set_link_dump_server_status(
    app: &AppHandle,
    state: &AppState,
    status: LinkDumpServerStatus,
) -> LinkDumpServerStatus {
    if let Ok(mut runtime) = state.link_dump_server.lock() {
        runtime.status = status.clone();
    }
    emit_link_dump_server_status(app, state);
    status
}

fn start_link_dump_server_from_settings(
    app: &AppHandle,
    state: &AppState,
) -> Result<LinkDumpServerStatus, String> {
    let mut settings = get_link_dump_settings(state)?;
    settings.host = normalize_link_dump_host(&settings.host);
    let url = link_dump_server_url(&settings);

    if !settings.server_enabled {
        let status = LinkDumpServerStatus {
            status: "stopped".to_string(),
            url,
            error_message: None,
        };
        return Ok(set_link_dump_server_status(app, state, status));
    }

    if !is_allowed_link_dump_host(&settings.host) {
        let status = LinkDumpServerStatus {
            status: "error".to_string(),
            url,
            error_message: Some("Link Dump Server only supports loopback hosts.".to_string()),
        };
        return Ok(set_link_dump_server_status(app, state, status));
    }

    let bind_addr = format!("{}:{}", settings.host, settings.port);
    let listener = match TcpListener::bind(&bind_addr) {
        Ok(listener) => listener,
        Err(err) => {
            let message = if err.kind() == std::io::ErrorKind::AddrInUse {
                format!(
                    "Link Dump Server could not start. Port {} is already in use.",
                    settings.port
                )
            } else {
                format!("Link Dump Server could not start: {err}")
            };
            let status = LinkDumpServerStatus {
                status: "error".to_string(),
                url,
                error_message: Some(message),
            };
            return Ok(set_link_dump_server_status(app, state, status));
        }
    };

    listener
        .set_nonblocking(true)
        .map_err(|e| format!("Link Dump listener setup failed: {e}"))?;

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_thread = shutdown.clone();
    let app_handle = app.clone();
    let url_for_thread = url.clone();
    let handle = thread::spawn(move || {
        println!("Server started on {bind_addr}");
        while !shutdown_thread.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let request_app = app_handle.clone();
                    thread::spawn(move || {
                        if let Err(err) = handle_link_dump_stream(stream, request_app) {
                            eprintln!("Link Dump request rejected: {err}");
                        }
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(err) => {
                    let state = app_handle.state::<AppState>();
                    let status = LinkDumpServerStatus {
                        status: "error".to_string(),
                        url: url_for_thread.clone(),
                        error_message: Some(format!("Link Dump Server stopped: {err}")),
                    };
                    let _ = set_link_dump_server_status(&app_handle, &state, status);
                    break;
                }
            }
        }
    });

    let status = LinkDumpServerStatus {
        status: "running".to_string(),
        url,
        error_message: None,
    };
    {
        let mut runtime = state
            .link_dump_server
            .lock()
            .map_err(|_| "Link Dump server lock poisoned")?;
        runtime.status = status.clone();
        runtime.shutdown = Some(shutdown);
        runtime.handle = Some(handle);
    }
    emit_link_dump_server_status(app, state);
    Ok(status)
}

fn handle_link_dump_stream(mut stream: TcpStream, app: AppHandle) -> Result<(), String> {
    let request = match read_http_request(&mut stream) {
        Ok(request) => request,
        Err(err) => {
            let _ = write_json_response(
                &mut stream,
                400,
                &json!({ "ok": false, "error": "Bad request" }),
            );
            return Err(err);
        }
    };

    let path = request.path.split('?').next().unwrap_or("").to_string();
    if request.method == "OPTIONS" {
        if is_link_dump_endpoint(&path) {
            return write_options_response(&mut stream);
        }
        return write_json_response(
            &mut stream,
            404,
            &json!({ "ok": false, "error": "Not found" }),
        );
    }

    if request.method != "POST" {
        return write_json_response(
            &mut stream,
            405,
            &json!({ "ok": false, "error": "Method not allowed" }),
        );
    }

    let state = app.state::<AppState>();
    match path.as_str() {
        "/addYoutubeLinkToQueue/" | "/addYoutubeLinkToQueue" => {
            handle_add_youtube_link(&app, state.inner(), &mut stream, &request.body)
        }
        "/addYoutubeLinksToQueue/" | "/addYoutubeLinksToQueue" => {
            handle_add_youtube_links(&app, state.inner(), &mut stream, &request.body)
        }
        _ => write_json_response(
            &mut stream,
            404,
            &json!({ "ok": false, "error": "Not found" }),
        ),
    }
}

fn is_link_dump_endpoint(path: &str) -> bool {
    matches!(
        path,
        "/addYoutubeLinkToQueue/"
            | "/addYoutubeLinkToQueue"
            | "/addYoutubeLinksToQueue/"
            | "/addYoutubeLinksToQueue"
    )
}

fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("Read timeout setup failed: {e}"))?;
    let mut buffer = Vec::new();
    let mut temp = [0_u8; 4096];
    let header_end;

    loop {
        let read = stream
            .read(&mut temp)
            .map_err(|e| format!("HTTP request read failed: {e}"))?;
        if read == 0 {
            return Err("HTTP request closed before headers".to_string());
        }
        buffer.extend_from_slice(&temp[..read]);
        if buffer.len() > LINK_DUMP_MAX_BODY_BYTES {
            return Err("HTTP request too large".to_string());
        }
        if let Some(index) = find_header_end(&buffer) {
            header_end = index;
            break;
        }
    }

    let header_text = std::str::from_utf8(&buffer[..header_end])
        .map_err(|_| "HTTP headers are not UTF-8".to_string())?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| "Missing HTTP request line".to_string())?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| "Missing HTTP method".to_string())?
        .to_string();
    let path = request_parts
        .next()
        .ok_or_else(|| "Missing HTTP path".to_string())?
        .to_string();

    let mut content_length = 0_usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "Invalid Content-Length".to_string())?;
            }
        }
    }

    if content_length > LINK_DUMP_MAX_BODY_BYTES {
        return Err("HTTP body too large".to_string());
    }

    let body_start = header_end + 4;
    let mut body = buffer.get(body_start..).unwrap_or_default().to_vec();
    while body.len() < content_length {
        let read = stream
            .read(&mut temp)
            .map_err(|e| format!("HTTP body read failed: {e}"))?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&temp[..read]);
    }
    body.truncate(content_length);

    Ok(HttpRequest { method, path, body })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn write_options_response(stream: &mut TcpStream) -> Result<(), String> {
    let response = concat!(
        "HTTP/1.1 204 No Content\r\n",
        "Access-Control-Allow-Origin: *\r\n",
        "Access-Control-Allow-Methods: POST, OPTIONS\r\n",
        "Access-Control-Allow-Headers: Content-Type\r\n",
        "Access-Control-Max-Age: 86400\r\n",
        "Content-Length: 0\r\n",
        "Connection: close\r\n",
        "\r\n"
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|e| format!("HTTP response write failed: {e}"))
}

fn write_json_response(
    stream: &mut TcpStream,
    status_code: u16,
    body: &serde_json::Value,
) -> Result<(), String> {
    let status_text = match status_code {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let body = serde_json::to_string(body).map_err(|e| format!("JSON encode failed: {e}"))?;
    let response = format!(
        concat!(
            "HTTP/1.1 {} {}\r\n",
            "Content-Type: application/json\r\n",
            "Access-Control-Allow-Origin: *\r\n",
            "Access-Control-Allow-Methods: POST, OPTIONS\r\n",
            "Access-Control-Allow-Headers: Content-Type\r\n",
            "Access-Control-Max-Age: 86400\r\n",
            "Content-Length: {}\r\n",
            "Connection: close\r\n",
            "\r\n",
            "{}"
        ),
        status_code,
        status_text,
        body.as_bytes().len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|e| format!("HTTP response write failed: {e}"))
}

fn handle_add_youtube_link(
    app: &AppHandle,
    state: &AppState,
    stream: &mut TcpStream,
    body: &[u8],
) -> Result<(), String> {
    let parsed = serde_json::from_slice::<AddYoutubeLinkRequestBody>(body);
    let parsed = match parsed {
        Ok(parsed) => parsed,
        Err(_) => {
            return write_json_response(
                stream,
                400,
                &json!({ "ok": false, "error": "Invalid request body" }),
            );
        }
    };

    let Some(_valid_secret) = validate_link_dump_secret(state, parsed.secret.as_deref())? else {
        println!("Link Dump request rejected");
        return write_json_response(
            stream,
            401,
            &json!({ "ok": false, "error": "Unauthorized" }),
        );
    };

    let Some(url) = parsed.url.as_deref().and_then(normalize_youtube_url) else {
        return write_json_response(
            stream,
            400,
            &json!({ "ok": false, "error": "Invalid YouTube URL" }),
        );
    };

    let mut summary = LinkDumpQueueSummary {
        received: 1,
        added: 0,
        skipped: 0,
        invalid: 0,
    };
    if add_normalized_youtube_urls_to_queue(app, state, &[url], &mut summary).is_err() {
        return write_json_response(
            stream,
            500,
            &json!({ "ok": false, "error": "Internal server error" }),
        );
    }
    println!("Link Dump request accepted");
    println!("Added {} links to queue", summary.added);
    write_json_response(
        stream,
        200,
        &json!({
            "ok": true,
            "added": summary.added,
            "skipped": summary.skipped,
            "message": format!("Added {} YouTube link{} to queue.", summary.added, if summary.added == 1 { "" } else { "s" }),
        }),
    )
}

fn handle_add_youtube_links(
    app: &AppHandle,
    state: &AppState,
    stream: &mut TcpStream,
    body: &[u8],
) -> Result<(), String> {
    let parsed = serde_json::from_slice::<AddYoutubeLinksRequestBody>(body);
    let parsed = match parsed {
        Ok(parsed) => parsed,
        Err(_) => {
            return write_json_response(
                stream,
                400,
                &json!({ "ok": false, "error": "Invalid request body" }),
            );
        }
    };

    let Some(_valid_secret) = validate_link_dump_secret(state, parsed.secret.as_deref())? else {
        println!("Link Dump request rejected");
        return write_json_response(
            stream,
            401,
            &json!({ "ok": false, "error": "Unauthorized" }),
        );
    };

    let urls = parsed.urls.unwrap_or_default();
    if urls.is_empty() {
        return write_json_response(
            stream,
            400,
            &json!({ "ok": false, "error": "No valid YouTube URLs" }),
        );
    }

    let mut summary = LinkDumpQueueSummary {
        received: urls.len(),
        added: 0,
        skipped: 0,
        invalid: 0,
    };
    let mut seen = std::collections::HashSet::new();
    let mut normalized_urls = Vec::new();

    for raw_url in urls.iter().take(LINK_DUMP_MAX_BATCH_SIZE) {
        let Some(normalized) = normalize_youtube_url(raw_url) else {
            summary.invalid += 1;
            continue;
        };
        if !seen.insert(normalized.key.clone()) {
            summary.skipped += 1;
            continue;
        }
        normalized_urls.push(normalized);
    }

    if urls.len() > LINK_DUMP_MAX_BATCH_SIZE {
        summary.skipped += urls.len() - LINK_DUMP_MAX_BATCH_SIZE;
    }

    if normalized_urls.is_empty() {
        return write_json_response(
            stream,
            400,
            &json!({ "ok": false, "error": "No valid YouTube URLs" }),
        );
    }

    if add_normalized_youtube_urls_to_queue(app, state, &normalized_urls, &mut summary).is_err() {
        return write_json_response(
            stream,
            500,
            &json!({ "ok": false, "error": "Internal server error" }),
        );
    }

    println!("Link Dump request accepted");
    println!("Added {} links to queue", summary.added);
    write_json_response(
        stream,
        200,
        &json!({
            "ok": true,
            "received": summary.received,
            "added": summary.added,
            "skipped": summary.skipped,
            "invalid": summary.invalid,
            "message": format!("Added {} YouTube link{} to queue.", summary.added, if summary.added == 1 { "" } else { "s" }),
        }),
    )
}

fn add_normalized_youtube_urls_to_queue(
    app: &AppHandle,
    state: &AppState,
    normalized_urls: &[NormalizedYoutubeUrl],
    summary: &mut LinkDumpQueueSummary,
) -> Result<(), String> {
    let mut queued_keys = queued_youtube_keys(state)?;
    let mut jobs = Vec::new();

    for normalized in normalized_urls {
        if !queued_keys.insert(normalized.key.clone()) {
            summary.skipped += 1;
            continue;
        }

        let request = build_link_dump_download_request(state, normalized)?;
        jobs.push(build_download_job(state, request)?);
    }

    let added_count = jobs.len();
    enqueue_download_jobs(app, state, jobs)?;
    summary.added += added_count;
    Ok(())
}

fn build_link_dump_download_request(
    state: &AppState,
    normalized: &NormalizedYoutubeUrl,
) -> Result<DownloadRequest, String> {
    let (preset, cut_at_timestamp_enabled) = {
        let cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
        (
            download_preset_for_key(cfg.selected_preset_key.as_deref()),
            cfg.cut_at_timestamp_enabled,
        )
    };

    Ok(DownloadRequest {
        url: normalized.url.clone(),
        format: preset.format.to_string(),
        output_dir: None,
        extract_audio: preset.extract_audio,
        audio_format: preset.audio_format.map(str::to_string),
        transcribe_text: preset.transcribe_text,
        cut_at_timestamp_enabled,
        cut_start_time: None,
        filename_suffix: preset.filename_suffix.map(str::to_string),
        title: None,
        thumbnail: youtube_thumbnail_url_from_normalized(normalized),
        upload_date: None,
        timestamp: None,
        duration_seconds: None,
    })
}

fn queued_youtube_keys(state: &AppState) -> Result<std::collections::HashSet<String>, String> {
    let queue = state.queue.lock().map_err(|_| "Queue lock poisoned")?;
    Ok(queue
        .iter()
        .filter_map(|job| normalize_youtube_url(&job.url).map(|normalized| normalized.key))
        .collect())
}

fn normalize_youtube_url(input: &str) -> Option<NormalizedYoutubeUrl> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed = url::Url::parse(trimmed).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }

    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let host_without_www = host.strip_prefix("www.").unwrap_or(&host);
    let path_parts = parsed
        .path_segments()
        .map(|segments| segments.collect::<Vec<_>>())
        .unwrap_or_default();

    let video_id = if host_without_www == "youtu.be" {
        path_parts.first().map(|part| (*part).to_string())
    } else if matches!(
        host_without_www,
        "youtube.com" | "m.youtube.com" | "music.youtube.com"
    ) {
        match path_parts
            .first()
            .map(|part| part.to_ascii_lowercase())
            .as_deref()
        {
            Some("watch") => parsed.query_pairs().find_map(|(name, value)| {
                if name == "v" {
                    Some(value.into_owned())
                } else {
                    None
                }
            }),
            Some("shorts") | Some("live") | Some("embed") | Some("v") => {
                path_parts.get(1).map(|part| (*part).to_string())
            }
            _ => None,
        }
    } else {
        None
    }?;

    if !is_plausible_youtube_video_id(&video_id) {
        return None;
    }

    Some(NormalizedYoutubeUrl {
        url: format!("https://www.youtube.com/watch?v={video_id}"),
        key: format!("youtube:{video_id}"),
    })
}

fn is_plausible_youtube_video_id(video_id: &str) -> bool {
    let len = video_id.len();
    (6..=64).contains(&len)
        && video_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn youtube_thumbnail_url_from_normalized(normalized: &NormalizedYoutubeUrl) -> Option<String> {
    normalized
        .key
        .strip_prefix("youtube:")
        .map(|video_id| format!("https://i.ytimg.com/vi/{video_id}/mqdefault.jpg"))
}

fn main() {
    let context = tauri::generate_context!();
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
            get_config,
            set_config,
            set_selected_preset_key,
            cache_last_download_url,
            pick_output_dir,
            pick_txt_file,
            open_folder,
            open_file_path,
            read_clipboard_text,
            load_info,
            get_yt_dlp_installed_version,
            get_queue_status,
            set_queue_auto_start,
            start_queue,
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
                stop_link_dump_server(state.inner());
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
    fn app_config_defaults_are_loaded_from_sqlite() {
        let conn = Connection::open_in_memory().unwrap();
        run_link_dump_migrations(&conn).unwrap();

        let config = load_config_from_db(&conn).unwrap();

        assert_eq!(
            config.selected_preset_key.as_deref(),
            Some(DEFAULT_DOWNLOAD_PRESET_KEY)
        );
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
            magic_import_enabled: false,
            cut_at_timestamp_enabled: false,
            last_download_url: Some("https://example.com/watch".to_string()),
        };

        upsert_app_config_in_conn(&conn, &config).unwrap();
        let loaded = load_config_from_db(&conn).unwrap();

        assert_eq!(loaded.yt_dlp_path.as_deref(), Some("/opt/pinefetch/yt-dlp"));
        assert_eq!(
            loaded.default_output_dir.as_deref(),
            Some("/Users/example/Downloads")
        );
        assert_eq!(loaded.selected_preset_key.as_deref(), Some("audio_mp3"));
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
        assert_eq!(request.filename_suffix, None);
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
    }

    #[test]
    fn history_entries_are_stored_in_sqlite_with_file_metadata() {
        let state = link_dump_test_state();
        let entry = HistoryEntry {
            id: "history-1".to_string(),
            url: "https://www.youtube.com/watch?v=abc123".to_string(),
            title: Some("Example title".to_string()),
            filename: Some("Example title - Uploader - abc123.mp4".to_string()),
            thumbnail: Some("https://i.ytimg.com/vi/abc123/mqdefault.jpg".to_string()),
            upload_date: Some("20240501".to_string()),
            timestamp: Some(1_714_560_000),
            duration_seconds: Some(754),
            file_size_bytes: Some(42_000_000),
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

        let stats = get_history_stats_from_db(&state).unwrap();
        assert_eq!(stats.video_count, 1);
        assert_eq!(stats.total_duration_seconds, 754);
        assert_eq!(stats.total_file_size_bytes, 42_000_000);
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
                filename: Some(format!("example-{index}.mp4")),
                thumbnail: None,
                upload_date: None,
                timestamp: None,
                duration_seconds: None,
                file_size_bytes: None,
                platform: Some("example".to_string()),
                output_path: None,
                created_at: timestamp,
                completed_at: Some(timestamp),
            };
            insert_history_entry_in_db(&state, &entry).unwrap();
        }

        let first_page = list_history_page_from_db(&state, 50, 0).unwrap();
        let second_page = list_history_page_from_db(&state, 50, 50).unwrap();

        assert_eq!(first_page.entries.len(), 50);
        assert!(first_page.has_more);
        assert_eq!(first_page.entries[0].id, "history-54");
        assert_eq!(first_page.entries[49].id, "history-05");
        assert_eq!(second_page.entries.len(), 5);
        assert!(!second_page.has_more);
        assert_eq!(second_page.entries[0].id, "history-04");
        assert_eq!(second_page.entries[4].id, "history-00");
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
}
