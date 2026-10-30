import { createKeywordEvidenceView } from './keyword-evidence-view.js';
import { searchKey, trimSearchQuery } from './unicode-search.js';
import { createDropdownChoice } from './dropdown-choice.js';

export const createHistoryKeywordsView = ({ els, api, appendLog }) => {
    const state = { id: null, request: 0, content: null, loading: false, limit: 200, error: '' };
    const sourceChoice = createDropdownChoice({
        menu: els.historyKeywordsSourceMenu,
        value: els.historyKeywordsSourceValue,
    });
    const evidence = createKeywordEvidenceView({ root: els.historyKeywordsResults, api, appendLog });
    const headers = els.historyKeywordsTable.querySelector('thead tr');
    const setChoices = () =>
        sourceChoice.setOptions([
            { value: 'all', label: 'All keywords' },
            {
                value: 'shared',
                label: Object.keys(state.content?.sources.subtitle_languages || {}).length
                    ? 'In all available sources'
                    : 'In all three sources',
            },
            { value: 'transcript', label: 'In transcript' },
            { value: 'ocr', label: 'In OCR' },
            { value: 'captions', label: 'In captions' },
            ...(Object.keys(state.content?.sources.subtitle_languages || {}).length
                ? [{ value: 'subtitles', label: 'In creator subtitles' }]
                : []),
            ...Object.keys(state.content?.sources.subtitle_languages || {}).map(language => ({
                value: `language:${language}`,
                label: `Creator subtitles · ${language}`,
            })),
        ]);
    setChoices();
    const render = () => {
        const content = state.content;
        const query = searchKey(trimSearchQuery(els.historyKeywordsSearch.value));
        const source = sourceChoice.getValue();
        const languages = Object.keys(content?.sources.subtitle_languages || {});
        const availableSources =
            Number(Boolean(content?.sources.transcript)) +
            Number(Boolean(content?.sources.ocr_frames || content?.sources.ocr_legacy)) +
            Number(Boolean(content?.sources.captions)) +
            Number(languages.length > 0);
        const terms = (content?.terms || []).filter(
            term =>
                searchKey(term.term).includes(query) &&
                (source === 'all' ||
                    (source === 'shared'
                        ? term.sources === availableSources
                        : source.startsWith('language:')
                          ? term.subtitle_languages?.[source.slice(9)] > 0
                          : term[source] > 0))
        );
        els.historyKeywordsRefreshBtn.disabled = !api.available || !state.id || state.loading;
        els.historyKeywordsRefreshBtn.setAttribute('aria-busy', String(state.loading));
        els.historyKeywordsSearch.disabled = state.loading || !content;
        els.historyKeywordsSourceTrigger.disabled = state.loading || !content;
        els.historyKeywordsResults.setAttribute('aria-busy', String(state.loading));
        const sources = content?.sources;
        els.historyKeywordsStatus.textContent =
            state.error ||
            (state.loading
                ? 'Mining keywords…'
                : content
                  ? `${terms.length.toLocaleString()} ${terms.length === 1 ? 'keyword' : 'keywords'}${terms.length > state.limit ? ` · ${state.limit.toLocaleString()} shown` : ''}`
                  : '');
        els.historyKeywordsEmpty.hidden = terms.length > 0 || state.loading || Boolean(state.error);
        els.historyKeywordsEmpty.textContent = content?.terms.length
            ? 'No keywords match these filters.'
            : sources &&
                !sources.transcript &&
                !sources.ocr_frames &&
                !sources.ocr_legacy &&
                !sources.captions &&
                !languages.length
              ? 'No saved text is available. Create a transcript, run OCR or save captions or creator subtitles first.'
              : 'No keywords occur at least 5 times in the saved texts.';
        els.historyKeywordsTable.hidden = !terms.length;
        els.historyKeywordsLoadMoreBtn.hidden = terms.length <= state.limit;
        els.historyKeywordsLoadMoreBtn.disabled = state.loading;
        for (const header of headers.querySelectorAll('[data-keyword-language]')) header.remove();
        for (const language of languages) {
            const header = document.createElement('th');
            header.scope = 'col';
            header.className = 'oj-numeric';
            header.dataset.keywordLanguage = language;
            header.textContent = `Creator subtitles · ${language}`;
            headers.insertBefore(header, headers.lastElementChild);
        }
        const occurrenceButton = (text, term, source, language = null) => {
            const button = document.createElement('button');
            button.type = 'button';
            button.className = 'oj-button oj-button-ghost oj-button-compact';
            button.textContent = text;
            button.setAttribute(
                'aria-label',
                `${term.term}: ${source || 'all'} occurrences${language ? ` · ${language}` : ''}`
            );
            button.addEventListener('click', () => evidence.open(term.term, { source, language }, button));
            return button;
        };
        const fragment = document.createDocumentFragment();
        for (const term of terms.slice(0, state.limit)) {
            const row = document.createElement('tr');
            const label = document.createElement('th');
            label.scope = 'row';
            const language = source.startsWith('language:') ? source.slice(9) : null;
            label.append(
                occurrenceButton(
                    term.term,
                    term,
                    language
                        ? 'subtitles'
                        : ['transcript', 'ocr', 'captions', 'subtitles'].includes(source)
                          ? source
                          : null,
                    language
                )
            );
            row.append(label);
            for (const [source, language] of [
                ['transcript', null],
                ['ocr', null],
                ['captions', null],
                ...languages.map(language => ['subtitles', language]),
                ['total', null],
            ]) {
                const count = document.createElement('td');
                count.className = 'oj-numeric';
                const value = language ? term.subtitle_languages?.[language] : term[source];
                if (value)
                    count.append(
                        occurrenceButton(value.toLocaleString(), term, source === 'total' ? null : source, language)
                    );
                else {
                    count.textContent = '—';
                    count.setAttribute('aria-label', 'Not found');
                }
                row.append(count);
            }
            fragment.append(row);
        }
        els.historyKeywordsRows.replaceChildren(fragment);
    };
    const reset = id => {
        Object.assign(state, { id, content: null, loading: false, limit: 200, error: '' });
        state.request++;
        evidence.reset(id);
        setChoices();
        els.historyKeywordsSearch.value = '';
        sourceChoice.setValue('all');
        render();
    };
    const load = async () => {
        if (!state.id || state.loading || !api.available) return;
        const id = state.id;
        const request = ++state.request;
        evidence.close();
        state.loading = true;
        state.error = '';
        render();
        try {
            const content = await api.getHistoryKeywords({ id });
            if (id !== state.id || request !== state.request) return;
            state.content = content;
            setChoices();
            state.limit = 200;
        } catch (error) {
            appendLog(`[Keywords] ${error}`, true);
            if (id === state.id && request === state.request)
                state.error = /limit/i.test(String(error))
                    ? 'The saved text is too large for keyword mining.'
                    : `Keywords could not be loaded. Try refreshing.${state.content ? ' Previous results are still shown.' : ''}`;
        } finally {
            if (id === state.id && request === state.request) {
                state.loading = false;
                render();
            }
        }
    };
    const bindEvents = () => {
        els.historyKeywordsRefreshBtn.addEventListener('click', () => void load());
        els.historyKeywordsSearch.addEventListener('input', () => {
            state.limit = 200;
            render();
        });
        els.historyKeywordsSourceDropdown.addEventListener('oj:select', event => {
            if (
                event.target !== els.historyKeywordsSourceDropdown ||
                els.historyKeywordsSourceTrigger.disabled ||
                !sourceChoice.hasValue(event.detail?.value)
            )
                return;
            sourceChoice.setValue(event.detail.value);
            state.limit = 200;
            render();
        });
        els.historyKeywordsLoadMoreBtn.addEventListener('click', () => {
            state.limit += 200;
            render();
        });
    };
    return Object.freeze({ reset, load, bindEvents, deactivate: evidence.deactivate });
};
