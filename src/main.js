import { initOJ, toast } from './vendor/oj/index.js';
import { createDropdownChoice } from './dropdown-choice.js';
import { formatDuration, formatFileSize } from './formatters.js';
import {
    createAppState,
    updateJobState,
    applyQueueStatus,
    applyDownloadState,
    applyDownloadProgress,
    isCancellable,
    isQueueBusy,
    isRemovable,
} from './app-state.js';
import { api } from './tauri-client.js';
import { createBrowserImportView } from './browser-import-view.js';
import { createSettingsView } from './settings-view.js';
import { createHistoryView } from './history-view.js';
import {
    detectPlatform,
    extractUrlStartTimestamp,
    isValidHttpUrl,
    normalizeTxtImportUrl,
    parseTxtImportLinks,
    resolveYouTubeThumbnail,
} from './url-utils.js';

const state = createAppState();
const els = Object.seal({
    magicImportTrigger: document.getElementById('magicImportTrigger'),
    magicImportEnabled: document.getElementById('magicImportEnabled'),
    cutAtTimestampEnabled: document.getElementById('cutAtTimestampEnabled'),
    urlInput: document.getElementById('urlInput'),
    loadInfoBtn: document.getElementById('loadInfoBtn'),
    startDownloadBtn: document.getElementById('startDownloadBtn'),
    importTxtBtn: document.getElementById('importTxtBtn'),
    txtImportStatus: document.getElementById('txtImportStatus'),
    pickDirBtn: document.getElementById('pickDirBtn'),
    settingsSaveStatus: document.getElementById('settingsSaveStatus'),
    notificationsEnabled: document.getElementById('notificationsEnabled'),
    saveCaptions: document.getElementById('saveCaptions'),
    saveThumbnails: document.getElementById('saveThumbnails'),
    openFolderBtn: document.getElementById('openFolderBtn'),
    outputDir: document.getElementById('outputDir'),
    ytDlpPath: document.getElementById('ytDlpPath'),
    fasterWhisperModelDropdown: document.getElementById('fasterWhisperModelDropdown'),
    fasterWhisperModelValue: document.getElementById('fasterWhisperModelValue'),
    fasterWhisperModelMenu: document.getElementById('fasterWhisperModelMenu'),
    downloadVideoWithTranscript: document.getElementById('downloadVideoWithTranscript'),
    ytDlpInstalledVersion: document.getElementById('ytDlpInstalledVersion'),
    ytDlpLatestVersion: document.getElementById('ytDlpLatestVersion'),
    linkDumpServerStatusBadge: document.getElementById('linkDumpServerStatusBadge'),
    linkDumpServerHint: document.getElementById('linkDumpServerHint'),
    linkDumpServerUrl: document.getElementById('linkDumpServerUrl'),
    linkDumpPort: document.getElementById('linkDumpPort'),
    linkDumpServerEnabled: document.getElementById('linkDumpServerEnabled'),
    saveLinkDumpServerBtn: document.getElementById('saveLinkDumpServerBtn'),
    restartLinkDumpServerBtn: document.getElementById('restartLinkDumpServerBtn'),
    linkDumpServerStatusText: document.getElementById('linkDumpServerStatusText'),
    linkDumpSecretName: document.getElementById('linkDumpSecretName'),
    generateLinkDumpSecretBtn: document.getElementById('generateLinkDumpSecretBtn'),
    generatedLinkDumpSecretPanel: document.getElementById('generatedLinkDumpSecretPanel'),
    generatedLinkDumpSecret: document.getElementById('generatedLinkDumpSecret'),
    copyGeneratedLinkDumpSecretBtn: document.getElementById('copyGeneratedLinkDumpSecretBtn'),
    linkDumpSecretList: document.getElementById('linkDumpSecretList'),
    linkDumpSecretHint: document.getElementById('linkDumpSecretHint'),
    linkDumpSecretStatus: document.getElementById('linkDumpSecretStatus'),
    presetDropdown: document.getElementById('presetDropdown'),
    presetTrigger: document.getElementById('presetTrigger'),
    presetValue: document.getElementById('presetValue'),
    presetMenu: document.getElementById('presetMenu'),
    infoCard: document.getElementById('infoCard'),
    infoTitle: document.getElementById('infoTitle'),
    infoUploader: document.getElementById('infoUploader'),
    infoDuration: document.getElementById('infoDuration'),
    infoThumb: document.getElementById('infoThumb'),
    queueList: document.getElementById('queueList'),
    queueBadge: document.getElementById('queueBadge'),
    queueCollapseBtn: document.getElementById('queueCollapseBtn'),
    queueAutoStartBtn: document.getElementById('queueAutoStartBtn'),
    startQueueBtn: document.getElementById('startQueueBtn'),
    pauseQueueBtn: document.getElementById('pauseQueueBtn'),
    queueModeHint: document.getElementById('queueModeHint'),
    queueEmptyHint: document.getElementById('queueEmptyHint'),
    clearQueueBtn: document.getElementById('clearQueueBtn'),
    captionHint: document.getElementById('captionHint'),
    infoBadge: document.getElementById('infoBadge'),
    logBody: document.getElementById('logBody'),
    copyLogsBtn: document.getElementById('copyLogsBtn'),
    clearLogsBtn: document.getElementById('clearLogsBtn'),
    leftPanelTitle: document.getElementById('leftPanelTitle'),
    rightPanelTitle: document.getElementById('rightPanelTitle'),
    downloadView: document.getElementById('downloadView'),
    historyView: document.getElementById('historyView'),
    historyList: document.getElementById('historyList'),
    historySearchInput: document.getElementById('historySearchInput'),
    historySearchFieldDropdown: document.getElementById('historySearchFieldDropdown'),
    historySearchFieldValue: document.getElementById('historySearchFieldValue'),
    historySearchFieldMenu: document.getElementById('historySearchFieldMenu'),
    historySourceDropdown: document.getElementById('historySourceDropdown'),
    historySourceValue: document.getElementById('historySourceValue'),
    historySourceMenu: document.getElementById('historySourceMenu'),
    historyHint: document.getElementById('historyHint'),
    settingsView: document.getElementById('settingsView'),
    linkDumpView: document.getElementById('linkDumpView'),
    queueProgressView: document.getElementById('queueProgressView'),
    historySummaryView: document.getElementById('historySummaryView'),
    historyVideoCount: document.getElementById('historyVideoCount'),
    historyTotalSize: document.getElementById('historyTotalSize'),
    historyTotalDuration: document.getElementById('historyTotalDuration'),
    historySourcesList: document.getElementById('historySourcesList'),
    historySourcesEmpty: document.getElementById('historySourcesEmpty'),
    linkDumpSideView: document.getElementById('linkDumpSideView'),
    viewDownloadBtn: document.getElementById('viewDownloadBtn'),
    viewHistoryBtn: document.getElementById('viewHistoryBtn'),
    viewLinkDumpBtn: document.getElementById('viewLinkDumpBtn'),
    viewSettingsBtn: document.getElementById('viewSettingsBtn'),
    linkDumpExtensionRepoLink: document.getElementById('linkDumpExtensionRepoLink'),
    loadMoreHistoryBtn: document.getElementById('loadMoreHistoryBtn'),
    clearHistoryBtn: document.getElementById('clearHistoryBtn'),
    historyActionStatus: document.getElementById('historyActionStatus'),
    historyContextMenu: document.getElementById('historyContextMenu'),
    historyShowMoreDataBtn: document.getElementById('historyShowMoreDataBtn'),
    historyDetailsDialog: document.getElementById('historyDetailsDialog'),
    historyDetailsTitle: document.getElementById('historyDetailsTitle'),
    historyDetailsSubtitle: document.getElementById('historyDetailsSubtitle'),
    historyDetailsCloseBtn: document.getElementById('historyDetailsCloseBtn'),
    historyDetailsStatus: document.getElementById('historyDetailsStatus'),
    historyDetailsContent: document.getElementById('historyDetailsContent'),
    historyDetailsTabs: document.getElementById('historyDetailsTabs'),
    historyOverviewTab: document.getElementById('historyOverviewTab'),
    historyTranscriptTab: document.getElementById('historyTranscriptTab'),
    historyCaptionsTab: document.getElementById('historyCaptionsTab'),
    historyOverviewPanel: document.getElementById('historyOverviewPanel'),
    historyTranscriptPanel: document.getElementById('historyTranscriptPanel'),
    historyCaptionsPanel: document.getElementById('historyCaptionsPanel'),
    historyOverviewList: document.getElementById('historyOverviewList'),
    historyTranscriptMeta: document.getElementById('historyTranscriptMeta'),
    historyTranscriptCopyBtn: document.getElementById('historyTranscriptCopyBtn'),
    historyTranscriptNotice: document.getElementById('historyTranscriptNotice'),
    historyTranscriptStatus: document.getElementById('historyTranscriptStatus'),
    historyTranscriptText: document.getElementById('historyTranscriptText'),
    historyCaptionTrackField: document.getElementById('historyCaptionTrackField'),
    historyCaptionTrackSelect: document.getElementById('historyCaptionTrackSelect'),
    historyCaptionCopyBtn: document.getElementById('historyCaptionCopyBtn'),
    historyCaptionMeta: document.getElementById('historyCaptionMeta'),
    historyCaptionNotice: document.getElementById('historyCaptionNotice'),
    historyCaptionStatus: document.getElementById('historyCaptionStatus'),
    historyCaptionText: document.getElementById('historyCaptionText'),
    queueContextMenu: document.getElementById('queueContextMenu'),
    queueContextDownloads: document.getElementById('queueContextDownloads'),
    queueContextCancelBtn: document.getElementById('queueContextCancelBtn'),
    queueContextRemoveBtn: document.getElementById('queueContextRemoveBtn'),
});
const presetLabels = Object.freeze([
    {
        key: 'best',
        selectLabel: 'Best (bestvideo+bestaudio)',
        queueLabel: 'Best',
        menuLabel: 'Download Best',
    },
    {
        key: '1080',
        selectLabel: 'Max 1080p',
        queueLabel: 'Max 1080p',
        menuLabel: 'Download Max 1080p',
    },
    {
        key: 'audio_mp3',
        selectLabel: 'Audio only (mp3)',
        queueLabel: 'Audio only (mp3)',
        menuLabel: 'Download Audio only (mp3)',
    },
    {
        key: 'audio_opus',
        selectLabel: 'Audio only (opus)',
        queueLabel: 'Audio only (opus)',
        menuLabel: 'Download Audio only (opus)',
    },
    {
        key: 'text',
        selectLabel: 'Transcribe to text',
        queueLabel: 'Transcription',
        menuLabel: 'Transcribe to text',
    },
    {
        key: 'text_timestamps',
        selectLabel: 'Transcribe with timestamps',
        queueLabel: 'Transcription with timestamps',
        menuLabel: 'Transcribe with timestamps',
    },
]);
let presetOptions = [];
let presets = Object.freeze({});
const presetChoice = createDropdownChoice({ menu: els.presetMenu, value: els.presetValue });
const normalizePresetKey = key => (Object.hasOwn(presets, key) ? key : presetLabels[0].key);
const getSelectedPresetKey = () => normalizePresetKey(presetChoice.getValue());
const setSelectedPresetKey = key => {
    presetChoice.setValue(normalizePresetKey(key));
    updateDownloadOptionHints();
};
const findPresetForDownloadJob = job =>
    presetOptions.find(
        preset =>
            job?.format === preset.format &&
            Boolean(job?.extract_audio) === preset.extractAudio &&
            (job?.audio_format ?? null) === (preset.audioFormat ?? null) &&
            Boolean(job?.transcribe_text) === preset.transcribeText &&
            Boolean(job?.transcribe_timestamps) === preset.transcribeTimestamps &&
            (job?.filename_suffix ?? null) === (preset.filenameSuffix ?? null)
    ) || null;

