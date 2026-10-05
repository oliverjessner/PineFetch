use crate::events::emit_queue;
use crate::link_dump_store::get_link_dump_settings;
use crate::link_dump_store::validate_link_dump_secret;
use crate::models::AddVideoLinkRequestBody;
use crate::models::AddVideoLinksRequestBody;
use crate::models::DownloadRequest;
use crate::models::HttpRequest;
use crate::models::LinkDumpQueueSummary;
use crate::models::LinkDumpServerStatus;
use crate::models::NormalizedVideoUrl;
use crate::presets::download_preset_for_key;
use crate::queue::insert_unique_video_jobs;
use crate::queue::is_queue_auto_start_enabled;
use crate::state::AppState;
use crate::video_urls::is_allowed_link_dump_host;
use crate::video_urls::link_dump_server_url;
use crate::video_urls::normalize_link_dump_host;
use crate::video_urls::normalize_video_url;
use crate::worker::build_download_job;
use crate::worker::ensure_worker;
use serde_json::json;
use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::AppHandle;
use tauri::Emitter;
use tauri::Manager;

pub(super) fn snapshot_link_dump_server_status(state: &AppState) -> LinkDumpServerStatus {
    state
        .link_dump_server
        .lock()
        .map(|runtime| runtime.status.clone())
        .unwrap_or_default()
}

pub(super) fn emit_link_dump_server_status(app: &AppHandle, state: &AppState) {
    let _ = app.emit(
        crate::events::LINK_DUMP_SERVER_STATUS,
        snapshot_link_dump_server_status(state),
    );
}

pub(super) fn restart_link_dump_server_internal(
    app: &AppHandle,
    state: &AppState,
) -> Result<LinkDumpServerStatus, String> {
    stop_link_dump_server(state);
    start_link_dump_server_from_settings(app, state)
}

