import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
    createAppState,
    updateJobState,
    applyQueueStatus,
    applyDownloadState,
    isCancellable,
    isQueueBusy,
    isRemovable,
} from '../src/app-state.js';

test('each app state owns its maps and sets without DOM references', () => {
    const first = createAppState();
    const second = createAppState();
    first.jobs.set('one', { id: 'one' });
    first.suppressedJobIds.add('one');
    assert.equal(second.jobs.size, 0);
    assert.equal(second.suppressedJobIds.size, 0);
    assert.equal(first.queueAutoStartEnabled, true);
    assert.equal(first.queuePaused, false);
    assert.equal(Object.isSealed(first), true);
});

test('job updates retain creation order and distinguish progress from full renders', () => {
    const state = createAppState();
    assert.equal(updateJobState(state, 'job', { state: 'queued', label: 'Synthetic' }, 100), 'full');
    assert.equal(updateJobState(state, 'job', { percent: 50, eta: '1s' }, 200), 'progress');
    assert.equal(state.jobs.get('job').createdAt, 100);
    assert.equal(state.jobs.get('job').label, 'Synthetic');
    assert.equal(updateJobState(state, 'job', { previewLoading: true }, 300), 'skip');
    assert.equal(state.jobs.get('job').previewLoading, true);
    assert.equal(updateJobState(state, 'job', { state: 'transcribing', percent: 100 }, 400), 'full');
    assert.equal(updateJobState(state, 'job', {}, 500), 'full');
});

test('queue status preserves false values and the caller-specific auto-start fallback', () => {
    const state = createAppState();
    applyQueueStatus(state, { auto_start: false, worker_running: true, paused: true });
    assert.deepEqual([state.queueAutoStartEnabled, state.queueWorkerRunning, state.queuePaused], [false, true, true]);
    applyQueueStatus(state, null, false);
    assert.deepEqual([state.queueAutoStartEnabled, state.queueWorkerRunning, state.queuePaused], [false, false, false]);
    applyQueueStatus(state, {});
    assert.equal(state.queueAutoStartEnabled, true);
});

test('pending clear waits for terminal events and suppresses late updates', () => {
    for (const status of ['success', 'error', 'cancelled']) {
        const state = createAppState();
        updateJobState(state, 'one', { state: 'downloading' }, 100);
        updateJobState(state, 'two', { state: 'queued' }, 200);
        state.queueIds = ['one', 'two'];
        state.pendingClearAfterTerminal.add('one');
        assert.equal(applyDownloadState(state, { id: 'one', state: 'cancelling' }).kind, 'updated');
        assert.equal(state.jobs.has('one'), true);
        assert.equal(applyDownloadState(state, { id: 'one', state: status }).kind, 'removed');
        assert.deepEqual(state.queueIds, ['two']);
        assert.equal(state.pendingClearAfterTerminal.has('one'), false);
        assert.equal(state.suppressedJobIds.has('one'), true);
        assert.equal(applyDownloadState(state, { id: 'one', state: 'success' }).kind, 'ignored');
        assert.equal(state.jobs.has('one'), false);
    }
});

test('success and persistence errors preserve existing event and output-path semantics', () => {
    const state = createAppState();
    assert.deepEqual(
        applyDownloadState(state, { id: 'one', state: 'success', output_path: '/synthetic/Grüße 🌲.mp4' }).patch,
        { state: 'success', outputPath: '/synthetic/Grüße 🌲.mp4', percent: 100, speed: 'done', eta: '-' }
    );
    const failure = applyDownloadState(state, {
        id: 'one',
        state: 'error',
        error: 'Output file preserved',
        output_path: '/synthetic/output',
    });
    assert.deepEqual(failure.patch, {
        state: 'error',
        error: 'Output file preserved',
        outputPath: '/synthetic/output',
    });
    assert.deepEqual(applyDownloadState(state, { id: 'one', state: 'cancelled', output_path: null }).patch, {
        state: 'cancelled',
    });
});