const loadDownloadPresets = async () => {
    const definitions = await api.getDownloadPresets();
    if (!Array.isArray(definitions)) throw new Error('Backend did not return download formats.');
    const byKey = new Map(definitions.map(definition => [definition.key, definition]));
    presetOptions = presetLabels.map(label => {
        const definition = byKey.get(label.key);
        if (!definition || typeof definition.format !== 'string') {
            throw new Error(`Download format “${label.key}” is unavailable.`);
        }
        return Object.freeze({
            ...label,
            format: definition.format,
            extractAudio: Boolean(definition.extract_audio),
            audioFormat: definition.audio_format ?? null,
            transcribeText: Boolean(definition.transcribe_text),
            transcribeTimestamps: Boolean(definition.transcribe_timestamps),
            filenameSuffix: definition.filename_suffix ?? null,
        });
    });
    presets = Object.freeze(Object.fromEntries(presetOptions.map(preset => [preset.key, preset])));
    renderPresetOptions();
    renderQueueContextMenu();
    updateDownloadOptionHints();
};
const maxLogLines = 500;
let urlShakeTimer = null;
let magicImportInFlight = false;
let queueRenderFrame = null;
let queueRenderDirty = true;
const queueProgressDirtyIds = new Set();
const queueCardElements = new Map();
let logDomDirty = false;
let settingsLogRenderReady = false;
let viewActivationId = 0;
let clearQueueInFlight = false;

const formatCutStartLabel = seconds => {
    if (!Number.isFinite(Number(seconds)) || Number(seconds) <= 0) return null;
    return `from ${formatDuration(Number(seconds))}`;
};

const createIcon = (name, family = 'fa-solid') => {
    const icon = document.createElement('i');
    icon.className = `${family} fa-${name}`;
    icon.setAttribute('aria-hidden', 'true');
    return icon;
};

const getPlatformIconElement = platform => {
    const icons = {
        youtube: 'youtube',
        facebook: 'facebook',
        twitch: 'twitch',
        x: 'x-twitter',
        tiktok: 'tiktok',
        instagram: 'instagram',
        reddit: 'reddit-alien',
    };
    return icons[platform] ? createIcon(icons[platform], 'fa-brands') : null;
};

const appendTextSpans = (parent, values) => {
    values.forEach(value => {
        if (value === null || value === undefined || value === '') return;
        const span = document.createElement('span');
        span.textContent = value;
        parent.appendChild(span);
    });
};

const shakeUrlInput = () => {
    if (urlShakeTimer) {
        clearTimeout(urlShakeTimer);
        urlShakeTimer = null;
    }
    els.urlInput.removeAttribute('aria-invalid');
    els.urlInput.classList.remove('pinefetch-invalid-shake');
    void els.urlInput.offsetWidth;
    els.urlInput.setAttribute('aria-invalid', 'true');
    els.urlInput.classList.add('pinefetch-invalid-shake');
    els.urlInput.focus();
    urlShakeTimer = setTimeout(() => {
        els.urlInput.removeAttribute('aria-invalid');
        els.urlInput.classList.remove('pinefetch-invalid-shake');
        urlShakeTimer = null;
    }, 420);
};

const readClipboardText = async () => {
    if (api.available) {
        const text = await api.readClipboardText();
        if (typeof text === 'string') return text;
    }
    if (navigator.clipboard?.readText) {
        return navigator.clipboard.readText();
    }
    return api.readClipboardPlugin();
};

const tryMagicImport = async () => {
    if (magicImportInFlight) return;
    if (!settings.isMagicImportEnabled()) return;
    if (state.activeView !== 'download') return;
    if (els.urlInput.value) return;

    magicImportInFlight = true;
    try {
        const clipboardText = (await readClipboardText()).trim();
        if (!settings.isMagicImportEnabled() || state.activeView !== 'download' || els.urlInput.value) return;
        if (!isValidHttpUrl(clipboardText)) return;
        const platform = detectPlatform(clipboardText);
        if (!platform) return;
        const lastDownloadedUrl = `${settings.getConfig()?.last_download_url || ''}`.trim();
        if (lastDownloadedUrl && clipboardText === lastDownloadedUrl) return;

        els.urlInput.value = clipboardText;
        updateDownloadOptionHints();
        state.info = null;
        state.infoUrl = null;
        renderInfo();
        setInfoBadge('Loading...');
        els.urlInput.focus();
        void loadInfo();
    } catch {
        // Ignore clipboard read failures; magic import is best-effort.
    } finally {
        magicImportInFlight = false;
    }
};

const getQueuedTxtImportKeys = () => {
    const keys = new Set();
    state.jobs.forEach(job => {
        const normalized = normalizeTxtImportUrl(job.url);
        if (normalized) keys.add(normalized.key);
    });
    return keys;
};

