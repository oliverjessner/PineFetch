const tauriGlobal = window.__TAURI__;
const invoke = tauriGlobal?.tauri?.invoke;
const listen = tauriGlobal?.event?.listen;

const state = Object.seal({
    jobs: new Map(),
    queueIds: [],
    suppressedJobIds: new Set(),
    selectedId: null,
    logs: [],
    info: null,
    infoUrl: null,
    config: null,
    activeView: 'download',
});
const els = Object.seal({
    urlInput: document.getElementById('urlInput'),
    loadInfoBtn: document.getElementById('loadInfoBtn'),
    startDownloadBtn: document.getElementById('startDownloadBtn'),
    pickDirBtn: document.getElementById('pickDirBtn'),
    saveSettingsBtn: document.getElementById('saveSettingsBtn'),
    openFolderBtn: document.getElementById('openFolderBtn'),
    outputDir: document.getElementById('outputDir'),
    ytDlpPath: document.getElementById('ytDlpPath'),
    ytDlpInstalledVersion: document.getElementById('ytDlpInstalledVersion'),
    ytDlpLatestVersion: document.getElementById('ytDlpLatestVersion'),
    presetSelect: document.getElementById('presetSelect'),
    infoTitle: document.getElementById('infoTitle'),
    infoUploader: document.getElementById('infoUploader'),
    infoDuration: document.getElementById('infoDuration'),
    infoThumb: document.getElementById('infoThumb'),
    queueList: document.getElementById('queueList'),
    queueBadge: document.getElementById('queueBadge'),
    clearQueueBtn: document.getElementById('clearQueueBtn'),
    infoBadge: document.getElementById('infoBadge'),
    logBody: document.getElementById('logBody'),
    copyLogsBtn: document.getElementById('copyLogsBtn'),
    leftPanelTitle: document.getElementById('leftPanelTitle'),
    downloadView: document.getElementById('downloadView'),
    settingsView: document.getElementById('settingsView'),
    viewDownloadBtn: document.getElementById('viewDownloadBtn'),
    viewSettingsBtn: document.getElementById('viewSettingsBtn'),
});
const ytDlpLatestReleaseUrl = 'https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest';
const presets = Object.freeze({
    best: {
        label: 'Best',
        format: 'bv*+ba/b',
        extractAudio: false,
        audioFormat: null,
        transcribeText: false,
    },
    1080: {
        label: 'Max 1080p',
        format: 'bv*[height<=1080]+ba/b[height<=1080]',
        extractAudio: false,
        audioFormat: null,
        transcribeText: false,
    },
    audio_mp3: {
        label: 'Audio only (mp3)',
        format: 'ba/b',
        extractAudio: true,
        audioFormat: 'mp3',
        transcribeText: false,
    },
    audio_opus: {
        label: 'Audio only (opus)',
        format: 'ba/b',
        extractAudio: true,
        audioFormat: 'opus',
        transcribeText: false,
    },
    text: {
        label: 'Text (fast-whisper)',
        format: 'ba/b',
        extractAudio: true,
        audioFormat: 'mp3',
        transcribeText: true,
    },
});
const defaultYtDlpPath = '/opt/homebrew/bin/yt-dlp';
let urlShakeTimer = null;
const formatDuration = seconds => {
    if (!seconds && seconds !== 0) return '-';
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    const hrs = Math.floor(mins / 60);
    if (hrs > 0) return `${hrs}h ${String(mins % 60).padStart(2, '0')}m`;
    return `${mins}m ${String(secs).padStart(2, '0')}s`;
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

const getPlatformIconSvg = platform => {
    switch (platform) {
    case 'youtube':
        return `
<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
  <path d="M22 12c0 2.7-.3 4.4-.6 5.3-.3.8-.9 1.4-1.7 1.7-.9.3-2.6.6-7.7.6s-6.8-.3-7.7-.6c-.8-.3-1.4-.9-1.7-1.7C2.3 16.4 2 14.7 2 12s.3-4.4.6-5.3c.3-.8.9-1.4 1.7-1.7C5.2 4.7 6.9 4.4 12 4.4s6.8.3 7.7.6c.8.3 1.4.9 1.7 1.7.3.9.6 2.6.6 5.3Z" fill="currentColor"/>
  <path d="M10 8.8 15.5 12 10 15.2V8.8Z" fill="#fff"/>
</svg>`;
    case 'facebook':
        return `
<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
  <path d="M13.6 8.6h2.3V5.4h-2.7c-2.6 0-4 1.5-4 4v1.9H7v3.1h2.2v5.2h3.3v-5.2h2.7l.4-3.1h-3.1V9.8c0-.8.3-1.2.8-1.2Z" fill="currentColor"/>
</svg>`;
    case 'twitch':
        return `
<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
  <path d="M4 3h16v11.2l-4 4H12l-2.8 2.8V18.2H4V3Zm2 2v11.2h3.2v1.6l1.6-1.6H15l3-3V5H6Zm4.2 2.4h1.8v4.2h-1.8V7.4Zm4 0H16v4.2h-1.8V7.4Z" fill="currentColor"/>
</svg>`;
    case 'x':
        return `
<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
  <path d="M4 4h3.8l4.7 6.4L17.8 4H20l-6.4 7.3L20.5 20h-3.8l-5-6.8L5.9 20H3.7l6.7-7.6L4 4Z" fill="currentColor"/>
</svg>`;
    case 'tiktok':
        return `
<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
  <path d="M14.5 4c1.1 1.6 2.3 2.4 4 2.5V9c-1.5 0-2.8-.4-4-1.2v6.6a4.8 4.8 0 1 1-3.8-4.7v2.6a2.2 2.2 0 1 0 1.3 2V4h2.5Z" fill="currentColor"/>
</svg>`;
    case 'instagram':
        return `
<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
  <rect x="3.5" y="3.5" width="17" height="17" rx="5" fill="none" stroke="currentColor" stroke-width="2"/>
  <circle cx="12" cy="12" r="3.5" fill="none" stroke="currentColor" stroke-width="2"/>
  <circle cx="17.2" cy="6.8" r="1.2" fill="currentColor"/>
</svg>`;
    default:
        return '';
    }
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
    els.urlInput.classList.remove('invalid-shake');
    void els.urlInput.offsetWidth;
    els.urlInput.classList.add('invalid-shake');
    els.urlInput.focus();
    urlShakeTimer = setTimeout(() => {
        els.urlInput.classList.remove('invalid-shake');
        urlShakeTimer = null;
    }, 420);
};

const extractYouTubeVideoId = url => {
    try {
        const parsed = new URL(url);
        const host = parsed.hostname.replace(/^www\./, '').toLowerCase();
        if (host === 'youtu.be') {
            return parsed.pathname.split('/').filter(Boolean)[0] || null;
        }
        if (host.endsWith('youtube.com')) {
            if (parsed.pathname === '/watch') {
                return parsed.searchParams.get('v');
            }
            if (parsed.pathname.startsWith('/shorts/') || parsed.pathname.startsWith('/embed/')) {
                return parsed.pathname.split('/').filter(Boolean)[1] || null;
            }
        }
    } catch {
        return null;
    }
    return null;
};

const resolveYouTubeThumbnail = url => {
    const videoId = extractYouTubeVideoId(url);
    return videoId ? `https://i.ytimg.com/vi/${videoId}/mqdefault.jpg` : null;
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
    const nextView = view === 'settings' ? 'settings' : 'download';
    const isSettings = nextView === 'settings';
    state.activeView = nextView;

    els.downloadView.hidden = isSettings;
    els.settingsView.hidden = !isSettings;
    els.downloadView.classList.toggle('active', !isSettings);
    els.settingsView.classList.toggle('active', isSettings);

    els.viewDownloadBtn.classList.toggle('active', !isSettings);
    els.viewDownloadBtn.setAttribute('aria-pressed', String(!isSettings));
    els.viewSettingsBtn.classList.toggle('active', isSettings);
    els.viewSettingsBtn.setAttribute('aria-pressed', String(isSettings));

    els.leftPanelTitle.textContent = isSettings ? 'Settings' : 'Download';
    els.infoBadge.style.display = isSettings ? 'none' : 'inline-flex';
    if (isSettings) void refreshYtDlpVersions();
};

const renderInfo = () => {
    if (!state.info) {
        els.infoTitle.textContent = '-';
        els.infoUploader.textContent = '-';
        els.infoDuration.textContent = '-';
        els.infoThumb.style.backgroundImage = '';
        return;
    }
    const { title, uploader, duration, thumbnail } = state.info;
    els.infoTitle.textContent = title || '-';
    els.infoUploader.textContent = uploader || '-';
    els.infoDuration.textContent = formatDuration(duration);
    if (thumbnail) {
        els.infoThumb.style.backgroundImage = `url('${thumbnail}')`;
    } else {
        els.infoThumb.style.backgroundImage = '';
    }
};

const renderQueue = () => {
    const items = Array.from(state.jobs.values()).sort((a, b) => a.createdAt - b.createdAt);
    els.queueList.innerHTML = '';
    items.forEach(job => {
        const item = document.createElement('div');
        item.className = `queue-item ${job.id === state.selectedId ? 'active' : ''}`;
        item.onclick = async () => {
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
        header.className = 'queue-header';

        const title = document.createElement('div');
        title.className = 'queue-title';
        const platform = detectPlatform(job.url || '');
        if (platform) {
            const platformIcon = document.createElement('span');
            platformIcon.className = `queue-platform-icon ${platform}`;
            platformIcon.innerHTML = getPlatformIconSvg(platform);
            title.appendChild(platformIcon);
        }
        const titleText = document.createElement('span');
        titleText.className = 'queue-title-text';
        titleText.textContent = job.label || job.url;
        title.appendChild(titleText);

        const badge = document.createElement('div');
        badge.className = 'queue-badge';
        badge.textContent = job.state || 'queued';

        header.append(title, badge);

        const progress = document.createElement('div');
        progress.className = 'progress';
        const bar = document.createElement('span');
        bar.style.width = `${job.percent || 0}%`;
        progress.appendChild(bar);

        const meta = document.createElement('div');
        meta.className = 'queue-meta';
        meta.innerHTML = `
      <span>${job.speed || '-'}</span>
      <span>${job.eta || '-'}</span>
      <span>${job.formatLabel || ''}</span>
    `;
        const main = document.createElement('div');
        main.className = 'queue-main';

        const content = document.createElement('div');
        content.className = 'queue-content';
        content.append(header, progress, meta);
        main.appendChild(content);

        const thumbUrl = job.thumbnail || resolveYouTubeThumbnail(job.url);
        if (thumbUrl) {
            const thumb = document.createElement('div');
            thumb.className = 'queue-thumb';
            thumb.style.backgroundImage = `url('${thumbUrl}')`;
            main.appendChild(thumb);
        } else {
            main.classList.add('no-thumb');
        }

        item.append(main);
        els.queueList.appendChild(item);
    });

    els.queueBadge.textContent = `${state.queueIds.length} queued`;
};

const appendLog = (text, isError) => {
    state.logs.push(text);
    const line = document.createElement('div');
    line.className = `log-line ${isError ? 'err' : ''}`;
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
        void refreshYtDlpVersions();
    } catch (err) {
        appendLog(`[config] ${err}`, true);
    }
};

const loadInfo = async () => {
    const url = els.urlInput.value.trim();
    if (!url) return;
    if (!isValidHttpUrl(url)) {
        shakeUrlInput();
        return;
    }
    els.loadInfoBtn.classList.add('loading');
    els.loadInfoBtn.disabled = true;
    setInfoBadge('Loading...');
    try {
        const info = await invoke('load_info', { url });
        state.info = info;
        state.infoUrl = url;
        renderInfo();
        setInfoBadge('Ready');
    } catch (err) {
        if (`${err || ''}`.includes('URL must start with')) {
            shakeUrlInput();
        }
        state.info = null;
        state.infoUrl = null;
        renderInfo();
        setInfoBadge('Error');
        appendLog(`[info] ${err}`, true);
    } finally {
        els.loadInfoBtn.classList.remove('loading');
        els.loadInfoBtn.disabled = false;
    }
};

const enqueueDownload = async () => {
    const url = els.urlInput.value.trim();
    if (!url) return;
    if (!isValidHttpUrl(url)) {
        shakeUrlInput();
        return;
    }

    const presetKey = els.presetSelect.value;
    const preset = presets[presetKey] || presets.best;
    const output_dir = els.outputDir.value.trim() || null;
    const hasLoadedInfo = state.info && state.infoUrl === url;

    try {
        const id = await invoke('enqueue_download', {
            request: {
                url,
                format: preset.format,
                output_dir,
                extract_audio: preset.extractAudio,
                audio_format: preset.audioFormat,
                transcribe_text: preset.transcribeText,
            },
        });

        updateJob(id, {
            url,
            label: hasLoadedInfo ? state.info?.title || url : url,
            thumbnail: hasLoadedInfo ? state.info?.thumbnail || null : resolveYouTubeThumbnail(url),
            state: 'queued',
            outputPath: null,
            previewResolved: Boolean(hasLoadedInfo || resolveYouTubeThumbnail(url)),
            previewLoading: false,
            percent: 0,
            speed: '-',
            eta: '-',
            formatLabel: preset.label,
        });
        maybeHydrateQueueThumbnail(id);

        els.urlInput.value = '';
        state.info = null;
        state.infoUrl = null;
        renderInfo();
        els.urlInput.focus();
    } catch (err) {
        if (`${err || ''}`.includes('URL must start with')) {
            shakeUrlInput();
        }
        appendLog(`[queue] ${err}`, true);
    }
};

const saveSettings = async () => {
    try {
        await invoke('set_config', {
            config: {
                yt_dlp_path: els.ytDlpPath.value.trim() || null,
                default_output_dir: els.outputDir.value.trim() || null,
            },
        });
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

const clearQueue = async () => {
    const idsToCancel = new Set(state.queueIds);
    state.jobs.forEach(job => {
        if (job.state === 'queued' || job.state === 'downloading' || job.state === 'transcribing' || job.state === 'cancelling') {
            idsToCancel.add(job.id);
        }
    });

    idsToCancel.forEach(id => state.suppressedJobIds.add(id));

    if (invoke && idsToCancel.size > 0) {
        const results = await Promise.allSettled(
            Array.from(idsToCancel).map(id => invoke('cancel_download', { id })),
        );
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
    els.loadInfoBtn.addEventListener('click', loadInfo);
    els.startDownloadBtn.addEventListener('click', enqueueDownload);
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
        }
    });
    els.saveSettingsBtn.addEventListener('click', saveSettings);
    els.pickDirBtn.addEventListener('click', pickDir);
    els.openFolderBtn.addEventListener('click', openFolder);
    els.clearQueueBtn.addEventListener('click', () => {
        void clearQueue();
    });
    els.ytDlpPath.addEventListener('change', () => {
        if (state.activeView === 'settings') void refreshYtDlpVersions();
    });
    els.viewDownloadBtn.addEventListener('click', () => setActiveView('download'));
    els.viewSettingsBtn.addEventListener('click', () => setActiveView('settings'));

    els.copyLogsBtn.addEventListener('click', async () => {
        try {
            await navigator.clipboard.writeText(state.logs.join('\n'));
        } catch (err) {
            appendLog(`[copy] ${err}`, true);
        }
    });
};

const bindBackendEvents = async () => {
    await listen('queue:update', event => {
        state.queueIds = event.payload
            .map(job => job.id)
            .filter(id => !state.suppressedJobIds.has(id));
        event.payload.forEach(job => {
            if (state.suppressedJobIds.has(job.id)) return;
            const existing = state.jobs.get(job.id);
            updateJob(job.id, {
                url: job.url,
                label: existing?.label || job.url,
                thumbnail: existing?.thumbnail || resolveYouTubeThumbnail(job.url),
                state: 'queued',
                outputPath: existing?.outputPath || null,
                previewResolved: existing?.previewResolved || Boolean(resolveYouTubeThumbnail(job.url)),
                previewLoading: existing?.previewLoading || false,
                formatLabel: existing?.formatLabel || job.format,
            });
            maybeHydrateQueueThumbnail(job.id);
        });
    });

    await listen('download:state', event => {
        const { id, state: status, output_path, exit_code, error } = event.payload;
        if (state.suppressedJobIds.has(id)) return;
        const patch = { state: status };
        if (output_path) patch.outputPath = output_path;
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
    await bindBackendEvents();
    renderQueue();
};

init();
