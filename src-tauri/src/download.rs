use super::*;

pub(super) fn run_download_job(
    app: &AppHandle,
    state: &AppState,
    job: &DownloadJob,
) -> Result<DownloadRunResult, String> {
    let effective_job = effective_download_job(job);
    let job = &effective_job;
    let yt_dlp = resolve_yt_dlp(app, state)?;
    let ffmpeg_location = resolve_ffmpeg_location(app, &yt_dlp);
    let deno_path = resolve_deno_executable(app);
    let output_template = build_output_template(&job.output_dir, job.filename_suffix.as_deref());
    let output_template_for_fallback = output_template.clone();
    let save_instagram_captions =
        job.save_instagram_captions && detect_platform(&job.url).as_deref() == Some("instagram");

    let mut args = vec![
        "--no-playlist".to_string(),
        "--newline".to_string(),
        "--progress".to_string(),
        "--no-color".to_string(),
        "--print".to_string(),
        "after_move:filepath".to_string(),
        "--print".to_string(),
        "after_video:filepath".to_string(),
        "--print".to_string(),
        "after_move:pinefetch_metadata:%(.{filepath,title,uploader,thumbnail,upload_date,timestamp,duration})j".to_string(),
        "-f".to_string(),
        job.format.clone(),
        "-o".to_string(),
        output_template,
    ];

    if save_instagram_captions {
        args.push("--print".to_string());
        args.push("after_move:pinefetch_caption:%(.{filepath,description})j".to_string());
    }

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

    if let Some(format_sort) = site_format_sort(&job.url) {
        args.push("--format-sort".to_string());
        args.push(format_sort.to_string());
    }

    args.push(job.url.clone());

    let mut command = Command::new(&yt_dlp);
    command.args(args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_child_process_group(&mut command);

    let child = command.spawn().map_err(|e| format!("Spawn failed: {e}"))?;
    let child = Arc::new(Mutex::new(child));

    if let Err(err) = register_current_child(state, &child) {
        if let Ok(mut process) = child.lock() {
            let _ = terminate_child_process_tree(&mut process);
            let _ = process.wait();
        }
        return Err(err);
    }

    let (stdout, stderr) = {
        let mut guard = child.lock().map_err(|_| "Child lock poisoned")?;
        (guard.stdout.take(), guard.stderr.take())
    };

    let progress_re = Regex::new(r"\[download\]\s+([\d\.]+)%.*?at\s+([^\s]+).*?ETA\s+([^\s]+)")
        .map_err(|e| format!("Regex error: {e}"))?;
    let output_path_capture: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let caption_capture: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let metadata_capture: Arc<Mutex<Vec<(String, InfoResponse)>>> =
        Arc::new(Mutex::new(Vec::new()));
    let error_capture: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let app_stdout = app.clone();
    let id_stdout = job.id.clone();
    let output_path_for_stdout = output_path_capture.clone();
    let captions_for_stdout = caption_capture.clone();
    let metadata_for_stdout = metadata_capture.clone();
    let handle_out = thread::spawn(move || {
        if let Some(out) = stdout {
            let reader = BufReader::new(out);
            for line in reader.lines().flatten() {
                if !line.starts_with("pinefetch_caption:")
                    && !line.starts_with("pinefetch_metadata:")
                {
                    emit_log(
                        &app_stdout,
                        LogEvent {
                            id: id_stdout.clone(),
                            line: line.clone(),
                            is_error: false,
                        },
                    );
                }

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
                if let Some(caption) = parse_instagram_caption_line(&line) {
                    if let Ok(mut captions) = captions_for_stdout.lock() {
                        captions.push(caption);
                    }
                }
                if let Some(metadata) = parse_download_metadata_line(&line) {
                    if let Ok(mut entries) = metadata_for_stdout.lock() {
                        entries.push(metadata);
                    }
                }
            }
        }
    });

    let app_stderr = app.clone();
    let id_stderr = job.id.clone();
    let error_for_stderr = error_capture.clone();
    let handle_err = thread::spawn(move || {
        if let Some(err) = stderr {
            let reader = BufReader::new(err);
            for line in reader.lines().flatten() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    if let Ok(mut reason) = error_for_stderr.lock() {
                        if trimmed.starts_with("ERROR:") || reason.is_none() {
                            *reason = Some(trimmed.to_string());
                        }
                    }
                }
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
    clear_current_child(state, &child);
    let _ = handle_out.join();
    let _ = handle_err.join();

    let mut output_path = output_path_capture
        .lock()
        .ok()
        .and_then(|guard| select_existing_output_path(&guard));
    let mut info = None;
    let mut saved_captions = Vec::new();

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

        info = metadata_capture.lock().ok().and_then(|entries| {
            entries
                .iter()
                .find(|(path, _)| output_path.as_deref() == Some(path.as_str()))
                .or_else(|| (entries.len() == 1).then(|| entries.first()).flatten())
                .map(|(_, info)| info.clone())
        });

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

        if save_instagram_captions {
            let captions = caption_capture
                .lock()
                .map_err(|_| "Caption capture lock poisoned")?;
            if captions.is_empty() {
                emit_log(
                    app,
                    LogEvent {
                        id: job.id.clone(),
                        line: "[caption] Instagram did not provide a caption for this download"
                            .to_string(),
                        is_error: false,
                    },
                );
            }
            for (downloaded_path, caption) in captions.iter() {
                let final_path = if original_output_path.as_deref() == Some(downloaded_path) {
                    output_path.as_deref().unwrap_or(downloaded_path)
                } else {
                    downloaded_path
                };
                let caption_path = write_instagram_caption_sidecar(Path::new(final_path), caption)?;
                saved_captions.push(SavedInstagramCaption {
                    media_path: final_path.to_string(),
                    caption_path: caption_path.to_string_lossy().into_owned(),
                    text: caption.clone(),
                });
                emit_log(
                    app,
                    LogEvent {
                        id: job.id.clone(),
                        line: format!("[caption] saved: {}", caption_path.display()),
                        is_error: false,
                    },
                );
            }
        }
    }

    Ok(DownloadRunResult {
        exit_code: status.code().unwrap_or(-1),
        output_path,
        error: error_capture.lock().ok().and_then(|reason| reason.clone()),
        info,
        captions: saved_captions,
    })
}

pub(super) fn parse_download_metadata_line(line: &str) -> Option<(String, InfoResponse)> {
    let json = line.strip_prefix("pinefetch_metadata:")?;
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let path = value.get("filepath")?.as_str()?.trim().to_string();
    if path.is_empty() {
        return None;
    }
    let string = |key: &str| {
        value
            .get(key)
            .and_then(|item| item.as_str())
            .map(str::to_string)
    };
    Some((
        path,
        InfoResponse {
            title: string("title"),
            uploader: string("uploader"),
            duration: value.get("duration").and_then(json_value_to_i64),
            thumbnail: string("thumbnail"),
            upload_date: string("upload_date"),
            timestamp: value.get("timestamp").and_then(json_value_to_i64),
            formats: None,
            description: None,
            id: None,
        },
    ))
}

pub(super) fn parse_instagram_caption_line(line: &str) -> Option<(String, String)> {
    let json = line.strip_prefix("pinefetch_caption:")?;
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let path = value.get("filepath")?.as_str()?.trim();
    let caption = value.get("description")?.as_str()?;
    if path.is_empty() || caption.trim().is_empty() {
        return None;
    }
    Some((path.to_string(), caption.to_string()))
}

pub(super) fn write_instagram_caption_sidecar(
    media_path: &Path,
    caption: &str,
) -> Result<PathBuf, String> {
    if !media_path.is_file() {
        return Err(format!(
            "Caption media file not found: {}",
            media_path.display()
        ));
    }
    let caption_path = media_path.with_extension("caption.txt");
    fs::write(&caption_path, caption).map_err(|e| {
        format!(
            "Failed to save Instagram caption to {}: {e}",
            caption_path.display()
        )
    })?;
    Ok(caption_path)
}

pub(super) fn effective_download_job(job: &DownloadJob) -> DownloadJob {
    let mut download_job = job.clone();
    if job.transcribe_text && job.download_video_with_transcript {
        download_job.format = download_preset_for_key(Some(DEFAULT_DOWNLOAD_PRESET_KEY))
            .format
            .to_string();
        download_job.extract_audio = false;
        download_job.audio_format = None;
    }
    download_job
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

    let temp_path = build_cut_sidecar_path(input_path, "cut")?;

    let input_path_str = input_path.to_string_lossy().to_string();
    let temp_path_str = temp_path.to_string_lossy().to_string();

    let mut command = Command::new(&ffmpeg_path);
    command.args([
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
    ]);
    let output = run_command_output(command, Some(state), None, None)
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

    let final_path = match preserve_unique_cut_output(&temp_path, input_path, cut_start_time) {
        Ok(path) => path,
        Err(err) => {
            let _ = fs::remove_file(&temp_path);
            return Err(err);
        }
    };
    let _ = fs::remove_file(&temp_path);
    // Only remove the full download after the cut exists at a distinct path.
    let _ = fs::remove_file(input_path);

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
    for index in 1..=1000 {
        let candidate = if index == 1 {
            base.clone()
        } else {
            let stem = base
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| "Cut file has no usable file name".to_string())?;
            let extension = base.extension().and_then(|value| value.to_str());
            let mut name = format!("{stem}__{index}");
            if let Some(extension) = extension {
                name.push('.');
                name.push_str(extension);
            }
            base.with_file_name(name)
        };

        match fs::hard_link(temp_path, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => {
                // Some removable and network filesystems do not support hard links.
                let mut source = fs::File::open(temp_path)
                    .map_err(|err| format!("Could not read cut file: {err}"))?;
                let mut destination = match fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&candidate)
                {
                    Ok(file) => file,
                    Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(err) => return Err(format!("Could not save cut file: {err}")),
                };
                if let Err(err) =
                    std::io::copy(&mut source, &mut destination).and_then(|_| destination.flush())
                {
                    let _ = fs::remove_file(&candidate);
                    return Err(format!("Could not copy cut file: {err}"));
                }
                return Ok(candidate);
            }
        }
    }
    Err("Could not find a free file name for the timestamp cut".to_string())
}