const setInfoBadge = text => {
    els.infoBadge.textContent = text;
    els.infoBadge.classList.toggle('oj-badge-success', text === 'Ready');
    els.infoBadge.classList.toggle('oj-badge-info', text === 'Loading...');
    els.infoBadge.classList.toggle(
        'oj-badge-danger',
        ['Error', 'Formats unavailable', 'Unsupported link', 'Invalid URL'].includes(text)
    );
};

const setActiveView = view => {
    const isDownload = view === 'download';
    const isHistory = view === 'history';
    const isLinkDump = view === 'linkDump';
    const isSettings = view === 'settings';
    state.activeView = view;
    const split = els.settingsView.closest('.pinefetch-split');
    split.classList.toggle('pinefetch-settings-active', isSettings);
    split.classList.toggle('pinefetch-history-active', isHistory);
    settingsLogRenderReady = false;
    const activationId = ++viewActivationId;
    const runAfterViewPaint = callback => {
        requestAnimationFrame(() => {
            requestAnimationFrame(() => {
                if (state.activeView === view && viewActivationId === activationId) callback();
            });
        });
    };

    els.downloadView.hidden = !isDownload;
    els.historyView.hidden = !isHistory;
    els.settingsView.hidden = !isSettings;
    els.linkDumpView.hidden = !isLinkDump;
    els.queueProgressView.hidden = !isDownload || state.queueCollapsed;
    els.queueCollapseBtn.hidden = !isDownload;
    split.classList.toggle('pinefetch-queue-collapsed', isDownload && state.queueCollapsed);
    els.historySummaryView.hidden = !isHistory;
    els.linkDumpSideView.hidden = !isLinkDump;
    els.downloadView.classList.toggle('pinefetch-is-active', isDownload);
    els.historyView.classList.toggle('pinefetch-is-active', isHistory);
    els.settingsView.classList.toggle('pinefetch-is-active', isSettings);
    els.linkDumpView.classList.toggle('pinefetch-is-active', isLinkDump);
    els.queueProgressView.classList.toggle('pinefetch-is-active', isDownload);
    els.historySummaryView.classList.toggle('pinefetch-is-active', isHistory);
    els.linkDumpSideView.classList.toggle('pinefetch-is-active', isLinkDump);

    els.viewDownloadBtn.setAttribute('aria-current', isDownload ? 'page' : 'false');
    els.viewHistoryBtn.setAttribute('aria-current', isHistory ? 'page' : 'false');
    els.viewLinkDumpBtn.setAttribute('aria-current', isLinkDump ? 'page' : 'false');
    els.viewSettingsBtn.setAttribute('aria-current', isSettings ? 'page' : 'false');

    if (isDownload) {
        els.leftPanelTitle.textContent = 'Download';
        els.rightPanelTitle.textContent = 'Queue / Progress';
        els.queueBadge.style.display = 'inline-flex';
        els.infoBadge.style.display = 'inline-flex';
        runAfterViewPaint(flushQueueRender);
    } else if (isHistory) {
        els.leftPanelTitle.textContent = 'History';
        els.rightPanelTitle.textContent = 'Statistics';
        els.queueBadge.style.display = 'none';
        els.infoBadge.style.display = 'none';
        runAfterViewPaint(() => {
            void historyView.render();
        });
    } else if (isLinkDump) {
        els.leftPanelTitle.textContent = 'Browser Import';
        els.rightPanelTitle.textContent = 'Connections';
        els.queueBadge.style.display = 'none';
        els.infoBadge.style.display = 'none';
        if (!browserImport.hasOverview()) {
            runAfterViewPaint(() => {
                void syncLinkDumpOverview();
            });
        }
    } else {
        els.leftPanelTitle.textContent = 'Settings';
        els.rightPanelTitle.textContent = 'Options';
        els.queueBadge.style.display = 'none';
        els.infoBadge.style.display = 'none';
        runAfterViewPaint(() => {
            settingsLogRenderReady = true;
            renderLogs();
            if (settings.getConfig()) void settings.refreshYtDlpVersionsOnce();
        });
    }
};

const renderPresetOptions = () => {
    presetChoice.setOptions(presetOptions.map(preset => ({ value: preset.key, label: preset.selectLabel })));
    updateDownloadOptionHints();
};

const renderQueueContextMenu = () => {
    els.queueContextDownloads.replaceChildren();
    presetOptions.forEach(preset => {
        const button = document.createElement('button');
        button.className = 'oj-menu-item pinefetch-queue-context-menu-btn';
        button.type = 'button';
        button.setAttribute('role', 'menuitem');
        button.dataset.action = 'download';
        button.dataset.presetKey = preset.key;
        button.textContent = preset.menuLabel;
        els.queueContextDownloads.appendChild(button);
    });
};

const hideQueueContextMenu = ({ restoreFocus = false } = {}) => {
    const trigger = queueCardElements.get(state.contextMenuJobId)?.querySelector('.pinefetch-queue-more-btn');
    trigger?.setAttribute('aria-expanded', 'false');
    state.contextMenuJobId = null;
    els.queueContextMenu.hidden = true;
    els.queueContextMenu.style.left = '';
    els.queueContextMenu.style.top = '';
    if (restoreFocus && trigger?.isConnected) trigger.focus({ preventScroll: true });
};

const getContextMenuJob = () => {
    if (!state.contextMenuJobId) return null;
    return state.jobs.get(state.contextMenuJobId) || null;
};

const syncQueueContextMenuState = () => {
    const job = getContextMenuJob();
    const isCancelling = job?.state === 'cancelling';
    const canCancel = Boolean(job && isCancellable(job.state));
    const showCancel = Boolean(job && (canCancel || isCancelling));
    const canRemove = Boolean(job && isRemovable(job.state));

    els.queueContextCancelBtn.hidden = !showCancel;
    els.queueContextCancelBtn.disabled = !canCancel;
    els.queueContextCancelBtn.textContent = isCancelling ? 'Cancelling...' : 'Cancel download';
    els.queueContextRemoveBtn.hidden = !canRemove;
};

const openQueueContextMenu = (job, x, y) => {
    if (state.contextMenuJobId && state.contextMenuJobId !== job.id) hideQueueContextMenu();
    state.contextMenuJobId = job.id;
    syncQueueContextMenuState();
    els.queueContextMenu.hidden = false;
    queueCardElements.get(job.id)?.querySelector('.pinefetch-queue-more-btn')?.setAttribute('aria-expanded', 'true');

    requestAnimationFrame(() => {
        if (els.queueContextMenu.hidden || state.contextMenuJobId !== job.id) return;
        const margin = 12;
        const menuWidth = els.queueContextMenu.offsetWidth;
        const menuHeight = els.queueContextMenu.offsetHeight;
        const left = Math.max(margin, Math.min(x, window.innerWidth - menuWidth - margin));
        const top = Math.max(margin, Math.min(y, window.innerHeight - menuHeight - margin));

        els.queueContextMenu.style.left = `${left}px`;
        els.queueContextMenu.style.top = `${top}px`;
        els.queueContextMenu.querySelector('button:not([hidden]):not(:disabled)')?.focus();
    });
};

const renderInfo = () => {
    els.infoCard.hidden = !state.info;
    if (!state.info) {
        els.infoTitle.textContent = '-';
        els.infoUploader.textContent = '-';
        els.infoDuration.textContent = '-';
        els.infoThumb.style.backgroundImage = '';
        return;
    }
    const { title, uploader, duration, thumbnail, description } = state.info;

    // For Instagram, use description if title is missing or generic; otherwise use title
    // For YouTube and others, always use title
    let displayTitle = title;
    if (!displayTitle || displayTitle.trim() === '') {
        // Fallback to description only if title is missing
        displayTitle = description && description.trim() ? description : '-';
        // Truncate long descriptions for display
        if (displayTitle.length > 200) {
            displayTitle = displayTitle.substring(0, 200) + '...';
        }
    }

    els.infoTitle.textContent = displayTitle;
    els.infoUploader.textContent = uploader || '-';
    els.infoDuration.textContent = formatDuration(duration);
    if (thumbnail) {
        els.infoThumb.style.backgroundImage = `url('${thumbnail}')`;
    } else {
        els.infoThumb.style.backgroundImage = '';
    }
};

