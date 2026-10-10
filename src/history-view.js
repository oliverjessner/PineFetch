import { confirmDialog } from './vendor/oj/index.js';
import { createDropdownChoice } from './dropdown-choice.js';
import { formatHistorySource, formatLocalDateTime, formatUploadDate } from './formatters.js';
import { createHistoryDetailsView } from './history-details-view.js';

export const createHistoryView = ({
    els,
    api,
    appendLog,
    formatFileSize,
    formatDuration,
    detectPlatform,
    appendTextSpans,
    isActive,
}) => {
    const state = Object.seal({
        historyOffset: 0,
        historyHasMore: false,
        historyLoading: false,
        historyLoaded: false,
        historyDirty: true,
        historyRevision: 0,
        historyQuery: '',
        historySearchField: 'title',
        historySource: '',
        historyClearing: false,
    });
    const historyPageSize = 20;
    const historySearchDelayMs = 250;
    let historySearchTimer = null;
    const searchFieldChoice = createDropdownChoice({
        menu: els.historySearchFieldMenu,
        value: els.historySearchFieldValue,
    });
    searchFieldChoice.setOptions([
        { value: 'title', label: 'Title' },
        { value: 'description', label: 'Description' },
        { value: 'user', label: 'User' },
        { value: 'transcript', label: 'Transcript' },
    ]);
    const sourceChoice = createDropdownChoice({ menu: els.historySourceMenu, value: els.historySourceValue });
    sourceChoice.setOptions([{ value: '', label: 'All sources' }]);
    const historyDetailsView = createHistoryDetailsView({
        els,
        api,
        appendLog,
        formatFileSize,
        formatDuration,
        detectPlatform,
    });

    const formatHistoryDate = timestamp => {
        if (!timestamp) return '-';
        const date = new Date(timestamp);
        const now = new Date();
        const diffMs = now - date;
        const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

        if (diffDays === 0) {
            return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
        } else if (diffDays === 1) {
            return 'Yesterday';
        } else if (diffDays < 7) {
            return date.toLocaleDateString([], { weekday: 'short' });
        } else {
            return date.toLocaleDateString([], { month: 'short', day: 'numeric' });
        }
    };

    const formatUploadTimestamp = timestamp => {
        const seconds = Number(timestamp);
        if (!Number.isFinite(seconds) || seconds <= 0) return null;

        const date = new Date(seconds * 1000);
        if (Number.isNaN(date.getTime())) return null;

        return formatLocalDateTime(date);
    };

    const setHistoryLoading = isLoading => {
        state.historyLoading = isLoading;
        els.loadMoreHistoryBtn.disabled = isLoading;
        els.clearHistoryBtn.disabled = isLoading || state.historyClearing;
        els.loadMoreHistoryBtn.textContent = isLoading ? 'Loading...' : 'Load more';
    };

    const updateHistoryActions = () => {
        els.loadMoreHistoryBtn.hidden = !state.historyHasMore;
    };

    const setHistoryActionStatus = (message, isError = false) => {
        els.historyActionStatus.textContent = message;
        els.historyActionStatus.hidden = !message;
        els.historyActionStatus.classList.toggle('oj-status-error', isError);
        els.historyActionStatus.classList.toggle('oj-status-success', Boolean(message && !isError));
    };

    const renderHistorySources = sourceCounts => {
        const fragment = document.createDocumentFragment();
        for (const entry of Array.isArray(sourceCounts) ? sourceCounts : []) {
            const count = Number(entry?.count);
            if (!Number.isFinite(count) || count <= 0) continue;
            const row = document.createElement('li');
            row.className = 'oj-list-item';
            const name = document.createElement('span');
            name.textContent = formatHistorySource(entry?.source);
            const value = document.createElement('strong');
            value.textContent = count.toLocaleString();
            row.append(name, value);
            fragment.appendChild(row);
        }
        els.historySourcesList.replaceChildren(fragment);
        els.historySourcesEmpty.hidden = els.historySourcesList.childElementCount > 0;
    };

    const renderHistorySourceOptions = sourceCounts => {
        const sources = [];
        const seen = new Set();
        for (const entry of Array.isArray(sourceCounts) ? sourceCounts : []) {
            const source = `${entry?.source || ''}`.trim().toLowerCase();
            if (!source || seen.has(source)) continue;
            seen.add(source);
            sources.push(source);
        }

        const previousSource = state.historySource;
        sourceChoice.setOptions([
            { value: '', label: 'All sources' },
            ...sources.map(source => ({ value: source, label: formatHistorySource(source) })),
        ]);
        state.historySource = sourceChoice.getValue();
        return state.historySource !== previousSource;
    };

    const renderHistoryStats = async () => {
        if (!api.available) return;

        try {
            const stats = await api.getHistoryStats();
            els.historyVideoCount.textContent = Number(stats?.video_count || 0).toLocaleString();
            els.historyTotalSize.textContent = formatFileSize(stats?.total_file_size_bytes);
            els.historyTotalDuration.textContent = formatDuration(Number(stats?.total_duration_seconds || 0));
            renderHistorySources(stats?.source_counts);
            if (renderHistorySourceOptions(stats?.source_counts)) {
                invalidateHistoryCache();
                if (!state.historyLoading && isActive()) void renderHistory({ force: true });
            }
        } catch (err) {
            appendLog(`[history] ${err}`, true);
        }
    };

    const invalidateHistoryCache = () => {
        state.historyDirty = true;
        state.historyRevision += 1;
    };

    const createHistoryItem = entry => {
        const item = document.createElement('div');
        item.className = 'oj-panel oj-panel-compact oj-panel-interactive pinefetch-history-item';
        item.dataset.historyId = entry.id;
        item.dataset.historyUrl = entry.url || '';
        const entryLabel = entry.title || entry.filename || entry.url || 'download';
        const openBtn = document.createElement('button');
        openBtn.type = 'button';
        openBtn.className = `pinefetch-history-open-btn pinefetch-list-card-layout ${entry.thumbnail ? '' : 'pinefetch-no-media'}`;
        openBtn.setAttribute('aria-label', `Open downloaded file: ${entryLabel}`);
        openBtn.setAttribute('aria-haspopup', 'menu');
        openBtn.setAttribute('aria-controls', 'historyContextMenu');

        openBtn.onclick = async () => {
            // Rust uses snake_case: output_path, not outputPath
            const outputPath = entry.output_path || entry.outputPath;
            if (outputPath && api.available) {
                try {
                    const exists = await api.openFilePath({ path: outputPath });
                    if (!exists) {
                        appendLog(`[history] File not found: ${outputPath}`, true);
                    }
                } catch (err) {
                    appendLog(`[open] ${err}`, true);
                }
            }
        };

        const content = document.createElement('div');
        content.className = 'pinefetch-history-content';

        const title = document.createElement('div');
        title.className = 'pinefetch-history-title';
        title.textContent = entryLabel;
        content.appendChild(title);

        const meta = document.createElement('div');
        meta.className = 'pinefetch-history-meta';
        const dateStr = formatHistoryDate(entry.completed_at);
        const source = entry.source || entry.platform || detectPlatform(entry.url) || 'unknown';
        const uploadDate = formatUploadTimestamp(entry.timestamp) || formatUploadDate(entry.upload_date);
        appendTextSpans(meta, [
            source,
            entry.medium || '',
            entry.uploader ? `by ${entry.uploader}` : '',
            entry.filename || '',
            uploadDate ? `uploaded ${uploadDate}` : '',
            dateStr,
        ]);
        content.appendChild(meta);

        openBtn.appendChild(content);

        if (entry.thumbnail) {
            const thumb = document.createElement('div');
            thumb.className = 'pinefetch-media-thumbnail pinefetch-history-thumb';
            thumb.style.backgroundImage = `url('${entry.thumbnail}')`;
            openBtn.appendChild(thumb);
        }
        item.appendChild(openBtn);

        const removeBtn = document.createElement('button');
        removeBtn.className = 'oj-icon-button oj-button-danger pinefetch-history-item-remove-btn';
        const removeIcon = document.createElement('i');
        removeIcon.className = 'fa-solid fa-xmark';
        removeIcon.setAttribute('aria-hidden', 'true');
        removeBtn.appendChild(removeIcon);
        removeBtn.title = 'Remove from history';
        removeBtn.setAttribute('aria-label', `Remove from history: ${entryLabel}`);
        removeBtn.onclick = async event => {
            event.stopPropagation();
            try {
                await api.removeHistoryEntry({ id: entry.id });
                invalidateHistoryCache();
                void renderHistory({ force: true });
            } catch (err) {
                appendLog(`[history] ${err}`, true);
            }
        };
        item.appendChild(removeBtn);

        return item;
    };

    const renderHistory = async ({ append = false, force = false } = {}) => {
        if (!append && !force && state.historyLoaded && !state.historyDirty) return;

        if (!api.available) {
            els.historyList.replaceChildren();
            els.historyHint.hidden = true;
            state.historyHasMore = false;
            state.historyLoaded = true;
            state.historyDirty = false;
            updateHistoryActions();
            return;
        }

        if (state.historyLoading) return;

        if (!append) void renderHistoryStats();
        const requestedRevision = state.historyRevision;
        const requestedQuery = state.historyQuery;
        const requestedSearchField = state.historySearchField;
        const requestedSource = state.historySource;
        let needsFollowUpRefresh = false;
        const offset = append ? state.historyOffset : 0;
        setHistoryLoading(true);

        try {
            const page = await api.getHistory({
                limit: historyPageSize,
                offset,
                ...(requestedQuery ? { query: requestedQuery } : {}),
                searchField: requestedSearchField,
                ...(requestedSource ? { source: requestedSource } : {}),
            });
            if (state.historyRevision !== requestedRevision) {
                needsFollowUpRefresh = true;
                return;
            }
            const entries = Array.isArray(page) ? page : page?.entries || [];
            const hasMore = Array.isArray(page)
                ? entries.length === historyPageSize
                : Boolean(page?.has_more ?? page?.hasMore);

            const fragment = document.createDocumentFragment();
            entries.forEach(entry => fragment.appendChild(createHistoryItem(entry)));
            if (append) {
                els.historyList.appendChild(fragment);
            } else {
                els.historyList.replaceChildren(fragment);
            }

            if (requestedQuery && requestedSource) {
                els.historyHint.textContent = `No ${formatHistorySource(requestedSource)} results for “${requestedQuery}”.`;
            } else if (requestedQuery) {
                els.historyHint.textContent = `No history results for “${requestedQuery}”.`;
            } else if (requestedSource) {
                els.historyHint.textContent = `No history from ${formatHistorySource(requestedSource)}.`;
            } else {
                els.historyHint.textContent = 'No history yet. Downloaded items will appear here.';
            }
            els.historyHint.hidden = entries.length > 0 || append;
            state.historyOffset = offset + entries.length;
            state.historyHasMore = hasMore;
            state.historyLoaded = true;
            needsFollowUpRefresh = state.historyRevision !== requestedRevision;
            state.historyDirty = needsFollowUpRefresh;
        } catch (err) {
            appendLog(`[history] ${err}`, true);
            state.historyDirty = true;
        } finally {
            setHistoryLoading(false);
            updateHistoryActions();
            if (needsFollowUpRefresh && isActive()) {
                void renderHistory({ force: true });
            }
        }
    };

    const bindEvents = () => {
        historyDetailsView.bindEvents();
        const searchPlaceholders = {
            title: 'Search titles',
            description: 'Search descriptions',
            user: 'Search users',
            transcript: 'Search transcripts',
        };
        const applyHistoryFilters = () => {
            const query = els.historySearchInput.value.trim();
            const searchField = searchFieldChoice.getValue();
            const source = sourceChoice.getValue();
            if (
                query === state.historyQuery &&
                searchField === state.historySearchField &&
                source === state.historySource
            )
                return;
            state.historyQuery = query;
            state.historySearchField = searchField;
            state.historySource = source;
            invalidateHistoryCache();
            void renderHistory({ force: true });
        };
        const applySelectFilters = () => {
            if (historySearchTimer !== null) clearTimeout(historySearchTimer);
            historySearchTimer = null;
            els.historySearchInput.placeholder = searchPlaceholders[searchFieldChoice.getValue()] || 'Search history';
            applyHistoryFilters();
        };

        els.loadMoreHistoryBtn.addEventListener('click', () => {
            void renderHistory({ append: true });
        });
        els.historySearchInput.addEventListener('input', () => {
            if (historySearchTimer !== null) clearTimeout(historySearchTimer);
            historySearchTimer = setTimeout(() => {
                historySearchTimer = null;
                applyHistoryFilters();
            }, historySearchDelayMs);
        });
        for (const [dropdown, choice] of [
            [els.historySearchFieldDropdown, searchFieldChoice],
            [els.historySourceDropdown, sourceChoice],
        ]) {
            dropdown.addEventListener('oj:select', event => {
                if (event.target !== dropdown || !choice.hasValue(event.detail?.value)) return;
                choice.setValue(event.detail.value);
                applySelectFilters();
            });
        }
        els.clearHistoryBtn.addEventListener('click', async () => {
            if (!api.available || state.historyClearing) return;
            state.historyClearing = true;
            els.clearHistoryBtn.focus({ preventScroll: true });
            if (
                !(await confirmDialog({
                    title: 'Clear history?',
                    message:
                        'Delete all history entries and saved transcripts? This cannot be undone. Downloaded files will stay on disk.',
                    confirmLabel: 'Clear history',
                    variant: 'danger',
                }))
            ) {
                state.historyClearing = false;
                els.clearHistoryBtn.disabled = state.historyLoading;
                return;
            }
            els.clearHistoryBtn.disabled = true;
            setHistoryActionStatus('Deleting history...');
            try {
                await api.clearHistory();
                if (historySearchTimer !== null) clearTimeout(historySearchTimer);
                historySearchTimer = null;
                els.historySearchInput.value = '';
                searchFieldChoice.setValue('title');
                els.historySearchInput.placeholder = searchPlaceholders.title;
                sourceChoice.setValue('');
                state.historyQuery = '';
                state.historySearchField = 'title';
                state.historySource = '';
                invalidateHistoryCache();
                await renderHistory({ force: true });
                setHistoryActionStatus('History deleted. Downloaded files remain on disk.');
            } catch (err) {
                setHistoryActionStatus(`Could not delete history: ${err}`, true);
                appendLog(`[history] ${err}`, true);
            } finally {
                state.historyClearing = false;
                els.clearHistoryBtn.disabled = state.historyLoading;
            }
        });
    };

    const onChanged = () => {
        historyDetailsView.hideContextMenu();
        invalidateHistoryCache();
        if (isActive()) void renderHistory({ force: true });
    };

    return Object.freeze({ render: renderHistory, bindEvents, onChanged });
};
