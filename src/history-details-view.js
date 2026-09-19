export const createHistoryDetailsView = ({ els, invoke, appendLog, formatFileSize, formatDuration, detectPlatform }) => {
    const state = {
        contextEntryId: null,
        contextReturnFocus: null,
        dialogReturnFocus: null,
        details: null,
        detailsRequestId: 0,
        transcript: undefined,
        captions: new Map(),
    };

    const formatSource = source => {
        const name = `${source || 'unknown'}`.trim().toLowerCase();
        const known = { youtube: 'YouTube', tiktok: 'TikTok', instagram: 'Instagram', facebook: 'Facebook', twitch: 'Twitch', linkedin: 'LinkedIn', reddit: 'Reddit', x: 'X' };
        return known[name] || (name ? name.charAt(0).toUpperCase() + name.slice(1) : 'Unknown');
    };

    const formatDateTime = timestamp => {
        const date = new Date(Number(timestamp));
        if (!Number.isFinite(Number(timestamp)) || Number.isNaN(date.getTime())) return null;
        return date.toLocaleString([], {
            year: 'numeric',
            month: 'short',
            day: 'numeric',
            hour: '2-digit',
            minute: '2-digit',
        });
    };

    const formatUploadDate = value => {
        const raw = `${value || ''}`.trim();
        if (!raw) return null;
        if (/^\d{8}$/.test(raw)) return `${raw.slice(0, 4)}-${raw.slice(4, 6)}-${raw.slice(6, 8)}`;
        return raw;
    };

    const basename = path => `${path || ''}`.split(/[\\/]/).filter(Boolean).pop() || '';

    const hideContextMenu = ({ restoreFocus = false } = {}) => {
        const returnFocus = state.contextReturnFocus;
        state.contextEntryId = null;
        state.contextReturnFocus = null;
        els.historyContextMenu.hidden = true;
        els.historyContextMenu.style.left = '';
        els.historyContextMenu.style.top = '';
        if (restoreFocus && returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
    };

    const openContextMenu = (entryId, x, y, returnFocus) => {
        state.contextEntryId = entryId;
        state.contextReturnFocus = returnFocus;
        els.historyContextMenu.hidden = false;

        requestAnimationFrame(() => {
            if (els.historyContextMenu.hidden || state.contextEntryId !== entryId) return;
            const margin = 12;
            const left = Math.max(margin, Math.min(x, window.innerWidth - els.historyContextMenu.offsetWidth - margin));
            const top = Math.max(margin, Math.min(y, window.innerHeight - els.historyContextMenu.offsetHeight - margin));
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
        const uploaded = Number(entry.timestamp) > 0
            ? formatDateTime(Number(entry.timestamp) * 1000)
            : formatUploadDate(entry.upload_date);
        const downloaded = formatDateTime(entry.completed_at || entry.created_at);
        const transcript = details.transcript;
        const transcriptLabel = transcript
            ? [transcript.language?.toUpperCase(), transcript.transcription_type, transcript.file_available ? null : 'file missing'].filter(Boolean).join(' · ')
            : null;
        const captionCount = Array.isArray(details.captions) ? details.captions.length : 0;

        els.historyOverviewList.replaceChildren();
        addOverviewRow('Source', source ? formatSource(source) : null);
        if (platform && platform.toLowerCase() !== `${source || ''}`.toLowerCase()) addOverviewRow('Platform', platform);
        addOverviewRow('Original URL', entry.url, entry.url);
        addOverviewRow('Title', entry.title);
        addOverviewRow('Creator', entry.uploader);
        addOverviewRow('Downloaded', downloaded);
        addOverviewRow('Uploaded', uploaded);
        addOverviewRow(
            'Duration',
            entry.duration_seconds !== null && entry.duration_seconds !== undefined && Number(entry.duration_seconds) >= 0
                ? formatDuration(Number(entry.duration_seconds))
                : null,
        );
        addOverviewRow('Media type', entry.medium);
        addOverviewRow('Filename', entry.filename);
        addOverviewRow('File path', entry.output_path, entry.output_path);
        addOverviewRow('File status', entry.output_path ? (details.output_file_available ? 'Available' : 'Missing') : null);
        addOverviewRow('Extension', details.file_extension?.toUpperCase());
        addOverviewRow(
            'File size',
            entry.file_size_bytes !== null && entry.file_size_bytes !== undefined && Number(entry.file_size_bytes) >= 0
                ? formatFileSize(entry.file_size_bytes)
                : null,
        );
        addOverviewRow('SHA-256', entry.sha256);
        addOverviewRow('Thumbnail', entry.thumbnail ? 'Available' : null);
        addOverviewRow('Transcript', transcriptLabel);
        addOverviewRow('Captions', captionCount ? `${captionCount} ${captionCount === 1 ? 'track' : 'tracks'}` : null);
        addOverviewRow('PineFetch version', entry.pinefetch_version);
    };

    const updateTabs = activeTab => {
        const tabs = [
            [els.historyOverviewTab, els.historyOverviewPanel, 'overview'],
            [els.historyTranscriptTab, els.historyTranscriptPanel, 'transcript'],
            [els.historyCaptionsTab, els.historyCaptionsPanel, 'captions'],
        ];
        for (const [tab, panel, name] of tabs) {
            const active = name === activeTab;
            tab.setAttribute('aria-selected', `${active}`);
            tab.tabIndex = active ? 0 : -1;
            panel.hidden = !active;
        }
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
            els.historyTranscriptNotice.textContent = 'Transcript file is no longer available. Stored transcript text is shown below.';
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
        els.historyTranscriptStatus.textContent = 'Loading transcript...';
        els.historyTranscriptText.hidden = true;
        els.historyTranscriptCopyBtn.disabled = true;
        try {
            const content = await invoke('get_history_transcript', { id: entryId });
            if (!els.historyDetailsDialog.open || state.details?.entry?.id !== entryId) return;
            state.transcript = content || null;
            renderTranscript(state.transcript);
        } catch (err) {
            appendLog(`[history details] ${err}`, true);
            if (state.details?.entry?.id === entryId) {
                els.historyTranscriptStatus.textContent = 'Transcript could not be loaded.';
            }
        }
    };

    const captionLabel = (caption, index) => basename(caption.media_path) || basename(caption.caption_path) || `Caption ${index + 1}`;

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
            els.historyCaptionNotice.textContent = 'Caption file is no longer available. Stored caption text is shown below.';
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
        if (state.captions.has(mediaPath)) {
            renderCaption(state.captions.get(mediaPath));
            return;
        }
        els.historyCaptionStatus.textContent = 'Loading captions...';
        els.historyCaptionText.hidden = true;
        els.historyCaptionCopyBtn.disabled = true;
        try {
            const content = await invoke('get_history_caption', { id: entryId, mediaPath });
            if (!els.historyDetailsDialog.open || state.details?.entry?.id !== entryId || els.historyCaptionTrackSelect.value !== mediaPath) return;
            state.captions.set(mediaPath, content || null);
            renderCaption(content || null);
        } catch (err) {
            appendLog(`[history details] ${err}`, true);
            if (state.details?.entry?.id === entryId) {
                els.historyCaptionStatus.textContent = 'Captions could not be loaded.';
            }
        }
    };

    const activateTab = tabName => {
        if (tabName === 'transcript' && els.historyTranscriptTab.hidden) return;
        if (tabName === 'captions' && els.historyCaptionsTab.hidden) return;
        updateTabs(tabName);
        if (tabName === 'transcript') void loadTranscript();
        if (tabName === 'captions') void loadSelectedCaption();
    };

    const closeDialog = () => {
        if (!els.historyDetailsDialog.open) return;
        const returnFocus = state.dialogReturnFocus;
        state.detailsRequestId += 1;
        els.historyDetailsDialog.close();
        state.dialogReturnFocus = null;
        if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
    };

    const openDetails = async (entryId, returnFocus) => {
        state.dialogReturnFocus = returnFocus;
        state.details = null;
        state.transcript = undefined;
        state.captions.clear();
        const requestId = ++state.detailsRequestId;
        els.historyDetailsSubtitle.textContent = '';
        els.historyDetailsStatus.textContent = 'Loading details...';
        els.historyDetailsContent.hidden = true;
        if (!els.historyDetailsDialog.open) els.historyDetailsDialog.showModal();
        els.historyDetailsCloseBtn.focus({ preventScroll: true });

        try {
            const details = await invoke('get_history_details', { id: entryId });
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
            els.historyCaptionsTab.hidden = !details.captions?.length;
            const visibleTabCount = 1 + Number(Boolean(details.transcript)) + Number(Boolean(details.captions?.length));
            els.historyDetailsTabs.hidden = visibleTabCount === 1;
            renderOverview(details);
            renderCaptionOptions(details.captions || []);
            els.historyDetailsContent.hidden = false;
            activateTab('overview');
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
        } catch (err) {
            appendLog(`[copy] ${err}`, true);
            statusElement.textContent = `${label} could not be copied.`;
        }
    };

    const bindEvents = () => {
        els.historyList.addEventListener('contextmenu', event => {
            const target = event.target instanceof Element ? event.target : null;
            const item = target?.closest('.pf-history-item');
            if (!item || !els.historyList.contains(item)) return;
            const interactive = target.closest('a, button, input, select, textarea, [contenteditable="true"]');
            if (interactive && !interactive.classList.contains('pf-history-open-btn')) {
                hideContextMenu();
                return;
            }
            event.preventDefault();
            openContextMenu(
                item.dataset.historyId,
                event.clientX,
                event.clientY,
                item.querySelector('.pf-history-open-btn'),
            );
        });
        els.historyContextMenu.addEventListener('contextmenu', event => event.preventDefault());
        els.historyShowMoreDataBtn.addEventListener('click', () => {
            const entryId = state.contextEntryId;
            const returnFocus = state.contextReturnFocus;
            hideContextMenu();
            if (entryId) void openDetails(entryId, returnFocus);
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
            if (!target?.closest('.pf-history-item')) hideContextMenu();
        });
        document.addEventListener('keydown', event => {
            if (event.key === 'Escape' && !els.historyContextMenu.hidden) {
                event.preventDefault();
                hideContextMenu({ restoreFocus: true });
                return;
            }
            if (els.historyContextMenu.hidden || !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
            event.preventDefault();
            els.historyShowMoreDataBtn.focus({ preventScroll: true });
        });
        window.addEventListener('resize', () => hideContextMenu());
        window.addEventListener('blur', () => hideContextMenu());
        els.historyList.addEventListener('scroll', () => hideContextMenu(), { passive: true });

        els.historyDetailsCloseBtn.addEventListener('click', closeDialog);
        els.historyDetailsDialog.addEventListener('cancel', event => {
            event.preventDefault();
            closeDialog();
        });
        els.historyDetailsDialog.addEventListener('click', event => {
            if (event.target === els.historyDetailsDialog) closeDialog();
        });
        els.historyDetailsTabs.addEventListener('click', event => {
            const tab = event.target.closest('[data-history-tab]');
            if (tab && !tab.hidden) activateTab(tab.dataset.historyTab);
        });
        els.historyDetailsTabs.addEventListener('keydown', event => {
            if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
            const tabs = Array.from(els.historyDetailsTabs.querySelectorAll('[role="tab"]:not([hidden])'));
            const currentIndex = tabs.indexOf(document.activeElement);
            if (currentIndex < 0) return;
            event.preventDefault();
            const nextIndex = event.key === 'Home'
                ? 0
                : event.key === 'End'
                    ? tabs.length - 1
                    : (currentIndex + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length;
            tabs[nextIndex].focus();
            activateTab(tabs[nextIndex].dataset.historyTab);
        });
        els.historyCaptionTrackSelect.addEventListener('change', () => void loadSelectedCaption());
        els.historyTranscriptCopyBtn.addEventListener('click', () => void copyContent(state.transcript, els.historyTranscriptStatus, 'Transcript'));
        els.historyCaptionCopyBtn.addEventListener('click', () => {
            const content = state.captions.get(els.historyCaptionTrackSelect.value);
            void copyContent(content, els.historyCaptionStatus, 'Caption');
        });
    };

    return Object.freeze({ bindEvents, hideContextMenu });
};