const updateDownloadOptionHints = () => {
    const preset = presets[getSelectedPresetKey()];
    const isTranscription = Boolean(preset?.transcribeText);
    const transcriptionOptions = document.getElementById('transcriptionOptions');
    if (transcriptionOptions) transcriptionOptions.hidden = !isTranscription;

    const captionPlatform = detectPlatform(els.urlInput.value.trim());
    const captionPlatformNames = {
        youtube: 'YouTube',
        tiktok: 'TikTok',
        instagram: 'Instagram',
        facebook: 'Facebook',
        x: 'X',
        reddit: 'Reddit',
    };
    const captionPlatformName = captionPlatformNames[captionPlatform];
    els.captionHint.hidden = !captionPlatformName;
    if (captionPlatformName) {
        els.captionHint.textContent = els.saveCaptions.checked
            ? `${captionPlatformName} caption: saved as .caption.txt when available.`
            : `${captionPlatformName} caption: enable Save post captions in Settings.`;
    }
};

const renderQueueControls = () => {
    const queuedCount = state.queueIds.length;
    const isBusy = state.queueWorkerRunning || Array.from(state.jobs.values()).some(job => isQueueBusy(job.state));
    const setQueueModeHint = text => {
        if (els.queueModeHint) {
            els.queueModeHint.textContent = text;
            els.queueModeHint.hidden = !text;
        }
    };

    els.queueAutoStartBtn.textContent = `Auto-start: ${state.queueAutoStartEnabled ? 'on' : 'off'}`;
    els.queueAutoStartBtn.setAttribute('aria-pressed', String(state.queueAutoStartEnabled));

    els.startQueueBtn.disabled = (state.queueAutoStartEnabled && !state.queuePaused) || queuedCount === 0 || isBusy;
    els.clearQueueBtn.disabled = state.jobs.size === 0 || clearQueueInFlight;
    els.pauseQueueBtn.disabled = !state.queuePaused && queuedCount === 0 && !isBusy;
    els.pauseQueueBtn.textContent = state.queuePaused ? 'Resume queue' : 'Pause after current';

    if (state.queuePaused) {
        setQueueModeHint('Paused. The current item can finish; waiting items start when you resume.');
        return;
    }

    if (state.queueAutoStartEnabled) {
        setQueueModeHint('');
        return;
    }

    if (isBusy) {
        setQueueModeHint('Manual mode is active. The current queue run will finish before new items wait.');
        return;
    }

    if (queuedCount > 0) {
        setQueueModeHint('Manual mode is active. Build the queue first, then click Download.');
        return;
    }

    setQueueModeHint('Manual mode is active. New items stay queued until you click Download.');
};

const toggleQueueCollapsed = () => {
    state.queueCollapsed = !state.queueCollapsed;
    els.queueCollapseBtn.textContent = state.queueCollapsed ? 'Show queue' : 'Hide queue';
    els.queueCollapseBtn.setAttribute('aria-expanded', String(!state.queueCollapsed));
    els.queueProgressView.hidden = state.queueCollapsed;
    document.querySelector('.pinefetch-split')?.classList.toggle('pinefetch-queue-collapsed', state.queueCollapsed);
    if (!state.queueCollapsed) scheduleQueueRender();
};

const getQueueMetaItems = job => {
    const items = [job.formatLabel || ''];
    if (job.state === 'downloading') {
        if (Number.isFinite(job.percent)) items.push(`Progress: ${Math.round(job.percent)}%`);
        if (job.speed && job.speed !== '-') items.push(`Speed: ${job.speed}`);
        if (job.eta && job.eta !== '-') items.push(`ETA: ${job.eta}`);
    }
    if (job.state === 'transcribing' || job.state === 'cancelling') {
        items.push(job.state === 'transcribing' ? 'Creating transcript' : 'Stopping download');
    }
    const cutStartLabel = formatCutStartLabel(job.cutStartTime);
    if (cutStartLabel) items.push(cutStartLabel);
    return items;
};

const renderQueue = () => {
    const items = Array.from(state.jobs.values()).sort((a, b) => a.createdAt - b.createdAt);
    const focusedItem = document.activeElement?.closest?.('.pinefetch-queue-item');
    const focusedJobId = focusedItem?.dataset.jobId;
    const focusedMoreButton = document.activeElement?.classList?.contains('pinefetch-queue-more-btn');
    els.queueList.replaceChildren();
    queueCardElements.clear();
    items.forEach(job => {
        const item = document.createElement('div');
        item.className = `oj-panel oj-panel-compact oj-panel-interactive pinefetch-queue-item ${job.id === state.selectedId ? 'pinefetch-is-active' : ''}`;
        item.dataset.jobId = job.id;
        if (job.id === state.selectedId) item.dataset.ojState = 'selected';
        item.oncontextmenu = event => {
            event.preventDefault();
            state.selectedId = job.id;
            scheduleQueueRender();
            openQueueContextMenu(job, event.clientX, event.clientY);
        };
        item.onclick = async () => {
            hideQueueContextMenu();
            state.selectedId = job.id;
            scheduleQueueRender();
            if ((job.state === 'success' || job.state === 'transcribing') && job.outputPath && api.available) {
                try {
                    await api.openFolder({ path: job.outputPath });
                } catch (err) {
                    appendLog(`[open] ${err}`, true);
                }
            }
        };

        const header = document.createElement('div');
        header.className = 'pinefetch-queue-header';

        const title = document.createElement('button');
        title.className = 'pinefetch-queue-title';
        title.type = 'button';
        title.setAttribute(
            'aria-label',
            `${(job.state === 'success' || job.state === 'transcribing') && job.outputPath ? 'Open folder for' : 'Select'} ${job.label || job.url}`
        );
        const platform = detectPlatform(job.url || '');
        if (platform) {
            const platformIcon = document.createElement('span');
            platformIcon.className = `pinefetch-queue-platform-icon pinefetch-platform-${platform}`;
            const icon = getPlatformIconElement(platform);
            if (icon) {
                platformIcon.appendChild(icon);
                title.appendChild(platformIcon);
            }
        }
        const titleText = document.createElement('span');
        titleText.className = 'pinefetch-queue-title-text';
        titleText.textContent = job.label || job.url;
        title.appendChild(titleText);

        const badge = document.createElement('div');
        const badgeVariant =
            {
                success: 'oj-badge-success',
                error: 'oj-badge-danger',
                downloading: 'oj-badge-info',
                transcribing: 'oj-badge-info',
                cancelling: 'oj-badge-warning',
            }[job.state] || '';
        badge.className = `oj-badge ${badgeVariant} pinefetch-queue-badge`;
        badge.textContent =
            { success: 'Completed', error: 'Failed', transcribing: 'Transcribing' }[job.state] || job.state || 'queued';

        const moreBtn = document.createElement('button');
        moreBtn.className = 'oj-icon-button pinefetch-queue-more-btn';
        moreBtn.type = 'button';
        moreBtn.appendChild(createIcon('ellipsis'));
        moreBtn.setAttribute('aria-haspopup', 'menu');
        moreBtn.setAttribute('aria-controls', 'queueContextMenu');
        moreBtn.setAttribute('aria-expanded', String(state.contextMenuJobId === job.id));
        moreBtn.setAttribute('aria-label', `More actions for ${job.label || job.url}`);
        moreBtn.onclick = event => {
            event.stopPropagation();
            const rect = moreBtn.getBoundingClientRect();
            state.selectedId = job.id;
            scheduleQueueRender();
            openQueueContextMenu(job, rect.right, rect.bottom);
        };

        header.append(title, badge, moreBtn);

        const isDownloading = job.state === 'downloading';
        const isProcessing = job.state === 'transcribing' || job.state === 'cancelling';
        const progress = document.createElement(isProcessing ? 'div' : 'progress');
        progress.className = `oj-progress${isProcessing ? ' oj-progress-indeterminate' : ''}`;
        progress.hidden = !isDownloading && !isProcessing && job.state !== 'success';
        progress.setAttribute(
            'aria-label',
            isProcessing ? (job.state === 'transcribing' ? 'Transcribing' : 'Cancelling') : 'Download progress'
        );
        if (isProcessing) {
            progress.setAttribute('role', 'progressbar');
        } else {
            progress.max = 100;
            progress.value = Math.max(0, Math.min(100, job.state === 'success' ? 100 : Number(job.percent) || 0));
        }

        const meta = document.createElement('div');
        meta.className = 'pinefetch-queue-meta';
        appendTextSpans(meta, getQueueMetaItems(job));
        const main = document.createElement('div');
        main.className = 'pinefetch-list-card-layout pinefetch-queue-main';

        const content = document.createElement('div');
        content.className = 'pinefetch-queue-content';
        content.append(header, progress, meta);
        if (job.clearError || job.error) {
            const errorText = document.createElement('p');
            const isHistoryWarning = job.state === 'success' && !job.clearError;
            errorText.className = `oj-status ${isHistoryWarning ? 'oj-status-warning' : 'oj-status-error'} pinefetch-queue-error`;
            errorText.textContent =
                job.clearError || (job.state === 'success' ? `History warning: ${job.error}` : job.error);
            content.appendChild(errorText);
        }
        main.appendChild(content);

        const thumbUrl = job.thumbnail || resolveYouTubeThumbnail(job.url);
        if (thumbUrl) {
            const thumb = document.createElement('div');
            thumb.className = 'pinefetch-media-thumbnail pinefetch-queue-thumb';
            thumb.style.backgroundImage = `url('${thumbUrl}')`;
            main.appendChild(thumb);
        } else {
            main.classList.add('pinefetch-no-media');
        }

        item.append(main);
        els.queueList.appendChild(item);
        queueCardElements.set(job.id, item);
    });
    if (focusedJobId) {
        const restoredItem = Array.from(els.queueList.children).find(item => item.dataset.jobId === focusedJobId);
        restoredItem
            ?.querySelector(focusedMoreButton ? '.pinefetch-queue-more-btn' : '.pinefetch-queue-title')
            ?.focus({ preventScroll: true });
    }

    if (state.contextMenuJobId && !state.jobs.has(state.contextMenuJobId)) {
        hideQueueContextMenu();
    }
    syncQueueContextMenuState();
    const activeCount = items.filter(job => isQueueBusy(job.state)).length;
    els.queueBadge.textContent = `${state.queueIds.length} waiting · ${activeCount} active`;
    els.queueEmptyHint.hidden = items.length > 0;
    els.queueList.hidden = items.length === 0;
    renderQueueControls();
};

