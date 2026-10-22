import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createYoutubeCookieSync, cookiePermissionHelp } from '../src/youtube-cookie-sync.js';

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
        ['youtubeCookieBrowser', 'syncYoutubeCookiesBtn', 'disconnectYoutubeCookiesBtn', 'youtubeCookieStatus'].map(
            key => [key, element()]
        )
    );
    const view = createYoutubeCookieSync({ els, api: { available: true, ...api } });
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
    assert.equal(els.youtubeCookieBrowser.value, 'firefox');
    assert.deepEqual(checks, []);
    assert.equal(els.disconnectYoutubeCookiesBtn.disabled, false);
    els.youtubeCookieBrowser.value = 'brave';
    els.syncYoutubeCookiesBtn.click();
    assert.equal(els.syncYoutubeCookiesBtn.disabled, true);
    await settle();
    assert.deepEqual(calls, [{ youtube_cookie_browser: 'brave' }]);
    assert.deepEqual(checks, [{ browser: 'brave' }]);
    assert.match(els.youtubeCookieStatus.textContent, /next YouTube request/);
    els.disconnectYoutubeCookiesBtn.click();
    await settle();
    assert.equal(calls[1].youtube_cookie_browser, null);
    assert.equal(checks.length, 1);
    assert.equal(els.disconnectYoutubeCookiesBtn.disabled, true);
    assert.equal(els.youtubeCookieStatus.textContent, 'Cookie sync is off.');
});

test('failed save preserves the connection and never displays backend details', async () => {
    const { els, view } = setup({
        getConfig: async () => ({ youtube_cookie_browser: 'brave' }),
        patchConfig: async () => {
            throw new Error('synthetic private backend detail');
        },
    });
    await view.load();
    els.disconnectYoutubeCookiesBtn.click();
    await settle();
    assert.equal(els.disconnectYoutubeCookiesBtn.disabled, false);
    assert.match(els.youtubeCookieStatus.textContent, /could not be saved/);
    assert.doesNotMatch(els.youtubeCookieStatus.textContent, /private/);
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
    assert.equal(els.syncYoutubeCookiesBtn.disabled, true);
    await view.load();
    assert.equal(view.isLoaded(), true);
    assert.equal(els.syncYoutubeCookiesBtn.disabled, false);
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
    els.youtubeCookieBrowser.value = 'brave';
    els.syncYoutubeCookiesBtn.click();
    els.syncYoutubeCookiesBtn.click();
    assert.equal(checks, 1);
    assert.equal(saves, 0);
    assert.equal(els.youtubeCookieBrowser.disabled, true);
    assert.match(els.youtubeCookieStatus.textContent, /Checking browser-cookie access/);
    rejectCheck(new Error('synthetic private error'));
    await settle();
    assert.equal(saves, 0);
    assert.equal(els.disconnectYoutubeCookiesBtn.disabled, false);
    assert.equal(els.youtubeCookieBrowser.disabled, false);
    assert.match(els.youtubeCookieStatus.textContent, /saved connection has not changed/);
    assert.doesNotMatch(els.youtubeCookieStatus.textContent, /private/);
});

test('permission guidance matches the operating system', () => {
    assert.match(cookiePermissionHelp('MacIntel'), /Always Allow/);
    assert.match(cookiePermissionHelp('macOS'), /Keychain/);
    assert.match(cookiePermissionHelp('Win32'), /Windows:/);
    assert.match(cookiePermissionHelp('Windows'), /Firefox/);
    assert.doesNotMatch(cookiePermissionHelp('Windows'), /Mac login/);
    assert.match(cookiePermissionHelp('Linux x86_64'), /keyring or wallet/);
    assert.match(cookiePermissionHelp('unknown'), /Your system/);
});
