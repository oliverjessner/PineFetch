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
            return structuredClone(tag);
        }
        if (command === 'delete_tag') {
            const index = definitions.findIndex(tag => tag.id === args.id);
            if (index < 0) throw new Error('This tag no longer exists.');
            definitions.splice(index, 1);
            for (const tags of assignments.values()) tags.delete(args.id);
            return;
        }
        if (command === 'get_history_tags') return structuredClone(tagsFor(args.id));
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
            return { entry, output_file_available: false, captions: [], subtitles: [], has_transcript: false };
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
            page.on('pageerror', error => errors.push(error.message));
            await page.route('**/*', route => {
                const url = route.request().url();
                if (url.startsWith(origin + '/')) return route.continue();
                if (url === 'https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest')
                    return route.fulfill({ json: { tag_name: '2026.10.01' } });
                return route.abort();
            });
            await page.goto(origin);
            await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
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
            const contextPicker = page.locator('#historyTagsDialog');
            await contextPicker.getByRole('button', { name: 'Add tag Research', exact: true }).click();
            await contextPicker.getByRole('button', { name: 'Remove tag Research', exact: true }).waitFor();
            assert.equal(await page.locator('#historyList .pinefetch-history-item').count(), 40);
            assert(await page.locator('#historyDetailsView').isHidden());
            await contextPicker.getByRole('button', { name: 'Remove tag Research', exact: true }).click();
            await contextPicker.getByRole('button', { name: 'Add tag Research', exact: true }).waitFor();
            await page.keyboard.press('Escape');
            await page.waitForFunction(
                () =>
                    globalThis.document.activeElement?.closest('[data-history-id]')?.dataset.historyId ===
                    'tag-entry-39'
            );
            await page.locator('#historyTagsBtn').click();
            await page.locator('[data-tags-definitions] article').first().waitFor();
            assert.equal(await page.locator('[data-tags-definitions] article').count(), 2);
            await page.locator('#historyTagsQuery').fill('unused');
            assert.equal(await page.locator('[data-tags-definitions] article').count(), 1);
            await page.locator('#historyTagsQuery').fill('');
            await page.locator('#historyTagsView [data-tags-create]').click();
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
            await page.locator('#historyTagsView [data-tags-create]').click();
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
            await page.evaluate(() => {
                globalThis.__tagsUi.failAssignment = true;
            });
            await page
                .locator('[data-tags-content]')
                .getByRole('button', { name: /Remove this tag from/ })
                .first()
                .click();
            await page.waitForFunction(() =>
                globalThis.document.querySelector('#historyTagsView [data-tags-status]').textContent.includes('failed')
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
            assert(await page.locator('#historyDetailsView .pinefetch-tag-return').isVisible());
            await page.locator('#historyDetailsView .pinefetch-tag-return').click();
            assert.equal(await page.locator('[data-tags-content] .pinefetch-history-item').count(), 30);
            await page.locator('#historyTagsQuery').fill('Clip 2');
            await page.waitForFunction(
                () =>
                    globalThis.__tagsUi.commands.findLast(call => call.command === 'get_tag_content')?.args.query ===
                    'Clip 2'
            );
            await page.waitForFunction(
                () => globalThis.document.querySelectorAll('[data-tags-content] .pinefetch-history-item').length < 20
            );
            await page.locator('#historyTagsQuery').fill('');
            await page.waitForFunction(
                () => globalThis.document.querySelectorAll('[data-tags-content] .pinefetch-history-item').length === 20
            );
            await page.locator('#tagContentTab').focus();
            await page.keyboard.press('ArrowRight');
            await page.locator('[data-tags-accounts] article').first().waitFor();
            assert.equal(await page.locator('#tagAccountsTab').getAttribute('aria-selected'), 'true');
            assert((await page.locator('[data-tags-accounts] article').count()) < 30);
            await page.locator('#historyTagsQuery').fill('Creator 1');
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
            assert(await page.locator('#historyAccountView .pinefetch-tag-return').isVisible());
            await page.locator('#historyAccountView .pinefetch-tag-return').click();
            assert.equal(await page.locator('#tagAccountsTab').getAttribute('aria-selected'), 'true');
            assert.equal(await page.locator('#historyTagsQuery').inputValue(), 'Creator 1');
            await page.locator('#tagContentTab').click();
            await page.locator('[data-tags-content] .pinefetch-history-open-btn').first().click();
            await page.locator('#historyDetailsContent').waitFor();
            await page.locator('#historyDetailsTags .pinefetch-tag-name').waitFor();
            await page.locator('#historyDetailsTags').getByRole('button', { name: '+ Add Tag', exact: true }).click();
            const picker = page.locator('#historyTagsDialog');
            await picker.getByRole('button', { name: 'Add tag Investigação', exact: true }).waitFor();
            assert.equal(await picker.getByRole('button', { name: 'Add tag Research', exact: true }).count(), 0);
            const additionsBeforeHold = await page.evaluate(
                () => globalThis.__tagsUi.commands.filter(call => call.command === 'add_history_tag').length
            );
            await page.evaluate(() => {
                globalThis.__tagsUi.holdAssignment = true;
            });
            await picker.getByRole('button', { name: 'Add tag Investigação', exact: true }).click();
            assert.equal(
                await picker.locator('[data-tags-assigned]').getByText('Investigação', { exact: true }).count(),
                0
            );
            await picker.getByRole('button', { name: 'Add tag Investigação', exact: true }).dispatchEvent('click');
            assert.equal(
                await page.evaluate(
                    () => globalThis.__tagsUi.commands.filter(call => call.command === 'add_history_tag').length
                ),
                additionsBeforeHold + 1
            );
            await page.evaluate(() => globalThis.__tagsUi.releaseAssignment());
            await picker.locator('[data-tags-assigned]').getByText('Investigação', { exact: true }).waitFor();
            await page.evaluate(() => {
                globalThis.__tagsUi.failAssignment = true;
            });
            await picker.getByRole('button', { name: 'Remove tag Investigação', exact: true }).click();
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('#historyTagsDialog [data-tags-status]')
                    .textContent.includes('failed')
            );
            assert(await picker.locator('[data-tags-assigned]').getByText('Investigação', { exact: true }).isVisible());
            await page.evaluate(() => {
                globalThis.__tagsUi.failAssignment = false;
            });
            await picker.getByRole('button', { name: 'Create New Tag', exact: true }).click();
            await editor.locator('#tagNameInput').fill('Fresh');
            await editor.locator('[type=submit]').click();
            await editor.waitFor({ state: 'hidden' });
            await picker.locator('[data-tags-assigned]').getByText('Fresh', { exact: true }).waitFor();
            await picker.getByRole('button', { name: 'Create New Tag', exact: true }).click();
            await editor.locator('#tagNameInput').fill('Overflow');
            await editor.locator('[type=submit]').click();
            await editor.waitFor({ state: 'hidden' });
            await picker.locator('[data-tags-assigned]').getByText('Overflow', { exact: true }).waitFor();

            await page.keyboard.press('Escape');
            await picker.waitFor({ state: 'hidden' });
            await page.locator('#historyDetailsTags').getByText('Fresh', { exact: true }).waitFor();
            await page.evaluate(async () => {
                const ui = globalThis.__tagsUi;
                ui.delayedTag = await globalThis.__TAURI__.core.invoke('create_tag', {
                    tag: { name: 'Delayed', description: '', color: '#123456' },
                });
            });
            await page.locator('#historyDetailsTags').getByRole('button', { name: '+ Add Tag', exact: true }).click();
            await picker.getByRole('button', { name: 'Add tag Delayed', exact: true }).waitFor();
            await page.evaluate(() => {
                globalThis.__tagsUi.holdAssignment = true;
            });
            await picker.getByRole('button', { name: 'Add tag Delayed', exact: true }).click();
            await page.keyboard.press('Escape');
            await page.locator('#historyDetailsView .pinefetch-tag-return').click();
            await page.getByRole('button', { name: 'Manage all 4 tags', exact: true }).waitFor();
            await page.locator('[data-tags-content] .pinefetch-history-open-btn').nth(1).click();
            await page.locator('#historyDetailsContent').waitFor();
            await page.locator('#historyDetailsTags').getByRole('button', { name: '+ Add Tag', exact: true }).click();
            await picker.getByRole('button', { name: 'Add tag Delayed', exact: true }).waitFor();
            assert(await picker.getByRole('button', { name: 'Add tag Delayed', exact: true }).isEnabled());
            await page.keyboard.press('Escape');
            await page.evaluate(() => globalThis.__tagsUi.releaseAssignment());
            await page.waitForFunction(() => {
                const ui = globalThis.__tagsUi;
                return ui.assignments.get('tag-entry-60').has(ui.delayedTag.id);
            });
            assert.equal(await page.locator('#historyDetailsTags').getByText('Delayed', { exact: true }).count(), 0);
            await page.locator('#historyDetailsView .pinefetch-tag-return').click();

            await page.locator('[data-tags-history]').click();
            await page.locator('#historyView').waitFor();
            await page.locator('#historyTagFilter').selectOption(researchId());
            await page.waitForFunction(
                () => globalThis.document.querySelector('#historyResultCount').textContent === '30 found'
            );
            await page.locator('#historySearchInput').fill('Clip 2');
            await page.waitForFunction(
                () =>
                    globalThis.__tagsUi.commands.findLast(call => call.command === 'get_history')?.args.query ===
                    'Clip 2'
            );
            await page.locator('#historyClearTagFilter').click();
            await page.waitForFunction(
                () => !globalThis.__tagsUi.commands.findLast(call => call.command === 'get_history')?.args.tagId
            );
            assert.equal(await page.locator('#historySearchInput').inputValue(), 'Clip 2');
            await page.locator('#historyTagsBtn').click();
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
                globalThis.document.querySelector('#historyTagsView [data-tags-status]').textContent.includes('failed')
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
            assert.deepEqual(errors, []);
            console.log(
                `[tags:ui] ${engine.name()}: CRUD, validation, assignments/acknowledgement/errors, chips, navigation, pagination, account grouping, filtering, keyboard, late replies and compact layout passed.`
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