const renderQueueProgress = () => {
    for (const id of queueProgressDirtyIds) {
        const item = queueCardElements.get(id);
        const job = state.jobs.get(id);
        if (!item || !job || job.state !== 'downloading') {
            queueRenderDirty = true;
            return;
        }
        const progress = item.querySelector('.oj-progress');
        const percent = Math.max(0, Math.min(100, Number(job.percent) || 0));
        progress.value = percent;
        const meta = item.querySelector('.pinefetch-queue-meta');
        meta.replaceChildren();
        appendTextSpans(meta, getQueueMetaItems(job));
    }
};

const flushQueueRender = () => {
    if (
        state.activeView !== 'download' ||
        queueRenderFrame !== null ||
        (!queueRenderDirty && queueProgressDirtyIds.size === 0)
    )
        return;

    queueRenderFrame = requestAnimationFrame(() => {
        queueRenderFrame = null;
        if (state.activeView !== 'download') return;
        if (!queueRenderDirty && queueProgressDirtyIds.size > 0) renderQueueProgress();
        if (queueRenderDirty) {
            queueRenderDirty = false;
            renderQueue();
        }
        queueProgressDirtyIds.clear();
    });
};

const scheduleQueueRender = ({ progressOnly = false } = {}) => {
    if (!progressOnly) queueRenderDirty = true;
    flushQueueRender();
};

const createLogLine = ({ text, isError }) => {
    const line = document.createElement('div');
    line.className = `pinefetch-terminal-line pinefetch-log-line ${isError ? 'oj-status-error' : ''}`;
    line.textContent = text;
    return line;
};

const renderLogs = () => {
    if (!logDomDirty) return;
    const fragment = document.createDocumentFragment();
    state.logs.forEach(entry => fragment.appendChild(createLogLine(entry)));
    els.logBody.replaceChildren(fragment);
    logDomDirty = false;
    els.logBody.scrollTop = els.logBody.scrollHeight;
};

const appendLog = (text, isError) => {
    state.logs.push({ text, isError: Boolean(isError) });
    if (state.logs.length > maxLogLines) {
        state.logs.splice(0, state.logs.length - maxLogLines);
    }

    els.copyLogsBtn.disabled = false;
    els.clearLogsBtn.disabled = false;

    if (state.activeView !== 'settings' || !settingsLogRenderReady) {
        logDomDirty = true;
        return;
    }

    if (logDomDirty) {
        renderLogs();
        return;
    }

    els.logBody.appendChild(createLogLine(state.logs[state.logs.length - 1]));
    while (els.logBody.childElementCount > maxLogLines) {
        els.logBody.firstElementChild?.remove();
    }
    els.logBody.scrollTop = els.logBody.scrollHeight;
};

const clearLogs = () => {
    state.logs.length = 0;
    logDomDirty = false;
    els.logBody.replaceChildren();
    els.copyLogsBtn.disabled = true;
    els.clearLogsBtn.disabled = true;
};

const updateJob = (id, patch) => {
    const mode = updateJobState(state, id, patch, state.jobs.has(id) ? undefined : Date.now());
    if (mode === 'skip') return;
    const progressOnly = mode === 'progress';
    if (progressOnly) queueProgressDirtyIds.add(id);
    scheduleQueueRender({ progressOnly });
};

const thumbnailHydrationQueue = [];
const maxConcurrentThumbnailHydrations = 2;
let activeThumbnailHydrations = 0;

const drainThumbnailHydrationQueue = () => {
    while (activeThumbnailHydrations < maxConcurrentThumbnailHydrations && thumbnailHydrationQueue.length > 0) {
        const id = thumbnailHydrationQueue.shift();
        const job = state.jobs.get(id);
        if (!job?.previewLoading || job.previewResolved) continue;
        activeThumbnailHydrations += 1;
        void (async () => {
            try {
                const info = await api.loadInfo({ url: job.url });
                const current = state.jobs.get(id);
                if (!current) return;

                const patch = {
                    previewLoading: false,
                    previewResolved: true,
                };
                if (info?.thumbnail) patch.thumbnail = info.thumbnail;
                if (info?.title && (!current.label || current.label === current.url)) {
                    patch.label = info.title;
                }
                updateJob(id, patch);
            } catch {
                if (state.jobs.has(id)) {
                    updateJob(id, { previewLoading: false, previewResolved: true });
                }
            } finally {
                activeThumbnailHydrations -= 1;
                drainThumbnailHydrationQueue();
            }
        })();
    }
};

const maybeHydrateQueueThumbnail = id => {
    if (!api.available) return;
    const job = state.jobs.get(id);
    if (!job?.url || job.thumbnail || job.previewResolved || job.previewLoading) return;
    updateJob(id, { previewLoading: true });
    thumbnailHydrationQueue.push(id);
    drainThumbnailHydrationQueue();
};

const syncQueueStatus = async () => {
    if (!api.available) return;
    try {
        const status = await api.getQueueStatus();
        applyQueueStatus(state, status, true);
        renderQueueControls();
        return status;
    } catch (err) {
        appendLog(`[queue] ${err}`, true);
    }
};

const browserImport = createBrowserImportView({ els, api, appendLog, appendTextSpans });
const { applyLinkDumpServerStatus, syncLinkDumpOverview } = browserImport;

let loadInfoInFlight = false;
let loadInfoPending = false;
let loadInfoRequestId = 0;

const canLoadInfoForUrl = url => isValidHttpUrl(url) && Boolean(detectPlatform(url));

