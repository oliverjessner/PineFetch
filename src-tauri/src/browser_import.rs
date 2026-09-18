use super::*;

pub(super) fn get_link_dump_settings_from_conn(
    conn: &Connection,
) -> rusqlite::Result<LinkDumpSettings> {
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

pub(super) fn get_link_dump_settings(state: &AppState) -> Result<LinkDumpSettings, String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    get_link_dump_settings_from_conn(&conn)
        .map_err(|e| format!("Link Dump settings read failed: {e}"))
}

pub(super) fn update_link_dump_settings_in_db(
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

pub(super) fn list_link_dump_secrets_from_conn(
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

pub(super) fn list_link_dump_secrets(state: &AppState) -> Result<Vec<LinkDumpSecretView>, String> {
    let conn = state.db.lock().map_err(|_| "SQLite lock poisoned")?;
    list_link_dump_secrets_from_conn(&conn)
        .map_err(|e| format!("Link Dump secrets read failed: {e}"))
}

pub(super) fn next_link_dump_secret_name(conn: &Connection) -> rusqlite::Result<String> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM link_dump_secrets WHERE deleted_at IS NULL",
        [],
        |row| row.get(0),
    )?;
    Ok(format!("Link Dump Connection {}", count + 1))
}

pub(super) fn generate_link_dump_secret_value() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("Secret generation failed: {e}"))?;
    Ok(format!("pfld_{}", URL_SAFE_NO_PAD.encode(bytes)))
}

pub(super) fn hash_link_dump_secret(secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    bytes_to_hex(&hasher.finalize())
}

