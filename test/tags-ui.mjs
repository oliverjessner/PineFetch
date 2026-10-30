import assert from 'node:assert/strict';
import { once } from 'node:events';
import { readFile } from 'node:fs/promises';
import { chromium, webkit } from 'playwright';
import { createDevServer } from '../scripts/dev-server.mjs';
import { installScreenshotBridge } from '../scripts/screenshot-fixtures.mjs';

// Isolated browser transport. SQLite behavior is independently covered by tags_tests.
const installTagsBridge = () => {
    const fallback = globalThis.__TAURI__.core.invoke;
    const records = Array.from({ length: 60 }, (_, i) => ({
        id: `tag-entry-${i + 1}`,
        url: `https://youtube.com/watch?v=abcdefghijk`,
        title: `Clip ${i + 1}`,
        uploader: i === 59 ? null : `Creator ${i % 4}`,
        source: i % 2 ? 'tiktok' : 'youtube',
        medium: i % 3 === 0 ? 'transcript' : i % 3 === 1 ? 'audio' : 'video',
        created_at: 1700000000000 + i * 1000,
        completed_at: 1700000000000 + i * 1000,
        filename: `clip-${i + 1}.mp4`,
        duration_seconds: 60,
        file_size_bytes: 1024,
        thumbnail: null,
        identity: i > 56 ? null : `user:${i % 4}`,
    }));
    const research = {
        id: '11111111-1111-4111-8111-111111111111',
        name: 'Research',
        description: 'Saved research',
        color: '#35C9A5',
        created_at: 1,
        updated_at: 1,
    };
    const unused = { ...research, id: '22222222-2222-4222-8222-222222222222', name: 'Unused' };
    const definitions = [research, unused];
    const assignments = new Map(
        records.map(row => [row.id, new Set(Number(row.id.split('-').at(-1)) % 2 === 0 ? [research.id] : [])])
    );
    const state = (globalThis.__tagsUi = {
        commands: [],
        definitions,
        assignments,
        records,
        failSave: false,
        failAssignment: false,
        holdAssignment: false,
        holdContent: false,
    });
    const key = s => s.normalize('NFD').toLowerCase().replaceAll('ß', 'ss').normalize('NFC');
    const identity = row => (row.uploader && row.source ? `${row.source}:${row.identity || row.id}` : null);
    const tagged = id => records.filter(row => assignments.get(row.id)?.has(id));
    const summary = tag => ({
        ...tag,
        content_count: tagged(tag.id).length,
        account_count: new Set(tagged(tag.id).map(identity).filter(Boolean)).size,
    });
    const tagsFor = id => definitions.filter(tag => assignments.get(id)?.has(tag.id));
    const page = (rows, args = {}) => {
        const offset = args.cursor ? JSON.parse(args.cursor).offset : args.offset || 0;
        const end = offset + (args.limit || 20);
        return {
            entries: rows.slice(offset, end),
            total_count: rows.length,
            has_more: end < rows.length,
            next_cursor: end < rows.length ? JSON.stringify({ offset: end, tag: args.id || args.tagId }) : null,
        };
    };
    globalThis.__TAURI__.core.invoke = async (command, args = {}) => {
        state.commands.push({ command, args: structuredClone(args) });
        if (command === 'get_tag_choices') return structuredClone(definitions);
        if (command === 'list_tags')
            return definitions.map(summary).sort((a, b) => key(a.name).localeCompare(key(b.name)));
        if (command === 'get_tag') {
            if (state.failTagRead) throw new Error('Fixture tag read failed.');
            const tag = definitions.find(tag => tag.id === args.id);
            if (!tag) throw new Error('This tag no longer exists.');
            return summary(tag);
        }
        if (command === 'create_tag' || command === 'update_tag') {
            if (state.failSave) throw new Error('Fixture tag storage failed.');
            const input = args.tag;
            const name = input.name.trim();
            if (definitions.some(tag => tag.id !== args.id && key(tag.name) === key(name)))
                throw new Error('A tag with this name already exists.');
            if (!/^#[0-9a-f]{6}$/i.test(input.color)) throw new Error('Choose a valid HEX color.');
            const tag =
                command === 'create_tag'
                    ? { id: crypto.randomUUID(), created_at: 1 }
                    : definitions.find(tag => tag.id === args.id);
            Object.assign(tag, {
                name,
                description: input.description.trim(),
                color: input.color.toUpperCase(),
                updated_at: 2,
            });
            if (command === 'create_tag') definitions.push(tag);
            if (command === 'create_tag' && state.holdCreation) {
                state.holdCreation = false;
                await new Promise(resolve => {
                    state.releaseCreation = resolve;
                });
            }
            return structuredClone(tag);
        }
        if (command === 'delete_tag') {
            const index = definitions.findIndex(tag => tag.id === args.id);
            if (index < 0) throw new Error('This tag no longer exists.');
            definitions.splice(index, 1);
            for (const tags of assignments.values()) tags.delete(args.id);
            return;
        }
        if (command === 'get_history_tags') {
            const result = structuredClone(tagsFor(args.id));
            if (state.holdTagsRead) {
                state.holdTagsRead = false;
                return new Promise(resolve => {
                    state.releaseTagsRead = () => resolve(result);
                });
            }
            return result;
        }
        if (command === 'get_history_tags_batch')
            return args.ids.map(id => ({ history_entry_id: id, tags: structuredClone(tagsFor(id)) }));
        if (command === 'add_history_tag' || command === 'remove_history_tag') {
            if (state.failAssignment) throw new Error('Fixture assignment storage failed.');
            if (state.holdAssignment) {
                state.holdAssignment = false;
                await new Promise(resolve => {
                    state.releaseAssignment = resolve;
                });
            }
            if (command === 'add_history_tag') assignments.get(args.id).add(args.tagId);
            else assignments.get(args.id)?.delete(args.tagId);
            if (state.holdCommittedAssignment) {
                state.holdCommittedAssignment = false;
                await new Promise(resolve => {
                    state.releaseCommittedAssignment = resolve;
                });
            }
            return;
        }
        if (command === 'get_tag_content') {
            const rows = tagged(args.id)
                .filter(row => !args.query || key(row.title).includes(key(args.query)))
                .sort((a, b) => b.created_at - a.created_at);
            const result = page(rows, args);
            if (state.holdContent) {
                state.holdContent = false;
                return new Promise(resolve => {
                    state.releaseContent = () => resolve(result);
                });
            }
            return result;
        }
        if (command === 'get_tag_accounts') {
            const groups = new Map();
            for (const row of tagged(args.id)) {
                const id = identity(row);
                if (!id) continue;
                const account = groups.get(id) || {
                    history_entry_id: row.id,
                    uploader: row.uploader,
                    source: row.source,
                    content_count: 0,
                    identity_verified: Boolean(row.identity),
                    latest_at: row.created_at,
                };
                account.content_count++;
                groups.set(id, account);
            }
            return page(
                [...groups.values()].filter(row => !args.query || key(row.uploader).includes(key(args.query))),
                args
            );
        }
        if (command === 'get_history')
            return page(
                records
                    .filter(
                        row =>
                            (!args.tagId || assignments.get(row.id)?.has(args.tagId)) &&
                            (!args.query || key(row.title).includes(key(args.query))) &&
                            (!args.source || row.source === args.source)
                    )
                    .sort((a, b) => b.created_at - a.created_at),
                args
            );
        if (command === 'get_history_stats')
            return {
                video_count: records.length,
                total_file_size_bytes: 60 * 1024,
                total_duration_seconds: 3600,
                source_counts: [
                    { source: 'youtube', count: 30 },
                    { source: 'tiktok', count: 30 },
                ],
            };
        if (command === 'get_history_details') {
            const entry = records.find(row => row.id === args.id);
            const result = { entry, output_file_available: false, captions: [], subtitles: [], has_transcript: false };
            if (state.holdDetailsRead) {
                state.holdDetailsRead = false;
                return new Promise(resolve => {
                    state.releaseDetailsRead = () => resolve(result);
                });
            }
            return result;
        }
        if (command === 'get_history_account') {
            const entry = records.find(row => row.id === args.id);
            const rows = entry.identity
                ? records.filter(row => row.identity === entry.identity && row.source === entry.source)
                : [entry];
            return {
                source: entry.source,
                uploader: entry.uploader,
                identity_verified: Boolean(entry.identity),
                saved: page(rows, args),
                insights: null,
            };
        }
        return fallback(command, args);
    };
};
const { version } = JSON.parse(await readFile(new URL('../package.json', import.meta.url)));
const server = createDevServer();
server.listen(0, '127.0.0.1');
await once(server, 'listening');
const origin = `http://127.0.0.1:${server.address().port}`;
try {
    for (const engine of [chromium, webkit]) {
        const browser = await engine.launch();
        try {
            const context = await browser.newContext({ viewport: { width: 1200, height: 900 } });
            await context.addInitScript({
                content: `(${installScreenshotBridge.toString()})(${JSON.stringify({ videos: [], version })}); (${installTagsBridge.toString()})();`,
            });
            const page = await context.newPage();
            const errors = [];
            page.on('pageerror', error => {
                errors.push(error.message);
                console.error(`[tags:ui] ${engine.name()}: ${error.message}`);
            });
            await page.route('**/*', route => {
                const url = route.request().url();
                if (url.startsWith(origin + '/')) return route.continue();
                if (url === 'https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest')
                    return route.fulfill({ json: { tag_name: '2026.10.01' } });
                return route.abort();
            });
            await page.goto(origin);
            await page.evaluate(() => {
                globalThis.__tagModalMaximum = 0;
                new globalThis.MutationObserver(() => {
                    globalThis.__tagModalMaximum = Math.max(
                        globalThis.__tagModalMaximum,
                        globalThis.document.querySelectorAll('dialog:modal').length
                    );
                }).observe(globalThis.document.body, { subtree: true, attributes: true, attributeFilter: ['open'] });
            });
            await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
            const openFilters = async () => {
                if (!(await page.locator('#historyFiltersPopover').evaluate(panel => panel.matches(':popover-open'))))
                    await page.locator('#historyFiltersBtn').click();
            };
            await page.locator('#viewHistoryBtn').click();
            await page.locator('#historyList [data-history-id="tag-entry-60"]').waitFor();
            assert.equal(
                await page
                    .locator('#historyList [data-tag-history-id="tag-entry-60"] .pinefetch-tag-name')
                    .textContent(),
                'Research'
            );
            assert.equal(
                await page.evaluate(
                    () =>
                        globalThis.__tagsUi.commands.filter(call => call.command === 'get_history_tags_batch').at(-1)
                            .args.ids.length
                ),
                20
            );
            await page.locator('#loadMoreHistoryBtn').click();
            await page.waitForFunction(
                () => globalThis.document.querySelectorAll('#historyList .pinefetch-history-item').length === 40
            );
            await page.locator('#historyList [data-history-id="tag-entry-39"] .pinefetch-history-open-btn').focus();
            await page.evaluate(
                () =>
                    new Promise(resolve =>
                        globalThis.requestAnimationFrame(() => globalThis.requestAnimationFrame(resolve))
                    )
            );
            await page.keyboard.press('Shift+F10');
            await page.getByRole('menuitem', { name: 'Manage Tags', exact: true }).click();
            const contextPicker = page.locator('#historyTagsPopover');
            await contextPicker.getByRole('checkbox', { name: 'Add tag Research', exact: true }).click();
            await contextPicker.getByRole('checkbox', { name: 'Remove tag Research', exact: true }).waitFor();
            assert.equal(await page.locator('#historyList .pinefetch-history-item').count(), 40);
            assert(await page.locator('#historyDetailsView').isHidden());
            await contextPicker.getByRole('checkbox', { name: 'Remove tag Research', exact: true }).click();
            await contextPicker.getByRole('checkbox', { name: 'Add tag Research', exact: true }).waitFor();
            await page.keyboard.press('Escape');
            await page.waitForFunction(
                () =>
                    globalThis.document.activeElement?.closest('[data-history-id]')?.dataset.historyId ===
                    'tag-entry-39'
            );
            await page.locator('#viewTagsBtn').click();
            await page.locator('[data-tags-definitions] article').first().waitFor();
            assert.equal(await page.locator('#viewTagsBtn').getAttribute('aria-current'), 'page');
            assert(await page.locator('#historySummaryView').isHidden());
            assert(await page.locator('[data-tags-breadcrumb]').isHidden());
            assert.equal(
                await page.getByRole('navigation', { name: 'Main view' }).locator('[aria-current="page"]').count(),
                1
            );
            await page.locator('#viewHistoryBtn').click();
            await page.locator('#historyView').waitFor();
            assert.equal(await page.locator('#historyList .pinefetch-history-item').count(), 40);
            assert.equal(await page.locator('#viewHistoryBtn').getAttribute('aria-current'), 'page');
            assert.equal(
                await page.locator('.pinefetch-history-sections, #historyAllContentBtn, #historyTagsBtn').count(),
                0
            );
            await page.locator('#viewTagsBtn').click();
            await page.locator('[data-tags-definitions] article').first().waitFor();
            assert.equal(await page.locator('#viewTagsBtn').getAttribute('aria-current'), 'page');
            assert.equal(await page.locator('[data-tags-definitions] article').count(), 2);
            await page.locator('#tagsQuery').fill('unused');
            assert.equal(await page.locator('[data-tags-definitions] article').count(), 1);
            await page.locator('#viewDownloadBtn').click();
            assert(await page.locator('#tagsView').isHidden());
            await page.locator('#viewTagsBtn').click();
            assert.equal(await page.locator('#tagsQuery').inputValue(), 'unused');
            assert.equal(await page.locator('[data-tags-definitions] article').count(), 1);
            await page.locator('#tagsQuery').fill('');
            await page.locator('#tagsView [data-tags-create]').click();
            const editor = page.locator('#tagEditorDialog');
            await editor.locator('[type=submit]').click();
            assert.match(await editor.locator('[data-tag-error]').textContent(), /Enter a tag name/);
            await editor.locator('#tagNameInput').fill('Investigation');
            await editor.locator('#tagDescriptionInput').fill('Evidence and notes');
            await page.screenshot({ path: `/tmp/pinefetch-tag-editor-${engine.name()}.png` });
            await editor.locator('#tagColorInput').fill(' ff0000 ');
            assert.equal(
                await editor
                    .locator('[data-tag-preview] .pinefetch-tag-chip')
                    .evaluate(chip => chip.style.getPropertyValue('--tag-color')),
                '#FF0000'
            );
            assert.equal(await editor.locator('#tagColorPicker').inputValue(), '#ff0000');
            await editor.locator('#tagColorInput').fill('#nothex');
            await editor.locator('[type=submit]').click();
            await page.waitForFunction(() =>
                globalThis.document.querySelector('[data-tag-error]').textContent.includes('HEX')
            );
            assert.equal(await page.evaluate(() => globalThis.__tagsUi.definitions.length), 2);
            await editor.getByRole('button', { name: 'Use #60A5FA', exact: true }).click();
            assert.equal(await editor.locator('[data-tag-preview] .pinefetch-tag-name').textContent(), 'Investigation');
            await editor.locator('[type=submit]').click();
            await editor.waitFor({ state: 'hidden' });
            await page.getByRole('button', { name: 'Edit tag Investigation', exact: true }).click();
            await page.evaluate(() => {
                globalThis.__tagsUi.failSave = true;
            });
            await editor.locator('#tagNameInput').fill('Failed edit');
            await editor.locator('[type=submit]').click();
            await page.waitForFunction(() =>
                globalThis.document.querySelector('[data-tag-error]').textContent.includes('failed')
            );
            await page.evaluate(() => {
                globalThis.__tagsUi.failSave = false;
            });
            await editor.locator('#tagNameInput').fill('Investigação');
            await editor.locator('[type=submit]').click();
            await editor.waitFor({ state: 'hidden' });
            await page.getByRole('button', { name: 'Open tag Investigação', exact: true }).waitFor();
            await page.locator('#tagsView [data-tags-create]').click();
            await editor.locator('#tagNameInput').fill('research');
            await editor.locator('[type=submit]').click();
            await page.waitForFunction(() =>
                globalThis.document.querySelector('[data-tag-error]').textContent.includes('already exists')
            );
            await page.keyboard.press('Escape');
            await editor.waitFor({ state: 'hidden' });
            await page.getByRole('button', { name: 'Delete tag Unused', exact: true }).click();
            const confirmation = page.getByRole('dialog', { name: 'Delete tag?', exact: true });
            await confirmation.waitFor();
            assert.match(await confirmation.textContent(), /0 saved items/);
            await confirmation.getByRole('button', { name: 'Delete Tag', exact: true }).click();
            await confirmation.waitFor({ state: 'hidden' });
            assert.equal(await page.evaluate(() => globalThis.__tagsUi.records.length), 60);
            await page.getByRole('button', { name: 'Open tag Research', exact: true }).click();
            await page.locator('[data-tags-content] .pinefetch-history-item').first().waitFor();
            assert.equal(await page.locator('[data-tags-content] .pinefetch-history-item').count(), 20);
            await page.locator('[data-tags-more]').click();
            await page.waitForFunction(
                () => globalThis.document.querySelectorAll('[data-tags-content] .pinefetch-history-item').length === 30
            );
            const tagReadsBeforeSwitch = await page.evaluate(
                () => globalThis.__tagsUi.commands.filter(call => call.command === 'get_tag_content').length
            );
            await page.locator('[data-tags-scroll]').evaluate(node => {
                node.scrollTop = 140;
            });
            await page.locator('#viewDownloadBtn').click();
            await page.locator('#viewTagsBtn').click();
            assert.equal(await page.locator('[data-tags-content] .pinefetch-history-item').count(), 30);
            assert.equal(await page.locator('[data-tags-scroll]').evaluate(node => node.scrollTop), 140);
            assert.equal(
                await page.evaluate(
                    () => globalThis.__tagsUi.commands.filter(call => call.command === 'get_tag_content').length
                ),
                tagReadsBeforeSwitch
            );
            await page.evaluate(() => {
                globalThis.__tagsUi.failAssignment = true;
            });
            await page
                .locator('[data-tags-content]')
                .getByRole('button', { name: /Remove this tag from/ })
                .first()
                .click();
            await page.waitForFunction(() =>
                globalThis.document.querySelector('#tagsView [data-tags-status]').textContent.includes('failed')
            );
            assert.equal(await page.locator('[data-tags-content] .pinefetch-history-item').count(), 30);
            await page.evaluate(() => {
                globalThis.__tagsUi.failAssignment = false;
            });
            await page.locator('[data-tags-content] .pinefetch-history-open-btn').first().focus();
            await page.evaluate(
                () =>
                    new Promise(resolve =>
                        globalThis.requestAnimationFrame(() => globalThis.requestAnimationFrame(resolve))
                    )
            );
            await page.keyboard.press('Shift+F10');
            await page.getByRole('menuitem', { name: 'Show More Data', exact: true }).click();
            await page.locator('#historyDetailsContent').waitFor();
            assert.equal(await page.locator('#viewTagsBtn').getAttribute('aria-current'), 'page');
            assert(await page.locator('#historyDetailsView .pinefetch-tag-return').isVisible());
            await page.locator('#historyDetailsView .pinefetch-tag-return').click();
            assert.equal(await page.locator('[data-tags-content] .pinefetch-history-item').count(), 30);
            await page.locator('#tagsQuery').fill('Clip 2');
            await page.waitForFunction(
                () =>
                    globalThis.__tagsUi.commands.findLast(call => call.command === 'get_tag_content')?.args.query ===
                    'Clip 2'
            );
            await page.waitForFunction(
                () => globalThis.document.querySelectorAll('[data-tags-content] .pinefetch-history-item').length < 20
            );
            await page.locator('#tagsQuery').fill('');
            await page.waitForFunction(
                () => globalThis.document.querySelectorAll('[data-tags-content] .pinefetch-history-item').length === 20
            );
            await page.locator('#tagContentTab').focus();
            await page.keyboard.press('ArrowRight');
            await page.locator('[data-tags-accounts] article').first().waitFor();
            assert.equal(await page.locator('#tagAccountsTab').getAttribute('aria-selected'), 'true');
            assert((await page.locator('[data-tags-accounts] article').count()) < 30);
            await page.locator('#tagsQuery').fill('Creator 1');
            await page.waitForFunction(
                () =>
                    globalThis.__tagsUi.commands.findLast(call => call.command === 'get_tag_accounts')?.args.query ===
                    'Creator 1'
            );
            await page.waitForFunction(
                () => globalThis.document.querySelectorAll('[data-tags-accounts] article').length === 2
            );
            await page.locator('[data-tags-accounts] article button').first().click();
            await page.locator('#historyAccountView').waitFor();
            assert.equal(await page.locator('#viewTagsBtn').getAttribute('aria-current'), 'page');
            assert(await page.locator('#historyAccountView .pinefetch-tag-return').isVisible());
            await page.locator('#historyAccountView .pinefetch-tag-return').click();
            assert.equal(await page.locator('#tagAccountsTab').getAttribute('aria-selected'), 'true');
            assert.equal(await page.locator('#tagsQuery').inputValue(), 'Creator 1');
            await page.locator('#tagContentTab').click();
            await page.locator('[data-tags-content] .pinefetch-history-open-btn').first().click();
            await page.locator('#historyDetailsContent').waitFor();
            await page.locator('#historyDetailsTags .pinefetch-tag-name').waitFor();
            assert.equal(await page.locator('#historyDetailsHeader #historyDetailsTags').count(), 1);
            assert.equal(await page.locator('#historyOverviewPanel #historyDetailsTags').count(), 0);
            const readsBeforeTabs = await page.evaluate(
                () => globalThis.__tagsUi.commands.filter(call => call.command === 'get_history_tags').length
            );
            for (const id of [
                'historyTranscriptTab',
                'historySubtitlesTab',
                'historyOcrTab',
                'historySocialTab',
                'historyKeywordsTab',
                'historyOverviewTab',
            ]) {
                await page.locator(`#${id}`).click();
                assert(await page.locator('#historyDetailsTags').isVisible());
                assert(await page.locator('#historyDetailsTags').getByText('Research', { exact: true }).isVisible());
            }
            assert.equal(
                await page.evaluate(
                    () => globalThis.__tagsUi.commands.filter(call => call.command === 'get_history_tags').length
                ),
                readsBeforeTabs
            );
            await page.locator('#historyTranscriptTab').click();
            await page.locator('#historyDetailsTags [data-detail-tags-add]').click();
            await page
                .locator('#historyTagsPopover')
                .getByRole('checkbox', { name: 'Remove tag Research', exact: true })
                .waitFor();
            await page.keyboard.press('Escape');
            await page.locator('#historyOverviewTab').click();
            await page.locator('#historyDetailsTags').getByRole('button', { name: '+ Add Tag', exact: true }).click();
            const picker = page.locator('#historyTagsPopover');
            await picker.getByRole('checkbox', { name: 'Add tag Investigação', exact: true }).waitFor();
            assert.equal(await picker.getByRole('checkbox', { name: 'Add tag Research', exact: true }).count(), 0);
            const additionsBeforeHold = await page.evaluate(
                () => globalThis.__tagsUi.commands.filter(call => call.command === 'add_history_tag').length
            );
            await page.evaluate(() => {
                globalThis.__tagsUi.holdAssignment = true;
            });
            await picker.getByRole('checkbox', { name: 'Add tag Investigação', exact: true }).click();
            assert.equal(
                await picker.getByRole('checkbox', { name: 'Remove tag Investigação', exact: true }).count(),
                0
            );
            await picker.getByRole('checkbox', { name: 'Add tag Investigação', exact: true }).dispatchEvent('click');
            assert.equal(
                await page.evaluate(
                    () => globalThis.__tagsUi.commands.filter(call => call.command === 'add_history_tag').length
                ),
                additionsBeforeHold + 1
            );
            await page.evaluate(() => globalThis.__tagsUi.releaseAssignment());
            await picker.getByRole('checkbox', { name: 'Remove tag Investigação', exact: true }).waitFor();
            await page.evaluate(() => {
                globalThis.__tagsUi.failAssignment = true;
            });
            await picker.getByRole('checkbox', { name: 'Remove tag Investigação', exact: true }).click();
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('#historyTagsPopover [data-quick-status]')
                    .textContent.includes('failed')
            );
            assert(await picker.getByRole('checkbox', { name: 'Remove tag Investigação', exact: true }).isVisible());
            await page.evaluate(() => {
                globalThis.__tagsUi.failAssignment = false;
            });
            await picker.locator('[data-quick-create]').click();
            await picker.locator('#quickTagName').fill('Fresh');
            assert.equal(await page.locator('dialog:modal').count(), 0);
            await picker.getByRole('button', { name: 'Create & Add', exact: true }).click();
            await picker.getByRole('checkbox', { name: 'Remove tag Fresh', exact: true }).waitFor();
            await picker.locator('[data-quick-create]').click();
            await picker.locator('#quickTagName').fill('Overflow');
            await picker.getByRole('button', { name: 'Create & Add', exact: true }).click();
            await picker.getByRole('checkbox', { name: 'Remove tag Overflow', exact: true }).waitFor();
            await picker.locator('[data-quick-create]').click();
            await picker.getByRole('button', { name: 'Create & Add', exact: true }).click();
            assert.match(await picker.locator('[data-quick-status]').textContent(), /Enter a tag name/);
            await picker.locator('#quickTagName').fill('research');
            await picker.getByRole('button', { name: 'Create & Add', exact: true }).click();
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('#historyTagsPopover [data-quick-status]')
                    .textContent.includes('already exists')
            );
            assert.equal(await page.locator('dialog:modal').count(), 0);
            await picker.getByRole('button', { name: 'Cancel', exact: true }).click();
            await picker.locator('[data-quick-create]').click();
            await picker.locator('#quickTagName').fill('Recovery');
            await page.evaluate(() => {
                globalThis.__tagsUi.failAssignment = true;
            });
            const creationsBefore = await page.evaluate(
                () => globalThis.__tagsUi.commands.filter(call => call.command === 'create_tag').length
            );
            await picker.getByRole('button', { name: 'Create & Add', exact: true }).click();
            await picker.getByRole('button', { name: 'Retry Add', exact: true }).waitFor();
            assert.match(await picker.locator('[data-quick-status]').textContent(), /created.*could not be added/i);
            assert.equal(
                await page.evaluate(
                    () => globalThis.__tagsUi.definitions.filter(tag => tag.name === 'Recovery').length
                ),
                1
            );
            await page.keyboard.press('Escape');
            await page.locator('#historyDetailsTags').getByRole('button', { name: '+ Add Tag', exact: true }).click();
            await picker.getByRole('button', { name: 'Retry Add', exact: true }).waitFor();
            await page.evaluate(() => {
                globalThis.__tagsUi.failAssignment = false;
            });
            await picker.getByRole('button', { name: 'Retry Add', exact: true }).click();
            await picker.getByRole('checkbox', { name: 'Remove tag Recovery', exact: true }).waitFor();
            assert.equal(
                await page.evaluate(
                    () => globalThis.__tagsUi.commands.filter(call => call.command === 'create_tag').length
                ),
                creationsBefore + 1
            );
            await picker.getByRole('checkbox', { name: 'Remove tag Recovery', exact: true }).click();
            await picker.getByRole('checkbox', { name: 'Add tag Recovery', exact: true }).waitFor();
            await picker.locator('#historyTagsSearch').fill('Recovery');
            await picker.locator('#historyTagsSearch').focus();
            await page.keyboard.press('ArrowDown');
            await page.keyboard.press('Enter');
            await picker.getByRole('checkbox', { name: 'Remove tag Recovery', exact: true }).waitFor();
            await page.waitForFunction(
                () => globalThis.document.activeElement?.getAttribute('aria-label') === 'Remove tag Recovery'
            );
            await page.keyboard.press('Space');
            await picker.getByRole('checkbox', { name: 'Add tag Recovery', exact: true }).waitFor();
            await picker.locator('#historyTagsSearch').fill('');
            await page.screenshot({ path: `/tmp/pinefetch-quick-tags-${engine.name()}.png` });
            await picker.locator('[data-quick-create]').click();
            await picker.locator('#quickTagName').fill('Preview');
            await page.setViewportSize({ width: 380, height: 600 });
            await page.waitForFunction(() => {
                const box = globalThis.document.querySelector('#historyTagsPopover').getBoundingClientRect();
                return box.x >= 0 && box.y >= 0 && box.right <= 380 && box.bottom <= 600;
            });
            const compactBounds = await picker.boundingBox();
            assert(
                compactBounds.x >= 0 &&
                    compactBounds.y >= 0 &&
                    compactBounds.x + compactBounds.width <= 380 &&
                    compactBounds.y + compactBounds.height <= 600
            );
            assert(await picker.evaluate(panel => panel.scrollWidth <= panel.clientWidth));
            await page.screenshot({ path: `/tmp/pinefetch-quick-create-${engine.name()}.png` });
            await picker.getByRole('button', { name: 'Cancel', exact: true }).click();
            await page.setViewportSize({ width: 1200, height: 900 });

            await page.keyboard.press('Escape');
            await picker.waitFor({ state: 'hidden' });
            await page.locator('#historyDetailsTags').getByText('Fresh', { exact: true }).waitFor();
            // A real native parent modal remains the only modal/focus trap.
            await page.evaluate(() => {
                const section = globalThis.document.querySelector('#historyDetailsTags');
                globalThis.__tagParent = { parent: section.parentNode, next: section.nextSibling };
                const dialog = globalThis.document.createElement('dialog');
                dialog.id = 'tagPopoverParent';
                dialog.className = 'oj-dialog pinefetch-tag-dialog';
                dialog.style.cssText =
                    'position:fixed;left:calc(100vw - 280px);top:calc(100vh - 180px);width:260px;max-height:160px;overflow:hidden;margin:0;';
                const action = globalThis.document.createElement('button');
                action.type = 'button';
                action.textContent = 'Parent action';
                action.onclick = () => {
                    globalThis.__tagParent.clicked = true;
                };
                dialog.append(section, action);
                globalThis.document.body.append(dialog);
                dialog.showModal();
            });
            await page.locator('#historyDetailsTags').getByRole('button', { name: '+ Add Tag', exact: true }).click();
            await picker.locator('#historyTagsSearch').waitFor();
            assert.equal(await page.locator('dialog:modal').count(), 1);
            assert.equal(await picker.getAttribute('aria-modal'), 'false');
            const bounds = await picker.boundingBox();
            assert(
                bounds.x >= 0 && bounds.y >= 0 && bounds.x + bounds.width <= 1200 && bounds.y + bounds.height <= 900
            );
            await picker.locator('[data-quick-create]').click();
            assert.equal(await page.locator('dialog:modal').count(), 1);
            assert(await editor.isHidden());
            await picker.getByRole('button', { name: 'Cancel', exact: true }).click();
            await picker.locator('#historyTagsSearch').focus();
            await page.keyboard.press('ArrowDown');
            assert.equal(await page.evaluate(() => globalThis.document.activeElement.getAttribute('role')), 'checkbox');
            await page.keyboard.press('Escape');
            assert(await page.locator('#tagPopoverParent').isVisible());
            assert(await picker.isHidden());
            assert(
                await page
                    .locator('#historyDetailsTags')
                    .getByRole('button', { name: '+ Add Tag', exact: true })
                    .evaluate(button => button === globalThis.document.activeElement)
            );
            await page.locator('#historyDetailsTags').getByRole('button', { name: '+ Add Tag', exact: true }).click();
            await picker.locator('#historyTagsSearch').waitFor();
            await page.locator('#tagPopoverParent').getByRole('button', { name: 'Parent action', exact: true }).click();
            assert(await picker.isHidden());
            assert(await page.evaluate(() => globalThis.__tagParent.clicked));
            await page.evaluate(() => {
                const section = globalThis.document.querySelector('#historyDetailsTags');
                const host = globalThis.__tagParent;
                globalThis.document.querySelector('#tagPopoverParent').close();
                host.parent.insertBefore(section, host.next);
                globalThis.document.querySelector('#tagPopoverParent').remove();
            });

            await page.evaluate(async () => {
                const ui = globalThis.__tagsUi;
                ui.delayedTag = await globalThis.__TAURI__.core.invoke('create_tag', {
                    tag: { name: 'Delayed', description: '', color: '#123456' },
                });
            });
            await page.locator('#historyDetailsTags').getByRole('button', { name: '+ Add Tag', exact: true }).click();
            await picker.getByRole('checkbox', { name: 'Add tag Delayed', exact: true }).waitFor();
            await page.evaluate(() => {
                globalThis.__tagsUi.holdAssignment = true;
            });
            await picker.getByRole('checkbox', { name: 'Add tag Delayed', exact: true }).click();
            await page.keyboard.press('Escape');
            await page.locator('#historyDetailsView .pinefetch-tag-return').click();
            await page.getByRole('button', { name: 'Manage all 4 tags', exact: true }).waitFor();
            await page.locator('[data-tags-content] .pinefetch-history-open-btn').nth(1).click();
            await page.locator('#historyDetailsContent').waitFor();
            await page.locator('#historyDetailsTags').getByRole('button', { name: '+ Add Tag', exact: true }).click();
            await picker.getByRole('checkbox', { name: 'Add tag Delayed', exact: true }).waitFor();
            assert(await picker.getByRole('checkbox', { name: 'Add tag Delayed', exact: true }).isEnabled());
            await page.keyboard.press('Escape');
            await page.evaluate(() => globalThis.__tagsUi.releaseAssignment());
            await page.waitForFunction(() => {
                const ui = globalThis.__tagsUi;
                return ui.assignments.get('tag-entry-60').has(ui.delayedTag.id);
            });
            assert.equal(await page.locator('#historyDetailsTags').getByText('Delayed', { exact: true }).count(), 0);
            // Create's reply arrives after moving to another video: only the captured ID is mutated.
            await page.locator('#historyDetailsTags').getByRole('button', { name: '+ Add Tag', exact: true }).click();
            await picker.locator('[data-quick-create]').click();
            await picker.locator('#quickTagName').fill('Captured');
            await page.evaluate(() => {
                globalThis.__tagsUi.holdCreation = true;
            });
            await picker.getByRole('button', { name: 'Create & Add', exact: true }).click();
            await page.waitForFunction(() => typeof globalThis.__tagsUi.releaseCreation === 'function');
            await picker.getByRole('button', { name: 'Create & Add', exact: true }).dispatchEvent('click');
            assert.equal(
                await page.evaluate(
                    () => globalThis.__tagsUi.definitions.filter(tag => tag.name === 'Captured').length
                ),
                1
            );
            await page.keyboard.press('Escape');
            await page.locator('#historyDetailsView .pinefetch-tag-return').click();
            await page.locator('[data-tags-content] .pinefetch-history-open-btn').nth(2).click();
            await page.locator('#historyDetailsTags').getByRole('button', { name: '+ Add Tag', exact: true }).click();
            await picker.getByRole('checkbox', { name: 'Add tag Captured', exact: true }).waitFor();
            await page.evaluate(() => globalThis.__tagsUi.releaseCreation());
            await page.waitForFunction(() => {
                const ui = globalThis.__tagsUi;
                return ui.assignments.get('tag-entry-58').has(ui.definitions.find(tag => tag.name === 'Captured').id);
            });
            assert(await picker.isVisible());
            assert(await picker.getByRole('checkbox', { name: 'Add tag Captured', exact: true }).isVisible());
            assert.equal(await page.locator('#historyDetailsTags').getByText('Captured', { exact: true }).count(), 0);
            await page.keyboard.press('Escape');
            await page.locator('#historyDetailsView .pinefetch-tag-return').click();
            await page.locator('[data-tags-content] .pinefetch-history-open-btn').nth(1).click();
            await page.locator('#historyDetailsTags').getByRole('button', { name: '+ Add Tag', exact: true }).click();
            await picker.getByRole('checkbox', { name: 'Remove tag Captured', exact: true }).waitFor();
            // A committed removal with a delayed reply must not swallow a subsequent Add.
            await page.evaluate(() => {
                globalThis.__tagsUi.holdCommittedAssignment = true;
            });
            await picker.getByRole('checkbox', { name: 'Remove tag Captured', exact: true }).click();
            await page.waitForFunction(() => typeof globalThis.__tagsUi.releaseCommittedAssignment === 'function');
            await page.keyboard.press('Escape');
            await page.locator('#historyDetailsTags').getByRole('button', { name: '+ Add Tag', exact: true }).click();
            await picker.getByRole('checkbox', { name: 'Add tag Captured', exact: true }).click();
            await page.evaluate(() => globalThis.__tagsUi.releaseCommittedAssignment());
            await picker.getByRole('checkbox', { name: 'Remove tag Captured', exact: true }).waitFor();
            assert(
                await page.evaluate(() => {
                    const ui = globalThis.__tagsUi;
                    return ui.assignments
                        .get('tag-entry-58')
                        .has(ui.definitions.find(tag => tag.name === 'Captured').id);
                })
            );
            await page.keyboard.press('Escape');
            await page.locator('#historyDetailsView .pinefetch-tag-return').click();

            await page.locator('#viewHistoryBtn').click();
            await page.locator('#historyView').waitFor();
            await openFilters();
            await page.locator('#historyTagFilter').selectOption(researchId());
            await page.waitForFunction(
                () => globalThis.document.querySelector('#historyResultCount').textContent === '30 found'
            );
            await page.locator('#historySourceTrigger').click();
            await page.locator('#historySourceMenu [data-oj-value="tiktok"]').click();
            assert.equal(await page.locator('#historyFiltersValue').textContent(), 'Filters · 2');
            await page.locator('#historyClearSourceFilter').click();
            assert.equal(await page.locator('#historyFiltersValue').textContent(), 'Filters · 1');
            await page.locator('#historySourceTrigger').click();
            await page.locator('#historySourceMenu [data-oj-value="tiktok"]').click();
            await page.locator('#historySearchInput').fill('Clip 2');
            await page.waitForFunction(
                () =>
                    globalThis.__tagsUi.commands.findLast(call => call.command === 'get_history')?.args.query ===
                    'Clip 2'
            );
            await openFilters();
            await page.locator('#historyClearTagFilter').click();
            assert(
                await page.locator('#historyTagFilter').evaluate(node => node === globalThis.document.activeElement)
            );
            await page.waitForFunction(
                () => !globalThis.__tagsUi.commands.findLast(call => call.command === 'get_history')?.args.tagId
            );
            assert.equal(await page.locator('#historySearchInput').inputValue(), 'Clip 2');
            assert.equal(await page.locator('#historyFiltersValue').textContent(), 'Filters · 1');
            await page.locator('#historyClearFiltersBtn').click();
            await page.waitForFunction(() => {
                const args = globalThis.__tagsUi.commands.findLast(call => call.command === 'get_history')?.args;
                return args?.query === 'Clip 2' && !args.source && !args.tagId;
            });
            assert.equal(await page.locator('#historyFiltersValue').textContent(), 'Filters');
            await page.keyboard.press('Escape');
            assert(
                await page.locator('#historyFiltersBtn').evaluate(node => node === globalThis.document.activeElement)
            );
            await page.locator('#viewTagsBtn').click();
            assert.equal(await page.locator('[data-tags-crumb]').textContent(), 'Research');
            await page.locator('[data-tags-overview]').click();
            await page.getByRole('button', { name: 'Open tag Research', exact: true }).waitFor();
            await page.evaluate(() => {
                globalThis.__tagsUi.holdContent = true;
            });
            await page.getByRole('button', { name: 'Open tag Research', exact: true }).click();
            await page.waitForFunction(() => typeof globalThis.__tagsUi.releaseContent === 'function');
            await page.locator('[data-tags-overview]').click();
            await page.getByRole('button', { name: 'Open tag Investigação', exact: true }).click();
            await page.evaluate(() => globalThis.__tagsUi.releaseContent());
            await page.waitForFunction(
                () => globalThis.document.querySelector('[data-tags-crumb]').textContent === 'Investigação'
            );
            assert.equal(await page.locator('[data-tags-content] .pinefetch-history-item').count(), 1);
            await page.locator('[data-tags-overview]').click();
            await page.getByRole('button', { name: 'Open tag Research', exact: true }).waitFor();
            await page.evaluate(() => {
                globalThis.__tagsUi.failTagRead = true;
            });
            await page.getByRole('button', { name: 'Open tag Research', exact: true }).click();
            await page.waitForFunction(() =>
                globalThis.document.querySelector('#tagsView [data-tags-status]').textContent.includes('failed')
            );
            assert(await page.locator('[data-tags-edit]').isDisabled());
            assert(await page.locator('[data-tags-delete]').isDisabled());
            await page.evaluate(() => {
                globalThis.__tagsUi.failTagRead = false;
            });
            await page.locator('[data-tags-refresh]').click();
            await page.waitForFunction(
                () => globalThis.document.querySelector('[data-tags-crumb]').textContent === 'Research'
            );
            await page.setViewportSize({ width: 520, height: 800 });
            assert(await page.evaluate(() => globalThis.document.documentElement.scrollWidth <= globalThis.innerWidth));
            await page.screenshot({ path: `/tmp/pinefetch-tags-${engine.name()}.png` });
            await page.locator('#viewHistoryBtn').click();
            assert.equal(await page.locator('#historyVideoCount').textContent(), '60');
            for (const width of [1440, 1000, 800, 520, 380]) {
                await page.setViewportSize({ width, height: 900 });
                await openFilters();
                await page.evaluate(
                    () =>
                        new Promise(resolve =>
                            globalThis.requestAnimationFrame(() => globalThis.requestAnimationFrame(resolve))
                        )
                );
                const box = await page.locator('#historyFiltersPopover').boundingBox();
                assert(
                    box.x >= 0 && box.y >= 0 && box.x + box.width <= width && box.y + box.height <= 900,
                    `${width}px: ${JSON.stringify(box)}`
                );
                assert(
                    await page.evaluate(() => globalThis.document.documentElement.scrollWidth <= globalThis.innerWidth)
                );
                assert.equal(await page.locator('dialog:modal').count(), 0);
                if (width === 1440)
                    await page.screenshot({ path: `/tmp/pinefetch-history-filters-${engine.name()}.png` });
                await page.keyboard.press('Escape');
                assert(await page.locator('#historyFiltersPopover').evaluate(panel => !panel.matches(':popover-open')));
                const visibleViews = await page
                    .locator(
                        '#downloadView, #historyView, #tagsView, #settingsView, #historyDetailsView, #historyAccountView'
                    )
                    .evaluateAll(nodes => nodes.filter(node => node.getClientRects().length).length);
                assert.equal(visibleViews, 1);
            }
            await page.setViewportSize({ width: 1200, height: 900 });
            await page.locator('#viewTagsBtn').focus();
            await page.keyboard.press('Enter');
            assert.equal(await page.locator('#viewTagsBtn').getAttribute('aria-current'), 'page');
            await page.locator('[data-tags-overview]').click();
            await page.locator('[data-tags-definitions] article').first().waitFor();
            assert(await page.locator('[data-tags-breadcrumb]').isHidden());
            await page.locator('[data-tags-scroll]').evaluate(node => {
                node.scrollTop = 0;
            });
            await page.screenshot({ path: `/tmp/pinefetch-tags-overview-${engine.name()}.png` });
            // Header metadata persists across tabs and keeps overflow mutations local.
            const longHeaderTagName = 'Investigation with a deliberately long label for compact header display';
            await page.evaluate(name => {
                const ui = globalThis.__tagsUi;
                const entry = ui.records.find(row => row.id === 'tag-entry-60');
                entry.title = 'A long saved video title with creator context and details that wrap safely '.repeat(2);
                entry.thumbnail =
                    'data:image/svg+xml,' +
                    encodeURIComponent(
                        '<svg xmlns="http://www.w3.org/2000/svg" width="160" height="90"><rect width="160" height="90" fill="#35C9A5"/></svg>'
                    );
                ui.definitions.find(tag => tag.name === 'Investigação').name = name;
            }, longHeaderTagName);
            await page.getByRole('button', { name: 'Open tag Research', exact: true }).click();
            await page.locator('[data-tags-content] .pinefetch-history-open-btn').first().click();
            const header = page.locator('#historyDetailsHeader');
            const headerTags = header.locator('#historyDetailsTags');
            const headerAdd = headerTags.locator('[data-detail-tags-add]');
            const headerMore = headerTags.locator('[data-detail-tags-overflow]');
            await headerAdd.waitFor();
            await page.waitForFunction(
                () => !globalThis.document.querySelector('#historyDetailsTags [data-detail-tags-add]').disabled
            );
            assert.equal(await headerTags.locator('.pinefetch-tag-chip').count(), 3);
            assert.equal(
                await headerTags.getByText(longHeaderTagName, { exact: true }).getAttribute('title'),
                longHeaderTagName
            );
            const addBox = await headerAdd.boundingBox();
            const moreBox = await headerMore.boundingBox();
            assert(addBox.x >= moreBox.x + moreBox.width && addBox.x - moreBox.x - moreBox.width <= 16);
            for (const width of [1200, 800, 520, 380]) {
                await page.setViewportSize({ width, height: 900 });
                await page.evaluate(
                    () =>
                        new Promise(resolve =>
                            globalThis.requestAnimationFrame(() => globalThis.requestAnimationFrame(resolve))
                        )
                );
                assert(
                    await page.evaluate(() => globalThis.document.documentElement.scrollWidth <= globalThis.innerWidth)
                );
                assert.equal(await headerTags.locator('.pinefetch-tag-chip').count(), width <= 620 ? 1 : 3);
                const titleBox = await header.locator('#historyDetailsTitle').boundingBox();
                const imageBox = await header.locator('#historyDetailsThumbnail').boundingBox();
                assert(titleBox.x + titleBox.width <= imageBox.x);
                await headerAdd.click();
                const popupBox = await picker.boundingBox();
                assert(
                    popupBox.x >= 0 &&
                        popupBox.y >= 0 &&
                        popupBox.x + popupBox.width <= width &&
                        popupBox.y + popupBox.height <= 900
                );
                await page.keyboard.press('Escape');
                assert(await headerAdd.evaluate(button => button === globalThis.document.activeElement));
                if ([1200, 380].includes(width))
                    await page.screenshot({ path: `/tmp/pinefetch-header-tags-${engine.name()}-${width}.png` });
            }
            await page.setViewportSize({ width: 1200, height: 900 });
            await headerMore.click();
            await picker.getByRole('checkbox', { name: 'Remove tag Delayed', exact: true }).waitFor();
            assert(await picker.locator('[data-quick-create]').isHidden());
            assert.equal(await picker.getByRole('checkbox', { name: 'Add tag Recovery', exact: true }).count(), 0);
            await page.evaluate(() => {
                globalThis.__tagsUi.failAssignment = true;
            });
            await picker.getByRole('checkbox', { name: 'Remove tag Delayed', exact: true }).click();
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('#historyTagsPopover [data-quick-status]')
                    .textContent.includes('failed')
            );
            assert(await picker.getByRole('checkbox', { name: 'Remove tag Delayed', exact: true }).isVisible());
            await page.evaluate(() => {
                globalThis.__tagsUi.failAssignment = false;
            });
            await picker.getByRole('checkbox', { name: 'Remove tag Delayed', exact: true }).click();
            await picker
                .getByRole('checkbox', { name: 'Remove tag Delayed', exact: true })
                .waitFor({ state: 'detached' });
            await picker.getByRole('checkbox', { name: 'Remove tag Overflow', exact: true }).click();
            await headerMore.waitFor({ state: 'hidden' });
            assert(await picker.isVisible());
            await page.keyboard.press('Escape');
            assert(await headerAdd.evaluate(button => button === globalThis.document.activeElement));
            await headerTags.getByRole('button', { name: 'Remove tag Fresh', exact: true }).click();
            await headerTags.getByText('Fresh', { exact: true }).waitFor({ state: 'detached' });
            await page.waitForFunction(
                () =>
                    globalThis.document.querySelectorAll('#historyDetailsTags .pinefetch-tag-remove:not(:disabled)')
                        .length === 2
            );
            await page.evaluate(() => {
                globalThis.__tagsUi.failAssignment = true;
            });
            await headerTags.getByRole('button', { name: `Remove tag ${longHeaderTagName}`, exact: true }).click();
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('#historyDetailsTags [data-detail-tag-status]')
                    .textContent.includes('failed')
            );
            assert(await headerTags.getByText(longHeaderTagName, { exact: true }).isVisible());
            await page.evaluate(() => {
                globalThis.__tagsUi.failAssignment = false;
            });
            await headerTags.getByRole('button', { name: `Remove tag ${longHeaderTagName}`, exact: true }).click();
            await headerTags.getByText(longHeaderTagName, { exact: true }).waitFor({ state: 'detached' });
            await page.locator('#historyDetailsView .pinefetch-tag-return').click();
            await page.evaluate(() => {
                globalThis.__tagsUi.holdTagsRead = true;
            });
            await page.locator('[data-tags-content] .pinefetch-history-open-btn').first().click();
            await page.waitForFunction(() => typeof globalThis.__tagsUi.releaseTagsRead === 'function');
            await page.locator('#viewHistoryBtn').click();
            await page.locator('#historySearchInput').fill('');
            await page.locator('#historyList [data-history-id="tag-entry-57"]').waitFor();
            await page.evaluate(() => {
                globalThis.__tagsUi.holdDetailsRead = true;
            });
            await page.locator('#historyList [data-history-id="tag-entry-57"] .pinefetch-history-open-btn').click();
            await page.waitForFunction(() => typeof globalThis.__tagsUi.releaseDetailsRead === 'function');
            assert(await headerTags.isHidden());
            await page.evaluate(() => globalThis.__tagsUi.releaseDetailsRead());
            await headerAdd.waitFor();
            assert.equal(await headerTags.locator('.pinefetch-tag-chip').count(), 0);
            assert(!(await headerTags.textContent()).includes('No tags assigned'));
            await page.evaluate(() => globalThis.__tagsUi.releaseTagsRead());
            assert.equal(await page.locator('#historyDetailsTitle').textContent(), 'Clip 57');
            assert.equal(await headerTags.locator('.pinefetch-tag-chip').count(), 0);
            await page.locator('#viewTagsBtn').click();
            await page.locator('[data-tags-overview]').click();
            // The isolated fixture can also represent a first launch with no tags.
            await page.evaluate(() => {
                globalThis.__tagsUi.definitions.splice(0);
                for (const assigned of globalThis.__tagsUi.assignments.values()) assigned.clear();
            });
            await page.locator('[data-tags-refresh]').click();
            await page.locator('[data-tags-empty-state]').waitFor();
            assert(await page.locator('[data-tags-search]').isHidden());
            await page.screenshot({ path: `/tmp/pinefetch-tags-empty-${engine.name()}.png` });
            await page.locator('[data-tags-empty-create]').click();
            await editor.locator('#tagNameInput').fill('First collection');
            await editor.locator('[type=submit]').click();
            await page.getByRole('button', { name: 'Open tag First collection', exact: true }).waitFor();
            assert(await page.locator('[data-tags-search]').isVisible());
            assert(await page.locator('[data-tags-empty-state]').isHidden());
            await page.waitForFunction(() => {
                const root = globalThis.document.querySelector('#tagsView');
                return (
                    root.contains(globalThis.document.activeElement) &&
                    globalThis.document.activeElement.getClientRects().length > 0
                );
            });
            assert(await page.evaluate(() => globalThis.__tagModalMaximum <= 1));
            assert.deepEqual(errors, []);
            console.log(
                `[tags:ui] ${engine.name()}: persistent header/tabs, responsive chip overflow/removal, stale header replies, standalone Tags/context, filters, CRUD, quick tagging, single parent modal, failure recovery, pagination, accounts and keyboard/focus passed.`
            );
        } finally {
            await browser.close();
        }
    }
} finally {
    await new Promise(resolve => server.close(resolve));
}
function researchId() {
    return '11111111-1111-4111-8111-111111111111';
}
