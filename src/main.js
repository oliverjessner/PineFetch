import { createBrowserImportView } from './browser-import-view.js';
import { createSettingsView } from './settings-view.js';
import { createHistoryView } from './history-view.js';
import { detectPlatform, extractUrlStartTimestamp, isValidHttpUrl, normalizeTxtImportUrl, parseTxtImportLinks, resolveYouTubeThumbnail } from './url-utils.js';

const tauriGlobal = window.__TAURI__;
const invoke = tauriGlobal?.core?.invoke;
const listen = tauriGlobal?.event?.listen;
const state = Object.seal({
    jobs: new Map(),
    queueIds: [],
    queueAutoStartEnabled: true,
    queueWorkerRunning: false,
    queuePaused: false,
    queueCollapsed: false,
    suppressedJobIds: new Set(),
    pendingClearAfterTerminal: new Set(),
    selectedId: null,
    contextMenuJobId: null,
    logs: [],
    info: null,
    infoUrl: null,
    activeView: 'download',
});
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
    fasterWhisperModel: document.getElementById('fasterWhisperModel'),
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
    presetSelect: document.getElementById('presetSelect'),
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
const normalizePresetKey = key => (presets[key] ? key : presetLabels[0]?.key || 'best');
const getSelectedPresetKey = () => normalizePresetKey(els.presetSelect.value);
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
    const definitions = await invoke('get_download_presets');
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
const cancellableJobStates = new Set(['downloading', 'transcribing']);
const queueBusyJobStates = new Set(['downloading', 'transcribing', 'cancelling']);
const removableJobStates = new Set(['queued', 'success', 'error', 'cancelled']);
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

const formatDuration = seconds => {
    if (!seconds && seconds !== 0) return '-';

    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    const hrs = Math.floor(mins / 60);

    if (hrs > 0) return `${hrs}h ${String(mins % 60).padStart(2, '0')}m`;
    return `${mins}m ${String(secs).padStart(2, '0')}s`;
};

const formatFileSize = bytes => {
    const size = Number(bytes);
    if (!Number.isFinite(size) || size <= 0) return '0 B';

    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    const unitIndex = Math.min(Math.floor(Math.log(size) / Math.log(1024)), units.length - 1);
    const value = size / 1024 ** unitIndex;
    const precision = unitIndex === 0 || value >= 100 ? 0 : value >= 10 ? 1 : 2;
    return `${value.toFixed(precision)} ${units[unitIndex]}`;
};

const formatCutStartLabel = seconds => {
    if (!Number.isFinite(Number(seconds)) || Number(seconds) <= 0) return null;
    return `from ${formatDuration(Number(seconds))}`;
};

const svgNamespace = 'http://www.w3.org/2000/svg';

const createSvgElement = shapes => {
    const svg = document.createElementNS(svgNamespace, 'svg');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('aria-hidden', 'true');
    svg.setAttribute('focusable', 'false');

    shapes.forEach(({ tag, attrs }) => {
        const shape = document.createElementNS(svgNamespace, tag);
        Object.entries(attrs).forEach(([name, value]) => {
            shape.setAttribute(name, value);
        });
        svg.appendChild(shape);
    });

    return svg;
};