pub(super) fn format_timestamp_filename_suffix(seconds: f64) -> String {
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

pub(super) fn parse_yt_dlp_filepath(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("pinefetch_caption:") {
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

pub(super) fn normalize_filepath_candidate(raw: &str) -> Option<String> {
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

    if let Some(format_sort) = site_format_sort(&job.url) {
        command.arg("--format-sort");
        command.arg(format_sort);
    }

    command.arg(&job.url);

    let output = run_command_output(command, Some(state), None, Some(INFO_TIMEOUT))
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

pub(super) fn ffmpeg_tool_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

pub(super) fn ffprobe_tool_name() -> &'static str {
    if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    }
}

pub(super) fn ffmpeg_tool_is_usable(path: &Path) -> bool {
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

pub(super) fn has_usable_ffmpeg_tools_in_dir(dir: &Path) -> bool {
    ffmpeg_tool_is_usable(&dir.join(ffmpeg_tool_name()))
        && ffmpeg_tool_is_usable(&dir.join(ffprobe_tool_name()))
}

pub(super) fn normalize_ffmpeg_location(path: &Path) -> Option<String> {
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

pub(super) fn resolve_bundled_ffmpeg_location(app: &AppHandle) -> Option<String> {
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

pub(super) fn resolve_ffmpeg_location(app: &AppHandle, yt_dlp_path: &str) -> Option<String> {
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

pub(super) fn resolve_bundled_python(app: &AppHandle) -> Option<String> {
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

pub(super) fn resolve_bundled_deno(app: &AppHandle) -> Option<String> {
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

pub(super) fn resolve_deno_executable(app: &AppHandle) -> Option<String> {
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

pub(super) fn resolve_python_executable(app: &AppHandle) -> Option<String> {
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

pub(super) fn run_faster_whisper_transcription(
    app: &AppHandle,
    state: &AppState,
    job: &DownloadJob,
    output_path: Option<&str>,
) -> Result<String, String> {
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

    let transcript_path = Path::new(media_path).with_extension("txt");
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
    command
        .arg("-c")
        .arg(FASTER_WHISPER_TRANSCRIBE_SNIPPET)
        .arg(transcription_input)
        .arg(&transcript_path_str)
        .arg(&model_name)
        .arg(if job.transcribe_timestamps { "1" } else { "0" })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_child_process_group(&mut command);

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to start faster-whisper transcription: {e}"))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child = Arc::new(Mutex::new(child));
    if let Err(err) = register_current_child(state, &child) {
        if let Ok(mut process) = child.lock() {
            let _ = terminate_child_process_tree(&mut process);
            let _ = process.wait();
        }
        return Err(err);
    }
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

    let status = loop {
        let maybe_status = {
            let mut process = child.lock().map_err(|_| "Child lock poisoned")?;
            process
                .try_wait()
                .map_err(|e| format!("Failed while waiting for faster-whisper: {e}"))?
        };
        if let Some(status) = maybe_status {
            break status;
        }
        thread::sleep(Duration::from_millis(100));
    };
    clear_current_child(state, &child);
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

pub(super) fn extract_temporary_transcription_audio(
    app: &AppHandle,
    state: &AppState,
    job: &DownloadJob,
    video_path: &str,
) -> Result<TemporaryTranscriptionAudio, String> {
    let yt_dlp = resolve_yt_dlp(app, state)?;
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
    let audio_path = parent.join(format!(".pinefetch-transcription-{}.wav", Uuid::new_v4()));
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
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-i",
        video_path_str.as_str(),
        "-vn",
        "-ac",
        "1",
        "-ar",
        "16000",
        "-c:a",
        "pcm_s16le",
        audio_path_str.as_str(),
    ]);
    let output = run_command_output(command, Some(state), None, None)
        .map_err(|e| format!("Failed to prepare audio for transcription: {e}"))?;

    if !output.status.success() {
        let _ = fs::remove_file(&audio_path);
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
    if !audio_path.exists() {
        return Err("ffmpeg finished but no transcription audio was created".to_string());
    }

    Ok(TemporaryTranscriptionAudio { path: audio_path })
}

pub(super) fn build_output_template(output_dir: &str, filename_suffix: Option<&str>) -> String {
    let mut path = PathBuf::from(output_dir);
    // Use title, but fallback to uploader and id for platforms where title might be missing or duplicate
    // %(title)s - video title
    // %(uploader)s - uploader name
    // %(id)s - unique video ID (ensures uniqueness for Instagram posts from same creator)
    let suffix = filename_suffix.unwrap_or("");
    path.push(format!("%(title)s - %(uploader)s - %(id)s{suffix}.%(ext)s"));
    path.to_string_lossy().to_string()
}

pub(super) fn normalize_filename_suffix(raw: Option<&str>) -> Option<String> {
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
