const tauriGlobal = window.__TAURI__;
const invoke = tauriGlobal?.tauri?.invoke;
const listen = tauriGlobal?.event?.listen;
const shellOpen = tauriGlobal?.shell?.open;
const state = Object.seal({
    jobs: new Map(),
    queueIds: [],
    queueAutoStartEnabled: true,
    queueWorkerRunning: false,
    suppressedJobIds: new Set(),
    selectedId: null,
    contextMenuJobId: null,
    logs: [],
    info: null,
    infoUrl: null,
    config: null,
    linkDump: null,
    generatedLinkDumpSecret: null,
    activeView: 'download',
    historyOffset: 0,
    historyHasMore: false,
    historyLoading: false,
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
    saveSettingsBtn: document.getElementById('saveSettingsBtn'),
    openFolderBtn: document.getElementById('openFolderBtn'),
    outputDir: document.getElementById('outputDir'),
    ytDlpPath: document.getElementById('ytDlpPath'),
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
    infoTitle: document.getElementById('infoTitle'),
    infoUploader: document.getElementById('infoUploader'),
    infoDuration: document.getElementById('infoDuration'),
    infoThumb: document.getElementById('infoThumb'),
    queueList: document.getElementById('queueList'),
    queueBadge: document.getElementById('queueBadge'),
    queueAutoStartBtn: document.getElementById('queueAutoStartBtn'),
    startQueueBtn: document.getElementById('startQueueBtn'),
    queueModeHint: document.getElementById('queueModeHint'),
    clearQueueBtn: document.getElementById('clearQueueBtn'),
    infoBadge: document.getElementById('infoBadge'),
    logBody: document.getElementById('logBody'),
    copyLogsBtn: document.getElementById('copyLogsBtn'),
    leftPanelTitle: document.getElementById('leftPanelTitle'),
    rightPanelTitle: document.getElementById('rightPanelTitle'),
    downloadView: document.getElementById('downloadView'),
    historyView: document.getElementById('historyView'),
    historyList: document.getElementById('historyList'),
    historyHint: document.getElementById('historyHint'),
    settingsView: document.getElementById('settingsView'),
    linkDumpView: document.getElementById('linkDumpView'),
    queueProgressView: document.getElementById('queueProgressView'),
    settingsSideView: document.getElementById('settingsSideView'),
    linkDumpSideView: document.getElementById('linkDumpSideView'),
    viewDownloadBtn: document.getElementById('viewDownloadBtn'),
    viewHistoryBtn: document.getElementById('viewHistoryBtn'),
    viewLinkDumpBtn: document.getElementById('viewLinkDumpBtn'),
    viewSettingsBtn: document.getElementById('viewSettingsBtn'),
    linkDumpExtensionRepoLink: document.getElementById('linkDumpExtensionRepoLink'),
    loadMoreHistoryBtn: document.getElementById('loadMoreHistoryBtn'),
    clearHistoryBtn: document.getElementById('clearHistoryBtn'),
    queueContextMenu: document.getElementById('queueContextMenu'),
    queueContextDownloads: document.getElementById('queueContextDownloads'),
    queueContextCancelBtn: document.getElementById('queueContextCancelBtn'),
    queueContextRemoveBtn: document.getElementById('queueContextRemoveBtn'),
});
const ytDlpLatestReleaseUrl = 'https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest';
const linkDumpExtensionRepoUrl = 'https://github.com/oliverjessner/PineFetch-Link-Dump';
const presetOptions = Object.freeze([
    {
        key: 'best',
        selectLabel: 'Best (bestvideo+bestaudio)',
        queueLabel: 'Best',
        menuLabel: 'Download Best',
        format: 'bestvideo+bestaudio/best',
        extractAudio: false,
        audioFormat: null,
        transcribeText: false,
        filenameSuffix: '_best',
    },
    {
        key: '1080',
        selectLabel: 'Max 1080p',
        queueLabel: 'Max 1080p',
        menuLabel: 'Download Max 1080p',
        format: 'bv*[height<=1080]+ba/b[height<=1080]',
        extractAudio: false,
        audioFormat: null,
        transcribeText: false,
        filenameSuffix: '__max',
    },
    {
        key: 'audio_mp3',
        selectLabel: 'Audio only (mp3)',
        queueLabel: 'Audio only (mp3)',
        menuLabel: 'Download Audio only (mp3)',
        format: 'ba/b',
        extractAudio: true,
        audioFormat: 'mp3',
        transcribeText: false,
        filenameSuffix: null,
    },
    {
        key: 'audio_opus',
        selectLabel: 'Audio only (opus)',
        queueLabel: 'Audio only (opus)',
        menuLabel: 'Download Audio only (opus)',
        format: 'ba/b',
        extractAudio: true,
        audioFormat: 'opus',
        transcribeText: false,
        filenameSuffix: null,
    },
    {
        key: 'text',
        selectLabel: 'Text (fast-whisper)',
        queueLabel: 'Text (fast-whisper)',
        menuLabel: 'Download Text (fast-whisper)',
        format: 'ba/b',
        extractAudio: true,
        audioFormat: 'mp3',
        transcribeText: true,
        filenameSuffix: null,
    },
]);
const presets = Object.freeze(Object.fromEntries(presetOptions.map(preset => [preset.key, preset])));
const normalizePresetKey = key => (presets[key] ? key : presetOptions[0]?.key || 'best');
const getSelectedPresetKey = () => normalizePresetKey(els.presetSelect.value);
const findPresetForDownloadJob = job =>
    presetOptions.find(
        preset =>
            job?.format === preset.format &&
            Boolean(job?.extract_audio) === preset.extractAudio &&
            (job?.audio_format ?? null) === (preset.audioFormat ?? null) &&
            Boolean(job?.transcribe_text) === preset.transcribeText &&
            (job?.filename_suffix ?? null) === (preset.filenameSuffix ?? null)
    ) || null;
