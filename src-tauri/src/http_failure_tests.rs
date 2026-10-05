use crate::browser_import::read_http_request_from;
use crate::browser_import::read_http_request_with_timeout;
use crate::browser_import::reserve_link_dump_connection;
use crate::browser_import::serve_link_dump_request;
use crate::browser_import::LINK_DUMP_MAX_BODY_BYTES;
use crate::browser_import::LINK_DUMP_MAX_CONNECTIONS;
use crate::browser_import::LINK_DUMP_MAX_HEADER_BYTES;
use crate::download_rules::prepare_download_job;
use crate::download_rules::DownloadOptions;
use crate::models::AppConfig;
use crate::state::AppState;
use rusqlite::Connection;
use serde_json::json;
use std::io::{Cursor, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

const IO_LIMIT: Duration = Duration::from_secs(10);
const VIDEO_URL: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

fn parse(raw: &[u8]) -> Result<crate::models::HttpRequest, String> {
    read_http_request_from(&mut Cursor::new(raw))
}

#[test]
fn incomplete_http_body_is_rejected_instead_of_accepting_a_valid_prefix() {
    let error = parse(b"POST /addVideoLinkToQueue HTTP/1.1\r\nContent-Length: 10\r\n\r\n{}")
        .expect_err("EOF before declared Content-Length must reject the request");
    assert!(error.contains("body"));
}

#[test]
fn duplicate_content_lengths_are_rejected_even_when_values_match() {
    for headers in [
        "Content-Length: 2\r\ncontent-length: 2",
        "Content-Length: 1\r\nContent-Length: 2",
    ] {
        let request = format!("POST /addVideoLinkToQueue HTTP/1.1\r\n{headers}\r\n\r\n{{}}");
        assert!(parse(request.as_bytes()).is_err(), "accepted {headers}");
    }
}

#[test]
fn unsupported_transfer_encoding_is_rejected_without_guessing_body_framing() {
    let request =
        b"POST /addVideoLinkToQueue HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n";
    assert!(parse(request).is_err());
}

#[test]
fn malformed_http_request_lines_are_rejected_without_panicking() {
    for line in [
        "",
        "POST",
        "POST /addVideoLinkToQueue",
        "POST /addVideoLinkToQueue nonsense",
        "POST /addVideoLinkToQueue HTTP/1.1 extra",
        "POST https://example.test/ HTTP/1.1",
    ] {
        let request = format!("{line}\r\n\r\n");
        assert!(parse(request.as_bytes()).is_err(), "accepted {line:?}");
    }
}

#[test]
fn http_request_body_follows_declared_length_and_connections_do_not_pipeline() {
    let parsed = parse(
        b"POST /addVideoLinkToQueue?client=test HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}extra",
    )
    .unwrap();
    assert_eq!(parsed.method, "POST");
    assert_eq!(parsed.path, "/addVideoLinkToQueue?client=test");
    assert_eq!(parsed.body, b"{}");
    for request in [
        b"OPTIONS /addVideoLinkToQueue HTTP/1.1\r\n\r\n".as_slice(),
        b"POST /addVideoLinkToQueue HTTP/1.0\r\nContent-Length: 0\r\n\r\n",
        b"POST /addVideoLinkToQueue HTTP/1.1\r\n\r\nignored",
    ] {
        assert!(parse(request).unwrap().body.is_empty());
    }
}

#[test]
fn malformed_content_lengths_and_over_limit_bodies_are_rejected() {
    for length in ["", "-1", "one", "1.5", "184467440737095516160", "+2"] {
        let request =
            format!("POST /addVideoLinkToQueue HTTP/1.1\r\nContent-Length: {length}\r\n\r\n");
        let error = parse(request.as_bytes()).unwrap_err();
        assert!(error.contains("Content-Length"), "{length}: {error}");
    }
    let request = format!(
        "POST /addVideoLinkToQueue HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
        LINK_DUMP_MAX_BODY_BYTES + 1
    );
    assert!(parse(request.as_bytes()).unwrap_err().contains("too large"));
    let mut at_limit = format!(
        "POST /addVideoLinkToQueue HTTP/1.1\r\nContent-Length: {LINK_DUMP_MAX_BODY_BYTES}\r\n\r\n"
    )
    .into_bytes();
    at_limit.resize(at_limit.len() + LINK_DUMP_MAX_BODY_BYTES, b'x');
    assert_eq!(
        parse(&at_limit).unwrap().body.len(),
        LINK_DUMP_MAX_BODY_BYTES
    );
}

#[test]
fn headers_are_bounded_and_invalid_header_utf8_is_rejected() {
    let mut oversized = b"POST /addVideoLinkToQueue HTTP/1.1\r\nX-Test: ".to_vec();
    oversized.resize(LINK_DUMP_MAX_HEADER_BYTES + 1, b'x');
    for terminated in [false, true] {
        let mut request = oversized.clone();
        if terminated {
            request.extend_from_slice(b"\r\n\r\n");
        }
        assert!(parse(&request).unwrap_err().contains("too large"));
    }
    assert!(
        parse(b"POST /addVideoLinkToQueue HTTP/1.1\r\nX-Test: \xff\r\n\r\n")
            .unwrap_err()
            .contains("UTF-8")
    );
    // Body bytes belong to the JSON boundary, not header decoding.
    assert_eq!(
        parse(b"POST /addVideoLinkToQueue HTTP/1.1\r\nContent-Length: 1\r\n\r\n\xff")
            .unwrap()
            .body,
        b"\xff"
    );
}

#[test]
fn fragmented_http_reads_and_read_failures_are_controlled() {
    struct OneByteReader<'a>(&'a [u8]);
    impl Read for OneByteReader<'_> {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            output[0] = self.0[0];
            self.0 = &self.0[1..];
            Ok(1)
        }
    }
    let bytes = b"POST /addVideoLinkToQueue HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}";
    assert_eq!(
        read_http_request_from(&mut OneByteReader(bytes))
            .unwrap()
            .body,
        b"{}"
    );
    struct TimedOut;
    impl Read for TimedOut {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::TimedOut.into())
        }
    }
    assert!(read_http_request_from(&mut TimedOut)
        .unwrap_err()
        .contains("read failed"));
}

