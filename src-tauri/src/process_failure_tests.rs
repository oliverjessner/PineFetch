//! Real local process and filesystem boundaries; no platform runtime or downloads.
use crate::files::{publish_unique_output, OwnedTemporaryFile};
use crate::process::{
    run_command_output, terminate_child_process_tree, ProcessError, ProcessState,
};
use crate::test_support::{FakeProcess, TempRoot, TEST_TIMEOUT};
use crate::worker::request_active_job_cancellation;
use crate::worker::stop_active_download_on_exit;
use crate::yt_dlp::{parse_caption_line, parse_download_metadata_line, parse_info_json};
use std::fs;
use std::process::{Command, Output};
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

fn start_runner(
    command: Command,
    state: Arc<ProcessState>,
) -> (
    mpsc::Receiver<Result<Output, ProcessError>>,
    thread::JoinHandle<()>,
) {
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        let result = run_command_output(command, Some(&state), None, Some(TEST_TIMEOUT));
        let _ = sender.send(result);
    });
    (receiver, handle)
}

fn runner_finished(
    receiver: mpsc::Receiver<Result<Output, ProcessError>>,
    handle: thread::JoinHandle<()>,
) -> Output {
    let output = receiver
        .recv_timeout(TEST_TIMEOUT + Duration::from_secs(2))
        .expect("process runner did not finish after controlled signal")
        .expect("process runner failed");
    handle.join().expect("process runner thread panicked");
    output
}

fn registered_child(state: &ProcessState) -> Arc<std::sync::Mutex<std::process::Child>> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    loop {
        if let Some(child) = state.current_child.lock().unwrap().clone() {
            return child;
        }
        assert!(
            Instant::now() < deadline,
            "running child was not registered"
        );
        thread::yield_now();
    }
}

#[test]
fn integration_runner_reads_stdout_and_stderr_and_preserves_exit_code() {
    let fake = FakeProcess::new("exit-error");
    let output = run_command_output(fake.command(), None, None, Some(TEST_TIMEOUT)).unwrap();
    assert_eq!(output.status.code(), Some(23));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("controlled external process failure"));
}

#[test]
fn integration_runner_accepts_success_without_a_local_media_runtime() {
    let root = TempRoot::new("runner-success");
    let path = root.path("Grüße 🌲 with spaces.mp4");
    let fake = FakeProcess::new("success");
    let mut command = fake.command();
    command.arg("--output").arg(&path);
    let output = run_command_output(command, None, None, Some(TEST_TIMEOUT)).unwrap();
    assert!(output.status.success());
    assert_eq!(fs::read(&path).unwrap(), b"PineFetch synthetic output\n");
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("pinefetch_metadata:"));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("fixture diagnostic"));
}

#[test]
fn integration_runner_drains_both_large_output_streams_without_deadlock() {
    let fake = FakeProcess::new("huge-output");
    let output = run_command_output(fake.command(), None, None, Some(TEST_TIMEOUT)).unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), 128 * 8192);
    assert_eq!(output.stderr.len(), 128 * 8192);
    assert!(output.stdout.iter().all(|byte| *byte == b'o'));
    assert!(output.stderr.iter().all(|byte| *byte == b'e'));
}

#[test]
fn integration_runner_retains_unterminated_output_from_a_failed_process() {
    let fake = FakeProcess::new("partial-output");
    let output = run_command_output(fake.command(), None, None, Some(TEST_TIMEOUT)).unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert!(output.stdout.ends_with(b"ETA 00:13"));
    assert_eq!(output.stderr, b"ERROR: interrupted");
}

#[test]
fn integration_malformed_process_output_is_delivered_without_becoming_metadata() {
    let fake = FakeProcess::new("malformed-output");
    let output = run_command_output(fake.command(), None, None, Some(TEST_TIMEOUT)).unwrap();
    assert!(output.status.success());
    assert!(std::str::from_utf8(&output.stdout).is_err());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(parse_info_json(&text).is_err());
    assert!(text
        .lines()
        .all(|line| parse_download_metadata_line(line).is_none()));
}