pub(super) fn stop_link_dump_server(state: &AppState) {
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

pub(super) fn set_link_dump_server_status(
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

pub(super) fn start_link_dump_server_from_settings(
    app: &AppHandle,
    state: &AppState,
) -> Result<LinkDumpServerStatus, String> {
    let mut settings = get_link_dump_settings(&state.db)?;
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
    let active_connections = Arc::new(AtomicUsize::new(0));
    let app_handle = app.clone();
    let url_for_thread = url.clone();
    let handle = thread::spawn(move || {
        println!("Server started on {bind_addr}");
        while !shutdown_thread.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    if active_connections.fetch_add(1, Ordering::SeqCst)
                        >= LINK_DUMP_MAX_CONNECTIONS
                    {
                        active_connections.fetch_sub(1, Ordering::SeqCst);
                        let _ = write_json_response(
                            &mut stream,
                            503,
                            &json!({ "ok": false, "error": "Server busy; try again shortly" }),
                        );
                        continue;
                    }
                    let request_app = app_handle.clone();
                    let active_connections = active_connections.clone();
                    thread::spawn(move || {
                        let _permit = ActiveConnectionPermit(active_connections);
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

pub(super) fn handle_link_dump_stream(mut stream: TcpStream, app: AppHandle) -> Result<(), String> {
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
        "/addVideoLinkToQueue/" | "/addVideoLinkToQueue" => {
            handle_add_video_link(&app, state.inner(), &mut stream, &request.body)
        }
        "/addVideoLinksToQueue/" | "/addVideoLinksToQueue" => {
            handle_add_video_links(&app, state.inner(), &mut stream, &request.body)
        }
        _ => write_json_response(
            &mut stream,
            404,
            &json!({ "ok": false, "error": "Not found" }),
        ),
    }
}

pub(super) fn is_link_dump_endpoint(path: &str) -> bool {
    matches!(
        path,
        "/addVideoLinkToQueue/"
            | "/addVideoLinkToQueue"
            | "/addVideoLinksToQueue/"
            | "/addVideoLinksToQueue"
    )
}

pub(super) fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
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

pub(super) fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

pub(super) fn write_options_response(stream: &mut TcpStream) -> Result<(), String> {
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

pub(super) fn write_json_response(
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
        503 => "Service Unavailable",
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
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|e| format!("HTTP response write failed: {e}"))
}

pub(super) fn handle_add_video_link(
    app: &AppHandle,
    state: &AppState,
    stream: &mut TcpStream,
    body: &[u8],
) -> Result<(), String> {
    let parsed = serde_json::from_slice::<AddVideoLinkRequestBody>(body);
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

    let Some(_valid_secret) = validate_link_dump_secret(&state.db, parsed.secret.as_deref())?
    else {
        println!("Link Dump request rejected");
        return write_json_response(
            stream,
            401,
            &json!({ "ok": false, "error": "Unauthorized" }),
        );
    };

    let Some(url) = parsed.url.as_deref().and_then(normalize_video_url) else {
        return write_json_response(
            stream,
            400,
            &json!({ "ok": false, "error": "Invalid video URL" }),
        );
    };

    let mut summary = LinkDumpQueueSummary {
        received: 1,
        added: 0,
        skipped: 0,
        invalid: 0,
    };
    if add_normalized_video_urls_to_queue(app, state, &[url], &mut summary).is_err() {
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
            "message": format!("Added {} video link{} to queue.", summary.added, if summary.added == 1 { "" } else { "s" }),
        }),
    )
}

pub(super) fn handle_add_video_links(
    app: &AppHandle,
    state: &AppState,
    stream: &mut TcpStream,
    body: &[u8],
) -> Result<(), String> {
    let parsed = serde_json::from_slice::<AddVideoLinksRequestBody>(body);
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

    let Some(_valid_secret) = validate_link_dump_secret(&state.db, parsed.secret.as_deref())?
    else {
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
            &json!({ "ok": false, "error": "No valid video URLs" }),
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
        let Some(normalized) = normalize_video_url(raw_url) else {
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
            &json!({ "ok": false, "error": "No valid video URLs" }),
        );
    }

    if add_normalized_video_urls_to_queue(app, state, &normalized_urls, &mut summary).is_err() {
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
            "message": format!("Added {} video link{} to queue.", summary.added, if summary.added == 1 { "" } else { "s" }),
        }),
    )
}

pub(super) fn add_normalized_video_urls_to_queue(
    app: &AppHandle,
    state: &AppState,
    normalized_urls: &[NormalizedVideoUrl],
    summary: &mut LinkDumpQueueSummary,
) -> Result<(), String> {
    let mut candidates = Vec::with_capacity(normalized_urls.len());
    for normalized in normalized_urls {
        let request = build_link_dump_download_request(&state.config, normalized)?;
        candidates.push((
            normalized.key.clone(),
            build_download_job(&state.config, request)?,
        ));
    }

    let added_count = {
        let mut queue = state
            .queue
            .pending
            .lock()
            .map_err(|_| "Queue lock poisoned")?;
        let active_key = state
            .queue
            .active_video_key
            .lock()
            .map_err(|_| "Active video key lock poisoned")?;
        insert_unique_video_jobs(&mut queue, active_key.as_deref(), candidates, summary)
    };
    if added_count > 0 {
        emit_queue(app, &state.queue)?;
        if is_queue_auto_start_enabled(&state.queue)? {
            ensure_worker(app, state)?;
        }
    }
    summary.added += added_count;
    Ok(())
}

pub(super) fn build_link_dump_download_request(
    state: &crate::config::ConfigState,
    normalized: &NormalizedVideoUrl,
) -> Result<DownloadRequest, String> {
    let (preset, cut_at_timestamp_enabled) = {
        let cfg = state.lock().map_err(|_| "Config lock poisoned")?;
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
        transcribe_timestamps: preset.transcribe_timestamps,
        cut_at_timestamp_enabled,
        cut_start_time: None,
        filename_suffix: preset.filename_suffix.map(str::to_string),
        title: None,
        uploader: None,
        thumbnail: normalized.thumbnail.clone(),
        upload_date: None,
        timestamp: None,
        duration_seconds: None,
    })
}

const LINK_DUMP_MAX_BATCH_SIZE: usize = 500;
const LINK_DUMP_MAX_BODY_BYTES: usize = 1024 * 1024;
const LINK_DUMP_MAX_CONNECTIONS: usize = 8;

pub(crate) struct ActiveConnectionPermit(pub(crate) Arc<AtomicUsize>);

impl Drop for ActiveConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
