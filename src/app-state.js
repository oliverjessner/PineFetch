export const createAppState = () =>
    Object.seal({
        jobs: new Map(),
        queueIds: [],
        queueAutoStartEnabled: true,
        queueWorkerRunning: false,
        queuePaused: false,
        queueCollapsed: false,
        suppressedJobIds: new Set(),
        pendingClearAfterTerminal: new Set(),
        selectedId: null,
        contextMenuJobId: null,
        logs: [],
        info: null,
        infoUrl: null,
        activeView: 'download',
    });
const cancellableJobStates = new Set(['downloading', 'transcribing']);
const queueBusyJobStates = new Set(['downloading', 'transcribing', 'cancelling']);
const removableJobStates = new Set(['queued', 'success', 'error', 'cancelled']);
const terminalJobStates = new Set(['success', 'error', 'cancelled']);
const knownJobStates = new Set([...removableJobStates, ...queueBusyJobStates]);

export const isCancellable = status => cancellableJobStates.has(status);
export const isQueueBusy = status => queueBusyJobStates.has(status);
export const isRemovable = status => removableJobStates.has(status);

// Returns the render work needed, keeping DOM scheduling in the view.
export const updateJobState = (state, id, patch, now) => {
    const previous = state.jobs.get(id);
    const existing = previous || { id, createdAt: now };
    state.jobs.set(id, { ...existing, ...patch });
    const changedKeys = Object.keys(patch);
    if (previous && changedKeys.length === 1 && changedKeys[0] === 'previewLoading') return 'skip';
    const progressOnly =
        previous &&
        changedKeys.length > 0 &&
        changedKeys.every(key => key === 'percent' || key === 'speed' || key === 'eta');
    return progressOnly ? 'progress' : 'full';
};

export const applyQueueStatus = (state, status, autoStartFallback = true) => {
    state.queueAutoStartEnabled = status?.auto_start ?? autoStartFallback;
    state.queueWorkerRunning = Boolean(status?.worker_running);
    state.queuePaused = Boolean(status?.paused);
};

export const applyDownloadState = (state, payload) => {
    if (typeof payload?.id !== 'string' || !payload.id || !knownJobStates.has(payload.state)) {
        return { kind: 'ignored', id: payload?.id };
    }
    const { id, state: status, output_path, error } = payload;
    if (state.suppressedJobIds.has(id)) return { kind: 'ignored', id };
    if (terminalJobStates.has(state.jobs.get(id)?.state) && !terminalJobStates.has(status)) {
        return { kind: 'ignored', id };
    }
    if (state.pendingClearAfterTerminal.has(id) && ['success', 'error', 'cancelled'].includes(status)) {
        state.pendingClearAfterTerminal.delete(id);
        state.suppressedJobIds.add(id);
        state.jobs.delete(id);
        state.queueIds = state.queueIds.filter(queuedId => queuedId !== id);
        return { kind: 'removed', id };
    }
    const patch = { state: status };
    if (error) patch.error = error;
    if (output_path) patch.outputPath = output_path;
    if (status === 'success') {
        patch.percent = 100;
        patch.speed = 'done';
        patch.eta = '-';
    }
    return { kind: 'updated', id, patch };
};

export const applyDownloadProgress = (state, payload) => {
    if (typeof payload?.id !== 'string' || !payload.id) return { kind: 'ignored', id: payload?.id };
    const { id, percent, speed, eta } = payload;
    if (state.suppressedJobIds.has(id) || terminalJobStates.has(state.jobs.get(id)?.state)) {
        return { kind: 'ignored', id };
    }
    return { kind: 'updated', id, patch: { percent: percent ?? 0, speed: speed || '-', eta: eta || '-' } };
};
