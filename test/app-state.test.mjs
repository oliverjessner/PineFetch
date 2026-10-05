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
