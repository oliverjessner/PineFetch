import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
    createHistoryTranscriptionJobs,
    transcriptionActive,
    transcriptionErrorMessage,
} from '../src/history-transcription-jobs.js';

const deferred = () => {
    let resolve;
    let reject;
    const promise = new Promise((yes, no) => {
        resolve = yes;
        reject = no;
    });
    return { promise, resolve, reject };
};
const progress = (status = 'running', overrides = {}) => ({
    history_id: 'video-a',
    attempt_id: 'attempt-a',
    attempt_at: 100,
    status,
    phase: 'transcribing',
    model: 'base',
    ...overrides,
});
const fixture = (overrides = {}) => {
    const starts = [];
    const cancellations = [];
    const logs = [];
    let handler;
    let unsubscribed = false;
    const jobs = createHistoryTranscriptionJobs({
        api: {
            available: true,
            eventsAvailable: true,
            getHistoryTranscriptionStatus: async ({ id }) => ({
                history_id: id,
                transcript_available: false,
                can_transcribe: true,
                progress: null,
            }),
            startHistoryTranscription: async args => {
                starts.push(args);
                return progress();
            },
            cancelHistoryTranscription: async args => cancellations.push(args),
            onHistoryTranscription: async listener => {
                handler = listener;
                return () => {
                    unsubscribed = true;
                };
            },
            ...overrides,
        },
        appendLog: (...args) => logs.push(args),
    });
    jobs.bindEvents();
    return {
        jobs,
        starts,
        cancellations,
        logs,
        emit: value => handler({ payload: value }),
        unsubscribed: () => unsubscribed,
    };
};

test('transcription sources and failures use actionable messages without raw process output', () => {
    assert.equal(transcriptionActive(progress()), true);
    for (const status of ['completed', 'failed', 'cancelled'])
        assert.equal(transcriptionActive(progress(status)), false);
    assert.match(transcriptionErrorMessage('The saved video is unavailable.'), /Restore.*file/);
    assert.match(transcriptionErrorMessage('Several downloaded videos match this transcript.'), /original video/);
    assert.match(transcriptionErrorMessage('The local Whisper runtime is unavailable.'), /Whisper.*Settings/);
    assert.match(transcriptionErrorMessage('The saved video changed.'), /changed/);
    assert.doesNotMatch(
        transcriptionErrorMessage('Traceback: /Users/private/model token=secret'),
        /Users|secret|Traceback/
    );
});

test('duplicate starts share one native attempt and a second video cannot start while busy', async () => {
    const reply = deferred();
    let count = 0;
    const { jobs } = fixture({
        startHistoryTranscription: () => {
            count++;
            return reply.promise;
        },
    });
    await jobs.load('video-a');
    const first = jobs.start('video-a');
    assert.equal(jobs.start('video-a'), first);
    await Promise.resolve();
    assert(transcriptionActive(jobs.get('video-a').progress));
    await jobs.start('video-b');
    assert.match(jobs.get('video-b').error, /Another transcription/);
    assert.equal(count, 1);
    reply.resolve(progress());
    await first;
    await jobs.start('video-a');
    assert.equal(count, 1);
});

test('completion survives subscriber navigation and delayed start/status replies cannot regress it', async () => {
    const start = deferred();
    const read = deferred();
    const { jobs, emit } = fixture({
        startHistoryTranscription: () => start.promise,
        getHistoryTranscriptionStatus: () => read.promise,
    });
    const seen = [];
    const leave = jobs.subscribe((id, value) => seen.push([id, value.progress?.status]));
    const loading = jobs.load('video-a');
    await Promise.resolve();
    const starting = jobs.start('video-a');
    await Promise.resolve();
    leave();
    emit(progress('completed'));
    start.resolve(progress());
    read.resolve({ history_id: 'video-a', can_transcribe: true, transcript_available: false, progress: null });
    await Promise.all([loading, starting]);
    assert.equal(jobs.get('video-a').progress.status, 'completed');
    assert.equal(jobs.get('video-a').transcript_available, true);
    assert.equal(jobs.get('video-a').can_transcribe, false);
    assert.equal(
        seen.some(([, status]) => status === 'completed'),
        false
    );
});

test('failed launches can retry; cancellation uses the exact native attempt and stale events are ignored', async () => {
    let count = 0;
    const { jobs, emit, cancellations } = fixture({
        startHistoryTranscription: async () => {
            if (++count === 1) throw new Error('Python runtime unavailable');
            return progress('running', { attempt_id: 'retry', attempt_at: 200 });
        },
    });
    await jobs.load('video-a');
    await jobs.start('video-a');
    assert.match(jobs.get('video-a').error, /tools are unavailable/);
    await jobs.start('video-a');
    await jobs.cancel('video-a');
    assert.deepEqual(cancellations, [{ id: 'video-a', attemptId: 'retry' }]);
    emit(progress('cancelled', { attempt_id: 'retry', attempt_at: 200 }));
    emit(progress('running', { attempt_id: 'retry', attempt_at: 200 }));
    emit(progress('completed'));
    assert.equal(jobs.get('video-a').progress.status, 'cancelled');
    assert.equal(jobs.get('video-a').transcript_available, false);
});

test('status errors retain an actionable recheck and disposing unsubscribes without cancelling native work', async () => {
    const { jobs, starts, cancellations, unsubscribed } = fixture({
        getHistoryTranscriptionStatus: async () => {
            throw new Error('Synthetic status read error');
        },
    });
    await jobs.load('video-a');
    assert.match(jobs.get('video-a').error, /Check again/);
    await jobs.start('video-a');
    assert.equal(starts.length, 1);
    jobs.dispose();
    assert.equal(unsubscribed(), true);
    assert.deepEqual(cancellations, []);
});

test('late progress revisions never replace a newer phase or terminal result', () => {
    const { jobs, emit } = fixture();
    emit(progress('running', { revision: 2, phase: 'saving' }));
    emit(progress('running', { revision: 1, phase: 'preparing' }));
    assert.equal(jobs.get('video-a').progress.phase, 'saving');
    emit(progress('completed', { revision: 3, phase: 'completed' }));
    emit(progress('failed', { revision: 2, message: 'Late failure.' }));
    assert.equal(jobs.get('video-a').progress.status, 'completed');
    assert.equal(jobs.get('video-a').transcript_available, true);
});

test('cancellation losing a completion race reloads the saved result instead of leaving an error', async () => {
    let completed = false;
    const { jobs } = fixture({
        getHistoryTranscriptionStatus: async () => ({
            history_id: 'video-a',
            can_transcribe: !completed,
            transcript_available: completed,
            progress: completed ? progress('completed') : null,
        }),
        cancelHistoryTranscription: async () => {
            completed = true;
            throw new Error('This transcription is no longer running.');
        },
    });
    await jobs.load('video-a');
    await jobs.start('video-a');
    await jobs.cancel('video-a');
    assert.equal(jobs.get('video-a').progress.status, 'completed');
    assert.equal(jobs.get('video-a').transcript_available, true);
    assert.equal(jobs.get('video-a').error, '');
});
