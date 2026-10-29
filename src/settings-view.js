import { createDropdownChoice } from './dropdown-choice.js';
import { createBrowserAuthentication } from './browser-authentication.js';

export const createSettingsView = ({
    els,
    api,
    appendLog,
    setSelectedPresetKey,
    getSelectedPresetKey,
    updateDownloadOptionHints,
    isActive,
}) => {
    const browserAuthentication = createBrowserAuthentication({ els, api });
    const ytDlpLatestReleaseUrl = 'https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest';
    const defaultFasterWhisperModel = 'base';
    const fasterWhisperModelChoice = createDropdownChoice({
        menu: els.fasterWhisperModelMenu,
        value: els.fasterWhisperModelValue,
    });
    fasterWhisperModelChoice.setOptions([
        { value: 'base', label: 'Fast (base)' },
        { value: 'small', label: 'Balanced (small)' },
        { value: 'medium', label: 'High (medium)' },
        { value: 'large-v3', label: 'Best (large-v3)' },
    ]);
    const normalizeFasterWhisperModel = model =>
        fasterWhisperModelChoice.hasValue(model) ? model : defaultFasterWhisperModel;
    let config = null;
    let configSaveQueue = Promise.resolve();
    let configSaveRevision = 0;
    let configLocalRevision = 0;
    let outputDirSaveRevision = 0;
    let ytDlpVersionsChecked = false;
    let ytDlpVersionsPromise = null;
    let ytDlpVersionRequestId = 0;
    let saveFeedbackTimer = null;
    let saveFeedbackRevision = 0;
    const newDownloadSettings = new Set([
        'selected_preset_key',
        'magic_import_enabled',
        'id_import_enabled',
        'cut_at_timestamp_enabled',
        'faster_whisper_model',
        'download_video_with_transcript',
        'save_captions',
        'save_creator_subtitles',
        'save_thumbnails',
        'default_output_dir',
    ]);
    const setSaveFeedback = (message, { error = false, temporary = false } = {}) => {
        const revision = ++saveFeedbackRevision;
        if (saveFeedbackTimer !== null) clearTimeout(saveFeedbackTimer);
        saveFeedbackTimer = null;
        els.settingsSaveStatus.textContent = message;
        els.settingsSaveStatus.classList.toggle('oj-status-error', error);
        if (temporary) {
            saveFeedbackTimer = setTimeout(() => {
                if (revision !== saveFeedbackRevision) return;
                els.settingsSaveStatus.textContent = '';
                saveFeedbackTimer = null;
            }, 4000);
        }
    };

    const isMagicImportEnabled = () => Boolean(els.magicImportEnabled?.checked);

    const cacheLastDownloadedUrl = async url => {
        const nextUrl = url.trim();
        if (!nextUrl) return;

        config = {
            ...(config || {}),
            last_download_url: nextUrl,
        };
        configLocalRevision += 1;

        if (!api.available) return;
        const queuedCache = configSaveQueue.then(() => api.cacheLastDownloadUrl({ url: nextUrl }));
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
        if (!api.available) return;
        const requestId = ++ytDlpVersionRequestId;
        const path = els.ytDlpPath.value.trim() || null;
        els.ytDlpInstalledVersion.textContent = 'Installed: checking...';
        els.ytDlpInstalledVersion.removeAttribute('title');
        els.ytDlpLatestVersion.textContent = 'Latest: checking...';

        const [installedResult, latestResult] = await Promise.allSettled([
            api.getYtDlpInstalledVersion({ path }),
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
        if (!api.available) return Promise.resolve();
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
            config = await api.getConfig();
            await browserAuthentication.load(config);
            els.notificationsEnabled.checked = config.notifications_enabled ?? false;
            els.saveCaptions.checked = config.save_captions ?? false;
            els.saveCreatorSubtitles.checked = config.save_creator_subtitles ?? false;
            els.saveThumbnails.checked = config.save_thumbnails ?? false;
            els.outputDir.value = config.default_output_dir || '';
            els.ytDlpPath.value = config.yt_dlp_path || '';
            fasterWhisperModelChoice.setValue(normalizeFasterWhisperModel(config.faster_whisper_model));
            els.downloadVideoWithTranscript.checked = config.download_video_with_transcript ?? false;
            setSelectedPresetKey(config.selected_preset_key);
            els.magicImportEnabled.checked = config.magic_import_enabled ?? true;
            els.idImportEnabled.checked = config.id_import_enabled ?? false;
            els.cutAtTimestampEnabled.checked = config.cut_at_timestamp_enabled ?? true;
            updateDownloadOptionHints();
            if (isActive()) void refreshYtDlpVersionsOnce();
        } catch (err) {
            setSaveFeedback(`Could not load settings: ${err}`, { error: true });
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
        if (!api.available) return false;
        if (!config) {
            try {
                config = await api.getConfig();
            } catch (err) {
                setSaveFeedback(`Could not load settings: ${err}`, { error: true });
                appendLog(`[config] ${err}`, true);
                return false;
            }
        }
        const patch = { ...changes };
        config = { ...config, ...patch };
        const localRevision = ++configLocalRevision;
        const revision = ++configSaveRevision;
        setSaveFeedback('Saving changes…');

        const queuedSave = configSaveQueue.then(() => api.patchConfig({ changes: patch }));
        configSaveQueue = queuedSave.catch(() => {});

        try {
            const mergedConfig = await queuedSave;
            if (localRevision === configLocalRevision && mergedConfig && typeof mergedConfig === 'object') {
                config = mergedConfig;
            }
            if (revision === configSaveRevision) {
                const appliesToNewDownloads = Object.keys(patch).some(key => newDownloadSettings.has(key));
                setSaveFeedback(appliesToNewDownloads ? 'Saved for new downloads.' : 'Saved.', { temporary: true });
            }
            return true;
        } catch (err) {
            appendLog(`[config] ${err}`, true);
            if (revision === configSaveRevision) {
                await syncConfig();
                setSaveFeedback(`Could not save changes: ${err}`, { error: true });
            }
            return false;
        }
    };

    const setOutputDirStatus = (message, isError = false) => {
        els.outputDirStatus.textContent = message;
        els.outputDirStatus.hidden = !message;
        els.outputDirStatus.classList.toggle('oj-status-error', isError);
    };

    const saveOutputDir = async value => {
        const revision = ++outputDirSaveRevision;
        setOutputDirStatus('Saving download folder…');
        const saved = await saveSettings({ default_output_dir: value.trim() || null });
        if (revision !== outputDirSaveRevision) return;
        setOutputDirStatus(
            saved ? 'Download folder saved. Applies to new downloads.' : 'Could not save download folder.',
            !saved
        );
    };

    const pickDir = async () => {
        try {
            const result = await api.pickOutputDir();
            if (result) {
                els.outputDir.value = result;
                await saveOutputDir(result);
            }
        } catch (err) {
            appendLog(`[dir] ${err}`, true);
            setOutputDirStatus(`Could not choose download folder: ${err}`, true);
        }
    };

    const openFolder = async () => {
        const path = els.outputDir.value.trim();
        if (!path) return;
        try {
            await api.openFolder({ path });
        } catch (err) {
            appendLog(`[open] ${err}`, true);
        }
    };

    const bindEvents = () => {
        const navigation = els.settingsView.querySelector('[role="tablist"]');
        const compact = window.matchMedia('(max-width: 680px)');
        const orientNavigation = () =>
            navigation.setAttribute('aria-orientation', compact.matches ? 'horizontal' : 'vertical');
        orientNavigation();
        compact.addEventListener('change', orientNavigation);
        els.settingsView.addEventListener('oj:change', event => {
            if (event.detail?.panel?.classList.contains('pinefetch-settings-category')) {
                els.settingsView.querySelector('#settingsContent').scrollTop = 0;
                if (event.detail.panel.id === 'settingsDiagnosticsPanel') {
                    requestAnimationFrame(() => {
                        els.logBody.scrollTop = els.logBody.scrollHeight;
                    });
                }
            }
        });
        window.addEventListener(
            'pagehide',
            () => {
                compact.removeEventListener('change', orientNavigation);
                if (saveFeedbackTimer !== null) clearTimeout(saveFeedbackTimer);
            },
            { once: true }
        );
        browserAuthentication.bindEvents();
        els.magicImportEnabled.addEventListener('change', () => {
            void saveSettings({ magic_import_enabled: els.magicImportEnabled.checked });
        });
        els.idImportEnabled.addEventListener('change', async () => {
            const previousValue = config?.id_import_enabled ?? false;
            els.idImportEnabled.disabled = true;
            try {
                const saved = await saveSettings({ id_import_enabled: els.idImportEnabled.checked });
                if (!saved) {
                    els.idImportEnabled.checked = previousValue;
                    if (config) config = { ...config, id_import_enabled: previousValue };
                }
            } finally {
                els.idImportEnabled.disabled = false;
                updateDownloadOptionHints();
                els.urlInput.dispatchEvent(new Event('input'));
            }
        });
        els.saveCaptions.addEventListener('change', () => {
            void persistCaptionSetting();
        });
        els.saveCreatorSubtitles.addEventListener('change', () => {
            void saveSettings({ save_creator_subtitles: els.saveCreatorSubtitles.checked });
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
        els.fasterWhisperModelDropdown.addEventListener('oj:select', event => {
            if (
                event.target !== els.fasterWhisperModelDropdown ||
                !fasterWhisperModelChoice.hasValue(event.detail?.value)
            )
                return;
            const model = fasterWhisperModelChoice.setValue(event.detail.value);
            void saveSettings({ faster_whisper_model: model });
        });
        els.downloadVideoWithTranscript.addEventListener('change', () => {
            void saveSettings({ download_video_with_transcript: els.downloadVideoWithTranscript.checked });
        });
        els.outputDir.addEventListener('change', () => {
            void saveOutputDir(els.outputDir.value);
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
        loadBrowserAuthentication: () =>
            browserAuthentication.isLoaded() ? Promise.resolve() : browserAuthentication.load(),
        persistSelectedPresetKey,
        refreshYtDlpVersionsOnce,
        save: saveSettings,
        sync: syncConfig,
        waitForPendingSave: () => configSaveQueue,
    });
};
