use crate::models::{
    DownloadJob, DownloadState, DownloadStateEvent, LinkDumpQueueSummary, QueueStatus,
};
use crate::video_urls::normalize_video_url;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct RecoveredDownload {
    pub(crate) job: DownloadJob,
    pub(crate) output_path: Option<String>,
    pub(crate) error: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QueueData {
    pub(crate) format_version: u32,
    pub(crate) pending: VecDeque<DownloadJob>,
    pub(crate) active: Option<DownloadJob>,
    pub(crate) interrupted: Vec<RecoveredDownload>,
    pub(crate) auto_start: bool,
    pub(crate) paused: bool,
    #[serde(skip)]
    pub(crate) revision: i64,
    #[serde(skip)]
    pub(crate) worker_running: bool,
    #[serde(skip)]
    pub(crate) stopping: bool,
    #[serde(skip)]
    pub(crate) active_video_key: Option<String>,
    #[serde(skip, default = "downloading")]
    pub(crate) active_state: DownloadState,
}
fn downloading() -> DownloadState {
    DownloadState::Downloading
}
impl Default for QueueData {
    fn default() -> Self {
        Self {
            format_version: 1,
            pending: VecDeque::new(),
            active: None,
            interrupted: Vec::new(),
            auto_start: true,
            paused: false,
            revision: 0,
            worker_running: false,
            stopping: false,
            active_video_key: None,
            active_state: downloading(),
        }
    }
}
#[derive(Clone, Serialize)]
pub(crate) struct ActiveDownload {
    job: DownloadJob,
    state: DownloadState,
}
#[derive(Clone, Serialize)]
pub(crate) struct QueueSnapshot {
    revision: i64,
    pending: Vec<DownloadJob>,
    active: Option<ActiveDownload>,
    interrupted: Vec<RecoveredDownload>,
}
#[derive(Default)]
pub(crate) struct QueueState {
    pub(crate) data: Mutex<QueueData>,
    pub(crate) worker_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    store: Option<crate::queue_store::QueueStore>,
}
impl QueueState {
    pub(crate) fn restore(db: Arc<crate::database::Database>) -> Result<Self, String> {
        let store = crate::queue_store::QueueStore::open(db)?;
        let data = store.load()?;
        Ok(Self {
            data: Mutex::new(data),
            worker_handle: Mutex::new(None),
            store: Some(store),
        })
    }
    fn persist(&self, data: &mut QueueData) -> Result<(), String> {
        if let Some(store) = &self.store {
            store.save(data)?;
        }
        Ok(())
    }
    fn update<T>(
        &self,
        change: impl FnOnce(&mut QueueData) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut data = self.data.lock().map_err(|_| "Queue lock poisoned")?;
        let mut next = data.clone();
        let result = change(&mut next)?;
        if serde_json::to_value(&next).map_err(|error| error.to_string())?
            != serde_json::to_value(&*data).map_err(|error| error.to_string())?
        {
            self.persist(&mut next)?;
        }
        *data = next;
        Ok(result)
    }
}
pub(super) fn set_queue_paused(state: &QueueState, paused: bool) -> Result<(), String> {
    state.update(|data| {
        data.paused = paused;
        if !paused {
            data.stopping = false;
        }
        Ok(())
    })
}
pub(crate) fn set_queue_auto_start(state: &QueueState, enabled: bool) -> Result<(), String> {
    state.update(|data| {
        data.auto_start = enabled;
        Ok(())
    })
}
pub(super) fn snapshot_queue_status(state: &QueueState) -> Result<QueueStatus, String> {
    let data = state.data.lock().map_err(|_| "Queue lock poisoned")?;
    Ok(QueueStatus {
        auto_start: data.auto_start,
        worker_running: data.worker_running,
        paused: data.paused || data.stopping,
    })
}
pub(crate) fn snapshot_queue(state: &QueueState) -> Result<QueueSnapshot, String> {
    let data = state.data.lock().map_err(|_| "Queue lock poisoned")?;
    Ok(QueueSnapshot {
        revision: data.revision,
        pending: data.pending.iter().cloned().collect(),
        active: data.active.clone().map(|job| ActiveDownload {
            job,
            state: data.active_state,
        }),
        interrupted: data.interrupted.clone(),
    })
}
pub(super) fn is_queue_auto_start_enabled(state: &QueueState) -> Result<bool, String> {
    Ok(state
        .data
        .lock()
        .map_err(|_| "Queue lock poisoned")?
        .auto_start)
}
pub(crate) fn claim_worker_start(state: &QueueState) -> Result<bool, String> {
    let mut data = state.data.lock().map_err(|_| "Queue lock poisoned")?;
    if data.paused || data.stopping || data.worker_running {
        return Ok(false);
    }
    data.worker_running = true;
    Ok(true)
}
pub(crate) fn set_active_phase(state: &QueueState, phase: DownloadState) {
    if let Ok(mut data) = state.data.lock() {
        data.active_state = phase;
    }
}
pub(crate) fn worker_failed(state: &QueueState) {
    if let Ok(mut data) = state.data.lock() {
        data.worker_running = false;
        data.stopping = true;
    }
}
pub(super) fn next_worker_job(
    state: &QueueState,
    current_job_id: &Mutex<Option<String>>,
) -> Result<(Option<DownloadJob>, bool), String> {
    let mut data = state.data.lock().map_err(|_| "Queue lock poisoned")?;
    if data.paused || data.stopping || data.pending.is_empty() {
        data.worker_running = false;
        return Ok((None, data.paused || data.stopping));
    }
    let mut current = current_job_id
        .lock()
        .map_err(|_| "Current job lock poisoned")?;
    let mut next = data.clone();
    let job = next.pending.pop_front().expect("nonempty queue");
    next.active = Some(job.clone());
    next.active_state = downloading();
    next.active_video_key = normalize_video_url(&job.url).map(|url| url.key);
    state.persist(&mut next)?;
    *current = Some(job.id.clone());
    *data = next;
    Ok((Some(job), false))
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
    let mut queued_keys: HashSet<_> = queue
        .iter()
        .filter_map(|job| normalize_video_url(&job.url).map(|url| url.key))
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
pub(crate) fn enqueue_unique(
    state: &QueueState,
    candidates: Vec<(String, DownloadJob)>,
    summary: &mut LinkDumpQueueSummary,
) -> Result<usize, String> {
    let mut delta = LinkDumpQueueSummary {
        received: 0,
        added: 0,
        skipped: 0,
        invalid: 0,
    };
    let added = state.update(|data| {
        Ok(insert_unique_video_jobs(
            &mut data.pending,
            data.active_video_key.as_deref(),
            candidates,
            &mut delta,
        ))
    })?;
    summary.skipped += delta.skipped;
    Ok(added)
}
pub(crate) fn enqueue_jobs(state: &QueueState, jobs: Vec<DownloadJob>) -> Result<(), String> {
    state.update(|data| {
        data.pending.extend(jobs);
        Ok(())
    })
}
pub(crate) fn remove_queued_job(state: &QueueState, id: &str) -> Result<bool, String> {
    state.update(|data| {
        let before = data.pending.len() + data.interrupted.len();
        data.pending.retain(|job| job.id != id);
        data.interrupted.retain(|entry| entry.job.id != id);
        Ok(before != data.pending.len() + data.interrupted.len())
    })
}
pub(crate) fn remove_waiting_at(state: &QueueState, number: usize) -> Result<DownloadJob, String> {
    state.update(|data| {
        let index = number.checked_sub(1).ok_or("Queue numbering starts at 1")?;
        if index < data.pending.len() {
            return Ok(data.pending.remove(index).expect("valid index"));
        }
        let index = index - data.pending.len();
        if index < data.interrupted.len() {
            return Ok(data.interrupted.remove(index).job);
        }
        Err(format!("No waiting queue item at position {number}"))
    })
}
pub(crate) fn retry_interrupted(state: &QueueState, id: &str) -> Result<DownloadJob, String> {
    state.update(|data| {
        let index = data
            .interrupted
            .iter()
            .position(|entry| entry.job.id == id)
            .ok_or("Interrupted job not found")?;
        let mut job = data.interrupted.remove(index).job;
        job.id = uuid::Uuid::new_v4().to_string();
        data.pending.push_back(job.clone());
        Ok(job)
    })
}
pub(crate) fn finalize_active_job_event(
    processes: &crate::process::ProcessState,
    queue: &QueueState,
    mut event: DownloadStateEvent,
    after_success: impl FnOnce() -> Result<(), String>,
) -> DownloadStateEvent {
    let Ok(mut data) = queue.data.lock() else {
        event.apply_result(Err(
            "Queue state lock poisoned; output file preserved".into()
        ));
        return event;
    };
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
            event.apply_result(after_success());
        }
        let mut next = data.clone();
        if let Some(job) = next.active.take() {
            if processes
                .shutting_down
                .load(std::sync::atomic::Ordering::SeqCst)
                && event.state != DownloadState::Success
            {
                next.interrupted.push(crate::queue_store::recovered(
                    job,
                    event.output_path.clone(),
                ));
            }
        }
        next.active_video_key = None;
        if let Err(error) = queue.persist(&mut next) {
            event.apply_result(Err(error));
            next.stopping = true;
            next.worker_running = false;
        }
        *data = next;
        *current = None;
    } else {
        event.apply_result(Err(
            "Completion state lock poisoned; output file preserved".into()
        ));
    }
    event
}