test('cancellation and removal categories match the existing queue behavior', () => {
    for (const status of [
        'queued',
        'downloading',
        'transcribing',
        'cancelling',
        'success',
        'error',
        'cancelled',
        'unknown',
    ]) {
        assert.equal(isCancellable(status), ['downloading', 'transcribing'].includes(status));
        assert.equal(isQueueBusy(status), ['downloading', 'transcribing', 'cancelling'].includes(status));
        assert.equal(isRemovable(status), ['queued', 'success', 'error', 'cancelled'].includes(status));
    }
});

test('queue snapshots restore waiting, active and interrupted jobs without starting interrupted work', async () => {
    const { mergeQueueSnapshot } = await import('../src/app-state.js');
    const state = createAppState();
    const pending = { id: 'waiting', url: 'https://example.com/waiting', cut_start_time: 12 };
    mergeQueueSnapshot(
        state,
        {
            pending: [pending],
            active: { job: { id: 'active', url: 'https://example.com/active' }, state: 'transcribing' },
            interrupted: [
                {
                    job: { id: 'stopped', url: 'https://example.com/stopped' },
                    output_path: '/saved/video.mp4',
                    error: 'Interrupted; output preserved.',
                },
            ],
        },
        job => ({ label: job.url }),
        10
    );
    assert.deepEqual(state.queueIds, ['waiting']);
    assert.equal(state.jobs.get('active').state, 'transcribing');
    assert.equal(state.jobs.get('stopped').state, 'error');
    assert.equal(state.jobs.get('stopped').recovered, true);
    assert.equal(state.jobs.get('stopped').outputPath, '/saved/video.mp4');
    assert.match(state.jobs.get('stopped').error, /Interrupted/);
});

test('late queue snapshots preserve newer active and terminal states and suppressed removals', async () => {
    const { mergeQueueSnapshot } = await import('../src/app-state.js');
    const state = createAppState();
    updateJobState(state, 'active', { state: 'transcribing' }, 10);
    updateJobState(state, 'done', { state: 'success' }, 10);
    state.suppressedJobIds.add('removed');
    mergeQueueSnapshot(
        state,
        ['active', 'done', 'removed'].map(id => ({ id, url: 'https://example.com/' + id }))
    );
    assert.deepEqual(state.queueIds, []);
    assert.equal(state.jobs.get('active').state, 'transcribing');
    assert.equal(state.jobs.get('done').state, 'success');
    assert.equal(state.jobs.has('removed'), false);
    mergeQueueSnapshot(state, {
        pending: [],
        active: { job: { id: 'active' }, state: 'downloading' },
        interrupted: [],
    });
    assert.equal(state.jobs.get('active').state, 'transcribing');
});

test('a durable removal or requeue snapshot clears recovered rows without clearing session results', async () => {
    const { mergeQueueSnapshot } = await import('../src/app-state.js');
    const state = createAppState();
    updateJobState(state, 'done', { state: 'success' }, 1);
    mergeQueueSnapshot(state, {
        pending: [],
        interrupted: [{ job: { id: 'old', url: 'https://example.com/old' }, error: 'Interrupted' }],
    });
    mergeQueueSnapshot(state, { pending: [{ id: 'new', url: 'https://example.com/old' }], interrupted: [] });
    assert.equal(state.jobs.has('old'), false);
    assert.equal(state.jobs.get('done').state, 'success');
    assert.equal(state.jobs.get('new').state, 'queued');
    assert.deepEqual(state.queueIds, ['new']);
});

test('older queue revisions cannot hide newly accepted waiting jobs', async () => {
    const { mergeQueueSnapshot } = await import('../src/app-state.js');
    const state = createAppState();
    mergeQueueSnapshot(state, {
        revision: 4,
        pending: [{ id: 'new', url: 'https://example.com/new' }],
        interrupted: [],
    });
    mergeQueueSnapshot(state, { revision: 3, pending: [], interrupted: [] });
    assert.deepEqual(state.queueIds, ['new']);
    assert.equal(state.jobs.get('new').state, 'queued');
    assert.equal(state.queueRevision, 4);
});