#[test]
fn integration_runner_waits_for_a_controlled_process_to_finish_output() {
    let fake = FakeProcess::new("slow-output");
    let (command, control) = fake.controlled_command();
    let state = Arc::new(ProcessState::default());
    let (receiver, handle) = start_runner(command, state.clone());
    let ready = control.wait_ready();
    assert_eq!(ready.child_pid, 0);
    assert_eq!(registered_child(&state).lock().unwrap().id(), ready.pid);
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    ready.release();
    let output = runner_finished(receiver, handle);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("10.0%"));
    assert!(text.contains("100.0%"));
    assert!(state.current_child.lock().unwrap().is_none());
}

#[test]
fn integration_missing_executable_returns_a_spawn_error() {
    let root = TempRoot::new("missing-executable");
    let command = Command::new(root.path("absent yt-dlp"));
    let error = run_command_output(command, None, None, Some(TEST_TIMEOUT)).unwrap_err();
    assert!(matches!(error, ProcessError::Spawn(_)));
}

#[test]
fn integration_hanging_process_is_timed_out_reaped_and_unregistered() {
    let fake = FakeProcess::new("hang");
    let (command, control) = fake.controlled_command();
    let state = Arc::new(ProcessState::default());
    let worker_state = state.clone();
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        let _ = sender.send(run_command_output(
            command,
            Some(&worker_state),
            None,
            Some(Duration::from_secs(1)),
        ));
    });
    let _ready = control.wait_ready();
    let child = registered_child(&state);
    let result = receiver.recv_timeout(TEST_TIMEOUT).unwrap();
    handle.join().unwrap();
    assert!(matches!(result, Err(ProcessError::TimedOut)));
    assert!(child.lock().unwrap().try_wait().unwrap().is_some());
    assert!(state.current_child.lock().unwrap().is_none());
}

#[test]
fn integration_cancellation_reaps_the_child_and_remains_idempotent_after_exit() {
    let fake = FakeProcess::new("hang");
    let (command, control) = fake.controlled_command();
    let state = Arc::new(ProcessState::default());
    *state.current_job_id.lock().unwrap() = Some("cancel-me".into());
    let (receiver, handle) = start_runner(command, state.clone());
    let _ready = control.wait_ready();
    let child = registered_child(&state);
    request_active_job_cancellation(&state, "cancel-me").unwrap();
    let output = runner_finished(receiver, handle);
    assert!(!output.status.success());
    assert_eq!(
        state.cancel_requested.lock().unwrap().as_deref(),
        Some("cancel-me")
    );
    assert!(state.current_child.lock().unwrap().is_none());
    request_active_job_cancellation(&state, "cancel-me").unwrap();
    request_active_job_cancellation(&state, "cancel-me").unwrap();
    let mut child = child.lock().unwrap();
    assert!(child.try_wait().unwrap().is_some());
    terminate_child_process_tree(&mut child).unwrap();
    terminate_child_process_tree(&mut child).unwrap();
}

#[test]
fn integration_cancel_requested_before_registration_stops_the_new_child() {
    let fake = FakeProcess::new("hang");
    let state = ProcessState::default();
    *state.current_job_id.lock().unwrap() = Some("already-cancelled".into());
    *state.cancel_requested.lock().unwrap() = Some("already-cancelled".into());
    let output =
        run_command_output(fake.command(), Some(&state), None, Some(TEST_TIMEOUT)).unwrap();
    assert!(!output.status.success());
    assert!(state.current_child.lock().unwrap().is_none());
}

#[test]
fn integration_shutdown_rejects_utility_work_and_cleans_up_the_spawned_process() {
    let fake = FakeProcess::new("hang");
    let state = ProcessState::default();
    state.shutting_down.store(true, Ordering::SeqCst);
    let error =
        run_command_output(fake.command(), None, Some(&state), Some(TEST_TIMEOUT)).unwrap_err();
    assert!(matches!(error, ProcessError::State(ref cause) if cause.contains("shutting down")));
    assert!(state.utility_children.lock().unwrap().is_empty());
}

