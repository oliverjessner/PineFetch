use crate::models::InstalledYtDlpVersion;
use crate::process::run_command_output;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use tauri::AppHandle;
use tauri::Manager;

pub(crate) fn ffmpeg_tool_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

pub(crate) fn ffprobe_tool_name() -> &'static str {
    if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    }
}

pub(crate) fn ffmpeg_tool_is_usable(path: &Path) -> bool {
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

pub(crate) fn has_usable_ffmpeg_tools_in_dir(dir: &Path) -> bool {
    ffmpeg_tool_is_usable(&dir.join(ffmpeg_tool_name()))
        && ffmpeg_tool_is_usable(&dir.join(ffprobe_tool_name()))
}

pub(crate) fn normalize_ffmpeg_location(path: &Path) -> Option<String> {
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

pub(crate) fn resolve_bundled_ffmpeg_location(app: &AppHandle) -> Option<String> {
    for relative in [
        "ffmpeg-runtime/bin",
        "ffmpeg-runtime",
        "resources/ffmpeg-runtime/bin",
        "resources/ffmpeg-runtime",
    ] {
        if let Ok(path) = app
            .path()
            .resolve(relative, tauri::path::BaseDirectory::Resource)
        {
            if let Some(location) = normalize_ffmpeg_location(&path) {
                return Some(location);
            }
        }
    }
    None
}

pub(crate) fn resolve_ffmpeg_location(app: &AppHandle, yt_dlp_path: &str) -> Option<String> {
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

pub(crate) fn resolve_bundled_python(app: &AppHandle) -> Option<String> {
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
        if let Ok(path) = app
            .path()
            .resolve(relative, tauri::path::BaseDirectory::Resource)
        {
            if path.exists() {
                return Some(path.to_string_lossy().to_string());
            }
        }
    }

    None
}

pub(crate) fn resolve_bundled_deno(app: &AppHandle) -> Option<String> {
    #[cfg(target_os = "windows")]
    let candidates = vec![
        "deno-runtime/bin/deno.exe",
        "resources/deno-runtime/bin/deno.exe",
    ];

    #[cfg(not(target_os = "windows"))]
    let candidates = vec!["deno-runtime/bin/deno", "resources/deno-runtime/bin/deno"];

    for relative in candidates {
        if let Ok(path) = app
            .path()
            .resolve(relative, tauri::path::BaseDirectory::Resource)
        {
            if path.exists() {
                return Some(path.to_string_lossy().to_string());
            }
        }
    }

    None
}

pub(crate) fn resolve_deno_executable(app: &AppHandle) -> Option<String> {
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

pub(crate) fn resolve_python_executable(app: &AppHandle) -> Option<String> {
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

pub(crate) fn resolve_output_dir(
    state: &crate::config::ConfigState,
    requested: Option<String>,
) -> Result<String, String> {
    if let Some(dir) = requested {
        if dir.trim().is_empty() {
            return Err("Output directory is empty".to_string());
        }
        return Ok(dir);
    }
    let cfg = state.lock().map_err(|_| "Config lock poisoned")?;
    cfg.default_output_dir
        .clone()
        .ok_or_else(|| "Default output directory not set".to_string())
}

pub(crate) fn resolve_yt_dlp(
    _app: &AppHandle,
    state: &crate::config::ConfigState,
) -> Result<String, String> {
    let cfg = state.lock().map_err(|_| "Config lock poisoned")?;
    resolve_yt_dlp_in_paths(
        cfg.yt_dlp_path.as_deref(),
        std::env::var_os("PATH").as_deref(),
    )
}

pub(crate) fn resolve_yt_dlp_in_paths(
    configured: Option<&str>,
    search_path: Option<&std::ffi::OsStr>,
) -> Result<String, String> {
    if let Some(path) = configured {
        if Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }
    if let Some(path) = search_path.and_then(|paths| find_in_search_path("yt-dlp", paths)) {
        return Ok(path);
    }

    Err("yt-dlp not found. Set its path in Settings.".to_string())
}

pub(crate) fn resolve_yt_dlp_for_version(
    app: &AppHandle,
    state: &crate::config::ConfigState,
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

pub(crate) fn find_in_path(binary: &str) -> Option<String> {
    let paths = std::env::var_os("PATH")?;
    find_in_search_path(binary, &paths)
}

fn find_in_search_path(binary: &str, paths: &std::ffi::OsStr) -> Option<String> {
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

pub(crate) fn installed_yt_dlp_version(
    app: AppHandle,
    state: &crate::config::ConfigState,
    path: Option<String>,
    processes: &crate::process::ProcessState,
) -> Result<InstalledYtDlpVersion, String> {
    let yt_dlp = resolve_yt_dlp_for_version(&app, state, path)?;
    let mut command = Command::new(&yt_dlp);
    command.arg("--version");
    let output = run_command_output(
        command,
        None,
        Some(processes),
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
