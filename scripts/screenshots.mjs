import assert from 'node:assert/strict';
import { once } from 'node:events';
import { mkdir, readdir, readFile, realpath, unlink } from 'node:fs/promises';
import { extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import sharp from 'sharp';
import { createDevServer } from './dev-server.mjs';
import { installScreenshotBridge } from './screenshot-fixtures.mjs';

const outputDirectory = fileURLToPath(new URL('../src/images/mockups/', import.meta.url));
const fixtureDirectory = fileURLToPath(new URL('../test/fixtures/screenshots/', import.meta.url));
const imageExtensions = new Set([
    '.webp',
    '.png',
    '.jpg',
    '.jpeg',
    '.gif',
    '.svg',
    '.avif',
    '.bmp',
    '.tif',
    '.tiff',
    '.ico',
]);
const links = JSON.parse(await readFile(new URL('../test/links.json', import.meta.url), 'utf8'));
assert(
    Array.isArray(links) && links.length > 0 && links.every(url => typeof url === 'string'),
    'test/links.json must contain URLs.'
);
const metadata = JSON.parse(await readFile(join(fixtureDirectory, 'videos.json'), 'utf8'));
const videos = await Promise.all(
    links.map(async (url, index) => {
        const video = metadata.find(video => video.url === url);
        assert(video, `Add metadata and a thumbnail for ${url} to test/fixtures/screenshots/videos.json.`);
        const thumbnail = await readFile(join(fixtureDirectory, video.thumbnail));
        return {
            ...video,
            duration: 510 + index * 72,
            thumbnail: `data:image/jpeg;base64,${thumbnail.toString('base64')}`,
        };
    })
);
const { version } = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'));

await mkdir(outputDirectory, { recursive: true });
// Refuse a redirected output directory; unlinking images must stay in this checkout.
assert.equal(
    await realpath(outputDirectory),
    outputDirectory.replace(/\/$/, ''),
    'Mockup directory must not be a symlink.'
);
for (const entry of await readdir(outputDirectory, { withFileTypes: true })) {
    if ((entry.isFile() || entry.isSymbolicLink()) && imageExtensions.has(extname(entry.name).toLowerCase())) {
        await unlink(join(outputDirectory, entry.name));
    }
}
console.log('[screenshots] Cleared images in src/images/mockups.');

const server = createDevServer();
let browser;
try {
    server.listen(0, '127.0.0.1');
    await once(server, 'listening');
    const origin = `http://127.0.0.1:${server.address().port}`;
    browser = await chromium.launch();
    const context = await browser.newContext({
        viewport: { width: 1400, height: 1080 },
        deviceScaleFactor: 2,
        locale: 'en-GB',
        timezoneId: 'Europe/Vienna',
        colorScheme: 'dark',
        reducedMotion: 'reduce',
    });
    // Keep captures offline, including the Settings version check.
    await context.route('**/*', async route => {
        const url = route.request().url();
        if (new URL(url).origin === origin) return route.continue();
        if (url === 'https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest') {
            return route.fulfill({ json: { tag_name: '2026.09.19' } });
        }
        return route.abort('blockedbyclient');
    });
    await context.addInitScript(installScreenshotBridge, { videos, version });
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => {
        if (message.type() === 'error') errors.push(message.text());
    });
    await page.clock.setFixedTime(new Date('2026-09-20T12:00:00Z'));
    await page.goto(origin);
    await page.waitForFunction(() => globalThis.__pinefetchScreenshot?.ready);

    const capture = async filename => {
        // CSS backgrounds are not covered by document.images: decode both kinds.
        await page.evaluate(async () => {
            await globalThis.document.fonts.ready;
            const urls = new Set();
            for (const element of globalThis.document.querySelectorAll('img, .pf-media-thumbnail')) {
                if (!element.checkVisibility()) continue;
                const background = globalThis.getComputedStyle(element).backgroundImage;
                const url = element.tagName === 'IMG' ? element.src : background.match(/^url\(["']?(.*?)["']?\)$/)?.[1];
                if (url) urls.add(url);
            }
            await Promise.all(
                [...urls].map(async url => {
                    const image = new globalThis.Image();
                    image.src = url;
                    await image.decode();
                })
            );
            globalThis.document.activeElement?.blur();
        });
        await page.mouse.move(0, 0);
        const png = await page.screenshot({ animations: 'disabled', caret: 'hide' });
        assert.deepEqual(errors, [], 'The screenshot page reported errors.');
        assert.equal(
            await page.locator('.pf-log-line.pf-status-error').count(),
            0,
            'A screenshot fixture command failed.'
        );
        await sharp(png).webp({ quality: 90, effort: 6 }).toFile(join(outputDirectory, filename));
        console.log(`[screenshots] ${filename}`);
    };

    for (const [index, url] of links.entries()) {
        await page.locator('#urlInput').fill(url);
        await page.locator('#loadInfoBtn').click();
        await page.waitForFunction(
            title => globalThis.document.querySelector('#infoTitle').textContent === title,
            videos[index].title
        );
        await page.locator('#startDownloadBtn').click();
        await page.waitForFunction(
            count => globalThis.document.querySelectorAll('.pf-queue-item').length === count,
            index + 1
        );
    }
    assert.deepEqual(await page.evaluate(() => globalThis.__pinefetchScreenshot.queuedUrls), links);
    // Leave a populated preview beside the complete queue.
    await page.locator('#urlInput').fill(links.at(-1));
    await page.locator('#loadInfoBtn').click();
    await page.waitForFunction(
        title => globalThis.document.querySelector('#infoTitle').textContent === title,
        videos.at(-1).title
    );
    await capture('download.webp');

    await page.locator('#viewHistoryBtn').click();
    await page.waitForFunction(
        count => globalThis.document.querySelectorAll('.pf-history-item').length === count,
        links.length
    );
    await page.waitForFunction(
        count => globalThis.document.querySelector('#historyVideoCount').textContent === String(count),
        links.length
    );
    await capture('history.webp');

    await page.locator('.pf-history-open-btn').first().click({ button: 'right' });
    await page.locator('#historyShowMoreDataBtn').click();
    await page.locator('#historyDetailsContent').waitFor({ state: 'visible' });
    await capture('more_data.webp');
    await page.locator('#historyDetailsCloseBtn').click();

    await page.locator('#viewLinkDumpBtn').click();
    await page.waitForFunction(
        () => globalThis.document.querySelector('#linkDumpServerStatusBadge').textContent === 'Running'
    );
    await page.locator('.pf-link-dump-secret-item').waitFor({ state: 'visible' });
    await capture('browser_import.webp');

    await page.locator('#viewSettingsBtn').click();
    await page.waitForFunction(
        () => globalThis.document.querySelector('#ytDlpLatestVersion').textContent === 'Latest: 2026.09.19'
    );
    await capture('settings.webp');
    await context.close();
} finally {
    try {
        await browser?.close();
    } finally {
        server.closeAllConnections();
        if (server.listening) {
            await new Promise((resolve, reject) => server.close(error => (error ? reject(error) : resolve())));
        }
    }
}
