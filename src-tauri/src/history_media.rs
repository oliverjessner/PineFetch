//! Local playback derivatives for formats the platform webview cannot decode.
use crate::files::OwnedTemporaryFile;
use crate::ocr::{fingerprint, Fingerprint};
use crate::process::{self, ProcessState};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{atomic::Ordering, Mutex};
use std::time::Duration;

#[derive(Default)]
pub(crate) struct HistoryMediaState {
    // Transcript, Subtitles and OCR can request the same derivative simultaneously.
    pub(crate) cache: Mutex<crate::playback_cache::PlaybackCache>,
    pub(crate) conversion: Mutex<()>,
    pub(crate) generation: std::sync::atomic::AtomicU64,
}

fn cache_path(directory: &Path, source: &Path, identity: &Fingerprint) -> PathBuf {
    let mut digest = Sha256::new();
    digest.update(b"pinefetch-playback-h264-aac-720p-v1\0");
    digest.update(source.as_os_str().as_encoded_bytes());
    digest.update(format!("{identity:?}"));
    directory.join(format!(
        "{}.mp4",
        crate::hashing::bytes_to_hex(&digest.finalize())
    ))
}

fn is_mp4(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() < 32 {
        return false;
    }
    let mut header = [0; 12];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && &header[4..8] == b"ftyp"
}

pub(crate) fn cached(directory: &Path, source: &Path) -> Result<Option<PathBuf>, String> {
    let path = cache_path(directory, source, &fingerprint(source)?);
    Ok(is_mp4(&path).then_some(path))
}

fn playback_command(tool: &Path, source: &Path, output: &Path) -> Command {
    let mut command = Command::new(tool);
    command
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y"])
        .args(["-threads", "2", "-i"])
        .arg(source)
        .args(["-map", "0:V:0", "-map", "0:a:0?", "-sn", "-dn"])
        .args(["-map_metadata", "-1", "-map_chapters", "-1", "-vf"])
        .arg("scale=w='min(1280,iw)':h='min(720,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2,setsar=1")
        .args([
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "25", "-pix_fmt", "yuv420p", "-threads",
            "2", "-c:a", "aac", "-b:a", "128k", "-ac", "2", "-movflags", "+faststart", "-f", "mp4",
        ])
        .arg(output);
    command
}

// This short-lived file pin bridges publication and the caller's player lease.
// It is acquired only after FFmpeg exits (Windows byte locks block writers).
pub(crate) struct PreparedPlayback {
    pub(crate) path: PathBuf,
    _pin: fs::File,
}

impl Drop for PreparedPlayback {
    fn drop(&mut self) {
        let _ = self._pin.unlock();
    }
}

fn pin(path: PathBuf) -> Result<PreparedPlayback, String> {
    let file = fs::File::open(&path).map_err(|_| "The playback preview is unavailable.")?;
    file.try_lock_shared()
        .map_err(|_| "The playback preview is busy.")?;
    Ok(PreparedPlayback { path, _pin: file })
}

struct PreparingPreview<'a> {
    temporary: OwnedTemporaryFile,
    cache: &'a Mutex<crate::playback_cache::PlaybackCache>,
}

impl Drop for PreparingPreview<'_> {
    fn drop(&mut self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.end_temporary(&self.temporary.path);
        }
    }
}

impl HistoryMediaState {
    #[cfg(test)]
    pub(crate) fn prepare(
        &self,
        directory: &Path,
        source: &Path,
        ffmpeg: &Path,
        processes: &ProcessState,
    ) -> Result<PathBuf, String> {
        let _conversion = self
            .conversion
            .lock()
            .map_err(|_| "Playback lock poisoned")?;
        prepare(directory, source, ffmpeg, processes, &self.cache)
            .map(|prepared| prepared.path.clone())
    }
}

