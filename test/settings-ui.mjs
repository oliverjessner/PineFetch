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
    let importState = JSON.parse(sessionStorage.getItem('import-fixture') || 'null') || {
        settings: { server_enabled: true, host: '127.0.0.1', port: 2255 },
        server_status: { status: 'running', url: 'http://127.0.0.1:2255' },
        secrets: [
            {
                id: 'saved-extension',
                name: 'Saved extension',
                status: 'active',
                created_at: '2026-09-20 10:00:00',
                last_used_at: null,
            },
        ],
    };
    const saveImport = () => sessionStorage.setItem('import-fixture', JSON.stringify(importState));
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
        failOverview: false,
        holdOverview: false,
        releaseOverview: null,
        importState: () => structuredClone(importState),
    };
    globalThis.__settingsUi = ui;
    globalThis.__TAURI__.core.invoke = async (command, args = {}) => {
        ui.calls.push({ command, args: structuredClone(args) });
        if (command === 'get_link_dump_overview') {
            if (ui.failOverview) throw new Error('Fixture import unavailable');
            if (ui.holdOverview) {
                ui.holdOverview = false;
                await new Promise(resolve => {
                    ui.releaseOverview = resolve;
                });
                ui.releaseOverview = null;
            }
            return structuredClone(importState);
        }
        if (command === 'update_link_dump_settings') {
            importState.settings = { ...importState.settings, ...args.patch };
            importState.server_status = {
                status: args.patch.server_enabled ? 'running' : 'stopped',
                url: 'http://127.0.0.1:' + args.patch.port,
            };
            saveImport();
            return structuredClone(importState);
        }
        if (command === 'restart_link_dump_server') return structuredClone(importState.server_status);
        if (command === 'create_link_dump_secret') {
            importState.secrets.push({
                id: 'new-extension',
                name: args.name,
                status: 'active',
                created_at: '2026-10-29 10:00:00',
                last_used_at: null,
            });
            saveImport();
            return { secret: 'fixture-only-secret' };
        }
        if (command === 'revoke_link_dump_secret') {
            importState.secrets = importState.secrets.map(row =>
                row.id === args.id ? { ...row, status: 'revoked' } : row
            );
            saveImport();
            return structuredClone(importState.secrets);
        }
        if (command === 'delete_link_dump_secret') {
            importState.secrets = importState.secrets.filter(row => row.id !== args.id);
            saveImport();
            return structuredClone(importState.secrets);
        }
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
            if (ui.failAccess) throw typeof ui.failAccess === 'string' ? ui.failAccess : 'PERMISSION_DENIED';
            return { public_access: 'checked', authentication: 'not_verified' };
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
            assert.equal(await page.locator('#viewLinkDumpBtn').count(), 0);
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
            const leftBounds = await page.locator('.pinefetch-panel-left').boundingBox();
            const settingsBounds = await page.locator('#settingsView').boundingBox();
            assert(settingsBounds.x - leftBounds.x < 32, 'Settings should start at the panel left padding');
            const track = await page.locator('label[for="idImportEnabled"] .oj-switch-track').boundingBox();
            const copy = await page.locator('#idImportEnabledLabel').boundingBox();
            assert(track.x < copy.x, 'Switches should precede their labels on the left');
            const modelBounds = await page.locator('#fasterWhisperModelTrigger').boundingBox();
            const modelLabel = await page.locator('#fasterWhisperModelLabel').boundingBox();
            assert(
                Math.abs(modelBounds.x - modelLabel.x) < 2,
                'Setting fields should align with the left edge of their labels'
            );
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
            assert.match(await page.locator('#settingsBrowserPanel').innerText(), /YouTube sign-in: Not verified/);
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
                () => globalThis.document.getElementById('browserAuthState').textContent === 'Check failed'
            );
            assert.equal((await persisted()).youtube_cookie_browser, 'firefox');
            assert.match(await page.locator('#browserAuthStatus').textContent(), /system denied access/);
            assert.doesNotMatch(await page.locator('#browserAuthStatus').textContent(), /Synthetic private/);
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
                () => globalThis.document.getElementById('browserAuthState').textContent === 'Public access checked'
            );
            assert.equal((await persisted()).youtube_cookie_browser, 'chrome');
            assert.match(await page.locator('#browserAuthStatus').textContent(), /sign-in is not verified/);
            await snapshot('settings-browser-public-checked');

            // Failures remain actionable at narrow widths and never overwrite the saved browser.
            await page.setViewportSize({ width: 390, height: 900 });
            for (const [code, expected] of [
                ['PROFILE_UNAVAILABLE', /found or read/],
                ['COOKIE_DECRYPTION', /could not be decrypted/],
                ['NETWORK_DNS', /DNS/],
                ['NETWORK_TLS', /clock, certificates/],
                ['CHECK_TIMEOUT', /pending system permission prompt/],
                ['synthetic-private-token /private/profile/Cookies', /could not be completed/],
            ]) {
                await page.evaluate(code => {
                    globalThis.__settingsUi.failAccess = code;
                }, code);
                await page.locator('#browserAuthBrowser').selectOption('brave');
                await page.locator('#connectBrowserAuthBtn').click();
                await page.waitForFunction(
                    () => globalThis.document.getElementById('browserAuthState').textContent === 'Check failed'
                );
                assert.match(await page.locator('#browserAuthStatus').textContent(), expected);
                assert.doesNotMatch(
                    await page.locator('#settingsBrowserPanel').innerText(),
                    /synthetic-private-token|private\/profile/
                );
                assert.equal((await persisted()).youtube_cookie_browser, 'chrome');
                assert.match(await page.locator('#browserAuthConnection').textContent(), /Chrome/);
                await noOverflow();
            }
            await snapshot('settings-browser-compact-error');
            await page.setViewportSize({ width: 1400, height: 960 });
            await page.evaluate(() => {
                globalThis.__settingsUi.failAccess = false;
            });

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

            // Import is part of Settings. Its independent settings/secrets flow
            // remains lazy, cached, retryable and reachable from the Dock route.
            const overviewCalls = () =>
                page.evaluate(
                    () => globalThis.__settingsUi.calls.filter(call => call.command === 'get_link_dump_overview').length
                );
            assert.equal(await overviewCalls(), 0);
            await page.evaluate(() => {
                globalThis.__settingsUi.failOverview = true;
            });
            await section('Import').click();
            await page.waitForFunction(() =>
                globalThis.document.getElementById('linkDumpServerStatusText').textContent.includes('unavailable')
            );
            await section('Downloads').click();
            await page.evaluate(() => {
                globalThis.__settingsUi.failOverview = false;
                globalThis.__settingsUi.holdOverview = true;
            });
            await section('Import').click();
            await page.waitForFunction(() => typeof globalThis.__settingsUi.releaseOverview === 'function');
            await section('Downloads').click();
            await section('Import').click();
            assert.equal(await overviewCalls(), 2);
            await page.evaluate(() => globalThis.__settingsUi.releaseOverview());
            await page.waitForFunction(
                () => globalThis.document.getElementById('linkDumpServerStatusBadge').textContent === 'Running'
            );
            assert(await page.locator('#linkDumpConnectionsPanel').isVisible());
            await snapshot('settings-import');
            const port = page.locator('#linkDumpPort');
            await port.fill('0');
            await page.locator('#saveLinkDumpServerBtn').click();
            await page.waitForFunction(() =>
                globalThis.document
                    .getElementById('linkDumpServerStatusText')
                    .textContent.includes('between 1 and 65535')
            );
            await port.fill('2256');
            await section('Browser').click();
            await section('Import').click();
            assert.equal(await port.inputValue(), '2256');
            assert.equal(await overviewCalls(), 2);
            await page.locator('#saveLinkDumpServerBtn').click();
            await page.waitForFunction(() => globalThis.__settingsUi.importState().settings.port === 2256);
            await page.locator('#restartLinkDumpServerBtn').click();
            await page.locator('#linkDumpSecretName').fill('New extension');
            await page.locator('#generateLinkDumpSecretBtn').click();
            await page.locator('#generatedLinkDumpSecretPanel').waitFor();
            await section('Files').click();
            await section('Import').click();
            assert.equal(await page.locator('#generatedLinkDumpSecret').inputValue(), 'fixture-only-secret');
            await page.locator('#copyGeneratedLinkDumpSecretBtn').click();
            await page.waitForFunction(() => globalThis.__settingsUi.copied === 'fixture-only-secret');
            assert(await page.locator('#generatedLinkDumpSecretPanel').isHidden());
            const connection = page.locator('.pinefetch-link-dump-secret-item').filter({ hasText: 'New extension' });
            await connection.getByRole('button', { name: 'Revoke', exact: true }).click();
            await page.locator('dialog[open]').getByRole('button', { name: 'Revoke', exact: true }).click();
            await page.waitForFunction(
                () =>
                    globalThis.__settingsUi.importState().secrets.find(row => row.id === 'new-extension').status ===
                    'revoked'
            );
            await connection.getByRole('button', { name: 'Delete', exact: true }).click();
            await page.locator('dialog[open]').getByRole('button', { name: 'Delete', exact: true }).click();
            await connection.waitFor({ state: 'hidden' });
            await page.reload();
            await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
            await page.evaluate(() =>
                globalThis.__settingsUi.emit('navigation:requested', { revision: 1, view: 'linkDump' })
            );
            await page.locator('#settingsImportPanel').waitFor();
            await page.waitForFunction(() => globalThis.document.getElementById('linkDumpPort').value === '2256');
            assert.equal(await page.locator('#viewSettingsBtn').getAttribute('aria-current'), 'page');
            assert.equal(await page.locator('.pinefetch-link-dump-secret-item').count(), 1);
            assert.equal(await page.locator('#browserAuthState').textContent(), 'Not connected');

            for (const width of [1000, 760, 600, 390]) {
                await page.setViewportSize({ width, height: 900 });
                for (const name of ['Downloads', 'Files', 'Browser', 'Import', 'Diagnostics']) {
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
