const tauriGlobal = window.__TAURI__;
const invoke = tauriGlobal?.tauri?.invoke;
const listen = tauriGlobal?.event?.listen;

const state = Object.seal({
    jobs: new Map(),
    queueIds: [],
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
const formatDuration = seconds => {
    if (!seconds && seconds !== 0) return '-';
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    const hrs = Math.floor(mins / 60);
    if (hrs > 0) return `${hrs}h ${String(mins % 60).padStart(2, '0')}m`;
    return `${mins}m ${String(secs).padStart(2, '0')}s`;
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
        title.textContent = job.label || job.url;

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
            percent: 0,
            speed: '-',
            eta: '-',
            formatLabel: preset.label,
        });

        els.urlInput.value = '';
        state.info = null;
        state.infoUrl = null;
        renderInfo();
    } catch (err) {
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

const bindEvents = () => {
    els.loadInfoBtn.addEventListener('click', loadInfo);
    els.startDownloadBtn.addEventListener('click', enqueueDownload);
    els.saveSettingsBtn.addEventListener('click', saveSettings);
    els.pickDirBtn.addEventListener('click', pickDir);
    els.openFolderBtn.addEventListener('click', openFolder);
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
        state.queueIds = event.payload.map(job => job.id);
        event.payload.forEach(job => {
            const existing = state.jobs.get(job.id);
            updateJob(job.id, {
                url: job.url,
                label: existing?.label || job.url,
                thumbnail: existing?.thumbnail || resolveYouTubeThumbnail(job.url),
                state: 'queued',
                outputPath: existing?.outputPath || null,
                formatLabel: existing?.formatLabel || job.format,
            });
        });
    });

    await listen('download:state', event => {
        const { id, state: status, output_path, exit_code, error } = event.payload;
        const patch = { state: status };
        if (output_path) patch.outputPath = output_path;
        updateJob(id, patch);
        if (error) appendLog(`[${id}] ${error} (${exit_code ?? '?'})`, true);
    });

    await listen('download:progress', event => {
        const { id, percent, speed, eta } = event.payload;
        updateJob(id, {
            percent: percent ?? 0,
            speed: speed || '-',
            eta: eta || '-',
        });
    });

    await listen('download:log', event => {
        const { id, line, is_error } = event.payload;
        appendLog(`[${id}] ${line}`, is_error);
    });
};

const init = async () => {
    bindEvents();
    setActiveView('download');
    if (!invoke || !listen) {
        appendLog('[tauri] API not available. Start the app with `npm run dev` (Tauri), not in a browser.', true);
        return;
    }
    await syncConfig();
    await bindBackendEvents();
    renderQueue();
};

init();