#[test]
fn corrupted_external_http_bytes_never_panic() {
    let valid = b"POST /addVideoLinkToQueue HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}";
    for end in 0..=valid.len() {
        assert!(std::panic::catch_unwind(|| parse(&valid[..end])).is_ok());
    }
    for offset in 0..valid.len() {
        for byte in [0, b'\n', b'\r', 0x7f, 0x80, 0xff] {
            let mut mutated = valid.to_vec();
            mutated[offset] = byte;
            assert!(std::panic::catch_unwind(|| parse(&mutated)).is_ok());
        }
    }
}

fn test_state() -> Arc<AppState> {
    let connection = Connection::open_in_memory().unwrap();
    crate::database::initialize(&connection).unwrap();
    Arc::new(AppState::new(AppConfig::default(), connection))
}

struct LoopbackServer {
    listener: TcpListener,
    state: Arc<AppState>,
    active: Arc<AtomicUsize>,
}

impl LoopbackServer {
    fn new(state: Arc<AppState>) -> Self {
        Self {
            listener: TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap(),
            state,
            active: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn connect(&self) -> (TcpStream, TcpStream) {
        let client = TcpStream::connect_timeout(&self.listener.local_addr().unwrap(), IO_LIMIT)
            .expect("loopback client failed to connect");
        client.set_read_timeout(Some(IO_LIMIT)).unwrap();
        client.set_write_timeout(Some(IO_LIMIT)).unwrap();
        let (server, _) = self
            .listener
            .accept()
            .expect("connected loopback client not accepted");
        server.set_write_timeout(Some(IO_LIMIT)).unwrap();
        (client, server)
    }

    fn exchange(&self, request: &[u8], fail_enqueue: bool) -> (u16, serde_json::Value) {
        let (mut client, mut server) = self.connect();
        let state = Arc::clone(&self.state);
        let permit = reserve_link_dump_connection(&mut server, &self.active);
        let handler = thread::spawn(move || {
            let Some(_permit) = permit else {
                return;
            };
            let _ = serve_link_dump_request(server, &state, |urls, summary| {
                if fail_enqueue {
                    return Err("controlled queue failure".into());
                }
                let config = state.config.lock().unwrap().clone();
                let candidates = urls
                    .iter()
                    .enumerate()
                    .map(|(index, url)| {
                        let request = crate::browser_import::build_link_dump_download_request(
                            &state.config,
                            url,
                        )?;
                        let job = prepare_download_job(
                            request,
                            DownloadOptions::from(&config),
                            "isolated-unused-output".into(),
                            format!("http-{index}"),
                        );
                        Ok((url.key.clone(), job))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let mut queue = state.queue.pending.lock().unwrap();
                let active = state.queue.active_video_key.lock().unwrap();
                summary.added += crate::queue::insert_unique_video_jobs(
                    &mut queue,
                    active.as_deref(),
                    candidates,
                    summary,
                );
                Ok(())
            });
        });
        client
            .write_all(request)
            .expect("loopback request write exceeded timeout");
        client.shutdown(Shutdown::Write).unwrap();
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .expect("loopback response exceeded timeout");
        handler.join().expect("HTTP handler panicked");
        let response = String::from_utf8(response).unwrap();
        let (headers, body) = response
            .split_once("\r\n\r\n")
            .expect("HTTP status response missing");
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert_eq!(body.len(), content_length);
        assert!(headers.contains("Connection: close"));
        assert!(headers.contains("Access-Control-Allow-Origin: *"));
        (
            status,
            if body.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::from_str(body).unwrap()
            },
        )
    }
}

fn request(method: &str, path: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "{method} {path} HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

#[test]
fn integration_loopback_requests_use_real_auth_normalization_and_queue_deduplication() {
    let state = test_state();
    let secret = crate::link_dump_store::create_link_dump_secret_in_db(
        &state.db,
        Some("synthetic TCP fixture".into()),
    )
    .unwrap();
    let server = LoopbackServer::new(Arc::clone(&state));
    let body = serde_json::to_vec(&json!({"secret":secret.secret,"url": VIDEO_URL})).unwrap();
    let (status, response) = server.exchange(
        &request("POST", "/addVideoLinkToQueue?client=test", &body),
        false,
    );
    assert_eq!(status, 200);
    assert_eq!(response["ok"], true);
    assert_eq!(response["added"], 1);
    let (_, repeated) = server.exchange(&request("POST", "/addVideoLinkToQueue/", &body), false);
    assert_eq!(repeated["added"], 0);
    assert_eq!(repeated["skipped"], 1);
    let batch = serde_json::to_vec(&json!({"secret":secret.secret,"urls":[VIDEO_URL, "https://youtu.be/dQw4w9WgXcQ", "https://example.test/no", "https://www.instagram.com/reel/Fixture123/"]})).unwrap();
    let (status, response) =
        server.exchange(&request("POST", "/addVideoLinksToQueue/", &batch), false);
    assert_eq!(status, 200);
    assert_eq!(response["received"], 4);
    assert_eq!(response["added"], 1);
    assert_eq!(response["skipped"], 2);
    assert_eq!(response["invalid"], 1);
    let queue = state.queue.pending.lock().unwrap();
    assert_eq!(queue.len(), 2);
    assert_eq!(queue[0].url, VIDEO_URL);
    assert_eq!(queue[1].url, "https://www.instagram.com/reel/Fixture123/");
    assert_eq!(server.active.load(Ordering::SeqCst), 0);
}

#[test]
fn integration_loopback_missing_wrong_revoked_and_deleted_secrets_never_enqueue() {
    let state = test_state();
    let generated = crate::link_dump_store::create_link_dump_secret_in_db(&state.db, None).unwrap();
    let server = LoopbackServer::new(Arc::clone(&state));
    let mut secrets = vec![serde_json::Value::Null, json!("pfld_wrong")];
    crate::link_dump_store::revoke_link_dump_secret_in_db(&state.db, &generated.connection.id)
        .unwrap();
    secrets.push(json!(generated.secret));
    let deleted = crate::link_dump_store::create_link_dump_secret_in_db(&state.db, None).unwrap();
    crate::link_dump_store::delete_link_dump_secret_in_db(&state.db, &deleted.connection.id)
        .unwrap();
    secrets.push(json!(deleted.secret));
    for secret in secrets {
        for (path, mut body) in [
            (
                "/addVideoLinkToQueue",
                json!({"secret":secret,"url":VIDEO_URL}),
            ),
            (
                "/addVideoLinksToQueue",
                json!({"secret":secret,"urls":[VIDEO_URL]}),
            ),
        ] {
            if secret.is_null() {
                body.as_object_mut().unwrap().remove("secret");
            }
            let (status, response) = server.exchange(
                &request("POST", path, &serde_json::to_vec(&body).unwrap()),
                false,
            );
            assert_eq!(status, 401);
            assert_eq!(response["ok"], false);
            assert!(state.queue.pending.lock().unwrap().is_empty());
        }
    }
}

#[test]
fn integration_loopback_malformed_requests_have_controlled_status_and_leave_queue_unchanged() {
    let state = test_state();
    let generated = crate::link_dump_store::create_link_dump_secret_in_db(&state.db, None).unwrap();
    let server = LoopbackServer::new(Arc::clone(&state));
    let valid = serde_json::to_vec(&json!({"secret":generated.secret,"url": VIDEO_URL})).unwrap();
    let invalid_url =
        serde_json::to_vec(&json!({"secret":generated.secret,"url":"not a video URL"})).unwrap();
    let empty_batch = serde_json::to_vec(&json!({"secret":generated.secret,"urls":[]})).unwrap();
    let mut incomplete_valid_body = format!(
        "POST /addVideoLinkToQueue HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
        valid.len() + 1,
    )
    .into_bytes();
    incomplete_valid_body.extend_from_slice(&valid);
    for (bytes, expected) in [
        (request("POST", "/addVideoLinkToQueue", b"{invalid"), 400),
        (request("POST", "/addVideoLinksToQueue", b"\xff"), 400),
        (request("POST", "/addVideoLinkToQueue", &invalid_url), 400),
        (request("POST", "/addVideoLinksToQueue", &empty_batch), 400),
        (request("GET", "/addVideoLinkToQueue", &[]), 405),
        (request("POST", "/unknown", &valid), 404),
        (request("OPTIONS", "/unknown", &[]), 404),
        (incomplete_valid_body, 400),
        (
            format!(
                "POST /addVideoLinkToQueue HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
                LINK_DUMP_MAX_BODY_BYTES + 1
            )
            .into_bytes(),
            400,
        ),
        (
            b"POST /addVideoLinkToQueue HTTP/1.1\r\nContent-Length: 10\r\n\r\n{}".to_vec(),
            400,
        ),
    ] {
        let (status, response) = server.exchange(&bytes, false);
        assert_eq!(status, expected);
        assert_eq!(response["ok"], false);
        assert!(state.queue.pending.lock().unwrap().is_empty());
    }
    let (status, response) =
        server.exchange(&request("OPTIONS", "/addVideoLinkToQueue", &[]), false);
    assert_eq!(status, 204);
    assert_eq!(response, serde_json::Value::Null);
    let (status, response) =
        server.exchange(&request("POST", "/addVideoLinkToQueue", &valid), true);
    assert_eq!(status, 500);
    assert_eq!(response["ok"], false);
    assert!(state.queue.pending.lock().unwrap().is_empty());
}

#[test]
fn integration_loopback_connection_limit_rejects_excess_and_recovers_after_permit_release() {
    let server = LoopbackServer::new(test_state());
    let mut held = Vec::new();
    for _ in 0..LINK_DUMP_MAX_CONNECTIONS {
        let (client, mut stream) = server.connect();
        let permit = reserve_link_dump_connection(&mut stream, &server.active).unwrap();
        held.push((client, stream, permit));
    }
    assert_eq!(
        server.active.load(Ordering::SeqCst),
        LINK_DUMP_MAX_CONNECTIONS
    );
    let (status, response) = server.exchange(&[], false);
    assert_eq!(status, 503);
    assert_eq!(response["ok"], false);
    assert_eq!(
        server.active.load(Ordering::SeqCst),
        LINK_DUMP_MAX_CONNECTIONS
    );
    drop(held);
    assert_eq!(server.active.load(Ordering::SeqCst), 0);
    let (status, _) = server.exchange(&request("OPTIONS", "/addVideoLinkToQueue", &[]), false);
    assert_eq!(status, 204);
    assert_eq!(server.active.load(Ordering::SeqCst), 0);
}

#[test]
fn integration_loopback_auth_database_failure_returns_controlled_error_and_allows_retry() {
    let state = test_state();
    let generated = crate::link_dump_store::create_link_dump_secret_in_db(&state.db, None).unwrap();
    let server = LoopbackServer::new(Arc::clone(&state));
    state
        .db
        .lock()
        .unwrap()
        .execute_batch("ALTER TABLE link_dump_secrets RENAME TO temporarily_unavailable_secrets")
        .unwrap();
    for (path, body) in [
        (
            "/addVideoLinkToQueue",
            json!({"secret":generated.secret,"url":VIDEO_URL}),
        ),
        (
            "/addVideoLinksToQueue",
            json!({"secret":generated.secret,"urls":[VIDEO_URL]}),
        ),
    ] {
        let (status, response) = server.exchange(
            &request("POST", path, &serde_json::to_vec(&body).unwrap()),
            false,
        );
        assert_eq!(status, 500);
        assert_eq!(response["ok"], false);
        assert!(state.queue.pending.lock().unwrap().is_empty());
    }
    state
        .db
        .lock()
        .unwrap()
        .execute_batch("ALTER TABLE temporarily_unavailable_secrets RENAME TO link_dump_secrets")
        .unwrap();
    let body = serde_json::to_vec(&json!({"secret":generated.secret,"url":VIDEO_URL})).unwrap();
    let (status, response) =
        server.exchange(&request("POST", "/addVideoLinkToQueue", &body), false);
    assert_eq!(status, 200);
    assert_eq!(response["added"], 1);
}

#[test]
fn integration_slow_loopback_client_hits_read_timeout_without_requiring_eof() {
    let server = LoopbackServer::new(test_state());
    let (mut client, mut stream) = server.connect();
    client
        .write_all(b"POST /addVideoLinkToQueue HTTP/1.1\r\nContent-Length: 2\r\n\r\n{")
        .unwrap();
    // Keep the write half open: only the socket deadline can end the body read.
    let (result_tx, result_rx) = mpsc::channel();
    let handler = thread::spawn(move || {
        let result = read_http_request_with_timeout(&mut stream, Duration::from_millis(500));
        result_tx.send(result).unwrap();
    });
    let error = result_rx
        .recv_timeout(IO_LIMIT)
        .expect("slow-client HTTP body read did not honor its deadline")
        .expect_err("an incomplete body cannot succeed without EOF or a read timeout");
    assert!(error.contains("body read failed"), "{error}");
    handler.join().unwrap();
    drop(client);
}

#[cfg(unix)]
#[test]
fn integration_nonblocking_listener_accepts_fragmented_http_request_without_premature_error() {
    use std::os::fd::AsRawFd;

    let server = LoopbackServer::new(test_state());
    server.listener.set_nonblocking(true).unwrap();
    let mut client =
        TcpStream::connect_timeout(&server.listener.local_addr().unwrap(), IO_LIMIT).unwrap();
    client.set_write_timeout(Some(IO_LIMIT)).unwrap();
    let mut ready = libc::pollfd {
        fd: server.listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let deadline = std::time::Instant::now() + IO_LIMIT;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "nonblocking HTTP listener never became readable"
        );
        let count = unsafe { libc::poll(&mut ready, 1, remaining.as_millis() as i32) };
        if count > 0 {
            break;
        }
        if count == 0 {
            panic!("nonblocking HTTP listener accept readiness exceeded deadline");
        }
        let error = std::io::Error::last_os_error();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted, "{error}");
    }
    let (mut accepted, _) = server.listener.accept().unwrap();
    // macOS inherits O_NONBLOCK from accept(); other Unix systems need this
    // explicit setup to exercise the same socket boundary.
    #[cfg(not(target_os = "macos"))]
    accepted.set_nonblocking(true).unwrap();
    let before = unsafe { libc::fcntl(accepted.as_raw_fd(), libc::F_GETFL) };
    assert!(before >= 0);
    assert_ne!(before & libc::O_NONBLOCK, 0);

    client
        .write_all(b"POST /addVideoLinkToQueue HTTP/1.1\r\nContent-Length: 2\r\n\r\n{")
        .unwrap();
    crate::browser_import::configure_http_stream(&mut accepted, IO_LIMIT).unwrap();
    let after = unsafe { libc::fcntl(accepted.as_raw_fd(), libc::F_GETFL) };
    assert!(after >= 0);
    assert_eq!(
        after & libc::O_NONBLOCK,
        0,
        "an accepted Link Dump socket must block until the next request fragment or its deadline",
    );

    struct ObservedRead {
        stream: TcpStream,
        first_fragment: Option<mpsc::Sender<()>>,
    }
    impl Read for ObservedRead {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            let count = self.stream.read(output)?;
            if count > 0 {
                if let Some(signal) = self.first_fragment.take() {
                    signal.send(()).unwrap();
                }
            }
            Ok(count)
        }
    }

    let (fragment_tx, fragment_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let handler = thread::spawn(move || {
        let mut reader = ObservedRead {
            stream: accepted,
            first_fragment: Some(fragment_tx),
        };
        result_tx.send(read_http_request_from(&mut reader)).unwrap();
    });
    fragment_rx
        .recv_timeout(IO_LIMIT)
        .expect("HTTP parser did not consume the first request fragment");
    client.write_all(b"}").unwrap();
    let parsed = result_rx
        .recv_timeout(IO_LIMIT)
        .expect("HTTP parser did not finish after the last request fragment")
        .expect("a valid fragmented request must not fail with WouldBlock");
    assert_eq!(parsed.body, b"{}");
    assert_eq!(parsed.method, "POST");
    handler.join().unwrap();
}
