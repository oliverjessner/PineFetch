export const createSettingsView = ({ els, invoke, appendLog, normalizePresetKey, getSelectedPresetKey, updateDownloadOptionHints, isActive }) => {
    const ytDlpLatestReleaseUrl = 'https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest';
    const defaultFasterWhisperModel = 'base';
    const fasterWhisperModels = new Set(['base', 'small', 'medium', 'large-v3']);
    const normalizeFasterWhisperModel = model =>
        fasterWhisperModels.has(model) ? model : defaultFasterWhisperModel;
    let config = null;
    let configSaveQueue = Promise.resolve();
    let configSaveRevision = 0;
    let configLocalRevision = 0;
    let ytDlpVersionsChecked = false;
    let ytDlpVersionsPromise = null;
    let ytDlpVersionRequestId = 0;

    const isMagicImportEnabled = () => Boolean(els.magicImportEnabled?.checked);

    const syncMagicImportTriggerState = () => {
        const enabled = isMagicImportEnabled();
        els.magicImportTrigger.disabled = !enabled;
        els.magicImportTrigger.title = enabled ? 'Magic import from clipboard' : 'Magic import is disabled in Settings';
    };

    const cacheLastDownloadedUrl = async url => {
        const nextUrl = url.trim();
        if (!nextUrl) return;

        config = {
            ...(config || {}),
            last_download_url: nextUrl,
        };
        configLocalRevision += 1;

        if (!invoke) return;
        const queuedCache = configSaveQueue.then(() => invoke('cache_last_download_url', { url: nextUrl }));
        configSaveQueue = queuedCache.catch(() => {});
        try {
            await queuedCache;
        } catch (err) {
            appendLog(`[config] ${err}`, true);
        }
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

    const performYtDlpVersionCheck = async () => {
        if (!invoke) return;
        const requestId = ++ytDlpVersionRequestId;
        const path = els.ytDlpPath.value.trim() || null;
        els.ytDlpInstalledVersion.textContent = 'Installed: checking...';
        els.ytDlpInstalledVersion.removeAttribute('title');
        els.ytDlpLatestVersion.textContent = 'Latest: checking...';

        const [installedResult, latestResult] = await Promise.allSettled([
            invoke('get_yt_dlp_installed_version', { path }),
            fetchLatestYtDlpVersion(),
        ]);
        if (requestId !== ytDlpVersionRequestId) return;

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

    const refreshYtDlpVersions = () => {
        if (!invoke) return Promise.resolve();
        ytDlpVersionsChecked = true;
        const pending = performYtDlpVersionCheck().finally(() => {
            if (ytDlpVersionsPromise === pending) ytDlpVersionsPromise = null;
        });
        ytDlpVersionsPromise = pending;
        return pending;
    };

    const refreshYtDlpVersionsOnce = () =>
        ytDlpVersionsChecked ? ytDlpVersionsPromise || Promise.resolve() : refreshYtDlpVersions();

    const syncConfig = async () => {
        try {
            config = await invoke('get_config');
            els.notificationsEnabled.checked = config.notifications_enabled ?? false;
            els.saveCaptions.checked = config.save_captions ?? false;
            els.saveThumbnails.checked = config.save_thumbnails ?? false;
            els.outputDir.value = config.default_output_dir || '';
            els.ytDlpPath.value = config.yt_dlp_path || '';
            els.fasterWhisperModel.value = normalizeFasterWhisperModel(config.faster_whisper_model);
            els.downloadVideoWithTranscript.checked = config.download_video_with_transcript ?? false;
            els.presetSelect.value = normalizePresetKey(config.selected_preset_key);
            els.magicImportEnabled.checked = config.magic_import_enabled ?? true;
            els.cutAtTimestampEnabled.checked = config.cut_at_timestamp_enabled ?? true;
            syncMagicImportTriggerState();
            updateDownloadOptionHints();
            if (isActive()) void refreshYtDlpVersionsOnce();
        } catch (err) {
            appendLog(`[config] ${err}`, true);
        }
    };

    const persistSelectedPresetKey = async () => {
        const selectedPresetKey = getSelectedPresetKey();
        await saveSettings({ selected_preset_key: selectedPresetKey });
    };

    const persistCaptionSetting = async () => {
        const enabled = Boolean(els.saveCaptions.checked);
        els.saveCaptions.disabled = true;
        try {
            await saveSettings({ save_captions: enabled });
        } finally {
            els.saveCaptions.disabled = false;
            updateDownloadOptionHints();
        }
    };

    const saveSettings = async changes => {
        if (!invoke) return false;
        if (!config) {
            try {
                config = await invoke('get_config');
            } catch (err) {
                els.settingsSaveStatus.textContent = `Could not load settings: ${err}`;
                els.settingsSaveStatus.classList.add('pf-status-error');
                appendLog(`[config] ${err}`, true);
                return false;
            }
        }
        const patch = { ...changes };
        config = { ...config, ...patch };
        const localRevision = ++configLocalRevision;
        const revision = ++configSaveRevision;
        els.settingsSaveStatus.textContent = 'Saving changes…';
        els.settingsSaveStatus.classList.remove('pf-status-error');

        const queuedSave = configSaveQueue.then(() => invoke('patch_config', { changes: patch }));
        configSaveQueue = queuedSave.catch(() => {});

        try {
            const mergedConfig = await queuedSave;
            if (localRevision === configLocalRevision && mergedConfig && typeof mergedConfig === 'object') {
                config = mergedConfig;
            }
            if (revision === configSaveRevision) {
                els.settingsSaveStatus.textContent = 'Changes saved. They apply to new downloads.';
            }
            syncMagicImportTriggerState();
            return true;
        } catch (err) {
            appendLog(`[config] ${err}`, true);
            if (revision === configSaveRevision) {
                await syncConfig();
                els.settingsSaveStatus.textContent = `Could not save changes: ${err}`;
                els.settingsSaveStatus.classList.add('pf-status-error');
            }
            return false;
        }
    };

    const pickDir = async () => {
        try {
            const result = await invoke('pick_output_dir');
            if (result) {
                els.outputDir.value = result;
                await saveSettings({ default_output_dir: result });
            }
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
        els.magicImportEnabled.addEventListener('change', () => {
            syncMagicImportTriggerState();
            void saveSettings({ magic_import_enabled: els.magicImportEnabled.checked });
        });
        els.saveCaptions.addEventListener('change', () => {
            void persistCaptionSetting();
        });
        els.saveThumbnails.addEventListener('change', () => {
            void saveSettings({ save_thumbnails: els.saveThumbnails.checked });
        });
        els.cutAtTimestampEnabled.addEventListener('change', () => {
            void saveSettings({ cut_at_timestamp_enabled: els.cutAtTimestampEnabled.checked });
        });
        els.notificationsEnabled.addEventListener('change', () => {
            void saveSettings({ notifications_enabled: els.notificationsEnabled.checked });
        });
        els.fasterWhisperModel.addEventListener('change', () => {
            void saveSettings({ faster_whisper_model: normalizeFasterWhisperModel(els.fasterWhisperModel.value) });
        });
        els.downloadVideoWithTranscript.addEventListener('change', () => {
            void saveSettings({ download_video_with_transcript: els.downloadVideoWithTranscript.checked });
        });
        els.outputDir.addEventListener('change', () => {
            void saveSettings({ default_output_dir: els.outputDir.value.trim() || null });
        });
        els.ytDlpPath.addEventListener('change', () => {
            void (async () => {
                await saveSettings({ yt_dlp_path: els.ytDlpPath.value.trim() || null });
                await refreshYtDlpVersions();
            })();
        });
        els.pickDirBtn.addEventListener('click', pickDir);
        els.openFolderBtn.addEventListener('click', openFolder);
    };

    return Object.freeze({
        bindEvents,
        cacheLastDownloadedUrl,
        getConfig: () => config,
        isMagicImportEnabled,
        persistSelectedPresetKey,
        refreshYtDlpVersionsOnce,
        save: saveSettings,
        sync: syncConfig,
        syncMagicImportTriggerState,
        waitForPendingSave: () => configSaveQueue,
    });
};