const getPlatformIconElement = platform => {
    switch (platform) {
        case 'youtube':
            return createSvgElement([
                {
                    tag: 'path',
                    attrs: {
                        d: 'M22 12c0 2.7-.3 4.4-.6 5.3-.3.8-.9 1.4-1.7 1.7-.9.3-2.6.6-7.7.6s-6.8-.3-7.7-.6c-.8-.3-1.4-.9-1.7-1.7C2.3 16.4 2 14.7 2 12s.3-4.4.6-5.3c.3-.8.9-1.4 1.7-1.7C5.2 4.7 6.9 4.4 12 4.4s6.8.3 7.7.6c.8.3 1.4.9 1.7 1.7.3.9.6 2.6.6 5.3Z',
                        fill: 'currentColor',
                    },
                },
                { tag: 'path', attrs: { d: 'M10 8.8 15.5 12 10 15.2V8.8Z', fill: '#fff' } },
            ]);
        case 'facebook':
            return createSvgElement([
                {
                    tag: 'path',
                    attrs: {
                        d: 'M13.6 8.6h2.3V5.4h-2.7c-2.6 0-4 1.5-4 4v1.9H7v3.1h2.2v5.2h3.3v-5.2h2.7l.4-3.1h-3.1V9.8c0-.8.3-1.2.8-1.2Z',
                        fill: 'currentColor',
                    },
                },
            ]);
        case 'twitch':
            return createSvgElement([
                {
                    tag: 'path',
                    attrs: {
                        d: 'M4 3h16v11.2l-4 4H12l-2.8 2.8V18.2H4V3Zm2 2v11.2h3.2v1.6l1.6-1.6H15l3-3V5H6Zm4.2 2.4h1.8v4.2h-1.8V7.4Zm4 0H16v4.2h-1.8V7.4Z',
                        fill: 'currentColor',
                    },
                },
            ]);
        case 'x':
            return createSvgElement([
                {
                    tag: 'path',
                    attrs: {
                        d: 'M4 4h3.8l4.7 6.4L17.8 4H20l-6.4 7.3L20.5 20h-3.8l-5-6.8L5.9 20H3.7l6.7-7.6L4 4Z',
                        fill: 'currentColor',
                    },
                },
            ]);
        case 'tiktok':
            return createSvgElement([
                {
                    tag: 'path',
                    attrs: {
                        d: 'M14.5 4c1.1 1.6 2.3 2.4 4 2.5V9c-1.5 0-2.8-.4-4-1.2v6.6a4.8 4.8 0 1 1-3.8-4.7v2.6a2.2 2.2 0 1 0 1.3 2V4h2.5Z',
                        fill: 'currentColor',
                    },
                },
            ]);
        case 'instagram':
            return createSvgElement([
                {
                    tag: 'rect',
                    attrs: {
                        x: '3.5',
                        y: '3.5',
                        width: '17',
                        height: '17',
                        rx: '5',
                        fill: 'none',
                        stroke: 'currentColor',
                        'stroke-width': '2',
                    },
                },
                {
                    tag: 'circle',
                    attrs: {
                        cx: '12',
                        cy: '12',
                        r: '3.5',
                        fill: 'none',
                        stroke: 'currentColor',
                        'stroke-width': '2',
                    },
                },
                { tag: 'circle', attrs: { cx: '17.2', cy: '6.8', r: '1.2', fill: 'currentColor' } },
            ]);
        default:
            return null;
    }
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
    els.urlInput.classList.remove('pf-is-invalid', 'pf-invalid-shake');
    void els.urlInput.offsetWidth;
    els.urlInput.classList.add('pf-is-invalid', 'pf-invalid-shake');
    els.urlInput.focus();
    urlShakeTimer = setTimeout(() => {
        els.urlInput.classList.remove('pf-is-invalid', 'pf-invalid-shake');
        urlShakeTimer = null;
    }, 420);
};

const readClipboardText = async () => {
    if (invoke) {
        const text = await invoke('read_clipboard_text');
        if (typeof text === 'string') return text;
    }
    if (navigator.clipboard?.readText) {
        return navigator.clipboard.readText();
    }
    if (typeof tauriGlobal?.clipboard?.readText === 'function') {
        return tauriGlobal.clipboard.readText();
    }
    return '';
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
};