const loadInfo = async () => {
    const url = els.urlInput.value.trim();
    if (!url) return;
    if (!isValidHttpUrl(url)) {
        shakeUrlInput();
        return;
    }
    if (loadInfoInFlight) {
        loadInfoPending = true;
        return;
    }

    const requestUrl = url;
    const requestId = ++loadInfoRequestId;
    loadInfoInFlight = true;
    els.loadInfoBtn.classList.add('oj-button-loading');
    els.loadInfoBtn.setAttribute('aria-busy', 'true');
    els.loadInfoBtn.disabled = true;
    setInfoBadge('Loading...');
    try {
        const info = await api.loadInfo({ url: requestUrl });
        if (requestId !== loadInfoRequestId || els.urlInput.value.trim() !== requestUrl) {
            loadInfoPending = true;
            return;
        }
        state.info = info;
        state.infoUrl = requestUrl;
        renderInfo();
        setInfoBadge('Ready');
    } catch (err) {
        if (requestId !== loadInfoRequestId || els.urlInput.value.trim() !== requestUrl) return;
        if (`${err || ''}`.includes('URL must start with')) {
            shakeUrlInput();
        }
        state.info = null;
        state.infoUrl = null;
        renderInfo();
        setInfoBadge('Error');
        appendLog(`[info] ${err}`, true);
    } finally {
        loadInfoInFlight = false;
        els.loadInfoBtn.classList.remove('oj-button-loading');
        els.loadInfoBtn.removeAttribute('aria-busy');
        els.loadInfoBtn.disabled = false;

        const nextUrl = els.urlInput.value.trim();
        const inputChanged = nextUrl !== requestUrl;
        const shouldReload = (loadInfoPending || inputChanged) && canLoadInfoForUrl(nextUrl);
        loadInfoPending = false;

        if (requestId === loadInfoRequestId && !inputChanged && document.activeElement === els.loadInfoBtn) {
            els.urlInput.focus();
        }

        if (shouldReload) {
            setInfoBadge('Loading...');
            window.setTimeout(() => {
                void loadInfo();
            }, 0);
        }
    }
};

const enqueueDownloadForUrl = async (url, presetKey, options = {}) => {
    if (!url) return null;
    if (!api.available) return null;
    if (!isValidHttpUrl(url)) {
        shakeUrlInput();
        return null;
    }

    const preset = presets[presetKey] || presets.best;
    if (!preset) {
        setInfoBadge('Formats unavailable');
        return null;
    }
    const output_dir = els.outputDir.value.trim() || null;
    const cutAtTimestampEnabled = Boolean(els.cutAtTimestampEnabled.checked);
    const cutStartTime = cutAtTimestampEnabled ? extractUrlStartTimestamp(url) : null;
    const hasLoadedInfo = state.info && state.infoUrl === url;
    const fallbackThumbnail = resolveYouTubeThumbnail(url);
    const thumbnail = options.thumbnail ?? (hasLoadedInfo ? state.info?.thumbnail || null : fallbackThumbnail);

    // Use title primarily; fallback to truncated description only if title missing
    const infoTitle = state.info?.title;
    const infoDescription = state.info?.description;
    let displayLabel = infoTitle;
    if (!displayLabel || displayLabel.trim() === '') {
        displayLabel = infoDescription && infoDescription.trim() ? infoDescription : url;
        if (displayLabel.length > 100) {
            displayLabel = displayLabel.substring(0, 100) + '...';
        }
    }
    const label = options.label ?? (hasLoadedInfo ? displayLabel || url : url);
    const titleForRequest = hasLoadedInfo ? state.info?.title || null : null;
    const uploaderForRequest = hasLoadedInfo ? state.info?.uploader || null : null;
    const thumbnailForRequest = hasLoadedInfo ? state.info?.thumbnail || null : (options.thumbnail ?? null);
    const uploadDateForRequest = hasLoadedInfo ? state.info?.upload_date || null : null;
    const timestampForRequest = hasLoadedInfo ? (state.info?.timestamp ?? null) : null;
    const durationSecondsForRequest = hasLoadedInfo ? (state.info?.duration ?? null) : null;

    try {
        await settings.waitForPendingSave();
        const id = await api.enqueueDownload({
            request: {
                url,
                format: preset.format,
                output_dir,
                extract_audio: preset.extractAudio,
                audio_format: preset.audioFormat,
                transcribe_text: preset.transcribeText,
                transcribe_timestamps: preset.transcribeTimestamps,
                cut_at_timestamp_enabled: cutAtTimestampEnabled,
                cut_start_time: cutStartTime,
                filename_suffix: preset.filenameSuffix,
                title: titleForRequest,
                uploader: uploaderForRequest,
                thumbnail: thumbnailForRequest,
                upload_date: uploadDateForRequest,
                timestamp: timestampForRequest,
                duration_seconds: durationSecondsForRequest,
            },
        });

        const existingJob = state.jobs.get(id);
        updateJob(id, {
            url,
            label,
            thumbnail,
            state: existingJob?.state || 'queued',
            outputPath: existingJob?.outputPath || null,
            previewResolved: Boolean(hasLoadedInfo || thumbnail || fallbackThumbnail),
            previewLoading: false,
            percent: existingJob?.percent ?? 0,
            speed: existingJob?.speed || '-',
            eta: existingJob?.eta || '-',
            formatLabel: preset.queueLabel,
            cutStartTime,
        });
        void settings.cacheLastDownloadedUrl(url);
        maybeHydrateQueueThumbnail(id);

        if (!options.preserveComposerState) {
            els.urlInput.value = '';
            updateDownloadOptionHints();
            state.info = null;
            state.infoUrl = null;
            renderInfo();
            els.urlInput.focus();
        }
        return id;
    } catch (err) {
        if (`${err || ''}`.includes('URL must start with')) {
            shakeUrlInput();
        }
        appendLog(`[queue] ${err}`, true);
        return null;
    }
};

const removeJobFromQueue = async job => {
    const existingJob = state.jobs.get(job.id);
    if (!existingJob) return;

    const previousQueueIds = [...state.queueIds];
    const previousSelectedId = state.selectedId;
    const wasSuppressed = state.suppressedJobIds.has(job.id);

    state.suppressedJobIds.add(job.id);
    state.jobs.delete(job.id);
    state.queueIds = state.queueIds.filter(id => id !== job.id);
    if (state.selectedId === job.id) state.selectedId = null;
    scheduleQueueRender();

    if (job.state !== 'queued') return;

    try {
        await api.cancelDownload({ id: job.id });
    } catch (err) {
        if (!wasSuppressed) state.suppressedJobIds.delete(job.id);
        state.jobs.set(job.id, existingJob);
        state.queueIds = previousQueueIds;
        state.selectedId = previousSelectedId;
        scheduleQueueRender();
        appendLog(`[remove] ${err}`, true);
    }
};

const enqueueDownload = async () => {
    const url = els.urlInput.value.trim();
    const presetKey = getSelectedPresetKey();
    await enqueueDownloadForUrl(url, presetKey);
};

const toggleQueueAutoStart = async () => {
    if (!api.available) return;
    try {
        const status = await api.setQueueAutoStart({
            enabled: !state.queueAutoStartEnabled,
        });
        applyQueueStatus(state, status, !state.queueAutoStartEnabled);
        renderQueueControls();
    } catch (err) {
        appendLog(`[queue] ${err}`, true);
    }
};

const startQueueProcessing = async () => {
    if (!api.available) return;
    try {
        const status = await api.startQueue();
        applyQueueStatus(state, status, state.queueAutoStartEnabled);
        renderQueueControls();
    } catch (err) {
        appendLog(`[queue] ${err}`, true);
    }
};

const toggleQueuePause = async () => {
    if (!api.available) return;
    try {
        const status = await (state.queuePaused ? api.resumeQueue() : api.pauseQueue());
        applyQueueStatus(state, status, state.queueAutoStartEnabled);
        renderQueueControls();
    } catch (err) {
        appendLog(`[queue] ${err}`, true);
    }
};

const pluralize = (count, singular, plural = `${singular}s`) => `${count} ${count === 1 ? singular : plural}`;

const formatTxtImportCounts = (importedCount, invalidCount, duplicateCount, failedCount = 0) => {
    const parts = [
        `Imported ${pluralize(importedCount, 'link')}.`,
        `Skipped ${pluralize(invalidCount, 'invalid line')} and ${pluralize(duplicateCount, 'duplicate')}.`,
    ];
    if (failedCount > 0) parts.push(`${pluralize(failedCount, 'link')} failed to queue.`);
    return parts.join(' ');
};

const setTxtImportStatus = (message, isError = false) => {
    if (!els.txtImportStatus) return;
    els.txtImportStatus.textContent = message;
    els.txtImportStatus.hidden = !message;
    els.txtImportStatus.classList.toggle('oj-status-error', Boolean(message && isError));
    els.txtImportStatus.classList.toggle('oj-status-success', Boolean(message && !isError));
};

const setTxtImportBusy = isBusy => {
    if (!els.importTxtBtn) return;
    els.importTxtBtn.disabled = isBusy;
    els.importTxtBtn.textContent = isBusy ? 'Importing...' : 'Import TXT';
};

