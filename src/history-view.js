import { confirmDialog } from './vendor/oj/index.js';
import { createDropdownChoice } from './dropdown-choice.js';
import { formatHistorySource, formatLocalDateTime, formatUploadDate } from './formatters.js';
import { createHistoryAccountView } from './history-account-view.js';
import { createHistoryDetailsView } from './history-details-view.js';
import { createHistoryOcrJobs, ocrActive } from './history-ocr-jobs.js';
import { createPlatformIcon } from './platform-icons.js';

export const createHistoryView = ({
    els,
    api,
    appendLog,
    formatFileSize,
    formatDuration,
    detectPlatform,
    appendTextSpans,
    isActive,
    prepareDownload,
}) => {
    const state = Object.seal({
        historyOffset: 0,
        historyHasMore: false,
        historyTotalCount: null,
        historyLoading: false,
        historyLoaded: false,
        historyDirty: true,
        historyRevision: 0,
        historyQuery: '',
        historySearchField: 'title',
        historySource: '',
        historyExactUser: false,
        historyClearing: false,
        historyRemoving: false,
        historyRemovalFocus: null,
        historyViewRevision: 0,
        historyFilterRevision: 0,
        page: 'list',
        historyReturnId: null,
        scrollTop: 0,
    });
    const historyPageSize = 20;
    const historySearchDelayMs = 250;
    let historySearchTimer = null;
    let historySources = [];
    const jobs = createHistoryOcrJobs({ api, appendLog });
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
    const returnToHistory = returnFocus => {
        historyDetailsView.leave();
        accountView.leave();
        state.page = 'list';
        activate();
        void renderHistory().then(() => {
            if (state.page !== 'list' || !isActive()) return;
            els.historyList.scrollTop = state.scrollTop;
            const row = [...els.historyList.children].find(item => item.dataset.historyId === state.historyReturnId);
            const target =
                returnFocus && els.historyList.contains(returnFocus)
                    ? returnFocus
                    : row?.querySelector('.pinefetch-history-open-btn') || els.historySearchInput;
            target.focus({ preventScroll: true });
        });
    };
    const historyDetailsView = createHistoryDetailsView({
        els,
        api,
        appendLog,
        formatFileSize,
        formatDuration,
        detectPlatform,
        showSavedVideosByCreator: creator => showSavedVideosByCreator(creator),
        jobs,
        openAccount: id => {
            historyDetailsView.leave();
            state.page = 'account';
            activate();
            accountView.open(id);
        },
        navigateToDetails: id => {
            if (state.page === 'list') {
                state.scrollTop = els.historyList.scrollTop;
                state.historyReturnId = id;
            }
            accountView.leave();
            state.page = 'detail';
            activate();
        },
        navigateToHistory: returnToHistory,
    });

    const accountView = createHistoryAccountView({
        els,
        api,
        appendLog,
        prepareDownload,
        openVideo: (id, button) => void historyDetailsView.open(id, button),
        navigateToHistory: () => returnToHistory(),
    });

    const activate = () => {
        state.historyViewRevision += 1;
        state.historyRemovalFocus = null;
        const detail = isActive() && state.page === 'detail';
        const account = isActive() && state.page === 'account';
        const list = isActive() && state.page === 'list';
        els.historyAccountView.hidden = !account;
        els.historyDetailsView.hidden = !detail;
        els.historyView.hidden = !list;
        els.historySummaryView.hidden = !list;
        els.historyView
            .closest('.pinefetch-split')
            .classList.toggle('pinefetch-history-details-active', detail || account);
        updateHistoryResultCount();
        historyDetailsView.hideContextMenu();
        historyDetailsView.activate();
    };

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

    const updateHistoryMutationControls = () => {
        const busy = state.historyClearing || state.historyRemoving;
        els.clearHistoryBtn.disabled = !api.available || state.historyLoading || busy;
        for (const button of els.historyList.querySelectorAll('.pinefetch-history-item-remove-btn')) {
            button.disabled = !api.available || busy;
        }
    };
    const setHistoryLoading = isLoading => {
        state.historyLoading = isLoading;
        els.loadMoreHistoryBtn.disabled = isLoading;
        updateHistoryMutationControls();
        els.loadMoreHistoryBtn.textContent = isLoading ? 'Loading...' : 'Load more';
    };

    const canReturnHistoryFocus = (context, opener) =>
        isActive() &&
        state.page === 'list' &&
        context.view === state.historyViewRevision &&
        context.filter === state.historyFilterRevision &&
        (document.activeElement === document.body ||
            document.activeElement === opener ||
            !document.activeElement?.isConnected);
    const restoreHistoryRemovalFocus = () => {
        if (state.historyLoading || !state.historyRemovalFocus) return;
        const focus = state.historyRemovalFocus;
        state.historyRemovalFocus = null;
        if (!canReturnHistoryFocus(focus, focus.opener)) return;
        const rows = [...els.historyList.children];
        const row = focus.ids.map(id => rows.find(row => row.dataset.historyId === id)).find(Boolean);
        const target = focus.clear
            ? els.clearHistoryBtn
            : row?.querySelector(focus.remove ? '.pinefetch-history-item-remove-btn' : '.pinefetch-history-open-btn') ||
              rows[0]?.querySelector('.pinefetch-history-open-btn') ||
              els.historySearchInput;
        target.focus({ preventScroll: true });
    };

    const updateHistoryResultCount = () => {
        const hasCount = Boolean(state.historyQuery) && state.historyTotalCount !== null;
        els.historyResultCount.textContent = hasCount ? `${state.historyTotalCount.toLocaleString()} found` : '';
        els.historyResultCount.hidden = !isActive() || state.page !== 'list' || !hasCount;
    };

    const updateHistoryActions = () => {
        els.loadMoreHistoryBtn.hidden = !state.historyHasMore;
        updateHistoryResultCount();
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
            name.className = 'oj-inline pinefetch-history-source-name';
            name.textContent = formatHistorySource(entry?.source);
            const icon = createPlatformIcon(entry?.source);
            if (icon) name.prepend(icon);
            const value = document.createElement('strong');
            value.textContent = count.toLocaleString();
            row.append(name, value);
            fragment.appendChild(row);
        }
        els.historySourcesList.replaceChildren(fragment);
        els.historySourcesEmpty.hidden = els.historySourcesList.childElementCount > 0;
    };

    const updateHistorySourceOptions = () => {
        const sources = [...historySources];
        if (state.historyExactUser && state.historySource && !sources.includes(state.historySource)) {
            sources.push(state.historySource);
        }
        sourceChoice.setOptions([
            { value: '', label: 'All sources' },
            ...sources.map(source => ({ value: source, label: formatHistorySource(source) })),
        ]);
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

        historySources = sources;
        const previousSource = state.historySource;
        updateHistorySourceOptions();
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
        state.historyTotalCount = null;
        updateHistoryResultCount();
    };

    const createHistoryItem = entry => {
        const item = document.createElement('div');
        item.className = 'oj-panel oj-panel-compact oj-panel-interactive pinefetch-history-item';
        item.dataset.historyId = entry.id;
        item.dataset.historyUrl = entry.url || '';
        item.dataset.historyUploader = `${entry.uploader || ''}`.trim();
        item.dataset.historySource = `${entry.source || ''}`.trim().toLowerCase();
        const entryLabel = entry.title || entry.filename || entry.url || 'download';
        const openBtn = document.createElement('button');
        openBtn.type = 'button';
        openBtn.className = `pinefetch-history-open-btn pinefetch-list-card-layout ${entry.thumbnail ? '' : 'pinefetch-no-media'}`;
        openBtn.setAttribute('aria-label', `Open video details: ${entryLabel}`);
        openBtn.setAttribute('aria-haspopup', 'menu');
        openBtn.setAttribute('aria-controls', 'historyContextMenu');

        openBtn.onclick = () => void historyDetailsView.open(entry.id, openBtn);

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
        const sourceLabel = document.createElement('span');
        sourceLabel.className = 'pinefetch-history-platform';
        sourceLabel.title = formatHistorySource(source);
        const sourceIcon = createPlatformIcon(source);
        if (sourceIcon) {
            const name = document.createElement('span');
            name.className = 'oj-sr-only';
            name.textContent = sourceLabel.title;
            sourceLabel.append(sourceIcon, name);
        } else {
            sourceLabel.textContent = sourceLabel.title;
        }
        meta.appendChild(sourceLabel);
        const uploadDate = formatUploadTimestamp(entry.timestamp) || formatUploadDate(entry.upload_date);
        appendTextSpans(meta, [
            entry.medium || '',
            entry.uploader ? `by ${entry.uploader}` : '',
            entry.filename || '',
            uploadDate ? `uploaded ${uploadDate}` : '',
            dateStr,
        ]);
        content.appendChild(meta);
        const ocrIndicator = document.createElement('span');
        ocrIndicator.className = 'oj-caption pinefetch-history-ocr-indicator';
        ocrIndicator.dataset.ocrHistoryId = entry.id;
        renderOcrIndicator(ocrIndicator, jobs.get(entry.id));
        content.appendChild(ocrIndicator);

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
        removeBtn.disabled = !api.available || state.historyClearing || state.historyRemoving;
        removeBtn.onclick = async event => {
            event.stopPropagation();
            if (!api.available || state.historyRemoving || state.historyClearing) return;
            state.historyRemoving = true;
            const context = { view: state.historyViewRevision, filter: state.historyFilterRevision };
            let removed = false;
            try {
                removeBtn.focus({ preventScroll: true });
                updateHistoryMutationControls();
                if (
                    !(await confirmDialog({
                        title: 'Remove from history?',
                        message:
                            'Remove “' +
                            entryLabel +
                            "” from History and delete its saved transcripts, captions, creator subtitles, OCR results and Social Insights? Saved account insights are also removed if this is the account's last History entry. This cannot be undone. Downloaded files will stay on disk.",
                        confirmLabel: 'Remove from history',
                        variant: 'danger',
                    }))
                )
                    return;
                if (!isActive() || state.page !== 'list' || context.view !== state.historyViewRevision) return;
                updateHistoryMutationControls();
                setHistoryActionStatus('Removing history entry...');
                const rows = [...els.historyList.children];
                const index = rows.findIndex(row => row.dataset.historyId === entry.id);
                const neighbours = [...rows.slice(index + 1), ...rows.slice(0, index).reverse()].map(
                    row => row.dataset.historyId
                );
                await api.removeHistoryEntry({ id: entry.id });
                removed = true;
                const returnFocus = canReturnHistoryFocus(context, removeBtn);
                const currentRow = [...els.historyList.children].find(row => row.dataset.historyId === entry.id);
                if (currentRow) {
                    currentRow.remove();
                    state.historyOffset = Math.max(0, state.historyOffset - 1);
                }
                invalidateHistoryCache();
                if (returnFocus)
                    state.historyRemovalFocus = {
                        ...context,
                        ids: neighbours,
                        opener: removeBtn,
                    };
                await renderHistory({ force: true });
                setHistoryActionStatus('History entry removed. Downloaded files remain on disk.');
            } catch (err) {
                setHistoryActionStatus('Could not remove this history entry. Try again.', true);
                appendLog(`[history] ${err}`, true);
            } finally {
                state.historyRemoving = false;
                updateHistoryMutationControls();
                if (!removed && canReturnHistoryFocus(context, removeBtn))
                    state.historyRemovalFocus = { ...context, ids: [entry.id], opener: removeBtn, remove: true };
                restoreHistoryRemovalFocus();
            }
        };
        item.appendChild(removeBtn);

        return item;
    };

    const renderHistory = async ({ append = false, force = false } = {}) => {
        updateHistoryResultCount();
        if (state.page === 'detail') return;
        if (!append && !force && state.historyLoaded && !state.historyDirty) return;

        if (!api.available) {
            els.historyList.replaceChildren();
            els.historyHint.hidden = true;
            state.historyHasMore = false;
            state.historyTotalCount = null;
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
        const requestedExactUser = state.historyExactUser;
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
                ...(requestedExactUser ? { exactUser: true } : {}),
            });
            if (state.historyRevision !== requestedRevision) {
                needsFollowUpRefresh = true;
                return;
            }
            const entries = Array.isArray(page) ? page : page?.entries || [];
            jobs.adoptSummaries(page?.ocr_statuses);
            const hasMore = Array.isArray(page)
                ? entries.length === historyPageSize
                : Boolean(page?.has_more ?? page?.hasMore);
            const totalCount = Number(page?.total_count);
            state.historyTotalCount = Number.isSafeInteger(totalCount) && totalCount >= 0 ? totalCount : null;

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
            } else {
                restoreHistoryRemovalFocus();
            }
        }
    };

    const showSavedVideosByCreator = ({ uploader, source }) => {
        if (!uploader || !source) return;
        state.historyFilterRevision += 1;
        if (historySearchTimer !== null) clearTimeout(historySearchTimer);
        historySearchTimer = null;
        els.historySearchInput.value = uploader;
        searchFieldChoice.setValue('user');
        els.historySearchInput.placeholder = 'Search users';
        state.historyQuery = uploader;
        state.historySearchField = 'user';
        state.historySource = source;
        state.historyExactUser = true;
        state.historyOffset = 0;
        updateHistorySourceOptions();
        sourceChoice.setValue(source);
        invalidateHistoryCache();
        els.historySearchInput.focus({ preventScroll: true });
        void renderHistory({ force: true });
    };

    const bindEvents = () => {
        jobs.bindEvents();
        const unsubscribeJobs = jobs.subscribe((id, record) => {
            for (const indicator of els.historyList.querySelectorAll('[data-ocr-history-id]')) {
                if (indicator.dataset.ocrHistoryId === id) renderOcrIndicator(indicator, record);
            }
        });
        window.addEventListener(
            'pagehide',
            () => {
                unsubscribeJobs();
                jobs.dispose();
            },
            { once: true }
        );
        historyDetailsView.bindEvents();
        accountView.bindEvents();
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
                source === state.historySource &&
                !state.historyExactUser
            )
                return;
            state.historyFilterRevision += 1;
            state.historyQuery = query;
            state.historySearchField = searchField;
            state.historySource = source;
            state.historyExactUser = false;
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
            if (!api.available || state.historyClearing || state.historyRemoving) return;
            state.historyClearing = true;
            const context = { view: state.historyViewRevision, filter: state.historyFilterRevision };
            let cleared = false;
            try {
                els.clearHistoryBtn.focus({ preventScroll: true });
                updateHistoryMutationControls();
                if (
                    !(await confirmDialog({
                        title: 'Clear history?',
                        message:
                            'Delete all History entries and their saved transcripts, captions, creator subtitles, OCR results and Social Insights, including saved account insights? This cannot be undone. Downloaded files will stay on disk.',
                        confirmLabel: 'Clear history',
                        variant: 'danger',
                    }))
                )
                    return;
                updateHistoryMutationControls();
                setHistoryActionStatus('Deleting history...');
                await api.clearHistory();
                cleared = true;
                if (historySearchTimer !== null) clearTimeout(historySearchTimer);
                historySearchTimer = null;
                els.historySearchInput.value = '';
                searchFieldChoice.setValue('title');
                els.historySearchInput.placeholder = searchPlaceholders.title;
                sourceChoice.setValue('');
                state.historyQuery = '';
                state.historySearchField = 'title';
                state.historySource = '';
                state.historyExactUser = false;
                invalidateHistoryCache();
                await renderHistory({ force: true });
                setHistoryActionStatus('History deleted. Downloaded files remain on disk.');
            } catch (err) {
                setHistoryActionStatus(`Could not delete history: ${err}`, true);
                appendLog(`[history] ${err}`, true);
            } finally {
                state.historyClearing = false;
                updateHistoryMutationControls();
                if (!cleared && canReturnHistoryFocus(context, els.clearHistoryBtn))
                    state.historyRemovalFocus = { ...context, ids: [], opener: els.clearHistoryBtn, clear: true };
                restoreHistoryRemovalFocus();
            }
        });
    };

    const onChanged = () => {
        historyDetailsView.hideContextMenu();
        invalidateHistoryCache();
        if (isActive() && state.page === 'list') void renderHistory({ force: true });
    };

    const renderOcrIndicator = (element, record) => {
        const labels = {
            queued: 'OCR pending',
            running: 'OCR processing',
            completed: 'OCR completed',
            failed: 'OCR failed',
            cancelled: 'OCR cancelled',
            interrupted: 'OCR interrupted',
        };
        const label = labels[record?.status];
        element.hidden = !label;
        element.replaceChildren();
        if (!label) return;
        element.title = label;
        const icon = document.createElement('i');
        icon.className = ocrActive(record.status)
            ? 'oj-spinner'
            : record.status === 'completed'
              ? 'fa-solid fa-font'
              : 'fa-solid fa-circle-exclamation';
        icon.setAttribute('aria-hidden', 'true');
        element.append(icon, document.createTextNode(label));
    };
    return Object.freeze({ render: renderHistory, bindEvents, onChanged, activate });
};
