// Local transcription belongs to the app, not to an open tab or video view.
export const transcriptionActive = progress => progress?.status === 'running';

export const transcriptionErrorMessage = error => {
    const message = String(error?.message || error || '');
    if (/already.*transcript|transcript.*already/i.test(message))
        return 'A transcript is already saved for this video.';
    if (/already running|another.*transcri|busy/i.test(message))
        return 'Another transcription is running. Wait for it to finish, then try again.';
    if (/several|ambiguous|multiple.*video/i.test(message))
        return 'Several saved videos match. Open the original video’s History entry to transcribe it.';
    if (/no.*audio|audio.*stream/i.test(message)) return 'This video has no audio track to transcribe.';
    if (/no.*speech|empty.*transcript|no usable transcript/i.test(message))
        return 'No speech was detected. No transcript was saved.';
    if (/macOS 14|macOS.*later/i.test(message)) return 'Local transcription requires macOS 14 or later.';
    if (/temporary|storage|could not be saved/i.test(message))
        return 'The transcript could not be saved. Check available disk space and try again.';
    if (/whisper|python|ffmpeg|ffprobe|runtime|helper|model.*(missing|unavailable)/i.test(message))
        return 'Local transcription tools are unavailable. Check Whisper in Settings and try again.';
    if (/not.*video|supported.*video|video.*required|video.*container/i.test(message))
        return 'Transcription requires a saved video with an audio track.';
    if (
        /missing|unavailable|no downloaded|no saved|not found|does not exist|unreadable|no longer|path.*invalid/i.test(
            message
        )
    )
        return 'The saved video is unavailable. Restore the local video file, then check again.';
    if (/changed/i.test(message)) return 'The saved video changed. Check the local file and try again.';
    if (/cancel/i.test(message)) return 'Transcription cancelled. You can try again.';
    return 'Transcription could not be completed. Try again; technical details are in Settings logs.';
};

export const createHistoryTranscriptionJobs = ({ api, appendLog = () => {} }) => {
    const records = new Map();
    const revisions = new Map();
    const listeners = new Set();
    const reads = new Map();
    const starts = new Map();
    let disposed = false;
    let cleanup;
    const get = id => records.get(id) || null;
    const revision = id => revisions.get(id) || 0;
    const publish = (id, value) => {
        const record = { ...value, history_id: id };
        records.set(id, record);
        revisions.set(id, revision(id) + 1);
        for (const listener of listeners) listener(id, record);
    };
    const adopt = progress => {
        const id = progress?.history_id;
        if (!id || !progress.attempt_id || !['running', 'completed', 'failed', 'cancelled'].includes(progress.status))
            return;
        const previous = get(id);
        const current = previous?.progress;
        if (current?.attempt_id) {
            if (
                current.attempt_id !== progress.attempt_id &&
                Number(progress.attempt_at || 0) <= Number(current.attempt_at || 0)
            )
                return;
            if (
                current.attempt_id === progress.attempt_id &&
                !transcriptionActive(current) &&
                transcriptionActive(progress)
            )
                return;
            if (
                current.attempt_id === progress.attempt_id &&
                Number.isFinite(progress.revision) &&
                Number.isFinite(current.revision) &&
                progress.revision <= current.revision
            )
                return;
        }
        if (progress.status === 'failed' && progress.message && current?.message !== progress.message)
            appendLog(`[transcription ${id}] ${progress.message}`, true);
        publish(id, {
            ...previous,
            history_id: id,
            loading: false,
            cancelling: Boolean(previous?.cancelling && transcriptionActive(progress)),
            error: '',
            ...(progress.status === 'completed' ? { transcript_available: true, can_transcribe: false } : {}),
            progress: { ...(current?.attempt_id === progress.attempt_id ? current : {}), ...progress },
        });
    };
    const load = id => {
        if (!id || !api.available || typeof api.getHistoryTranscriptionStatus !== 'function')
            return Promise.resolve(get(id));
        if (reads.has(id)) return reads.get(id);
        const request = Promise.resolve().then(async () => {
            publish(id, { ...get(id), history_id: id, loading: true, error: '' });
            const expected = revision(id);
            try {
                const snapshot = await api.getHistoryTranscriptionStatus({ id });
                if (disposed || revision(id) !== expected) return get(id);
                publish(id, {
                    ...snapshot,
                    history_id: id,
                    loading: false,
                    cancelling: Boolean(get(id)?.cancelling && transcriptionActive(snapshot.progress)),
                    error: '',
                });
                return get(id);
            } catch (error) {
                appendLog(`[transcription status] ${error}`, true);
                if (!disposed && revision(id) === expected)
                    publish(id, {
                        ...get(id),
                        loading: false,
                        error: 'The saved video could not be checked. Check again.',
                    });
                return get(id);
            } finally {
                reads.delete(id);
                if (!disposed && get(id)?.loading) publish(id, { ...get(id), loading: false });
            }
        });
        reads.set(id, request);
        return request;
    };
    const start = id => {
        if (starts.has(id)) return starts.get(id);
        if (transcriptionActive(get(id)?.progress)) return Promise.resolve(get(id));
        if (starts.size || [...records.values()].some(record => transcriptionActive(record.progress))) {
            publish(id, { ...get(id), error: transcriptionErrorMessage('Another transcription is running.') });
            return Promise.resolve(get(id));
        }
        const previous = get(id);
        const request = Promise.resolve().then(async () => {
            publish(id, {
                ...get(id),
                history_id: id,
                error: '',
                progress: { history_id: id, status: 'running', phase: 'preparing', attempt_id: null },
            });
            try {
                adopt(await api.startHistoryTranscription({ id }));
            } catch (error) {
                appendLog(`[transcription] ${error}`, true);
                if (!get(id)?.progress?.attempt_id)
                    publish(id, { ...previous, history_id: id, error: transcriptionErrorMessage(error) });
            } finally {
                starts.delete(id);
            }
            return get(id);
        });
        starts.set(id, request);
        return request;
    };
    const cancel = async id => {
        const progress = get(id)?.progress;
        if (!transcriptionActive(progress) || !progress.attempt_id || get(id)?.cancelling) return;
        publish(id, { ...get(id), cancelling: true, error: '' });
        try {
            const cancelled = await api.cancelHistoryTranscription({ id, attemptId: progress.attempt_id });
            if (cancelled?.attempt_id) adopt(cancelled);
            else await load(id);
        } catch (error) {
            appendLog(`[transcription cancellation] ${error}`, true);
            // Completion may have won the race with cancellation. Recover the
            // authoritative terminal state even if its event was missed.
            await load(id);
            if (get(id)?.progress?.attempt_id === progress.attempt_id && transcriptionActive(get(id)?.progress))
                publish(id, {
                    ...get(id),
                    cancelling: false,
                    error: 'Cancellation could not be confirmed. Try again.',
                });
        }
    };
    return Object.freeze({
        get,
        load,
        start,
        cancel,
        subscribe: listener => {
            listeners.add(listener);
            return () => listeners.delete(listener);
        },
        bindEvents: () => {
            if (!api.eventsAvailable || typeof api.onHistoryTranscription !== 'function') return;
            void api
                .onHistoryTranscription(event => !disposed && adopt(event.payload))
                .then(unsubscribe => {
                    if (disposed) unsubscribe();
                    else cleanup = unsubscribe;
                })
                .catch(error => appendLog(`[transcription events] ${error}`, true));
        },
        dispose: () => {
            disposed = true;
            cleanup?.();
            listeners.clear();
        },
    });
};
