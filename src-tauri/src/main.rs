use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    yt_dlp_path: Option<String>,
    default_output_dir: Option<String>,
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
    formats: Option<Vec<InfoFormat>>,
    description: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryEntry {
    id: String,
    url: String,
    title: Option<String>,
    thumbnail: Option<String>,
    platform: Option<String>,
    output_path: Option<String>,
    created_at: u64,
    completed_at: Option<u64>,
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

struct AppState {
    config: Mutex<AppConfig>,
    queue: Mutex<VecDeque<DownloadJob>>,
    queue_auto_start: Mutex<bool>,
    worker_running: Mutex<bool>,
    current_job_id: Mutex<Option<String>>,
    current_child: Mutex<Option<Arc<Mutex<Child>>>>,
    cancel_requested: Mutex<Option<String>>,
    history: Mutex<Vec<HistoryEntry>>,
}

impl AppState {
    fn new(config: AppConfig) -> Self {
        Self {
            config: Mutex::new(config),
            queue: Mutex::new(VecDeque::new()),
            queue_auto_start: Mutex::new(true),
            worker_running: Mutex::new(false),
            current_job_id: Mutex::new(None),
            current_child: Mutex::new(None),
            cancel_requested: Mutex::new(None),
            history: Mutex::new(Vec::new()),
        }
    }
}

#[tauri::command]
fn get_config(state: State<AppState>) -> Result<AppConfig, String> {
    let cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
    Ok(cfg.clone())
}

#[tauri::command]
fn set_config(app: AppHandle, state: State<AppState>, config: AppConfig) -> Result<(), String> {
    {
        let mut cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
        *cfg = config.clone();
    }
    save_config(&app, &config)
}

#[tauri::command]
fn cache_last_download_url(
    app: AppHandle,
    state: State<AppState>,
    url: String,
) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let next_config = {
        let mut cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
        cfg.last_download_url = Some(trimmed.to_string());
        cfg.clone()
    };

    save_config(&app, &next_config)
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
    let mut command = Command::new(yt_dlp);
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
        duration: value.get("duration").and_then(|v| v.as_i64()),
        thumbnail: value
            .get("thumbnail")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
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
    if !is_valid_url(&request.url) {
        return Err("URL must start with http:// or https://".to_string());
    }

    let output_dir = resolve_output_dir(&state, request.output_dir.clone())?;
    let cut_start_time = resolve_cut_start_time(
        request.cut_at_timestamp_enabled,
        request.cut_start_time,
        &request.url,
    );
    let id = Uuid::new_v4().to_string();
    let job = DownloadJob {
        id: id.clone(),
        url: request.url,
        format: request.format,
        output_dir,
        extract_audio: request.extract_audio,
        audio_format: request.audio_format,
        transcribe_text: request.transcribe_text,
        title: request.title,
        thumbnail: request.thumbnail,
        cut_start_time,
        filename_suffix: normalize_filename_suffix(request.filename_suffix.as_deref()),
    };

    {
        let mut queue = state.queue.lock().map_err(|_| "Queue lock poisoned")?;
        queue.push_back(job);
    }

    emit_queue(&app, &state)?;
    if is_queue_auto_start_enabled(state.inner())? {
        ensure_worker(&app, state.inner())?;
    }
    Ok(id)
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
fn get_history(state: State<AppState>) -> Result<Vec<HistoryEntry>, String> {
    let history = state.history.lock().map_err(|_| "History lock poisoned")?;
    Ok(history.clone())
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

fn add_history_entry_on_success(
    app: &AppHandle,
    state: &AppState,
    job: &DownloadJob,
    output_path: Option<&str>,
) {
    let entry = HistoryEntry {
        id: Uuid::new_v4().to_string(),
        url: job.url.clone(),
        title: job.title.clone(),
        thumbnail: job.thumbnail.clone(),
        platform: detect_platform(&job.url),
        output_path: output_path.map(|s| s.to_string()),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        completed_at: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        ),
    };

    let mut history = match state.history.lock() {
        Ok(h) => h,
        Err(_) => return,
    };

    history.push(entry);

    // Clone for saving, then drop lock
    let history_clone = history.clone();
    drop(history);
    let _ = save_history(app, &history_clone);
}

#[tauri::command]
fn remove_history_entry(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    let mut history = state.history.lock().map_err(|_| "History lock poisoned")?;
    let before = history.len();
    history.retain(|entry| entry.id != id);
    let removed = before != history.len();
    if removed {
        let history_clone = history.clone();
        drop(history);
        save_history(&app, &history_clone)?;
    }
    Ok(())
}

#[tauri::command]
fn clear_history(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let mut history = state.history.lock().map_err(|_| "History lock poisoned")?;
    history.clear();
    drop(history);
    save_history(&app, &[])?;
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
                                    Some(transcript_path.as_str()),
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

    let mut args = vec![
        "--no-playlist".to_string(),
        "--newline".to_string(),
        "--progress".to_string(),
        "--no-color".to_string(),
        "--print".to_string(),
        "after_move:filepath".to_string(),
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

    let mut command = Command::new(yt_dlp);
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
    let output_path_capture: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

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

                if let Some(path_line) = parse_after_move_filepath(&line) {
                    if let Ok(mut slot) = output_path_for_stdout.lock() {
                        *slot = Some(path_line);
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
        .and_then(|guard| guard.clone())
        .filter(|candidate| Path::new(candidate).exists());

    if status.success() {
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

fn parse_after_move_filepath(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('[') {
        return None;
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return None;
    }
    Some(trimmed.to_string())
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

fn has_ffmpeg_tools_in_dir(dir: &Path) -> bool {
    dir.join(ffmpeg_tool_name()).exists() && dir.join(ffprobe_tool_name()).exists()
}

fn normalize_ffmpeg_location(path: &Path) -> Option<String> {
    if path.is_dir() {
        if has_ffmpeg_tools_in_dir(path) {
            return Some(path.to_string_lossy().to_string());
        }
        return None;
    }

    if path.is_file() {
        if let Some(parent) = path.parent() {
            if has_ffmpeg_tools_in_dir(parent) {
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

fn history_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = tauri::api::path::app_data_dir(&app.config()).ok_or("Data directory unavailable")?;
    fs::create_dir_all(&dir).map_err(|e| format!("Data dir create failed: {e}"))?;
    Ok(dir.join("history.json"))
}

fn load_history(app: &AppHandle) -> Vec<HistoryEntry> {
    if let Ok(path) = history_path(app) {
        if let Ok(raw) = fs::read_to_string(path) {
            if let Ok(history) = serde_json::from_str::<Vec<HistoryEntry>>(&raw) {
                return history;
            }
        }
    }
    Vec::new()
}

fn save_history(app: &AppHandle, history: &[HistoryEntry]) -> Result<(), String> {
    let path = history_path(app)?;
    let data = serde_json::to_string_pretty(history)
        .map_err(|e| format!("History serialize failed: {e}"))?;
    fs::write(path, data).map_err(|e| format!("History write failed: {e}"))
}

fn load_config(app: &AppHandle) -> AppConfig {
    if let Ok(path) = config_path(app) {
        if let Ok(raw) = fs::read_to_string(path) {
            if let Ok(cfg) = serde_json::from_str::<AppConfig>(&raw) {
                return cfg;
            }
        }
    }
    AppConfig::default()
}

fn save_config(app: &AppHandle, config: &AppConfig) -> Result<(), String> {
    let path = config_path(app)?;
    let data = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Config serialize failed: {e}"))?;
    fs::write(path, data).map_err(|e| format!("Config write failed: {e}"))
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let config = load_config(&app.handle());
            let state = AppState::new(config);
            // Load history from disk
            let history = load_history(&app.handle());
            *state.history.lock().map_err(|_| "History lock failed")? = history;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            cache_last_download_url,
            pick_output_dir,
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
            remove_history_entry,
            clear_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn rejects_unsafe_filename_suffix() {
        assert_eq!(
            normalize_filename_suffix(Some("__max")),
            Some("__max".to_string())
        );
        assert_eq!(normalize_filename_suffix(Some("../max")), None);
        assert_eq!(normalize_filename_suffix(Some("")), None);
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
}
