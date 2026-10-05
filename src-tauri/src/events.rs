use crate::models::{DownloadProgress, DownloadStateEvent, LogEvent};
use crate::queue::{snapshot_queue_status, QueueState};
use tauri::{AppHandle, Emitter};

pub(crate) const QUEUE_STATUS: &str = "queue:status";
pub(crate) const QUEUE_UPDATE: &str = "queue:update";
pub(crate) const DOWNLOAD_PROGRESS: &str = "download:progress";
pub(crate) const DOWNLOAD_STATE: &str = "download:state";
pub(crate) const DOWNLOAD_LOG: &str = "download:log";
pub(crate) const HISTORY_CHANGED: &str = "history:changed";
pub(crate) const LINK_DUMP_SERVER_STATUS: &str = "link-dump:server-status";

pub(super) fn emit_queue_status(app: &AppHandle, state: &QueueState) {
    if let Ok(status) = snapshot_queue_status(state) {
        let _ = app.emit(QUEUE_STATUS, status);
    }
}

pub(super) fn emit_queue(app: &AppHandle, state: &QueueState) -> Result<(), String> {
    let queue = state.pending.lock().map_err(|_| "Queue lock poisoned")?;
    app.emit(QUEUE_UPDATE, queue.clone())
        .map_err(|e| format!("Emit queue failed: {e}"))
}

pub(super) fn emit_progress(app: &AppHandle, progress: DownloadProgress) {
    let _ = app.emit(DOWNLOAD_PROGRESS, progress);
}

pub(super) fn emit_state(app: &AppHandle, state: DownloadStateEvent) {
    let _ = app.emit(DOWNLOAD_STATE, state);
}

pub(super) fn emit_log(app: &AppHandle, log: LogEvent) {
    let _ = app.emit(DOWNLOAD_LOG, log);
}