#[test]
fn integration_shutdown_during_cancellation_reaps_the_active_child_and_pauses_queue() {
    let fake = FakeProcess::new("hang");
    let (command, control) = fake.controlled_command();
    let state = Arc::new(crate::state::AppState::new(
        crate::models::AppConfig::default(),
        rusqlite::Connection::open_in_memory().unwrap(),
    ));
    *state.processes.current_job_id.lock().unwrap() = Some("shutdown-job".into());
    let worker_state = state.clone();
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        let _ = sender.send(run_command_output(
            command,
            Some(&worker_state.processes),
            None,
            Some(TEST_TIMEOUT),
        ));
    });
    let _ready = control.wait_ready();
    let child = registered_child(&state.processes);
    request_active_job_cancellation(&state.processes, "shutdown-job").unwrap();
    stop_active_download_on_exit(&state);
    let output = receiver.recv_timeout(TEST_TIMEOUT).unwrap().unwrap();
    handle.join().unwrap();
    assert!(!output.status.success());
    assert!(state.processes.shutting_down.load(Ordering::SeqCst));
    assert!(*state.queue.paused.lock().unwrap());
    assert_eq!(
        state.processes.cancel_requested.lock().unwrap().as_deref(),
        Some("shutdown-job")
    );
    assert!(child.lock().unwrap().try_wait().unwrap().is_some());
    assert!(state.processes.current_child.lock().unwrap().is_none());
}

#[cfg(unix)]
#[test]
fn integration_cancellation_terminates_a_confirmed_descendant_process() {
    let fake = FakeProcess::new("spawn-child");
    let (command, control) = fake.controlled_command();
    let state = Arc::new(ProcessState::default());
    *state.current_job_id.lock().unwrap() = Some("parent-job".into());
    let (receiver, handle) = start_runner(command, state.clone());
    let ready = control.wait_ready();
    assert!(ready.child_pid > 0);
    let _cleanup = crate::test_support::DescendantCleanup::new(&ready);
    assert_eq!(
        unsafe { libc::kill(i32::try_from(ready.child_pid).unwrap(), 0) },
        0
    );
    let child = registered_child(&state);
    assert_eq!(
        unsafe { libc::getpgid(i32::try_from(ready.child_pid).unwrap()) },
        i32::try_from(ready.pid).unwrap()
    );
    request_active_job_cancellation(&state, "parent-job").unwrap();
    let output = runner_finished(receiver, handle);
    assert!(!output.status.success());
    assert!(child.lock().unwrap().try_wait().unwrap().is_some());
    crate::test_support::assert_process_stopped(ready.child_pid);
}

#[test]
fn malformed_external_data_is_rejected_without_panicking() {
    let inputs = [
        "",
        "\0",
        "\u{85}",
        "{",
        "null",
        "[]",
        "{\"filepath\":[]}",
        "{\"duration\":1e999}",
        "{\"filepath\":\"Grüße 🌲\",\"title\":null}",
    ];
    for input in inputs {
        std::panic::catch_unwind(|| {
            let _ = parse_info_json(input);
            let _ = parse_download_metadata_line(&format!("pinefetch_metadata:{input}"));
            let _ = parse_caption_line(&format!("pinefetch_caption:{input}"), true);
        })
        .expect("untrusted process JSON must not panic");
    }
}

#[test]
fn integration_output_publication_preserves_existing_files_and_directories() {
    let root = TempRoot::new("output-collisions");
    let source = root.path("reserved.tmp");
    fs::write(&source, b"new output").unwrap();
    let destination = root.path("media.mp4");
    fs::write(&destination, b"previous output").unwrap();
    fs::create_dir(root.path("media__2.mp4")).unwrap();
    let published = publish_unique_output(&source, &destination).unwrap();
    assert_eq!(published, root.path("media__3.mp4"));
    assert_eq!(fs::read(&published).unwrap(), b"new output");
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");
    assert!(root.path("media__2.mp4").is_dir());
    assert_eq!(fs::read(&source).unwrap(), b"new output");
}

#[test]
fn integration_output_publication_rejects_missing_or_non_directory_parents() {
    let root = TempRoot::new("output-parent-errors");
    let source = root.path("reserved.tmp");
    fs::write(&source, b"preserved output").unwrap();
    let not_directory = root.path("not a directory");
    fs::write(&not_directory, b"existing file").unwrap();
    for destination in [
        root.path("missing/media.mp4"),
        not_directory.join("media.mp4"),
    ] {
        let error = publish_unique_output(&source, &destination).unwrap_err();
        assert!(error.contains("output file"));
        assert_eq!(fs::read(&source).unwrap(), b"preserved output");
        assert!(!destination.exists());
    }
    assert_eq!(fs::read(&not_directory).unwrap(), b"existing file");
}

