import { searchContains, trimSearchQuery } from './unicode-search.js';
import { createTextEntry, highlightText, copyAllText, bindTextSearch, revealTextEntry } from './history-text-view.js';
import { textTimestamp as timestamp, entryAtTime } from './history-text.js';
import { formatLocalDateTime, formatMediaTime } from './formatters.js';
import { ocrActive, ocrErrorMessage, ocrStatus } from './history-ocr-jobs.js';
import { confirmDialog, initTooltips, toast } from './vendor/oj/index.js';

const statusLabels = {
    queued: 'OCR pending',
    running: 'Checking for OCR…',
    completed: 'OCR complete',
    failed: 'OCR failed',
    cancelled: 'OCR cancelled',
    interrupted: 'OCR interrupted',
};

export const ocrEntries = (frames, fallbackText = '') => {
    const rows = (Array.isArray(frames) ? frames : []).map(frame => ({
        id: frame.id,
        time: frame.timestamp_ms,
        text: frame.full_text,
    }));
    if (fallbackText) rows.push({ id: null, time: null, text: fallbackText });
    return rows;
};

export const createHistoryOcrView = ({ els, api, appendLog, jobs, media }) => {
    const state = {
        id: null,
        request: 0,
        content: undefined,
        loading: false,
        startError: '',
        cancelling: false,
        disposed: false,
        layoutAttempt: null,
        timeCharacters: 8,
        frameCharacters: 1,
        rows: [],
        frames: [],
        pageRequest: 0,
        pageLoading: false,
        loadedQuery: '',
        copying: false,
        pendingSelection: null,
        renderedRows: null,
        rowElements: new Map(),
        cleanupRows: () => {},
        emptyMessage: '',
        selected: null,
        frameRequest: 0,
        frameLoading: false,
        frameImage: '',
        frameError: '',
        deleting: false,
        cleaning: false,
        cleanupRequest: 0,
        deleteRequest: 0,
        actionError: '',
    };
    const visible = () => !els.historyDetailsView.hidden && els.historyOcrTab.getAttribute('aria-selected') === 'true';
    const hasResult = () => state.content?.result_created_at !== null && state.content?.result_created_at !== undefined;
    const rowKey = row => row.id || 'legacy-text';
    const selection = row => ({
        result_revision: state.content.result_revision,
        frame_id: row.id,
    });
    const resultRows = content => {
        const query = trimSearchQuery(els.historyOcrSearchInput.value);
        const fallback = content?.text && (!query || searchContains(content.text, query)) ? content.text : '';
        return ocrEntries(state.frames, content?.has_more ? '' : fallback);
    };
    const clearFrame = () => {
        state.frameRequest += 1;
        state.selected = null;
        state.frameLoading = false;
        state.frameImage = '';
        state.frameError = '';
    };
    const renderPreview = () => {
        els.historyOcrPreview.setAttribute('aria-busy', String(state.frameLoading));
        media.setFrame(state.frameImage);
        if (state.frameImage) els.historyOcrFrame.alt = `Saved video frame at ${timestamp(state.selected.time)}`;
        els.historyOcrFrameStatus.hidden = Boolean(state.frameImage) || media.hasVideo();
        els.historyOcrFrameStatus.textContent = state.frameLoading
            ? 'Loading selected frame…'
            : state.frameError || state.emptyMessage || 'No frame selected.';
        els.historyOcrFrameTime.textContent = state.selected
            ? state.selected.time === null
                ? '—'
                : timestamp(state.selected.time)
            : '';
        els.historyOcrFrameTime.title =
            state.selected?.time !== null && state.selected ? timestamp(state.selected.time) : '';
        els.historyOcrActionNotice.hidden = !state.actionError;
        els.historyOcrActionNotice.textContent = state.actionError;
    };
    const updateRowControls = () => {
        const selectedKey = state.selected ? rowKey(state.selected) : null;
        const busy =
            state.loading ||
            state.pageLoading ||
            state.deleting ||
            state.cleaning ||
            ocrActive(jobs.get(state.id)?.status);
        for (const [key, { item, time, remove }] of state.rowElements) {
            const selected = key === selectedKey;
            item.setAttribute('aria-current', String(selected));
            time.setAttribute('aria-pressed', String(selected));
            time.tabIndex = selected ? 0 : -1;
            remove.tabIndex = selected ? 0 : -1;
            remove.disabled = time.disabled || busy || state.content?.last_attempt_status === 'running';
        }
    };
    const selectFrame = async row => {
        if (!visible()) return;
        if (row.time !== null) media.seek(row.time / 1000);
        if (
            state.selected &&
            rowKey(state.selected) === rowKey(row) &&
            state.selected.revision === state.content?.result_revision &&
            (state.frameLoading || state.frameImage || media.hasVideo())
        )
            return;
        const id = state.id;
        const request = ++state.frameRequest;
        state.selected = { ...row, revision: state.content?.result_revision };
        state.frameLoading =
            row.time !== null && Boolean(row.id) && Boolean(state.content?.result_revision) && !media.hasVideo();
        state.frameImage = '';
        state.frameError = state.frameLoading || media.hasVideo() ? '' : 'No video frame is available for this text.';
        updateRowControls();
        renderPreview();
        if (!state.frameLoading) return;
        const selected = selection(row);
        let timer;
        try {
            const image = await Promise.race([
                api.getHistoryOcrFrame({ id, selection: selected }),
                new Promise((_, reject) => {
                    timer = setTimeout(() => reject(new Error('Frame preview timed out.')), 30000);
                }),
            ]);
            if (id !== state.id || request !== state.frameRequest || !visible() || state.disposed) return;
            state.frameImage = image;
        } catch (error) {
            appendLog(`[OCR frame] ${error}`, true);
            if (id !== state.id || request !== state.frameRequest || !visible() || state.disposed) return;
            state.frameError = /timed out/i.test(String(error))
                ? 'The frame took too long to load. Select the text to try again.'
                : /changed/i.test(String(error))
                  ? 'The saved video or OCR results changed. Reopen OCR or run the analysis again.'
                  : 'The selected frame is unavailable. Check that the saved video and FFmpeg are available.';
        } finally {
            clearTimeout(timer);
        }
        state.frameLoading = false;
        renderPreview();
    };
    const deleteRow = async row => {
        if (
            !row.id ||
            state.deleting ||
            state.cleaning ||
            state.loading ||
            state.pageLoading ||
            ocrActive(jobs.get(state.id)?.status)
        )
            return;
        const id = state.id;
        const selected = selection(row);
        if (
            !(await confirmDialog({
                title: 'Delete frame text?',
                message: `Delete all OCR text from the frame at ${timestamp(row.time)}? Other frames and the saved video will stay unchanged.`,
                confirmLabel: 'Delete frame text',
                variant: 'danger',
            }))
        )
            return;
        if (id !== state.id || selected.result_revision !== state.content?.result_revision || state.disposed) return;
        const request = ++state.deleteRequest;
        state.request += 1;
        state.pageRequest += 1;
        state.deleting = true;
        state.actionError = '';
        const current = state.selected;
        const position = current ? state.rows.findIndex(entry => rowKey(entry) === rowKey(current)) : 0;
        render();
        try {
            const content = await api.deleteHistoryOcrFrameText({ id, selection: selected });
            if (id !== state.id || request !== state.deleteRequest || state.disposed) return;
            state.content = content;
            clearFrame();
            state.pendingSelection = { id: current?.id !== row.id ? current?.id : null, index: Math.max(0, position) };
            await loadPage(false);
        } catch (error) {
            appendLog(`[OCR delete] ${error}`, true);
            if (id !== state.id || request !== state.deleteRequest || state.disposed) return;
            state.actionError = /changed|no longer/i.test(String(error))
                ? 'OCR results changed. Reopen OCR and try again.'
                : 'The frame text could not be deleted. Try again after analysis finishes.';
        }
        if (id !== state.id || request !== state.deleteRequest || state.disposed) return;
        state.deleting = false;
        render();
        if (visible()) {
            const focused = state.actionError
                ? state.rowElements.get(rowKey(row))?.remove
                : state.selected
                  ? state.rowElements.get(rowKey(state.selected))?.time
                  : null;
            (focused || els.historyOcrListViewport).focus({ preventScroll: true });
        }
        const currentJob = jobs.get(id);
        if (currentJob?.attempt_id && currentJob.attempt_id !== state.content?.last_attempt_id) void load();
    };
    const renderResults = () => {
        els.historyOcrCopyBtn.disabled =
            !hasResult() ||
            (!state.content.total_count && !state.content.text) ||
            state.copying ||
            state.deleting ||
            state.cleaning;
        els.historyOcrCopyBtn.setAttribute('aria-busy', String(state.copying));
        els.historyOcrDuplicatesBtn.disabled =
            !hasResult() ||
            !state.content?.result_revision ||
            !state.content?.total_count ||
            state.loading ||
            state.pageLoading ||
            state.deleting ||
            state.cleaning ||
            state.copying ||
            !api.available ||
            ocrActive(jobs.get(state.id)?.status) ||
            state.content?.last_attempt_status === 'running';
        els.historyOcrDuplicatesBtn.setAttribute('aria-busy', String(state.cleaning));
        els.historyOcrDuplicatesBtn.textContent = state.cleaning ? 'Checking duplicates…' : 'Remove All Duplicates';
        els.historyOcrSearchInput.disabled = !hasResult() || state.cleaning || state.deleting;
        const rows = state.rows;
        if (
            state.selected &&
            (state.selected.revision !== state.content?.result_revision ||
                !rows.some(row => rowKey(row) === rowKey(state.selected)))
        )
            clearFrame();
        if (state.renderedRows !== rows) {
            state.renderedRows = rows;
            const scrollTop = els.historyOcrListViewport.scrollTop;
            const focusedItem = document.activeElement?.closest('[data-ocr-key]');
            const focusedKey = els.historyOcrText.contains(focusedItem) ? focusedItem?.dataset.ocrKey : null;
            const fragment = document.createDocumentFragment();
            state.cleanupRows();
            state.rowElements.clear();
            for (const row of rows) {
                const { item, time, text } = createTextEntry(row, trimSearchQuery(els.historyOcrSearchInput.value));
                item.classList.add('pinefetch-ocr-result');
                item.dataset.ocrKey = rowKey(row);
                time.classList.add('pinefetch-ocr-select');
                time.disabled = row.time === null || !row.id || !state.content.result_revision;
                text.id = `ocr-text-${rowKey(row)}`;
                time.setAttribute('aria-describedby', text.id);
                const remove = document.createElement('button');
                remove.type = 'button';
                remove.className = 'oj-icon-button oj-button-compact pinefetch-ocr-delete';
                remove.setAttribute('aria-label', `Delete frame text at ${timestamp(row.time)}`);
                remove.setAttribute('data-oj-tooltip', 'Delete frame text');
                const icon = document.createElement('i');
                icon.className = 'fa-solid fa-trash-can';
                icon.setAttribute('aria-hidden', 'true');
                remove.append(icon);
                item.append(time, text, remove);
                state.rowElements.set(rowKey(row), { item, time, remove, row });
                fragment.append(item);
            }
            els.historyOcrText.replaceChildren(fragment);
            state.cleanupRows = initTooltips(els.historyOcrText);
            els.historyOcrListViewport.scrollTop = scrollTop;
            if (focusedKey) state.rowElements.get(focusedKey)?.time.focus({ preventScroll: true });
        }
        const query = trimSearchQuery(els.historyOcrSearchInput.value);
        for (const { item, row } of state.rowElements.values())
            highlightText(item.querySelector('.oj-text'), row.text, query);
        const loaded = !state.pageLoading && state.loadedQuery === query;
        els.historyOcrResultCount.hidden = !hasResult();
        const count = query ? state.content?.matched_count : state.content?.total_count;
        els.historyOcrResultCount.textContent = loaded
            ? `${(count || 0).toLocaleString()} ${count === 1 ? 'frame' : 'frames'} ${query ? 'found' : 'with text'}${state.content?.has_more ? ` · ${state.frames.length.toLocaleString()} shown` : ''}${state.content?.text ? ' · Legacy text available' : ''}`
            : state.pageLoading
              ? 'Loading frames…'
              : '';
        state.emptyMessage = rows.length
            ? ''
            : query && loaded
              ? 'No detected text matches your search.'
              : state.loading || state.pageLoading
                ? 'Loading OCR…'
                : hasResult()
                  ? 'No text was detected in the sampled video frames.'
                  : 'No saved OCR text.';
        els.historyOcrEmpty.hidden = rows.length > 0;
        els.historyOcrEmpty.textContent = state.emptyMessage;
        els.historyOcrLoadMoreBtn.hidden = !state.content?.has_more;
        els.historyOcrLoadMoreBtn.disabled = state.pageLoading || state.loading || state.deleting || state.cleaning;
        els.historyOcrListViewport.setAttribute('aria-busy', String(state.pageLoading));
        updateRowControls();
        renderPreview();
        if (
            visible() &&
            !state.pageLoading &&
            rows.length &&
            (!state.selected || (!state.frameImage && !state.frameLoading && !state.frameError))
        ) {
            const preferred = state.pendingSelection;
            state.pendingSelection = null;
            const next =
                state.selected ||
                (preferred?.id && rows.find(row => row.id === preferred.id)) ||
                rows[Math.min(preferred?.index || 0, rows.length - 1)];
            void selectFrame(next);
        }
    };
    const scrollRowIntoList = item => revealTextEntry(els.historyOcrListViewport, item);
    const render = () => {
        const content = state.content;
        const progress = jobs.get(state.id);
        const status = progress?.status || ocrStatus(content?.last_attempt_status);
        const active = ocrActive(status);
        const running = status === 'running';
        const duration = progress?.total_duration_ms;
        const hasDuration = Number.isFinite(duration) && duration > 0;
        const observedTime = Number.isFinite(progress?.observed_at_ms) ? Math.max(0, progress.observed_at_ms) : 0;
        const totalFrames = progress?.total_frames;
        const hasTotalFrames = Number.isSafeInteger(totalFrames) && totalFrames > 0;
        const finishing = running && hasTotalFrames && progress.total_frames_is_estimate === false;
        const statusLabel = finishing
            ? 'Finishing OCR…'
            : state.loading && !progress
              ? 'Loading OCR…'
              : statusLabels[status] || 'Not checked yet';
        if (els.historyOcrStatus.textContent !== statusLabel) els.historyOcrStatus.textContent = statusLabel;
        const error = state.startError || progress?.message || (!active ? content?.last_error : '');
        els.historyOcrNotice.textContent = error ? ocrErrorMessage(error) : '';
        els.historyOcrNotice.hidden = !error || (status === 'cancelled' && !state.startError);
        els.historyOcrStartBtn.hidden = active;
        els.historyOcrStartBtn.disabled =
            state.loading || state.pageLoading || state.deleting || state.cleaning || !api.available;
        els.historyOcrStartBtn.textContent =
            status === 'failed' || status === 'interrupted'
                ? 'Retry Analysis'
                : status === 'cancelled'
                  ? 'Start Again'
                  : hasResult()
                    ? 'Run OCR Again'
                    : 'Start OCR Analysis';
        els.historyOcrProgress.hidden = !active;
        els.historyOcrAnalysis.hidden = !active;
        els.historyOcrProgress.max = 1;
        if (running && hasDuration) {
            // Follow actual media PTS, retaining final source checks below 100%.
            els.historyOcrProgress.value = finishing ? 0.999 : Math.min(0.999, observedTime / duration);
        } else {
            els.historyOcrProgress.removeAttribute('value');
        }
        const showCounters = running && (hasDuration || progress?.frames_checked > 0);
        els.historyOcrPreparing.textContent =
            status === 'queued' ? 'Waiting for OCR to start…' : 'Preparing the saved video…';
        els.historyOcrPreparing.hidden = !active || showCounters;
        els.historyOcrProgressText.hidden = !showCounters;
        els.historyOcrProgress.setAttribute(
            'aria-describedby',
            showCounters ? 'historyOcrProgressText' : 'historyOcrPreparing'
        );
        if (active && state.layoutAttempt !== progress?.attempt_id) {
            state.layoutAttempt = progress?.attempt_id;
            state.timeCharacters = 8;
            state.frameCharacters = 1;
        }
        if (showCounters) {
            const time = formatMediaTime(observedTime);
            const timeTotal = hasDuration ? formatMediaTime(duration) : 'Unknown';
            const frames = String(progress?.frames_checked || 0);
            const framesTotal = hasTotalFrames ? String(totalFrames) : 'Unknown';
            // Keep reserved widths if extraction's final total has fewer digits.
            state.timeCharacters = Math.max(state.timeCharacters, time.length, timeTotal.length);
            state.frameCharacters = Math.max(state.frameCharacters, frames.length, framesTotal.length);
            els.historyOcrProgressText.style.setProperty('--pinefetch-ocr-time-width', `${state.timeCharacters}ch`);
            els.historyOcrProgressText.style.setProperty('--pinefetch-ocr-frame-width', `${state.frameCharacters}ch`);
            els.historyOcrTime.textContent = time;
            els.historyOcrTimeTotal.textContent = timeTotal;
            els.historyOcrFrames.textContent = frames;
            els.historyOcrFramesTotal.textContent = framesTotal;
        }
        els.historyOcrCancelBtn.hidden = !active;
        els.historyOcrCancelBtn.disabled = !progress?.attempt_id || state.cancelling;
        renderResults();
        els.historyOcrAnalyzed.textContent = hasResult()
            ? formatLocalDateTime(new Date(content.result_created_at))
            : '—';
        els.historyOcrPreviousResult.hidden = !hasResult() || status === 'completed';
    };
    const deactivate = () => {
        state.request += 1;
        state.pageRequest += 1;
        state.pageLoading = false;
        if (state.frameLoading) {
            state.frameRequest += 1;
            state.frameLoading = false;
            state.frameError = '';
        }
    };
    const reset = id => {
        state.id = id;
        state.request += 1;
        state.content = undefined;
        state.loading = false;
        state.startError = '';
        state.cancelling = false;
        state.deleting = false;
        state.cleaning = false;
        state.cleanupRequest += 1;
        state.deleteRequest += 1;
        state.actionError = '';
        clearFrame();
        state.layoutAttempt = null;
        state.timeCharacters = 8;
        state.frameCharacters = 1;
        state.rows = [];
        state.frames = [];
        state.pageRequest += 1;
        state.pageLoading = false;
        state.loadedQuery = '';
        state.copying = false;
        state.pendingSelection = null;
        state.renderedRows = null;
        els.historyOcrListViewport.scrollTop = 0;
        els.historyOcrSearchInput.value = '';
        render();
    };
    const loadPage = async append => {
        if (!state.id || !visible() || state.disposed || !state.content?.result_revision) return;
        const id = state.id;
        const request = ++state.pageRequest;
        const query = trimSearchQuery(els.historyOcrSearchInput.value);
        const revision = state.content.result_revision;
        const offset = append ? state.frames.length : 0;
        state.pageLoading = true;
        state.actionError = '';
        if (!append) {
            if (!state.pendingSelection) state.pendingSelection = { id: state.selected?.id, index: 0 };
            state.frames = [];
            state.rows = [];
            clearFrame();
            els.historyOcrListViewport.scrollTop = 0;
        }
        render();
        try {
            const content = await api.getHistoryOcr({
                id,
                limit: 100,
                offset,
                search: query,
                resultRevision: revision,
            });
            if (id !== state.id || request !== state.pageRequest || !visible() || state.disposed) return;
            if (!content || content.result_revision !== revision) throw new Error('OCR results changed.');
            state.content = content;
            state.frames = append ? [...state.frames, ...content.frames] : content.frames;
            state.loadedQuery = query;
            state.rows = resultRows(content);
        } catch (error) {
            appendLog(`[OCR frames] ${error}`, true);
            if (id !== state.id || request !== state.pageRequest || state.disposed) return;
            state.actionError = /changed/i.test(String(error))
                ? 'OCR results changed. Reloading frames…'
                : 'OCR frames could not be loaded. Try again.';
            if (/changed/i.test(String(error))) void load();
        } finally {
            if (id === state.id && request === state.pageRequest && !state.disposed) {
                state.pageLoading = false;
                render();
            }
        }
    };
    const load = async () => {
        if (!state.id || !visible() || state.disposed || state.deleting || state.cleaning) return;
        const id = state.id;
        const request = ++state.request;
        state.pageRequest += 1;
        state.pageLoading = false;
        const revision = jobs.revision(id);
        const query = trimSearchQuery(els.historyOcrSearchInput.value);
        state.loading = true;
        render();
        try {
            const [content, progress] = await Promise.all([
                api.getHistoryOcr({ id, limit: 100, offset: 0, search: query }),
                api.getHistoryOcrProgress({ id }),
            ]);
            if (id !== state.id || request !== state.request || !visible() || state.disposed) return;
            state.content = content || null;
            state.frames = content?.frames || [];
            state.loadedQuery = query;
            state.rows = resultRows(content);
            state.loading = false;
            const applied = jobs.snapshot(id, content, progress, revision);
            render();
            if (!applied && !ocrActive(jobs.get(id)?.status)) void load();
        } catch (error) {
            if (id !== state.id || request !== state.request || !visible() || state.disposed) return;
            state.loading = false;
            appendLog(`[OCR] ${error}`, true);
            state.startError = String(error);
            render();
        }
    };
    const removeDuplicates = async () => {
        if (els.historyOcrDuplicatesBtn.disabled) return;
        const id = state.id;
        const revision = state.content.result_revision;
        const request = ++state.cleanupRequest;
        const current = () => id === state.id && request === state.cleanupRequest && !state.disposed;
        state.cleaning = true;
        state.actionError = '';
        render();
        try {
            const preview = await api.previewHistoryOcrDuplicates({ id });
            if (!current() || !visible()) return;
            if (preview.result_revision !== revision) throw new Error('OCR results changed.');
            if (!preview.duplicate_count) {
                toast('No duplicate OCR frames found.');
                return;
            }
            const confirmed = await confirmDialog({
                title: 'Remove all duplicate OCR frames?',
                message: `Remove ${preview.duplicate_count.toLocaleString()} duplicate OCR ${preview.duplicate_count === 1 ? 'frame' : 'frames'} from all saved results? The first frame of each consecutive repetition is kept. The saved video stays unchanged.`,
                confirmLabel: 'Remove duplicates',
                variant: 'danger',
            });
            if (!confirmed || !current() || !visible()) return;
            if (state.content?.result_revision !== revision || ocrActive(jobs.get(id)?.status))
                throw new Error('OCR results changed or analysis is running.');
            const selected = state.selected;
            const index = Math.max(
                0,
                state.rows.findIndex(row => row.id === selected?.id)
            );
            state.request += 1;
            state.pageRequest += 1;
            const result = await api.removeHistoryOcrDuplicates({ id, resultRevision: preview.result_revision });
            if (!current()) return;
            state.content = result.content;
            clearFrame();
            state.pendingSelection = { id: selected?.id, index };
            await loadPage(false);
            if (!current()) return;
            toast(
                result.removed_count
                    ? `Removed ${result.removed_count.toLocaleString()} duplicate OCR ${result.removed_count === 1 ? 'frame' : 'frames'}.`
                    : 'No duplicate OCR frames found.',
                { type: 'success' }
            );
        } catch (error) {
            appendLog(`[OCR duplicates] ${error}`, true);
            if (!current()) return;
            state.actionError = /changed|no longer/i.test(String(error))
                ? 'OCR results changed. Reopen OCR and try again.'
                : 'Duplicate OCR frames could not be removed. Wait for analysis to finish and try again.';
        } finally {
            if (current()) {
                state.cleaning = false;
                render();
                if (visible())
                    (els.historyOcrDuplicatesBtn.disabled
                        ? els.historyOcrListViewport
                        : els.historyOcrDuplicatesBtn
                    ).focus({ preventScroll: true });
                const job = jobs.get(id);
                if (job?.attempt_id && job.attempt_id !== state.content?.last_attempt_id) void load();
            }
        }
    };
    const bindEvents = () => {
        els.historyOcrPreview.addEventListener('pinefetch:media-unavailable', () => {
            if (visible() && state.selected) void selectFrame(state.selected);
        });
        els.historyOcrDuplicatesBtn.addEventListener('click', () => void removeDuplicates());
        els.historyOcrStartBtn.addEventListener('click', async () => {
            const id = state.id;
            if (!id || state.loading || state.deleting || state.cleaning) return;
            state.startError = '';
            try {
                await jobs.start(id);
            } catch (error) {
                appendLog(`[OCR] ${error}`, true);
                if (state.id === id) state.startError = String(error);
            }
            if (state.id === id) {
                render();
                void load();
            }
        });
        els.historyOcrCancelBtn.addEventListener('click', async () => {
            const id = state.id;
            const attemptId = jobs.get(id)?.attempt_id;
            if (!id || !attemptId) return;
            state.cancelling = true;
            render();
            try {
                await api.cancelHistoryOcr({ id, attemptId });
            } catch (error) {
                appendLog(`[OCR cancel] ${error}`, true);
                if (state.id === id) state.startError = String(error);
            }
            if (state.id === id) {
                state.cancelling = false;
                render();
                void load();
            }
        });
        els.historyOcrCopyBtn.addEventListener('click', async () => {
            if (!hasResult() || state.copying || state.deleting || state.cleaning) return;
            const id = state.id;
            const revision = state.content.result_revision;
            state.copying = true;
            renderResults();
            try {
                await copyAllText({
                    getText: () => api.getHistoryOcrText({ id, resultRevision: revision }),
                    isCurrent: () => id === state.id && state.content?.result_revision === revision && !state.disposed,
                    status: els.historyOcrActionNotice,
                    appendLog,
                });
            } finally {
                if (id === state.id) {
                    state.copying = false;
                    els.historyOcrActionNotice.hidden = false;
                    state.actionError = els.historyOcrActionNotice.textContent;
                    renderResults();
                }
            }
        });
        bindTextSearch(els.historyOcrSearchInput, () => {
            state.request += 1;
            state.loading = false;
            state.pendingSelection = null;
            void loadPage(false);
        });
        els.historyOcrText.addEventListener('click', event => {
            const item = event.target.closest('[data-ocr-key]');
            const entry = state.rowElements.get(item?.dataset.ocrKey);
            if (!entry) return;
            if (entry.remove.contains(event.target)) {
                if (!entry.remove.disabled) void deleteRow(entry.row);
                return;
            }
            entry.time.focus({ preventScroll: true });
            void selectFrame(entry.row);
        });
        els.historyOcrListViewport.addEventListener('keydown', async event => {
            if (
                !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key) ||
                event.altKey ||
                event.ctrlKey ||
                event.metaKey ||
                event.shiftKey
            )
                return;
            if (event.target !== els.historyOcrListViewport && !event.target.closest('.pinefetch-ocr-select')) return;
            if (!state.rows.length || state.pageLoading) return;
            event.preventDefault();
            const id = state.id;
            const query = trimSearchQuery(els.historyOcrSearchInput.value);
            const current = state.selected ? state.rows.findIndex(row => rowKey(row) === rowKey(state.selected)) : -1;
            if (event.key === 'ArrowDown' && current === state.rows.length - 1 && state.content?.has_more)
                await loadPage(true);
            if (
                id !== state.id ||
                !visible() ||
                query !== trimSearchQuery(els.historyOcrSearchInput.value) ||
                !state.rows.length
            )
                return;
            const index =
                event.key === 'Home'
                    ? 0
                    : event.key === 'End'
                      ? state.rows.length - 1
                      : event.key === 'ArrowDown'
                        ? Math.min(state.rows.length - 1, current + 1)
                        : Math.max(0, current - 1);
            const row = state.rows[index];
            const entry = state.rowElements.get(rowKey(row));
            entry?.time.focus({ preventScroll: true });
            void selectFrame(row);
            if (entry) scrollRowIntoList(entry.item);
        });
        els.historyOcrLoadMoreBtn.addEventListener('click', () => {
            void loadPage(true);
        });
        els.historyOcrFrame.addEventListener('error', () => {
            state.frameImage = '';
            state.frameError = 'The selected frame could not be displayed. Select the text to try again.';
            renderPreview();
        });
        const unsubscribe = jobs.subscribe((id, progress) => {
            if (id !== state.id || state.disposed) return;
            render();
            if (
                progress &&
                !ocrActive(progress.status) &&
                visible() &&
                (state.content?.last_attempt_id !== progress.attempt_id ||
                    ocrStatus(state.content?.last_attempt_status) !== progress.status)
            )
                void load();
        });
        window.addEventListener(
            'pagehide',
            () => {
                state.disposed = true;
                state.request += 1;
                state.pageRequest += 1;
                state.frameRequest += 1;
                state.cleanupRows();
                unsubscribe();
            },
            { once: true }
        );
    };
    const syncTime = seconds => {
        const row = entryAtTime(state.rows, seconds * 1000);
        if (!row || rowKey(row) === rowKey(state.selected || {})) return;
        state.frameRequest++;
        state.frameLoading = false;
        state.frameImage = '';
        state.frameError = '';
        state.selected = { ...row, revision: state.content?.result_revision };
        updateRowControls();
        els.historyOcrFrameTime.textContent = timestamp(row.time);
        const entry = state.rowElements.get(rowKey(row));
        if (entry && !els.historyOcrListViewport.matches(':focus-within')) scrollRowIntoList(entry.item);
    };
    return Object.freeze({ reset, load, deactivate, bindEvents, syncTime });
};
