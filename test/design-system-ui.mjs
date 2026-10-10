import assert from 'node:assert/strict';
import { once } from 'node:events';
import { readFile } from 'node:fs/promises';
import { chromium, webkit } from 'playwright';
import { createDevServer } from '../scripts/dev-server.mjs';
import { installScreenshotBridge } from '../scripts/screenshot-fixtures.mjs';

// Exercise the production markup and modules with isolated native API responses.
// No real downloads, history deletion, settings changes or external requests.
const { version } = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'));
const metadata = JSON.parse(await readFile(new URL('./fixtures/screenshots/videos.json', import.meta.url), 'utf8'));
const videos = await Promise.all(
    metadata.slice(0, 2).map(async video => ({
        ...video,
        duration: 510,
        thumbnail: `data:image/jpeg;base64,${(
            await readFile(new URL(`./fixtures/screenshots/${video.thumbnail}`, import.meta.url))
        ).toString('base64')}`,
    }))
);

const installUiResponses = () => {
    const invoke = globalThis.__TAURI__.core.invoke;
    const listen = globalThis.__TAURI__.event.listen;
    const listeners = new Map();
    let config;
    let historySources = ['youtube', 'tiktok'];
    let historyCleared = false;
    let releasePresets;
    const presetsReady = new Promise(resolve => {
        releasePresets = resolve;
    });
    const ui = {
        commands: [],
        copied: '',
        failNextModelSave: false,
        releasePresets,
        setHistorySources: sources => {
            historySources = [...sources];
        },
        emit: (name, payload) => {
            for (const handler of listeners.get(name) || []) handler({ payload });
        },
    };
    globalThis.__pinefetchUi = ui;
    Object.defineProperty(globalThis.navigator, 'clipboard', {
        value: {
            writeText: async text => {
                ui.copied = text;
            },
        },
    });
    globalThis.__TAURI__.event.listen = async (name, handler) => {
        if (!listeners.has(name)) listeners.set(name, new Set());
        listeners.get(name).add(handler);
        return listen(name, handler);
    };
    globalThis.__TAURI__.core.invoke = async (command, args = {}) => {
        ui.commands.push({ command, args });
        if (command === 'get_download_presets') await presetsReady;
        if (command === 'get_config') {
            config ||= {
                ...(await invoke(command, args)),
                selected_preset_key: 'audio_opus',
                faster_whisper_model: 'medium',
            };
            return { ...config };
        }
        if (command === 'patch_config') {
            if (ui.failNextModelSave && Object.hasOwn(args.changes, 'faster_whisper_model')) {
                ui.failNextModelSave = false;
                throw new Error('Fixture model save failed.');
            }
            config = { ...config, ...args.changes };
            return { ...config };
        }
        if (command === 'get_history_stats') {
            if (historyCleared) {
                return { video_count: 0, total_file_size_bytes: 0, total_duration_seconds: 0, source_counts: [] };
            }
            const stats = await invoke(command, args);
            return { ...stats, source_counts: historySources.map(source => ({ source, count: 1 })) };
        }
        if (command === 'get_history' && historyCleared) return { entries: [], has_more: false };
        if (command === 'get_history_details') {
            const details = await invoke(command, args);
            if (args.id === 'history-2') return { ...details, transcript: null, captions: [] };
            return details;
        }
        if (command === 'get_history_transcript') {
            return {
                language: 'de',
                transcription_type: 'text',
                file_available: true,
                text: 'Transcript <img src=x onerror="alert(1)"> stays plain text.',
            };
        }
        if (command === 'get_history_caption') {
            return { format: 'txt', file_available: true, text: 'Caption content.' };
        }
        if (command === 'clear_history') {
            historyCleared = true;
            historySources = [];
            return true;
        }
        if (command === 'open_file_path') return true;
        if (command === 'revoke_link_dump_secret' || command === 'delete_link_dump_secret') return [];
        return invoke(command, args);
    };
};