#[test]
fn integration_vanished_temporary_output_does_not_replace_an_existing_file() {
    let root = TempRoot::new("vanished-output");
    let source = root.path("reserved.tmp");
    fs::write(&source, b"temporary output").unwrap();
    let destination = root.path("media.mp4");
    fs::write(&destination, b"previous output").unwrap();
    fs::remove_file(&source).unwrap();
    assert!(publish_unique_output(&source, &destination).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");
    assert!(!root.path("media__2.mp4").exists());
}

#[cfg(unix)]
#[test]
fn integration_non_writable_output_directory_preserves_source_and_existing_files() {
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(
        unsafe { libc::geteuid() },
        0,
        "permission-boundary test requires an unprivileged Unix user"
    );
    struct RestorePermissions {
        path: std::path::PathBuf,
        original: fs::Permissions,
    }
    impl Drop for RestorePermissions {
        fn drop(&mut self) {
            if let Err(error) = fs::set_permissions(&self.path, self.original.clone()) {
                if thread::panicking() {
                    eprintln!("could not restore test directory permissions: {error}");
                } else {
                    panic!("could not restore test directory permissions: {error}");
                }
            }
        }
    }
    let root = TempRoot::new("non-writable-output");
    let source = root.path("reserved.tmp");
    fs::write(&source, b"new output preserved").unwrap();
    let output_directory = root.path("read only target");
    fs::create_dir(&output_directory).unwrap();
    let destination = output_directory.join("media.mp4");
    fs::write(&destination, b"existing output preserved").unwrap();
    let restore = RestorePermissions {
        path: output_directory.clone(),
        original: fs::metadata(&output_directory).unwrap().permissions(),
    };
    fs::set_permissions(&output_directory, fs::Permissions::from_mode(0o500)).unwrap();
    let error = publish_unique_output(&source, &destination).unwrap_err();
    assert!(error.contains("output file"));
    assert_eq!(fs::read(&source).unwrap(), b"new output preserved");
    assert_eq!(
        fs::read(&destination).unwrap(),
        b"existing output preserved"
    );
    assert!(!output_directory.join("media__2.mp4").exists());
    drop(restore);
}

#[test]
fn integration_output_publication_handles_unicode_spaces_and_long_names() {
    let root = TempRoot::new("output-names");
    let source = root.path("Grüße 🌲 reserved.tmp");
    let mut temporary = OwnedTemporaryFile::create_at(source.clone()).unwrap();
    temporary.write_all(b"synthetic bytes").unwrap();
    temporary.sync().unwrap();
    let destination = root.path(format!("Grüße 🌲 {}.mp4", "a".repeat(180)));
    let published = publish_unique_output(&source, &destination).unwrap();
    assert_eq!(published, destination);
    assert_eq!(fs::read(&published).unwrap(), b"synthetic bytes");
    drop(temporary);
    #[cfg(unix)]
    assert!(!source.exists());
    assert_eq!(fs::read(&published).unwrap(), b"synthetic bytes");
}

#[cfg(unix)]
#[test]
fn integration_publication_and_temporary_cleanup_preserve_symlink_targets() {
    use std::os::unix::fs::symlink;
    let root = TempRoot::new("symlink-output");
    let target = root.path("precious.txt");
    fs::write(&target, b"unrelated user file").unwrap();
    let source = root.path("reserved.tmp");
    let mut temporary = OwnedTemporaryFile::create_at(source.clone()).unwrap();
    temporary.write_all(b"new output").unwrap();
    let destination = root.path("media.mp4");
    symlink(&target, &destination).unwrap();
    let published = publish_unique_output(&source, &destination).unwrap();
    assert_eq!(published, root.path("media__2.mp4"));
    assert!(fs::symlink_metadata(&destination)
        .unwrap()
        .file_type()
        .is_symlink());
    fs::remove_file(&source).unwrap();
    symlink(&target, &source).unwrap();
    drop(temporary);
    assert!(fs::symlink_metadata(&source)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(OwnedTemporaryFile::create_at(source).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"unrelated user file");
    assert_eq!(fs::read(&published).unwrap(), b"new output");
}
