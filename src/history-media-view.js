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
    const fallbackPlayer = () => {
        video.hidden = true;
        video.pause();
        const source = youtubeEmbedSource(entry?.url, pending);
        embed.hidden = !source;
        if (source) embed.src = source;
        fallback.hidden = Boolean(source) || !image;
        if (image) fallback.src = image;
        status.hidden = Boolean(source) || Boolean(image);
        status.textContent = 'No playable saved video is available for this entry.';
        root.dispatchEvent(new CustomEvent('pinefetch:media-unavailable'));
    };
    const seek = seconds => {
        pending = Math.max(0, Number(seconds) || 0);
        if (!video.hidden) {
            if (video.readyState >= 1)
                video.currentTime = Number.isFinite(video.duration) ? Math.min(pending, video.duration) : pending;
        } else if (!embed.hidden) embed.src = youtubeEmbedSource(entry?.url, pending);
    };
    video.addEventListener('loadedmetadata', () => seek(pending));
    video.addEventListener('timeupdate', () => {
        if (!video.hidden) onTime(video.currentTime);
    });
    video.addEventListener('error', fallbackPlayer);
    fallback.addEventListener('error', () => {
        fallback.hidden = true;
        status.hidden = false;
    });
    return Object.freeze({
        setEntry: next => {
            revision++;
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
            status.textContent = 'Loading saved video…';
            try {
                const path = await api.getHistoryMedia({ id: entry.id });
                if (request !== revision) return;
                const url = path && api.fileSource(path);
                if (!url) {
                    fallbackPlayer();
                    return;
                }
                video.hidden = false;
                embed.hidden = true;
                fallback.hidden = true;
                status.hidden = true;
                video.src = url;
            } catch {
                if (request === revision) fallbackPlayer();
            }
        },
        seek,
        hasVideo: () => !video.hidden,
        setFrame: src => {
            image = src || '';
            if (video.hidden && image) {
                embed.removeAttribute('src');
                embed.hidden = true;
                fallback.hidden = false;
                if (fallback.getAttribute('src') !== image) fallback.src = image;
                status.hidden = true;
            } else if (video.hidden && embed.hidden) {
                fallback.hidden = true;
            }
        },
        pause: () => {
            revision++;
            if (video.hidden) requested = false;
            video.pause();
            if (!embed.hidden) {
                embed.removeAttribute('src');
                requested = false;
                embed.hidden = true;
            }
        },
    });
};
