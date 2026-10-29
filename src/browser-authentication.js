export const browserPermissionHelp = platform => {
    if (/mac/i.test(platform)) {
        return 'macOS: Allow browser-storage access when prompted. For a Keychain prompt such as “Chrome Safe Storage”, use your Mac login password. Choose Always Allow to remember access, if available.';
    }
    if (/win/i.test(platform)) {
        return 'Windows: If access fails, close the browser and retry. Some Chromium browsers protect cookies that yt-dlp cannot decrypt; try Firefox if needed.';
    }
    if (/linux/i.test(platform)) {
        return 'Linux: Unlock your keyring or wallet if prompted. If access fails, close the browser and retry.';
    }
    return 'Your system may ask for browser-cookie access. Approve the request in the system prompt. If access fails, close the selected browser and retry.';
};

export const createBrowserAuthentication = ({
    els,
    api,
    platform = globalThis.navigator?.userAgentData?.platform || globalThis.navigator?.platform || '',
}) => {
    if (els.browserAuthPermissionHelp) els.browserAuthPermissionHelp.textContent = browserPermissionHelp(platform);
    let connectedBrowser = null;
    let checkedBrowser = null;
    let phase = 'idle';
    let busy = false;
    const browserNames = {
        brave: 'Brave',
        chrome: 'Chrome',
        chromium: 'Chromium',
        edge: 'Edge',
        firefox: 'Firefox',
        opera: 'Opera',
        safari: 'Safari',
        vivaldi: 'Vivaldi',
    };
    let loaded = false;

    const status = (message, error = false) => {
        els.browserAuthStatus.textContent = message;
        els.browserAuthStatus.classList.toggle('oj-status-error', error);
    };
    const render = () => {
        els.browserAuthBrowser.disabled = busy;
        els.connectBrowserAuthBtn.disabled = busy || !api.available || !loaded;
        els.disconnectBrowserAuthBtn.disabled = busy || !connectedBrowser || !api.available;
        const accessChecked = connectedBrowser && checkedBrowser === connectedBrowser;
        const labels = {
            checking: 'Checking…',
            saving: 'Saving…',
            error: 'Connection failed',
            unavailable: 'Unavailable',
        };
        if (els.browserAuthState) {
            els.browserAuthState.textContent =
                labels[phase] || (connectedBrowser ? (accessChecked ? 'Connected' : 'Browser saved') : 'Not connected');
            els.browserAuthState.classList.toggle('oj-badge-success', phase === 'idle' && Boolean(accessChecked));
            els.browserAuthState.classList.toggle('oj-badge-danger', phase === 'error' || phase === 'unavailable');
        }
        if (els.browserAuthConnection) {
            els.browserAuthConnection.textContent = connectedBrowser
                ? `${browserNames[connectedBrowser] || connectedBrowser} is saved for new YouTube requests.`
                : 'No browser connection is saved.';
        }
    };
    const showConnection = () => {
        if (connectedBrowser) {
            els.browserAuthBrowser.value = connectedBrowser;
            status(
                checkedBrowser === connectedBrowser
                    ? `Browser access checked for ${connectedBrowser}. Applies to new YouTube requests.`
                    : 'Access has not been checked this session. Connect Browser to check it.'
            );
        } else {
            status('Browser authentication is off.');
        }
        render();
    };
    const load = async savedConfig => {
        try {
            const config = savedConfig || (await api.getConfig());
            connectedBrowser = config.youtube_cookie_browser || null;
            loaded = true;
            phase = 'idle';
            showConnection();
        } catch {
            phase = 'unavailable';
            status('Browser authentication could not be loaded. Reopen Settings to retry.', true);
            render();
        }
    };
    const save = async browser => {
        if (busy || !api.available || !loaded) return;
        busy = true;
        phase = browser ? 'checking' : 'saving';
        render();
        let checkingAccess = Boolean(browser);
        try {
            if (browser) {
                status('Checking browser access… Approve any system permission prompt.');
                await api.checkYoutubeCookieAccess({ browser });
            }
            checkingAccess = false;
            phase = 'saving';
            status(browser ? 'Saving browser connection…' : 'Disconnecting browser…');
            render();
            const config = await api.patchConfig({ changes: { youtube_cookie_browser: browser } });
            connectedBrowser = config.youtube_cookie_browser || null;
            checkedBrowser = browser && browser === connectedBrowser ? browser : null;
            phase = 'idle';
            showConnection();
        } catch (error) {
            phase = 'error';
            status(
                checkingAccess
                    ? String(error) === 'YT_DLP_UNAVAILABLE'
                        ? 'yt-dlp was not found. Set its path in Settings, then try again.'
                        : 'Browser access could not be verified. Check permissions and your internet connection, then retry. Your saved connection has not changed.'
                    : 'Browser authentication could not be saved. Please try again.',
                true
            );
        } finally {
            busy = false;
            render();
        }
    };
    const bindEvents = () => {
        els.connectBrowserAuthBtn.addEventListener('click', () => void save(els.browserAuthBrowser.value));
        els.disconnectBrowserAuthBtn.addEventListener('click', () => void save(null));
    };
    return Object.freeze({ load, bindEvents, isLoaded: () => loaded });
};