const defaultYtDlpPath = '/opt/homebrew/bin/yt-dlp';
const historyPageSize = 50;
const cancellableJobStates = new Set(['downloading', 'transcribing']);
const queueBusyJobStates = new Set(['downloading', 'transcribing', 'cancelling']);
const removableJobStates = new Set(['queued', 'success', 'error', 'cancelled']);
let urlShakeTimer = null;
let magicImportInFlight = false;

const formatDuration = seconds => {
    if (!seconds && seconds !== 0) return '-';

    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    const hrs = Math.floor(mins / 60);

    if (hrs > 0) return `${hrs}h ${String(mins % 60).padStart(2, '0')}m`;
    return `${mins}m ${String(secs).padStart(2, '0')}s`;
};

const formatCutStartLabel = seconds => {
    if (!Number.isFinite(Number(seconds)) || Number(seconds) <= 0) return null;
    return `from ${formatDuration(Number(seconds))}`;
};

const timestampParamNames = new Set(['t', 'start', 'start_time', 'time_continue']);

const normalizePositiveTimestamp = seconds => {
    const value = Number(seconds);
    return Number.isFinite(value) && value > 0 ? value : null;
};

const parseTimestampValue = raw => {
    const value = `${raw || ''}`.trim().toLowerCase();
    if (!value) return null;

    const numeric = Number(value);
    if (Number.isFinite(numeric)) return normalizePositiveTimestamp(numeric);

    if (value.includes(':')) {
        const parts = value.split(':');
        if (parts.length < 2 || parts.length > 3) return null;

        let total = 0;
        for (const part of parts) {
            if (!part) return null;
            const amount = Number(part);
            if (!Number.isFinite(amount) || amount < 0) return null;
            total = total * 60 + amount;
        }
        return normalizePositiveTimestamp(total);
    }

    const matches = [...value.matchAll(/(\d+(?:\.\d+)?)([hms])/g)];
    if (!matches.length || matches.map(match => match[0]).join('') !== value) return null;

    const total = matches.reduce((sum, [, amount, unit]) => {
        const multiplier = unit === 'h' ? 3600 : unit === 'm' ? 60 : 1;
        return sum + Number(amount) * multiplier;
    }, 0);
    return normalizePositiveTimestamp(total);
};

const extractUrlStartTimestamp = url => {
    try {
        const parsed = new URL(url);
        for (const [name, value] of parsed.searchParams.entries()) {
            if (!timestampParamNames.has(name)) continue;
            const seconds = parseTimestampValue(value);
            if (seconds) return seconds;
        }

        if (parsed.hash) {
            const fragment = parsed.hash.slice(1);
            const fragmentParams = new URLSearchParams(fragment);
            for (const [name, value] of fragmentParams.entries()) {
                if (!timestampParamNames.has(name)) continue;
                const seconds = parseTimestampValue(value);
                if (seconds) return seconds;
            }
            return parseTimestampValue(fragment);
        }
    } catch {
        return null;
    }
    return null;
};

