import assert from 'node:assert/strict';
import test from 'node:test';
import { createAppState, updateJobState, applyDownloadState, applyDownloadProgress } from '../src/app-state.js';

test('late running states cannot reactivate a completed, failed or cancelled job', () => {
    for (const terminal of ['success', 'error', 'cancelled']) {
        const state = createAppState();
        updateJobState(state, 'job', { state: terminal, outputPath: '/synthetic/Grüße 🌲.mp4' }, 10);
        for (const late of ['queued', 'downloading', 'transcribing', 'cancelling']) {
            assert.equal(applyDownloadState(state, { id: 'job', state: late }).kind, 'ignored');
            assert.equal(state.jobs.get('job').state, terminal);
        }
    }
});

test('malformed state events are ignored without creating jobs or throwing', () => {
    const state = createAppState();
    for (const payload of [null, undefined, {}, { id: '', state: 'success' }, { id: 'job', state: 'unknown' }]) {
        assert.equal(applyDownloadState(state, payload).kind, 'ignored');
        assert.equal(state.jobs.size, 0);
    }
});

test('duplicate completion is idempotent and preserves creation time and output', () => {
    const state = createAppState();
    updateJobState(state, 'job', { state: 'downloading' }, 10);
    const payload = { id: 'job', state: 'success', output_path: '/synthetic/Clip file.mp4' };
    for (let index = 0; index < 2; index += 1) {
        const result = applyDownloadState(state, payload);
        updateJobState(state, result.id, result.patch, 20);
    }
    assert.equal(state.jobs.size, 1);
    assert.equal(state.jobs.get('job').createdAt, 10);
    assert.equal(state.jobs.get('job').state, 'success');
    assert.equal(state.jobs.get('job').percent, 100);
    assert.equal(state.jobs.get('job').outputPath, payload.output_path);
});

test('late progress cannot overwrite terminal progress and removed jobs stay absent', () => {
    const state = createAppState();
    updateJobState(state, 'job', { state: 'success', percent: 100, speed: 'done' }, 10);
    assert.equal(applyDownloadProgress(state, { id: 'job', percent: 42, speed: 'slow' }).kind, 'ignored');
    assert.equal(state.jobs.get('job').percent, 100);
    state.suppressedJobIds.add('removed');
    assert.equal(applyDownloadProgress(state, { id: 'removed', percent: 10 }).kind, 'ignored');
    assert.equal(state.jobs.has('removed'), false);
});

test('progress before a queue snapshot remains usable and malformed progress is harmless', () => {
    const state = createAppState();
    const result = applyDownloadProgress(state, { id: 'early', percent: 42, speed: '2MiB/s', eta: '10s' });
    assert.equal(result.kind, 'updated');
    updateJobState(state, result.id, result.patch, 10);
    updateJobState(state, 'early', { state: 'downloading', label: 'Clip' }, 20);
    assert.equal(state.jobs.get('early').percent, 42);
    assert.equal(state.jobs.get('early').createdAt, 10);
    for (const payload of [null, undefined, {}, { id: '' }]) {
        assert.equal(applyDownloadProgress(state, payload).kind, 'ignored');
    }
});
