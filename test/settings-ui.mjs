import assert from 'node:assert/strict';
import { once } from 'node:events';
import { mkdir, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { chromium, webkit } from 'playwright';
import { createDevServer } from '../scripts/dev-server.mjs';
import { installScreenshotBridge } from '../scripts/screenshot-fixtures.mjs';

// Real Settings markup/modules; isolated native responses and saved fixture data.
// Reload proves UI restoration, not a native restart or real cookie permissions.
const { version } = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'));
const artifacts = process.env.PINEFETCH_SETTINGS_ARTIFACTS;
if (artifacts) await mkdir(artifacts, { recursive: true });
const installSettingsFixture = () => {
    const original = globalThis.__TAURI__.core.invoke;
    const originalListen = globalThis.__TAURI__.event.listen;
    const listeners = new Map();
    globalThis.__TAURI__.event.listen = async (name, handler) => {
        if (!listeners.has(name)) listeners.set(name, new Set());
        listeners.get(name).add(handler);
        const stop = await originalListen(name, handler);
        return () => {
            listeners.get(name).delete(handler);
            stop();
        };
    };
    Object.defineProperty(globalThis.navigator, 'clipboard', {
        value: {
            writeText: async text => {
                ui.copied = text;
            },
        },
        configurable: true,
    });
    Object.defineProperty(globalThis.navigator, 'platform', { value: 'MacIntel', configurable: true });
    let saved = JSON.parse(sessionStorage.getItem('settings-fixture') || 'null') || {
        selected_preset_key: 'best',
        default_output_dir: '/fixture/Downloads',
        yt_dlp_path: '',
        faster_whisper_model: 'medium',
        id_import_enabled: true,
        magic_import_enabled: false,
        cut_at_timestamp_enabled: false,
        notifications_enabled: true,
        save_captions: true,
        save_creator_subtitles: true,
        save_thumbnails: false,
        download_video_with_transcript: true,
        youtube_cookie_browser: 'firefox',
    };
    const ui = {
        calls: [],
        holdSave: false,
        failSave: false,
        releaseSave: null,
        holdAccess: false,
        failAccess: false,
        releaseAccess: null,
        persisted: () => structuredClone(saved),
        emit: (name, payload) => {
            for (const handler of listeners.get(name) || []) handler({ payload });
        },
        copied: '',
    };
    globalThis.__settingsUi = ui;
    globalThis.__TAURI__.core.invoke = async (command, args = {}) => {
        ui.calls.push({ command, args: structuredClone(args) });
        if (command === 'get_config') return structuredClone(saved);
        if (command === 'patch_config') {
            const fail = ui.failSave;
            ui.failSave = false;
            if (ui.holdSave) {
                ui.holdSave = false;
                await new Promise(resolve => {
                    ui.releaseSave = resolve;
                });
                ui.releaseSave = null;
            }
            if (fail) throw new Error('Fixture persistence failure');
            saved = { ...saved, ...args.changes };
            sessionStorage.setItem('settings-fixture', JSON.stringify(saved));
            return structuredClone(saved);
        }
        if (command === 'check_youtube_cookie_access') {
            if (ui.holdAccess) {
                ui.holdAccess = false;
                await new Promise(resolve => {
                    ui.releaseAccess = resolve;
                });
                ui.releaseAccess = null;
            }
            if (ui.failAccess) throw new Error('Synthetic private cookie diagnostic');
            return;
        }
        return original(command, args);
    };
};
const server = createDevServer();
server.listen(0, '127.0.0.1');
await once(server, 'listening');
const origin = 'http://127.0.0.1:' + server.address().port;
try {
    for (const engine of [chromium, webkit]) {
        const browser = await engine.launch();
        try {
            const context = await browser.newContext({ viewport: { width: 1400, height: 960 }, deviceScaleFactor: 2 });
            const page = await context.newPage();
            const errors = [];
            page.on('pageerror', error => errors.push(error.message));
            await page.route('**/*', route => {
                const url = route.request().url();
                if (url.startsWith(origin)) return route.continue();
                if (url === 'https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest')
                    return route.fulfill({ json: { tag_name: '2026.10.29' } });
                return route.abort();
            });
            await page.addInitScript({
                content:
                    '(' +
                    installScreenshotBridge.toString() +
                    ')(' +
                    JSON.stringify({ videos: [], version }) +
                    ');(' +
                    installSettingsFixture.toString() +
                    ')();',
            });
            const open = async () => {
                await page.goto(origin);
                await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
                await page.locator('#viewSettingsBtn').click();
                await page.locator('#settingsDownloadsPanel').waitFor();
            };
            const section = name => page.locator('#settings' + name + 'Tab');
            const snapshot = async name => {
                if (artifacts) {
                    await page.mouse.move(1, 1);
                    await page.evaluate(() => globalThis.document.fonts.ready);
                    await page.screenshot({
                        path: join(artifacts, name + '-' + engine.name() + '.png'),
                        fullPage: true,
                    });
                }
            };
            const noOverflow = async () =>
                assert(
                    await page.evaluate(() => globalThis.document.documentElement.scrollWidth <= globalThis.innerWidth)
                );
            const onePanel = async () =>
                assert.equal(await page.locator('.pinefetch-settings-category:visible').count(), 1);
            const persisted = () => page.evaluate(() => globalThis.__settingsUi.persisted());
            await open();
            await page.clock.install();
            assert.equal(await page.locator('#settingsSaveStatus').textContent(), '');
            assert.equal(await section('Downloads').getAttribute('aria-selected'), 'true');
            for (const [id, checked] of [
                ['idImportEnabled', true],
                ['magicImportEnabled', false],
                ['cutAtTimestampEnabled', false],
                ['notificationsEnabled', true],
            ])
                assert.equal(await page.locator('#' + id).isChecked(), checked);
            assert.equal(await page.locator('#fasterWhisperModelValue').textContent(), 'High (medium)');
            const sidebarWidth = (await page.locator('.pinefetch-settings-sidebar').boundingBox()).width;
            assert(sidebarWidth >= 200 && sidebarWidth <= 230);
            await snapshot('settings-downloads');

            // Existing OJ tabs own roving focus, arrows and active panel semantics.
            await section('Downloads').focus();
            await page.keyboard.press('ArrowDown');
            assert.equal(await section('Files').getAttribute('aria-selected'), 'true');
            assert(await section('Files').evaluate(node => node === globalThis.document.activeElement));
            await onePanel();
            for (const [id, checked] of [
                ['saveCaptions', true],
                ['saveCreatorSubtitles', true],
                ['saveThumbnails', false],
            ])
                assert.equal(await page.locator('#' + id).isChecked(), checked);
            await snapshot('settings-files');
            await page.keyboard.press('End');
            assert.equal(await section('Diagnostics').getAttribute('aria-selected'), 'true');
            await page.locator('#ytDlpPath').fill('/fixture/custom-yt-dlp');
            await page.locator('#ytDlpPath').press('Tab');
            await page.waitForFunction(
                () => globalThis.__settingsUi.persisted().yt_dlp_path === '/fixture/custom-yt-dlp'
            );
            assert(await page.locator('#logPanel').isVisible());
            await page.evaluate(() =>
                globalThis.__settingsUi.emit('download:log', {
                    id: 'fixture',
                    line: 'Settings log fixture',
                    is_error: false,
                })
            );
            await page.waitForFunction(() =>
                globalThis.document.getElementById('logBody').textContent.includes('Settings log fixture')
            );
            await page.locator('#copyLogsBtn').click();
            await page.waitForFunction(() => globalThis.__settingsUi.copied.includes('Settings log fixture'));
            await page.locator('#clearLogsBtn').click();
            assert.equal(await page.locator('#logBody').textContent(), '');
            await snapshot('settings-diagnostics');
            await section('Diagnostics').focus();
            await page.keyboard.press('Home');
            assert.equal(await section('Downloads').getAttribute('aria-selected'), 'true');
            assert.notEqual(
                await section('Downloads').evaluate(node => globalThis.getComputedStyle(node).outlineStyle),
                'none'
            );
            await page.keyboard.press('Tab');
            assert(
                await page
                    .locator('#settingsDownloadsPanel')
                    .evaluate(node => node === globalThis.document.activeElement)
            );
            await page.keyboard.press(engine.name() === 'webkit' ? 'Alt+Tab' : 'Tab');
            assert(await page.locator('#idImportEnabled').evaluate(node => node === globalThis.document.activeElement));

            // Nothing is confirmed until the existing queued native save resolves.
            await page.evaluate(() => {
                globalThis.__settingsUi.holdSave = true;
            });
            await page.locator('#idImportEnabled').click();
            await page.waitForFunction(() => typeof globalThis.__settingsUi.releaseSave === 'function');
            assert.match(await page.locator('#settingsSaveStatus').textContent(), /Saving/);
            assert.equal((await persisted()).id_import_enabled, true);
            await section('Files').click();
            await section('Downloads').click();
            assert.equal(await page.locator('#idImportEnabled').isChecked(), false);
            await page.evaluate(() => globalThis.__settingsUi.releaseSave());
            await page.waitForFunction(
                () =>
                    globalThis.document.getElementById('settingsSaveStatus').textContent === 'Saved for new downloads.'
            );
            await page.clock.fastForward(4100);
            assert.equal(await page.locator('#settingsSaveStatus').textContent(), '');

            await page.evaluate(() => {
                globalThis.__settingsUi.failSave = true;
            });
            await page.locator('#magicImportEnabled').click();
            await page.waitForFunction(() =>
                globalThis.document.getElementById('settingsSaveStatus').textContent.includes('Could not save')
            );
            assert.equal(await page.locator('#magicImportEnabled').isChecked(), false);
            await page.clock.fastForward(6000);
            assert.match(await page.locator('#settingsSaveStatus').textContent(), /Could not save/);
            await snapshot('settings-save-error');
            await page.locator('#magicImportEnabled').click();
            await page.waitForFunction(() => globalThis.__settingsUi.persisted().magic_import_enabled);
            await page.evaluate(() => {
                globalThis.__settingsUi.holdSave = true;
            });
            await page.locator('#cutAtTimestampEnabled').click();
            await page.waitForFunction(() => typeof globalThis.__settingsUi.releaseSave === 'function');
            await page.clock.fastForward(5000);
            assert.match(await page.locator('#settingsSaveStatus').textContent(), /Saving/);
            await page.evaluate(() => globalThis.__settingsUi.releaseSave());
            await page.waitForFunction(() => globalThis.__settingsUi.persisted().cut_at_timestamp_enabled);
            await page.clock.fastForward(4100);
            assert.equal(await page.locator('#settingsSaveStatus').textContent(), '');

            await page.locator('#fasterWhisperModelTrigger').click();
            await page.locator('#fasterWhisperModelMenu [data-oj-value="small"]').click();
            await page.waitForFunction(() => globalThis.__settingsUi.persisted().faster_whisper_model === 'small');
            await section('Files').click();
            for (const [id, key, expected] of [
                ['saveCaptions', 'save_captions', false],
                ['saveCreatorSubtitles', 'save_creator_subtitles', false],
                ['saveThumbnails', 'save_thumbnails', true],
            ]) {
                await page.locator('#' + id).click();
                await page.waitForFunction(
                    ({ key, expected }) => globalThis.__settingsUi.persisted()[key] === expected,
                    { key, expected }
                );
            }
            await page.locator('#settingsFilesPanel details').first().locator('summary').click();
            assert.match(await page.locator('#settingsFilesPanel').innerText(), /always saved in History/);
            await section('Downloads').click();
            await page.locator('#notificationsEnabled').click();
            await page.waitForFunction(() => !globalThis.__settingsUi.persisted().notifications_enabled);

            await section('Browser').click();
            assert.equal(await page.locator('#browserAuthState').textContent(), 'Browser saved');
            assert.match(await page.locator('#browserAuthConnection').textContent(), /Firefox/);
            assert.equal(
                await page.evaluate(
                    () =>
                        globalThis.__settingsUi.calls.filter(call => call.command === 'check_youtube_cookie_access')
                            .length
                ),
                0
            );
            assert.equal(await page.locator('.pinefetch-browser-troubleshooting').getAttribute('open'), null);
            assert.match(await page.locator('#settingsBrowserPanel').innerText(), /does not verify/);
            await snapshot('settings-browser');
            await page.locator('#browserAuthBrowser').selectOption('chrome');
            assert.equal(await page.locator('#browserAuthState').textContent(), 'Browser saved');
            await page.evaluate(() => {
                globalThis.__settingsUi.holdAccess = true;
                globalThis.__settingsUi.failAccess = true;
            });
            await page.locator('#connectBrowserAuthBtn').click();
            await page.waitForFunction(() => typeof globalThis.__settingsUi.releaseAccess === 'function');
            assert.equal(await page.locator('#browserAuthState').textContent(), 'Checking…');
            assert(await page.locator('#browserAuthBrowser').isDisabled());
            assert(await page.locator('#disconnectBrowserAuthBtn').isDisabled());
            await page.evaluate(() => globalThis.__settingsUi.releaseAccess());
            await page.waitForFunction(
                () => globalThis.document.getElementById('browserAuthState').textContent === 'Connection failed'
            );
            assert.equal((await persisted()).youtube_cookie_browser, 'firefox');
            assert.doesNotMatch(await page.locator('#browserAuthStatus').textContent(), /private/);
            await page.locator('.pinefetch-browser-troubleshooting summary').click();
            assert.match(await page.locator('#browserAuthHelp').innerText(), /most recently used profile/);
            assert.match(await page.locator('#browserAuthPermissionHelp').innerText(), /Keychain/);
            await snapshot('settings-browser-error');
            await page.evaluate(() => {
                globalThis.__settingsUi.failAccess = false;
                globalThis.__settingsUi.holdSave = true;
            });
            await page.locator('#connectBrowserAuthBtn').click();
            await page.waitForFunction(() => typeof globalThis.__settingsUi.releaseSave === 'function');
            assert.equal(await page.locator('#browserAuthState').textContent(), 'Saving…');
            await page.evaluate(() => globalThis.__settingsUi.releaseSave());
            await page.waitForFunction(
                () => globalThis.document.getElementById('browserAuthState').textContent === 'Connected'
            );
            assert.equal((await persisted()).youtube_cookie_browser, 'chrome');

            // Keep navigation state for this window; restore values after reload.
            await page.locator('#viewDownloadBtn').click();
            await page.locator('#viewSettingsBtn').click();
            assert.equal(await section('Browser').getAttribute('aria-selected'), 'true');
            await page.reload();
            await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
            await page.locator('#viewSettingsBtn').click();
            assert(await page.locator('#settingsDownloadsPanel').isVisible());
            assert.equal(await page.locator('#idImportEnabled').isChecked(), false);
            assert.equal(await page.locator('#magicImportEnabled').isChecked(), true);
            assert.equal(await page.locator('#cutAtTimestampEnabled').isChecked(), true);
            assert.equal(await page.locator('#notificationsEnabled').isChecked(), false);
            assert.equal(await page.locator('#fasterWhisperModelValue').textContent(), 'Balanced (small)');
            await section('Files').click();
            assert.equal(await page.locator('#saveCaptions').isChecked(), false);
            assert.equal(await page.locator('#saveCreatorSubtitles').isChecked(), false);
            assert.equal(await page.locator('#saveThumbnails').isChecked(), true);
            await section('Diagnostics').click();
            assert.equal(await page.locator('#ytDlpPath').inputValue(), '/fixture/custom-yt-dlp');
            await section('Browser').click();
            assert.equal(await page.locator('#browserAuthBrowser').inputValue(), 'chrome');
            assert.equal(await page.locator('#browserAuthState').textContent(), 'Browser saved');
            assert.equal(
                await page.evaluate(
                    () =>
                        globalThis.__settingsUi.calls.filter(call => call.command === 'check_youtube_cookie_access')
                            .length
                ),
                0
            );
            await page.locator('#disconnectBrowserAuthBtn').click();
            await page.waitForFunction(
                () => globalThis.document.getElementById('browserAuthState').textContent === 'Not connected'
            );
            assert.equal((await persisted()).youtube_cookie_browser, null);
            assert(await page.locator('#disconnectBrowserAuthBtn').isDisabled());

            for (const width of [1000, 760, 600, 390]) {
                await page.setViewportSize({ width, height: 900 });
                for (const name of ['Downloads', 'Files', 'Browser', 'Diagnostics']) {
                    await section(name).click();
                    await onePanel();
                    await noOverflow();
                }
                assert.equal(
                    await page.locator('#settingsCategories').getAttribute('aria-orientation'),
                    width <= 680 ? 'horizontal' : 'vertical'
                );
                if (width === 390) {
                    await section('Downloads').focus();
                    await page.keyboard.press('ArrowRight');
                    assert.equal(await section('Files').getAttribute('aria-selected'), 'true');
                    await snapshot('settings-compact');
                }
            }
            assert.deepEqual(errors, []);
            await context.close();
            console.log(
                engine.name() +
                    ': category navigation, persisted values, queued saves/rollback/feedback, browser states, reload and responsive layouts passed'
            );
        } finally {
            await browser.close();
        }
    }
} finally {
    await new Promise(resolve => server.close(resolve));
}
