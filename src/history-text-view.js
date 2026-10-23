import { toast } from './vendor/oj/index.js';
import { textTimestamp, textMatches, textEntries, entryAtTime } from './history-text.js';

export const highlightText = (element, text, query) => {
    const fragment = document.createDocumentFragment();
    let position = 0;
    for (const match of textMatches(text, query)) {
        fragment.append(document.createTextNode(text.slice(position, match.start)));
        const mark = document.createElement('mark');
        mark.textContent = text.slice(match.start, match.end);
        fragment.append(mark);
        position = match.end;
    }
    fragment.append(document.createTextNode(text.slice(position)));
    element.replaceChildren(fragment);
};
export const bindTextSearch = (input, search) => {
    input.addEventListener('input', search);
    input.addEventListener('keydown', event => {
        if (event.key === 'Escape' && input.value) {
            event.preventDefault();
            input.value = '';
            search();
        }
    });
};
export const copyAllText = async ({ getText, isCurrent = () => true, status, appendLog }) => {
    try {
        const text = await getText();
        if (!isCurrent() || !text) return false;
        await navigator.clipboard.writeText(text);
        if (!isCurrent()) return false;
        status.textContent = 'All text copied.';
        toast('All text copied.', { type: 'success' });
        return true;
    } catch (error) {
        appendLog?.(`[History copy] ${error}`, true);
        if (isCurrent()) status.textContent = 'Text could not be copied. Try again.';
        return false;
    }
};
export const createTextEntry = (row, query = '') => {
    const item = document.createElement('li');
    item.className = 'pinefetch-text-entry';
    if (row.time !== null) item.dataset.timeMs = String(row.time);
    const time = document.createElement('button');
    time.type = 'button';
    time.className = 'oj-button oj-button-ghost oj-button-compact oj-mono pinefetch-text-time';
    time.textContent = row.time === null ? '—' : textTimestamp(row.time);
    time.disabled = row.time === null;
    if (row.time !== null) time.title = textTimestamp(row.time);
    time.setAttribute('aria-label', row.time === null ? 'Text without timestamps' : `Go to ${textTimestamp(row.time)}`);
    const text = document.createElement('p');
    text.className = 'oj-text';
    highlightText(text, row.text, query);
    item.append(time, text);
    return { item, time, text };
};
export const revealTextEntry = (viewport, item) => {
    const view = viewport.getBoundingClientRect();
    const row = item.getBoundingClientRect();
    if (row.top < view.top || row.height > view.height) viewport.scrollTop += row.top - view.top;
    else if (row.bottom > view.bottom) viewport.scrollTop += row.bottom - view.bottom;
};
export const createTextReader = ({ input, copy, status, count, viewport, list, appendLog, onSelect = () => {} }) => {
    let source = '';
    let rows = [];
    let visible = [];
    let selected = null;
    let revision = 0;
    const elements = new Map();
    const select = (row, seek = true) => {
        selected = row?.id;
        const selectedVisible = visible.some(entry => entry.id === selected);
        for (const [id, entry] of elements) {
            entry.item.setAttribute('aria-current', String(id === selected));
            entry.time.setAttribute('aria-pressed', String(id === selected));
            entry.time.tabIndex = id === selected || (!selectedVisible && id === visible[0]?.id) ? 0 : -1;
        }
        if (row && seek && row.time !== null) onSelect(row.time / 1000);
    };
    const render = () => {
        const query = input.value.trim();
        visible = query
            ? rows.filter(
                  row =>
                      textMatches(row.text, query).length ||
                      textMatches(row.time === null ? '' : textTimestamp(row.time), query).length
              )
            : rows;
        elements.clear();
        const fragment = document.createDocumentFragment();
        for (const row of visible) {
            const entry = createTextEntry(row, query);
            entry.item.dataset.textId = row.id;
            elements.set(row.id, entry);
            fragment.append(entry.item);
        }
        list.replaceChildren(fragment);
        list.hidden = !source;
        copy.disabled = !source;
        copy.setAttribute('aria-busy', 'false');
        input.disabled = !source;
        count.textContent = source
            ? query
                ? `${visible.length.toLocaleString()} ${visible.length === 1 ? 'entry' : 'entries'} found`
                : `${rows.length.toLocaleString()} ${rows.length === 1 ? 'entry' : 'entries'}`
            : '';
        if (source && !visible.length) {
            const empty = document.createElement('li');
            empty.className = 'oj-helper';
            empty.textContent = 'No text matches your search.';
            list.append(empty);
        }
        if (!visible.some(row => row.id === selected)) selected = null;
        select(
            rows.find(row => row.id === selected),
            false
        );
    };
    bindTextSearch(input, () => {
        viewport.scrollTop = 0;
        render();
    });
    copy.addEventListener('click', async () => {
        if (!source || copy.disabled) return;
        const request = revision;
        copy.disabled = true;
        copy.setAttribute('aria-busy', 'true');
        await copyAllText({ getText: () => source, isCurrent: () => request === revision, status, appendLog });
        if (request === revision) {
            copy.disabled = !source;
            copy.setAttribute('aria-busy', 'false');
        }
    });
    list.addEventListener('click', event => {
        const id = event.target.closest('[data-text-id]')?.dataset.textId;
        const row = visible.find(row => row.id === id);
        if (!row) return;
        select(row);
        if (row.time !== null) elements.get(row.id).time.focus({ preventScroll: true });
    });
    viewport.addEventListener('keydown', event => {
        if (
            !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key) ||
            event.altKey ||
            event.ctrlKey ||
            event.metaKey ||
            event.shiftKey ||
            !visible.length
        )
            return;
        event.preventDefault();
        const current = visible.findIndex(row => row.id === selected);
        const index =
            event.key === 'Home'
                ? 0
                : event.key === 'End'
                  ? visible.length - 1
                  : Math.max(0, Math.min(visible.length - 1, current + (event.key === 'ArrowDown' ? 1 : -1)));
        const row = visible[index];
        select(row);
        const entry = elements.get(row.id);
        (row.time === null ? viewport : entry.time).focus({ preventScroll: true });
        revealTextEntry(viewport, entry.item);
    });
    return Object.freeze({
        set: text => {
            if (source === (text || '')) return;
            revision++;
            source = text || '';
            rows = textEntries(source);
            selected = null;
            render();
        },
        reset: () => {
            revision++;
            source = '';
            rows = [];
            selected = null;
            input.value = '';
            status.textContent = '';
            render();
        },
        sync: seconds => {
            const row = entryAtTime(rows, seconds * 1000);
            if (row?.id === selected) return;
            select(row, false);
            const entry = elements.get(row?.id);
            if (entry && !viewport.matches(':focus-within')) revealTextEntry(viewport, entry.item);
        },
        deactivate: () => {
            revision++;
            copy.disabled = !source;
            copy.setAttribute('aria-busy', 'false');
        },
    });
};
