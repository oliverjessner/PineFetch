use crate::completion;
use crate::config_rules::normalize_faster_whisper_model;
use crate::download_rules::build_output_template;
use crate::download_rules::effective_download_job;
use crate::download_rules::format_timestamp_filename_suffix;
use crate::download_rules::format_yt_dlp_timestamp;
use crate::events::emit_log;
use crate::events::emit_progress;
use crate::files::publish_unique_output;
use crate::files::OwnedTemporaryFile;
use crate::files::TemporaryTranscriptionAudio;
use crate::media_tools::{
    build_cut_args, build_transcription_args, build_transcription_audio_args,
    parse_transcription_line, transcription_failure_message, TranscriptionLine,
};
use crate::metadata::INFO_TIMEOUT;
use crate::models::DownloadJob;
use crate::models::DownloadProgress;
use crate::models::DownloadRunResult;
use crate::models::InfoResponse;
use crate::models::LogEvent;
use crate::models::SavedCaption;
use crate::models::TranscriptionRunResult;
use crate::platform::caption_platform;
use crate::process::{run_command_output, run_command_streaming, ProcessError};
use crate::runtime::ffmpeg_tool_name;
use crate::runtime::resolve_deno_executable;
use crate::runtime::resolve_ffmpeg_location;
use crate::runtime::resolve_python_executable;
use crate::runtime::resolve_yt_dlp;
use crate::state::AppState;
use crate::yt_dlp::build_download_args;
use crate::yt_dlp::build_filename_probe_args;
use crate::yt_dlp::parse_caption_line;
use crate::yt_dlp::parse_download_metadata_line;
use crate::yt_dlp::parse_progress_line;
use crate::yt_dlp::parse_yt_dlp_filepath;
use regex::Regex;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use tauri::AppHandle;
use uuid::Uuid;