const setActiveView = view => {
    const isDownload = view === 'download';
    const isHistory = view === 'history';
    const isLinkDump = view === 'linkDump';
    const isSettings = view === 'settings';
    state.activeView = view;
    const split = els.settingsView.closest('.pf-pinefetch-split');
    split.classList.toggle('pf-settings-active', isSettings);
    split.classList.toggle('pf-history-active', isHistory);
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
    split.classList.toggle('pf-queue-collapsed', isDownload && state.queueCollapsed);
    els.historySummaryView.hidden = !isHistory;
    els.linkDumpSideView.hidden = !isLinkDump;
    els.downloadView.classList.toggle('pf-is-active', isDownload);
    els.historyView.classList.toggle('pf-is-active', isHistory);
    els.settingsView.classList.toggle('pf-is-active', isSettings);
    els.linkDumpView.classList.toggle('pf-is-active', isLinkDump);
    els.queueProgressView.classList.toggle('pf-is-active', isDownload);
    els.historySummaryView.classList.toggle('pf-is-active', isHistory);
    els.linkDumpSideView.classList.toggle('pf-is-active', isLinkDump);

    els.viewDownloadBtn.classList.toggle('pf-is-active', isDownload);
    els.viewDownloadBtn.setAttribute('aria-pressed', String(isDownload));
    els.viewHistoryBtn.classList.toggle('pf-is-active', isHistory);
    els.viewHistoryBtn.setAttribute('aria-pressed', String(isHistory));
    els.viewLinkDumpBtn.classList.toggle('pf-is-active', isLinkDump);
    els.viewLinkDumpBtn.setAttribute('aria-pressed', String(isLinkDump));
    els.viewSettingsBtn.classList.toggle('pf-is-active', isSettings);
    els.viewSettingsBtn.setAttribute('aria-pressed', String(isSettings));

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
    const selectedPresetKey = normalizePresetKey(els.presetSelect.value);
    els.presetSelect.replaceChildren();

    presetOptions.forEach(preset => {
        const option = document.createElement('option');
        option.value = preset.key;
        option.textContent = preset.selectLabel;
        els.presetSelect.appendChild(option);
    });

    els.presetSelect.value = selectedPresetKey;
};

const renderQueueContextMenu = () => {
    els.queueContextDownloads.replaceChildren();
    presetOptions.forEach(preset => {
        const button = document.createElement('button');
        button.className = 'pf-menu-item pf-queue-context-menu-btn';
        button.type = 'button';
        button.dataset.action = 'download';
        button.dataset.presetKey = preset.key;
        button.textContent = preset.menuLabel;
        els.queueContextDownloads.appendChild(button);
    });
};

const hideQueueContextMenu = () => {
    state.contextMenuJobId = null;
    els.queueContextMenu.hidden = true;
    els.queueContextMenu.style.left = '';
    els.queueContextMenu.style.top = '';
};

const getContextMenuJob = () => {
    if (!state.contextMenuJobId) return null;
    return state.jobs.get(state.contextMenuJobId) || null;
};

const syncQueueContextMenuState = () => {
    const job = getContextMenuJob();
    const isCancelling = job?.state === 'cancelling';
    const canCancel = Boolean(job && cancellableJobStates.has(job.state));
    const showCancel = Boolean(job && (canCancel || isCancelling));
    const canRemove = Boolean(job && removableJobStates.has(job.state));

    els.queueContextCancelBtn.hidden = !showCancel;
    els.queueContextCancelBtn.disabled = !canCancel;
    els.queueContextCancelBtn.textContent = isCancelling ? 'Cancelling...' : 'Cancel download';
    els.queueContextRemoveBtn.hidden = !canRemove;
};

