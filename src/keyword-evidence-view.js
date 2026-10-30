import { createTextReader } from './history-text-view.js';
import { createHistoryMediaView } from './history-media-view.js';
import { textTimestamp } from './history-text.js';

const sourceLabels = { transcript: 'Transcript', ocr: 'OCR', captions: 'Caption', subtitles: 'Creator subtitles' };
export const createKeywordEvidenceView = ({ root, api, appendLog }) => {
    const panel = document.createElement('section');
    panel.className = 'oj-stack pinefetch-keyword-evidence';
    panel.setAttribute('aria-label', 'Keyword occurrences');
    panel.innerHTML = `<div class="oj-cluster"><h4 class="oj-heading-3 oj-grow" tabindex="-1" data-evidence-title></h4><button type="button" class="oj-button oj-button-ghost" data-evidence-close>Close occurrences</button></div>
        <p class="oj-helper" role="status" data-evidence-status></p><ol class="oj-stack" data-evidence-list></ol><div class="oj-cluster"><button type="button" class="oj-button oj-button-ghost" data-evidence-prev>Previous occurrences</button><button type="button" class="oj-button oj-button-ghost" data-evidence-next>Next occurrences</button></div>
        <section class="oj-stack" data-evidence-context hidden><h4 class="oj-heading-3" data-source-title></h4><p class="oj-helper" data-source-notice></p><button type="button" class="oj-button oj-button-secondary" data-source-play hidden></button>
        <div class="pinefetch-history-media" data-source-player><div class="pinefetch-media-stage"><video controls playsinline preload="metadata" aria-label="Saved source video" hidden></video><img data-media-fallback alt="Selected local video frame" hidden><p class="oj-helper" data-media-status></p></div></div>
        <div class="oj-cluster"><label class="oj-field oj-grow">Search source text<input class="oj-input" type="search" data-source-search disabled></label><button type="button" class="oj-button oj-button-secondary" data-source-copy disabled>Copy All Text</button></div><p class="oj-helper" data-source-count></p><p class="oj-helper" role="status" data-source-status></p><div class="pinefetch-text-viewport" tabindex="0" aria-label="Original keyword source" data-source-viewport><ul class="pinefetch-text-entries" data-source-list></ul></div></section>`;
    root.append(panel);
    panel.hidden = true;
    const element = name => panel.querySelector(`[data-${name}]`);
    let entryId = null;
    let generation = 0;
    let sourceRequest = 0;
    let term = '';
    let filter = {};
    let offset = 0;
    let nextOffset = 0;
    let returnFocus;
    let playback = null;
    const media = createHistoryMediaView({ root: element('source-player'), api, appendLog });
    const reader = createTextReader({
        input: element('source-search'),
        copy: element('source-copy'),
        status: element('source-status'),
        count: element('source-count'),
        viewport: element('source-viewport'),
        list: element('source-list'),
        appendLog,
    });
    const label = item =>
        `${sourceLabels[item.source]}${item.language ? ` · ${item.language}` : ''}${item.timestamp_ms == null ? '' : ` · ${textTimestamp(item.timestamp_ms)}`}`;
    const clearSource = () => {
        sourceRequest++;
        playback = null;
        reader.reset();
        media.setEntry(entryId ? { id: entryId } : null);
        element('evidence-context').hidden = true;
    };
    const openSource = async item => {
        clearSource();
        const request = ++sourceRequest;
        const id = entryId;
        element('evidence-status').textContent = 'Loading the saved source…';
        try {
            const source = await api.getHistoryKeywordSource({
                id,
                sourceKey: item.source_key,
                revision: item.revision,
            });
            if (request !== sourceRequest || id !== entryId || panel.hidden) return;
            element('evidence-context').hidden = false;
            element('source-title').textContent =
                `${label(item)}${source.format ? ` · ${source.format.toUpperCase()}` : ''}`;
            element('source-notice').textContent =
                source.source_start_seconds > 0
                    ? `Original source timing is preserved. The saved video starts at ${source.source_start_seconds}s.`
                    : 'Original saved text. Search highlights and copying preserve the source.';
            reader.set(source.text, { raw: true });
            reader.reveal({ query: term, offset: item.offset_utf16 });
            const seconds = item.timestamp_ms == null ? null : item.timestamp_ms / 1000 - source.source_start_seconds;
            playback = seconds !== null && seconds >= 0 ? seconds : null;
            element('source-play').hidden = playback === null;
            element('source-play').textContent = `Play saved video at ${textTimestamp((seconds || 0) * 1000)}`;
            element('source-player').hidden = true;
            element('evidence-status').textContent =
                seconds !== null && seconds < 0
                    ? 'This occurrence is before the retained video cut. Its original text is shown.'
                    : '';
        } catch (error) {
            if (request !== sourceRequest || id !== entryId || panel.hidden) return;
            clearSource();
            appendLog(`[Keyword source] ${error}`, true);
            element('evidence-status').textContent =
                'The saved source could not be opened or changed. Refresh keywords and select the occurrence again.';
        }
    };
    const load = async () => {
        const request = ++generation;
        const id = entryId;
        clearSource();
        panel.hidden = false;
        panel.setAttribute('aria-busy', 'true');
        element('evidence-title').textContent = `Occurrences of “${term}”`;
        element('evidence-status').textContent = 'Loading saved occurrences…';
        element('evidence-list').replaceChildren();
        element('evidence-prev').disabled = true;
        element('evidence-next').disabled = true;
        try {
            const page = await api.getHistoryKeywordOccurrences({
                id,
                query: { term, source: filter.source || null, language: filter.language || null, offset },
            });
            if (request !== generation || id !== entryId) return;
            nextOffset = page.next_offset;
            element('evidence-status').textContent =
                `${page.total.toLocaleString()} occurrences${page.total ? ` · ${offset + 1}–${page.next_offset} shown` : ''}.`;
            const fragment = document.createDocumentFragment();
            for (const item of page.entries) {
                const row = document.createElement('li');
                const open = document.createElement('button');
                open.type = 'button';
                open.className = 'oj-button oj-button-ghost';
                open.textContent = `${label(item)} · Show source`;
                open.addEventListener('click', () => void openSource(item));
                const snippet = document.createElement('p');
                snippet.className = 'oj-text';
                snippet.textContent = item.snippet;
                row.append(open, snippet);
                fragment.append(row);
            }
            element('evidence-list').replaceChildren(fragment);
            element('evidence-prev').disabled = offset === 0;
            element('evidence-next').disabled = !page.has_more;
        } catch (error) {
            if (request !== generation || id !== entryId) return;
            appendLog(`[Keyword occurrences] ${error}`, true);
            element('evidence-status').textContent = 'Occurrences could not be loaded. Select the keyword to retry.';
        } finally {
            if (request === generation) panel.setAttribute('aria-busy', 'false');
        }
    };
    const close = () => {
        generation++;
        clearSource();
        panel.hidden = true;
    };
    element('evidence-close').addEventListener('click', () => {
        close();
        returnFocus?.focus();
    });
    element('evidence-prev').addEventListener('click', () => {
        offset = Math.max(0, offset - 100);
        void load();
    });
    element('evidence-next').addEventListener('click', () => {
        offset = nextOffset;
        void load();
    });
    element('source-play').addEventListener('click', () => {
        if (playback !== null) {
            element('source-player').hidden = false;
            void media.activate();
            media.seek(playback);
        }
    });
    return Object.freeze({
        reset: id => {
            close();
            entryId = id;
            media.setEntry(id ? { id } : null);
        },
        close,
        open: (word, source, trigger) => {
            term = word;
            filter = source;
            offset = 0;
            returnFocus = trigger;
            void load();
            element('evidence-title').focus();
        },
        deactivate: () => {
            generation++;
            sourceRequest++;
            reader.deactivate();
            media.pause();
        },
    });
};