pub(super) fn run_download_job(
    app: &AppHandle,
    state: &AppState,
    job: &DownloadJob,
) -> Result<DownloadRunResult, String> {
    let effective_job = effective_download_job(job);
    let job = &effective_job;
    let yt_dlp = resolve_yt_dlp(app, &state.config)?;
    let ffmpeg_location = resolve_ffmpeg_location(app, &yt_dlp);
    let deno_path = resolve_deno_executable(app);
    let output_template = build_output_template(&job.output_dir, job.filename_suffix.as_deref());
    let output_template_for_fallback = output_template.clone();
    let caption_platform = caption_platform(&job.url);
    let capture_captions = caption_platform.is_some();

    let args = build_download_args(
        job,
        output_template,
        ffmpeg_location.as_deref(),
        deno_path.as_deref(),
    )?;
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

    let mut command = Command::new(&yt_dlp);
    let browser = state
        .config
        .lock()
        .map_err(|_| "Config lock poisoned")?
        .youtube_cookie_browser
        .clone();
    crate::yt_dlp::apply_browser_cookies(&mut command, &job.url, browser.as_deref());
    command.args(args);
    let progress_re =
        Regex::new(crate::yt_dlp::PROGRESS_PATTERN).map_err(|e| format!("Regex error: {e}"))?;
    let mut output_paths = Vec::new();
    let mut subtitle_capture = Vec::new();
    let mut caption_capture = Vec::new();
    let mut metadata_capture: Vec<(String, InfoResponse)> = Vec::new();
    let mut error_capture = None;
    let reddit_caption = caption_platform.as_deref() == Some("reddit");
    let status = run_command_streaming(
        command,
        &state.processes,
        |line| {
            if line.starts_with("pinefetch_subtitles:") {
                if subtitle_capture.len() < 16 && line.len() <= 4 * 1024 * 1024 {
                    subtitle_capture.push(line.to_string());
                }
                return;
            }
            if !line.starts_with("pinefetch_caption:") && !line.starts_with("pinefetch_metadata:") {
                emit_log(
                    app,
                    LogEvent {
                        id: job.id.clone(),
                        line: line.to_string(),
                        is_error: false,
                    },
                );
            }
            if let Some(progress) = parse_progress_line(line, &progress_re, &job.id) {
                emit_progress(app, progress);
            }
            if let Some(path) = parse_yt_dlp_filepath(line) {
                output_paths.push(path);
            }
            if let Some(caption) = parse_caption_line(line, reddit_caption) {
                caption_capture.push(caption);
            }
            if let Some(metadata) = parse_download_metadata_line(line) {
                metadata_capture.push(metadata);
            }
        },
        |line| {
            let trimmed = line.trim();
            if !trimmed.is_empty() && (trimmed.starts_with("ERROR:") || error_capture.is_none()) {
                error_capture = Some(trimmed.to_string());
            }
            emit_log(
                app,
                LogEvent {
                    id: job.id.clone(),
                    line: line.to_string(),
                    is_error: true,
                },
            );
        },
    )
    .map_err(|error| match error {
        ProcessError::Spawn(error) => format!("Spawn failed: {error}"),
        ProcessError::Wait(error) => format!("Wait failed: {error}"),
        error => error.to_string(),
    })?;
    let mut output_path = select_existing_output_path(&output_paths);
    let mut info = None;
    let mut saved_captions = Vec::new();
    let mut saved_subtitles = Vec::new();

    if status.success() {
        if output_path.is_none() {
            output_path = resolve_existing_output_path_fallback(
                state,
                job,
                &yt_dlp,
                deno_path.as_deref(),
                &output_template_for_fallback,
            );
        }

        info = metadata_capture
            .iter()
            .find(|(path, _)| output_path.as_deref() == Some(path.as_str()))
            .or_else(|| {
                (metadata_capture.len() == 1)
                    .then(|| metadata_capture.first())
                    .flatten()
            })
            .map(|(_, info)| info.clone());

        completion::output_ready(&state.db, job, output_path.as_deref()).map_err(|e| {
            format!("Downloaded output receipt failed: {e}; any produced file preserved")
        })?;

        if job.save_creator_subtitles {
            if let Some(media) = output_path.as_deref() {
                for line in &subtitle_capture {
                    match crate::subtitles::collect(line, Path::new(media), job.cut_start_time.unwrap_or(0.0)) {
                        Ok(tracks) => {
                            for track in tracks {
                                if !saved_subtitles.iter().any(|existing: &crate::subtitles::SavedSubtitle| existing.path == track.path) {
                                    saved_subtitles.push(track);
                                }
                            }
                        },
                        Err(_) => emit_log(app, LogEvent { id: job.id.clone(), line: "[subtitles] Some subtitle files could not be saved to History; downloaded files are preserved.".into(), is_error: true }),
                    }
                }
            }
        }
        let original_output_path = output_path.clone();
        if let Some(cut_start_time) = job.cut_start_time {
            let trimmed_path = trim_downloaded_file(
                app,
                state,
                job,
                output_path.as_deref(),
                ffmpeg_location.as_deref(),
                cut_start_time,
            )?;
            output_path = Some(trimmed_path);
        }

        if capture_captions {
            if caption_capture.is_empty() {
                emit_log(
                    app,
                    LogEvent {
                        id: job.id.clone(),
                        line: format!(
                            "[caption] {} did not provide a caption for this download",
                            caption_platform.as_deref().unwrap_or("Site")
                        ),
                        is_error: false,
                    },
                );
            }
            for (downloaded_path, caption) in &caption_capture {
                let final_path = if original_output_path.as_deref() == Some(downloaded_path) {
                    output_path.as_deref().unwrap_or(downloaded_path)
                } else {
                    downloaded_path
                };
                let saved = match prepare_saved_caption(
                    Path::new(final_path),
                    caption,
                    job.save_captions,
                ) {
                    Ok(saved) => saved,
                    Err(error) if job.save_captions => {
                        emit_log(app, LogEvent { id: job.id.clone(), line: format!("[caption] TXT export failed: {error}. Caption will be saved in History."), is_error: true });
                        prepare_saved_caption(Path::new(final_path), caption, false)?
                    }
                    Err(error) => return Err(error),
                };
                if let Some(path) = &saved.caption_path {
                    emit_log(
                        app,
                        LogEvent {
                            id: job.id.clone(),
                            line: format!("[caption] saved: {path}"),
                            is_error: false,
                        },
                    );
                }
                saved_captions.push(saved);
            }
        }
    }

    Ok(DownloadRunResult {
        exit_code: status.code().unwrap_or(-1),
        output_path,
        error: error_capture,
        info,
        captions: saved_captions,
        subtitles: saved_subtitles,
    })
}

pub(super) fn prepare_saved_caption(
    media_path: &Path,
    caption: &str,
    save_file: bool,
) -> Result<SavedCaption, String> {
    if !media_path.is_file() {
        return Err("Caption media file not found; saved text requires downloaded media".into());
    }
    let caption_path = if save_file {
        Some(
            write_caption_sidecar(media_path, caption)?
                .to_string_lossy()
                .into_owned(),
        )
    } else {
        None
    };
    Ok(SavedCaption {
        media_path: media_path.to_string_lossy().into_owned(),
        caption_path,
        text: caption.to_string(),
    })
}

pub(super) fn write_caption_sidecar(media_path: &Path, caption: &str) -> Result<PathBuf, String> {
    if !media_path.is_file() {
        return Err(format!(
            "Caption media file not found: {}",
            media_path.display()
        ));
    }
    let mut temporary =
        OwnedTemporaryFile::create_at(build_cut_sidecar_path(media_path, "caption")?)?;
    temporary
        .write_all(caption.as_bytes())
        .map_err(|e| format!("Caption write failed: {e}; media preserved"))?;
    temporary.sync()?;
    publish_unique_output(&temporary.path, &media_path.with_extension("caption.txt"))
}