const openQueueContextMenu = (job, x, y) => {
    state.contextMenuJobId = job.id;
    syncQueueContextMenuState();
    els.queueContextMenu.hidden = false;

    requestAnimationFrame(() => {
        if (els.queueContextMenu.hidden || state.contextMenuJobId !== job.id) return;
        const margin = 12;
        const menuWidth = els.queueContextMenu.offsetWidth;
        const menuHeight = els.queueContextMenu.offsetHeight;
        const left = Math.max(margin, Math.min(x, window.innerWidth - menuWidth - margin));
        const top = Math.max(margin, Math.min(y, window.innerHeight - menuHeight - margin));

        els.queueContextMenu.style.left = `${left}px`;
        els.queueContextMenu.style.top = `${top}px`;
        els.queueContextMenu.querySelector('button:not([hidden])')?.focus();
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
    const isBusy =
        state.queueWorkerRunning || Array.from(state.jobs.values()).some(job => queueBusyJobStates.has(job.state));
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
    document.querySelector('.pf-pinefetch-split')?.classList.toggle('pf-queue-collapsed', state.queueCollapsed);
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
    const focusedItem = document.activeElement?.closest?.('.pf-queue-item');
    const focusedJobId = focusedItem?.dataset.jobId;
    const focusedMoreButton = document.activeElement?.classList?.contains('pf-queue-more-btn');
    els.queueList.replaceChildren();
    queueCardElements.clear();
    items.forEach(job => {
        const item = document.createElement('div');
        item.className = `pf-list-card pf-queue-item ${job.id === state.selectedId ? 'pf-is-active' : ''}`;
        item.dataset.jobId = job.id;
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
            if ((job.state === 'success' || job.state === 'transcribing') && job.outputPath && invoke) {
                try {
                    await invoke('open_folder', { path: job.outputPath });
                } catch (err) {
                    appendLog(`[open] ${err}`, true);
                }
            }
        };

        const header = document.createElement('div');
        header.className = 'pf-queue-header';

        const title = document.createElement('button');
        title.className = 'pf-queue-title';
        title.type = 'button';
        title.setAttribute(
            'aria-label',
            `${(job.state === 'success' || job.state === 'transcribing') && job.outputPath ? 'Open folder for' : 'Select'} ${job.label || job.url}`
        );
        const platform = detectPlatform(job.url || '');
        if (platform) {
            const platformIcon = document.createElement('span');
            platformIcon.className = `pf-queue-platform-icon pf-platform-${platform}`;
            const icon = getPlatformIconElement(platform);
            if (icon) {
                platformIcon.appendChild(icon);
                title.appendChild(platformIcon);
            }
        }
        const titleText = document.createElement('span');
        titleText.className = 'pf-queue-title-text';
        titleText.textContent = job.label || job.url;
        title.appendChild(titleText);

        const badge = document.createElement('div');
        badge.className = 'pf-badge pf-badge-muted pf-queue-badge';
        badge.textContent = ({ success: 'Completed', error: 'Failed', transcribing: 'Transcribing' })[job.state] || job.state || 'queued';

        const moreBtn = document.createElement('button');
        moreBtn.className = 'pf-icon-btn pf-queue-more-btn';
        moreBtn.type = 'button';
        moreBtn.textContent = '⋯';
        moreBtn.setAttribute('aria-label', `More actions for ${job.label || job.url}`);
        moreBtn.onclick = event => {
            event.stopPropagation();
            const rect = moreBtn.getBoundingClientRect();
            state.selectedId = job.id;
            scheduleQueueRender();
            openQueueContextMenu(job, rect.right, rect.bottom);
        };

        header.append(title, badge, moreBtn);

        const progress = document.createElement('div');
        progress.className = 'pf-progress';
        const isDownloading = job.state === 'downloading';
        const isProcessing = job.state === 'transcribing' || job.state === 'cancelling';
        progress.hidden = !isDownloading && !isProcessing && job.state !== 'success';
        progress.classList.toggle('pf-progress-indeterminate', isProcessing);
        progress.setAttribute('role', 'progressbar');
        progress.setAttribute('aria-label', isProcessing ? (job.state === 'transcribing' ? 'Transcribing' : 'Cancelling') : 'Download progress');
        if (!isProcessing) progress.setAttribute('aria-valuenow', String(Math.round(job.state === 'success' ? 100 : job.percent || 0)));
        progress.setAttribute('aria-valuemin', '0');
        progress.setAttribute('aria-valuemax', '100');
        const bar = document.createElement('span');
        bar.className = 'pf-progress-bar';
        bar.style.width = `${isProcessing ? 35 : job.state === 'success' ? 100 : job.percent || 0}%`;
        progress.appendChild(bar);

        const meta = document.createElement('div');
        meta.className = 'pf-queue-meta';
        appendTextSpans(meta, getQueueMetaItems(job));
        const main = document.createElement('div');
        main.className = 'pf-list-card-layout pf-queue-main';

        const content = document.createElement('div');
        content.className = 'pf-queue-content';
        content.append(header, progress, meta);
        if (job.clearError || job.error) {
            const errorText = document.createElement('p');
            const isHistoryWarning = job.state === 'success' && !job.clearError;
            errorText.className = `pf-status ${isHistoryWarning ? 'pf-queue-warning' : 'pf-status-error'} pf-queue-error`;
            errorText.textContent = job.clearError || (job.state === 'success' ? `History warning: ${job.error}` : job.error);
            content.appendChild(errorText);
        }
        main.appendChild(content);

        const thumbUrl = job.thumbnail || resolveYouTubeThumbnail(job.url);
        if (thumbUrl) {
            const thumb = document.createElement('div');
            thumb.className = 'pf-media-thumbnail pf-queue-thumb';
            thumb.style.backgroundImage = `url('${thumbUrl}')`;
            main.appendChild(thumb);
        } else {
            main.classList.add('pf-no-media');
        }

        item.append(main);
        els.queueList.appendChild(item);
        queueCardElements.set(job.id, item);
    });
    if (focusedJobId) {
        const restoredItem = Array.from(els.queueList.children).find(item => item.dataset.jobId === focusedJobId);
        restoredItem?.querySelector(focusedMoreButton ? '.pf-queue-more-btn' : '.pf-queue-title')?.focus({ preventScroll: true });
    }

    if (state.contextMenuJobId && !state.jobs.has(state.contextMenuJobId)) {
        hideQueueContextMenu();
    }
    syncQueueContextMenuState();
    const activeCount = items.filter(job => queueBusyJobStates.has(job.state)).length;
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
        const progress = item.querySelector('.pf-progress');
        const percent = Math.max(0, Math.min(100, Number(job.percent) || 0));
        progress.setAttribute('aria-valuenow', String(Math.round(percent)));
        progress.querySelector('.pf-progress-bar').style.width = `${percent}%`;
        const meta = item.querySelector('.pf-queue-meta');
        meta.replaceChildren();
        appendTextSpans(meta, getQueueMetaItems(job));
    }
};