const detectPlatform = url => {
    try {
        const host = new URL(url).hostname.replace(/^www\./, '').toLowerCase();

        if (host === 'youtu.be' || host.endsWith('youtube.com')) return 'youtube';
        if (host.endsWith('facebook.com') || host === 'fb.watch') return 'facebook';
        if (host.endsWith('twitch.tv')) return 'twitch';
        if (host === 'x.com' || host.endsWith('.x.com') || host.endsWith('twitter.com')) return 'x';
        if (host.endsWith('tiktok.com')) return 'tiktok';
        if (host.endsWith('instagram.com') || host.endsWith('instagr.am')) return 'instagram';
    } catch {
        return null;
    }
    return null;
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

const isValidHttpUrl = value => {
    try {
        const parsed = new URL(value);
        return parsed.protocol === 'http:' || parsed.protocol === 'https:';
    } catch {
        return false;
    }
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

const isMagicImportEnabled = () => Boolean(els.magicImportEnabled?.checked);

const syncMagicImportTriggerState = () => {
    const enabled = isMagicImportEnabled();
    els.magicImportTrigger.setAttribute('aria-disabled', String(!enabled));
    els.magicImportTrigger.title = enabled ? 'Magic import from clipboard' : 'Magic import is disabled in Settings';
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
    if (!isMagicImportEnabled()) return;
    if (state.activeView !== 'download') return;
    if (els.urlInput.value.trim()) return;

    magicImportInFlight = true;
    try {
        const clipboardText = (await readClipboardText()).trim();
        if (!isValidHttpUrl(clipboardText)) return;
        const platform = detectPlatform(clipboardText);
        if (!platform) return;
        const lastDownloadedUrl = `${state.config?.last_download_url || ''}`.trim();
        if (lastDownloadedUrl && clipboardText === lastDownloadedUrl) return;

        els.urlInput.value = clipboardText;
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

const cacheLastDownloadedUrl = async url => {
    const nextUrl = url.trim();
    if (!nextUrl) return;

    state.config = {
        ...(state.config || {}),
        last_download_url: nextUrl,
    };

    if (!invoke) return;
    try {
        await invoke('cache_last_download_url', { url: nextUrl });
    } catch (err) {
        appendLog(`[config] ${err}`, true);
    }
};

const normalizeHostname = hostname => `${hostname || ''}`.replace(/\.$/, '').toLowerCase();

const isYouTubeHostname = hostname => {
    const host = normalizeHostname(hostname).replace(/^www\./, '');
    return host === 'youtu.be' || host === 'youtube.com' || host.endsWith('.youtube.com');
};

const getYouTubeVideoIdFromParsedUrl = parsed => {
    const host = normalizeHostname(parsed.hostname).replace(/^www\./, '');
    const pathParts = parsed.pathname.split('/').filter(Boolean);

    if (host === 'youtu.be') return pathParts[0] || null;
    if (host !== 'youtube.com' && !host.endsWith('.youtube.com')) return null;

    const route = (pathParts[0] || '').toLowerCase();
    if (route === 'watch') return parsed.searchParams.get('v')?.trim() || null;
    if (route === 'shorts' || route === 'embed' || route === 'v' || route === 'live') {
        return pathParts[1] || null;
    }

    return null;
};

const extractYouTubeVideoId = url => {
    try {
        const parsed = new URL(url);
        if (!isYouTubeHostname(parsed.hostname)) return null;
        return getYouTubeVideoIdFromParsedUrl(parsed);
    } catch {
        return null;
    }
};

const resolveYouTubeThumbnail = url => {
    const videoId = extractYouTubeVideoId(url);
    return videoId ? `https://i.ytimg.com/vi/${videoId}/mqdefault.jpg` : null;
};

const getYouTubeImportTimestampKey = url => {
    const seconds = extractUrlStartTimestamp(url);
    return Number.isFinite(Number(seconds)) && Number(seconds) > 0 ? `${Number(seconds)}` : '';
};

const normalizeYouTubeUrl = value => {
    const trimmed = `${value || ''}`.trim();
    if (!trimmed) return null;

    try {
        const parsed = new URL(trimmed);
        if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return null;
        if (!isYouTubeHostname(parsed.hostname)) return null;

        const videoId = getYouTubeVideoIdFromParsedUrl(parsed);
        if (!videoId) return null;

        parsed.hostname = normalizeHostname(parsed.hostname);
        const url = parsed.toString();
        const timestampKey = getYouTubeImportTimestampKey(url);
        return {
            url,
            key: `youtube:${videoId}:${timestampKey}`,
        };
    } catch {
        return null;
    }
};

const parseTxtImportLinks = content => {
    const rawContent = `${content || ''}`;
    const seenKeys = new Set();
    const result = {
        items: [],
        invalidCount: 0,
        duplicateCount: 0,
        ignoredCount: 0,
        isEmpty: rawContent.trim().length === 0,
    };

    rawContent.split(/\r\n|\n|\r/).forEach(rawLine => {
        const line = rawLine.trim();
        if (!line || line.startsWith('#')) {
            result.ignoredCount += 1;
            return;
        }

        const normalized = normalizeYouTubeUrl(line);
        if (!normalized) {
            result.invalidCount += 1;
            return;
        }

        if (seenKeys.has(normalized.key)) {
            result.duplicateCount += 1;
            return;
        }

        seenKeys.add(normalized.key);
        result.items.push(normalized);
    });

    return result;
};

const getQueuedYouTubeImportKeys = () => {
    const keys = new Set();
    state.jobs.forEach(job => {
        const normalized = normalizeYouTubeUrl(job.url);
        if (normalized) keys.add(normalized.key);
    });
    return keys;
};

const setInfoBadge = text => {
    els.infoBadge.textContent = text;
};

const parseLatestYtDlpVersion = payload => {
    if (!payload || typeof payload.tag_name !== 'string') return null;
    const value = payload.tag_name.trim();
    if (!value) return null;
    return value.startsWith('v') ? value.slice(1) : value;
};

const fetchLatestYtDlpVersion = async () => {
    const response = await fetch(ytDlpLatestReleaseUrl, {
        headers: {
            Accept: 'application/vnd.github+json',
        },
    });
    if (!response.ok) {
        throw new Error(`latest version request failed (${response.status})`);
    }
    const payload = await response.json();
    const latest = parseLatestYtDlpVersion(payload);
    if (!latest) {
        throw new Error('latest version missing from response');
    }
    return latest;
};

const refreshYtDlpVersions = async () => {
    if (!invoke) return;
    const path = els.ytDlpPath.value.trim() || null;
    els.ytDlpInstalledVersion.textContent = 'Installed: checking...';
    els.ytDlpInstalledVersion.removeAttribute('title');
    els.ytDlpLatestVersion.textContent = 'Latest: checking...';

    const [installedResult, latestResult] = await Promise.allSettled([
        invoke('get_yt_dlp_installed_version', { path }),
        fetchLatestYtDlpVersion(),
    ]);

    if (installedResult.status === 'fulfilled') {
        const installed = installedResult.value;
        els.ytDlpInstalledVersion.textContent = `Installed: ${installed.version}`;
        els.ytDlpInstalledVersion.title = installed.path;
    } else {
        els.ytDlpInstalledVersion.textContent = 'Installed: unavailable';
        const reason = `${installedResult.reason || ''}`.trim();
        if (reason) els.ytDlpInstalledVersion.title = reason;
    }

    if (latestResult.status === 'fulfilled') {
        els.ytDlpLatestVersion.textContent = `Latest: ${latestResult.value}`;
    } else {
        els.ytDlpLatestVersion.textContent = 'Latest: unavailable';
    }
};

const setActiveView = view => {
    const isDownload = view === 'download';
    const isHistory = view === 'history';
    const isLinkDump = view === 'linkDump';
    const isSettings = view === 'settings';
    state.activeView = view;

    els.downloadView.hidden = !isDownload;
    els.historyView.hidden = !isHistory;
    els.settingsView.hidden = !isSettings;
    els.linkDumpView.hidden = !isLinkDump;
    els.queueProgressView.hidden = isSettings || isLinkDump;
    els.settingsSideView.hidden = !isSettings;
    els.linkDumpSideView.hidden = !isLinkDump;
    els.downloadView.classList.toggle('pf-is-active', isDownload);
    els.historyView.classList.toggle('pf-is-active', isHistory);
    els.settingsView.classList.toggle('pf-is-active', isSettings);
    els.linkDumpView.classList.toggle('pf-is-active', isLinkDump);
    els.queueProgressView.classList.toggle('pf-is-active', !isSettings && !isLinkDump);
    els.settingsSideView.classList.toggle('pf-is-active', isSettings);
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
    } else if (isHistory) {
        els.leftPanelTitle.textContent = 'History';
        els.rightPanelTitle.textContent = 'Queue / Progress';
        els.queueBadge.style.display = 'inline-flex';
        els.infoBadge.style.display = 'none';
        void renderHistory();
    } else if (isLinkDump) {
        els.leftPanelTitle.textContent = 'Link Dump';
        els.rightPanelTitle.textContent = 'Connections';
        els.queueBadge.style.display = 'none';
        els.infoBadge.style.display = 'none';
        void syncLinkDumpOverview();
    } else {
        els.leftPanelTitle.textContent = 'Settings';
        els.rightPanelTitle.textContent = 'Options';
        els.queueBadge.style.display = 'none';
        els.infoBadge.style.display = 'none';
        void refreshYtDlpVersions();
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
        button.className = 'pf-queue-context-menu-btn';
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
    });
};

const renderInfo = () => {
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

const renderQueueControls = () => {
    const queuedCount = state.queueIds.length;
    const isBusy =
        state.queueWorkerRunning || Array.from(state.jobs.values()).some(job => queueBusyJobStates.has(job.state));
    const setQueueModeHint = text => {
        if (els.queueModeHint) {
            els.queueModeHint.textContent = text;
        }
    };

    els.queueAutoStartBtn.textContent = `Auto-start: ${state.queueAutoStartEnabled ? 'on' : 'off'}`;
    els.queueAutoStartBtn.setAttribute('aria-pressed', String(state.queueAutoStartEnabled));

    els.startQueueBtn.disabled = state.queueAutoStartEnabled || queuedCount === 0 || isBusy;

    if (state.queueAutoStartEnabled) {
        setQueueModeHint('New items start downloading as soon as they are queued.');
        return;
    }

    if (isBusy) {
        setQueueModeHint('Manual mode is active. The current queue run will finish before new items wait.');
        return;
    }

    if (queuedCount > 0) {
        setQueueModeHint('Manual mode is active. Build the queue first, then click Start queue.');
        return;
    }

    setQueueModeHint('Manual mode is active. New items stay queued until you click Start queue.');
};

const renderQueue = () => {
    const items = Array.from(state.jobs.values()).sort((a, b) => a.createdAt - b.createdAt);
    els.queueList.replaceChildren();
    items.forEach(job => {
        const item = document.createElement('div');
        item.className = `pf-queue-item ${job.id === state.selectedId ? 'pf-is-active' : ''}`;
        item.oncontextmenu = event => {
            event.preventDefault();
            state.selectedId = job.id;
            renderQueue();
            openQueueContextMenu(job, event.clientX, event.clientY);
        };
        item.onclick = async () => {
            hideQueueContextMenu();
            state.selectedId = job.id;
            renderQueue();
            if (job.state === 'success' && job.outputPath && invoke) {
                try {
                    await invoke('open_folder', { path: job.outputPath });
                } catch (err) {
                    appendLog(`[open] ${err}`, true);
                }
            }
        };

        const header = document.createElement('div');
        header.className = 'pf-queue-header';

        const title = document.createElement('div');
        title.className = 'pf-queue-title';
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
        badge.textContent = job.state || 'queued';

        header.append(title, badge);

        const progress = document.createElement('div');
        progress.className = 'pf-progress';
        const bar = document.createElement('span');
        bar.style.width = `${job.percent || 0}%`;
        progress.appendChild(bar);

        const meta = document.createElement('div');
        meta.className = 'pf-queue-meta';
        const metaItems = [job.speed || '-', job.eta || '-', job.formatLabel || ''];
        const cutStartLabel = formatCutStartLabel(job.cutStartTime);
        if (cutStartLabel) metaItems.push(cutStartLabel);
        appendTextSpans(meta, metaItems);
        const main = document.createElement('div');
        main.className = 'pf-queue-main';

        const content = document.createElement('div');
        content.className = 'pf-queue-content';
        content.append(header, progress, meta);
        main.appendChild(content);

        const thumbUrl = job.thumbnail || resolveYouTubeThumbnail(job.url);
        if (thumbUrl) {
            const thumb = document.createElement('div');
            thumb.className = 'pf-queue-thumb';
            thumb.style.backgroundImage = `url('${thumbUrl}')`;
            main.appendChild(thumb);
        } else {
            main.classList.add('pf-no-thumb');
        }

        item.append(main);
        els.queueList.appendChild(item);
    });

    if (state.contextMenuJobId && !state.jobs.has(state.contextMenuJobId)) {
        hideQueueContextMenu();
    }
    syncQueueContextMenuState();
    els.queueBadge.textContent = `${state.queueIds.length} queued`;
    renderQueueControls();
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
    els.clearHistoryBtn.disabled = isLoading;
    els.loadMoreHistoryBtn.textContent = isLoading ? 'Loading...' : 'Load more';
};

const updateHistoryActions = () => {
    els.loadMoreHistoryBtn.hidden = !state.historyHasMore;
};

const createHistoryItem = entry => {
    const item = document.createElement('div');
    item.className = `pf-history-item ${entry.thumbnail ? '' : 'pf-no-thumb'}`;

    item.onclick = async () => {
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
    title.textContent = entry.title || entry.filename || entry.url;
    content.appendChild(title);

    const meta = document.createElement('div');
    meta.className = 'pf-history-meta';
    const dateStr = formatHistoryDate(entry.completed_at);
    const platform = entry.platform || detectPlatform(entry.url) || 'unknown';
    const uploadDate = formatUploadTimestamp(entry.timestamp) || formatUploadDate(entry.upload_date);
    appendTextSpans(meta, [
        platform,
        entry.filename || '',
        uploadDate ? `uploaded ${uploadDate}` : '',
        dateStr,
    ]);
    content.appendChild(meta);

    item.appendChild(content);

    if (entry.thumbnail) {
        const thumb = document.createElement('div');
        thumb.className = 'pf-history-thumb';
        thumb.style.backgroundImage = `url('${entry.thumbnail}')`;
        item.appendChild(thumb);
    }

    const removeBtn = document.createElement('button');
    removeBtn.className = 'pf-history-item-remove-btn';
    removeBtn.textContent = '×';
    removeBtn.title = 'Remove from history';
    removeBtn.onclick = async event => {
        event.stopPropagation();
        try {
            await invoke('remove_history_entry', { id: entry.id });
            void renderHistory();
        } catch (err) {
            appendLog(`[history] ${err}`, true);
        }
    };
    item.appendChild(removeBtn);

    return item;
};

const renderHistory = async ({ append = false } = {}) => {
    if (!invoke) {
        els.historyList.replaceChildren();
        els.historyHint.hidden = true;
        state.historyHasMore = false;
        updateHistoryActions();
        return;
    }

    if (state.historyLoading) return;

    const offset = append ? state.historyOffset : 0;
    if (!append) {
        state.historyOffset = 0;
        state.historyHasMore = false;
        els.historyList.replaceChildren();
        els.historyHint.hidden = true;
        updateHistoryActions();
    }

    setHistoryLoading(true);

    try {
        const page = await invoke('get_history', { limit: historyPageSize, offset });
        const entries = Array.isArray(page) ? page : page?.entries || [];
        const hasMore = Array.isArray(page)
            ? entries.length === historyPageSize
            : Boolean(page?.has_more ?? page?.hasMore);

        if (!append && entries.length === 0) {
            els.historyHint.hidden = false;
            state.historyOffset = 0;
            state.historyHasMore = false;
            return;
        }

        els.historyHint.hidden = true;

        entries.forEach(entry => {
            els.historyList.appendChild(createHistoryItem(entry));
        });
        state.historyOffset = offset + entries.length;
        state.historyHasMore = hasMore;
    } catch (err) {
        appendLog(`[history] ${err}`, true);
    } finally {
        setHistoryLoading(false);
        updateHistoryActions();
    }
};

const appendLog = (text, isError) => {
    state.logs.push(text);
    const line = document.createElement('div');
    line.className = `pf-log-line ${isError ? 'pf-status-error' : ''}`;
    line.textContent = text;
    els.logBody.appendChild(line);
    els.logBody.scrollTop = els.logBody.scrollHeight;
};

const updateJob = (id, patch) => {
    const existing = state.jobs.get(id) || { id, createdAt: Date.now() };
    state.jobs.set(id, { ...existing, ...patch });
    renderQueue();
};

const maybeHydrateQueueThumbnail = id => {
    if (!invoke) return;
    const job = state.jobs.get(id);
    if (!job || !job.url) return;
    if (job.thumbnail || job.previewResolved || job.previewLoading) return;

    updateJob(id, { previewLoading: true });
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
        }
    })();
};

const syncConfig = async () => {
    try {
        state.config = await invoke('get_config');
        els.outputDir.value = state.config.default_output_dir || '';
        els.ytDlpPath.value = state.config.yt_dlp_path || defaultYtDlpPath;
        els.presetSelect.value = normalizePresetKey(state.config.selected_preset_key);
        els.magicImportEnabled.checked = state.config.magic_import_enabled ?? true;
        els.cutAtTimestampEnabled.checked = state.config.cut_at_timestamp_enabled ?? true;
        syncMagicImportTriggerState();
        void refreshYtDlpVersions();
    } catch (err) {
        appendLog(`[config] ${err}`, true);
    }
};

const persistSelectedPresetKey = async () => {
    const selectedPresetKey = getSelectedPresetKey();
    state.config = {
        ...(state.config || {}),
        selected_preset_key: selectedPresetKey,
    };

    if (!invoke) return;

    try {
        state.config = await invoke('set_selected_preset_key', { presetKey: selectedPresetKey });
    } catch (err) {
        appendLog(`[config] ${err}`, true);
    }
};

const syncQueueStatus = async () => {
    if (!invoke) return;
    try {
        const status = await invoke('get_queue_status');
        state.queueAutoStartEnabled = status?.auto_start ?? true;
        state.queueWorkerRunning = Boolean(status?.worker_running);
        renderQueueControls();
    } catch (err) {
        appendLog(`[queue] ${err}`, true);
    }
};

const formatDateTime = value => {
    if (!value) return '-';
    const date = new Date(`${value.replace(' ', 'T')}Z`);
    if (Number.isNaN(date.getTime())) return value;
    return date.toLocaleString([], {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
    });
};

const statusLabel = value => {
    const normalized = `${value || ''}`.trim().toLowerCase();
    return normalized ? normalized.charAt(0).toUpperCase() + normalized.slice(1) : 'Stopped';
};

const setLinkDumpStatusText = (message, isError = false) => {
    if (!els.linkDumpServerStatusText) return;
    els.linkDumpServerStatusText.textContent = message || '';
    els.linkDumpServerStatusText.classList.toggle('pf-status-error', Boolean(message && isError));
    els.linkDumpServerStatusText.classList.toggle('pf-status-success', Boolean(message && !isError));
};

const setLinkDumpSecretStatus = (message, isError = false) => {
    if (!els.linkDumpSecretStatus) return;
    els.linkDumpSecretStatus.textContent = message || '';
    els.linkDumpSecretStatus.classList.toggle('pf-status-error', Boolean(message && isError));
    els.linkDumpSecretStatus.classList.toggle('pf-status-success', Boolean(message && !isError));
};

const applyLinkDumpServerStatus = serverStatus => {
    if (!serverStatus || !els.linkDumpServerStatusBadge) return;
    const status = `${serverStatus.status || 'stopped'}`.toLowerCase();
    els.linkDumpServerStatusBadge.textContent = statusLabel(status);
    els.linkDumpServerStatusBadge.classList.toggle('pf-badge-danger', status === 'error');
    els.linkDumpServerStatusBadge.classList.toggle('pf-badge-warning', status === 'stopped');
    els.linkDumpServerStatusBadge.classList.toggle('pf-badge-muted', status !== 'running' && status !== 'error');
    els.linkDumpServerStatusBadge.classList.toggle('pf-badge', true);

    if (status === 'running') {
        setLinkDumpStatusText('Browser extensions can send YouTube links to this PineFetch instance.');
    } else if (status === 'error') {
        setLinkDumpStatusText(serverStatus.error_message || 'Link Dump Server could not start.', true);
    } else {
        setLinkDumpStatusText('Link Dump Server is stopped.', false);
    }
};

const renderLinkDumpSecrets = secrets => {
    if (!els.linkDumpSecretList) return;
    els.linkDumpSecretList.replaceChildren();
    const visibleSecrets = Array.isArray(secrets)
        ? secrets.filter(connection => `${connection.status || ''}`.toLowerCase() !== 'deleted')
        : [];
    els.linkDumpSecretHint.hidden = visibleSecrets.length > 0;

    visibleSecrets.forEach(connection => {
        const item = document.createElement('div');
        item.className = 'pf-link-dump-secret-item';

        const content = document.createElement('div');
        content.className = 'pf-link-dump-secret-content';

        const title = document.createElement('div');
        title.className = 'pf-link-dump-secret-title';
        title.textContent = connection.name || 'Link Dump Connection';

        const meta = document.createElement('div');
        meta.className = 'pf-link-dump-secret-meta';
        appendTextSpans(meta, [
            `Created ${formatDateTime(connection.created_at)}`,
            `Last used ${formatDateTime(connection.last_used_at)}`,
        ]);

        content.append(title, meta);

        const actions = document.createElement('div');
        actions.className = 'pf-row pf-link-dump-secret-actions';

        const badge = document.createElement('span');
        const status = `${connection.status || 'active'}`.toLowerCase();
        badge.className = `pf-badge ${
            status === 'active' ? '' : status === 'revoked' ? 'pf-badge-warning' : 'pf-badge-muted'
        }`;
        badge.textContent = statusLabel(status);
        actions.appendChild(badge);

        if (status === 'active') {
            const revokeBtn = document.createElement('button');
            revokeBtn.className = 'pf-btn pf-btn-ghost';
            revokeBtn.type = 'button';
            revokeBtn.textContent = 'Revoke';
            revokeBtn.onclick = () => {
                if (!window.confirm('Revoke this connection? Extensions using this secret will no longer be able to send links.')) {
                    return;
                }
                void revokeLinkDumpSecret(connection.id);
            };
            actions.appendChild(revokeBtn);
        }

        if (status !== 'deleted') {
            const deleteBtn = document.createElement('button');
            deleteBtn.className = 'pf-btn pf-btn-danger';
            deleteBtn.type = 'button';
            deleteBtn.textContent = 'Delete';
            deleteBtn.onclick = () => {
                if (!window.confirm('Delete this connection? Extensions using this secret will no longer be able to send links.')) {
                    return;
                }
                void deleteLinkDumpSecret(connection.id);
            };
            actions.appendChild(deleteBtn);
        }

        item.append(content, actions);
        els.linkDumpSecretList.appendChild(item);
    });
};

const renderLinkDumpOverview = overview => {
    state.linkDump = overview;
    const settings = overview?.settings || {};
    const serverStatus = overview?.server_status || {};
    if (els.linkDumpServerUrl) {
        els.linkDumpServerUrl.value =
            serverStatus.url || `http://${settings.host || '127.0.0.1'}:${settings.port || 2255}`;
    }
    if (els.linkDumpPort) {
        els.linkDumpPort.value = settings.port || 2255;
    }
    if (els.linkDumpServerEnabled) {
        els.linkDumpServerEnabled.checked = settings.server_enabled !== false;
    }
    applyLinkDumpServerStatus(serverStatus);
    renderLinkDumpSecrets(overview?.secrets || []);
};

const syncLinkDumpOverview = async () => {
    if (!invoke) return;
    try {
        renderLinkDumpOverview(await invoke('get_link_dump_overview'));
    } catch (err) {
        setLinkDumpStatusText(`Link Dump settings unavailable: ${err}`, true);
        appendLog(`[link-dump] ${err}`, true);
    }
};

const openLinkDumpExtensionRepo = async event => {
    event.preventDefault();
    const url = els.linkDumpExtensionRepoLink?.href || linkDumpExtensionRepoUrl;
    if (shellOpen) {
        try {
            await shellOpen(url);
            return;
        } catch (err) {
            appendLog(`[link-dump] Could not open extension repository: ${err}`, true);
        }
    }
    window.open(url, '_blank', 'noopener,noreferrer');
};

const saveLinkDumpServer = async () => {
    if (!invoke) return;
    const port = Number(els.linkDumpPort.value);
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
        setLinkDumpStatusText('Port must be between 1 and 65535.', true);
        return;
    }

    try {
        const overview = await invoke('update_link_dump_settings', {
            patch: {
                server_enabled: Boolean(els.linkDumpServerEnabled.checked),
                port,
            },
        });
        renderLinkDumpOverview(overview);
        appendLog('[link-dump] server settings saved', false);
    } catch (err) {
        setLinkDumpStatusText(`${err}`, true);
        appendLog(`[link-dump] ${err}`, true);
    }
};

