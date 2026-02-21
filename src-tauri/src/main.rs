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
};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AppConfig {
  yt_dlp_path: Option<String>,
  default_output_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DownloadRequest {
  url: String,
  format: String,
  output_dir: Option<String>,
  extract_audio: bool,
  audio_format: Option<String>,
  transcribe_text: bool,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledYtDlpVersion {
  version: String,
  path: String,
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
  worker_running: Mutex<bool>,
  current_job_id: Mutex<Option<String>>,
  current_child: Mutex<Option<Arc<Mutex<Child>>>>,
  cancel_requested: Mutex<Option<String>>,
}

impl AppState {
  fn new(config: AppConfig) -> Self {
    Self {
      config: Mutex::new(config),
      queue: Mutex::new(VecDeque::new()),
      worker_running: Mutex::new(false),
      current_job_id: Mutex::new(None),
      current_child: Mutex::new(None),
      cancel_requested: Mutex::new(None),
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
  tauri::api::shell::open(&app.shell_scope(), path, None)
    .map_err(|e| format!("Open folder failed: {e}"))
}

#[tauri::command]
fn load_info(app: AppHandle, state: State<AppState>, url: String) -> Result<InfoResponse, String> {
  if !is_valid_url(&url) {
    return Err("URL must start with http:// or https://".to_string());
  }
  let yt_dlp = resolve_yt_dlp(&app, &state)?;
  let mut command = Command::new(yt_dlp);
  command.args(["--dump-json", "--no-playlist", "--no-warnings"]);
  if let Some(deno) = resolve_deno_executable(&app) {
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
  let value: serde_json::Value = serde_json::from_str(&raw)
    .map_err(|e| format!("Invalid JSON from yt-dlp: {e}"))?;

  let formats = value
    .get("formats")
    .and_then(|v| v.as_array())
    .map(|arr| {
      arr
        .iter()
        .map(|f| InfoFormat {
          format_id: f.get("format_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
          ext: f.get("ext").and_then(|v| v.as_str()).map(|s| s.to_string()),
          vcodec: f.get("vcodec").and_then(|v| v.as_str()).map(|s| s.to_string()),
          acodec: f.get("acodec").and_then(|v| v.as_str()).map(|s| s.to_string()),
          height: f.get("height").and_then(|v| v.as_i64()),
          width: f.get("width").and_then(|v| v.as_i64()),
          fps: f.get("fps").and_then(|v| v.as_f64()),
        })
        .collect::<Vec<_>>()
    });

  Ok(InfoResponse {
    title: value.get("title").and_then(|v| v.as_str()).map(|s| s.to_string()),
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
fn enqueue_download(
  app: AppHandle,
  state: State<AppState>,
  request: DownloadRequest,
) -> Result<String, String> {
  if !is_valid_url(&request.url) {
    return Err("URL must start with http:// or https://".to_string());
  }

  let output_dir = resolve_output_dir(&state, request.output_dir.clone())?;
  let id = Uuid::new_v4().to_string();
  let job = DownloadJob {
    id: id.clone(),
    url: request.url,
    format: request.format,
    output_dir,
    extract_audio: request.extract_audio,
    audio_format: request.audio_format,
    transcribe_text: request.transcribe_text,
  };

  {
    let mut queue = state.queue.lock().map_err(|_| "Queue lock poisoned")?;
    queue.push_back(job);
  }

  emit_queue(&app, &state)?;
  ensure_worker(app, state)?;
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

  let mut child_guard = state
    .current_child
    .lock()
    .map_err(|_| "Child lock poisoned")?;
  if let Some(child) = child_guard.as_mut() {
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

fn ensure_worker(app: AppHandle, state: State<AppState>) -> Result<(), String> {
  let mut running = state
    .worker_running
    .lock()
    .map_err(|_| "Worker lock poisoned")?;
  if *running {
    return Ok(());
  }
  *running = true;

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

            match run_faster_whisper_transcription(&app_handle, &job, run_result.output_path.as_deref()) {
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
  let output_template = build_output_template(&job.output_dir);

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
    job.url.clone(),
  ];

  if let Some(location) = ffmpeg_location.as_ref() {
    args.push("--ffmpeg-location".to_string());
    args.push(location.clone());
  } else if job.extract_audio || job.transcribe_text || job.format.contains('+') {
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
        emit_log(&app_stdout, LogEvent {
          id: id_stdout.clone(),
          line: line.clone(),
          is_error: false,
        });

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
        emit_log(&app_stderr, LogEvent {
          id: id_stderr.clone(),
          line,
          is_error: true,
        });
      }
    }
  });

  let status = {
    let mut guard = child.lock().map_err(|_| "Child lock poisoned")?;
    guard.wait().map_err(|e| format!("Wait failed: {e}"))?
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

  let output_path = output_path_capture
    .lock()
    .ok()
    .and_then(|guard| guard.clone())
    .filter(|candidate| Path::new(candidate).exists());

  Ok(DownloadRunResult {
    exit_code: status.code().unwrap_or(-1),
    output_path,
  })
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
  if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }
}

fn ffprobe_tool_name() -> &'static str {
  if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" }
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
  let candidates = vec![
    "deno-runtime/bin/deno",
    "resources/deno-runtime/bin/deno",
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

  for candidate in ["python3.12", "python3.11", "python3.10", "python3", "python"] {
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
    return Err(format!("Downloaded file not found for transcription: {audio_path}"));
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

fn build_output_template(output_dir: &str) -> String {
  let mut path = PathBuf::from(output_dir);
  path.push("%(title)s.%(ext)s");
  path.to_string_lossy().to_string()
}

fn emit_queue(app: &AppHandle, state: &AppState) -> Result<(), String> {
  let queue = state.queue.lock().map_err(|_| "Queue lock poisoned")?;
  app
    .emit_all("queue:update", queue.clone())
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

fn resolve_output_dir(state: &AppState, requested: Option<String>) -> Result<String, String> {
  if let Some(dir) = requested {
    if dir.trim().is_empty() {
      return Err("Output directory is empty".to_string());
    }
    return Ok(dir);
  }
  let cfg = state.config.lock().map_err(|_| "Config lock poisoned")?;
  cfg
    .default_output_dir
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
  let dir = tauri::api::path::app_config_dir(&app.config())
    .ok_or("Config directory unavailable")?;
  fs::create_dir_all(&dir).map_err(|e| format!("Config dir create failed: {e}"))?;
  Ok(dir.join("config.json"))
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
  let data = serde_json::to_string_pretty(config).map_err(|e| format!("Config serialize failed: {e}"))?;
  fs::write(path, data).map_err(|e| format!("Config write failed: {e}"))
}

fn main() {
  tauri::Builder::default()
    .setup(|app| {
      let config = load_config(&app.handle());
      app.manage(AppState::new(config));
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      get_config,
      set_config,
      pick_output_dir,
      open_folder,
      load_info,
      get_yt_dlp_installed_version,
      enqueue_download,
      cancel_download
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