const flushQueueRender = () => {
    if (state.activeView !== 'download' || queueRenderFrame !== null || (!queueRenderDirty && queueProgressDirtyIds.size === 0)) return;

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
    line.className = `pf-terminal-line pf-log-line ${isError ? 'pf-status-error' : ''}`;
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
    const previous = state.jobs.get(id);
    const existing = previous || { id, createdAt: Date.now() };
    state.jobs.set(id, { ...existing, ...patch });
    const changedKeys = Object.keys(patch);
    if (previous && changedKeys.length === 1 && changedKeys[0] === 'previewLoading') return;
    const progressOnly = previous && changedKeys.length > 0 &&
        changedKeys.every(key => key === 'percent' || key === 'speed' || key === 'eta');
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
                const info = await invoke('load_info', { url: job.url });
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
    if (!invoke) return;
    const job = state.jobs.get(id);
    if (!job?.url || job.thumbnail || job.previewResolved || job.previewLoading) return;
    updateJob(id, { previewLoading: true });
    thumbnailHydrationQueue.push(id);
    drainThumbnailHydrationQueue();
};

const syncQueueStatus = async () => {
    if (!invoke) return;
    try {
        const status = await invoke('get_queue_status');
        state.queueAutoStartEnabled = status?.auto_start ?? true;
        state.queueWorkerRunning = Boolean(status?.worker_running);
        state.queuePaused = Boolean(status?.paused);
        renderQueueControls();
        return status;
    } catch (err) {
        appendLog(`[queue] ${err}`, true);
    }
};