const importTxtLinks = async () => {
    if (!api.available) {
        setTxtImportStatus('TXT import is only available in the Tauri app.', true);
        return;
    }

    setTxtImportStatus('');
    setTxtImportBusy(true);
    try {
        const file = await api.pickTxtFile();
        if (!file) return;

        const parsed = parseTxtImportLinks(file.content);
        if (parsed.isEmpty) {
            const message = 'TXT file is empty.';
            setTxtImportStatus(message, true);
            appendLog(`[txt-import] ${message}`, true);
            return;
        }

        if (parsed.items.length === 0) {
            const message = `No valid YouTube, TikTok, or Instagram links found. Skipped ${pluralize(
                parsed.invalidCount,
                'invalid line'
            )} and ${pluralize(parsed.duplicateCount, 'duplicate')}.`;
            setTxtImportStatus(message, true);
            appendLog(`[txt-import] ${message}`, true);
            return;
        }

        const presetKey = getSelectedPresetKey();
        const queuedKeys = getQueuedTxtImportKeys();
        let importedCount = 0;
        let duplicateCount = parsed.duplicateCount;
        let failedCount = 0;

        for (const item of parsed.items) {
            if (queuedKeys.has(item.key)) {
                duplicateCount += 1;
                continue;
            }

            queuedKeys.add(item.key);
            const id = await enqueueDownloadForUrl(item.url, presetKey, {
                preserveComposerState: true,
            });
            if (id) {
                importedCount += 1;
            } else {
                failedCount += 1;
            }
        }

        const skippedSummary = `Skipped ${pluralize(parsed.invalidCount, 'invalid line')} and ${pluralize(
            duplicateCount,
            'duplicate'
        )}.`;
        const message =
            importedCount === 0 && failedCount === 0 && duplicateCount > 0
                ? `No new links imported. ${skippedSummary}`
                : formatTxtImportCounts(importedCount, parsed.invalidCount, duplicateCount, failedCount);
        const isError = importedCount === 0;
        setTxtImportStatus(message, isError);
        appendLog(`[txt-import] ${message}`, isError || failedCount > 0);
    } catch (err) {
        const message = `TXT import failed: ${err}`;
        setTxtImportStatus(message, true);
        appendLog(`[txt-import] ${message}`, true);
    } finally {
        setTxtImportBusy(false);
    }
};

const clearQueue = async () => {
    if (clearQueueInFlight) return;
    clearQueueInFlight = true;
    renderQueueControls();
    const idsToCancel = new Set(state.queueIds);
    state.jobs.forEach(job => {
        if (
            job.state === 'queued' ||
            job.state === 'downloading' ||
            job.state === 'transcribing' ||
            job.state === 'cancelling'
        ) {
            idsToCancel.add(job.id);
        }
    });

    try {
        const ids = Array.from(idsToCancel);
        const results = api.available
            ? await Promise.allSettled(ids.map(id => api.cancelDownload({ id })))
            : ids.map(() => ({ status: 'rejected', reason: 'Backend unavailable' }));
        const confirmed = new Set();
        let cancelFailed = false;
        results.forEach((result, index) => {
            const id = ids[index];
            if (result.status === 'fulfilled') {
                confirmed.add(id);
                return;
            }
            const reason = `${result.reason || 'Unknown error'}`;
            if (reason.toLowerCase().includes('job not found')) {
                const job = state.jobs.get(id);
                if (job && ['success', 'error', 'cancelled'].includes(job.state)) {
                    confirmed.add(id);
                } else {
                    state.pendingClearAfterTerminal.add(id);
                    cancelFailed = true;
                }
                return;
            }
            cancelFailed = true;
            appendLog(`[clear] ${id}: ${reason}`, true);
            if (state.jobs.has(id)) updateJob(id, { clearError: `Could not cancel: ${reason}` });
        });

        for (const id of state.jobs.keys()) {
            if (idsToCancel.has(id) && !confirmed.has(id)) continue;
            state.suppressedJobIds.add(id);
            state.pendingClearAfterTerminal.delete(id);
            state.jobs.delete(id);
        }
        state.queueIds = state.queueIds.filter(id => !confirmed.has(id));
        if (state.selectedId && !state.jobs.has(state.selectedId)) state.selectedId = null;

        if (cancelFailed && api.available) {
            try {
                const queue = await api.getQueue();
                applyQueueSnapshot(queue);
                const status = await syncQueueStatus();
                if (status && !status.worker_running) {
                    for (const id of state.pendingClearAfterTerminal) {
                        if (state.queueIds.includes(id)) continue;
                        state.pendingClearAfterTerminal.delete(id);
                        state.suppressedJobIds.add(id);
                        state.jobs.delete(id);
                    }
                }
            } catch (err) {
                appendLog(`[clear] Could not refresh queue: ${err}`, true);
            }
        }
    } finally {
        clearQueueInFlight = false;
        scheduleQueueRender();
    }
};

const settings = createSettingsView({
    els,
    api,
    appendLog,
    setSelectedPresetKey,
    getSelectedPresetKey,
    updateDownloadOptionHints,
    isActive: () => state.activeView === 'settings',
});

const historyView = createHistoryView({
    els,
    api,
    appendLog,
    formatFileSize,
    formatDuration,
    detectPlatform,
    appendTextSpans,
    isActive: () => state.activeView === 'history',
});

