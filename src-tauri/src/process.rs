use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Output;
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use std::time::Instant;

pub(crate) const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_STREAM_LINE_BYTES: usize = 8 * 1024 * 1024;

pub(crate) fn register_current_child(
    state: &ProcessState,
    child: &Arc<Mutex<Child>>,
) -> Result<(), String> {
    let mut slot = state
        .current_child
        .lock()
        .map_err(|_| "Child lock poisoned")?;
    *slot = Some(child.clone());
    // Cancel may have been requested between process creation and registration.
    let current = state
        .current_job_id
        .lock()
        .map_err(|_| "Current job lock poisoned")?;
    let cancelled = state.shutting_down.load(Ordering::SeqCst)
        || current.is_some()
            && state
                .cancel_requested
                .lock()
                .map_err(|_| "Cancel lock poisoned")?
                .as_deref()
                == current.as_deref();
    if cancelled {
        if let Ok(mut process) = child.lock() {
            let _ = terminate_child_process_tree(&mut process);
        }
    }
    Ok(())
}

pub(crate) fn register_utility_child(
    state: &ProcessState,
    child: &Arc<Mutex<Child>>,
) -> Result<(), String> {
    let mut children = state
        .utility_children
        .lock()
        .map_err(|_| "Utility child lock poisoned")?;
    if state.shutting_down.load(Ordering::SeqCst) {
        return Err("Application is shutting down".to_string());
    }
    children.push(child.clone());
    Ok(())
}

pub(crate) fn clear_utility_child(state: &ProcessState, child: &Arc<Mutex<Child>>) {
    if let Ok(mut children) = state.utility_children.lock() {
        children.retain(|active| !Arc::ptr_eq(active, child));
    }
}

pub(crate) fn clear_current_child(state: &ProcessState, child: &Arc<Mutex<Child>>) {
    if let Ok(mut slot) = state.current_child.lock() {
        if slot
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, child))
        {
            *slot = None;
        }
    }
}

pub(crate) fn configure_child_process_group(command: &mut Command) {
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(not(unix))]
    let _ = command;
}

pub(crate) fn terminate_child_process_tree(process: &mut Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        // Keep an exited leader waitable until its inherited pipes are drained.
        // Its unreaped PID still owns the group identity; after reaping, never
        // signal that numeric PGID because the operating system can reuse it.
        let Some(exited) = child_exit_without_reaping(process)? else {
            return Ok(());
        };
        let group_id = i32::try_from(process.id())
            .map_err(|_| std::io::Error::other("Child process ID is out of range"))?;
        if unsafe { libc::kill(-group_id, libc::SIGKILL) } == 0 {
            return Ok(());
        }
        let group_error = std::io::Error::last_os_error();
        if exited && group_error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        let _ = process.kill();
        Err(group_error)
    }
    #[cfg(not(unix))]
    {
        if process.try_wait()?.is_some() {
            return Ok(());
        }
        process.kill()
    }
}

#[cfg(unix)]
fn child_exit_without_reaping(process: &Child) -> std::io::Result<Option<bool>> {
    let mut status = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    loop {
        // WNOWAIT preserves the leader PID while a descendant owns an output
        // pipe. All access to this Child, including cancellation, is locked.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                process.id(),
                status.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            return Ok(Some(unsafe { (*status.as_ptr()).si_pid() != 0 }));
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ECHILD) {
            return Ok(None);
        }
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

trait StreamingPipe: Read {
    fn prepare(&self) -> std::io::Result<()>;
    fn read_available(&mut self, bytes: &mut [u8]) -> std::io::Result<usize>;
}

#[cfg(unix)]
impl<T: Read + std::os::fd::AsRawFd> StreamingPipe for T {
    fn prepare(&self) -> std::io::Result<()> {
        let descriptor = self.as_raw_fd();
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags == -1
            || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    fn read_available(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.read(bytes)
    }
}

#[cfg(windows)]
impl<T: Read + std::os::windows::io::AsRawHandle> StreamingPipe for T {
    fn prepare(&self) -> std::io::Result<()> {
        Ok(())
    }

    fn read_available(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn PeekNamedPipe(
                pipe: *mut std::ffi::c_void,
                buffer: *mut std::ffi::c_void,
                size: u32,
                read: *mut u32,
                available: *mut u32,
                remaining: *mut u32,
            ) -> i32;
        }
        let mut available = 0;
        let result = unsafe {
            PeekNamedPipe(
                self.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if result == 0 {
            let error = std::io::Error::last_os_error();
            return if error.raw_os_error() == Some(109) {
                Ok(0) // ERROR_BROKEN_PIPE: the last writer closed its handle.
            } else {
                Err(error)
            };
        }
        if available == 0 {
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        let count = bytes.len().min(available as usize);
        self.read(&mut bytes[..count])
    }
}

struct LineStream<T> {
    pipe: T,
    pending: Vec<u8>,
    closed: bool,
}

impl<T: StreamingPipe> LineStream<T> {
    fn new(pipe: T) -> std::io::Result<Self> {
        pipe.prepare()?;
        Ok(Self {
            pipe,
            pending: Vec::new(),
            closed: false,
        })
    }

    fn poll(&mut self, line: &mut impl FnMut(&str)) -> std::io::Result<bool> {
        if self.closed {
            return Ok(false);
        }
        let mut bytes = [0; 16384];
        let count = match self.pipe.read_available(&mut bytes) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => return Ok(true),
            Err(error) => return Err(error),
        };
        if count == 0 {
            self.closed = true;
            if !self.pending.is_empty() {
                self.emit(line)?;
            }
            return Ok(true);
        }
        for chunk in bytes[..count].split_inclusive(|byte| *byte == b'\n') {
            if self.pending.len() + chunk.len() > MAX_STREAM_LINE_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Process output line exceeds the 8 MiB limit",
                ));
            }
            self.pending.extend_from_slice(chunk);
            if chunk.last() == Some(&b'\n') {
                self.pending.pop();
                if self.pending.last() == Some(&b'\r') {
                    self.pending.pop();
                }
                self.emit(line)?;
            }
        }
        Ok(true)
    }

    fn emit(&mut self, line: &mut impl FnMut(&str)) -> std::io::Result<()> {
        line(
            std::str::from_utf8(&self.pending)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
        );
        self.pending.clear();
        Ok(())
    }
}