const browserImport = createBrowserImportView({ els, invoke, appendLog, appendTextSpans });
const {
    applyLinkDumpServerStatus,
    syncLinkDumpOverview,
} = browserImport;

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
    els.loadInfoBtn.classList.add('pf-btn-loading');
    els.loadInfoBtn.disabled = true;
    setInfoBadge('Loading...');
    try {
        const info = await invoke('load_info', { url: requestUrl });
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
        els.loadInfoBtn.classList.remove('pf-btn-loading');
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
    if (!invoke) return null;
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
    const timestampForRequest = hasLoadedInfo ? state.info?.timestamp ?? null : null;
    const durationSecondsForRequest = hasLoadedInfo ? state.info?.duration ?? null : null;

    try {
        await settings.waitForPendingSave();
        const id = await invoke('enqueue_download', {
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
        await invoke('cancel_download', { id: job.id });
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
    if (!invoke) return;
    try {
        const status = await invoke('set_queue_auto_start', {
            enabled: !state.queueAutoStartEnabled,
        });
        state.queueAutoStartEnabled = status?.auto_start ?? !state.queueAutoStartEnabled;
        state.queueWorkerRunning = Boolean(status?.worker_running);
        state.queuePaused = Boolean(status?.paused);
        renderQueueControls();
    } catch (err) {
        appendLog(`[queue] ${err}`, true);
    }
};

const startQueueProcessing = async () => {
    if (!invoke) return;
    try {
        const status = await invoke('start_queue');
        state.queueAutoStartEnabled = status?.auto_start ?? state.queueAutoStartEnabled;
        state.queueWorkerRunning = Boolean(status?.worker_running);
        state.queuePaused = Boolean(status?.paused);
        renderQueueControls();
    } catch (err) {
        appendLog(`[queue] ${err}`, true);
    }
};

const toggleQueuePause = async () => {
    if (!invoke) return;
    try {
        const status = await invoke(state.queuePaused ? 'resume_queue' : 'pause_queue');
        state.queueAutoStartEnabled = status?.auto_start ?? state.queueAutoStartEnabled;
        state.queueWorkerRunning = Boolean(status?.worker_running);
        state.queuePaused = Boolean(status?.paused);
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
    els.txtImportStatus.classList.toggle('pf-status-error', Boolean(message && isError));
    els.txtImportStatus.classList.toggle('pf-status-success', Boolean(message && !isError));
};

const setTxtImportBusy = isBusy => {
    if (!els.importTxtBtn) return;
    els.importTxtBtn.disabled = isBusy;
    els.importTxtBtn.textContent = isBusy ? 'Importing...' : 'Import TXT';
};

const importTxtLinks = async () => {
    if (!invoke) {
        setTxtImportStatus('TXT import is only available in the Tauri app.', true);
        return;
    }

    setTxtImportStatus('');
    setTxtImportBusy(true);
    try {
        const file = await invoke('pick_txt_file');
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
        const results = invoke
            ? await Promise.allSettled(ids.map(id => invoke('cancel_download', { id })))
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

        if (cancelFailed && invoke) {
            try {
                const queue = await invoke('get_queue');
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
    els, invoke, appendLog, normalizePresetKey, getSelectedPresetKey, updateDownloadOptionHints,
    isActive: () => state.activeView === 'settings',
});

const historyView = createHistoryView({
    els, invoke, appendLog, formatFileSize, formatDuration, detectPlatform, appendTextSpans,
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
    els.presetSelect.addEventListener('change', () => {
        void settings.persistSelectedPresetKey();
        updateDownloadOptionHints();
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
            els.loadInfoBtn.classList.remove('pf-btn-loading');
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
    els.queueContextMenu.addEventListener('click', async event => {
        const button = event.target.closest('button[data-action]');
        if (!button) return;

        const job = getContextMenuJob();
        hideQueueContextMenu();
        if (!job) return;

        if (button.dataset.action === 'copy-link') {
            try {
                await navigator.clipboard.writeText(job.url || '');
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
                await invoke('cancel_download', { id: job.id });
            } catch (err) {
                appendLog(`[cancel] ${err}`, true);
            }
            return;
        }

        if (button.dataset.action === 'remove') {
            await removeJobFromQueue(job);
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
        if (!target?.closest('.pf-queue-item')) hideQueueContextMenu();
    });
    document.addEventListener('keydown', event => {
        if (event.key !== 'Escape' || els.queueContextMenu.hidden) return;
        const jobId = state.contextMenuJobId;
        hideQueueContextMenu();
        Array.from(els.queueList.children)
            .find(item => item.dataset.jobId === jobId)
            ?.querySelector('.pf-queue-more-btn')
            ?.focus({ preventScroll: true });
    });
    window.addEventListener('resize', hideQueueContextMenu);
    window.addEventListener('blur', hideQueueContextMenu);
    els.queueList.addEventListener('scroll', hideQueueContextMenu, { passive: true });

    els.copyLogsBtn.addEventListener('click', async () => {
        try {
            await navigator.clipboard.writeText(state.logs.map(entry => entry.text).join('\n'));
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
    await listen('link-dump:server-status', event => {
        applyLinkDumpServerStatus(event.payload);
    });

    await listen('history:changed', historyView.onChanged);

    await listen('queue:status', event => {
        state.queueAutoStartEnabled = event.payload?.auto_start ?? true;
        state.queueWorkerRunning = Boolean(event.payload?.worker_running);
        state.queuePaused = Boolean(event.payload?.paused);
        renderQueueControls();
    });

    await listen('queue:update', event => {
        applyQueueSnapshot(event.payload);
    });

    await listen('download:state', event => {
        const { id, state: status, output_path, exit_code, error } = event.payload;
        if (state.suppressedJobIds.has(id)) return;
        if (state.pendingClearAfterTerminal.has(id) && ['success', 'error', 'cancelled'].includes(status)) {
            state.pendingClearAfterTerminal.delete(id);
            state.suppressedJobIds.add(id);
            state.jobs.delete(id);
            state.queueIds = state.queueIds.filter(queuedId => queuedId !== id);
            scheduleQueueRender();
            return;
        }
        const patch = { state: status };
        if (error) patch.error = error;
        if (output_path) patch.outputPath = output_path;
        if (status === 'success') {
            patch.percent = 100;
            patch.speed = 'done';
            patch.eta = '-';
        }
        updateJob(id, patch);
        if (error) appendLog(`[${id}] ${error} (${exit_code ?? '?'})`, true);
    });

    await listen('download:progress', event => {
        const { id, percent, speed, eta } = event.payload;
        if (state.suppressedJobIds.has(id)) return;
        updateJob(id, {
            percent: percent ?? 0,
            speed: speed || '-',
            eta: eta || '-',
        });
    });

    await listen('download:log', event => {
        const { id, line, is_error } = event.payload;
        if (state.suppressedJobIds.has(id)) return;
        appendLog(`[${id}] ${line}`, is_error);
    });
};

const init = async () => {
    els.presetSelect.disabled = true;
    els.startDownloadBtn.disabled = true;
    els.importTxtBtn.disabled = true;
    const loadingFormat = document.createElement('option');
    loadingFormat.textContent = 'Loading formats...';
    els.presetSelect.replaceChildren(loadingFormat);
    settings.syncMagicImportTriggerState();
    renderQueueControls();
    bindEvents();
    setActiveView('download');
    requestAnimationFrame(() => {
        els.urlInput.focus();
    });
    if (!invoke || !listen) {
        appendLog('[tauri] API not available. Start the app with `npm run dev` (Tauri), not in a browser.', true);
        return;
    }
    try {
        await loadDownloadPresets();
        els.presetSelect.disabled = false;
        els.startDownloadBtn.disabled = false;
        els.importTxtBtn.disabled = false;
    } catch (err) {
        setInfoBadge('Formats unavailable');
        setTxtImportStatus(`Could not load download formats: ${err}`, true);
        appendLog(`[presets] ${err}`, true);
    }
    await settings.sync();
    await syncQueueStatus();
    await bindBackendEvents();
    try {
        await invoke('initialize_cli');
    } catch (err) {
        appendLog(`[cli] ${err}`, true);
    }
    scheduleQueueRender();
};

init();