const bindEvents = () => {
    historyView.bindEvents();
    settings.bindEvents();
    browserImport.bindEvents();
    els.magicImportTrigger.addEventListener('click', () => {
        void tryMagicImport();
    });
    window.addEventListener('focus', () => {
        void tryMagicImport();
    });
    els.loadInfoBtn.addEventListener('click', loadInfo);
    els.startDownloadBtn.addEventListener('click', enqueueDownload);
    els.presetDropdown.addEventListener('oj:select', event => {
        if (event.target !== els.presetDropdown || !Object.hasOwn(presets, event.detail?.value)) return;
        setSelectedPresetKey(event.detail.value);
        void settings.persistSelectedPresetKey();
    });
    els.importTxtBtn.addEventListener('click', () => {
        void importTxtLinks();
    });
    els.queueAutoStartBtn.addEventListener('click', () => {
        void toggleQueueAutoStart();
    });
    els.queueCollapseBtn.addEventListener('click', toggleQueueCollapsed);
    els.startQueueBtn.addEventListener('click', () => {
        void startQueueProcessing();
    });
    els.pauseQueueBtn.addEventListener('click', () => {
        void toggleQueuePause();
    });

    let urlInputDebounceTimer = null;
    els.urlInput.addEventListener('input', () => {
        if (urlInputDebounceTimer !== null) {
            clearTimeout(urlInputDebounceTimer);
            urlInputDebounceTimer = null;
        }
        const url = els.urlInput.value.trim();
        updateDownloadOptionHints();
        if (!canLoadInfoForUrl(url)) {
            loadInfoRequestId += 1;
            loadInfoPending = false;
            state.info = null;
            state.infoUrl = null;
            renderInfo();
            setInfoBadge(!url ? 'Idle' : isValidHttpUrl(url) ? 'Unsupported link' : 'Invalid URL');
            return;
        }
        urlInputDebounceTimer = setTimeout(() => {
            urlInputDebounceTimer = null;
            if (!canLoadInfoForUrl(els.urlInput.value.trim())) return;
            setInfoBadge('Loading...');
            void loadInfo();
        }, 600);
    });

    els.urlInput.addEventListener('keydown', event => {
        const key = event.key.toLowerCase();
        if (event.metaKey && !event.ctrlKey && !event.altKey && key === 'i') {
            if (!els.urlInput.value.trim()) return;
            event.preventDefault();
            void loadInfo();
            return;
        }
        if (key === 'enter' && !event.metaKey && !event.ctrlKey && !event.altKey) {
            if (!els.urlInput.value.trim()) return;
            event.preventDefault();
            void enqueueDownload();
            return;
        }
        if (key === 'escape') {
            event.preventDefault();
            loadInfoRequestId += 1;
            loadInfoPending = false;
            els.urlInput.value = '';
            updateDownloadOptionHints();
            state.info = null;
            state.infoUrl = null;
            renderInfo();
            setInfoBadge('Idle');
            els.loadInfoBtn.classList.remove('oj-button-loading');
            els.loadInfoBtn.removeAttribute('aria-busy');
            els.loadInfoBtn.disabled = false;
            els.urlInput.focus();
            return;
        }
    });
    els.clearQueueBtn.addEventListener('click', () => {
        void clearQueue();
    });
    els.viewDownloadBtn.addEventListener('click', () => setActiveView('download'));
    els.viewHistoryBtn.addEventListener('click', () => setActiveView('history'));
    els.viewLinkDumpBtn.addEventListener('click', () => setActiveView('linkDump'));
    els.viewSettingsBtn.addEventListener('click', () => setActiveView('settings'));
    els.queueContextMenu.addEventListener('contextmenu', event => {
        event.preventDefault();
    });
    let menuSearch = '';
    let menuSearchTimer = null;
    els.queueContextMenu.addEventListener('keydown', event => {
        const items = Array.from(els.queueContextMenu.querySelectorAll('button')).filter(
            button => !button.disabled && !button.hidden
        );
        if (event.key === 'Tab') {
            const jobId = state.contextMenuJobId;
            hideQueueContextMenu();
            queueCardElements.get(jobId)?.querySelector('.pinefetch-queue-more-btn')?.focus();
            return;
        }
        const index = items.indexOf(document.activeElement);
        let next = null;
        if (event.key === 'ArrowDown') next = items[(index + 1) % items.length];
        if (event.key === 'ArrowUp') next = items[(index - 1 + items.length) % items.length];
        if (event.key === 'Home') next = items[0];
        if (event.key === 'End') next = items.at(-1);
        if (event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
            if (menuSearchTimer !== null) clearTimeout(menuSearchTimer);
            menuSearch += event.key.toLowerCase();
            menuSearchTimer = setTimeout(() => {
                menuSearch = '';
                menuSearchTimer = null;
            }, 500);
            next = items.find(button => button.textContent.trim().toLowerCase().startsWith(menuSearch));
        }
        if (next) {
            event.preventDefault();
            next.focus();
        }
    });
    els.queueContextMenu.addEventListener('click', async event => {
        const button = event.target.closest('button[data-action]');
        if (!button) return;

        const job = getContextMenuJob();
        hideQueueContextMenu({ restoreFocus: true });
        if (!job) return;

        if (button.dataset.action === 'copy-link') {
            try {
                await navigator.clipboard.writeText(job.url || '');
                toast('Link copied.', { type: 'success' });
            } catch (err) {
                appendLog(`[copy] ${err}`, true);
            }
            return;
        }

        if (button.dataset.action === 'download') {
            if (!job.url) return;
            await enqueueDownloadForUrl(job.url, button.dataset.presetKey, {
                preserveComposerState: true,
                label: job.label || job.url,
                thumbnail: job.thumbnail || null,
            });
            return;
        }

        if (button.dataset.action === 'cancel') {
            try {
                await api.cancelDownload({ id: job.id });
            } catch (err) {
                appendLog(`[cancel] ${err}`, true);
            }
            return;
        }

        if (button.dataset.action === 'remove') {
            await removeJobFromQueue(job);
            requestAnimationFrame(() => {
                const nextTrigger = els.queueList.querySelector('.pinefetch-queue-more-btn');
                (nextTrigger || els.queueAutoStartBtn).focus({ preventScroll: true });
            });
        }
    });
    document.addEventListener('pointerdown', event => {
        const target = event.target instanceof Element ? event.target : null;
        if (els.queueContextMenu.hidden) return;
        if (target && els.queueContextMenu.contains(target)) return;
        hideQueueContextMenu();
    });
    document.addEventListener('contextmenu', event => {
        const target = event.target instanceof Element ? event.target : null;
        if (target && els.queueContextMenu.contains(target)) {
            event.preventDefault();
            return;
        }
        if (!target?.closest('.pinefetch-queue-item')) hideQueueContextMenu();
    });
    document.addEventListener('keydown', event => {
        if (event.key !== 'Escape' || els.queueContextMenu.hidden) return;
        const jobId = state.contextMenuJobId;
        hideQueueContextMenu();
        Array.from(els.queueList.children)
            .find(item => item.dataset.jobId === jobId)
            ?.querySelector('.pinefetch-queue-more-btn')
            ?.focus({ preventScroll: true });
    });
    window.addEventListener('resize', hideQueueContextMenu);
    window.addEventListener('blur', hideQueueContextMenu);
    els.queueList.addEventListener('scroll', hideQueueContextMenu, { passive: true });

    els.copyLogsBtn.addEventListener('click', async () => {
        try {
            await navigator.clipboard.writeText(state.logs.map(entry => entry.text).join('\n'));
            toast('Logs copied.', { type: 'success' });
        } catch (err) {
            appendLog(`[copy] ${err}`, true);
        }
    });
    els.clearLogsBtn.addEventListener('click', clearLogs);
};

const applyQueueSnapshot = jobs => {
    const queue = Array.isArray(jobs) ? jobs : [];
    state.queueIds = queue.map(job => job.id).filter(id => !state.suppressedJobIds.has(id));
    queue.forEach(job => {
        if (state.suppressedJobIds.has(job.id)) return;
        const existing = state.jobs.get(job.id);
        const preset = findPresetForDownloadJob(job);
        const fallbackThumbnail = resolveYouTubeThumbnail(job.url);
        updateJob(job.id, {
            url: job.url,
            label: existing?.label || job.url,
            thumbnail: existing?.thumbnail || fallbackThumbnail,
            state: 'queued',
            outputPath: existing?.outputPath || null,
            cutStartTime: job.cut_start_time ?? null,
            previewResolved: existing?.previewResolved || Boolean(fallbackThumbnail),
            previewLoading: existing?.previewLoading || false,
            formatLabel: existing?.formatLabel || preset?.queueLabel || job.format,
        });
        maybeHydrateQueueThumbnail(job.id);
    });
    scheduleQueueRender();
};

const bindBackendEvents = async () => {
    await api.onLinkDumpServerStatus(event => {
        applyLinkDumpServerStatus(event.payload);
    });

    await api.onHistoryChanged(historyView.onChanged);

    await api.onQueueStatus(event => {
        applyQueueStatus(state, event.payload);
        renderQueueControls();
    });

    await api.onQueueUpdate(event => {
        applyQueueSnapshot(event.payload);
    });

    await api.onDownloadState(event => {
        const result = applyDownloadState(state, event.payload);
        if (result.kind === 'ignored') return;
        const { id, exit_code, error } = event.payload;
        if (result.kind === 'removed') {
            scheduleQueueRender();
            return;
        }
        updateJob(id, result.patch);
        if (error) appendLog(`[${id}] ${error} (${exit_code ?? '?'})`, true);
    });

    await api.onDownloadProgress(event => {
        const result = applyDownloadProgress(state, event.payload);
        if (result.kind === 'updated') updateJob(result.id, result.patch);
    });

    await api.onDownloadLog(event => {
        const { id, line, is_error } = event.payload;
        if (state.suppressedJobIds.has(id)) return;
        appendLog(`[${id}] ${line}`, is_error);
    });
};

const init = async () => {
    els.presetTrigger.disabled = true;
    els.presetTrigger.setAttribute('aria-busy', 'true');
    els.startDownloadBtn.disabled = true;
    els.importTxtBtn.disabled = true;
    els.presetValue.textContent = 'Loading formats...';
    settings.syncMagicImportTriggerState();
    renderQueueControls();
    bindEvents();
    const cleanupOJ = initOJ();
    window.addEventListener('pagehide', cleanupOJ, { once: true });
    setActiveView('download');
    requestAnimationFrame(() => {
        els.urlInput.focus();
    });
    if (!api.available || !api.eventsAvailable) {
        els.presetValue.textContent = 'Formats unavailable';
        els.presetTrigger.removeAttribute('aria-busy');
        appendLog('[tauri] API not available. Start the app with `npm run dev` (Tauri), not in a browser.', true);
        return;
    }
    try {
        await loadDownloadPresets();
        els.presetTrigger.disabled = false;
        els.startDownloadBtn.disabled = false;
        els.importTxtBtn.disabled = false;
    } catch (err) {
        els.presetValue.textContent = 'Formats unavailable';
        setInfoBadge('Formats unavailable');
        setTxtImportStatus(`Could not load download formats: ${err}`, true);
        appendLog(`[presets] ${err}`, true);
    } finally {
        els.presetTrigger.removeAttribute('aria-busy');
    }
    await settings.sync();
    await syncQueueStatus();
    await bindBackendEvents();
    try {
        await api.initializeCli();
    } catch (err) {
        appendLog(`[cli] ${err}`, true);
    }
    scheduleQueueRender();
};

init();