const server = createDevServer();
server.listen(0, '127.0.0.1');
await once(server, 'listening');
const origin = `http://127.0.0.1:${server.address().port}`;
try {
    for (const engine of [chromium, webkit]) {
        console.log(`[test:ui] Checking ${engine.name()}...`);
        const browser = await engine.launch();
        try {
            const context = await browser.newContext({
                viewport: { width: 1400, height: 960 },
                timezoneId: 'Europe/Vienna',
                reducedMotion: 'reduce',
            });
            await context.route('**/*', route => {
                const url = route.request().url();
                if (new URL(url).origin === origin) return route.continue();
                if (new URL(url).hostname === 'i.ytimg.com' && route.request().resourceType() === 'image') {
                    return route.fulfill({
                        contentType: 'image/jpeg',
                        body: Buffer.from(videos[0].thumbnail.split(',')[1], 'base64'),
                    });
                }
                if (url === 'https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest') {
                    return route.fulfill({ json: { tag_name: '2026.09.19' } });
                }
                return route.abort('blockedbyclient');
            });
            await context.addInitScript({
                content: `(${installScreenshotBridge.toString()})(${JSON.stringify({ videos, version })}); (${installUiResponses.toString()})();`,
            });
            const page = await context.newPage();
            const errors = [];
            const loadedFonts = [];
            page.on('pageerror', error => errors.push(error.message));
            page.on('console', message => {
                if (message.type() === 'error') errors.push(message.text());
            });
            page.on('response', response => {
                if (response.url().endsWith('.woff2')) {
                    assert.equal(response.status(), 200, response.url());
                    assert(response.url().startsWith(`${origin}/vendor/oj/assets/`));
                    loadedFonts.push(response.url());
                }
            });
            await page.goto(origin);
            const presetTrigger = page.locator('#presetTrigger');
            const presetMenu = page.locator('#presetMenu');
            assert(await presetTrigger.isDisabled());
            assert(await page.locator('#startDownloadBtn').isDisabled());
            await page.evaluate(() => globalThis.__pinefetchUi.releasePresets());
            await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
            await page.evaluate(() => globalThis.document.fonts.ready);
            assert.match(
                await page.locator('body').evaluate(node => globalThis.getComputedStyle(node).fontFamily),
                /Comfortaa/
            );
            assert(await page.evaluate(() => globalThis.document.fonts.check('14px Comfortaa')));
            assert.match(
                await page
                    .locator('#viewSettingsBtn i')
                    .evaluate(node => globalThis.getComputedStyle(node, '::before').content),
                /[^"\s]/
            );
            await page.locator('#magicImportTrigger').focus();
            await page.locator('.oj-tooltip').waitFor();
            assert(await page.locator('#magicImportTrigger').getAttribute('aria-describedby'));
            await page.keyboard.press('Escape');
            await page.locator('.oj-tooltip').waitFor({ state: 'hidden' });

            // Saved format restoration and the library's radio-menu keyboard contract.
            assert.equal(await presetTrigger.isDisabled(), false);
            assert.equal(await page.locator('#presetValue').textContent(), 'Audio only (opus)');
            assert.equal(await page.locator('#presetSelect').count(), 0);
            assert.equal(await presetMenu.getByRole('menuitemradio', { includeHidden: true }).count(), 6);
            assert.equal(await presetMenu.locator('[aria-checked="true"]').count(), 1);
            assert.equal(await presetMenu.locator('[data-oj-value="audio_opus"]').getAttribute('aria-checked'), 'true');
            assert.equal(await page.locator('#transcriptionOptions').isVisible(), false);
            const assertPresetFocus = async key => {
                await page.waitForFunction(
                    value =>
                        globalThis.document.querySelector(`#presetMenu [data-oj-value="${value}"]`) ===
                        globalThis.document.activeElement,
                    key
                );
            };
            await presetTrigger.focus();
            await page.keyboard.press('ArrowDown');
            await presetMenu.waitFor();
            const triggerBox = await presetTrigger.boundingBox();
            const menuBox = await presetMenu.boundingBox();
            assert(Math.abs(menuBox.x - triggerBox.x) < 1);
            assert(menuBox.width <= triggerBox.width);
            assert.equal(await presetTrigger.getAttribute('aria-expanded'), 'true');
            await assertPresetFocus('best');
            await page.keyboard.press('ArrowDown');
            await assertPresetFocus('1080');
            await page.keyboard.press('End');
            await assertPresetFocus('text_timestamps');
            await page.keyboard.press('Home');
            await assertPresetFocus('best');
            await page.keyboard.press('ArrowUp');
            await assertPresetFocus('text_timestamps');
            await page.keyboard.press('t');
            await assertPresetFocus('text');
            await page.keyboard.press('t');
            await assertPresetFocus('text_timestamps');
            await page.keyboard.press('Escape');
            await presetMenu.waitFor({ state: 'hidden' });
            assert(await presetTrigger.evaluate(node => node === globalThis.document.activeElement));
            assert.equal(await presetTrigger.getAttribute('aria-expanded'), 'false');
            await page.keyboard.press('ArrowUp');
            await presetMenu.waitFor();
            await assertPresetFocus('text_timestamps');
            await page.keyboard.press('Escape');
            await presetMenu.waitFor({ state: 'hidden' });
            await presetTrigger.click();
            await presetMenu.waitFor();
            await page.locator('#urlInput').click();
            await presetMenu.waitFor({ state: 'hidden' });
            assert.equal(await presetTrigger.getAttribute('aria-expanded'), 'false');
            assert.equal(await page.locator('#presetValue').textContent(), 'Audio only (opus)');
            const selectPreset = async (key, label, transcription, keyboard = false) => {
                await presetTrigger.click();
                await presetMenu.waitFor();
                const item = presetMenu.locator(`[data-oj-value="${key}"]`);
                if (keyboard) {
                    await item.focus();
                    await page.keyboard.press('Enter');
                } else {
                    await item.click();
                }
                await presetMenu.waitFor({ state: 'hidden' });
                await page.waitForFunction(
                    value =>
                        globalThis.__pinefetchUi.commands.some(
                            call => call.command === 'patch_config' && call.args.changes.selected_preset_key === value
                        ),
                    key
                );
                assert.equal(await page.locator('#presetValue').textContent(), label);
                assert.equal(await presetMenu.locator('[aria-checked="true"]').count(), 1);
                assert.equal(await presetMenu.locator(`[data-oj-value="${key}"]`).getAttribute('aria-checked'), 'true');
                assert.equal(await page.locator('#transcriptionOptions').isVisible(), transcription);
                assert.equal(await presetTrigger.getAttribute('aria-expanded'), 'false');
                assert(await presetTrigger.evaluate(node => node === globalThis.document.activeElement));
            };
            await selectPreset('best', 'Best (bestvideo+bestaudio)', false);
            await selectPreset('audio_mp3', 'Audio only (mp3)', false);
            await selectPreset('text', 'Transcribe to text', true, true);
            await selectPreset('text_timestamps', 'Transcribe with timestamps', true);

            // Queue rendering and live native progress updates remain attached to jobs.
            await page.locator('#urlInput').fill(videos[0].url);
            await page.locator('#startDownloadBtn').click();
            await page.locator('.pinefetch-queue-item').waitFor();
            const transcriptRequest = await page.evaluate(
                () => globalThis.__pinefetchUi.commands.find(call => call.command === 'enqueue_download').args.request
            );
            assert.equal(transcriptRequest.format, 'ba/b');
            assert.equal(transcriptRequest.extract_audio, true);
            assert.equal(transcriptRequest.audio_format, 'mp3');
            assert.equal(transcriptRequest.transcribe_text, true);
            assert.equal(transcriptRequest.transcribe_timestamps, true);
            assert.equal(transcriptRequest.filename_suffix, '_timestamps');
            await page.evaluate(() => {
                globalThis.__pinefetchUi.emit('download:state', { id: 'screenshot-1', state: 'downloading' });
                globalThis.__pinefetchUi.emit('download:progress', { id: 'screenshot-1', percent: 42 });
            });
            await page.waitForFunction(() => globalThis.document.querySelector('progress.oj-progress')?.value === 42);
            await page.evaluate(() =>
                globalThis.__pinefetchUi.emit('download:progress', { id: 'screenshot-1', percent: 73 })
            );
            await page.waitForFunction(() => globalThis.document.querySelector('progress.oj-progress')?.value === 73);
            await page.evaluate(() =>
                globalThis.__pinefetchUi.emit('download:state', { id: 'screenshot-1', state: 'transcribing' })
            );
            await page.locator('.oj-progress-indeterminate').waitFor();
            const more = page.locator('.pinefetch-queue-more-btn');
            await more.click();
            await page.locator('#queueContextMenu').waitFor();
            await page.waitForFunction(() =>
                globalThis.document.getElementById('queueContextMenu').contains(globalThis.document.activeElement)
            );
            await page.keyboard.press('End');
            assert.equal(
                await page
                    .locator('#queueContextMenu button:not([hidden]):not(:disabled)')
                    .last()
                    .evaluate(node => node === globalThis.document.activeElement),
                true
            );
            await page.keyboard.press('Escape');
            assert(await more.evaluate(node => node === globalThis.document.activeElement));
            await more.click();
            await page.waitForFunction(() =>
                globalThis.document.getElementById('queueContextMenu').contains(globalThis.document.activeElement)
            );
            await page.keyboard.press('Home');
            await page.keyboard.press('Enter');
            await page.waitForFunction(url => globalThis.__pinefetchUi.copied === url, videos[0].url);
            await page.waitForFunction(
                () =>
                    globalThis.document.querySelector('.pinefetch-queue-more-btn') === globalThis.document.activeElement
            );

            await selectPreset('audio_opus', 'Audio only (opus)', false);
            await page.locator('#urlInput').fill(videos[1].url);
            await page.locator('#startDownloadBtn').click();
            await page.waitForFunction(
                () => globalThis.document.querySelectorAll('.pinefetch-queue-item').length === 2
            );
            const audioRequest = await page.evaluate(
                () =>
                    globalThis.__pinefetchUi.commands.filter(call => call.command === 'enqueue_download').at(-1).args
                        .request
            );
            assert.equal(audioRequest.format, 'ba/b');
            assert.equal(audioRequest.extract_audio, true);
            assert.equal(audioRequest.audio_format, 'opus');
            assert.equal(audioRequest.transcribe_text, false);
            assert.equal(audioRequest.transcribe_timestamps, false);
            assert.equal(audioRequest.filename_suffix, null);

            // History dropdowns preserve filters and clear a source that disappears.
            await page.locator('#viewHistoryBtn').click();
            assert.equal(await page.locator('#viewHistoryBtn').getAttribute('aria-current'), 'page');
            const searchInput = page.locator('#historySearchInput');
            const searchFieldMenu = page.locator('#historySearchFieldMenu');
            const sourceMenu = page.locator('#historySourceMenu');
            await sourceMenu.locator('[data-oj-value="tiktok"]').waitFor({ state: 'attached' });
            assert.equal(await page.locator('#historySearchFieldSelect, #historySourceSelect').count(), 0);
            assert.equal(await page.locator('#historySearchFieldValue').textContent(), 'Title');
            assert.equal(await page.locator('#historySourceValue').textContent(), 'All sources');
            assert.equal(await searchFieldMenu.locator('[aria-checked="true"]').count(), 1);
            assert.equal(await sourceMenu.locator('[aria-checked="true"]').count(), 1);
            assert.deepEqual(
                await sourceMenu
                    .locator('[role="menuitemradio"]')
                    .evaluateAll(nodes => nodes.map(node => node.dataset.ojValue)),
                ['', 'youtube', 'tiktok']
            );
            const assertHistoryFilterFocus = async (menuId, value) => {
                await page.waitForFunction(
                    expected =>
                        globalThis.document.querySelector(`#${expected.menuId} [data-oj-value="${expected.value}"]`) ===
                        globalThis.document.activeElement,
                    { menuId, value }
                );
            };
            for (const [prefix, first, next, last] of [
                ['historySearchField', 'title', 'description', 'transcript'],
                ['historySource', '', 'youtube', 'tiktok'],
            ]) {
                const trigger = page.locator(`#${prefix}Trigger`);
                const menu = page.locator(`#${prefix}Menu`);
                await trigger.focus();
                await page.keyboard.press('ArrowDown');
                await menu.waitFor();
                await assertHistoryFilterFocus(`${prefix}Menu`, first);
                await page.keyboard.press('ArrowDown');
                await assertHistoryFilterFocus(`${prefix}Menu`, next);
                await page.keyboard.press('End');
                await assertHistoryFilterFocus(`${prefix}Menu`, last);
                await page.keyboard.press('Home');
                await assertHistoryFilterFocus(`${prefix}Menu`, first);
                await page.keyboard.press('Escape');
                await menu.waitFor({ state: 'hidden' });
                assert(await trigger.evaluate(node => node === globalThis.document.activeElement));
                assert.equal(await trigger.getAttribute('aria-expanded'), 'false');
                await page.keyboard.press('ArrowUp');
                await menu.waitFor();
                await assertHistoryFilterFocus(`${prefix}Menu`, last);
                await page.keyboard.press('Escape');
                await menu.waitFor({ state: 'hidden' });
                await trigger.click();
                await menu.waitFor();
                await searchInput.click();
                await menu.waitFor({ state: 'hidden' });
                assert.equal(await trigger.getAttribute('aria-expanded'), 'false');
            }
            const historyCommandCount = () => page.evaluate(() => globalThis.__pinefetchUi.commands.length);
            const waitForHistoryRequest = async (after, searchField, source = '', query = '') => {
                await page.waitForFunction(
                    expected =>
                        globalThis.__pinefetchUi.commands
                            .slice(expected.after)
                            .some(
                                call =>
                                    call.command === 'get_history' &&
                                    call.args.searchField === expected.searchField &&
                                    (expected.source
                                        ? call.args.source === expected.source
                                        : !Object.hasOwn(call.args, 'source')) &&
                                    (expected.query
                                        ? call.args.query === expected.query
                                        : !Object.hasOwn(call.args, 'query'))
                            ),
                    { after, searchField, source, query }
                );
            };
            const selectHistoryFilter = async (prefix, value, label, keyboard = false) => {
                const trigger = page.locator(`#${prefix}Trigger`);
                const menu = page.locator(`#${prefix}Menu`);
                await trigger.click();
                await menu.waitFor();
                const item = menu.locator(`[data-oj-value="${value}"]`);
                if (keyboard) {
                    await item.focus();
                    await page.keyboard.press('Enter');
                } else {
                    await item.click();
                }
                await menu.waitFor({ state: 'hidden' });
                assert.equal(await page.locator(`#${prefix}Value`).textContent(), label);
                assert.equal(await menu.locator('[aria-checked="true"]').count(), 1);
                assert.equal(await menu.locator(`[data-oj-value="${value}"]`).getAttribute('aria-checked'), 'true');
                assert(await trigger.evaluate(node => node === globalThis.document.activeElement));
            };
            let beforeHistoryRequest = await historyCommandCount();
            await searchInput.fill('  history query  ');
            await waitForHistoryRequest(beforeHistoryRequest, 'title', '', 'history query');
            beforeHistoryRequest = await historyCommandCount();
            await selectHistoryFilter('historySearchField', 'description', 'Description');
            assert.equal(await searchInput.getAttribute('placeholder'), 'Search descriptions');
            await waitForHistoryRequest(beforeHistoryRequest, 'description', '', 'history query');
            beforeHistoryRequest = await historyCommandCount();
            await selectHistoryFilter('historySearchField', 'transcript', 'Transcript', true);
            assert.equal(await searchInput.getAttribute('placeholder'), 'Search transcripts');
            await waitForHistoryRequest(beforeHistoryRequest, 'transcript', '', 'history query');
            beforeHistoryRequest = await historyCommandCount();
            await selectHistoryFilter('historySource', 'youtube', 'YouTube');
            await waitForHistoryRequest(beforeHistoryRequest, 'transcript', 'youtube', 'history query');
            beforeHistoryRequest = await historyCommandCount();
            await selectHistoryFilter('historySource', '', 'All sources');
            await waitForHistoryRequest(beforeHistoryRequest, 'transcript', '', 'history query');
            beforeHistoryRequest = await historyCommandCount();
            await selectHistoryFilter('historySearchField', 'user', 'User', true);
            assert.equal(await searchInput.getAttribute('placeholder'), 'Search users');
            await waitForHistoryRequest(beforeHistoryRequest, 'user', '', 'history query');
            beforeHistoryRequest = await historyCommandCount();
            await selectHistoryFilter('historySource', 'youtube', 'YouTube');
            await waitForHistoryRequest(beforeHistoryRequest, 'user', 'youtube', 'history query');
            beforeHistoryRequest = await historyCommandCount();
            await selectHistoryFilter('historySource', 'tiktok', 'TikTok', true);
            await waitForHistoryRequest(beforeHistoryRequest, 'user', 'tiktok', 'history query');
            beforeHistoryRequest = await historyCommandCount();
            await page.evaluate(() => {
                globalThis.__pinefetchUi.setHistorySources(['youtube']);
                globalThis.__pinefetchUi.emit('history:changed', {});
            });
            await sourceMenu.locator('[data-oj-value="tiktok"]').waitFor({ state: 'detached' });
            await waitForHistoryRequest(beforeHistoryRequest, 'user', '', 'history query');
            assert.equal(await page.locator('#historySourceValue').textContent(), 'All sources');
            assert.equal(await sourceMenu.locator('[aria-checked="true"]').count(), 1);
            assert.equal(await sourceMenu.locator('[data-oj-value=""]').getAttribute('aria-checked'), 'true');
            beforeHistoryRequest = await historyCommandCount();
            await searchInput.fill('');
            await waitForHistoryRequest(beforeHistoryRequest, 'user');
            beforeHistoryRequest = await historyCommandCount();
            await selectHistoryFilter('historySearchField', 'title', 'Title');
            await waitForHistoryRequest(beforeHistoryRequest, 'title');
            assert.equal(await searchInput.getAttribute('placeholder'), 'Search titles');
            await page.waitForFunction(
                () => globalThis.document.querySelectorAll('.pinefetch-history-item').length === 2
            );

            // Library tabs own roving focus, activation and lazy text fetching.
            await page.locator('.pinefetch-history-open-btn').first().focus();
            await page.keyboard.press('Shift+F10');
            await page.locator('#historyShowMoreDataBtn').click();
            await page.locator('#historyDetailsContent').waitFor();
            await page.locator('#historyOverviewTab').focus();
            await page.keyboard.press('ArrowRight');
            await page.locator('#historyTranscriptText').waitFor();
            assert.equal(await page.locator('#historyTranscriptTab').getAttribute('aria-selected'), 'true');
            assert.equal(await page.locator('#historyTranscriptText img').count(), 0);
            await page.locator('#historyTranscriptCopyBtn').click();
            await page.locator('#historyDetailsDialog .oj-toast').waitFor();
            assert.match(await page.evaluate(() => globalThis.__pinefetchUi.copied), /^Transcript <img/);
            await page.locator('#historyTranscriptTab').focus();
            await page.keyboard.press('End');
            await page.locator('#historyCaptionText').waitFor();
            await page.keyboard.press('Home');
            await page.keyboard.press('ArrowRight');
            assert.equal(
                await page.evaluate(
                    () =>
                        globalThis.__pinefetchUi.commands.filter(call => call.command === 'get_history_transcript')
                            .length
                ),
                1
            );
            await page.keyboard.press('Escape');
            await page.locator('#historyDetailsDialog').waitFor({ state: 'hidden' });
            await page.waitForFunction(
                () =>
                    globalThis.document.querySelector('.pinefetch-history-open-btn') ===
                    globalThis.document.activeElement
            );
            await page.locator('.pinefetch-history-open-btn').nth(1).focus();
            await page.keyboard.press('Shift+F10');
            await page.locator('#historyShowMoreDataBtn').click();
            await page.locator('#historyDetailsContent').waitFor();
            assert.equal(await page.locator('#historyOverviewTab').getAttribute('aria-selected'), 'true');
            assert(await page.locator('#historyTranscriptTab').isDisabled());
            assert.equal(await page.locator('#historyTranscriptTab').isVisible(), false);
            await page.locator('#historyDetailsCloseBtn').click();
            await page.locator('#historyDetailsDialog').waitFor({ state: 'hidden' });

            // Confirmation defaults to Cancel; cancellation never invokes deletion.
            await page.locator('#clearHistoryBtn').click();
            const confirmation = page.locator('dialog.oj-dialog[open]');
            await confirmation.waitFor();
            assert.equal(await page.evaluate(() => globalThis.document.activeElement.textContent), 'Cancel');
            await page.keyboard.press('Escape');
            await confirmation.waitFor({ state: 'hidden' });
            assert.equal(
                await page.evaluate(
                    () => globalThis.__pinefetchUi.commands.filter(call => call.command === 'clear_history').length
                ),
                0
            );
            await page.waitForFunction(
                () => globalThis.document.getElementById('clearHistoryBtn') === globalThis.document.activeElement
            );

            // Confirmed clearing resets both controls, the query and subsequent requests.
            beforeHistoryRequest = await historyCommandCount();
            await selectHistoryFilter('historySearchField', 'description', 'Description');
            await waitForHistoryRequest(beforeHistoryRequest, 'description');
            beforeHistoryRequest = await historyCommandCount();
            await selectHistoryFilter('historySource', 'youtube', 'YouTube');
            await waitForHistoryRequest(beforeHistoryRequest, 'description', 'youtube');
            beforeHistoryRequest = await historyCommandCount();
            await searchInput.fill('clear this search');
            await waitForHistoryRequest(beforeHistoryRequest, 'description', 'youtube', 'clear this search');
            beforeHistoryRequest = await historyCommandCount();
            await page.locator('#clearHistoryBtn').click();
            await page
                .locator('dialog.oj-dialog[open]')
                .getByRole('button', { name: 'Clear history', exact: true })
                .click();
            await page.waitForFunction(() => !globalThis.document.querySelector('dialog[open]'));
            await waitForHistoryRequest(beforeHistoryRequest, 'title');
            assert.equal(await searchInput.inputValue(), '');
            assert.equal(await searchInput.getAttribute('placeholder'), 'Search titles');
            assert.equal(await page.locator('#historySearchFieldValue').textContent(), 'Title');
            assert.equal(await page.locator('#historySourceValue').textContent(), 'All sources');
            assert.equal(await searchFieldMenu.locator('[aria-checked="true"]').count(), 1);
            assert.equal(await searchFieldMenu.locator('[data-oj-value="title"]').getAttribute('aria-checked'), 'true');
            assert.equal(await sourceMenu.locator('[aria-checked="true"]').count(), 1);
            assert.equal(await sourceMenu.locator('[data-oj-value=""]').getAttribute('aria-checked'), 'true');
            assert.equal(
                await page.evaluate(
                    () => globalThis.__pinefetchUi.commands.filter(call => call.command === 'clear_history').length
                ),
                1
            );

            await page.locator('#viewLinkDumpBtn').click();
            await page.locator('.pinefetch-link-dump-secret-item').waitFor();
            const revoke = page.getByRole('button', { name: 'Revoke', exact: true });
            await revoke.evaluate(node => {
                node.click();
                node.click();
            });
            await page.locator('dialog.oj-dialog[open]').waitFor();
            assert.equal(await page.locator('dialog.oj-dialog[open]').count(), 1);
            await page.keyboard.press('Escape');
            await page.waitForFunction(() => !globalThis.document.querySelector('dialog[open]'));
            assert.equal(
                await page.evaluate(
                    () =>
                        globalThis.__pinefetchUi.commands.filter(call => call.command === 'revoke_link_dump_secret')
                            .length
                ),
                0
            );
            await page.getByRole('button', { name: 'Delete', exact: true }).click();
            await page.locator('dialog.oj-dialog[open]').getByRole('button', { name: 'Delete', exact: true }).click();
            await page.locator('.pinefetch-link-dump-secret-item').waitFor({ state: 'hidden' });
            assert.equal(
                await page.evaluate(
                    () =>
                        globalThis.__pinefetchUi.commands.filter(call => call.command === 'delete_link_dump_secret')
                            .length
                ),
                1
            );

            await page.locator('#viewSettingsBtn').click();
            const modelTrigger = page.locator('#fasterWhisperModelTrigger');
            const modelMenu = page.locator('#fasterWhisperModelMenu');
            const modelValue = page.locator('#fasterWhisperModelValue');
            const saveStatus = page.locator('#settingsSaveStatus');
            assert.equal(await page.locator('#fasterWhisperModel').count(), 0);
            assert.equal(await modelValue.textContent(), 'High (medium)');
            assert.deepEqual(
                await modelMenu
                    .locator('[role="menuitemradio"]')
                    .evaluateAll(nodes =>
                        nodes.map(node => ({ value: node.dataset.ojValue, label: node.textContent.trim() }))
                    ),
                [
                    { value: 'base', label: 'Fast (base)' },
                    { value: 'small', label: 'Balanced (small)' },
                    { value: 'medium', label: 'High (medium)' },
                    { value: 'large-v3', label: 'Best (large-v3)' },
                ]
            );
            assert.equal(await modelMenu.locator('[aria-checked="true"]').count(), 1);
            assert.equal(await modelMenu.locator('[data-oj-value="medium"]').getAttribute('aria-checked'), 'true');
            await modelTrigger.focus();
            await page.keyboard.press('ArrowDown');
            await modelMenu.waitFor();
            await assertHistoryFilterFocus('fasterWhisperModelMenu', 'base');
            await page.keyboard.press('ArrowDown');
            await assertHistoryFilterFocus('fasterWhisperModelMenu', 'small');
            await page.keyboard.press('End');
            await assertHistoryFilterFocus('fasterWhisperModelMenu', 'large-v3');
            await page.keyboard.press('Home');
            await assertHistoryFilterFocus('fasterWhisperModelMenu', 'base');
            await page.keyboard.press('ArrowUp');
            await assertHistoryFilterFocus('fasterWhisperModelMenu', 'large-v3');
            await page.keyboard.press('h');
            await assertHistoryFilterFocus('fasterWhisperModelMenu', 'medium');
            await page.keyboard.press('Escape');
            await modelMenu.waitFor({ state: 'hidden' });
            assert(await modelTrigger.evaluate(node => node === globalThis.document.activeElement));
            assert.equal(await modelTrigger.getAttribute('aria-expanded'), 'false');
            assert.equal(await modelValue.textContent(), 'High (medium)');
            await page.keyboard.press('ArrowUp');
            await modelMenu.waitFor();
            await assertHistoryFilterFocus('fasterWhisperModelMenu', 'large-v3');
            await page.keyboard.press('Escape');
            await modelMenu.waitFor({ state: 'hidden' });
            await modelTrigger.click();
            await modelMenu.waitFor();
            await page.locator('#ytDlpPath').click();
            await modelMenu.waitFor({ state: 'hidden' });
            assert.equal(await modelTrigger.getAttribute('aria-expanded'), 'false');
            const waitForModelPatch = async (after, value) => {
                await page.waitForFunction(
                    expected =>
                        globalThis.__pinefetchUi.commands
                            .slice(expected.after)
                            .some(
                                call =>
                                    call.command === 'patch_config' &&
                                    Object.keys(call.args.changes).length === 1 &&
                                    call.args.changes.faster_whisper_model === expected.value
                            ),
                    { after, value }
                );
            };
            const selectModel = async (value, label, keyboard = false) => {
                const before = await historyCommandCount();
                await modelTrigger.click();
                await modelMenu.waitFor();
                const item = modelMenu.locator(`[data-oj-value="${value}"]`);
                if (keyboard) {
                    await item.focus();
                    await page.keyboard.press('Enter');
                } else {
                    await item.click();
                }
                await modelMenu.waitFor({ state: 'hidden' });
                await waitForModelPatch(before, value);
                await page.waitForFunction(
                    () =>
                        globalThis.document.getElementById('settingsSaveStatus').textContent ===
                        'Changes saved. They apply to new downloads.'
                );
                assert.equal(await modelValue.textContent(), label);
                assert.equal(await modelMenu.locator('[aria-checked="true"]').count(), 1);
                assert.equal(await item.getAttribute('aria-checked'), 'true');
                assert(await modelTrigger.evaluate(node => node === globalThis.document.activeElement));
            };
            await selectModel('base', 'Fast (base)');
            await selectModel('small', 'Balanced (small)', true);
            await selectModel('large-v3', 'Best (large-v3)');

            // A failed model save restores the persisted model and permits a retry.
            const beforeModelFailure = await historyCommandCount();
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failNextModelSave = true;
            });
            await modelTrigger.click();
            await modelMenu.locator('[data-oj-value="medium"]').click();
            await modelMenu.waitFor({ state: 'hidden' });
            await waitForModelPatch(beforeModelFailure, 'medium');
            await page.waitForFunction(() =>
                globalThis.document
                    .getElementById('settingsSaveStatus')
                    .textContent.includes('Fixture model save failed.')
            );
            assert.match(await saveStatus.getAttribute('class'), /oj-status-error/);
            assert.equal(await modelValue.textContent(), 'Best (large-v3)');
            assert.equal(await modelMenu.locator('[aria-checked="true"]').count(), 1);
            assert.equal(await modelMenu.locator('[data-oj-value="large-v3"]').getAttribute('aria-checked'), 'true');
            assert(
                await page.evaluate(
                    after => globalThis.__pinefetchUi.commands.slice(after).some(call => call.command === 'get_config'),
                    beforeModelFailure
                )
            );
            await selectModel('medium', 'High (medium)', true);
            assert.equal(await saveStatus.evaluate(node => node.classList.contains('oj-status-error')), false);

            const setting = page.locator('#saveCaptions');
            await setting.focus();
            await page.keyboard.press('Space');
            await page.waitForFunction(() =>
                globalThis.__pinefetchUi.commands.some(
                    call => call.command === 'patch_config' && call.args.changes.save_captions === false
                )
            );
            await page.waitForFunction(() => !globalThis.document.getElementById('saveCaptions').disabled);
            assert.equal(await setting.isChecked(), false);

            // Verify desktop minimum size and compact browser layout after font loading.
            for (const width of [1000, 520]) {
                await page.setViewportSize({ width, height: 960 });
                assert(
                    await page.evaluate(() => globalThis.document.documentElement.scrollWidth <= globalThis.innerWidth)
                );
            }
            assert(loadedFonts.some(url => url.includes('comfortaa')));
            assert(loadedFonts.some(url => url.includes('jetbrains')));
            assert(loadedFonts.some(url => url.includes('fa-solid')));
            assert.deepEqual(errors, []);
            console.log(
                `[test:ui] ${engine.name()}: offline assets, preset restoration/keyboard/persistence, queue requests/progress, history filters/reset, transcription model/save rollback, menus, tabs, dialog focus, confirmations and switches passed.`
            );
        } finally {
            await browser.close();
        }
    }
} finally {
    await new Promise(resolve => server.close(resolve));
}
