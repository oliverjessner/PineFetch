// Transcript and OCR share the saved video authorized by its History ID.
export const createHistoryMediaView = ({ root, api, onTime = () => {}, appendLog = () => {} }) => {
    const video = root.querySelector('video');
    const fallback = root.querySelector('[data-media-fallback]');
    const status = root.querySelector('[data-media-status]');
    let entry = null;
    let revision = 0;
    let requested = false;
    let pending = 0;
    let image = '';
    let localState = 'idle';
    let compatibilityAttempted = false;
    let unavailableNotified = false;
    let loadTimer;
    let leaseId = null;
    const releaseLease = id => {
        if (!id) return;
        void api.releaseHistoryMedia({ leaseId: id }).catch(() => {
            appendLog(
                '[local playback] Could not release the playback preview. Reopen PineFetch to retry cleanup.',
                true
            );
        });
    };
    const clearLoadTimer = () => clearTimeout(loadTimer);
    const renderFallback = () => {
        // Keep the selected OCR frame visible until local video decoding succeeds.
        const showImage = localState !== 'ready' && Boolean(image);
        fallback.hidden = !showImage;
        if (showImage && fallback.getAttribute('src') !== image) fallback.src = image;
        status.hidden = localState === 'ready' || showImage;
    };
    const notifyUnavailable = () => {
        if (unavailableNotified) return;
        unavailableNotified = true;
        root.dispatchEvent(new CustomEvent('pinefetch:media-unavailable'));
    };
    const stopVideo = () => {
        clearLoadTimer();
        video.hidden = true;
        video.pause();
        video.removeAttribute('src');
        video.load();
        const previous = leaseId;
        leaseId = null;
        releaseLease(previous);
    };
    const fallbackPlayer = message => {
        localState = 'unavailable';
        stopVideo();
        status.textContent = message;
        renderFallback();
        notifyUnavailable();
    };
    const loadVideo = (source, request) => {
        let url;
        try {
            url = source?.path && api.fileSource(source.path);
            if (!url) throw new Error('The local playback file could not be opened.');
        } catch (error) {
            releaseLease(source?.lease_id);
            throw error;
        }
        stopVideo();
        leaseId = source.lease_id || null;
        localState = 'loading';
        video.hidden = false;
        video.preload = 'auto';
        renderFallback();
        video.src = url;
        video.load();
        // Some platform decoders never report unsupported media as an error.
        loadTimer = setTimeout(() => {
            if (request === revision && localState === 'loading') void recoverPlayback();
        }, 12000);
    };
    const recoverPlayback = async () => {
        if (compatibilityAttempted) {
            fallbackPlayer('The local playback video could not be loaded. Reopen this tab to try again.');
            return;
        }
        compatibilityAttempted = true;
        const request = revision;
        if (localState === 'ready' && Number.isFinite(video.currentTime)) pending = video.currentTime;
        localState = 'preparing';
        stopVideo();
        status.textContent = 'Preparing local video for playback… This can take a few minutes.';
        renderFallback();
        notifyUnavailable();
        try {
            const source = await api.getHistoryMedia({ id: entry.id, compatible: true });
            if (request !== revision) {
                releaseLease(source?.lease_id);
                return;
            }
            status.textContent = 'Loading local playback video…';
            loadVideo(source, request);
        } catch (error) {
            appendLog(`[local playback] ${error}`, true);
            if (request === revision)
                fallbackPlayer('The video could not be prepared for local playback. See Settings → Terminal Log.');
        }
    };
    const seek = seconds => {
        pending = Math.max(0, Number(seconds) || 0);
        if (localState === 'loading' || localState === 'ready') {
            if (video.readyState >= 1)
                video.currentTime = Number.isFinite(video.duration) ? Math.min(pending, video.duration) : pending;
        }
    };
    video.addEventListener('loadedmetadata', () => {
        if (localState === 'loading') seek(pending);
    });
    const showDecodedVideo = () => {
        if (localState !== 'loading' || video.readyState < 2 || !video.videoWidth) return;
        clearLoadTimer();
        localState = 'ready';
        unavailableNotified = false;
        renderFallback();
    };
    video.addEventListener('loadeddata', showDecodedVideo);
    video.addEventListener('canplay', showDecodedVideo);
    video.addEventListener('timeupdate', () => {
        if (localState === 'ready') onTime(video.currentTime);
    });
    video.addEventListener('error', () => {
        if (localState === 'loading' || localState === 'ready') void recoverPlayback();
    });
    fallback.addEventListener('error', () => {
        image = '';
        fallback.hidden = true;
        renderFallback();
    });
    return Object.freeze({
        setEntry: next => {
            revision++;
            clearLoadTimer();
            localState = 'idle';
            compatibilityAttempted = false;
            unavailableNotified = false;
            entry = next;
            requested = false;
            pending = 0;
            image = '';
            video.removeAttribute('poster');
            stopVideo();
            fallback.removeAttribute('src');
            fallback.hidden = true;
            status.hidden = false;
            status.textContent = 'Select a timestamp to navigate the saved video.';
        },
        activate: async () => {
            if (requested || !entry?.id) return;
            requested = true;
            compatibilityAttempted = false;
            unavailableNotified = false;
            const request = revision;
            status.hidden = false;
            status.textContent = 'Loading saved video…';
            try {
                const source = await api.getHistoryMedia({ id: entry.id });
                if (request !== revision) {
                    releaseLease(source?.lease_id);
                    return;
                }
                if (!source?.path) {
                    releaseLease(source?.lease_id);
                    fallbackPlayer('The saved video could not be opened. Check that the local file is available.');
                    return;
                }
                loadVideo(source, request);
            } catch (error) {
                appendLog(`[local playback] ${error}`, true);
                if (request === revision)
                    fallbackPlayer(
                        'No saved video is available. Keep the video with the transcript or download the video to play it here.'
                    );
            }
        },
        seek,
        hasVideo: () => localState === 'ready',
        setFrame: src => {
            image = src || '';
            renderFallback();
        },
        pause: () => {
            revision++;
            clearLoadTimer();
            if (localState !== 'ready') {
                requested = false;
                localState = 'idle';
                stopVideo();
            }
            video.pause();
        },
    });
};