pub(super) fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub(super) fn constant_time_eq_str(left: &str, right: &str) -> bool {
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

pub(super) fn create_link_dump_secret_in_db(
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

pub(super) fn revoke_link_dump_secret_in_db(state: &AppState, id: &str) -> Result<(), String> {
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

pub(super) fn delete_link_dump_secret_in_db(state: &AppState, id: &str) -> Result<(), String> {
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

pub(super) fn validate_link_dump_secret(
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

pub(super) fn snapshot_link_dump_server_status(state: &AppState) -> LinkDumpServerStatus {
    state
        .link_dump_server
        .lock()
        .map(|runtime| runtime.status.clone())
        .unwrap_or_default()
}

pub(super) fn emit_link_dump_server_status(app: &AppHandle, state: &AppState) {
    let _ = app.emit(
        "link-dump:server-status",
        snapshot_link_dump_server_status(state),
    );
}

#[tauri::command]
pub(super) fn get_link_dump_overview(state: State<AppState>) -> Result<LinkDumpOverview, String> {
    Ok(LinkDumpOverview {
        settings: get_link_dump_settings(state.inner())?,
        secrets: list_link_dump_secrets(state.inner())?,
        server_status: snapshot_link_dump_server_status(state.inner()),
    })
}

#[tauri::command]
pub(super) fn update_link_dump_settings(
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
pub(super) fn create_link_dump_secret(
    state: State<AppState>,
    name: Option<String>,
) -> Result<GeneratedLinkDumpSecret, String> {
    create_link_dump_secret_in_db(state.inner(), name)
}

#[tauri::command]
pub(super) fn revoke_link_dump_secret(
    state: State<AppState>,
    id: String,
) -> Result<Vec<LinkDumpSecretView>, String> {
    revoke_link_dump_secret_in_db(state.inner(), &id)?;
    list_link_dump_secrets(state.inner())
}

#[tauri::command]
pub(super) fn delete_link_dump_secret(
    state: State<AppState>,
    id: String,
) -> Result<Vec<LinkDumpSecretView>, String> {
    delete_link_dump_secret_in_db(state.inner(), &id)?;
    list_link_dump_secrets(state.inner())
}

#[tauri::command]
pub(super) fn restart_link_dump_server(
    app: AppHandle,
    state: State<AppState>,
) -> Result<LinkDumpServerStatus, String> {
    restart_link_dump_server_internal(&app, state.inner())
}

pub(super) fn normalize_link_dump_host(host: &str) -> String {
    if host.trim() == "127.0.1" {
        return LINK_DUMP_DEFAULT_HOST.to_string();
    }
    host.trim().to_string()
}

pub(super) fn is_allowed_link_dump_host(host: &str) -> bool {
    let normalized = normalize_link_dump_host(host);
    if normalized.eq_ignore_ascii_case("localhost") {
        return true;
    }
    normalized
        .parse::<std::net::IpAddr>()
        .map(|addr| addr.is_loopback())
        .unwrap_or(false)
}

pub(super) fn link_dump_server_url(settings: &LinkDumpSettings) -> String {
    format!("http://{}:{}", settings.host, settings.port)
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
        body.as_bytes().len(),
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

    let Some(_valid_secret) = validate_link_dump_secret(state, parsed.secret.as_deref())? else {
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
        let request = build_link_dump_download_request(state, normalized)?;
        candidates.push((normalized.key.clone(), build_download_job(state, request)?));
    }

    let added_count = {
        let mut queue = state.queue.lock().map_err(|_| "Queue lock poisoned")?;
        let active_key = state
            .active_video_key
            .lock()
            .map_err(|_| "Active video key lock poisoned")?;
        insert_unique_video_jobs(&mut queue, active_key.as_deref(), candidates, summary)
    };
    if added_count > 0 {
        emit_queue(app, state)?;
        if is_queue_auto_start_enabled(state)? {
            ensure_worker(app, state)?;
        }
    }
    summary.added += added_count;
    Ok(())
}

pub(super) fn insert_unique_video_jobs(
    queue: &mut VecDeque<DownloadJob>,
    active_key: Option<&str>,
    candidates: Vec<(String, DownloadJob)>,
    summary: &mut LinkDumpQueueSummary,
) -> usize {
    let mut queued_keys: HashSet<String> = queue
        .iter()
        .filter_map(|job| normalize_video_url(&job.url).map(|normalized| normalized.key))
        .collect();
    if let Some(active_key) = active_key {
        queued_keys.insert(active_key.to_string());
    }
    let mut added = 0;
    for (key, job) in candidates {
        if queued_keys.insert(key) {
            queue.push_back(job);
            added += 1;
        } else {
            summary.skipped += 1;
        }
    }
    added
}

pub(super) fn build_link_dump_download_request(
    state: &AppState,
    normalized: &NormalizedVideoUrl,
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

pub(super) fn normalize_video_url(input: &str) -> Option<NormalizedVideoUrl> {
    normalize_youtube_url(input)
        .or_else(|| normalize_tiktok_url(input))
        .or_else(|| normalize_instagram_url(input))
        .or_else(|| normalize_facebook_url(input))
        .or_else(|| normalize_x_url(input))
        .or_else(|| normalize_reddit_url(input))
}

pub(super) fn normalize_youtube_url(input: &str) -> Option<NormalizedVideoUrl> {
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

    Some(NormalizedVideoUrl {
        url: format!("https://www.youtube.com/watch?v={video_id}"),
        key: format!("youtube:{video_id}"),
        thumbnail: Some(format!("https://i.ytimg.com/vi/{video_id}/mqdefault.jpg")),
    })
}

pub(super) fn is_plausible_youtube_video_id(video_id: &str) -> bool {
    let len = video_id.len();
    (6..=64).contains(&len)
        && video_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

pub(super) fn normalize_tiktok_url(input: &str) -> Option<NormalizedVideoUrl> {
    let parsed = parse_http_url(input)?;
    let host = normalized_url_host(&parsed)?;
    if host != "tiktok.com" && !host.ends_with(".tiktok.com") {
        return None;
    }

    let path_parts = parsed
        .path_segments()
        .map(|segments| segments.filter(|part| !part.is_empty()).collect::<Vec<_>>())
        .unwrap_or_default();

    if path_parts.len() >= 3
        && path_parts[0].starts_with('@')
        && path_parts[1].eq_ignore_ascii_case("video")
    {
        let handle = path_parts[0].strip_prefix('@')?;
        let video_id = path_parts[2];
        if is_plausible_tiktok_handle(handle) && is_plausible_numeric_id(video_id) {
            return Some(NormalizedVideoUrl {
                url: format!("https://www.tiktok.com/@{handle}/video/{video_id}"),
                key: format!("tiktok:{video_id}"),
                thumbnail: None,
            });
        }
    }

    let short_code = if matches!(host.as_str(), "vm.tiktok.com" | "vt.tiktok.com") {
        path_parts.first().copied()
    } else if path_parts
        .first()
        .is_some_and(|part| part.eq_ignore_ascii_case("t"))
    {
        path_parts.get(1).copied()
    } else {
        None
    }?;

    if !is_plausible_content_code(short_code) {
        return None;
    }

    let url = if matches!(host.as_str(), "vm.tiktok.com" | "vt.tiktok.com") {
        format!("https://{host}/{short_code}/")
    } else {
        format!("https://www.tiktok.com/t/{short_code}/")
    };
    Some(NormalizedVideoUrl {
        url,
        key: format!("tiktok-short:{short_code}"),
        thumbnail: None,
    })
}

pub(super) fn normalize_instagram_url(input: &str) -> Option<NormalizedVideoUrl> {
    let parsed = parse_http_url(input)?;
    let host = normalized_url_host(&parsed)?;
    let is_instagram_host = host == "instagram.com"
        || host.ends_with(".instagram.com")
        || host == "instagr.am"
        || host.ends_with(".instagr.am");
    if !is_instagram_host {
        return None;
    }

    let path_parts = parsed
        .path_segments()
        .map(|segments| segments.filter(|part| !part.is_empty()).collect::<Vec<_>>())
        .unwrap_or_default();
    let (route, content_code) = match path_parts.as_slice() {
        [route, content_code, ..] if is_instagram_content_route(route) => {
            ((*route).to_ascii_lowercase(), *content_code)
        }
        [_, route, content_code, ..] if is_instagram_content_route(route) => {
            ((*route).to_ascii_lowercase(), *content_code)
        }
        _ => return None,
    };
    if !is_plausible_content_code(content_code) {
        return None;
    }

    Some(NormalizedVideoUrl {
        url: format!("https://www.instagram.com/{route}/{content_code}/"),
        key: format!("instagram:{content_code}"),
        thumbnail: None,
    })
}

pub(super) fn normalize_facebook_url(input: &str) -> Option<NormalizedVideoUrl> {
    let parsed = parse_http_url(input)?;
    let host = normalized_url_host(&parsed)?;
    let parts = parsed
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if host == "fb.watch" {
        let code = *parts.first()?;
        if !is_plausible_content_code(code) {
            return None;
        }
        return Some(NormalizedVideoUrl {
            url: format!("https://fb.watch/{code}/"),
            key: format!("facebook-short:{code}"),
            thumbnail: None,
        });
    }
    if host != "facebook.com" && !host.ends_with(".facebook.com") {
        return None;
    }

    let video_id = if parts
        .first()
        .is_some_and(|part| matches!(part.to_ascii_lowercase().as_str(), "watch" | "video.php"))
    {
        parsed
            .query_pairs()
            .find_map(|(name, value)| (name == "v").then(|| value.into_owned()))
    } else if parts
        .first()
        .is_some_and(|part| part.eq_ignore_ascii_case("reel"))
    {
        parts.get(1).map(|part| (*part).to_string())
    } else {
        parts
            .windows(2)
            .find(|pair| pair[0].eq_ignore_ascii_case("videos"))
            .map(|pair| pair[1].to_string())
    };
    if let Some(video_id) = video_id.filter(|id| is_plausible_numeric_id(id)) {
        return Some(NormalizedVideoUrl {
            url: format!("https://www.facebook.com/watch/?v={video_id}"),
            key: format!("facebook:{video_id}"),
            thumbnail: None,
        });
    }

    let post = match parts.as_slice() {
        [account, "posts", id, ..] if !account.is_empty() => Some(format!("{account}/posts/{id}")),
        ["groups", group, "posts", id, ..] if !group.is_empty() => {
            Some(format!("groups/{group}/posts/{id}"))
        }
        _ => None,
    };
    if let Some(post) = post {
        let post_id = post.rsplit('/').next()?;
        if is_plausible_numeric_id(post_id)
            || (post_id.starts_with("pfbid") && is_plausible_content_code(post_id))
        {
            return Some(NormalizedVideoUrl {
                url: format!("https://www.facebook.com/{post}/"),
                key: format!("facebook-post:{post_id}"),
                thumbnail: None,
            });
        }
    }

    let (route, code) = match parts.as_slice() {
        ["share", route @ ("v" | "r"), code, ..] => (*route, *code),
        _ => return None,
    };
    if !is_plausible_content_code(code) {
        return None;
    }
    Some(NormalizedVideoUrl {
        url: format!("https://www.facebook.com/share/{route}/{code}/"),
        key: format!("facebook-share:{route}:{code}"),
        thumbnail: None,
    })
}

pub(super) fn normalize_x_url(input: &str) -> Option<NormalizedVideoUrl> {
    let parsed = parse_http_url(input)?;
    let host = normalized_url_host(&parsed)?;
    if !matches!(
        host.as_str(),
        "x.com"
            | "www.x.com"
            | "mobile.x.com"
            | "twitter.com"
            | "www.twitter.com"
            | "mobile.twitter.com"
    ) {
        return None;
    }
    let parts = parsed
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let (handle, route, status_id) = match parts.as_slice() {
        ["i", "web", route, status_id, ..] => ("i", *route, *status_id),
        [handle, route, status_id, ..] => (*handle, *route, *status_id),
        _ => return None,
    };
    if !route.eq_ignore_ascii_case("status")
        || !is_plausible_numeric_id(status_id)
        || (handle != "i"
            && (handle.is_empty()
                || handle.len() > 15
                || !handle
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')))
    {
        return None;
    }
    Some(NormalizedVideoUrl {
        url: format!("https://x.com/{handle}/status/{status_id}"),
        key: format!("x:{status_id}"),
        thumbnail: None,
    })
}

pub(super) fn normalize_reddit_url(input: &str) -> Option<NormalizedVideoUrl> {
    let parsed = parse_http_url(input)?;
    let host = normalized_url_host(&parsed)?;
    let parts = parsed
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let post_id = if host == "redd.it" {
        *parts.first()?
    } else if matches!(host.as_str(), "reddit.com" | "redditmedia.com")
        || host.ends_with(".reddit.com")
        || host.ends_with(".redditmedia.com")
    {
        match parts.as_slice() {
            ["comments", id, ..]
            | ["r", _, "comments", id, ..]
            | ["user", _, "comments", id, ..] => *id,
            _ => return None,
        }
    } else {
        return None;
    };
    if !(5..=16).contains(&post_id.len())
        || !post_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return None;
    }
    let post_id = post_id.to_ascii_lowercase();
    Some(NormalizedVideoUrl {
        url: format!("https://www.reddit.com/comments/{post_id}/"),
        key: format!("reddit:{post_id}"),
        thumbnail: None,
    })
}

pub(super) fn parse_http_url(input: &str) -> Option<url::Url> {
    let parsed = url::Url::parse(input.trim()).ok()?;
    matches!(parsed.scheme(), "http" | "https").then_some(parsed)
}

pub(super) fn normalized_url_host(parsed: &url::Url) -> Option<String> {
    Some(
        parsed
            .host_str()?
            .trim_end_matches('.')
            .to_ascii_lowercase(),
    )
}

pub(super) fn is_plausible_tiktok_handle(handle: &str) -> bool {
    !handle.is_empty()
        && handle.len() <= 64
        && handle
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
}

pub(super) fn is_plausible_numeric_id(value: &str) -> bool {
    (6..=32).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) fn is_plausible_content_code(value: &str) -> bool {
    (3..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(super) fn is_instagram_content_route(route: &str) -> bool {
    matches!(route.to_ascii_lowercase().as_str(), "p" | "reel" | "tv")
}
