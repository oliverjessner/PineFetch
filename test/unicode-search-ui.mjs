import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const cases = JSON.parse(await readFile(new URL('./fixtures/unicode-search.json', import.meta.url)));

// Real modules and DOM in both engines. SQL is covered independently by the
// shared native corpus; this page uses no native bridge or personal data.
export const checkUnicodeSearch = async (browser, origin) => {
    const context = await browser.newContext({ locale: 'tr-TR', viewport: { width: 900, height: 700 } });
    try {
        const page = await context.newPage();
        const errors = [];
        page.on('pageerror', error => errors.push(error.message));
        await page.route('**/*', route => {
            const url = route.request().url();
            if (url === origin + '/unicode-search-fixture')
                return route.fulfill({
                    contentType: 'text/html',
                    body: '<!doctype html><html lang="en"><body><input id="query"><button id="copy">Copy all</button><p id="status"></p><p id="count"></p><div id="viewport" tabindex="0"><ul id="list"></ul></div></body></html>',
                });
            return url.startsWith(origin + '/') ? route.continue() : route.abort();
        });
        await page.goto(origin + '/unicode-search-fixture');
        const mismatches = await page.evaluate(async corpus => {
            const { searchContains, searchMatches, trimSearchQuery } = await import('/unicode-search.js');
            const { highlightText, createTextReader } = await import('/history-text-view.js');
            const failures = [];
            for (const item of corpus) {
                if (searchContains(item.text, item.query) !== item.match) failures.push(item);
                if (trimSearchQuery(item.query) && searchMatches(item.text, item.query).length > 0 !== item.match)
                    failures.push(item);
                const text = item.text || '';
                const element = globalThis.document.createElement('p');
                highlightText(element, text, item.query);
                if (element.textContent !== text) failures.push({ text, reason: 'source changed during highlighting' });
            }
            const source =
                '[00:00:01.000]\nStraße 🌲\n[00:00:02.000]\nO\u0308sterreich\n[00:00:03.000]\nCAFÉ\n[00:00:04.000]\nΟΣ';
            globalThis.__unicodeSource = source;
            Object.defineProperty(navigator, 'clipboard', {
                configurable: true,
                value: {
                    writeText: async text => {
                        globalThis.__unicodeCopied = text;
                    },
                },
            });
            const element = id => globalThis.document.getElementById(id);
            const reader = createTextReader({
                input: element('query'),
                copy: element('copy'),
                status: element('status'),
                count: element('count'),
                viewport: element('viewport'),
                list: element('list'),
                onSelect: seconds => {
                    globalThis.__unicodeSeek = seconds;
                },
                appendLog: message => {
                    failures.push(message);
                },
            });
            reader.set(source);
            return failures;
        }, cases);
        assert.deepEqual(mismatches, []);
        const input = page.locator('#query');
        for (const [query, highlighted] of [
            ['STRASSE', 'Straße'],
            ['österreich', 'O\u0308sterreich'],
            ['cafe\u0301', 'CAFÉ'],
            ['οσ', 'ΟΣ'],
        ]) {
            await input.fill(query);
            assert.equal(await page.locator('#count').textContent(), '1 entry found');
            assert.deepEqual(await page.locator('#list mark').allTextContents(), [highlighted]);
        }
        await input.fill('STRASSE');
        await page.locator('#list button').click();
        assert.equal(await page.evaluate(() => globalThis.__unicodeSeek), 1);
        await page.locator('#copy').click();
        await page.waitForFunction(() => globalThis.__unicodeCopied === globalThis.__unicodeSource);
        await input.fill('cafe');
        assert.equal(await page.locator('#count').textContent(), '0 entries found');
        assert.match(await page.locator('#list').textContent(), /No text matches/);
        assert.deepEqual(errors, []);
    } finally {
        await context.close();
    }
};
