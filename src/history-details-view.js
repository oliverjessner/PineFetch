import { formatHistorySource, formatLocalDateTime, formatUploadDate } from './formatters.js';
import { initTabs, toast } from './vendor/oj/index.js';
import { createHistoryOcrView } from './history-ocr-view.js';
import { createHistorySocialInsightsView } from './history-social-insights-view.js';
import { createHistoryKeywordsView } from './history-keywords-view.js';
import { createHistorySubtitlesView } from './history-subtitles-view.js';
import { copyAllText, createTextReader } from './history-text-view.js';
import { createHistoryMediaView } from './history-media-view.js';
import { createHistoryAiView } from './history-ai-view.js';
import { transcriptionActive, transcriptionErrorMessage } from './history-transcription-jobs.js';

export const createHistoryDetailsView = ({
    els,
    api,
    appendLog,
    formatFileSize,
    formatDuration,
    detectPlatform,
    showSavedVideosByCreator,
    openAccount,
    jobs,
    transcriptionJobs,
    navigateToDetails,
    navigateToHistory,
    tags,
}) => {
    const state = {
        contextEntryId: null,
        contextEntryUrl: '',
        contextCreator: null,
        contextReturnFocus: null,
        details: null,
        detailsRequestId: 0,
        transcript: undefined,
        captions: new Map(),
        entryId: null,
        returnFocus: null,
    };
    let transcriptReader;
    let ocrView;
    let subtitlesView;
    const transcriptMedia = createHistoryMediaView({
        root: els.historyTranscriptPreview,
        api,
        appendLog,
        onTime: seconds => transcriptReader?.sync(seconds),
    });
    const ocrMedia = createHistoryMediaView({
        root: els.historyOcrPreview,
        api,
        appendLog,
        onTime: seconds => ocrView?.syncTime(seconds),
    });
    const subtitleMedia = createHistoryMediaView({
        root: els.historySubtitlePreview,
        api,
        appendLog,
        onTime: seconds => subtitlesView?.syncTime(seconds),
    });
    transcriptReader = createTextReader({
        input: els.historyTranscriptSearchInput,
        copy: els.historyTranscriptCopyBtn,
        status: els.historyTranscriptStatus,
        count: els.historyTranscriptResultCount,
        viewport: els.historyTranscriptListViewport,
        list: els.historyTranscriptText,
        appendLog,
        onSelect: seconds => transcriptMedia.seek(seconds),
    });
    ocrView = createHistoryOcrView({ els, api, appendLog, jobs, media: ocrMedia });
    const socialView = createHistorySocialInsightsView({ els, api, appendLog });
    subtitlesView = createHistorySubtitlesView({ els, api, appendLog, media: subtitleMedia });
    const keywordsView = createHistoryKeywordsView({ els, api, appendLog });
    const aiView = createHistoryAiView({
        root: els.historyAiPanel,
        api,
        appendLog,
        onTranscribe: id => {
            if (id !== state.entryId) return;
            const requestId = state.detailsRequestId;
            els.historyTranscriptTab.click();
            els.historyTranscriptTab.focus({ preventScroll: true });
            void transcriptionJobs.load(id).then(() => {
                if (
                    id === state.entryId &&
                    requestId === state.detailsRequestId &&
                    document.activeElement === els.historyTranscriptTab &&
                    !els.historyTranscribeBtn.disabled
                )
                    els.historyTranscribeBtn.focus({ preventScroll: true });
            });
        },
    });
    let transcriptRefresh = null;

    const renderTranscription = () => {
        const record = transcriptionJobs.get(state.entryId);
        const progress = record?.progress;
        const active = transcriptionActive(progress);
        const saved = Boolean(state.details?.transcript);
        const controls = els.historyTranscriptionControls;
        controls.hidden = !state.details || saved;
        const button = els.historyTranscribeBtn;
        button.hidden = active;
        button.disabled = !api.available || !record?.can_transcribe || record.loading || active;
        button.setAttribute('aria-busy', String(active));
        const retry = Boolean(record?.error || ['failed', 'cancelled'].includes(progress?.status));
        button.querySelector('[data-transcription-label]').textContent = retry
            ? 'Retry Transcription'
            : 'Transcribe Video';
        const cancel = els.historyTranscriptionCancelBtn;
        const restoreCancelFocus = document.activeElement === cancel && !active;
        cancel.hidden = !active;
        cancel.disabled = !progress?.attempt_id || record?.cancelling;
        cancel.textContent = record?.cancelling ? 'Cancelling…' : 'Cancel';
        els.historyTranscriptionRefreshBtn.hidden = active;
        els.historyTranscriptionRefreshBtn.disabled = Boolean(record?.loading) || !api.available;
        els.historyTranscriptionRefreshBtn.setAttribute('aria-busy', String(Boolean(record?.loading)));
        const status = els.historyTranscriptionStatus;
        const failed = progress?.status === 'failed';
        status.classList.toggle('oj-status-error', Boolean(record?.error || failed));
        status.textContent = record?.error
            ? record.error
            : active
              ? record?.cancelling
                  ? 'Cancelling transcription…'
                  : progress.phase === 'saving'
                    ? 'Saving transcript…'
                    : progress.phase === 'transcribing'
                      ? `Transcribing locally${progress.model ? ` · Whisper ${progress.model}` : ''}…`
                      : 'Preparing local transcription…'
              : record?.loading
                ? 'Checking saved video…'
                : failed
                  ? transcriptionErrorMessage(progress.message)
                  : progress?.status === 'cancelled'
                    ? 'Transcription cancelled. You can try again.'
                    : record?.transcript_available
                      ? 'Loading saved transcript…'
                      : record && !record.can_transcribe
                        ? transcriptionErrorMessage(record.unavailable_reason)
                        : 'Transcribe the saved video locally with Whisper.';
        if (restoreCancelFocus && !button.disabled && els.historyTranscriptTab.getAttribute('aria-selected') === 'true')
            button.focus({ preventScroll: true });
    };

    const refreshSavedTranscript = async id => {
        const requestId = state.detailsRequestId;
        if (id !== state.entryId || (transcriptRefresh?.id === id && transcriptRefresh.requestId === requestId)) return;
        const refresh = { id, requestId };
        transcriptRefresh = refresh;
        try {
            const details = await api.getHistoryDetails({ id });
            if (id !== state.entryId || requestId !== state.detailsRequestId || !details) return;
            state.details = details;
            state.transcript = undefined;
            renderOverview(details);
            renderTranscription();
            if (els.historyTranscriptTab.getAttribute('aria-selected') === 'true') await loadTranscript();
            if (id !== state.entryId || requestId !== state.detailsRequestId) return;
            void aiView.refreshTranscript();
            if (els.historyKeywordsTab.getAttribute('aria-selected') === 'true') void keywordsView.load();
        } catch (error) {
            appendLog(`[transcription refresh] ${error}`, true);
            if (id === state.entryId && requestId === state.detailsRequestId)
                els.historyTranscriptStatus.textContent = 'Transcript saved. Reopen this video to load it.';
        } finally {
            if (transcriptRefresh === refresh) transcriptRefresh = null;
        }
    };

    const formatDateTime = timestamp => {
        const date = new Date(Number(timestamp));
        if (!Number.isFinite(Number(timestamp)) || Number.isNaN(date.getTime())) return null;
        return formatLocalDateTime(date);
    };

    const hideContextMenu = ({ restoreFocus = false } = {}) => {
        const returnFocus = state.contextReturnFocus;
        returnFocus?.setAttribute('aria-expanded', 'false');
        state.contextEntryId = null;
        state.contextEntryUrl = '';
        state.contextCreator = null;
        state.contextReturnFocus = null;
        els.historyContextMenu.hidden = true;
        els.historyContextMenu.style.left = '';
        els.historyContextMenu.style.top = '';
        if (restoreFocus && returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
    };

    const openContextMenu = (item, x, y, returnFocus) => {
        hideContextMenu();
        const entryId = item.dataset.historyId;
        state.contextEntryId = entryId;
        state.contextEntryUrl = item.dataset.historyUrl || '';
        state.contextCreator = {
            entryId,
            uploader: item.dataset.historyUploader || '',
            source: item.dataset.historySource || '',
        };
        state.contextReturnFocus = returnFocus;
        els.historyOpenInBrowserBtn.disabled = !state.contextEntryUrl;
        els.historyShowCreatorVideosBtn.disabled = !state.contextCreator.uploader || !state.contextCreator.source;
        returnFocus?.setAttribute('aria-expanded', 'true');
        els.historyContextMenu.hidden = false;

        requestAnimationFrame(() => {
            if (els.historyContextMenu.hidden || state.contextEntryId !== entryId) return;
            const margin = 12;
            const left = Math.max(margin, Math.min(x, window.innerWidth - els.historyContextMenu.offsetWidth - margin));
            const top = Math.max(
                margin,
                Math.min(y, window.innerHeight - els.historyContextMenu.offsetHeight - margin)
            );
            els.historyContextMenu.style.left = `${left}px`;
            els.historyContextMenu.style.top = `${top}px`;
            els.historyShowMoreDataBtn.focus({ preventScroll: true });
        });
    };

    const addOverviewRow = (label, value, title = null) => {
        if (value === null || value === undefined || value === '') return;
        const term = document.createElement('dt');
        term.textContent = label;
        const description = document.createElement('dd');
        description.textContent = `${value}`;
        if (title) description.title = title;
        els.historyOverviewList.append(term, description);
    };

    const renderOverview = details => {
        const entry = details.entry || {};
        const source = entry.source || detectPlatform(entry.url);
        const platform = `${entry.platform || ''}`.trim();
        const uploaded =
            Number(entry.timestamp) > 0
                ? formatDateTime(Number(entry.timestamp) * 1000)
                : formatUploadDate(entry.upload_date);
        const downloaded = formatDateTime(entry.completed_at || entry.created_at);
        const transcript = details.transcript;
        const transcriptLabel = transcript
            ? ['Saved', transcript.language?.toUpperCase(), transcript.file_available ? null : 'media file missing']
                  .filter(Boolean)
                  .join(' · ')
            : null;
        const captionCount = Array.isArray(details.captions) ? details.captions.length : 0;

        els.historyOverviewList.replaceChildren();
        addOverviewRow('Source', source ? formatHistorySource(source) : null);
        if (platform && platform.toLowerCase() !== `${source || ''}`.toLowerCase())
            addOverviewRow('Platform', platform);
        addOverviewRow('Original URL', entry.url, entry.url);
        addOverviewRow('Title', entry.title);
        if (entry.uploader) {
            const term = document.createElement('dt');
            term.textContent = 'Creator';
            const description = document.createElement('dd');
            const button = document.createElement('button');
            button.type = 'button';
            button.className = 'oj-button oj-button-ghost pinefetch-account-link';
            button.textContent = entry.uploader;
            button.disabled = !source;
            button.addEventListener('click', () => openAccount(entry.id, button));
            description.append(button);
            els.historyOverviewList.append(term, description);
        }
        addOverviewRow('Downloaded', downloaded);
        addOverviewRow('Uploaded', uploaded);
        addOverviewRow(
            'Duration',
            entry.duration_seconds !== null &&
                entry.duration_seconds !== undefined &&
                Number(entry.duration_seconds) >= 0
                ? formatDuration(Number(entry.duration_seconds))
                : null
        );
        addOverviewRow('Media type', entry.medium);
        if (entry.file_size_bytes != null) addOverviewRow('File size', formatFileSize(entry.file_size_bytes));
        addOverviewRow('Thumbnail', entry.thumbnail ? 'Available' : null);
        addOverviewRow('AI transcript', transcriptLabel);
        addOverviewRow(
            'Creator subtitles',
            details.subtitles?.length
                ? `${details.subtitles.length} ${details.subtitles.length === 1 ? 'track' : 'tracks'}`
                : null
        );
        addOverviewRow(captionTitle(), captionCount ? `${captionCount} saved` : null);
    };

    const renderTranscript = content => {
        els.historyTranscriptStatus.textContent = '';
        els.historyTranscriptNotice.hidden = true;
        transcriptReader.set(content?.text);
        if (!content) {
            if (state.details?.transcript) els.historyTranscriptStatus.textContent = 'Loading transcript…';
            return;
        }

        if (!content.file_available) {
            els.historyTranscriptNotice.textContent =
                'The downloaded file is missing. The saved AI transcript is still available.';
            els.historyTranscriptNotice.hidden = false;
        }
        if (!content.text) {
            els.historyTranscriptStatus.textContent = 'The stored transcript is empty.';
            return;
        }
    };

    const loadTranscript = async () => {
        if (!state.details?.transcript) {
            renderTranscript(null);
            return;
        }
        if (state.transcript !== undefined) {
            renderTranscript(state.transcript);
            return;
        }
        const entryId = state.details?.entry?.id;
        if (!entryId) return;
        const requestId = state.detailsRequestId;
        els.historyTranscriptStatus.textContent = 'Loading transcript...';
        els.historyTranscriptText.hidden = true;
        els.historyTranscriptCopyBtn.disabled = true;
        try {
            const content = await api.getHistoryTranscript({ id: entryId });
            if (
                requestId !== state.detailsRequestId ||
                state.entryId !== entryId ||
                state.details?.entry?.id !== entryId
            )
                return;
            state.transcript = content || null;
            renderTranscript(state.transcript);
        } catch (err) {
            appendLog(`[history details] ${err}`, true);
            if (requestId === state.detailsRequestId && state.entryId === entryId) {
                els.historyTranscriptStatus.textContent = 'Transcript could not be loaded.';
            }
        }
    };

    const captionTitle = () =>
        ['youtube', 'vimeo'].includes(state.details?.entry?.source || detectPlatform(state.details?.entry?.url))
            ? 'Video description'
            : 'Post caption';

    const renderCaptionOptions = captions => {
        const fragment = document.createDocumentFragment();
        captions.forEach((caption, index) => {
            const option = document.createElement('option');
            option.value = caption.media_path;
            option.textContent = `${captionTitle()} ${index + 1}`;
            fragment.appendChild(option);
        });
        els.historyCaptionTrackSelect.replaceChildren(fragment);
        els.historyCaptionTrackField.hidden = captions.length <= 1;
    };

    const renderCaption = content => {
        els.historyCaptionStatus.textContent = '';
        els.historyCaptionNotice.hidden = true;
        els.historyCaptionText.hidden = true;
        els.historyCaptionCopyBtn.disabled = true;
        if (!content) {
            els.historyCaptionStatus.textContent = `No saved ${captionTitle().toLowerCase()} is available.`;
            return;
        }

        if (content.caption_path && !content.file_available) {
            els.historyCaptionNotice.textContent =
                'The saved text file is missing. The stored text is still available.';
            els.historyCaptionNotice.hidden = false;
        }
        if (!content.text) {
            els.historyCaptionStatus.textContent = 'The saved text is empty.';
            return;
        }
        els.historyCaptionText.textContent = content.text;
        els.historyCaptionText.hidden = false;
        els.historyCaptionCopyBtn.disabled = false;
    };

    const loadSelectedCaption = async () => {
        const entryId = state.details?.entry?.id;
        const mediaPath = els.historyCaptionTrackSelect.value;
        if (!entryId || !mediaPath) return;
        const requestId = state.detailsRequestId;
        if (state.captions.has(mediaPath)) {
            renderCaption(state.captions.get(mediaPath));
            return;
        }
        renderCaption(null);
        els.historyCaptionStatus.textContent = 'Loading saved text...';
        els.historyCaptionText.hidden = true;
        els.historyCaptionCopyBtn.disabled = true;
        try {
            const content = await api.getHistoryCaption({ id: entryId, mediaPath });
            if (
                requestId !== state.detailsRequestId ||
                state.entryId !== entryId ||
                state.details?.entry?.id !== entryId ||
                els.historyCaptionTrackSelect.value !== mediaPath
            )
                return;
            state.captions.set(mediaPath, content || null);
            renderCaption(content || null);
        } catch (err) {
            appendLog(`[history details] ${err}`, true);
            if (
                requestId === state.detailsRequestId &&
                state.entryId === entryId &&
                els.historyCaptionTrackSelect.value === mediaPath
            ) {
                els.historyCaptionStatus.textContent = 'Saved text could not be loaded.';
            }
        }
    };

    const openDetails = async (entryId, returnFocus, { ocr = false } = {}) => {
        tags.closeQuick();
        void tags.showDetails(null);
        state.entryId = entryId;
        state.returnFocus = returnFocus;
        state.details = null;
        state.transcript = undefined;
        state.captions.clear();
        renderTranscription();
        subtitlesView.reset();
        transcriptReader.reset();
        transcriptMedia.setEntry(null);
        ocrMedia.setEntry(null);
        subtitleMedia.setEntry(null);
        const requestId = ++state.detailsRequestId;
        els.historyDetailsContent.hidden = true;
        els.historyOverviewTab.click();
        ocrView.reset(entryId);
        socialView.reset(entryId);
        keywordsView.reset(entryId);
        aiView.reset(entryId);
        navigateToDetails(entryId);
        els.historyDetailsTitle.textContent = 'Video Details';
        els.historyDetailsBreadcrumbCurrent.textContent = 'Video Details';
        els.historyDetailsBreadcrumbCurrent.removeAttribute('title');
        els.historyDetailsAccountCrumb.hidden = true;
        els.historyDetailsBreadcrumbAccount.textContent = '';
        els.historyDetailsAccountBreadcrumbBtn.removeAttribute('title');
        els.historyDetailsSubtitle.textContent = '';
        els.historyDetailsThumbnail.hidden = true;
        els.historyDetailsThumbnail.removeAttribute('src');
        els.historyDetailsHeader.classList.add('pinefetch-no-media');
        els.historyDetailsStatus.textContent = 'Loading details...';
        els.historyDetailsTitle.focus({ preventScroll: true });

        try {
            const details = await api.getHistoryDetails({ id: entryId });
            if (requestId !== state.detailsRequestId || state.entryId !== entryId) return;
            if (!details) {
                els.historyDetailsStatus.textContent = 'This history entry is no longer available.';
                return;
            }

            state.details = details;
            const title = details.entry?.title || details.entry?.filename || 'History entry';
            els.historyDetailsTitle.textContent = title;
            els.historyDetailsBreadcrumbCurrent.textContent = title;
            els.historyDetailsBreadcrumbCurrent.title = title;
            els.historyDetailsSubtitle.textContent = [
                formatHistorySource(details.entry?.source || detectPlatform(details.entry?.url)),
            ]
                .filter(Boolean)
                .join(' · ');
            const account = details.entry?.uploader;
            const hasAccount = Boolean(account && (details.entry?.source || detectPlatform(details.entry?.url)));
            els.historyDetailsBreadcrumbAccount.textContent = account || '';
            els.historyDetailsAccountBreadcrumbBtn.title = account || '';
            els.historyDetailsAccountCrumb.hidden = !hasAccount;
            if (details.entry?.thumbnail) {
                els.historyDetailsThumbnail.src = details.entry.thumbnail;
                els.historyDetailsThumbnail.hidden = false;
                els.historyDetailsHeader.classList.remove('pinefetch-no-media');
            }
            els.historyDetailsOpenFileBtn.disabled = !details.output_file_available;
            els.historyDetailsOpenBrowserBtn.disabled = !details.entry?.url;
            els.historyDetailsStatus.textContent = '';
            els.historyCaptionsPanel.hidden = false;
            els.historyTranscriptStatus.textContent = '';
            els.historyTranscriptText.hidden = true;
            els.historyTranscriptCopyBtn.disabled = true;
            els.historyDetailsTabs.hidden = false;
            els.historyCaptionTitle.textContent = captionTitle();
            els.historyCaptionCopyBtn.setAttribute('aria-label', `Copy ${captionTitle().toLowerCase()}`);
            els.historyCaptionDescription.textContent =
                captionTitle() === 'Video description'
                    ? 'Original video description saved with this download.'
                    : 'Original post text saved with this download.';
            renderTranscript(null);
            renderCaption(null);
            renderOverview(details);
            renderTranscription();
            if (!details.transcript) void transcriptionJobs.load(entryId);
            void tags.showDetails(entryId);
            renderCaptionOptions(details.captions || []);
            subtitlesView.reset(entryId, details.subtitles || []);
            transcriptMedia.setEntry({ id: entryId });
            ocrMedia.setEntry({ id: entryId });
            subtitleMedia.setEntry({ id: entryId });
            if (details.captions?.length) void loadSelectedCaption();
            els.historyDetailsContent.hidden = false;
            if (ocr || jobs.get(entryId)?.status === 'running') els.historyOcrTab.click();
        } catch (err) {
            appendLog(`[history details] ${err}`, true);
            if (requestId === state.detailsRequestId) {
                els.historyDetailsStatus.textContent = 'History details could not be loaded.';
            }
        }
    };

    const bindEvents = () => {
        ocrView.bindEvents();
        socialView.bindEvents();
        keywordsView.bindEvents();
        aiView.bindEvents();
        const unsubscribeTranscription = transcriptionJobs.subscribe((id, record) => {
            if (!state.entryId || !state.details) return;
            if (id !== state.entryId) {
                const current = transcriptionJobs.get(state.entryId);
                if (
                    record?.progress &&
                    !transcriptionActive(record.progress) &&
                    /another.*transcri/i.test(current?.unavailable_reason || current?.error || '')
                )
                    void transcriptionJobs.load(state.entryId);
                return;
            }
            renderTranscription();
            if (record?.transcript_available && !state.details.transcript) void refreshSavedTranscript(id);
        });
        window.addEventListener('pagehide', unsubscribeTranscription, { once: true });
        els.historyTranscribeBtn.addEventListener('click', () => {
            if (state.entryId && !els.historyTranscribeBtn.disabled) void transcriptionJobs.start(state.entryId);
        });
        els.historyTranscriptionCancelBtn.addEventListener('click', () => {
            if (state.entryId) void transcriptionJobs.cancel(state.entryId);
        });
        els.historyTranscriptionRefreshBtn.addEventListener('click', () => {
            if (state.entryId) void transcriptionJobs.load(state.entryId);
        });
        subtitlesView.bindEvents();
        const cleanupTabs = initTabs(els.historyDetailsContent);
        window.addEventListener('pagehide', cleanupTabs, { once: true });

        const historyCardsRoot = els.historyList.closest('.pinefetch-split');
        historyCardsRoot.addEventListener('contextmenu', event => {
            const target = event.target instanceof Element ? event.target : null;
            const item = target?.closest('.pinefetch-history-item');
            if (!item || !historyCardsRoot.contains(item)) return;
            const interactive = target.closest('a, button, input, select, textarea, [contenteditable="true"]');
            if (interactive && !interactive.matches('.pinefetch-history-open-btn, .pinefetch-history-thumbnail-btn')) {
                hideContextMenu();
                return;
            }
            event.preventDefault();
            openContextMenu(
                item,
                event.clientX,
                event.clientY,
                interactive || item.querySelector('.pinefetch-history-open-btn')
            );
        });
        historyCardsRoot.addEventListener('keydown', event => {
            if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return;
            const target = event.target instanceof Element ? event.target : null;
            const opener = target?.closest('.pinefetch-history-open-btn, .pinefetch-history-thumbnail-btn');
            const item = opener?.closest('.pinefetch-history-item');
            if (!item || !historyCardsRoot.contains(item)) return;
            event.preventDefault();
            const rect = opener.getBoundingClientRect();
            openContextMenu(item, rect.left, rect.bottom, opener);
        });
        els.historyContextMenu.addEventListener('contextmenu', event => event.preventDefault());
        els.historyShowMoreDataBtn.addEventListener('click', () => {
            const entryId = state.contextEntryId;
            const returnFocus = state.contextReturnFocus;
            hideContextMenu();
            if (entryId) void openDetails(entryId, returnFocus);
        });
        els.historyOpenInBrowserBtn.addEventListener('click', async () => {
            const url = state.contextEntryUrl;
            hideContextMenu({ restoreFocus: true });
            if (!url || !api.available) return;
            try {
                await api.openExternalUrl({ url });
            } catch (err) {
                appendLog(`[open] ${err}`, true);
            }
        });
        els.historyShowCreatorVideosBtn.addEventListener('click', () => {
            const creator = state.contextCreator;
            hideContextMenu();
            if (creator?.uploader && creator.source) showSavedVideosByCreator(creator);
        });
        els.historyManageTagsBtn.addEventListener('click', () => {
            const id = state.contextEntryId;
            const opener = state.contextReturnFocus;
            hideContextMenu();
            if (id) void tags.manage(id, opener);
        });
        els.historyOpenOcrBtn.addEventListener('click', () => {
            const entryId = state.contextEntryId;
            const returnFocus = state.contextReturnFocus;
            hideContextMenu();
            if (entryId) void openDetails(entryId, returnFocus, { ocr: true });
        });
        document.addEventListener('pointerdown', event => {
            const target = event.target instanceof Element ? event.target : null;
            if (els.historyContextMenu.hidden || target?.closest('#historyContextMenu')) return;
            hideContextMenu();
        });
        document.addEventListener('contextmenu', event => {
            const target = event.target instanceof Element ? event.target : null;
            if (target?.closest('#historyContextMenu')) {
                event.preventDefault();
                return;
            }
            if (!target?.closest('.pinefetch-history-item')) hideContextMenu();
        });
        document.addEventListener('keydown', event => {
            if (event.key === 'Tab' && !els.historyContextMenu.hidden) {
                hideContextMenu({ restoreFocus: true });
                return;
            }
            if (event.key === 'Escape' && !els.historyContextMenu.hidden) {
                event.preventDefault();
                hideContextMenu({ restoreFocus: true });
                return;
            }
            if (els.historyContextMenu.hidden || !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
            const items = Array.from(els.historyContextMenu.querySelectorAll('button')).filter(
                button => !button.disabled && !button.hidden
            );
            const index = items.indexOf(document.activeElement);
            let next;
            if (event.key === 'ArrowDown') next = items[(index + 1) % items.length];
            if (event.key === 'ArrowUp') next = items[(index - 1 + items.length) % items.length];
            if (event.key === 'Home') next = items[0];
            if (event.key === 'End') next = items.at(-1);
            event.preventDefault();
            next?.focus({ preventScroll: true });
        });
        window.addEventListener('resize', () => hideContextMenu());
        window.addEventListener('blur', () => hideContextMenu());
        historyCardsRoot.addEventListener('scroll', () => hideContextMenu(), { capture: true, passive: true });

        els.historyDetailsAccountBreadcrumbBtn.addEventListener('click', () => {
            if (state.details?.entry?.id) openAccount(state.details.entry.id, els.historyDetailsAccountBreadcrumbBtn);
        });
        els.historyDetailsHistoryBtn.addEventListener('click', () => navigateToHistory(state.returnFocus));
        els.historyDetailsThumbnail.addEventListener('error', () => {
            els.historyDetailsThumbnail.hidden = true;
            els.historyDetailsHeader.classList.add('pinefetch-no-media');
        });
        els.historyDetailsOpenFileBtn.addEventListener('click', async () => {
            const path = state.details?.entry?.output_path;
            if (!path || !api.available) return;
            try {
                if (!(await api.openFilePath({ path })))
                    toast('The saved file is no longer available.', { type: 'error' });
            } catch (error) {
                appendLog(`[open] ${error}`, true);
            }
        });
        els.historyDetailsOpenBrowserBtn.addEventListener('click', async () => {
            const url = state.details?.entry?.url;
            if (!url || !api.available) return;
            try {
                await api.openExternalUrl({ url });
            } catch (error) {
                appendLog(`[open] ${error}`, true);
            }
        });
        els.historyDetailsContent.addEventListener('oj:change', event => {
            if (event.target !== els.historyDetailsContent) return;
            tags.closeQuick();
            transcriptMedia.pause();
            ocrMedia.pause();
            subtitleMedia.pause();
            if (event.detail.tab === els.historyOverviewTab) void loadSelectedCaption();
            if (event.detail.tab === els.historyTranscriptTab) {
                void loadTranscript();
                void transcriptMedia.activate();
                renderTranscription();
                if (!state.details?.transcript && state.entryId) void transcriptionJobs.load(state.entryId);
            } else transcriptReader.deactivate();
            if (event.detail.tab === els.historySubtitlesTab) {
                void subtitlesView.load();
                void subtitleMedia.activate();
            } else subtitlesView.deactivate();
            if (event.detail.tab === els.historyOcrTab) {
                void ocrView.load();
                void ocrMedia.activate();
            } else ocrView.deactivate();
            if (event.detail.tab === els.historySocialTab) void socialView.load();
            if (event.detail.tab === els.historyKeywordsTab) void keywordsView.load();
            else keywordsView.deactivate();
            if (event.detail.tab === els.historyAiTab) void aiView.load();
        });
        els.historyCaptionTrackSelect.addEventListener('change', () => void loadSelectedCaption());
        els.historyCaptionCopyBtn.addEventListener('click', () => {
            const requestId = state.detailsRequestId;
            const entryId = state.entryId;
            const mediaPath = els.historyCaptionTrackSelect.value;
            const content = state.captions.get(mediaPath);
            const label = captionTitle();
            void copyAllText({
                getText: () => content?.text,
                isCurrent: () =>
                    requestId === state.detailsRequestId &&
                    entryId === state.entryId &&
                    mediaPath === els.historyCaptionTrackSelect.value &&
                    !els.historyDetailsView.hidden &&
                    els.historyOverviewTab.getAttribute('aria-selected') === 'true',
                status: els.historyCaptionStatus,
                appendLog,
                successMessage: `${label} copied.`,
                errorMessage: `${label} could not be copied.`,
            });
        });
    };

    return Object.freeze({
        bindEvents,
        hideContextMenu,
        open: openDetails,
        activate: () => {
            if (!els.historyDetailsView.hidden) void ocrView.load();
            else ocrView.deactivate();
        },
        leave: () => {
            tags.closeQuick();
            void tags.showDetails(null);
            state.entryId = null;
            state.detailsRequestId += 1;
            subtitlesView.deactivate();
            transcriptReader.deactivate();
            transcriptMedia.setEntry(null);
            ocrMedia.setEntry(null);
            subtitleMedia.setEntry(null);
            ocrView.deactivate();
            socialView.reset(null);
            keywordsView.reset(null);
            aiView.deactivate();
        },
    });
};