// Caller serializes conversions. Active temporary files and the publication pin
// allow cleanup to continue safely while FFmpeg is working.
pub(crate) fn prepare(
    directory: &Path,
    source: &Path,
    ffmpeg: &Path,
    processes: &ProcessState,
    cache: &Mutex<crate::playback_cache::PlaybackCache>,
) -> Result<PreparedPlayback, String> {
    if processes.shutting_down.load(Ordering::SeqCst) {
        return Err("Application is shutting down.".into());
    }
    crate::ocr_source::validate(source)?;
    let before = fingerprint(source)?;
    let destination = cache_path(directory, source, &before);
    let preparing = {
        let mut guard = cache.lock().map_err(|_| "Playback lock poisoned")?;
        if is_mp4(&destination) {
            return pin(destination);
        }
        fs::create_dir_all(directory).map_err(|e| format!("Playback cache unavailable: {e}"))?;
        let temporary = OwnedTemporaryFile::create_at(
            directory.join(format!("{}.tmp.mp4", uuid::Uuid::new_v4())),
        )?;
        guard.begin_temporary(temporary.path.clone());
        PreparingPreview { temporary, cache }
    };
    let temporary = &preparing.temporary;
    let output = process::run_command_output(
        playback_command(ffmpeg, source, &temporary.path),
        None,
        Some(processes),
        Some(Duration::from_secs(30 * 60)),
    )
    .map_err(|e| format!("Local playback preparation failed: {e}"))?;
    if !output.status.success() || !is_mp4(&temporary.path) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr.lines().rev().find(|line| !line.trim().is_empty());
        return Err(format!(
            "Local playback preparation failed: {}",
            reason.unwrap_or("FFmpeg did not produce a playable MP4.")
        ));
    }
    if fingerprint(source)? != before {
        return Err("The saved video changed while preparing playback. Reopen the tab.".into());
    }
    temporary.sync()?;
    // Cache cleanup and publication cannot interleave. The file pin stays live
    // after rename and until the caller has acquired its longer-lived lease.
    let _guard = cache.lock().map_err(|_| "Playback lock poisoned")?;
    let mut prepared = pin(temporary.path.clone())?;
    fs::rename(&temporary.path, &destination)
        .map_err(|e| format!("Could not save playback cache: {e}"))?;
    prepared.path = destination;
    Ok(prepared)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn integration_playback_prepares_h264_aac_with_the_bundled_runtime() {
        let tools = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/ffmpeg-runtime/bin");
        let ffmpeg = tools.join("ffmpeg");
        let ffprobe = tools.join("ffprobe");
        if !ffmpeg.is_file() || !ffprobe.is_file() {
            eprintln!("Playback smoke requires npm run prepare:ffmpeg-runtime.");
            return;
        }
        let root = crate::test_support::TempRoot::new("playback-runtime");
        let source = root.path("Grüße #1.mkv");
        let processes = ProcessState::default();
        let mut generate = Command::new(&ffmpeg);
        generate
            .args(["-nostdin", "-v", "error", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=160x90:rate=10:duration=2")
            .args(["-f", "lavfi", "-i"])
            .arg("sine=frequency=440:sample_rate=48000:duration=2")
            .args(["-c:v", "ffv1", "-c:a", "pcm_s16le"])
            .arg(&source);
        let made = process::run_command_output(
            generate,
            None,
            Some(&processes),
            Some(Duration::from_secs(20)),
        )
        .unwrap();
        assert!(
            made.status.success(),
            "{}",
            String::from_utf8_lossy(&made.stderr)
        );
        let original = fs::read(&source).unwrap();
        let prepared = HistoryMediaState::default()
            .prepare(&root.path("cache"), &source, &ffmpeg, &processes)
            .unwrap();
        assert_eq!(fs::read(&source).unwrap(), original);
        let mut probe = Command::new(ffprobe);
        probe
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
            ])
            .arg(&prepared);
        let output = process::run_command_output(
            probe,
            None,
            Some(&processes),
            Some(Duration::from_secs(20)),
        )
        .unwrap();
        assert!(output.status.success());
        let data: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let streams = data["streams"].as_array().unwrap();
        assert!(streams
            .iter()
            .any(|s| s["codec_name"] == "h264" && s["pix_fmt"] == "yuv420p"));
        assert!(streams.iter().any(|s| s["codec_name"] == "aac"));
        let duration: f64 = data["format"]["duration"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!((duration - 2.0).abs() < 0.15, "{duration}");
        assert_eq!(
            cached(&root.path("cache"), &source).unwrap(),
            Some(prepared)
        );
    }

    #[cfg(unix)]
    fn fixture_tool(root: &crate::test_support::TempRoot, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let tool = root.path("ffmpeg");
        fs::write(&tool, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o700)).unwrap();
        tool
    }

    #[cfg(unix)]
    const SUCCESS: &str = r#"
