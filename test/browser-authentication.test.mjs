import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createBrowserAuthentication, browserPermissionHelp } from '../src/browser-authentication.js';

const setup = api => {
    const element = () => ({
        value: 'brave',
        disabled: true,
        textContent: '',
        classList: { toggle() {} },
        addEventListener(event, handler) {
            this[event] = handler;
        },
    });
    const els = Object.fromEntries(
        ['browserAuthBrowser', 'connectBrowserAuthBtn', 'disconnectBrowserAuthBtn', 'browserAuthStatus'].map(key => [
            key,
            element(),
        ])
    );
    const view = createBrowserAuthentication({ els, api: { available: true, ...api } });
    view.bindEvents();
    return { els, view };
};
const settle = () => new Promise(resolve => setImmediate(resolve));

test('restores saved browser, enables fresh cookie reads, and disconnects with a nullable patch', async () => {
    const calls = [];
    const checks = [];
    const { els, view } = setup({
        checkYoutubeCookieAccess: async args => {
            checks.push(args);
        },
        getConfig: async () => ({ youtube_cookie_browser: 'firefox' }),
        patchConfig: async ({ changes }) => {
            calls.push(changes);
            return changes;
        },
    });
    await view.load();
    assert.equal(els.browserAuthBrowser.value, 'firefox');
    assert.deepEqual(checks, []);
    assert.equal(els.disconnectBrowserAuthBtn.disabled, false);
    els.browserAuthBrowser.value = 'brave';
    els.connectBrowserAuthBtn.click();
    assert.equal(els.connectBrowserAuthBtn.disabled, true);
    await settle();
    assert.deepEqual(calls, [{ youtube_cookie_browser: 'brave' }]);
    assert.deepEqual(checks, [{ browser: 'brave' }]);
    assert.match(els.browserAuthStatus.textContent, /new YouTube requests/);
    els.disconnectBrowserAuthBtn.click();
    await settle();
    assert.equal(calls[1].youtube_cookie_browser, null);
    assert.equal(checks.length, 1);
    assert.equal(els.disconnectBrowserAuthBtn.disabled, true);
    assert.equal(els.browserAuthStatus.textContent, 'Browser authentication is off.');
});

test('failed save preserves the connection and never displays backend details', async () => {
    const { els, view } = setup({
        getConfig: async () => ({ youtube_cookie_browser: 'brave' }),
        patchConfig: async () => {
            throw new Error('synthetic private backend detail');
        },
    });
    await view.load();
    els.disconnectBrowserAuthBtn.click();
    await settle();
    assert.equal(els.disconnectBrowserAuthBtn.disabled, false);
    assert.match(els.browserAuthStatus.textContent, /could not be saved/);
    assert.doesNotMatch(els.browserAuthStatus.textContent, /private/);
});

test('load failure leaves sync disabled and permits retry', async () => {
    let attempt = 0;
    const { els, view } = setup({
        getConfig: async () => {
            if (!attempt++) throw new Error('unavailable');
            return {};
        },
    });
    await view.load();
    assert.equal(view.isLoaded(), false);
    assert.equal(els.connectBrowserAuthBtn.disabled, true);
    assert.match(els.browserAuthStatus.textContent, /Reopen Settings/);
    await view.load();
    assert.equal(view.isLoaded(), true);
    assert.equal(els.connectBrowserAuthBtn.disabled, false);
});

test('uses loaded Settings config without another read or an access check', async () => {
    const { els, view } = setup({
        getConfig: () => assert.fail('Settings config is already loaded'),
        checkYoutubeCookieAccess: () => assert.fail('Restoring a browser must not read cookies'),
    });
    await view.load({ youtube_cookie_browser: 'chrome' });
    assert.equal(view.isLoaded(), true);
    assert.equal(els.browserAuthBrowser.value, 'chrome');
    assert.equal(els.disconnectBrowserAuthBtn.disabled, false);
});

test('unavailable native API keeps authentication disabled and sends no requests', async () => {
    const { els, view } = setup({
        available: false,
        getConfig: async () => ({}),
        checkYoutubeCookieAccess: () => assert.fail('Native API is unavailable'),
        patchConfig: () => assert.fail('Native API is unavailable'),
    });
    await view.load();
    els.connectBrowserAuthBtn.click();
    await settle();
    assert.equal(els.connectBrowserAuthBtn.disabled, true);
});

test('enable waits for access approval, prevents duplicate checks, and preserves saved settings on failure', async () => {
    let rejectCheck;
    let checks = 0;
    let saves = 0;
    const { els, view } = setup({
        getConfig: async () => ({ youtube_cookie_browser: 'firefox' }),
        checkYoutubeCookieAccess: () => {
            checks++;
            return new Promise((resolve, reject) => {
                rejectCheck = reject;
            });
        },
        patchConfig: async () => {
            saves++;
            return {};
        },
    });
    await view.load();
    els.browserAuthBrowser.value = 'brave';
    els.connectBrowserAuthBtn.click();
    els.connectBrowserAuthBtn.click();
    assert.equal(checks, 1);
    assert.equal(saves, 0);
    assert.equal(els.browserAuthBrowser.disabled, true);
    assert.match(els.browserAuthStatus.textContent, /Checking browser access/);
    rejectCheck(new Error('synthetic private error'));
    await settle();
    assert.equal(saves, 0);
    assert.equal(els.disconnectBrowserAuthBtn.disabled, false);
    assert.equal(els.browserAuthBrowser.disabled, false);
    assert.match(els.browserAuthStatus.textContent, /saved connection has not changed/);
    assert.doesNotMatch(els.browserAuthStatus.textContent, /private/);
});

test('permission guidance matches the operating system', () => {
    assert.match(browserPermissionHelp('MacIntel'), /Always Allow/);
    assert.match(browserPermissionHelp('macOS'), /Keychain/);
    assert.match(browserPermissionHelp('Win32'), /Windows:/);
    assert.match(browserPermissionHelp('Windows'), /Firefox/);
    assert.doesNotMatch(browserPermissionHelp('Windows'), /Mac login/);
    assert.match(browserPermissionHelp('Linux x86_64'), /keyring or wallet/);
    assert.match(browserPermissionHelp('unknown'), /Your system/);
});
