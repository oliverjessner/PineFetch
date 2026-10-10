import { formatHistorySource, formatLocalDateTime, formatUploadDate } from './formatters.js';
import { closeDialog, initTabs, openDialog, toast } from './vendor/oj/index.js';

export const createHistoryDetailsView = ({
    els,
    api,
    appendLog,
    formatFileSize,
    formatDuration,
    detectPlatform,
    showSavedVideosByCreator,
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
    };

    const formatDateTime = timestamp => {
        const date = new Date(Number(timestamp));
        if (!Number.isFinite(Number(timestamp)) || Number.isNaN(date.getTime())) return null;
        return formatLocalDateTime(date);
    };

    const basename = path => `${path || ''}`.split(/[\\/]/).filter(Boolean).pop() || '';

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

    const addOverviewRow = (label, value, title = null, className = null) => {
        if (value === null || value === undefined || value === '') return;
        const term = document.createElement('dt');
        term.textContent = label;
        const description = document.createElement('dd');
        description.textContent = `${value}`;
        if (title) description.title = title;
        if (className) description.className = className;
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
            ? [
                  transcript.language?.toUpperCase(),
                  transcript.transcription_type,
                  transcript.file_available ? null : 'file missing',
              ]
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
        addOverviewRow('Creator', entry.uploader);
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
        addOverviewRow('Filename', entry.filename, null, 'oj-mono');
        addOverviewRow('File path', entry.output_path, entry.output_path, 'oj-path');
        addOverviewRow(
            'File status',
            entry.output_path ? (details.output_file_available ? 'Available' : 'Missing') : null
        );
        addOverviewRow('Extension', details.file_extension?.toUpperCase());
        addOverviewRow(
            'File size',
            entry.file_size_bytes !== null && entry.file_size_bytes !== undefined && Number(entry.file_size_bytes) >= 0
                ? formatFileSize(entry.file_size_bytes)
                : null
        );
        addOverviewRow('SHA-256', entry.sha256, null, 'oj-mono');
        addOverviewRow('Thumbnail', entry.thumbnail ? 'Available' : null);
        addOverviewRow('Transcript', transcriptLabel);
        addOverviewRow('Captions', captionCount ? `${captionCount} ${captionCount === 1 ? 'track' : 'tracks'}` : null);
        addOverviewRow('PineFetch version', entry.pinefetch_version);
    };

    const renderTranscript = content => {
        els.historyTranscriptStatus.textContent = '';
        els.historyTranscriptMeta.textContent = '';
        els.historyTranscriptNotice.hidden = true;
        els.historyTranscriptText.hidden = true;
        els.historyTranscriptCopyBtn.disabled = true;
        if (!content) {
            els.historyTranscriptStatus.textContent = 'Transcript is no longer available.';
            return;
        }

        els.historyTranscriptMeta.textContent = [content.language?.toUpperCase(), content.transcription_type]
            .filter(Boolean)
            .join(' · ');
        if (!content.file_available) {
            els.historyTranscriptNotice.textContent =
                'Transcript file is no longer available. Stored transcript text is shown below.';
            els.historyTranscriptNotice.hidden = false;
        }
        if (!content.text) {
            els.historyTranscriptStatus.textContent = 'The stored transcript is empty.';
            return;
        }
        els.historyTranscriptText.textContent = content.text;
        els.historyTranscriptText.hidden = false;
        els.historyTranscriptCopyBtn.disabled = false;
    };

    const loadTranscript = async () => {
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
                !els.historyDetailsDialog.open ||
                state.details?.entry?.id !== entryId
            )
                return;
            state.transcript = content || null;
            renderTranscript(state.transcript);
        } catch (err) {
            appendLog(`[history details] ${err}`, true);
            if (requestId === state.detailsRequestId && els.historyDetailsDialog.open) {
                els.historyTranscriptStatus.textContent = 'Transcript could not be loaded.';
            }
        }
    };

    const captionLabel = (caption, index) =>
        basename(caption.media_path) || basename(caption.caption_path) || `Caption ${index + 1}`;

    const renderCaptionOptions = captions => {
        const fragment = document.createDocumentFragment();
        captions.forEach((caption, index) => {
            const option = document.createElement('option');
            option.value = caption.media_path;
            option.textContent = captionLabel(caption, index);
            fragment.appendChild(option);
        });
        els.historyCaptionTrackSelect.replaceChildren(fragment);
        els.historyCaptionTrackField.hidden = captions.length <= 1;
    };

    const renderCaption = content => {
        els.historyCaptionStatus.textContent = '';
        els.historyCaptionMeta.textContent = '';
        els.historyCaptionNotice.hidden = true;
        els.historyCaptionText.hidden = true;
        els.historyCaptionCopyBtn.disabled = true;
        if (!content) {
            els.historyCaptionStatus.textContent = 'Captions are no longer available.';
            return;
        }

        const metadata = [
            content.format?.toUpperCase(),
            basename(content.caption_path),
            content.sha256 ? `SHA-256: ${content.sha256}` : null,
        ].filter(Boolean);
        els.historyCaptionMeta.textContent = metadata.join(' · ');
        if (!content.file_available) {
            els.historyCaptionNotice.textContent =
                'Caption file is no longer available. Stored caption text is shown below.';
            els.historyCaptionNotice.hidden = false;
        }
        if (!content.text) {
            els.historyCaptionStatus.textContent = 'The stored caption is empty.';
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
        els.historyCaptionStatus.textContent = 'Loading captions...';
        els.historyCaptionText.hidden = true;
        els.historyCaptionCopyBtn.disabled = true;
        try {
            const content = await api.getHistoryCaption({ id: entryId, mediaPath });
            if (
                requestId !== state.detailsRequestId ||
                !els.historyDetailsDialog.open ||
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
                els.historyDetailsDialog.open &&
                els.historyCaptionTrackSelect.value === mediaPath
            ) {
                els.historyCaptionStatus.textContent = 'Captions could not be loaded.';
            }
        }
    };

    const openDetails = async (entryId, returnFocus) => {
        state.details = null;
        state.transcript = undefined;
        state.captions.clear();
        const requestId = ++state.detailsRequestId;
        els.historyDetailsSubtitle.textContent = '';
        els.historyDetailsStatus.textContent = 'Loading details...';
        els.historyDetailsContent.hidden = true;
        els.historyOverviewTab.click();
        openDialog(els.historyDetailsDialog, { trigger: returnFocus });

        try {
            const details = await api.getHistoryDetails({ id: entryId });
            if (requestId !== state.detailsRequestId || !els.historyDetailsDialog.open) return;
            if (!details) {
                els.historyDetailsStatus.textContent = 'This history entry is no longer available.';
                return;
            }

            state.details = details;
            const title = details.entry?.title || details.entry?.filename || 'History entry';
            els.historyDetailsSubtitle.textContent = title;
            els.historyDetailsStatus.textContent = '';
            els.historyTranscriptTab.hidden = !details.transcript;
            els.historyTranscriptTab.disabled = !details.transcript;
            els.historyCaptionsTab.hidden = !details.captions?.length;
            els.historyCaptionsTab.disabled = !details.captions?.length;
            const visibleTabCount = 1 + Number(Boolean(details.transcript)) + Number(Boolean(details.captions?.length));
            els.historyDetailsTabs.hidden = visibleTabCount === 1;
            renderOverview(details);
            renderCaptionOptions(details.captions || []);
            els.historyDetailsContent.hidden = false;
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
            toast(`${label} copied.`, { type: 'success', root: els.historyDetailsDialog });
        } catch (err) {
            appendLog(`[copy] ${err}`, true);
            statusElement.textContent = `${label} could not be copied.`;
        }
    };

    const bindEvents = () => {
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

        els.historyDetailsDialog.addEventListener('oj:close', event => {
            if (event.target !== els.historyDetailsDialog) return;
            state.detailsRequestId += 1;
        });
        els.historyDetailsDialog.addEventListener('click', event => {
            if (event.target === els.historyDetailsDialog) closeDialog(els.historyDetailsDialog, 'close');
        });
        els.historyDetailsContent.addEventListener('oj:change', event => {
            if (event.target !== els.historyDetailsContent) return;
            if (event.detail.tab === els.historyTranscriptTab) void loadTranscript();
            if (event.detail.tab === els.historyCaptionsTab) void loadSelectedCaption();
        });
        els.historyCaptionTrackSelect.addEventListener('change', () => void loadSelectedCaption());
        els.historyTranscriptCopyBtn.addEventListener(
            'click',
            () => void copyContent(state.transcript, els.historyTranscriptStatus, 'Transcript')
        );
        els.historyCaptionCopyBtn.addEventListener('click', () => {
            const content = state.captions.get(els.historyCaptionTrackSelect.value);
            void copyContent(content, els.historyCaptionStatus, 'Caption');
        });
    };

    return Object.freeze({ bindEvents, hideContextMenu });
};
