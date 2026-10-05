//! Small shared infrastructure for tests that cross real local boundaries.
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) const TEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct TempRoot(PathBuf);

impl TempRoot {
    pub(crate) fn new(label: &str) -> Self {
        assert!(
            Path::new(label).components().count() == 1
                && !matches!(label, "." | "..")
                && Path::new(label).is_relative(),
            "test root label must be one relative path component"
        );
        let root = std::env::temp_dir().join(format!(
            "pinefetch-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).expect("create isolated test root");
        Self(root)
    }

    pub(crate) fn root(&self) -> &Path {
        &self.0
    }

    pub(crate) fn path(&self, name: impl AsRef<Path>) -> PathBuf {
        let name = name.as_ref();
        assert!(
            name.components().all(|component| matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )),
            "test paths must stay inside their isolated root"
        );
        self.0.join(name)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            if std::thread::panicking() {
                eprintln!("test root cleanup failed ({}): {error}", self.0.display());
            } else {
                panic!("test root cleanup failed ({}): {error}", self.0.display());
            }
        }
    }
}

struct CompiledFake {
    _root: TempRoot,
    executable: PathBuf,
}

static COMPILED_FAKE: Mutex<Option<Weak<CompiledFake>>> = Mutex::new(None);

fn compiled_fake() -> Arc<CompiledFake> {
    let mut cache = COMPILED_FAKE.lock().expect("fake compiler cache lock");
    if let Some(fake) = cache.as_ref().and_then(Weak::upgrade) {
        return fake;
    }
    let root = TempRoot::new("fake-process-build");
    let executable = root.path(format!("fake-process{}", std::env::consts::EXE_SUFFIX));
    let errors = fs::File::create(root.path("rustc.stderr")).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test/fixtures/fake-process.rs");
    let mut compiler = Command::new("rustc")
        .arg("--edition=2021")
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(errors)
        .spawn()
        .expect("compile fixture with repository Rust toolchain");
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = compiler.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = compiler.kill();
            let _ = compiler.wait();
            panic!("fake-process compilation exceeded 30 seconds");
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(
        status.success(),
        "fake-process compilation failed: {}",
        fs::read_to_string(root.path("rustc.stderr")).unwrap()
    );
    let fake = Arc::new(CompiledFake {
        _root: root,
        executable,
    });
    *cache = Some(Arc::downgrade(&fake));
    fake
}

pub(crate) struct FakeProcess {
    compiled: Arc<CompiledFake>,
    scenario: String,
}

impl FakeProcess {
    pub(crate) fn new(scenario: &str) -> Self {
        Self {
            compiled: compiled_fake(),
            scenario: scenario.to_string(),
        }
    }

    pub(crate) fn command(&self) -> Command {
        let mut command = Command::new(&self.compiled.executable);
        command.arg(&self.scenario).stdin(Stdio::null());
        command
    }

    pub(crate) fn controlled_command(&self) -> (Command, ProcessControl) {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut command = self.command();
        command
            .arg("--control")
            .arg(listener.local_addr().unwrap().to_string());
        (command, ProcessControl { listener })
    }
}

pub(crate) struct ProcessControl {
    listener: TcpListener,
}

impl ProcessControl {
    pub(crate) fn wait_ready(self) -> ReadyProcess {
        let deadline = Instant::now() + TEST_TIMEOUT;
        let stream = loop {
            match self.listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "fake process did not connect");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("fake readiness connection failed: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(TEST_TIMEOUT)).unwrap();
        stream.set_write_timeout(Some(TEST_TIMEOUT)).unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("fake process readiness handshake");
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 3, "invalid fake readiness handshake: {line}");
        assert_eq!(fields[0], "READY");
        ReadyProcess {
            stream: reader.into_inner(),
            pid: fields[1].parse().unwrap(),
            child_pid: fields[2].parse().unwrap(),
        }
    }
}

pub(crate) struct ReadyProcess {
    stream: TcpStream,
    pub(crate) pid: u32,
    pub(crate) child_pid: u32,
}

impl ReadyProcess {
    pub(crate) fn release(mut self) {
        self.stream.write_all(b"G").expect("release fake process");
    }
}

impl Drop for ReadyProcess {
    fn drop(&mut self) {
        // Release blocked fixtures during test unwinding as well. Normal runner
        // tests additionally have a deadline and reap their registered child.
        let _ = self.stream.write_all(b"G");
    }
}

#[cfg(unix)]
pub(crate) struct DescendantCleanup {
    parent_pid: i32,
    child_pid: i32,
}

#[cfg(unix)]
impl DescendantCleanup {
    pub(crate) fn new(ready: &ReadyProcess) -> Self {
        assert!(ready.child_pid > 0);
        Self {
            parent_pid: ready.pid.try_into().unwrap(),
            child_pid: ready.child_pid.try_into().unwrap(),
        }
    }
}

#[cfg(unix)]
impl Drop for DescendantCleanup {
    fn drop(&mut self) {
        // Retain a group only while the confirmed descendant still belongs to
        // it. This fixture guard also cleans up when a parent intentionally
        // exits with output pipes still held by its child.
        if unsafe { libc::getpgid(self.child_pid) } == self.parent_pid {
            unsafe { libc::kill(-self.parent_pid, libc::SIGKILL) };
        }
    }
}

#[cfg(unix)]
fn process_is_running(pid: u32) -> bool {
    let pid = i32::try_from(pid).unwrap();
    if unsafe { libc::kill(pid, 0) } != 0 {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        // A grandchild is reaped by its new parent after the group leader dies.
        // A zombie is already terminated and cannot continue fixture work.
        if fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| {
                stat.rsplit_once(") ")
                    .map(|(_, tail)| tail.starts_with('Z'))
            })
            == Some(true)
        {
            return false;
        }
    }
    true
}

#[cfg(unix)]
pub(crate) fn assert_process_stopped(pid: u32) {
    let deadline = Instant::now() + TEST_TIMEOUT;
    while process_is_running(pid) {
        assert!(
            Instant::now() < deadline,
            "descendant {pid} survived process-group cancellation"
        );
        thread::yield_now();
    }
}
