export const createAppState = () =>
    Object.seal({
        jobs: new Map(),
        queueIds: [],
        queueRevision: -1,
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

// Snapshots also bootstrap a reopened window after backend events were missed.
// Late responses must not turn active or completed jobs back into waiting jobs.
export const mergeQueueSnapshot = (state, snapshot, describe = () => ({}), now = Date.now()) => {
    if (Number.isSafeInteger(snapshot?.revision)) {
        if (snapshot.revision < state.queueRevision) return [];
        state.queueRevision = snapshot.revision;
    }
    const pending = Array.isArray(snapshot) ? snapshot : Array.isArray(snapshot?.pending) ? snapshot.pending : [];
    const interrupted = Array.isArray(snapshot?.interrupted) ? snapshot.interrupted : [];
    const active = snapshot?.active;
    const recoveredIds = new Set(interrupted.map(entry => entry.job?.id));
    for (const [id, job] of state.jobs) {
        if (job.recovered && !recoveredIds.has(id)) state.jobs.delete(id);
    }
    const entries = [
        ...pending.map(job => ({ job, state: 'queued' })),
        ...(active?.job ? [{ job: active.job, state: active.state || 'downloading' }] : []),
        ...interrupted.map(entry => ({ ...entry, state: 'error', recovered: true })),
    ];
    const waiting = [];
    const changed = [];
    const phaseRank = { downloading: 1, transcribing: 2, cancelling: 3 };
    for (const entry of entries) {
        const job = entry.job;
        if (!job?.id || state.suppressedJobIds.has(job.id)) continue;
        const existing = state.jobs.get(job.id);
        let status = entry.state;
        if (terminalJobStates.has(existing?.state) && !entry.recovered) status = existing.state;
        if (existing?.state === 'success' || existing?.state === 'cancelled') status = existing.state;
        if (isQueueBusy(existing?.state) && (phaseRank[status] || 0) < phaseRank[existing.state])
            status = existing.state;
        updateJobState(
            state,
            job.id,
            {
                ...describe(job, existing),
                url: job.url,
                state: status,
                recovered: Boolean(entry.recovered && status === 'error'),
                error: entry.recovered ? entry.error : existing?.error,
                outputPath: entry.output_path || existing?.outputPath || null,
            },
            now
        );
        if (status === 'queued' && !entry.recovered) waiting.push(job.id);
        changed.push(job.id);
    }
    state.queueIds = waiting;
    return changed;
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