source=''
previous=''
for argument do
    if [ "$previous" = '-i' ]; then source="$argument"; fi
    previous="$argument"
done
output="$previous"
printf '\000\000\000\040ftypisom000000000000000000000000' > "$output"
"#;

    #[cfg(unix)]
    #[test]
    fn integration_playback_cache_reuses_output_and_invalidates_changed_sources() {
        let root = crate::test_support::TempRoot::new("playback-cache");
        let source = root.path("Grüße #1.webm");
        fs::write(&source, b"original video").unwrap();
        let directory = root.path("cache");
        let tool = fixture_tool(&root, SUCCESS);
        let state = HistoryMediaState::default();
        let processes = ProcessState::default();
        let prepared = state
            .prepare(&directory, &source, &tool, &processes)
            .unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"original video");
        assert_eq!(cached(&directory, &source).unwrap(), Some(prepared.clone()));
        // An unavailable encoder cannot affect an already completed cache entry.
        let reused = state
            .prepare(
                &directory,
                &source,
                &root.path("missing-ffmpeg"),
                &processes,
            )
            .unwrap();
        assert_eq!(reused, prepared);
        fs::write(&source, b"changed and longer original video").unwrap();
        assert_eq!(cached(&directory, &source).unwrap(), None);
        let changed = state
            .prepare(&directory, &source, &tool, &processes)
            .unwrap();
        assert_ne!(changed, prepared);
    }

    #[test]
    fn integration_conversion_keeps_player_leases_available_and_protects_its_temporary_file() {
        let root = crate::test_support::TempRoot::new("playback-concurrent");
        let source = root.path("source.webm");
        fs::write(&source, b"original video").unwrap();
        let directory = root.path("cache");
        let tool = fixture_tool(
            &root,
            &format!(
                r#"{SUCCESS}
printf ready > "$source.ready"
count=0
while [ ! -f "$source.continue" ]; do
    count=$((count + 1))
    [ "$count" -lt 500 ] || exit 1
    sleep 0.01
done
"#
            ),
        );
        let state = std::sync::Arc::new(HistoryMediaState::default());
        let processes = std::sync::Arc::new(ProcessState::default());
        let worker_state = state.clone();
        let worker_processes = processes.clone();
        let worker_source = source.clone();
        let worker_directory = directory.clone();
        let worker = std::thread::spawn(move || {
            worker_state.prepare(&worker_directory, &worker_source, &tool, &worker_processes)
        });
        let ready = source.with_extension("webm.ready");
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !ready.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let started = ready.exists();
        let leases_available = state.cache.try_lock().is_ok();
        let conversion_protected = state.conversion.try_lock().is_err();
        state
            .cache
            .lock()
            .unwrap()
            .prune(&directory, 0, Duration::ZERO, std::time::SystemTime::now())
            .unwrap();
        let temporary_protected = fs::read_dir(&directory).unwrap().any(|entry| {
            entry
                .unwrap()
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".tmp.mp4")
        });
        fs::write(source.with_extension("webm.continue"), b"continue").unwrap();
        let prepared = worker.join().unwrap().unwrap();
        assert!(started && leases_available && conversion_protected && temporary_protected);
        assert!(is_mp4(&prepared));
        assert_eq!(fs::read(&source).unwrap(), b"original video");
        assert!(processes.utility_children.lock().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn integration_failed_or_stale_playback_is_never_published() {
        let root = crate::test_support::TempRoot::new("playback-failure");
        let source = root.path("source.webm");
        fs::write(&source, b"original video").unwrap();
        let directory = root.path("cache");
        let state = HistoryMediaState::default();
        let processes = ProcessState::default();
        let failed = fixture_tool(
            &root,
            &format!("{SUCCESS}\necho 'encoder failed' >&2\nexit 1"),
        );
        assert!(state
            .prepare(&directory, &source, &failed, &processes)
            .unwrap_err()
            .contains("encoder failed"));
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        assert_eq!(fs::read(&source).unwrap(), b"original video");
        let changed = fixture_tool(
            &root,
            &format!("{SUCCESS}\nprintf ' changed' >> \"$source\""),
        );
        assert!(state
            .prepare(&directory, &source, &changed, &processes)
            .unwrap_err()
            .contains("changed"));
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
    }
}
