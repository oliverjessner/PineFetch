import { formatHistorySource, formatLocalDateTime, formatUploadDate } from './formatters.js';
import { initTabs, toast } from './vendor/oj/index.js';
import { createHistoryOcrView } from './history-ocr-view.js';
import { createHistorySocialInsightsView } from './history-social-insights-view.js';
import { createHistoryKeywordsView } from './history-keywords-view.js';
import { createHistorySubtitlesView } from './history-subtitles-view.js';
import { createTextReader } from './history-text-view.js';
import { createHistoryMediaView } from './history-media-view.js';

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
    navigateToDetails,
    navigateToHistory,
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
    const transcriptMedia = createHistoryMediaView({
        root: els.historyTranscriptPreview,
        api,
        onTime: seconds => transcriptReader?.sync(seconds),
    });
    const ocrMedia = createHistoryMediaView({
        root: els.historyOcrPreview,
        api,
        onTime: seconds => ocrView?.syncTime(seconds),
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
    const subtitlesView = createHistorySubtitlesView({ els, api, appendLog });
    const keywordsView = createHistoryKeywordsView({ els, api, appendLog });

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
            els.historyTranscriptStatus.textContent = 'No transcript is saved for this video.';
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
        state.entryId = entryId;
        state.returnFocus = returnFocus;
        state.details = null;
        state.transcript = undefined;
        state.captions.clear();
        subtitlesView.reset();
        transcriptReader.reset();
        transcriptMedia.setEntry(null);
        ocrMedia.setEntry(null);
        const requestId = ++state.detailsRequestId;
        els.historyDetailsContent.hidden = true;
        els.historyOverviewTab.click();
        ocrView.reset(entryId);
        socialView.reset(entryId);
        keywordsView.reset(entryId);
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
            renderCaptionOptions(details.captions || []);
            subtitlesView.reset(entryId, details.subtitles || []);
            transcriptMedia.setEntry({ id: entryId, url: details.entry?.url, thumbnail: details.entry?.thumbnail });
            ocrMedia.setEntry({ id: entryId, url: details.entry?.url, thumbnail: details.entry?.thumbnail });
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

    const copyContent = async (content, statusElement, label) => {
        if (!content?.text) return;
        try {
            await navigator.clipboard.writeText(content.text);
            statusElement.textContent = `${label} copied.`;
            toast(`${label} copied.`, { type: 'success' });
        } catch (err) {
            appendLog(`[copy] ${err}`, true);
            statusElement.textContent = `${label} could not be copied.`;
        }
    };

    const bindEvents = () => {
        ocrView.bindEvents();
        socialView.bindEvents();
        keywordsView.bindEvents();
        subtitlesView.bindEvents();
        const cleanupTabs = initTabs(els.historyDetailsContent);
        window.addEventListener('pagehide', cleanupTabs, { once: true });

        els.historyList.addEventListener('contextmenu', event => {
            const target = event.target instanceof Element ? event.target : null;
            const item = target?.closest('.pinefetch-history-item');
            if (!item || !els.historyList.contains(item)) return;
            const interactive = target.closest('a, button, input, select, textarea, [contenteditable="true"]');
            if (interactive && !interactive.classList.contains('pinefetch-history-open-btn')) {
                hideContextMenu();
                return;
            }
            event.preventDefault();
            openContextMenu(item, event.clientX, event.clientY, item.querySelector('.pinefetch-history-open-btn'));
        });
        els.historyList.addEventListener('keydown', event => {
            if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return;
            const target = event.target instanceof Element ? event.target : null;
            const opener = target?.closest('.pinefetch-history-open-btn');
            const item = opener?.closest('.pinefetch-history-item');
            if (!item || !els.historyList.contains(item)) return;
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
        els.historyList.addEventListener('scroll', () => hideContextMenu(), { passive: true });

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
            transcriptMedia.pause();
            ocrMedia.pause();
            if (event.detail.tab === els.historyOverviewTab) void loadSelectedCaption();
            if (event.detail.tab === els.historyTranscriptTab) {
                void loadTranscript();
                void transcriptMedia.activate();
            } else transcriptReader.deactivate();
            if (event.detail.tab === els.historySubtitlesTab) void subtitlesView.load();
            else subtitlesView.deactivate();
            if (event.detail.tab === els.historyOcrTab) {
                void ocrView.load();
                void ocrMedia.activate();
            } else ocrView.deactivate();
            if (event.detail.tab === els.historySocialTab) void socialView.load();
            if (event.detail.tab === els.historyKeywordsTab) void keywordsView.load();
        });
        els.historyCaptionTrackSelect.addEventListener('change', () => void loadSelectedCaption());
        els.historyCaptionCopyBtn.addEventListener('click', () => {
            const content = state.captions.get(els.historyCaptionTrackSelect.value);
            void copyContent(content, els.historyCaptionStatus, captionTitle());
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
            state.entryId = null;
            state.detailsRequestId += 1;
            subtitlesView.deactivate();
            transcriptReader.deactivate();
            transcriptMedia.setEntry(null);
            ocrMedia.setEntry(null);
            ocrView.deactivate();
            socialView.reset(null);
            keywordsView.reset(null);
        },
    });
};