const restartLinkDumpServer = async () => {
    if (!invoke) return;
    try {
        applyLinkDumpServerStatus(await invoke('restart_link_dump_server'));
        appendLog('[link-dump] server restarted', false);
    } catch (err) {
        setLinkDumpStatusText(`${err}`, true);
        appendLog(`[link-dump] ${err}`, true);
    }
};

const generateLinkDumpSecret = async () => {
    if (!invoke) return;
    try {
        const generated = await invoke('create_link_dump_secret', {
            name: els.linkDumpSecretName.value.trim() || null,
        });
        state.generatedLinkDumpSecret = generated.secret;
        els.generatedLinkDumpSecret.value = generated.secret;
        els.generatedLinkDumpSecretPanel.hidden = false;
        els.linkDumpSecretName.value = '';
        setLinkDumpSecretStatus('Secret generated.');
        renderLinkDumpOverview(await invoke('get_link_dump_overview'));
    } catch (err) {
        setLinkDumpSecretStatus(`${err}`, true);
        appendLog(`[link-dump] ${err}`, true);
    }
};

const copyGeneratedLinkDumpSecret = async () => {
    if (!state.generatedLinkDumpSecret) return;
    try {
        await navigator.clipboard.writeText(state.generatedLinkDumpSecret);
        state.generatedLinkDumpSecret = null;
        els.generatedLinkDumpSecret.value = '';
        els.generatedLinkDumpSecretPanel.hidden = true;
        setLinkDumpSecretStatus('Secret copied.');
    } catch (err) {
        setLinkDumpSecretStatus(`Copy failed: ${err}`, true);
        appendLog(`[copy] ${err}`, true);
    }
};

