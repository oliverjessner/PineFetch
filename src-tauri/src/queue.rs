use crate::models::DownloadJob;
use crate::models::DownloadState;
use crate::models::LinkDumpQueueSummary;
use crate::models::QueueStatus;
use crate::video_urls::normalize_video_url;
use std::collections::HashSet;
use std::collections::VecDeque;

pub(super) fn set_queue_paused(state: &QueueState, paused: bool) -> Result<(), String> {
    let mut value = state
        .paused
        .lock()
        .map_err(|_| "Queue pause lock poisoned")?;
    *value = paused;
    Ok(())
}

pub(super) fn snapshot_queue_status(state: &QueueState) -> Result<QueueStatus, String> {
    let auto_start = *state
        .auto_start
        .lock()
        .map_err(|_| "Queue auto-start lock poisoned")?;
    let worker_running = *state
        .worker_running
        .lock()
        .map_err(|_| "Worker lock poisoned")?;
    let paused = *state
        .paused
        .lock()
        .map_err(|_| "Queue pause lock poisoned")?;

    Ok(QueueStatus {
        auto_start,
        worker_running,
        paused,
    })
}

pub(super) fn is_queue_auto_start_enabled(state: &QueueState) -> Result<bool, String> {
    let auto_start = state
        .auto_start
        .lock()
        .map_err(|_| "Queue auto-start lock poisoned")?;
    Ok(*auto_start)
}

pub(super) fn next_worker_job(
    state: &QueueState,
    current_job_id: &std::sync::Mutex<Option<String>>,
) -> Result<(Option<DownloadJob>, bool), String> {
    let pause_guard = state
        .paused
        .lock()
        .map_err(|_| "Queue pause lock poisoned")?;
    let paused = *pause_guard;
    let mut queue = state.pending.lock().map_err(|_| "Queue lock poisoned")?;
    let job = if paused { None } else { queue.pop_front() };
    if let Some(job) = job.as_ref() {
        let mut current = current_job_id
            .lock()
            .map_err(|_| "Current job lock poisoned")?;
        *current = Some(job.id.clone());
        let mut active_key = state
            .active_video_key
            .lock()
            .map_err(|_| "Active video key lock poisoned")?;
        *active_key = normalize_video_url(&job.url).map(|normalized| normalized.key);
    }
    // Keep the queue locked until idle is published so an enqueue or resume
    // cannot miss starting a worker between the empty check and shutdown.
    if job.is_none() {
        let mut running = state
            .worker_running
            .lock()
            .map_err(|_| "Worker lock poisoned")?;
        *running = false;
    }
    Ok((job, paused))
}

#[derive(Default)]
pub(super) struct QueueRunSummary {
    pub(super) started: usize,
    pub(super) succeeded: usize,
}

impl QueueRunSummary {
    pub(super) fn should_notify(&self, enabled: bool) -> bool {
        enabled && self.started > 1 && self.succeeded == self.started
    }
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

pub(crate) struct QueueState {
    pub(crate) pending: std::sync::Mutex<std::collections::VecDeque<DownloadJob>>,
    pub(crate) auto_start: std::sync::Mutex<bool>,
    pub(crate) paused: std::sync::Mutex<bool>,
    pub(crate) worker_running: std::sync::Mutex<bool>,
    pub(crate) worker_handle: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    pub(crate) active_video_key: std::sync::Mutex<Option<String>>,
}
impl Default for QueueState {
    fn default() -> Self {
        Self {
            pending: std::sync::Mutex::new(std::collections::VecDeque::new()),
            auto_start: std::sync::Mutex::new(true),
            paused: std::sync::Mutex::new(false),
            worker_running: std::sync::Mutex::new(false),
            worker_handle: std::sync::Mutex::new(None),
            active_video_key: std::sync::Mutex::new(None),
        }
    }
}
pub(crate) fn enqueue_jobs(state: &QueueState, jobs: Vec<DownloadJob>) -> Result<(), String> {
    state
        .pending
        .lock()
        .map_err(|_| "Queue lock poisoned")?
        .extend(jobs);
    Ok(())
}
pub(crate) fn remove_queued_job(state: &QueueState, id: &str) -> Result<bool, String> {
    let mut queue = state.pending.lock().map_err(|_| "Queue lock poisoned")?;
    let before = queue.len();
    queue.retain(|job| job.id != id);
    Ok(before != queue.len())
}

pub(crate) fn finalize_active_job_event(
    processes: &crate::process::ProcessState,
    queue: &QueueState,
    mut event: crate::models::DownloadStateEvent,
    after_success: impl FnOnce() -> Result<(), String>,
) -> crate::models::DownloadStateEvent {
    if let Ok(mut current) = processes.current_job_id.lock() {
        if let Ok(mut cancel) = processes.cancel_requested.lock() {
            if cancel.as_deref() == Some(event.id.as_str()) {
                *cancel = None;
                event.state = DownloadState::Cancelled;
                event.error = event
                    .output_path
                    .as_ref()
                    .map(|_| "Cancelled after output creation; output file preserved".to_string());
            }
        } else {
            event.apply_result(Err(
                "Cancellation state lock poisoned; output file preserved".into(),
            ));
        }
        if event.state == DownloadState::Success {
            let result = after_success();
            event.apply_result(result);
        }
        *current = None;
        if let Ok(mut active_key) = queue.active_video_key.lock() {
            *active_key = None;
        }
    } else {
        event.apply_result(Err(
            "Completion state lock poisoned; output file preserved".into()
        ));
    }
    event
}
