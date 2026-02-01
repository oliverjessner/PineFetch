const tauriGlobal = window.__TAURI__;
const invoke = tauriGlobal?.tauri?.invoke;
const listen = tauriGlobal?.event?.listen;

const state = Object.seal({
    jobs: new Map(),
    queueIds: [],
    selectedId: null,
    logs: [],
    info: null,
    config: null,
});
const els = Object.seal({
    urlInput: document.getElementById('urlInput'),
    loadInfoBtn: document.getElementById('loadInfoBtn'),
    startDownloadBtn: document.getElementById('startDownloadBtn'),
    pickDirBtn: document.getElementById('pickDirBtn'),
    saveSettingsBtn: document.getElementById('saveSettingsBtn'),
    cancelBtn: document.getElementById('cancelBtn'),
    openFolderBtn: document.getElementById('openFolderBtn'),
    outputDir: document.getElementById('outputDir'),
    ytDlpPath: document.getElementById('ytDlpPath'),
    presetSelect: document.getElementById('presetSelect'),
    extractAudio: document.getElementById('extractAudio'),
    audioFormat: document.getElementById('audioFormat'),
    infoTitle: document.getElementById('infoTitle'),
    infoUploader: document.getElementById('infoUploader'),
    infoDuration: document.getElementById('infoDuration'),
    infoThumb: document.getElementById('infoThumb'),
    queueList: document.getElementById('queueList'),
    queueBadge: document.getElementById('queueBadge'),
    infoBadge: document.getElementById('infoBadge'),
    globalStatus: document.getElementById('globalStatus'),
    logBody: document.getElementById('logBody'),
    logPanel: document.getElementById('logPanel'),
    toggleLogsBtn: document.getElementById('toggleLogsBtn'),
    copyLogsBtn: document.getElementById('copyLogsBtn'),
});
const presets = Object.freeze({
    best: 'bv*+ba/b',
    1080: 'bv*[height<=1080]+ba/b[height<=1080]',
    audio: 'ba/b',
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

const setInfoBadge = text => {
    els.infoBadge.textContent = text;
};

const setGlobalStatus = text => {
    els.globalStatus.textContent = text || '';
    els.globalStatus.style.display = text ? 'inline-flex' : 'none';
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
        item.onclick = () => {
            state.selectedId = job.id;
            renderQueue();
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

        item.append(header, progress, meta);
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
        renderInfo();
        setInfoBadge('Ready');
    } catch (err) {
        state.info = null;
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
    const format = presets[presetKey] || presets.best;
    const extractAudio = els.extractAudio.checked;
    const audioFormat = els.audioFormat.value;
    const output_dir = els.outputDir.value.trim() || null;

    try {
        const id = await invoke('enqueue_download', {
            request: {
                url,
                format,
                output_dir,
                extract_audio: extractAudio,
                audio_format: extractAudio ? audioFormat : null,
            },
        });

        updateJob(id, {
            url,
            label: state.info?.title || url,
            state: 'queued',
            percent: 0,
            speed: '-',
            eta: '-',
            formatLabel: presetKey,
        });

        els.urlInput.value = '';
        state.info = null;
        renderInfo();

        setGlobalStatus('Queued');
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

const cancelSelected = async () => {
    if (!state.selectedId) return;
    try {
        await invoke('cancel_download', { id: state.selectedId });
    } catch (err) {
        appendLog(`[cancel] ${err}`, true);
    }
};

const bindEvents = () => {
    els.loadInfoBtn.addEventListener('click', loadInfo);
    els.startDownloadBtn.addEventListener('click', enqueueDownload);
    els.saveSettingsBtn.addEventListener('click', saveSettings);
    els.pickDirBtn.addEventListener('click', pickDir);
    els.openFolderBtn.addEventListener('click', openFolder);
    els.cancelBtn.addEventListener('click', cancelSelected);

    els.presetSelect.addEventListener('change', () => {
        if (els.presetSelect.value === 'audio') {
            els.extractAudio.checked = true;
        }
    });

    els.toggleLogsBtn.addEventListener('click', () => {
        els.logPanel.classList.toggle('collapsed');
        els.toggleLogsBtn.textContent = els.logPanel.classList.contains('collapsed') ? 'Show' : 'Hide';
    });

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
            updateJob(job.id, {
                url: job.url,
                label: job.url,
                state: 'queued',
                formatLabel: job.format,
            });
        });
    });

    await listen('download:state', event => {
        const { id, state: status, exit_code, error } = event.payload;
        updateJob(id, { state: status });
        if (status === 'downloading') setGlobalStatus('Downloading');
        if (status === 'success') setGlobalStatus('Success');
        if (status === 'error') setGlobalStatus('Error');
        if (status === 'cancelled') setGlobalStatus('Cancelled');
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
    setGlobalStatus('');
    if (!invoke || !listen) {
        appendLog('[tauri] API not available. Start the app with `npm run dev` (Tauri), not in a browser.', true);
        setGlobalStatus('No Tauri');
        return;
    }
    await syncConfig();
    await bindBackendEvents();
    renderQueue();
};

init();
