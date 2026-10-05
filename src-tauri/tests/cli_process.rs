//! Built CLI boundary: isolated child environment and controlled local endpoint.
//! SQLite/history interpretation itself is covered by the application tests.
#[path = "../src/test_support.rs"]
#[allow(dead_code)] // This harness only needs the shared RAII temporary root.
mod test_support;

use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use test_support::TempRoot;

fn cli(root: &TempRoot, args: &[&str]) -> (ExitStatus, String, String) {
    let home = root.path("home");
    let temp = root.path("tmp");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&temp).unwrap();
    let stdout_path = root.path("cli.stdout");
    let stderr_path = root.path("cli.stderr");
    let mut child = Command::new(env!("CARGO_BIN_EXE_pinefetch"))
        .args(args)
        .current_dir(root.root())
        .env("HOME", &home)
        .env("XDG_DATA_HOME", root.path("data"))
        .env("XDG_CONFIG_HOME", root.path("config"))
        .env("XDG_CACHE_HOME", root.path("cache"))
        .env("TMPDIR", &temp)
        .env("TMP", &temp)
        .env("TEMP", &temp)
        .env_remove("PINEFETCH_FFMPEG_LOCATION")
        .env_remove("PINEFETCH_DENO_PATH")
        .env_remove("PINEFETCH_FASTER_WHISPER_PYTHON")
        .stdin(Stdio::null())
        .stdout(fs::File::create(&stdout_path).unwrap())
        .stderr(fs::File::create(&stderr_path).unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("CLI process did not finish: {args:?}");
        }
        thread::sleep(Duration::from_millis(10));
    };
    (
        status,
        fs::read_to_string(stdout_path).unwrap(),
        fs::read_to_string(stderr_path).unwrap(),
    )
}

#[test]
fn help_and_version_exit_successfully_without_opening_app_data() {
    let root = TempRoot::new("cli-help-version");
    let (status, stdout, stderr) = cli(&root, &["--help"]);
    assert!(status.success());
    assert!(stdout.contains("Usage:") && stdout.contains("queue add"));
    assert!(stderr.is_empty());
    let (status, stdout, stderr) = cli(&root, &["--version"]);
    assert!(status.success());
    assert_eq!(
        stdout.trim(),
        format!("PineFetch {}", env!("CARGO_PKG_VERSION"))
    );
    assert!(stderr.is_empty());
    assert!(!root.path("home/Library/Application Support").exists());
    assert!(!root.path("data").exists());
}

#[test]
fn invalid_arguments_exit_two_on_stderr_before_any_desktop_or_endpoint_access() {
    let root = TempRoot::new("cli-invalid");
    for (args, expected) in [
        (vec!["unknown"], "command"),
        (vec!["queue", "add"], "link"),
        (vec!["queue", "add", "--link", "https://"], "URL"),
        (
            vec!["queue", "add", "--link", "ftp://example.com/video"],
            "URL",
        ),
        (
            vec![
                "queue",
                "add",
                "--link",
                "https://example.com/video",
                "--preset",
                "unknown",
            ],
            "preset",
        ),
        (vec!["queue", "remove", "0"], "1"),
        (vec!["queue", "remove", "not-a-number"], "integer"),
        (vec!["history", "delete"], "command"),
    ] {
        let (status, stdout, stderr) = cli(&root, &args);
        assert_eq!(status.code(), Some(2), "{args:?}");
        assert!(stdout.is_empty(), "{args:?}: {stdout}");
        assert!(
            stderr.to_lowercase().contains(&expected.to_lowercase()),
            "{args:?}: {stderr}"
        );
    }
    assert!(!root.path("home/Library/Application Support").exists());
    assert!(!root.path("data").exists());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn endpoint(root: &TempRoot) -> std::path::PathBuf {
    let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let base = if cfg!(target_os = "macos") {
        root.path("home/Library/Application Support")
    } else {
        root.path("data")
    };
    base.join(config["identifier"].as_str().unwrap())
        .join("cli-endpoint.json")
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn exchange(args: &[&str], expected: &str, response: Vec<u8>) -> (ExitStatus, String, String) {
    let root = TempRoot::new("cli-loopback");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let path = endpoint(&root);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        serde_json::to_vec(
            &json!({"port":listener.local_addr().unwrap().port(),"token":"offline-test-token"}),
        )
        .unwrap(),
    )
    .unwrap();
    let expected = expected.to_string();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "CLI never connected to isolated endpoint"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("CLI accept failed: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut request = String::new();
        BufReader::new((&mut stream).take(1024 * 1024 + 1))
            .read_line(&mut request)
            .unwrap();
        let request: Value = serde_json::from_str(&request).unwrap();
        assert_eq!(request["token"], "offline-test-token");
        assert_eq!(request["command"]["command"], expected);
        // An oversized response may be rejected while the endpoint is still writing.
        if let Err(error) = stream.write_all(&response) {
            assert!(
                matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                ),
                "endpoint write failed: {error:?}"
            );
        }
    });
    let output = cli(&root, args);
    server.join().unwrap();
    let app_dir = path.parent().unwrap();
    assert_eq!(
        fs::read_dir(app_dir).unwrap().count(),
        1,
        "CLI client must not create user database or settings"
    );
    output
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn queue_and_history_read_commands_use_isolated_loopback_and_stdout() {
    for (args, command, reply) in [
        (vec!["queue", "list"], "queue_list", "No waiting downloads."),
        (
            vec!["history", "list"],
            "history_list",
            "🌲 Synthetic history entry",
        ),
        (vec!["stats"], "stats", "Total: 1 synthetic download"),
    ] {
        let response = serde_json::to_vec(&json!({"Ok":reply})).unwrap();
        let (status, stdout, stderr) = exchange(&args, command, [response, vec![b'\n']].concat());
        assert!(status.success());
        assert_eq!(stdout.trim(), reply);
        assert!(stderr.is_empty());
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn backend_errors_exit_one_on_stderr_without_becoming_cli_success() {
    let response = serde_json::to_vec(&json!({"Err":"controlled SQLite read failure"})).unwrap();
    let (status, stdout, stderr) = exchange(
        &["history", "list"],
        "history_list",
        [response, vec![b'\n']].concat(),
    );
    assert_eq!(status.code(), Some(1));
    assert!(stdout.is_empty());
    assert!(stderr.contains("SQLite"));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn malformed_incomplete_and_oversized_responses_are_controlled_cli_failures() {
    for response in [
        b"not JSON\n".to_vec(),
        b"{\"Ok\":\"incomplete\"}".to_vec(),
        [vec![b'x'; 1024 * 1024 + 1], vec![b'\n']].concat(),
    ] {
        let (status, stdout, stderr) = exchange(&["queue", "list"], "queue_list", response);
        assert_eq!(status.code(), Some(1));
        assert!(stdout.is_empty());
        assert!(stderr.to_lowercase().contains("cli message"));
    }
}