pub(super) fn trim_downloaded_file(
    app: &AppHandle,
    state: &AppState,
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

    let temporary = OwnedTemporaryFile::create_at(build_cut_sidecar_path(
        input_path,
        &format!("cut-{}", job.id),
    )?)?;
    let temp_path = &temporary.path;

    let input_path_str = input_path.to_string_lossy().to_string();
    let temp_path_str = temp_path.to_string_lossy().to_string();

    let mut command = Command::new(&ffmpeg_path);
    command.args(build_cut_args(
        &input_path_str,
        &temp_path_str,
        &cut_timestamp,
    ));
    let output = run_command_output(command, Some(&state.processes), None, None)
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
        let code = output.status.code().unwrap_or(-1);
        return Err(format!("ffmpeg timestamp cut failed with exit code {code}"));
    }

    if !temp_path.is_file() || fs::metadata(temp_path).map_err(|e| e.to_string())?.len() == 0 {
        return Err("ffmpeg finished but no cut file was created".to_string());
    }

    temporary.sync()?;
    let final_path = preserve_unique_cut_output(temp_path, input_path, cut_start_time)?;
    // yt-dlp can reuse a pre-existing full download. Its ownership is unknown;
    // keep it even after the new cut has been published.

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

pub(super) fn build_cut_sidecar_path(input_path: &Path, label: &str) -> Result<PathBuf, String> {
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

pub(super) fn build_timestamp_cut_output_path(
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

pub(super) fn preserve_unique_cut_output(
    temp_path: &Path,
    input_path: &Path,
    cut_start_time: f64,
) -> Result<PathBuf, String> {
    let base = build_timestamp_cut_output_path(input_path, cut_start_time)?;
    publish_unique_output(temp_path, &base)
}

pub(super) fn select_existing_output_path(candidates: &[String]) -> Option<String> {
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

pub(super) fn is_format_part_path(path: &Path) -> bool {
    path.file_stem()
        .and_then(|value| value.to_str())
        .and_then(|stem| stem.rsplit_once(".f"))
        .map(|(_, suffix)| !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()))
        .unwrap_or(false)
}

pub(super) fn resolve_existing_output_path_fallback(
    state: &AppState,
    job: &DownloadJob,
    yt_dlp: &str,
    deno_path: Option<&str>,
    output_template: &str,
) -> Option<String> {
    let expected_path =
        probe_expected_output_filename(state, job, yt_dlp, deno_path, output_template).ok()??;
    let candidates = existing_output_candidates_from_expected(&expected_path, job);
    select_existing_output_path(&candidates)
}

pub(super) fn probe_expected_output_filename(
    state: &AppState,
    job: &DownloadJob,
    yt_dlp: &str,
    deno_path: Option<&str>,
    output_template: &str,
) -> Result<Option<String>, String> {
    let mut command = Command::new(yt_dlp);
    let browser = state
        .config
        .lock()
        .map_err(|_| "Config lock poisoned")?
        .youtube_cookie_browser
        .clone();
    crate::yt_dlp::apply_browser_cookies(&mut command, &job.url, browser.as_deref());
    command.args(build_filename_probe_args(job, output_template, deno_path));

    let output = run_command_output(command, Some(&state.processes), None, Some(INFO_TIMEOUT))
        .map_err(|e| format!("Failed to probe output filename with yt-dlp: {e}"))?;

    if !output.status.success() {
        return Ok(None);
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find_map(parse_yt_dlp_filepath))
}

pub(super) fn existing_output_candidates_from_expected(
    expected_path: &str,
    job: &DownloadJob,
) -> Vec<String> {
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

pub(super) fn related_existing_output_paths(expected: &Path) -> Vec<String> {
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

pub(super) fn run_faster_whisper_transcription(
    app: &AppHandle,
    state: &AppState,
    job: &DownloadJob,
    output_path: Option<&str>,
) -> Result<TranscriptionRunResult, String> {
    let media_path = output_path
        .ok_or_else(|| "Could not determine downloaded file path for transcription".to_string())?;
    if !Path::new(media_path).exists() {
        return Err(format!(
            "Downloaded file not found for transcription: {media_path}"
        ));
    }

    let temporary_audio = if job.download_video_with_transcript {
        Some(extract_temporary_transcription_audio(
            app, state, job, media_path,
        )?)
    } else {
        None
    };
    let transcription_input = temporary_audio
        .as_ref()
        .map(|audio| audio.path.as_path())
        .unwrap_or_else(|| Path::new(media_path));

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

    let transcript_temporary = OwnedTemporaryFile::create_at(build_cut_sidecar_path(
        Path::new(media_path),
        &format!("transcript-{}", job.id),
    )?)?;
    let transcript_path = &transcript_temporary.path;
    let transcript_path_str = transcript_path.to_string_lossy().to_string();
    let model_name = normalize_faster_whisper_model(&job.faster_whisper_model);
    emit_log(
        app,
        LogEvent {
            id: job.id.clone(),
            line: format!("[faster-whisper] model: {model_name}"),
            is_error: false,
        },
    );

    let mut command = Command::new(python);
    command.args(build_transcription_args(
        transcription_input,
        &transcript_path_str,
        &model_name,
        job.transcribe_timestamps,
    ));
    let mut language = None;
    let mut stderr_line = String::new();
    let status = run_command_streaming(
        command,
        &state.processes,
        |line| match parse_transcription_line(line) {
            TranscriptionLine::Language(detected) => {
                if language.is_none() {
                    language = detected;
                }
            }
            TranscriptionLine::Log(line) => emit_log(
                app,
                LogEvent {
                    id: job.id.clone(),
                    line: format!("[faster-whisper] {line}"),
                    is_error: false,
                },
            ),
        },
        |line| {
            if !line.trim().is_empty() {
                stderr_line = line.trim().chars().take(1024).collect();
            }
            emit_log(
                app,
                LogEvent {
                    id: job.id.clone(),
                    line: format!("[faster-whisper] {line}"),
                    is_error: true,
                },
            );
        },
    )
    .map_err(|error| match error {
        ProcessError::Spawn(error) => {
            format!("Failed to start faster-whisper transcription: {error}")
        }
        ProcessError::Wait(error) => {
            format!("Failed while waiting for faster-whisper: {error}")
        }
        error => error.to_string(),
    })?;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        return Err(transcription_failure_message(code, &stderr_line));
    }

    if !transcript_path.exists() {
        return Err("faster-whisper finished but no transcript file was created".to_string());
    }

    let language = language.ok_or_else(|| {
        "faster-whisper finished without reporting the detected language".to_string()
    })?;
    emit_log(
        app,
        LogEvent {
            id: job.id.clone(),
            line: format!("[faster-whisper] language: {language}"),
            is_error: false,
        },
    );

    transcript_temporary.sync()?;
    let final_path = publish_unique_output(
        transcript_path,
        &Path::new(media_path).with_extension("txt"),
    )?;
    Ok(TranscriptionRunResult {
        transcript_path: final_path.to_string_lossy().to_string(),
        language,
    })
}

pub(super) fn extract_temporary_transcription_audio(
    app: &AppHandle,
    state: &AppState,
    job: &DownloadJob,
    video_path: &str,
) -> Result<TemporaryTranscriptionAudio, String> {
    let yt_dlp = resolve_yt_dlp(app, &state.config)?;
    let ffmpeg_location = resolve_ffmpeg_location(app, &yt_dlp)
        .ok_or_else(|| "ffmpeg not available for transcription audio extraction".to_string())?;
    let ffmpeg_path = Path::new(&ffmpeg_location).join(ffmpeg_tool_name());
    if !ffmpeg_path.exists() {
        return Err(format!(
            "ffmpeg executable not found for transcription: {}",
            ffmpeg_path.to_string_lossy()
        ));
    }

    let video_path = Path::new(video_path);
    let parent = video_path
        .parent()
        .ok_or_else(|| "Downloaded video has no parent directory".to_string())?;
    let audio_path = parent.join(format!(
        ".pinefetch-transcription-{}-{}.wav",
        job.id,
        Uuid::new_v4()
    ));
    let temporary = OwnedTemporaryFile::create_at(audio_path.clone())?;
    let video_path_str = video_path.to_string_lossy().to_string();
    let audio_path_str = audio_path.to_string_lossy().to_string();

    emit_log(
        app,
        LogEvent {
            id: job.id.clone(),
            line: "[transcript] preparing audio from video".to_string(),
            is_error: false,
        },
    );

    let mut command = Command::new(&ffmpeg_path);
    command.args(build_transcription_audio_args(
        &video_path_str,
        &audio_path_str,
    ));
    let output = run_command_output(command, Some(&state.processes), None, None)
        .map_err(|e| format!("Failed to prepare audio for transcription: {e}"))?;

    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if details.is_empty() {
            format!(
                "ffmpeg transcription audio extraction failed with exit code {}",
                output.status.code().unwrap_or(-1)
            )
        } else {
            format!("ffmpeg transcription audio extraction failed: {details}")
        });
    }
    if !audio_path.is_file() || fs::metadata(&audio_path).map_err(|e| e.to_string())?.len() == 0 {
        return Err("ffmpeg finished but no transcription audio was created".to_string());
    }

    temporary.sync()?;
    Ok(temporary)
}
