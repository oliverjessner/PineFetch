use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::Child;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use std::time::Instant;

pub(crate) const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

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
    // A running child is its own Unix process-group leader. Only signal the
    // group while that leader is still alive, so the PGID cannot be reused.
    if process.try_wait()?.is_some() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        let group_id = i32::try_from(process.id())
            .map_err(|_| std::io::Error::other("Child process ID is out of range"))?;
        if unsafe { libc::kill(-group_id, libc::SIGKILL) } == 0 {
            return Ok(());
        }
        let group_error = std::io::Error::last_os_error();
        let _ = process.kill();
        Err(group_error)
    }
    #[cfg(not(unix))]
    process.kill()
}

pub(crate) fn run_command_output(
    mut command: Command,
    active_state: Option<&ProcessState>,
    utility_state: Option<&ProcessState>,
    timeout: Option<Duration>,
) -> Result<Output, ProcessError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_child_process_group(&mut command);
    let mut child = command.spawn().map_err(ProcessError::Spawn)?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child = Arc::new(Mutex::new(child));
    if let Some(state) = active_state {
        if let Err(err) = register_current_child(state, &child) {
            if let Ok(mut process) = child.lock() {
                let _ = terminate_child_process_tree(&mut process);
                let _ = process.wait();
            }
            return Err(ProcessError::State(err));
        }
    }
    if let Some(state) = utility_state {
        if let Err(err) = register_utility_child(state, &child) {
            if let Ok(mut process) = child.lock() {
                let _ = terminate_child_process_tree(&mut process);
                let _ = process.wait();
            }
            return Err(ProcessError::State(err));
        }
    }

    let (output_sender, output_receiver) = std::sync::mpsc::channel();
    let stdout_sender = output_sender.clone();
    thread::spawn(move || {
        let mut output = Vec::new();
        if let Some(mut stdout) = stdout {
            let _ = stdout.read_to_end(&mut output);
        }
        let _ = stdout_sender.send((true, output));
    });
    thread::spawn(move || {
        let mut output = Vec::new();
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_end(&mut output);
        }
        let _ = output_sender.send((false, output));
    });

    let started = Instant::now();
    let mut failure = None;
    let status = loop {
        let result = child
            .lock()
            .map_err(|_| ProcessError::State("Child lock poisoned".into()))
            .and_then(|mut process| process.try_wait().map_err(ProcessError::Wait));
        match result {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(err) => {
                failure = Some(err);
                break None;
            }
        }
        if timeout.is_some_and(|limit| started.elapsed() >= limit) {
            failure = Some(ProcessError::TimedOut);
            break None;
        }
        thread::sleep(Duration::from_millis(100));
    };
    if status.is_none() {
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
    if let Some(err) = failure {
        // A descendant may still own a pipe after the direct child exits.
        // Do not let that keep a timed-out metadata request blocked here.
        return Err(err);
    }
    let mut drain_deadline = Instant::now() + OUTPUT_DRAIN_TIMEOUT;
    if let Some(limit) = timeout.and_then(|limit| started.checked_add(limit)) {
        drain_deadline = drain_deadline.min(limit);
    }
    let mut stdout = None;
    let mut stderr = None;
    for _ in 0..2 {
        let remaining = drain_deadline.saturating_duration_since(Instant::now());
        let (is_stdout, output) = output_receiver
            .recv_timeout(remaining)
            .map_err(|_| ProcessError::OutputDrain)?;
        if is_stdout {
            stdout = Some(output);
        } else {
            stderr = Some(output);
        }
    }
    Ok(Output {
        status: status.ok_or(ProcessError::MissingOutput("Process status unavailable"))?,
        stdout: stdout.ok_or(ProcessError::MissingOutput("Process stdout missing"))?,
        stderr: stderr.ok_or(ProcessError::MissingOutput("Process stderr missing"))?,
    })
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
    Wait(std::io::Error),
    State(String),
    TimedOut,
    OutputDrain,
    MissingOutput(&'static str),
}
impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(error) | Self::Wait(error) => write!(f, "{error}"),
            Self::State(error) => f.write_str(error),
            Self::TimedOut => f.write_str("process timed out"),
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
