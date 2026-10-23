// Only known YouTube video URLs can become remote player URLs.
export const youtubeEmbedSource = (value, seconds = 0) => {
    try {
        const url = new URL(value);
        if (url.protocol !== 'https:' && url.protocol !== 'http:') return null;
        const host = url.hostname.toLowerCase();
        let id;
        if (host === 'youtu.be') id = url.pathname.split('/')[1];
        else if (['youtube.com', 'www.youtube.com', 'm.youtube.com'].includes(host)) {
            id =
                url.pathname === '/watch'
                    ? url.searchParams.get('v')
                    : /^\/(?:shorts|live|embed)\//.test(url.pathname)
                      ? url.pathname.split('/')[2]
                      : null;
        }
        if (!/^[\w-]{11}$/.test(id || '')) return null;
        return `https://www.youtube-nocookie.com/embed/${id}?start=${Math.max(0, Math.floor(Number(seconds) || 0))}&rel=0`;
    } catch {
        return null;
    }
};
export const createHistoryMediaView = ({ root, api, onTime = () => {} }) => {
    const video = root.querySelector('video');
    const embed = root.querySelector('iframe');
    const fallback = root.querySelector('[data-media-fallback]');
    const status = root.querySelector('[data-media-status]');
    let entry = null;
    let revision = 0;
    let requested = false;
    let pending = 0;
    let image = '';
    let localState = 'idle';
    let loadTimer;
    const clearLoadTimer = () => clearTimeout(loadTimer);
    const renderFallback = () => {
        // A decoded OCR frame takes precedence over an unverified remote embed.
        const showImage = localState !== 'ready' && Boolean(image);
        fallback.hidden = !showImage;
        if (showImage && fallback.getAttribute('src') !== image) fallback.src = image;
        if (showImage) {
            embed.removeAttribute('src');
            embed.hidden = true;
        }
        status.hidden = localState === 'ready' || showImage || !embed.hidden;
    };
    const fallbackPlayer = () => {
        clearLoadTimer();
        localState = 'unavailable';
        video.hidden = true;
        video.pause();
        video.removeAttribute('src');
        video.load();
        const source = !image && youtubeEmbedSource(entry?.url, pending);
        embed.hidden = !source;
        if (source) embed.src = source;
        status.textContent = 'This video could not be played in the app.';
        renderFallback();
        root.dispatchEvent(new CustomEvent('pinefetch:media-unavailable'));
    };
    const seek = seconds => {
        pending = Math.max(0, Number(seconds) || 0);
        if (localState === 'loading' || localState === 'ready') {
            if (video.readyState >= 1)
                video.currentTime = Number.isFinite(video.duration) ? Math.min(pending, video.duration) : pending;
        } else if (!embed.hidden) embed.src = youtubeEmbedSource(entry?.url, pending);
    };
    video.addEventListener('loadedmetadata', () => {
        if (localState === 'loading') seek(pending);
    });
    const showDecodedVideo = () => {
        if (localState !== 'loading' || video.readyState < 2 || !video.videoWidth) return;
        clearLoadTimer();
        localState = 'ready';
        renderFallback();
    };
    video.addEventListener('loadeddata', showDecodedVideo);
    video.addEventListener('canplay', showDecodedVideo);
    video.addEventListener('timeupdate', () => {
        if (localState === 'ready') onTime(video.currentTime);
    });
    video.addEventListener('error', () => {
        if (localState === 'loading' || localState === 'ready') fallbackPlayer();
    });
    fallback.addEventListener('error', () => {
        image = '';
        fallback.hidden = true;
        status.hidden = false;
    });
    return Object.freeze({
        setEntry: next => {
            revision++;
            clearLoadTimer();
            localState = 'idle';
            entry = next;
            requested = false;
            pending = 0;
            image = '';
            video.pause();
            video.poster = next?.thumbnail || '';
            video.removeAttribute('src');
            video.load();
            video.hidden = true;
            embed.removeAttribute('src');
            embed.hidden = true;
            fallback.removeAttribute('src');
            fallback.hidden = true;
            status.hidden = false;
            status.textContent = 'Select a timestamp to navigate the video.';
        },
        activate: async () => {
            if (requested || !entry?.id) return;
            requested = true;
            const request = revision;
            status.hidden = false;
            status.textContent = 'Loading saved video…';
            try {
                const path = await api.getHistoryMedia({ id: entry.id });
                if (request !== revision) return;
                const url = path && api.fileSource(path);
                if (!url) {
                    fallbackPlayer();
                    return;
                }
                localState = 'loading';
                video.hidden = false;
                embed.hidden = true;
                video.preload = 'auto';
                renderFallback();
                video.src = url;
                // Some native decoders never emit an error for unsupported media.
                loadTimer = setTimeout(() => {
                    if (request === revision && localState === 'loading') fallbackPlayer();
                }, 12000);
            } catch {
                if (request === revision) fallbackPlayer();
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
                video.removeAttribute('src');
                video.load();
                video.hidden = true;
            }
            video.pause();
            if (!embed.hidden) {
                embed.removeAttribute('src');
                requested = false;
                embed.hidden = true;
            }
        },
    });
};
