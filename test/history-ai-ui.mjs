import assert from 'node:assert/strict';
import { once } from 'node:events';
import { readFile } from 'node:fs/promises';
import { chromium, webkit } from 'playwright';
import { createDevServer } from '../scripts/dev-server.mjs';
import { installScreenshotBridge } from '../scripts/screenshot-fixtures.mjs';

// Production markup/modules with an isolated native bridge. No live Codex,
// private transcripts, account credentials, network calls or user databases.
const { version } = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'));
const videos = ['First synthetic video', 'Second synthetic video'].map((title, index) => ({
    title,
    url: `https://www.youtube.com/watch?v=fixture-${index}`,
    duration: 120,
    uploader: 'Synthetic creator',
    thumbnail: null,
}));
const installAiFixture = () => {
    const original = globalThis.__TAURI__.core.invoke;
    const rows = JSON.parse(sessionStorage.getItem('ai-fixture') || '{}');
    const pending = new Map();
    const ui = {
        calls: [],
        diagnostics: { status: 'chatgpt', ready: true, version: '0.164.0' },
        transcripts: { 'history-1': true, 'history-2': true },
        holdSummary: false,
        holdChat: false,
        failSummary: '',
        failChat: '',
        answerText: '',
        summaryResult: null,
        releaseSummary: null,
        releaseChat: null,
        copied: '',
    };
    globalThis.__aiUi = ui;
    Object.defineProperty(globalThis.navigator, 'clipboard', {
        value: {
            writeText: async text => {
                ui.copied = text;
            },
        },
        configurable: true,
    });
    const row = id => (rows[id] ||= { summary: null, messages: [] });
    const save = () => sessionStorage.setItem('ai-fixture', JSON.stringify(rows));
    const wait = async (lane, args) => {
        const holdKey = lane === 'summary' ? 'holdSummary' : 'holdChat';
        if (!ui[holdKey]) return;
        ui[holdKey] = false;
        await new Promise((resolve, reject) => {
            pending.set(args.requestId, reject);
            ui[lane === 'summary' ? 'releaseSummary' : 'releaseChat'] = resolve;
        });
        pending.delete(args.requestId);
    };
    globalThis.__TAURI__.core.invoke = async (command, args = {}) => {
        ui.calls.push({ command, args: structuredClone(args) });
        if (command === 'get_codex_diagnostics') return structuredClone(ui.diagnostics);
        if (command === 'get_video_ai')
            return structuredClone({
                ...row(args.historyId),
                transcript_hash: 'a'.repeat(64),
                transcript_available: ui.transcripts[args.historyId],
            });
        if (command === 'analyze_video') {
            await wait('summary', args);
            if (ui.failSummary) throw new Error(ui.failSummary);
            row(args.historyId).summary = ui.summaryResult || {
                quick_summary:
                    'The saved transcript describes a reproducible experiment. It compares two methods. The speaker reports the measured outcome.',
                key_takeaways: [
                    'Results were repeated.',
                    'The speaker compared two methods.',
                    '<img src=x onerror="throw new Error()"> is plain source text.',
                ],
            };
            save();
            return structuredClone(row(args.historyId).summary);
        }
        if (command === 'ask_video') {
            await wait('chat', args);
            if (ui.failChat) throw new Error(ui.failChat);
            row(args.historyId).messages.push(
                { role: 'user', content: args.question },
                {
                    role: 'assistant',
                    content: ui.answerText || `Transcript answer for ${args.historyId}: the experiment was repeated.`,
                }
            );
            save();
            return { messages: structuredClone(row(args.historyId).messages) };
        }
        if (command === 'cancel_video_ai') {
            pending.get(args.requestId)?.(new Error('cancelled'));
            pending.delete(args.requestId);
            return true;
        }
        return original(command, args);
    };
};

