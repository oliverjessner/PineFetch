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
    let busy = false;
    let loaded = false;

    const status = (message, error = false) => {
        els.browserAuthStatus.textContent = message;
        els.browserAuthStatus.classList.toggle('oj-status-error', error);
    };
    const render = () => {
        els.browserAuthBrowser.disabled = busy;
        els.connectBrowserAuthBtn.disabled = busy || !api.available || !loaded;
        els.disconnectBrowserAuthBtn.disabled = busy || !connectedBrowser;
    };
    const showConnection = () => {
        if (connectedBrowser) {
            els.browserAuthBrowser.value = connectedBrowser;
            status(`Browser authentication enabled for ${connectedBrowser}. Applies to new YouTube requests.`);
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
            showConnection();
        } catch {
            status('Browser authentication could not be loaded. Reopen Settings to retry.', true);
        }
    };
    const save = async browser => {
        if (busy || !api.available || !loaded) return;
        busy = true;
        render();
        let checkingAccess = Boolean(browser);
        try {
            if (browser) {
                status('Checking browser access… Approve any system permission prompt.');
                await api.checkYoutubeCookieAccess({ browser });
            }
            checkingAccess = false;
            const config = await api.patchConfig({ changes: { youtube_cookie_browser: browser } });
            connectedBrowser = config.youtube_cookie_browser || null;
            showConnection();
        } catch (error) {
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
