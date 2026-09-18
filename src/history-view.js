export const createHistoryView = ({ els, invoke, appendLog, formatFileSize, formatDuration, detectPlatform, appendTextSpans, isActive }) => {
    const state = Object.seal({
        historyOffset: 0,
        historyHasMore: false,
        historyLoading: false,
        historyLoaded: false,
        historyDirty: true,
        historyRevision: 0,
        historyQuery: '',
        historyClearing: false,
    });
    const historyPageSize = 20;
    const historySearchDelayMs = 250;
    let historySearchTimer = null;

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

    const formatUploadDate = uploadDate => {
        const raw = `${uploadDate || ''}`.trim();
        if (!raw) return null;

        if (/^\d{8}$/.test(raw)) {
            const year = raw.slice(0, 4);
            const month = raw.slice(4, 6);
            const day = raw.slice(6, 8);
            return `${year}-${month}-${day}`;
        }

        return raw;
    };

    const formatUploadTimestamp = timestamp => {
        const seconds = Number(timestamp);
        if (!Number.isFinite(seconds) || seconds <= 0) return null;

        const date = new Date(seconds * 1000);
        if (Number.isNaN(date.getTime())) return null;

        return date.toLocaleString([], {
            year: 'numeric',
            month: 'short',
            day: 'numeric',
            hour: '2-digit',
            minute: '2-digit',
        });
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
        els.historyActionStatus.classList.toggle('pf-status-error', isError);
        els.historyActionStatus.classList.toggle('pf-status-success', Boolean(message && !isError));
    };

    const formatHistorySource = source => {
        const name = `${source || 'unknown'}`.trim().toLowerCase();
        const known = { youtube: 'YouTube', tiktok: 'TikTok', instagram: 'Instagram', facebook: 'Facebook', twitch: 'Twitch', linkedin: 'LinkedIn', x: 'X' };
        return known[name] || (name ? name.charAt(0).toUpperCase() + name.slice(1) : 'Unknown');
    };

    const renderHistorySources = sourceCounts => {
        const fragment = document.createDocumentFragment();
        for (const entry of Array.isArray(sourceCounts) ? sourceCounts : []) {
            const count = Number(entry?.count);
            if (!Number.isFinite(count) || count <= 0) continue;
            const row = document.createElement('li');
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

    const renderHistoryStats = async () => {
        if (!invoke) return;

        try {
            const stats = await invoke('get_history_stats');
            els.historyVideoCount.textContent = Number(stats?.video_count || 0).toLocaleString();
            els.historyTotalSize.textContent = formatFileSize(stats?.total_file_size_bytes);
            els.historyTotalDuration.textContent = formatDuration(Number(stats?.total_duration_seconds || 0));
            renderHistorySources(stats?.source_counts);
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
        item.className = 'pf-list-card pf-history-item';
        const entryLabel = entry.title || entry.filename || entry.url || 'download';
        const openBtn = document.createElement('button');
        openBtn.type = 'button';
        openBtn.className = `pf-history-open-btn pf-list-card-layout ${entry.thumbnail ? '' : 'pf-no-media'}`;
        openBtn.setAttribute('aria-label', `Open downloaded file: ${entryLabel}`);

        openBtn.onclick = async () => {
            // Rust uses snake_case: output_path, not outputPath
            const outputPath = entry.output_path || entry.outputPath;
            if (outputPath && invoke) {
                try {
                    const exists = await invoke('open_file_path', { path: outputPath });
                    if (!exists) {
                        appendLog(`[history] File not found: ${outputPath}`, true);
                    }
                } catch (err) {
                    appendLog(`[open] ${err}`, true);
                }
            }
        };

        const content = document.createElement('div');
        content.className = 'pf-history-content';

        const title = document.createElement('div');
        title.className = 'pf-history-title';
        title.textContent = entryLabel;
        content.appendChild(title);

        const meta = document.createElement('div');
        meta.className = 'pf-history-meta';
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
            thumb.className = 'pf-media-thumbnail pf-history-thumb';
            thumb.style.backgroundImage = `url('${entry.thumbnail}')`;
            openBtn.appendChild(thumb);
        }
        item.appendChild(openBtn);

        const removeBtn = document.createElement('button');
        removeBtn.className = 'pf-icon-btn pf-icon-btn-danger pf-history-item-remove-btn';
        removeBtn.textContent = '×';
        removeBtn.title = 'Remove from history';
        removeBtn.setAttribute('aria-label', `Remove from history: ${entryLabel}`);
        removeBtn.onclick = async event => {
            event.stopPropagation();
            try {
                await invoke('remove_history_entry', { id: entry.id });
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

        if (!invoke) {
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
        let needsFollowUpRefresh = false;
        const offset = append ? state.historyOffset : 0;
        setHistoryLoading(true);

        try {
            const page = await invoke('get_history', {
                limit: historyPageSize,
                offset,
                ...(requestedQuery ? { query: requestedQuery } : {}),
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

            els.historyHint.textContent = requestedQuery
                ? `No history results for “${requestedQuery}”.`
                : 'No history yet. Downloaded items will appear here.';
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
        els.loadMoreHistoryBtn.addEventListener('click', () => {
            void renderHistory({ append: true });
        });
        els.historySearchInput.addEventListener('input', () => {
            if (historySearchTimer !== null) clearTimeout(historySearchTimer);
            historySearchTimer = setTimeout(() => {
                historySearchTimer = null;
                const query = els.historySearchInput.value.trim();
                if (query === state.historyQuery) return;
                state.historyQuery = query;
                invalidateHistoryCache();
                void renderHistory({ force: true });
            }, historySearchDelayMs);
        });
        els.clearHistoryBtn.addEventListener('click', async () => {
            if (!invoke || state.historyClearing) return;
            if (!window.confirm('Delete all history entries and saved transcripts? This cannot be undone. Downloaded files will stay on disk.')) return;

            state.historyClearing = true;
            els.clearHistoryBtn.disabled = true;
            setHistoryActionStatus('Deleting history...');
            try {
                await invoke('clear_history');
                if (historySearchTimer !== null) clearTimeout(historySearchTimer);
                historySearchTimer = null;
                els.historySearchInput.value = '';
                state.historyQuery = '';
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
        invalidateHistoryCache();
        if (isActive()) void renderHistory({ force: true });
    };

    return Object.freeze({ render: renderHistory, bindEvents, onChanged });
};