// Downloads and Whisper need live lines, but neither may join a blocking pipe
// reader forever after the direct process exits. Poll both pipes in this thread,
// retaining child ownership through the bounded drain and closing them on error.
pub(crate) fn run_command_streaming(
    mut command: Command,
    state: &ProcessState,
    mut stdout_line: impl FnMut(&str),
    mut stderr_line: impl FnMut(&str),
) -> Result<ExitStatus, ProcessError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_child_process_group(&mut command);
    let mut process = command.spawn().map_err(ProcessError::Spawn)?;
    let stdout = process.stdout.take();
    let stderr = process.stderr.take();
    let child = Arc::new(Mutex::new(process));
    let result = (|| {
        register_current_child(state, &child).map_err(ProcessError::State)?;
        let mut stdout =
            LineStream::new(stdout.ok_or(ProcessError::MissingOutput("Process stdout missing"))?)
                .map_err(ProcessError::Read)?;
        let mut stderr =
            LineStream::new(stderr.ok_or(ProcessError::MissingOutput("Process stderr missing"))?)
                .map_err(ProcessError::Read)?;
        let mut drain_deadline = None;
        loop {
            let progressed = stdout.poll(&mut stdout_line).map_err(ProcessError::Read)?
                | stderr.poll(&mut stderr_line).map_err(ProcessError::Read)?;
            let mut process = child
                .lock()
                .map_err(|_| ProcessError::State("Child lock poisoned".into()))?;
            #[cfg(unix)]
            let exited = child_exit_without_reaping(&process)
                .map_err(ProcessError::Wait)?
                .is_none_or(|exited| exited);
            #[cfg(not(unix))]
            let exited = process.try_wait().map_err(ProcessError::Wait)?.is_some();
            if exited {
                if stdout.closed && stderr.closed {
                    return process.wait().map_err(ProcessError::Wait);
                }
                let deadline =
                    drain_deadline.get_or_insert_with(|| Instant::now() + OUTPUT_DRAIN_TIMEOUT);
                if Instant::now() >= *deadline {
                    return Err(ProcessError::OutputDrain);
                }
            }
            drop(process);
            if !progressed {
                thread::sleep(Duration::from_millis(10));
            }
        }
    })();
    if result.is_err() {
        let mut process = child.lock().unwrap_or_else(|error| error.into_inner());
        let _ = terminate_child_process_tree(&mut process);
        let _ = process.wait();
    }
    clear_current_child(state, &child);
    result
}

pub(crate) const MAX_OUTPUT_BYTES: usize = 128 * 1024 * 1024;