const server = createDevServer();
server.listen(0, '127.0.0.1');
await once(server, 'listening');
const origin = `http://127.0.0.1:${server.address().port}`;
try {
    for (const engine of [chromium, webkit]) {
        const browser = await engine.launch();
        try {
            const context = await browser.newContext({
                viewport: { width: 1400, height: 1000 },
                reducedMotion: 'reduce',
            });
            await context.route('**/*', route => {
                const url = route.request().url();
                if (new URL(url).origin === origin) return route.continue();
                if (url === 'https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest')
                    return route.fulfill({ json: { tag_name: 'fixture' } });
                return route.abort('blockedbyclient');
            });
            await context.addInitScript({
                content: `(${installScreenshotBridge.toString()})(${JSON.stringify({ videos, version })}); (${installAiFixture.toString()})();`,
            });
            const page = await context.newPage();
            const errors = [];
            page.on('pageerror', error => errors.push(error.message));
            const ai = name => page.locator(`[data-ai-${name}]`);
            const openVideo = async id => {
                await page.locator('#viewHistoryBtn').click();
                await page.locator(`[data-history-id="${id}"] .pinefetch-history-open-btn`).click();
                await page.locator('#historyAiTab').click();
                await ai('chat').waitFor();
                await page.waitForFunction(
                    () => !globalThis.document.querySelector('[data-ai-setup]').textContent.includes('Checking')
                );
            };
            const question = async text => {
                await ai('question').fill(text);
                await ai('send').click();
            };
            const messageCount = () => page.locator('[data-ai-messages] > li').count();
            const waitMessages = count =>
                page.waitForFunction(
                    count =>
                        globalThis.document.querySelector('[data-ai-messages]').children.length === count &&
                        globalThis.document.querySelector('[data-ai-cancel-chat]').hidden,
                    count
                );
            const chatGeometry = () =>
                page.evaluate(() => {
                    const rect = selector => {
                        const { x, y, width, height, right, bottom } = globalThis.document
                            .querySelector(selector)
                            .getBoundingClientRect();
                        return { x, y, width, height, right, bottom };
                    };
                    return {
                        chat: rect('[data-ai-chat]'),
                        composer: rect('[data-ai-form]'),
                        messages: rect('[data-ai-messages]'),
                        question: rect('[data-ai-question]'),
                        viewport: { width: globalThis.innerWidth, height: globalThis.innerHeight },
                    };
                });
            const assertComposerVisible = async () => {
                const geometry = await chatGeometry();
                assert(geometry.composer.height > 0, JSON.stringify(geometry));
                assert(geometry.composer.bottom <= geometry.viewport.height + 1, JSON.stringify(geometry));
                assert(geometry.question.y >= geometry.chat.y, JSON.stringify(geometry));
                assert(geometry.messages.bottom <= geometry.composer.y + 1, JSON.stringify(geometry));
                assert(geometry.messages.height > 40, JSON.stringify(geometry));
            };
            await page.goto(origin);
            await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
            await openVideo('history-1');
            await ai('question').waitFor({ state: 'visible' });
            assert.equal(await ai('question').isEnabled(), true);
            assert.equal(await ai('question').getAttribute('maxlength'), '4000');
            assert.equal(await ai('summary').isHidden(), true);
            await page.locator('#historyAiTab').focus();
            await page.keyboard.press('ArrowLeft');
            assert.equal(await page.locator('#historyKeywordsTab').getAttribute('aria-selected'), 'true');
            await page.keyboard.press('ArrowRight');
            assert.equal(await page.locator('#historyAiTab').getAttribute('aria-selected'), 'true');
            await page.screenshot({ path: `/tmp/pinefetch-ai-initial-${engine.name()}.png` });
            assert.equal(
                await page.evaluate(
                    () => globalThis.__aiUi.calls.filter(call => call.command === 'analyze_video').length
                ),
                0
            );
            await question('What happened?');
            await waitMessages(2);
            assert.equal(await ai('summary').isHidden(), true);

            await page.reload();
            await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
            await openVideo('history-1');
            assert.equal(await messageCount(), 2);
            assert.equal(await ai('summary').isHidden(), true);
            await page.evaluate(() => {
                globalThis.__aiUi.holdSummary = true;
                globalThis.__aiUi.holdChat = true;
            });
            await ai('analyze').click();
            assert.equal(await ai('question').isEnabled(), true);
            await question('Explain the method.');
            await ai('cancel-chat').waitFor();
            assert.equal(await ai('cancel-summary').isVisible(), true);
            assert.equal(await ai('question').inputValue(), '');
            assert.equal(await ai('question').isDisabled(), true);
            assert.equal(await ai('send').getAttribute('aria-busy'), 'true');
            assert.equal(await ai('send').locator('.fa-arrow-up').isHidden(), true);
            await page.evaluate(() => globalThis.__aiUi.releaseSummary());
            await ai('summary').waitFor();
            assert.equal(await ai('cancel-chat').isVisible(), true);
            assert.equal(await ai('question').inputValue(), '');
            assert.equal(await ai('question').isDisabled(), true);
            assert.equal(await messageCount(), 3);
            assert.equal(await ai('takeaways').locator('img').count(), 0);
            await page.evaluate(() => globalThis.__aiUi.releaseChat());
            await waitMessages(4);
            assert.equal(await ai('question').isEnabled(), true);
            assert.equal(await ai('send').getAttribute('aria-busy'), 'false');
            assert.equal(await ai('send').locator('.fa-arrow-up').isVisible(), true);
            await ai('question').fill('Keep this next draft');
            const messages = await ai('messages').textContent();
            await ai('analyze').click();
            await page.waitForFunction(
                () => globalThis.document.querySelector('[data-ai-analyze]').getAttribute('aria-busy') === 'false'
            );
            assert.equal(await ai('summary-status').textContent(), '');
            assert.equal(await ai('messages').textContent(), messages);
            assert.equal(await ai('question').inputValue(), 'Keep this next draft');
            await ai('copy').click();
            await page.waitForFunction(() => globalThis.__aiUi.copied.includes('Key Takeaways'));

            await page.evaluate(() => {
                globalThis.__aiUi.failSummary = 'malformed JSON';
            });
            await ai('analyze').click();
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('[data-ai-summary-status]')
                    .textContent.includes('incomplete or unsupported')
            );
            await question('Can chat still answer?');
            await waitMessages(6);
            assert.match(await ai('summary-status').textContent(), /incomplete or unsupported/);
            assert.equal(await ai('chat-status').textContent(), '');
            await page.evaluate(() => {
                globalThis.__aiUi.failSummary = '';
                globalThis.__aiUi.holdSummary = true;
            });
            await ai('analyze').click();
            await ai('cancel-summary').click();
            await page.waitForFunction(() =>
                globalThis.document.querySelector('[data-ai-summary-status]').textContent.includes('cancelled')
            );
            assert.equal(await messageCount(), 6);

            const longAnswer = [
                '**Measured result** with a [source](https://example.org/transcript).',
                '- The first method was repeated.\n- The second method used the same sample.',
                '1. Compare the methods.\n2. Record the outcome.',
                '```text\nconst result = "<safe>";\n```',
                '<img src=x onerror="throw new Error()"> [unsafe](javascript:alert(1))',
                ...Array.from(
                    { length: 28 },
                    (_, index) => `Paragraph ${index + 1}. ${'Transcript evidence '.repeat(18)}`
                ),
            ].join('\n\n');
            await page.evaluate(answer => {
                globalThis.__aiUi.answerText = answer;
            }, longAnswer);
            await ai('question').fill('First line');
            await ai('question').press('Shift+Enter');
            await ai('question').press('End');
            await page.keyboard.insertText('Second line');
            assert.equal(await ai('question').inputValue(), 'First line\nSecond line');
            await ai('question').press('Enter');
            await waitMessages(8);
            const assistant = page.locator('.pinefetch-ai-message-assistant').last();
            assert((await assistant.locator('p').count()) >= 28);
            assert.equal(await assistant.locator('ul > li').count(), 2);
            assert.equal(await assistant.locator('ol > li').count(), 2);
            assert.equal(await assistant.locator('strong').first().textContent(), 'Measured result');
            assert.equal((await assistant.locator('pre code').textContent()).trimEnd(), 'const result = "<safe>";');
            assert.equal(
                await assistant
                    .locator(
                        '.pinefetch-ai-message-content img, .pinefetch-ai-message-content script, .pinefetch-ai-message-content [href^="javascript:"]'
                    )
                    .count(),
                0
            );
            await assistant.locator('.pinefetch-ai-message-copy').click();
            assert.equal(await page.evaluate(() => globalThis.__aiUi.copied), longAnswer);
            await assertComposerVisible();
            assert(await ai('messages').evaluate(element => element.scrollHeight > element.clientHeight));

            // An arriving answer must not move a reader inspecting older messages.
            await page.evaluate(() => {
                globalThis.__aiUi.holdChat = true;
            });
            await question('Keep my reading position.');
            await ai('cancel-chat').waitFor();
            await ai('messages').evaluate(element => {
                element.scrollTop = 0;
            });
            await page.evaluate(() => globalThis.__aiUi.releaseChat());
            await waitMessages(10);
            assert.equal(await ai('messages').evaluate(element => element.scrollTop), 0);
            await assertComposerVisible();

            // Stopping a request restores the composer after the request settles.
            await page.evaluate(() => {
                globalThis.__aiUi.holdChat = true;
            });
            await question('Cancel this question.');
            await ai('cancel-chat').waitFor();
            assert.equal(await ai('question').inputValue(), '');
            assert.equal(await ai('question').isDisabled(), true);
            assert.equal(await ai('send').locator('.fa-arrow-up').isHidden(), true);
            await ai('cancel-chat').click();
            await waitMessages(10);
            assert.equal(await ai('question').isEnabled(), true);
            assert.equal(await ai('send').getAttribute('aria-busy'), 'false');
            assert.equal(await ai('send').locator('.fa-arrow-up').isVisible(), true);

            // A failed question has an explicit retry and keeps the independent draft.
            await page.evaluate(() => {
                globalThis.__aiUi.failChat = 'Codex service is temporarily unavailable. Try again later.';
            });
            await question('Retry this exact question.');
            await ai('retry-chat').waitFor();
            assert.equal(await ai('question').inputValue(), '');
            assert.equal(await ai('question').isEnabled(), true);
            assert.equal(await ai('send').locator('.fa-arrow-up').isVisible(), true);
            await ai('question').fill('Keep this new draft');
            await page.evaluate(() => {
                globalThis.__aiUi.failChat = '';
                globalThis.__aiUi.holdChat = true;
            });
            await ai('retry-chat').click();
            assert.equal(await ai('question').isDisabled(), true);
            assert.equal(await ai('question').inputValue(), 'Keep this new draft');
            assert.equal(await ai('send').locator('.fa-arrow-up').isHidden(), true);
            await page.evaluate(() => globalThis.__aiUi.releaseChat());
            await waitMessages(12);
            assert.equal(await ai('question').inputValue(), 'Keep this new draft');
            assert.equal(await ai('question').isEnabled(), true);
            assert.equal(await ai('send').locator('.fa-arrow-up').isVisible(), true);
            assert.equal(
                await page.evaluate(
                    () => globalThis.__aiUi.calls.findLast(call => call.command === 'ask_video').args.question
                ),
                'Retry this exact question.'
            );

            await page.evaluate(() => {
                globalThis.__aiUi.summaryResult = {
                    quick_summary: [
                        `The transcript describes ${'a carefully repeated experiment '.repeat(30)}.`,
                        `The methods compare ${'the recorded observations '.repeat(30)}.`,
                        `The conclusion considers ${'the available evidence '.repeat(30)}.`,
                    ].join(' '),
                    key_takeaways: Array.from(
                        { length: 8 },
                        (_, index) =>
                            `Distinct insight ${index + 1}: ${'The saved transcript supplies supporting evidence. '.repeat(4)}`
                    ),
                };
            });
            await ai('analyze').click();
            await page.waitForFunction(
                () => globalThis.document.querySelector('[data-ai-takeaways]').children.length === 8
            );
            await assertComposerVisible();
            const messagesTop = await ai('messages').evaluate(element => element.scrollTop);
            assert(await ai('summary-scroll').evaluate(element => element.scrollHeight > element.clientHeight));
            await ai('summary-scroll').evaluate(element => {
                element.scrollTop = 200;
            });
            assert.equal(await ai('messages').evaluate(element => element.scrollTop), messagesTop);
            const columns = await page.evaluate(() => {
                const chat = globalThis.document.querySelector('[data-ai-chat]').getBoundingClientRect();
                const summary = globalThis.document.querySelector('[data-ai-summary-panel]').getBoundingClientRect();
                return { chat: chat.width, summary: summary.width, left: chat.right, right: summary.x };
            });
            assert(columns.left <= columns.right, JSON.stringify(columns));
            assert(Math.abs(columns.chat / (columns.chat + columns.summary) - 0.6) < 0.06, JSON.stringify(columns));

            for (const viewport of [
                { width: 1400, height: 1000 },
                { width: 1000, height: 700 },
                { width: 760, height: 700 },
                { width: 390, height: 700 },
                { width: 390, height: 560 },
            ]) {
                await page.setViewportSize(viewport);
                if (await ai('show-chat').isVisible()) await ai('show-chat').click();
                assert(
                    await page.evaluate(() => globalThis.document.documentElement.scrollWidth <= globalThis.innerWidth)
                );
                assert(await ai('chat').evaluate(element => element.scrollWidth <= element.clientWidth));
                assert(await ai('messages').evaluate(element => element.scrollWidth <= element.clientWidth));
                await assertComposerVisible();
                const shortHeight = await ai('question').evaluate(element => element.clientHeight);
                await ai('question').fill('A line in a growing composer.\n'.repeat(20));
                assert((await ai('question').evaluate(element => element.clientHeight)) > shortHeight);
                await assertComposerVisible();
                await ai('question').fill('Keep this next draft');
                if (await ai('show-summary').isVisible()) {
                    await ai('show-summary').click();
                    assert.equal(await ai('summary-panel').isVisible(), true);
                    assert.equal(await ai('takeaways').locator('li').count(), 8);
                    await ai('show-chat').click();
                    await assertComposerVisible();
                }
                await page.screenshot({
                    path: `/tmp/pinefetch-ai-chat-${viewport.width}-${viewport.height}-${engine.name()}.png`,
                });
            }
            await page.setViewportSize({ width: 1400, height: 1000 });
            await page.evaluate(() => {
                globalThis.__aiUi.holdSummary = true;
            });
            await ai('analyze').click();
            await page.setViewportSize({ width: 760, height: 700 });
            await ai('show-summary').click();
            await page.locator('#historyDetailsHistoryBtn').click();
            await openVideo('history-2');
            assert.equal(await ai('show-chat').getAttribute('aria-pressed'), 'true');
            await assertComposerVisible();
            await page.setViewportSize({ width: 1400, height: 1000 });
            assert.equal(await messageCount(), 0);
            assert.equal(await ai('summary').isHidden(), true);
            assert(
                await page.evaluate(() =>
                    globalThis.__aiUi.calls.some(
                        call => call.command === 'cancel_video_ai' && call.args.historyId === 'history-1'
                    )
                )
            );
            await page.locator('#historyDetailsHistoryBtn').click();
            await page.evaluate(() => {
                globalThis.__aiUi.transcripts['history-2'] = false;
            });
            await openVideo('history-2');
            assert.equal(await ai('question').isDisabled(), true);
            assert.equal(await ai('analyze').isDisabled(), true);
            assert.match(await ai('source-notice').textContent(), /saved transcript is required/i);

            await page.locator('#viewSettingsBtn').click();
            await page.locator('#settingsDiagnosticsTab').click();
            const status = page.locator('[data-codex-status]');
            await page.waitForFunction(
                () => globalThis.document.querySelector('[data-codex-status]').textContent === 'Connected'
            );
            assert.equal(await page.locator('[data-codex-version]').textContent(), '· v0.164.0');
            assert.equal(await page.locator('[data-codex-check]').getAttribute('aria-label'), 'Check Again');
            assert.equal(await page.locator('[data-codex-check]').getAttribute('data-oj-tooltip'), 'Check Again');
            for (const [state, label] of [
                ['not_installed', 'Not installed — install Codex'],
                ['unauthenticated', 'Sign in — run codex login'],
                ['api_key', 'Connected (API key)'],
                ['unknown_auth', 'Check login — run codex login status'],
                ['failed', 'Check failed — try again'],
            ]) {
                await page.evaluate(state => {
                    globalThis.__aiUi.diagnostics = { status: state, ready: state === 'api_key' };
                }, state);
                await page.locator('[data-codex-check]').click();
                await page.waitForFunction(
                    label => globalThis.document.querySelector('[data-codex-status]').textContent === label,
                    label
                );
                assert.equal(await status.textContent(), label);
                assert.equal(await page.locator('[data-codex-version]').isHidden(), true);
            }
            await page.screenshot({ path: `/tmp/pinefetch-ai-diagnostics-${engine.name()}.png` });
            assert.deepEqual(errors, []);
            await context.close();
            console.log(
                `${engine.name()}: transcript-first chat, persistence without summaries, independent concurrency/errors, regeneration, cancellation, video isolation, diagnostics and responsive UI passed`
            );
        } finally {
            await browser.close();
        }
    }
} finally {
    await new Promise(resolve => server.close(resolve));
}
