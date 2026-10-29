export const browserPermissionHelp = platform => {
    if (/mac/i.test(platform)) {
        return 'macOS: If browser storage cannot be found or read, enable PineFetch in System Settings → Privacy & Security → Full Disk Access, then reopen PineFetch. Allow browser-storage access when prompted. For a Keychain prompt such as “Chrome Safe Storage”, use your Mac login password. Choose Always Allow to remember access, if available.';
    }
    if (/win/i.test(platform)) {
        return 'Windows: If access fails, close the browser and retry. Some Chromium browsers protect cookies that yt-dlp cannot decrypt; try Firefox if needed.';
    }
    if (/linux/i.test(platform)) {
        return 'Linux: Unlock your keyring or wallet if prompted. If access fails, close the browser and retry.';
    }
    return 'Your system may ask for browser-cookie access. Approve the request in the system prompt. If access fails, close the selected browser and retry.';
};

// Match only fixed native codes. Unknown errors and raw backend details never reach the UI.
export const browserAccessFailure = error => {
    const code = typeof error === 'string' ? error : error instanceof Error ? error.message : '';
    const messages = {
        YT_DLP_UNAVAILABLE: 'yt-dlp could not be started. Check its path in Settings, then retry.',
        INVALID_BROWSER: 'Choose one of the listed browsers, then retry.',
        PROFILE_UNAVAILABLE:
            'The selected browser profile could not be found or read. Open that browser once and check that you selected the right browser family (Chrome and Chromium are separate). Review storage permissions in Troubleshooting.',
        PERMISSION_DENIED:
            'Your system denied access to browser storage. Review the permissions in Troubleshooting, then reopen PineFetch and retry.',
        BROWSER_LOCKED:
            'The browser cookie database could not be copied or is busy. Close the selected browser completely, then retry. If it still fails, review storage permissions in Troubleshooting.',
        COOKIE_DECRYPTION:
            'Some browser cookies could not be decrypted. Unlock your Keychain or keyring and approve the system prompt. On Windows, try Firefox if Chromium cookie protection prevents access. See Troubleshooting.',
        COOKIES_UNAVAILABLE:
            'No cookies were found in the selected browser profile. Open YouTube in that browser and sign in, then retry. Check that its most recently used profile is the one you want.',
        NETWORK_DNS:
            'YouTube could not be found on the network. Check your internet connection, DNS and VPN or proxy settings, then retry.',
        NETWORK_TLS:
            'A secure connection to YouTube could not be established. Check your system clock, certificates and VPN or proxy settings, then retry.',
        NETWORK_TIMEOUT:
            'The YouTube request timed out. Check your internet connection and VPN or proxy settings, then retry.',
        NETWORK_CONNECTION:
            'The connection to YouTube failed. Check your internet connection, firewall and VPN or proxy settings, then retry.',
        RATE_LIMITED:
            'YouTube is limiting requests. Wait before retrying; browser cookies do not guarantee avoiding rate limits.',
        SIGN_IN_REQUIRED:
            'YouTube requested a sign-in or challenge. Open YouTube in the selected browser and complete it, then retry. This does not verify access to protected videos.',
        ACCESS_REJECTED:
            'YouTube rejected the public request. Check access in the selected browser and review VPN or proxy settings, then retry. Your sign-in could not be verified.',
        CHECK_TIMEOUT:
            'The access check did not finish in time. Look for a pending system permission prompt, then check your internet connection and retry.',
    };
    return Object.hasOwn(messages, code)
        ? messages[code]
        : 'The public access check could not be completed. Check the selected profile and Troubleshooting, then retry. Your YouTube sign-in could not be verified.';
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
            error: 'Check failed',
            save_error: 'Save failed',
            unavailable: 'Unavailable',
        };
        if (els.browserAuthState) {
            els.browserAuthState.textContent =
                labels[phase] ||
                (connectedBrowser ? (accessChecked ? 'Public access checked' : 'Browser saved') : 'Not connected');
            els.browserAuthState.classList.toggle('oj-badge-success', phase === 'idle' && Boolean(accessChecked));
            els.browserAuthState.classList.toggle(
                'oj-badge-danger',
                ['error', 'save_error', 'unavailable'].includes(phase)
            );
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
                    ? `Public YouTube metadata was retrieved using ${browserNames[connectedBrowser]}. Applies to new YouTube requests; YouTube sign-in is not verified.`
                    : 'Public access has not been checked this session. Connect Browser to check it; YouTube sign-in will remain unverified.'
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
                status('Checking public YouTube access… Approve any system permission prompt.');
                const report = await api.checkYoutubeCookieAccess({ browser });
                if (report?.public_access !== 'checked' || report.authentication !== 'not_verified') {
                    throw new Error('CHECK_FAILED');
                }
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
            phase = checkingAccess ? 'error' : 'save_error';
            status(
                checkingAccess
                    ? `${browserAccessFailure(error)} Your saved connection has not changed.`
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