fn capture_pipe(
    pipe: &mut impl StreamingPipe,
    output: &mut Vec<u8>,
    total: &mut usize,
    limit: usize,
) -> Result<(bool, bool), ProcessError> {
    let mut bytes = [0u8; 16384];
    let mut progressed = false;
    // Bound each turn so a busy stdout cannot starve stderr or cancellation.
    for _ in 0..64 {
        let capacity = bytes
            .len()
            .min(limit.saturating_sub(*total).saturating_add(1));
        match pipe.read_available(&mut bytes[..capacity]) {
            Ok(0) => return Ok((true, progressed)),
            Ok(count) => {
                if count > limit.saturating_sub(*total) {
                    return Err(ProcessError::OutputLimit);
                }
                *total += count;
                output.extend_from_slice(&bytes[..count]);
                progressed = true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(ProcessError::Read(error)),
        }
    }
    Ok((false, progressed))
}

pub(crate) fn run_command_output(
    command: Command,
    active_state: Option<&ProcessState>,
    utility_state: Option<&ProcessState>,
    timeout: Option<Duration>,
) -> Result<Output, ProcessError> {
    run_command_output_limited(
        command,
        active_state,
        utility_state,
        timeout,
        MAX_OUTPUT_BYTES,
    )
}

pub(crate) fn run_command_output_limited(
    mut command: Command,
    active_state: Option<&ProcessState>,
    utility_state: Option<&ProcessState>,
    timeout: Option<Duration>,
    limit: usize,
) -> Result<Output, ProcessError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_child_process_group(&mut command);
    let mut process = command.spawn().map_err(ProcessError::Spawn)?;
    let stdout = process.stdout.take();
    let stderr = process.stderr.take();
    let child = Arc::new(Mutex::new(process));
    let result = (|| {
        if let Some(state) = active_state {
            register_current_child(state, &child).map_err(ProcessError::State)?;
        }
        if let Some(state) = utility_state {
            register_utility_child(state, &child).map_err(ProcessError::State)?;
        }
        let mut stdout = stdout.ok_or(ProcessError::MissingOutput("Process stdout missing"))?;
        let mut stderr = stderr.ok_or(ProcessError::MissingOutput("Process stderr missing"))?;
        stdout.prepare().map_err(ProcessError::Read)?;
        stderr.prepare().map_err(ProcessError::Read)?;
        let (mut out, mut err, mut total) = (Vec::new(), Vec::new(), 0);
        let (mut out_eof, mut err_eof) = (false, false);
        let started = Instant::now();
        let mut drain_deadline = None;
        loop {
            let mut progressed = false;
            if !out_eof {
                let (eof, read) = capture_pipe(&mut stdout, &mut out, &mut total, limit)?;
                out_eof = eof;
                progressed |= read;
            }
            if !err_eof {
                let (eof, read) = capture_pipe(&mut stderr, &mut err, &mut total, limit)?;
                err_eof = eof;
                progressed |= read;
            }
            let mut process = child
                .lock()
                .map_err(|_| ProcessError::State("Child lock poisoned".into()))?;
            #[cfg(unix)]
            let exited = child_exit_without_reaping(&process)
                .map_err(ProcessError::Wait)?
                .is_none_or(|exited| exited);
            #[cfg(not(unix))]
            let exited = process.try_wait().map_err(ProcessError::Wait)?.is_some();
            if exited {
                if out_eof && err_eof {
                    return Ok(Output {
                        status: process.wait().map_err(ProcessError::Wait)?,
                        stdout: out,
                        stderr: err,
                    });
                }
                let deadline =
                    drain_deadline.get_or_insert_with(|| Instant::now() + OUTPUT_DRAIN_TIMEOUT);
                if Instant::now() >= *deadline {
                    return Err(ProcessError::OutputDrain);
                }
            }
            if timeout.is_some_and(|limit| started.elapsed() >= limit) {
                return Err(ProcessError::TimedOut);
            }
            drop(process);
            if !progressed {
                thread::sleep(Duration::from_millis(10));
            }
        }
    })();
    if result.is_err() {
        if let Ok(mut process) = child.lock() {
            let _ = terminate_child_process_tree(&mut process);
            let _ = process.wait();
        }
    }
    if let Some(state) = active_state {
        clear_current_child(state, &child);
    }
    if let Some(state) = utility_state {
        clear_utility_child(state, &child);
    }
    result
}

#[derive(Default)]
pub(crate) struct ProcessState {
    pub(crate) current_job_id: Mutex<Option<String>>,
    pub(crate) current_child: Mutex<Option<Arc<Mutex<Child>>>>,
    pub(crate) utility_children: Mutex<Vec<Arc<Mutex<Child>>>>,
    pub(crate) cancel_requested: Mutex<Option<String>>,
    pub(crate) shutting_down: std::sync::atomic::AtomicBool,
}

#[derive(Debug)]
pub(crate) enum ProcessError {
    Spawn(std::io::Error),
    Read(std::io::Error),
    Wait(std::io::Error),
    State(String),
    TimedOut,
    OutputDrain,
    OutputLimit,
    MissingOutput(&'static str),
}
impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(error) | Self::Read(error) | Self::Wait(error) => write!(f, "{error}"),
            Self::State(error) => f.write_str(error),
            Self::TimedOut => f.write_str("process timed out"),
            Self::OutputLimit => f.write_str("Process output exceeded the capture budget"),
            Self::OutputDrain => f.write_str("Process output did not close after exit"),
            Self::MissingOutput(message) => f.write_str(message),
        }
    }
}
impl From<ProcessError> for String {
    fn from(error: ProcessError) -> Self {
        error.to_string()
    }
}
