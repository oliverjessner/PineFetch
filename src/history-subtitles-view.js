import { createTextReader } from './history-text-view.js';
import { renderContentMetadata } from './history-content-details.js';

export const createHistorySubtitlesView = ({ els, api, appendLog, media }) => {
    let entryId = null;
    let tracks = [];
    let revision = 0;
    let sourceStartSeconds = 0;
    let playbackTime = 0;
    const reader = createTextReader({
        input: els.historySubtitleSearchInput,
        copy: els.historySubtitleCopyBtn,
        status: els.historySubtitleStatus,
        count: els.historySubtitleResultCount,
        viewport: els.historySubtitleListViewport,
        list: els.historySubtitleText,
        appendLog,
        onSelect: seconds => {
            playbackTime = Math.max(0, seconds - sourceStartSeconds);
            media.seek(playbackTime);
        },
    });
    const cache = new Map();

    const render = content => {
        const start = Number(content?.source_start_seconds);
        sourceStartSeconds = Number.isFinite(start) ? Math.max(0, start) : 0;
        reader.set(content?.text);
        if (content?.text) reader.sync(playbackTime + sourceStartSeconds);
        renderContentMetadata(
            els.historySubtitleMeta,
            content
                ? [
                      ['File path', content.path],
                      ['Language', content.language],
                      ['Format', content.format.toUpperCase()],
                  ]
                : []
        );
        const notices = [];
        if (content && !content.file_available)
            notices.push('The subtitle file is missing. Saved text is shown below.');
        if (content?.source_start_seconds > 0)
            notices.push(
                `Original timing is preserved. The downloaded media starts at ${content.source_start_seconds}s in the full video.`
            );
        els.historySubtitleNotice.textContent = notices.join(' ');
        els.historySubtitleNotice.hidden = !notices.length;
    };

    const load = async () => {
        const path = els.historySubtitleTrackSelect.value;
        const request = ++revision;
        render(null);
        els.historySubtitleRetryBtn.hidden = true;
        els.historySubtitleStatus.classList.remove('oj-status-error');
        if (!entryId || !tracks.length) {
            els.historySubtitleStatus.textContent =
                'No creator subtitles were saved for this video. Automatically generated captions are excluded from Save creator subtitles.';
            return;
        }
        els.historySubtitleStatus.textContent = 'Loading subtitles…';
        try {
            const content = cache.has(path) ? cache.get(path) : await api.getHistorySubtitle({ id: entryId, path });
            if (request !== revision) return;
            cache.set(path, content);
            render(content);
            els.historySubtitleStatus.textContent = content?.text
                ? ''
                : 'No subtitle text is available for this track.';
        } catch (error) {
            if (request !== revision) return;
            appendLog(`[subtitles] ${error}`, true);
            els.historySubtitleStatus.textContent = 'Subtitles could not be loaded. Try again.';
            els.historySubtitleStatus.classList.add('oj-status-error');
            els.historySubtitleRetryBtn.hidden = false;
        }
    };

    const reset = (id = null, nextTracks = []) => {
        revision++;
        entryId = id;
        tracks = nextTracks;
        sourceStartSeconds = 0;
        playbackTime = 0;
        cache.clear();
        reader.reset();
        render(null);
        els.historySubtitleRetryBtn.hidden = true;
        els.historySubtitleStatus.classList.remove('oj-status-error');
        els.historySubtitleStatus.textContent = '';
        const fragment = document.createDocumentFragment();
        for (const track of tracks) {
            const option = document.createElement('option');
            option.value = track.path;
            option.textContent = `${track.language} · ${track.format.toUpperCase()}`;
            fragment.appendChild(option);
        }
        els.historySubtitleTrackSelect.replaceChildren(fragment);
        els.historySubtitleTrackField.hidden = !tracks.length;
    };

    const bindEvents = () => {
        els.historySubtitleRetryBtn.addEventListener('click', () => void load());
        els.historySubtitleTrackSelect.addEventListener('change', () => void load());
    };
    return Object.freeze({
        reset,
        load,
        bindEvents,
        syncTime: seconds => {
            playbackTime = seconds;
            reader.sync(playbackTime + sourceStartSeconds);
        },
        deactivate: () => {
            revision++;
            reader.deactivate();
        },
    });
};