const revokeLinkDumpSecret = async id => {
    if (!invoke) return;
    try {
        const secrets = await invoke('revoke_link_dump_secret', { id });
        renderLinkDumpSecrets(secrets);
        setLinkDumpSecretStatus('Connection revoked.');
    } catch (err) {
        setLinkDumpSecretStatus(`${err}`, true);
        appendLog(`[link-dump] ${err}`, true);
    }
};

const deleteLinkDumpSecret = async id => {
    if (!invoke) return;
    try {
        const secrets = await invoke('delete_link_dump_secret', { id });
        renderLinkDumpSecrets(secrets);
        setLinkDumpSecretStatus('Connection deleted.');
    } catch (err) {
        setLinkDumpSecretStatus(`${err}`, true);
        appendLog(`[link-dump] ${err}`, true);
    }
};

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
    const thumbnailForRequest = hasLoadedInfo ? state.info?.thumbnail || null : (options.thumbnail ?? null);
    const uploadDateForRequest = hasLoadedInfo ? state.info?.upload_date || null : null;
    const timestampForRequest = hasLoadedInfo ? state.info?.timestamp ?? null : null;

    try {
        const id = await invoke('enqueue_download', {
            request: {
                url,
                format: preset.format,
                output_dir,
                extract_audio: preset.extractAudio,
                audio_format: preset.audioFormat,
                transcribe_text: preset.transcribeText,
                cut_at_timestamp_enabled: cutAtTimestampEnabled,
                cut_start_time: cutStartTime,
                filename_suffix: preset.filenameSuffix,
                title: titleForRequest,
                thumbnail: thumbnailForRequest,
                upload_date: uploadDateForRequest,
                timestamp: timestampForRequest,
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
        void cacheLastDownloadedUrl(url);
        maybeHydrateQueueThumbnail(id);

        if (!options.preserveComposerState) {
            els.urlInput.value = '';
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
    renderQueue();

    if (job.state !== 'queued') return;

    try {
        await invoke('cancel_download', { id: job.id });
    } catch (err) {
        if (!wasSuppressed) state.suppressedJobIds.delete(job.id);
        state.jobs.set(job.id, existingJob);
        state.queueIds = previousQueueIds;
        state.selectedId = previousSelectedId;
        renderQueue();
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
        renderQueueControls();
    } catch (err) {
        appendLog(`[queue] ${err}`, true);
    }
};

const saveSettings = async () => {
    const selectedPresetKey = getSelectedPresetKey();
    try {
        await invoke('set_config', {
            config: {
                yt_dlp_path: els.ytDlpPath.value.trim() || null,
                default_output_dir: els.outputDir.value.trim() || null,
                selected_preset_key: selectedPresetKey,
                magic_import_enabled: Boolean(els.magicImportEnabled.checked),
                cut_at_timestamp_enabled: Boolean(els.cutAtTimestampEnabled.checked),
                last_download_url: state.config?.last_download_url || null,
            },
        });
        state.config = {
            ...(state.config || {}),
            yt_dlp_path: els.ytDlpPath.value.trim() || null,
            default_output_dir: els.outputDir.value.trim() || null,
            selected_preset_key: selectedPresetKey,
            magic_import_enabled: Boolean(els.magicImportEnabled.checked),
            cut_at_timestamp_enabled: Boolean(els.cutAtTimestampEnabled.checked),
            last_download_url: state.config?.last_download_url || null,
        };
        syncMagicImportTriggerState();
        appendLog('[config] saved', false);
        void refreshYtDlpVersions();
    } catch (err) {
        appendLog(`[config] ${err}`, true);
    }
};

const pickDir = async () => {
    try {
        const result = await invoke('pick_output_dir');
        if (result) els.outputDir.value = result;
    } catch (err) {
        appendLog(`[dir] ${err}`, true);
    }
};

const openFolder = async () => {
    const path = els.outputDir.value.trim();
    if (!path) return;
    try {
        await invoke('open_folder', { path });
    } catch (err) {
        appendLog(`[open] ${err}`, true);
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
            const message = `No valid YouTube links found. Skipped ${pluralize(
                parsed.invalidCount,
                'invalid line'
            )} and ${pluralize(parsed.duplicateCount, 'duplicate')}.`;
            setTxtImportStatus(message, true);
            appendLog(`[txt-import] ${message}`, true);
            return;
        }

        const presetKey = getSelectedPresetKey();
        const queuedKeys = getQueuedYouTubeImportKeys();
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
                ? `No new YouTube links imported. ${skippedSummary}`
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

    idsToCancel.forEach(id => state.suppressedJobIds.add(id));

    if (invoke && idsToCancel.size > 0) {
        const results = await Promise.allSettled(Array.from(idsToCancel).map(id => invoke('cancel_download', { id })));
        results.forEach(result => {
            if (result.status === 'rejected') {
                const message = `${result.reason || ''}`.toLowerCase();
                if (!message.includes('job not found')) {
                    appendLog(`[clear] ${result.reason}`, true);
                }
            }
        });
    }

    state.jobs.clear();
    state.queueIds = [];
    state.selectedId = null;
    renderQueue();
};

const bindEvents = () => {
    els.magicImportTrigger.addEventListener('click', () => {
        void tryMagicImport();
    });
    els.magicImportTrigger.addEventListener('keydown', event => {
        if (event.key !== 'Enter' && event.key !== ' ') return;
        event.preventDefault();
        void tryMagicImport();
    });
    window.addEventListener('focus', () => {
        void tryMagicImport();
    });
    els.magicImportEnabled.addEventListener('change', syncMagicImportTriggerState);
    els.loadInfoBtn.addEventListener('click', loadInfo);
    els.startDownloadBtn.addEventListener('click', enqueueDownload);
    els.presetSelect.addEventListener('change', () => {
        void persistSelectedPresetKey();
    });
    els.importTxtBtn.addEventListener('click', () => {
        void importTxtLinks();
    });
    els.queueAutoStartBtn.addEventListener('click', () => {
        void toggleQueueAutoStart();
    });
    els.startQueueBtn.addEventListener('click', () => {
        void startQueueProcessing();
    });

    let urlInputDebounceTimer = null;
    els.urlInput.addEventListener('input', () => {
        const url = els.urlInput.value.trim();
        if (!url) {
            loadInfoRequestId += 1;
            loadInfoPending = false;
            state.info = null;
            state.infoUrl = null;
            renderInfo();
            setInfoBadge('Idle');
            return;
        }
        if (!isValidHttpUrl(url)) return;
        const platform = detectPlatform(url);
        if (!platform) return;
        if (urlInputDebounceTimer) clearTimeout(urlInputDebounceTimer);
        urlInputDebounceTimer = setTimeout(() => {
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
    els.saveSettingsBtn.addEventListener('click', saveSettings);
    els.pickDirBtn.addEventListener('click', pickDir);
    els.openFolderBtn.addEventListener('click', openFolder);
    els.saveLinkDumpServerBtn.addEventListener('click', () => {
        void saveLinkDumpServer();
    });
    els.restartLinkDumpServerBtn.addEventListener('click', () => {
        void restartLinkDumpServer();
    });
    els.generateLinkDumpSecretBtn.addEventListener('click', () => {
        void generateLinkDumpSecret();
    });
    els.copyGeneratedLinkDumpSecretBtn.addEventListener('click', () => {
        void copyGeneratedLinkDumpSecret();
    });
    els.clearQueueBtn.addEventListener('click', () => {
        void clearQueue();
    });
    els.ytDlpPath.addEventListener('change', () => {
        if (state.activeView === 'settings') void refreshYtDlpVersions();
    });
    els.viewDownloadBtn.addEventListener('click', () => setActiveView('download'));
    els.viewHistoryBtn.addEventListener('click', () => setActiveView('history'));
    els.viewLinkDumpBtn.addEventListener('click', () => setActiveView('linkDump'));
    els.viewSettingsBtn.addEventListener('click', () => setActiveView('settings'));
    els.linkDumpExtensionRepoLink.addEventListener('click', event => {
        void openLinkDumpExtensionRepo(event);
    });
    els.loadMoreHistoryBtn.addEventListener('click', () => {
        void renderHistory({ append: true });
    });
    els.clearHistoryBtn.addEventListener('click', async () => {
        if (!invoke) return;

        try {
            await invoke('clear_history');
            await renderHistory();
        } catch (err) {
            appendLog(`[history] ${err}`, true);
        }
    });
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
        if (event.key === 'Escape') hideQueueContextMenu();
    });
    window.addEventListener('resize', hideQueueContextMenu);
    window.addEventListener('blur', hideQueueContextMenu);
    els.queueList.addEventListener('scroll', hideQueueContextMenu, { passive: true });

    els.copyLogsBtn.addEventListener('click', async () => {
        try {
            await navigator.clipboard.writeText(state.logs.join('\n'));
        } catch (err) {
            appendLog(`[copy] ${err}`, true);
        }
    });
};

const bindBackendEvents = async () => {
    await listen('link-dump:server-status', event => {
        applyLinkDumpServerStatus(event.payload);
    });

    await listen('queue:status', event => {
        state.queueAutoStartEnabled = event.payload?.auto_start ?? true;
        state.queueWorkerRunning = Boolean(event.payload?.worker_running);
        renderQueueControls();
    });

    await listen('queue:update', event => {
        state.queueIds = event.payload.map(job => job.id).filter(id => !state.suppressedJobIds.has(id));
        event.payload.forEach(job => {
            if (state.suppressedJobIds.has(job.id)) return;
            const existing = state.jobs.get(job.id);
            const preset = findPresetForDownloadJob(job);
            updateJob(job.id, {
                url: job.url,
                label: existing?.label || job.url,
                thumbnail: existing?.thumbnail || resolveYouTubeThumbnail(job.url),
                state: 'queued',
                outputPath: existing?.outputPath || null,
                cutStartTime: job.cut_start_time ?? null,
                previewResolved: existing?.previewResolved || Boolean(resolveYouTubeThumbnail(job.url)),
                previewLoading: existing?.previewLoading || false,
                formatLabel: existing?.formatLabel || preset?.queueLabel || job.format,
            });
            maybeHydrateQueueThumbnail(job.id);
        });
    });

    await listen('download:state', event => {
        const { id, state: status, output_path, exit_code, error } = event.payload;
        if (state.suppressedJobIds.has(id)) return;
        const patch = { state: status };
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
    renderPresetOptions();
    renderQueueContextMenu();
    syncMagicImportTriggerState();
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
    await syncConfig();
    await syncLinkDumpOverview();
    await syncQueueStatus();
    await bindBackendEvents();
    renderQueue();
};

init();
