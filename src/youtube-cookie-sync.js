export const cookiePermissionHelp = platform => {
    if (/mac/i.test(platform)) {
        return 'macOS: A Keychain prompt may mention “Brave Safe Storage” or “Chrome Safe Storage”. Enter your Mac login password in that system prompt and choose Always Allow, if available, to remember access for the requesting program. Allow grants access only this time.';
    }
    if (/win/i.test(platform)) {
        return 'Windows: Cookie access may work without a prompt. If it fails, close the selected browser and retry. Some Chromium browsers protect cookies in a way yt-dlp cannot decrypt; try Firefox if needed.';
    }
    if (/linux/i.test(platform)) {
        return 'Linux: Your desktop may ask you to unlock its keyring or wallet. Approve browser-cookie access there; whether approval can be remembered depends on your desktop. If access fails, close the browser and retry.';
    }
    return 'Your system may ask for browser-cookie access. Approve the request in the system prompt. If access fails, close the selected browser and retry.';
};

export const createYoutubeCookieSync = ({
    els,
    api,
    platform = globalThis.navigator?.userAgentData?.platform || globalThis.navigator?.platform || '',
}) => {
    if (els.youtubeCookiePermissionHelp) els.youtubeCookiePermissionHelp.textContent = cookiePermissionHelp(platform);
    let connectedBrowser = null;
    let busy = false;
    let loaded = false;

    const status = (message, error = false) => {
        els.youtubeCookieStatus.textContent = message;
        els.youtubeCookieStatus.classList.toggle('oj-status-error', error);
    };
    const render = () => {
        els.youtubeCookieBrowser.disabled = busy;
        els.syncYoutubeCookiesBtn.disabled = busy || !api.available || !loaded;
        els.disconnectYoutubeCookiesBtn.disabled = busy || !connectedBrowser;
    };
    const showConnection = () => {
        if (connectedBrowser) {
            els.youtubeCookieBrowser.value = connectedBrowser;
            status(
                `Cookie sync enabled for ${connectedBrowser}. Cookies will be read when the next YouTube request starts.`
            );
        } else {
            status('Cookie sync is off.');
        }
        render();
    };
    const load = async () => {
        try {
            const config = await api.getConfig();
            connectedBrowser = config.youtube_cookie_browser || null;
            loaded = true;
            showConnection();
        } catch {
            status('Cookie sync settings could not be loaded. Reopen Browser Import to retry.', true);
        }
    };
    const save = async browser => {
        if (busy || !api.available || !loaded) return;
        busy = true;
        render();
        let checkingAccess = Boolean(browser);
        try {
            if (browser) {
                status('Checking browser-cookie access… Complete any system permission prompt to continue.');
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
                        ? 'yt-dlp was not found. Set its path in Settings, then enable cookie sync again.'
                        : 'Browser-cookie access could not be verified. Check the permission guidance above and your internet connection, then try again. Your saved connection has not changed.'
                    : 'Cookie sync settings could not be saved. Please try again.',
                true
            );
        } finally {
            busy = false;
            render();
        }
    };
    const bindEvents = () => {
        els.syncYoutubeCookiesBtn.addEventListener('click', () => void save(els.youtubeCookieBrowser.value));
        els.disconnectYoutubeCookiesBtn.addEventListener('click', () => void save(null));
    };
    return Object.freeze({ load, bindEvents, isLoaded: () => loaded });
};
