import assert from 'node:assert/strict';
import { once } from 'node:events';
import { readFile } from 'node:fs/promises';
import { chromium, webkit } from 'playwright';
import { createDevServer } from '../scripts/dev-server.mjs';
import { installScreenshotBridge } from '../scripts/screenshot-fixtures.mjs';

// Native responses and persistence are controlled fixtures. Real SQLite/process
// restart behavior is covered separately by queue_persistence_tests in Rust.
const { version } = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'));
const installQueueFixture = () => {
    const original = globalThis.__TAURI__.core.invoke;
    const listeners = new Map();
    const clearCase = new URLSearchParams(globalThis.location.search).has('clear');
    const job = (id, suffix = '') => ({
        id,
        url: 'https://example.com/' + id,
        output_dir: '/fixture/Grüße',
        format: 'ba/b',
        extract_audio: true,
        audio_format: 'mp3',
        transcribe_text: true,
        transcribe_timestamps: true,
        download_video_with_transcript: true,
        faster_whisper_model: 'small',
        save_captions: true,
        save_creator_subtitles: true,
        save_thumbnails: false,
        cut_start_time: 12.5,
        filename_suffix: '_timestamps' + suffix,
    });
    const initial = {
        revision: 0,
        pending: clearCase ? [] : [job('waiting-one'), job('waiting-two')],
        active: null,
        interrupted: [
            {
                job: job('interrupted'),
                output_path: '/fixture/kept.mp4',
                error: 'Interrupted when PineFetch closed. Existing output files are preserved.',
            },
        ],
    };
    let saved = JSON.parse(sessionStorage.getItem('queue-native-fixture') || 'null') || initial;
    const emit = (name, payload) => {
        for (const handler of listeners.get(name) || []) handler({ payload: structuredClone(payload) });
    };
    const commit = () => {
        saved.revision += 1;
        sessionStorage.setItem('queue-native-fixture', JSON.stringify(saved));
        emit('queue:update', saved);
    };
    const fixture = { failRetry: false, retries: [], cancellations: [], saved: () => structuredClone(saved) };
    globalThis.__queueNative = fixture;
    globalThis.__TAURI__.event.listen = async (name, handler) => {
        if (!listeners.has(name)) listeners.set(name, new Set());
        listeners.get(name).add(handler);
        return () => listeners.get(name).delete(handler);
    };
    globalThis.__TAURI__.core.invoke = async (command, args = {}) => {
        if (command === 'get_queue') return structuredClone(saved);
        if (command === 'get_queue_status') return { auto_start: false, paused: true, worker_running: false };
        if (command === 'load_info') return { title: args.url, uploader: 'Fixture', duration: 20, formats: [] };
        if (command === 'retry_interrupted_download') {
            if (fixture.failRetry) throw new Error('Synthetic queue write failure');
            const found = saved.interrupted.find(entry => entry.job.id === args.id);
            if (!found) throw new Error('Interrupted job not found');
            const queued = { ...found.job, id: 'requeued' };
            fixture.retries.push(structuredClone(queued));
            saved = { ...saved, pending: [...saved.pending, queued], interrupted: [] };
            commit();
            return queued.id;
        }
        if (command === 'cancel_download') {
            fixture.cancellations.push(args.id);
            saved = {
                ...saved,
                pending: saved.pending.filter(job => job.id !== args.id),
                interrupted: saved.interrupted.filter(entry => entry.job.id !== args.id),
            };
            commit();
            emit('download:state', { id: args.id, state: 'cancelled' });
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
            for (const clearCase of [false, true]) {
                const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
                const page = await context.newPage();
                const errors = [];
                page.on('pageerror', error => errors.push(error.message));
                await page.route('**/*', route =>
                    route.request().url().startsWith(origin) ? route.continue() : route.abort()
                );
                await page.addInitScript({
                    content:
                        '(' +
                        installScreenshotBridge.toString() +
                        ')(' +
                        JSON.stringify({ videos: [], version }) +
                        ');(' +
                        installQueueFixture.toString() +
                        ')();',
                });
                await page.goto(origin + (clearCase ? '/?clear=1' : '/'));
                await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
                await page.locator('[data-job-id="interrupted"]').waitFor();
                assert.match(
                    await page.locator('[data-job-id="interrupted"] .pinefetch-queue-badge').textContent(),
                    /Interrupted/
                );
                assert.match(await page.locator('#queueAutoStartBtn').textContent(), /off/);
                assert.match(await page.locator('#pauseQueueBtn').textContent(), /Resume/);
                if (clearCase) {
                    await page.locator('#clearQueueBtn').click();
                    await page.waitForFunction(() => globalThis.__queueNative.cancellations.includes('interrupted'));
                    await page.reload();
                    await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
                    await page.evaluate(
                        () =>
                            new Promise(resolve =>
                                globalThis.requestAnimationFrame(() => globalThis.requestAnimationFrame(resolve))
                            )
                    );
                    assert.equal(await page.locator('.pinefetch-queue-item').count(), 0);
                } else {
                    assert.equal(await page.locator('.pinefetch-queue-item').count(), 3);
                    const original = await page.evaluate(() => globalThis.__queueNative.saved().interrupted[0].job);
                    await page.evaluate(() => {
                        globalThis.__queueNative.failRetry = true;
                    });
                    await page.locator('[data-job-id="interrupted"] .pinefetch-queue-more-btn').click();
                    await page.locator('#queueContextRetryBtn').click();
                    await page.waitForFunction(() =>
                        globalThis.document
                            .querySelector('[data-job-id="interrupted"]')
                            ?.textContent.includes('Could not queue again')
                    );
                    assert.equal(await page.locator('.pinefetch-queue-item').count(), 3);
                    await page.evaluate(() => {
                        globalThis.__queueNative.failRetry = false;
                    });
                    await page.locator('[data-job-id="interrupted"] .pinefetch-queue-more-btn').click();
                    await page.locator('#queueContextRetryBtn').click();
                    await page.locator('[data-job-id="requeued"]').waitFor();
                    assert.equal(await page.locator('[data-job-id="interrupted"]').count(), 0);
                    const retried = await page.evaluate(() => globalThis.__queueNative.retries[0]);
                    assert.deepEqual(retried, { ...original, id: 'requeued' });
                    await page.reload();
                    await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
                    await page.locator('[data-job-id="requeued"]').waitFor();
                    assert.equal(await page.locator('.pinefetch-queue-item').count(), 3);
                    assert.match(await page.locator('#queueAutoStartBtn').textContent(), /off/);
                    assert.match(await page.locator('#pauseQueueBtn').textContent(), /Resume/);
                    await page.locator('[data-job-id="waiting-one"] .pinefetch-queue-more-btn').click();
                    await page.locator('#queueContextRemoveBtn').click();
                    await page.reload();
                    await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);
                    await page.locator('[data-job-id="requeued"]').waitFor();
                    assert.equal(await page.locator('[data-job-id="waiting-one"]').count(), 0);
                }
                assert.deepEqual(errors, []);
                await context.close();
            }
            console.log(
                engine.name() + ': restored queue, interrupted retry/failure, exact options, clear and reload passed'
            );
        } finally {
            await browser.close();
        }
    }
} finally {
    await new Promise(resolve => server.close(resolve));
}
