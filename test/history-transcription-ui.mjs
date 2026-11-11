import assert from 'node:assert/strict';
import { once } from 'node:events';
import { readFile } from 'node:fs/promises';
import { chromium, webkit } from 'playwright';
import { createDevServer } from '../scripts/dev-server.mjs';
import { installScreenshotBridge } from '../scripts/screenshot-fixtures.mjs';

// Synthetic native replies only: never starts Whisper, downloads a model or
// reads the user's saved videos, transcripts or database.
const { version } = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'));
const videos = ['Available video', 'Missing video', 'Retry video'].map((title, index) => ({
    title,
    url: `https://www.youtube.com/watch?v=transcription-fixture-${index}`,
    duration: 120,
    uploader: 'Synthetic creator',
    thumbnail: null,
}));
const installTranscriptionFixture = () => {
    const invoke = globalThis.__TAURI__.core.invoke;
    const listen = globalThis.__TAURI__.event.listen;
    const listeners = new Map();
    const rows = JSON.parse(sessionStorage.getItem('transcription-fixture') || '{}');
    const attempts = new Map();
    let attempt = 0;
    const ui = {
        calls: [],
        failStart: false,
        emit(name, payload) {
            for (const handler of listeners.get(name) || []) handler({ payload: structuredClone(payload) });
        },
        finish(id, status = 'completed') {
            const previous = attempts.get(id);
            if (!previous) throw new Error('No synthetic attempt to finish.');
            const progress = {
                ...previous,
                revision: previous.revision + 1,
                status,
                phase: status === 'completed' ? 'completed' : previous.phase,
                message: status === 'failed' ? 'Synthetic transcription failure.' : null,
            };
            attempts.set(id, progress);
            if (status === 'completed') {
                row(id).transcript =
                    '[00:00:00] A locally generated transcript.\n[00:00:03] The experiment was repeated.';
                save();
            }
            ui.emit('history:transcription', progress);
            if (status === 'completed') ui.emit('history:changed', { id });
        },
    };
    globalThis.__transcriptionUi = ui;
    const row = id => (rows[id] ||= { transcript: '', messages: [], summary: null });
    const save = () => sessionStorage.setItem('transcription-fixture', JSON.stringify(rows));
    globalThis.__TAURI__.event.listen = async (name, handler) => {
        if (!listeners.has(name)) listeners.set(name, new Set());
        listeners.get(name).add(handler);
        const cleanup = await listen(name, handler);
        return () => {
            listeners.get(name).delete(handler);
            cleanup();
        };
    };
    globalThis.__TAURI__.core.invoke = async (command, args = {}) => {
        ui.calls.push({ command, args: structuredClone(args) });
        if (command === 'get_codex_diagnostics') return { status: 'chatgpt', ready: true, version: '0.164.0' };
        if (command === 'get_history_details') {
            const details = await invoke(command, args);
            return {
                ...details,
                output_file_available: args.id !== 'history-2',
                transcript: row(args.id).transcript
                    ? { language: 'en', transcription_type: 'text with timestamps', file_available: true }
                    : null,
            };
        }
        if (command === 'get_history_transcript')
            return row(args.id).transcript
                ? {
                      text: row(args.id).transcript,
                      language: 'en',
                      transcription_type: 'text with timestamps',
                      file_available: true,
                  }
                : null;
        if (command === 'get_history_transcription_status') {
            const value = {
                history_id: args.id,
                transcript_available: Boolean(row(args.id).transcript),
                can_transcribe: args.id !== 'history-2' && !row(args.id).transcript,
                unavailable_reason: args.id === 'history-2' ? 'The saved video is unavailable.' : null,
                progress: attempts.get(args.id) || null,
            };
            return structuredClone(value);
        }
        if (command === 'start_history_transcription') {
            if (ui.failStart) {
                ui.failStart = false;
                throw new Error('The local Whisper runtime is unavailable.');
            }
            const progress = {
                history_id: args.id,
                attempt_id: `synthetic-${++attempt}`,
                attempt_at: attempt,
                revision: 0,
                status: 'running',
                phase: 'transcribing',
                model: 'base',
                message: null,
            };
            attempts.set(args.id, progress);
            ui.emit('history:transcription', progress);
            return structuredClone(progress);
        }
        if (command === 'cancel_history_transcription') {
            const current = attempts.get(args.id);
            assertAttempt(current, args.attemptId);
            ui.finish(args.id, 'cancelled');
            return structuredClone(attempts.get(args.id));
        }
        if (command === 'get_video_ai') {
            const value = row(args.historyId);
            return structuredClone({
                summary: value.summary,
                messages: value.messages,
                transcript_available: Boolean(value.transcript),
                transcript_hash: value.transcript ? 'a'.repeat(64) : null,
            });
        }
        if (command === 'ask_video') {
            const value = row(args.historyId);
            if (!value.transcript) throw new Error('Missing synthetic transcript.');
            value.messages.push(
                { role: 'user', content: args.question },
                { role: 'assistant', content: 'The experiment was repeated.' }
            );
            save();
            return { messages: structuredClone(value.messages) };
        }
        return invoke(command, args);
    };
    const assertAttempt = (current, id) => {
        if (!current || current.attempt_id !== id) throw new Error('Wrong synthetic attempt.');
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
                content: `(${installScreenshotBridge.toString()})(${JSON.stringify({ videos, version })}); (${installTranscriptionFixture.toString()})();`,
            });
            const page = await context.newPage();
            const errors = [];
            page.on('pageerror', error => errors.push(error.message));
            const action = page.locator('#historyTranscribeBtn');
            const cancel = page.locator('#historyTranscriptionCancelBtn');
            const status = page.locator('#historyTranscriptionStatus');
            const ai = name => page.locator(`[data-ai-${name}]`);
            const open = async id => {
                await page.locator('#viewHistoryBtn').click();
                await page.locator(`[data-history-id="${id}"] .pinefetch-history-open-btn`).click();
                await page.locator('#historyTranscriptTab').click();
            };
            const waitReady = () =>
                page.waitForFunction(() => !globalThis.document.getElementById('historyTranscribeBtn').disabled);
            await page.goto(origin);
            await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
            await open('history-1');
            await waitReady();
            await page.locator('#historyAiTab').click();
            await ai('transcribe').waitFor({ state: 'visible' });
            assert.equal(await ai('question').isDisabled(), true);
            await ai('transcribe').click();
            assert.equal(await page.locator('#historyTranscriptTab').getAttribute('aria-selected'), 'true');
            await page.waitForFunction(() => globalThis.document.activeElement.id === 'historyTranscribeBtn');
            await action.click();
            await cancel.waitFor({ state: 'visible' });
            assert.equal(await action.isDisabled(), true);

            // Jobs continue when navigating; completion must never replace the
            // newly selected video's details with the old result.
            await open('history-2');
            assert.equal(await action.isDisabled(), true);
            await page.evaluate(() => globalThis.__transcriptionUi.finish('history-1'));
            assert.equal(await page.locator('#historyDetailsTitle').textContent(), 'Missing video');
            assert.equal(await page.locator('#historyTranscriptText').textContent(), '');
            await page.waitForFunction(
                () => globalThis.document.getElementById('historyTranscriptionStatus').textContent.length > 0
            );
            assert.match(await status.textContent(), /unavailable|missing|saved video/i);
            await open('history-1');
            await page.waitForFunction(() =>
                globalThis.document.getElementById('historyTranscriptText').textContent.includes('locally generated')
            );
            assert.equal(await page.locator('#historyTranscriptionControls').isHidden(), true);
            await page.locator('#historyAiTab').click();
            await page.waitForFunction(() => !globalThis.document.querySelector('[data-ai-question]').disabled);
            assert.equal(await ai('summary').isHidden(), true);
            await ai('question').fill('What happened?');
            await ai('send').click();
            await page.waitForFunction(
                () => globalThis.document.querySelector('[data-ai-messages]').children.length === 2
            );

            // Failures and cancellation leave a retryable action and never
            // create a transcript. Retrying can complete without leaving Details.
            await open('history-3');
            await waitReady();
            await page.evaluate(() => {
                globalThis.__transcriptionUi.failStart = true;
            });
            await action.click();
            await waitReady();
            await page.waitForFunction(
                () => globalThis.document.getElementById('historyTranscriptionStatus').textContent.length > 0
            );
            await action.click();
            await cancel.waitFor({ state: 'visible' });
            await cancel.click();
            await cancel.waitFor({ state: 'hidden' });
            await waitReady();
            await action.click();
            await cancel.waitFor({ state: 'visible' });
            await page.locator('#historyAiTab').click();
            await ai('question').evaluate(element => {
                element.value = 'A draft to preserve';
                element.dispatchEvent(new Event('input', { bubbles: true }));
            });
            await page.evaluate(() => globalThis.__transcriptionUi.finish('history-3'));
            await page.waitForFunction(() => !globalThis.document.querySelector('[data-ai-question]').disabled);
            assert.equal(await ai('question').inputValue(), 'A draft to preserve');
            assert.equal(await ai('summary').isHidden(), true);

            // Local persistence is available after a new frontend session.
            await page.reload();
            await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
            await open('history-1');
            await page.waitForFunction(() =>
                globalThis.document.getElementById('historyTranscriptText').textContent.includes('locally generated')
            );
            await page.locator('#historyAiTab').click();
            await page.waitForFunction(
                () => globalThis.document.querySelector('[data-ai-messages]').children.length === 2
            );
            const calls = await page.evaluate(() => globalThis.__transcriptionUi.calls);
            assert.equal(
                calls.some(({ command }) => command === 'enqueue_download'),
                false
            );
            assert.equal(
                calls.some(({ command }) => command === 'analyze_video'),
                false
            );
            assert.deepEqual(errors, []);
            console.log(
                `${engine.name()}: local transcription, missing media, retry/cancel, background navigation, AI refresh and persistence passed`
            );
            await context.close();
        } finally {
            await browser.close();
        }
    }
} finally {
    await new Promise((resolve, reject) => server.close(error => (error ? reject(error) : resolve())));
}
