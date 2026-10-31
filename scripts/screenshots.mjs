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
const ocr = JSON.parse(await readFile(join(fixtureDirectory, 'ocr.json'), 'utf8'));
assert(
    videos.some(video => video.url === ocr.url),
    'The OCR fixture must belong to a video in test/links.json.'
);
ocr.frame = `data:image/jpeg;base64,${(await readFile(join(fixtureDirectory, ocr.frame))).toString('base64')}`;

const social = JSON.parse(await readFile(join(fixtureDirectory, 'social_insights.json'), 'utf8'));
const keywords = JSON.parse(await readFile(join(fixtureDirectory, 'keywords.json'), 'utf8'));
const tags = JSON.parse(await readFile(join(fixtureDirectory, 'tags.json'), 'utf8'));
assert.equal(social.url, ocr.url);
assert.equal(keywords.url, ocr.url);
assert.deepEqual(
    keywords.ocr,
    ocr.frames.map(frame => frame.full_text)
);
assert(Array.isArray(tags) && tags.length > 0, 'Add example tags to the screenshot fixtures.');
assert.equal(new Set(tags.map(tag => tag.id)).size, tags.length, 'Screenshot tag IDs must be unique.');
for (const tag of tags) {
    assert(tag.id && tag.name && /^#[\dA-F]{6}$/i.test(tag.color), 'Screenshot tags need an ID, name and HEX color.');
    assert(Array.isArray(tag.video_urls), 'Screenshot tags need a video_urls assignment list.');
    assert.equal(new Set(tag.video_urls).size, tag.video_urls.length, 'Screenshot tag assignments must be unique.');
    for (const url of tag.video_urls) {
        assert(videos.some(video => video.url === url), `Missing tagged screenshot video: ${url}`);
    }
}
const primaryTag = tags[0];
assert(primaryTag.video_urls.length > 0, 'The first screenshot tag must have saved content.');
const taggedHistoryIds = videos
    .flatMap((video, index) => (primaryTag.video_urls.includes(video.url) ? [`history-${index + 1}`] : []))
    .sort();

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
    await context.addInitScript(installScreenshotBridge, { videos, version, ocr, social, keywords, tags });
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
            for (const element of globalThis.document.querySelectorAll('img, .pinefetch-media-thumbnail')) {
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
        await page.evaluate(
            () =>
                new Promise(resolve =>
                    globalThis.requestAnimationFrame(() => globalThis.requestAnimationFrame(resolve))
                )
        );
        const png = await page.screenshot({ animations: 'disabled', caret: 'hide' });
        assert.deepEqual(errors, [], 'The screenshot page reported errors.');
        assert.deepEqual(
            await page.locator('.pinefetch-log-line.oj-status-error').allTextContents(),
            [],
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
            count => globalThis.document.querySelectorAll('.pinefetch-queue-item').length === count,
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
        count => globalThis.document.querySelectorAll('.pinefetch-history-item').length === count,
        links.length
    );
    await page.waitForFunction(
        count => globalThis.document.querySelector('#historyVideoCount').textContent === String(count),
        links.length
    );
    await capture('history.webp');

    await page.locator('.pinefetch-history-open-btn').first().click({ button: 'right' });
    await page.locator('#historyShowMoreDataBtn').click();
    await page.locator('#historyDetailsContent').waitFor({ state: 'visible' });
    await capture('more_data.webp');
    await page.locator('#historyDetailsHistoryBtn').click();

    const ocrHistoryId = `history-${videos.findIndex(video => video.url === ocr.url) + 1}`;
    await page.locator(`[data-history-id="${ocrHistoryId}"] .pinefetch-history-open-btn`).click({ button: 'right' });
    await page.locator('#historyOpenOcrBtn').click();
    await page.locator('#historyOcrPanel').waitFor({ state: 'visible' });
    await page.waitForFunction(
        () => globalThis.document.querySelector('#historyOcrStatus').textContent === 'OCR complete'
    );
    assert.deepEqual(
        await page.locator('#historyOcrText .oj-text').allTextContents(),
        ocr.frames.map(frame => frame.full_text),
        'The OCR screenshot must show all fixture frame texts.'
    );
    await page.locator('#historyOcrText .pinefetch-ocr-select').first().click();
    await page.locator('#historyOcrFrame').waitFor({ state: 'visible' });
    assert.equal(await page.locator('#historyOcrFrame').getAttribute('src'), ocr.frame);
    assert.equal(await page.locator('#historyOcrCopyBtn').isEnabled(), true);
    await capture('ocr.webp');

    await page.locator('#historySocialTab').click();
    await page.waitForFunction(
        () => globalThis.document.querySelectorAll('#historySocialMetrics .pinefetch-social-metric').length === 9
    );
    assert.equal(await page.locator('#historySocialLoadBtn').textContent(), 'Refresh Social Insights');
    await capture('social_insights.webp');

    await page.locator('#historyKeywordsTab').click();
    await page.waitForFunction(
        count => globalThis.document.querySelectorAll('#historyKeywordsRows tr').length === count,
        keywords.result.terms.length
    );
    assert.deepEqual(
        await page.locator('#historyKeywordsRows th').allTextContents(),
        keywords.result.terms.map(term => term.term)
    );
    await capture('keywords.webp');
    await page.locator('#historyDetailsAccountBreadcrumbBtn').click();
    await page.waitForFunction(
        () => globalThis.document.querySelector('#historyAccountSocialMetrics').childElementCount === 1
    );
    await capture('account.webp');
    await page.locator('#historyAccountHistoryBtn').click();
    await page.locator('#historyView').waitFor();

    await page.locator('#viewTagsBtn').click();
    const tagsView = page.locator('#tagsView');
    await tagsView.waitFor({ state: 'visible' });
    await page.waitForFunction(
        count =>
            globalThis.document.querySelectorAll('#tagsView .pinefetch-tag-definition').length === count &&
            globalThis.document.querySelector('#tagsView [data-tags-feedback]').hidden,
        tags.length
    );
    for (const tag of tags) {
        const row = tagsView.locator(`[data-tag-id="${tag.id}"]`);
        assert.equal(await row.locator('.pinefetch-tag-definition-name').textContent(), tag.name);
        assert.deepEqual(
            await row.locator('.pinefetch-tag-definition-counts span').allTextContents(),
            [`${tag.video_urls.length} saved items`, `${tag.video_urls.length} accounts`]
        );
    }
    await capture('tags.webp');

    await tagsView.locator(`[data-tag-id="${primaryTag.id}"] .pinefetch-tag-definition-open`).click();
    await page.locator('#tagContentPanel').waitFor({ state: 'visible' });
    await page.waitForFunction(
        count =>
            globalThis.document.querySelectorAll('#tagsView [data-tags-content] [data-history-id]').length === count &&
            globalThis.document.querySelector('#tagsView [data-tags-feedback]').hidden,
        taggedHistoryIds.length
    );
    const taggedContent = tagsView.locator('[data-tags-content] [data-history-id]');
    assert.deepEqual(
        (await taggedContent.evaluateAll(rows => rows.map(row => row.dataset.historyId))).sort(),
        taggedHistoryIds,
        'The tag Content screenshot must show the assigned fixture videos.'
    );
    assert.equal(await tagsView.locator('#tagsTitle .pinefetch-tag-name').textContent(), primaryTag.name);
    const tagChipNames = await taggedContent.locator('.pinefetch-tag-name').allTextContents();
    assert.equal(
        tagChipNames.filter(name => name === primaryTag.name).length,
        taggedHistoryIds.length,
        'Each tagged video must show its tag chip.'
    );
    await capture('tag_content.webp');

    await page.locator('#tagAccountsTab').click();
    await page.locator('#tagAccountsPanel').waitFor({ state: 'visible' });
    // These cached videos have no verified creator IDs, so each saved item stays separate.
    await page.waitForFunction(
        count =>
            globalThis.document.querySelectorAll('#tagsView [data-tags-accounts] [data-history-id]').length === count &&
            globalThis.document.querySelector('#tagsView [data-tags-feedback]').hidden,
        taggedHistoryIds.length
    );
    const taggedAccounts = tagsView.locator('[data-tags-accounts] [data-history-id]');
    assert.deepEqual(
        (await taggedAccounts.evaluateAll(rows => rows.map(row => row.dataset.historyId))).sort(),
        taggedHistoryIds,
        'The tag Accounts screenshot must preserve unverified creator entries.'
    );
    await capture('tag_accounts.webp');

    await page.locator('#viewSettingsBtn').click();
    await page.locator('#settingsImportTab').click();
    await page.waitForFunction(
        () => globalThis.document.querySelector('#linkDumpServerStatusBadge').textContent === 'Running'
    );
    await page.locator('.pinefetch-link-dump-secret-item').waitFor({ state: 'visible' });
    await capture('browser_import.webp');

    await page.locator('#viewSettingsBtn').click();
    await page.locator('#settingsDownloadsTab').click();
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
