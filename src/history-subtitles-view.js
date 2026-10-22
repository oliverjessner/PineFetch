import { renderContentDetails } from './history-content-details.js';

export const createHistorySubtitlesView = ({ els, api, appendLog }) => {
    let entryId = null;
    let tracks = [];
    let revision = 0;
    let current = null;
    const cache = new Map();

    const render = content => {
        current = content;
        els.historySubtitleCopyBtn.disabled = !content?.text;
        els.historySubtitleText.hidden = !content?.text;
        els.historySubtitleText.textContent = content?.text || '';
        renderContentDetails(
            els.historySubtitleDetails,
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
        cache.clear();
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
        els.historySubtitleCopyBtn.addEventListener('click', async () => {
            if (!current?.text) return;
            const request = revision;
            try {
                await navigator.clipboard.writeText(current.text);
                if (request === revision) els.historySubtitleStatus.textContent = 'Subtitles copied.';
            } catch {
                if (request === revision) els.historySubtitleStatus.textContent = 'Subtitles could not be copied.';
            }
        });
    };
    return Object.freeze({ reset, load, bindEvents, deactivate: () => revision++ });
};
