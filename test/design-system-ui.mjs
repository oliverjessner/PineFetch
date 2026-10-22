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

const keywordFixture = JSON.parse(
    await readFile(new URL('./fixtures/screenshots/keywords.json', import.meta.url), 'utf8')
).result;

const installUiResponses = ({ fallbackFrame, playlistVideos, keywordFixture }) => {
    const invoke = globalThis.__TAURI__.core.invoke;
    const listen = globalThis.__TAURI__.event.listen;
    const listeners = new Map();
    let config;
    let historySources = ['youtube', 'tiktok'];
    let historyEntries = null;
    let historyCleared = false;
    let releasePresets;
    const presetsReady = new Promise(resolve => {
        releasePresets = resolve;
    });
    const ui = {
        commands: [],
        copied: '',
        ocrRecords: {},
        socialRecords: {},
        accountRecords: {},
        accountFetches: 0,
        holdAccount: false,
        releaseAccount: null,
        failAccount: false,
        unavailableAccount: false,
        profileFetches: 0,
        holdProfileVideos: false,
        releaseProfileVideos: null,
        failProfileVideos: false,
        emptyProfileVideos: false,
        failAccountRead: false,
        keywordRecords: { 'history-1': keywordFixture },
        holdKeywords: false,
        releaseKeywords: null,
        failKeywords: false,
        socialFetches: 0,
        socialScore: null,
        holdSocial: false,
        releaseSocial: null,
        failSocial: false,
        unsupportedSocial: false,
        ocrProgress: {},
        ocrAttempt: 0,
        holdOcr: false,
        releaseOcr: null,
        holdFrame: false,
        releaseFrame: null,
        failNextFrame: false,
        ocrSourceError: null,
        frameImages: {},
        failNextOcrDelete: false,
        duplicateIds: [],
        holdDuplicatePreview: false,
        releaseDuplicatePreview: null,
        holdDuplicateRemoval: false,
        releaseDuplicateRemoval: null,
        failDuplicateRemoval: false,
        failNextModelSave: false,
        failBrowserAccess: false,
        holdSubtitle: false,
        releaseSubtitle: null,
        failNextSubtitle: false,
        subtitleTracks: [
            {
                path: '/fixture/video.en.vtt',
                language: 'en',
                format: 'vtt',
                file_available: false,
                source_start_seconds: 0,
                text: 'WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nCreator <img src=x onerror="alert(1)"> 🌲',
            },
            {
                path: '/fixture/video.de.srt',
                language: 'de',
                format: 'srt',
                file_available: true,
                source_start_seconds: 61,
                text: '1\n00:00:01,000 --> 00:00:02,000\nGrüße aus dem Original',
            },
        ],
        holdPlaylist: false,
        releasePlaylist: null,
        failNextPlaylist: false,
        playlistVideos,
        clipboard: '',
        navigationRequest: { revision: 1, view: 'settings' },
        releasePresets,
        setHistorySources: sources => {
            historySources = [...sources];
        },
        setHistoryEntries: entries => {
            historyEntries = entries;
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
        if (command === 'get_navigation_request') return ui.navigationRequest;
        if (command === 'read_clipboard_text') return ui.clipboard;
        if (command === 'enqueue_playlist') {
            if (ui.holdPlaylist) {
                ui.holdPlaylist = false;
                await new Promise(resolve => {
                    ui.releasePlaylist = resolve;
                });
            }
            if (ui.failNextPlaylist) {
                ui.failNextPlaylist = false;
                throw new Error('Fixture playlist is unavailable.');
            }
            const jobs = [];
            for (const video of ui.playlistVideos) {
                const job = {
                    ...args.request,
                    url: video.url,
                    title: video.title,
                    thumbnail: video.thumbnail,
                    duration_seconds: video.duration,
                    uploader: video.uploader,
                    cut_start_time: null,
                };
                const id = await invoke('enqueue_download', { request: job });
                jobs.push({ ...job, id });
            }
            return { title: 'Fixture playlist', jobs, skipped_count: 1 };
        }
        if (command === 'get_download_presets') await presetsReady;
        if (command === 'get_config') {
            config ||= {
                ...(await invoke(command, args)),
                selected_preset_key: 'audio_opus',
                faster_whisper_model: 'medium',
                youtube_cookie_browser: 'firefox',
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
        if (command === 'check_youtube_cookie_access') {
            if (ui.failBrowserAccess) throw new Error('Synthetic private cookie diagnostic');
            return;
        }
        if (command === 'get_history_stats') {
            if (historyCleared) {
                return { video_count: 0, total_file_size_bytes: 0, total_duration_seconds: 0, source_counts: [] };
            }
            const stats = await invoke(command, args);
            return { ...stats, source_counts: historySources.map(source => ({ source, count: 1 })) };
        }
        if (command === 'get_history' && historyCleared) return { entries: [], has_more: false, total_count: 0 };
        if (command === 'get_history' && historyEntries) {
            const filtered = historyEntries.filter(entry => {
                if (args.source && entry.source !== args.source) return false;
                if (!args.query || args.searchField !== 'user') return true;
                if (args.exactUser) return entry.uploader === args.query;
                return entry.uploader?.toLowerCase().includes(args.query.toLowerCase());
            });
            const offset = args.offset || 0;
            const end = offset + args.limit;
            return {
                entries: filtered.slice(offset, end),
                has_more: filtered.length > end,
                total_count: filtered.length,
            };
        }
        if (command === 'get_history_details') {
            const customEntry = historyEntries?.find(entry => entry.id === args.id);
            if (customEntry)
                return { entry: customEntry, output_file_available: false, transcript: null, captions: [] };
            const details = await invoke(command, args);
            if (args.id === 'history-2')
                return { ...details, entry: { ...details.entry, thumbnail: null }, transcript: null, captions: [] };
            return {
                ...details,
                entry: { ...details.entry, source: ui.detailSource || details.entry.source },
                captions: [
                    ...details.captions,
                    { media_path: '/fixture/alternate.mp4', caption_path: '/fixture/alternate.caption.txt' },
                ].slice(0, ui.detailCaptionCount ?? 2),
                subtitles: ui.subtitleTracks.map(
                    ({ path, language, format, file_available, source_start_seconds }) => ({
                        path,
                        language,
                        format,
                        file_available,
                        source_start_seconds,
                    })
                ),
            };
        }
        if (command === 'get_history_subtitle') {
            if (ui.holdSubtitle) {
                ui.holdSubtitle = false;
                await new Promise(resolve => {
                    ui.releaseSubtitle = resolve;
                });
            }
            if (ui.failNextSubtitle) {
                ui.failNextSubtitle = false;
                throw new Error('Synthetic private subtitle error');
            }
            return ui.subtitleTracks.find(track => track.path === args.path) || null;
        }
        const ocrPage = request => {
            const content = structuredClone(ui.ocrRecords[request.id] || null);
            if (!content) return null;
            const frames = content.frames || [];
            const revision = content.result_created_at ? JSON.stringify(frames) : null;
            if (request.resultRevision && revision !== request.resultRevision) throw new Error('Results changed');
            const search = (request.search || '').trim().toLocaleLowerCase();
            const filtered = search
                ? frames.filter(frame => frame.full_text.toLocaleLowerCase().includes(search))
                : frames;
            const offset = request.offset || 0;
            return {
                ...content,
                result_revision: revision,
                result_format: content.text ? 'legacy_text' : 'frames',
                frames: filtered.slice(offset, offset + (request.limit || 100)),
                total_count: frames.length,
                matched_count: filtered.length,
                offset,
                has_more: offset + (request.limit || 100) < filtered.length,
            };
        };
        if (command === 'get_history_keywords') {
            const result = structuredClone({
                history_entry_id: args.id,
                ...(ui.keywordRecords[args.id] || {
                    sources: { transcript: false, ocr_frames: 0, ocr_legacy: false, captions: 0 },
                    terms: [],
                }),
            });
            if (ui.failKeywords) throw new Error('Fixture SQLite read failed');
            if (ui.holdKeywords) {
                ui.holdKeywords = false;
                return new Promise(resolve => {
                    ui.releaseKeywords = () => resolve(result);
                });
            }
            return result;
        }
        if (command === 'get_history_account') {
            if (ui.failAccountRead) throw new Error('Fixture account read failed');
            const entries = historyEntries || (await invoke('get_history', { offset: 0, limit: 100 })).entries;
            const entry = entries.find(entry => entry.id === args.id);
            if (!entry) throw new Error('Missing fixture account');
            const matching = entries.filter(row => row.uploader === entry.uploader && row.source === entry.source);
            const offset = args.offset || 0;
            return {
                source: entry.source,
                uploader: entry.uploader,
                saved: {
                    entries: matching.slice(offset, offset + 20),
                    total_count: matching.length,
                    has_more: matching.length > offset + 20,
                },
                insights: structuredClone(ui.accountRecords[entry.uploader] || null),
            };
        }
        if (command === 'load_history_account_insights') {
            ui.accountFetches++;
            const account = await globalThis.__TAURI__.core.invoke('get_history_account', { id: args.id });
            const complete = () => {
                if (ui.failAccount) throw new Error('Fixture account network failure');
                const snapshot = {
                    source: account.source,
                    uploader: account.uploader,
                    profile_url: 'https://www.youtube.com/@fixture',
                    fetched_at: 1700000000000,
                    fetchary_version: '2.0.4',
                    data: {
                        platform: account.source,
                        kind: 'channel',
                        url: 'https://www.youtube.com/@fixture',
                        status: 'partial',
                        metrics: {
                            views: null,
                            likes: null,
                            comments: null,
                            shares: null,
                            saves: null,
                            followers: null,
                            following: null,
                            posts: 0,
                            subscribers: 12345,
                        },
                        sources: {},
                        unavailable: { likes: 'unsupported' },
                    },
                };
                if (ui.unavailableAccount) {
                    snapshot.data.metrics = Object.fromEntries(
                        Object.keys(snapshot.data.metrics).map(name => [name, null])
                    );
                    snapshot.data.status = 'unavailable';
                    snapshot.data.unavailable.followers = 'identity_mismatch';
                }
                ui.accountRecords[account.uploader] = snapshot;
                return structuredClone(snapshot);
            };
            if (ui.holdAccount) {
                ui.holdAccount = false;
                return new Promise((resolve, reject) => {
                    ui.releaseAccount = () => {
                        try {
                            resolve(complete());
                        } catch (error) {
                            reject(error);
                        }
                    };
                });
            }
            return complete();
        }
        if (command === 'load_history_account_videos') {
            ui.profileFetches++;
            const complete = () => {
                if (ui.failProfileVideos) throw new Error('Fixture profile unavailable');
                return {
                    profile_url: 'https://www.youtube.com/@fixture',
                    entries: ui.emptyProfileVideos
                        ? []
                        : [{ url: ui.playlistVideos[0].url, title: '<img src=x> Profile video' }],
                    next_offset: 50,
                    has_more: !ui.emptyProfileVideos && !args.offset,
                };
            };
            if (ui.holdProfileVideos) {
                ui.holdProfileVideos = false;
                return new Promise((resolve, reject) => {
                    ui.releaseProfileVideos = () => {
                        try {
                            resolve(complete());
                        } catch (error) {
                            reject(error);
                        }
                    };
                });
            }
            return complete();
        }
        if (command === 'get_history_social_insights') return structuredClone(ui.socialRecords[args.id] || null);
        if (command === 'load_history_social_insights') {
            ui.socialFetches++;
            const complete = () => {
                if (ui.failSocial) throw new Error('Fixture network request failed');
                const names = [
                    'views',
                    'likes',
                    'comments',
                    'shares',
                    'saves',
                    'followers',
                    'following',
                    'posts',
                    'subscribers',
                ];
                const metrics = Object.fromEntries(names.map(name => [name, null]));
                if (!ui.unsupportedSocial) Object.assign(metrics, { views: 12345, likes: 0, comments: 56 });
                if (Number.isSafeInteger(ui.socialScore)) metrics.score = ui.socialScore;
                const result = {
                    history_entry_id: args.id,
                    source_url: 'https://www.youtube.com/watch?v=fixture',
                    fetched_at: 1700000000000 + ui.socialFetches * 60000,
                    fetchary_version: '2.0.4',
                    data: {
                        platform: 'youtube',
                        kind: 'video',
                        url: 'https://www.youtube.com/watch?v=fixture',
                        metrics,
                        status: ui.unsupportedSocial ? 'unsupported' : 'partial',
                        sources: { views: { approximate: true } },
                        unavailable: { shares: 'unsupported' },
                    },
                };
                ui.socialRecords[args.id] = result;
                return structuredClone(result);
            };
            if (ui.holdSocial) {
                ui.holdSocial = false;
                return new Promise((resolve, reject) => {
                    ui.releaseSocial = () => {
                        try {
                            resolve(complete());
                        } catch (error) {
                            reject(error);
                        }
                    };
                });
            }
            return complete();
        }
        if (command === 'get_history_ocr') {
            const content = ocrPage(args);
            if (ui.holdOcr) {
                ui.holdOcr = false;
                return new Promise(resolve => {
                    ui.releaseOcr = () => resolve(content);
                });
            }
            return content;
        }
        if (command === 'get_history_ocr_text') {
            const content = ui.ocrRecords[args.id];
            if (JSON.stringify(content.frames || []) !== args.resultRevision) throw new Error('Results changed');
            const stamp = ms =>
                `${[Math.floor(ms / 3600000), Math.floor(ms / 60000) % 60, Math.floor(ms / 1000) % 60].map(p => String(p).padStart(2, '0')).join(':')}.${String(ms % 1000).padStart(3, '0')}`;
            return (
                (content.frames || [])
                    .map(frame => `[${stamp(frame.timestamp_ms)}]\n${frame.full_text}`)
                    .join('\n\n') ||
                content.text ||
                ''
            );
        }
        if (command === 'get_history_ocr_progress') return structuredClone(ui.ocrProgress[args.id] || null);
        if (command === 'get_history_ocr_frame') {
            if (ui.failNextFrame) {
                ui.failNextFrame = false;
                throw new Error('Fixture saved file is missing.');
            }
            const content = ui.ocrRecords[args.id];
            if (JSON.stringify(content.frames) !== args.selection.result_revision) throw new Error('Results changed');
            const frame = content.frames.find(frame => frame.id === args.selection.frame_id);
            if (!frame) throw new Error('Frame no longer available');
            const image =
                ui.frameImages[frame.timestamp_ms] ||
                globalThis.document.querySelector('#historyDetailsThumbnail').getAttribute('src') ||
                fallbackFrame;
            if (ui.holdFrame) {
                ui.holdFrame = false;
                return new Promise(resolve => {
                    ui.releaseFrame = () => resolve(image);
                });
            }
            return image;
        }
        if (command === 'delete_history_ocr_frame_text') {
            if (ui.failNextOcrDelete) {
                ui.failNextOcrDelete = false;
                throw new Error('Fixture SQLite write failed.');
            }
            const content = ui.ocrRecords[args.id];
            if (content.last_attempt_status === 'running') throw new Error('Analysis is running');
            if (JSON.stringify(content.frames) !== args.selection.result_revision) throw new Error('Results changed');
            content.frames = content.frames.filter(frame => frame.id !== args.selection.frame_id);
            return ocrPage({ id: args.id });
        }
        if (command === 'preview_history_ocr_duplicates') {
            const content = ocrPage({ id: args.id });
            if (content.last_attempt_status === 'running') throw new Error('Analysis is running');
            const preview = { duplicate_count: ui.duplicateIds.length, result_revision: content.result_revision };
            if (ui.holdDuplicatePreview) {
                ui.holdDuplicatePreview = false;
                return new Promise(resolve => {
                    ui.releaseDuplicatePreview = () => resolve(preview);
                });
            }
            return preview;
        }
        if (command === 'remove_history_ocr_duplicates') {
            const remove = () => {
                if (ui.failDuplicateRemoval) throw new Error('Fixture SQLite write failed');
                const content = ui.ocrRecords[args.id];
                if (content.last_attempt_status === 'running') throw new Error('Analysis is running');
                if (JSON.stringify(content.frames) !== args.resultRevision) throw new Error('Results changed');
                const before = content.frames.length;
                content.frames = content.frames.filter(frame => !ui.duplicateIds.includes(frame.id));
                ui.duplicateIds = [];
                return { removed_count: before - content.frames.length, content: ocrPage({ id: args.id }) };
            };
            if (ui.holdDuplicateRemoval) {
                ui.holdDuplicateRemoval = false;
                return new Promise((resolve, reject) => {
                    ui.releaseDuplicateRemoval = () => {
                        try {
                            resolve(remove());
                        } catch (error) {
                            reject(error);
                        }
                    };
                });
            }
            return remove();
        }
        if (command === 'start_history_ocr') {
            if (ui.ocrSourceError) throw new Error(ui.ocrSourceError);
            if (ui.ocrProgress[args.id]) return structuredClone(ui.ocrProgress[args.id]);
            if (Object.keys(ui.ocrProgress).length) throw new Error('An OCR analysis is already running.');
            const progress = {
                history_id: args.id,
                attempt_id: `ocr-${++ui.ocrAttempt}`,
                attempt_at: Date.now(),
                status: 'running',
                frames_checked: 0,
                total_duration_ms: null,
                total_frames: null,
                total_frames_is_estimate: false,
            };
            ui.ocrProgress[args.id] = progress;
            ui.ocrRecords[args.id] = {
                ...ui.ocrRecords[args.id],
                history_entry_id: args.id,
                last_attempt_id: progress.attempt_id,
                last_attempt_at: progress.attempt_at,
                last_attempt_status: 'running',
                last_error: null,
            };
            return structuredClone(progress);
        }
        if (command === 'cancel_history_ocr') {
            const progress = ui.ocrProgress[args.id];
            if (!progress || progress.attempt_id !== args.attemptId) throw new Error('Attempt is not running');
            ui.ocrRecords[args.id].last_attempt_status = 'cancelled';
            delete ui.ocrProgress[args.id];
            ui.emit('history:ocr', { ...progress, status: 'cancelled' });
            return;
        }
        if (command === 'get_history_transcript') {
            return {
                language: 'de',
                transcription_type: 'text',
                file_available: true,
                text: 'Transcript <img src=x onerror="alert(1)"> stays plain text.\n'.repeat(60),
            };
        }
        if (command === 'get_history_caption') {
            return {
                format: 'txt',
                file_available: false,
                caption_path: args.mediaPath === '/fixture/alternate.mp4' ? '/fixture/missing.caption.txt' : null,
                media_path: args.mediaPath,
                sha256: 'a'.repeat(64),
                created_at: 1700000000000,
                text:
                    args.mediaPath === '/fixture/alternate.mp4'
                        ? 'Alternate description.'
                        : 'Caption content.\n'.repeat(60),
            };
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
                content: `(${installScreenshotBridge.toString()})(${JSON.stringify({ videos, version })}); (${installUiResponses.toString()})(${JSON.stringify({ fallbackFrame: videos[0].thumbnail, playlistVideos: videos, keywordFixture })});`,
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
            assert(await page.locator('#settingsView').isVisible());
            const authentication = page.locator(
                '#settingsView section[aria-labelledby="settingsDownloadTitle"] #browserAuthentication'
            );
            assert(await authentication.isVisible());
            assert.equal(await page.locator('#linkDumpView #browserAuthentication').count(), 0);
            assert.equal(await page.locator('#browserAuthBrowser').inputValue(), 'firefox');
            assert((await page.locator('#browserAuthBrowser').boundingBox()).height >= 32);
            assert(await page.locator('#disconnectBrowserAuthBtn').isEnabled());
            assert.equal(
                await page.evaluate(
                    () =>
                        globalThis.__pinefetchUi.commands.filter(call => call.command === 'check_youtube_cookie_access')
                            .length
                ),
                0
            );
            // Dock requests restore startup selections and use the same views as the toolbar.
            for (const [index, [view, panel, button]] of [
                ['history', '#historyView', '#viewHistoryBtn'],
                ['linkDump', '#linkDumpView', '#viewLinkDumpBtn'],
                ['settings', '#settingsView', '#viewSettingsBtn'],
                ['download', '#downloadView', '#viewDownloadBtn'],
            ].entries()) {
                await page.evaluate(payload => globalThis.__pinefetchUi.emit('navigation:requested', payload), {
                    revision: index + 2,
                    view,
                });
                assert(await page.locator(panel).isVisible());
                assert.equal(await page.locator(button).getAttribute('aria-current'), 'page');
            }
            // A delayed startup response or an invalid native request cannot undo the latest selection.
            await page.evaluate(() => {
                const ui = globalThis.__pinefetchUi;
                ui.emit('navigation:requested', { revision: 1, view: 'settings' });
                ui.emit('navigation:requested', { revision: 6, view: 'unknown' });
            });
            assert(await page.locator('#downloadView').isVisible());
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
            await page.locator('#brandHistoryBtn').focus();
            await page.locator('.oj-tooltip').waitFor();
            assert(await page.locator('#brandHistoryBtn').getAttribute('aria-describedby'));
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
            await page.locator('#brandHistoryBtn').click();
            assert.equal(await page.locator('#viewHistoryBtn').getAttribute('aria-current'), 'page');
            const searchInput = page.locator('#historySearchInput');
            const resultCount = page.locator('#historyResultCount');
            assert.equal(await resultCount.isVisible(), false);
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
            // Showing and clearing a search count must not move the History controls or list.
            const historyLayout = () =>
                page.evaluate(() =>
                    [
                        '.pinefetch-panel-left > .oj-section-header',
                        '#leftPanelTitle',
                        '.pinefetch-history-search-controls',
                        '#historyList',
                    ].map(selector => {
                        const { x, y } = globalThis.document.querySelector(selector).getBoundingClientRect();
                        return { selector, x, y };
                    })
                );
            for (const width of [1400, 1000, 520]) {
                await page.setViewportSize({ width, height: 960 });
                await searchInput.scrollIntoViewIfNeeded();
                const beforeSearch = await historyLayout();
                await searchInput.fill('layout stability');
                await resultCount.waitFor({ state: 'visible' });
                assert.deepEqual(await historyLayout(), beforeSearch, `Search badge moved the layout at ${width}px`);
                await searchInput.fill('');
                await resultCount.waitFor({ state: 'hidden' });
                assert.deepEqual(await historyLayout(), beforeSearch, `Clearing search moved the layout at ${width}px`);
            }
            await page.setViewportSize({ width: 1400, height: 960 });
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
            const historyReadCount = () =>
                page.evaluate(
                    () => globalThis.__pinefetchUi.commands.filter(call => call.command === 'get_history').length
                );
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

            // Creator shortcuts reset pagination and scope the exact stored name to its platform.
            await page.evaluate(() => {
                const creators = Array.from({ length: 25 }, (_, index) => ({
                    id: `creator-${index}`,
                    url: `https://example.com/${index}`,
                    title: `Blender tutorial ${index}`,
                    uploader: 'Blender',
                    source: 'youtube',
                }));
                globalThis.__pinefetchUi.setHistoryEntries([
                    { ...creators[0], id: 'similar-name', uploader: 'Blender Guru' },
                    { ...creators[0], id: 'other-platform', source: 'tiktok' },
                    { ...creators[0], id: 'missing-name', uploader: null },
                    ...creators,
                ]);
                globalThis.__pinefetchUi.emit('history:changed', {});
            });
            await page.waitForFunction(
                () => globalThis.document.querySelectorAll('.pinefetch-history-item').length === 20
            );
            await page.locator('#loadMoreHistoryBtn').click();
            await page.locator('[data-history-id="creator-24"]').waitFor();
            beforeHistoryRequest = await historyCommandCount();
            const lastCreator = page.locator('[data-history-id="creator-24"] .pinefetch-history-open-btn');
            await lastCreator.scrollIntoViewIfNeeded();
            await page.evaluate(
                () =>
                    new Promise(resolve =>
                        globalThis.requestAnimationFrame(() => globalThis.requestAnimationFrame(resolve))
                    )
            );
            await lastCreator.click({ button: 'right' });
            const creatorAction = page.getByRole('menuitem', {
                name: 'Show saved videos by this creator',
                exact: true,
            });
            assert(await creatorAction.isEnabled());
            await creatorAction.click();
            await waitForHistoryRequest(beforeHistoryRequest, 'user', 'youtube', 'Blender');
            await page.waitForFunction(() => {
                const entries = [...globalThis.document.querySelectorAll('.pinefetch-history-item')];
                return entries.length === 20 && entries.every(entry => entry.dataset.historyId.startsWith('creator-'));
            });
            assert.equal(await searchInput.inputValue(), 'Blender');
            assert.equal(await searchInput.getAttribute('placeholder'), 'Search users');
            assert.equal(await page.locator('#historySearchFieldValue').textContent(), 'User');
            assert.equal(await page.locator('#historySourceValue').textContent(), 'YouTube');
            assert.equal(await page.locator('#historyContextMenu').isVisible(), false);
            assert(await searchInput.evaluate(node => node === globalThis.document.activeElement));
            const latestHistoryArgs = () =>
                page.evaluate(
                    () => globalThis.__pinefetchUi.commands.findLast(call => call.command === 'get_history').args
                );
            assert.equal((await latestHistoryArgs()).offset, 0);
            assert.equal((await latestHistoryArgs()).exactUser, true);

            assert.equal(await resultCount.textContent(), '25 found');
            assert(await resultCount.isVisible());
            await page.setViewportSize({ width: 1000, height: 960 });
            const titleBounds = await page.locator('#leftPanelTitle').boundingBox();
            const countBounds = await resultCount.boundingBox();
            assert(countBounds.x > titleBounds.x + titleBounds.width);
            assert(Math.abs(countBounds.y + countBounds.height / 2 - titleBounds.y - titleBounds.height / 2) < 6);
            assert(await page.evaluate(() => globalThis.document.documentElement.scrollWidth <= globalThis.innerWidth));
            await page.setViewportSize({ width: 1400, height: 960 });
            await page.locator('#loadMoreHistoryBtn').click();
            await page.locator('[data-history-id="creator-24"]').waitFor();
            assert.equal(await page.locator('.pinefetch-history-item').count(), 25);
            assert.equal((await latestHistoryArgs()).offset, 20);
            assert.equal(await resultCount.textContent(), '25 found');
            assert.equal((await latestHistoryArgs()).exactUser, true);

            // Detail navigation preserves the exact query, all loaded rows and
            // the actual list nodes/scroll position without a History reload.
            await page.locator('#historyList').evaluate(list => {
                list.scrollTop = list.scrollHeight;
            });
            const retainedScroll = await page.locator('#historyList').evaluate(list => list.scrollTop);
            assert(retainedScroll > 0);
            const retainedRequests = await historyReadCount();
            await page.evaluate(() => {
                globalThis.__retainedHistoryRow = globalThis.document.querySelector('.pinefetch-history-item');
            });
            await page.locator('[data-history-id="creator-24"] .pinefetch-history-open-btn').click();
            await page.locator('#historyDetailsContent').waitFor();
            assert.equal(await page.locator('#historyDetailsTitle').textContent(), 'Blender tutorial 24');
            assert.equal(await page.locator('#historyDetailsView').evaluate(element => element.tagName), 'SECTION');
            assert.equal(await page.locator('dialog[open]').count(), 0);
            assert(await page.locator('#viewDownloadBtn').isVisible());
            const fullContainer = await page.locator('#historyDetailsView').boundingBox();
            assert(fullContainer.width >= 900);
            assert.equal(await page.locator('#historyDetailsBreadcrumbAccount').textContent(), 'Blender');
            assert.equal(await page.locator('#historyDetailsBreadcrumbCurrent').textContent(), 'Blender tutorial 24');
            assert.equal(await page.locator('#historyDetailsBreadcrumbCurrent').getAttribute('aria-current'), 'page');
            assert.equal(await page.locator('#historyDetailsBreadcrumbCurrent').evaluate(el => el.tagName), 'SPAN');
            await page.locator('#historyDetailsAccountBreadcrumbBtn').focus();
            await page.keyboard.press('Enter');
            await page.waitForFunction(
                () => globalThis.document.querySelector('#historyAccountBreadcrumbCurrent').textContent === 'Blender'
            );
            assert.equal(await page.locator('#historyAccountBreadcrumbCurrent').getAttribute('aria-current'), 'page');
            await page.locator('#historyAccountHistoryBtn').focus();
            await page.keyboard.press('Enter');
            await page.locator('#historyView').waitFor();
            assert.equal(await searchInput.inputValue(), 'Blender');
            assert.equal(await page.locator('#historySourceValue').textContent(), 'YouTube');
            assert.equal(await page.locator('#historySearchFieldValue').textContent(), 'User');
            assert.equal(await page.locator('.pinefetch-history-item').count(), 25);
            assert.equal(await historyReadCount(), retainedRequests);
            assert.equal(await page.locator('#historyList').evaluate(list => list.scrollTop), retainedScroll);
            assert(
                await page.evaluate(
                    () =>
                        globalThis.__retainedHistoryRow === globalThis.document.querySelector('.pinefetch-history-item')
                )
            );

            // A refresh and a view switch keep the shortcut; manual edits restore partial matching.
            beforeHistoryRequest = await historyCommandCount();
            await page.evaluate(() => globalThis.__pinefetchUi.emit('history:changed', {}));
            await waitForHistoryRequest(beforeHistoryRequest, 'user', 'youtube', 'Blender');
            assert.equal((await latestHistoryArgs()).exactUser, true);
            await page.locator('#viewDownloadBtn').click();
            assert.equal(await resultCount.isVisible(), false);
            await page.locator('#viewHistoryBtn').click();
            await resultCount.waitFor({ state: 'visible' });
            assert.equal(await resultCount.textContent(), '25 found');
            assert.equal(await searchInput.inputValue(), 'Blender');
            assert.equal(await page.locator('#historySourceValue').textContent(), 'YouTube');
            beforeHistoryRequest = await historyCommandCount();
            await searchInput.fill('Blend');
            await waitForHistoryRequest(beforeHistoryRequest, 'user', 'youtube', 'Blend');
            await page.locator('[data-history-id="similar-name"]').waitFor();
            assert.equal(await resultCount.textContent(), '26 found');
            assert.equal(Object.hasOwn(await latestHistoryArgs(), 'exactUser'), false);
            assert.equal((await latestHistoryArgs()).offset, 0);

            beforeHistoryRequest = await historyCommandCount();
            await searchInput.fill('No creator matches');
            await waitForHistoryRequest(beforeHistoryRequest, 'user', 'youtube', 'No creator matches');
            await page.waitForFunction(
                () => globalThis.document.getElementById('historyResultCount').textContent === '0 found'
            );
            assert(await resultCount.isVisible());

            // No creator metadata leaves the action disabled, including keyboard navigation.
            beforeHistoryRequest = await historyCommandCount();
            await searchInput.fill('');
            await waitForHistoryRequest(beforeHistoryRequest, 'user', 'youtube');
            assert.equal(await resultCount.isVisible(), false);
            await page.locator('[data-history-id="missing-name"] .pinefetch-history-open-btn').focus();
            await page.evaluate(
                () =>
                    new Promise(resolve =>
                        globalThis.requestAnimationFrame(() => globalThis.requestAnimationFrame(resolve))
                    )
            );
            await page.keyboard.press('Shift+F10');
            await page.locator('#historyContextMenu').waitFor();
            assert(await creatorAction.isDisabled());
            await page.waitForFunction(
                () => globalThis.document.getElementById('historyShowMoreDataBtn') === globalThis.document.activeElement
            );
            await page.keyboard.press('End');
            await page.waitForFunction(
                () => globalThis.document.getElementById('historyOpenOcrBtn') === globalThis.document.activeElement
            );
            await page.keyboard.press('Escape');
            await page.locator('#historyContextMenu').waitFor({ state: 'hidden' });

            await page.locator('[data-history-id="missing-name"] .pinefetch-history-open-btn').click();
            await page.locator('#historyDetailsContent').waitFor();
            assert(await page.locator('#historyDetailsAccountCrumb').isHidden());
            assert.equal(await page.locator('#historyDetailsView .oj-breadcrumb li:visible').count(), 2);
            await page.locator('#historyDetailsHistoryBtn').click();
            await page.locator('#historyView').waitFor();

            // Restore the standard history fixtures for the detail and deletion checks.
            await page.evaluate(() => {
                globalThis.__pinefetchUi.setHistoryEntries(null);
                globalThis.__pinefetchUi.emit('history:changed', {});
            });
            beforeHistoryRequest = await historyCommandCount();
            await selectHistoryFilter('historySource', '', 'All sources');
            await waitForHistoryRequest(beforeHistoryRequest, 'user');
            beforeHistoryRequest = await historyCommandCount();
            await selectHistoryFilter('historySearchField', 'title', 'Title');
            await waitForHistoryRequest(beforeHistoryRequest, 'title');
            await page.locator('[data-history-id="history-2"]').waitFor();

            // Library tabs own roving focus, activation and lazy text fetching.
            await page.locator('.pinefetch-history-open-btn').first().focus();
            await page.keyboard.press('Shift+F10');
            await page.locator('#historyShowMoreDataBtn').click();
            await page.locator('#historyDetailsContent').waitFor();
            await page.locator('#historyDetailsOpenFileBtn').click();
            await page.locator('#historyDetailsOpenBrowserBtn').click();
            assert(
                await page.evaluate(() =>
                    globalThis.__pinefetchUi.commands.some(call => call.command === 'open_file_path')
                )
            );
            // File data stays available in a closed disclosure rather than the reading flow.
            assert(await page.locator('#historyOverviewDetails').isVisible());
            assert.equal(await page.locator('#historyOverviewDetails').getAttribute('open'), null);
            assert.doesNotMatch(await page.locator('#historyOverviewList').textContent(), /Filename|SHA-256/);
            await page.locator('#historyOverviewDetails summary').click();
            assert(await page.locator('#historyOverviewMeta').isVisible());
            assert.match(await page.locator('#historyOverviewMeta').textContent(), /Filename/);
            await page.locator('#historyOverviewDetails summary').click();
            await page.locator('#historyOverviewTab').focus();
            await page.keyboard.press('ArrowRight');
            await page.locator('#historyTranscriptText').waitFor();
            assert.equal(await page.locator('#historyTranscriptTab').getAttribute('aria-selected'), 'true');
            assert.equal(await page.locator('#historyTranscriptText img').count(), 0);
            await page.locator('#historyTranscriptCopyBtn').click();
            await page.locator('.oj-toast').first().waitFor();
            assert.match(await page.evaluate(() => globalThis.__pinefetchUi.copied), /^Transcript <img/);
            await page.locator('#historyCaptionText').waitFor();
            assert.equal(await page.locator('#historyTranscriptTitle').textContent(), 'AI transcript');
            assert.equal(await page.locator('#historyCaptionTitle').textContent(), 'Video description');
            assert.equal(await page.locator('#historyTranscriptPanel details').count(), 0);
            assert(await page.locator('#historyCaptionNotice').isHidden());
            assert.doesNotMatch(await page.locator('#historyCaptionTrackSelect').textContent(), /\.mp4|\.txt/);
            await page.locator('#historyCaptionCopyBtn').click();
            assert.equal(await page.evaluate(() => globalThis.__pinefetchUi.copied), 'Caption content.\n'.repeat(60));
            await page.locator('#historyCaptionTrackSelect').selectOption('/fixture/alternate.mp4');
            await page.waitForFunction(
                () => globalThis.document.getElementById('historyCaptionText').textContent === 'Alternate description.'
            );
            assert(await page.locator('#historyCaptionNotice').isVisible());
            await page.locator('#historyCaptionCopyBtn').click();
            assert.equal(await page.evaluate(() => globalThis.__pinefetchUi.copied), 'Alternate description.');
            await page.locator('#historyCaptionTrackSelect').selectOption({ index: 0 });
            await page.waitForFunction(() =>
                globalThis.document.getElementById('historyCaptionText').textContent.startsWith('Caption content.')
            );
            await page.evaluate(() => {
                for (const button of globalThis.document.querySelectorAll('.oj-toast button')) button.click();
            });
            await page.locator('.oj-toast').first().waitFor({ state: 'hidden' });
            for (const viewport of [
                { width: 1400, height: 960 },
                { width: 1000, height: 720 },
                { width: 600, height: 720 },
            ]) {
                await page.setViewportSize(viewport);
                for (const prefix of ['historyTranscript', 'historyCaption']) {
                    const section = page.locator(`#${prefix}Text`).locator('..');
                    await section.locator('.pinefetch-history-section-header').scrollIntoViewIfNeeded();
                    const layout = await section.evaluate(element => {
                        const header = element.querySelector('header').getBoundingClientRect();
                        const title = element.querySelector('h3').getBoundingClientRect();
                        const button = element.querySelector('header button').getBoundingClientRect();
                        const text = element.querySelector('pre');
                        const helper = element.querySelector('.oj-helper').getBoundingClientRect();
                        let previous = text.previousElementSibling;
                        while (previous && !previous.getBoundingClientRect().height)
                            previous = previous.previousElementSibling;
                        return {
                            header,
                            title,
                            button,
                            text: text.getBoundingClientRect(),
                            helper,
                            previous: previous.getBoundingClientRect(),
                            scrollHeight: text.scrollHeight,
                            clientHeight: text.clientHeight,
                            overflow: globalThis.getComputedStyle(text).overflowY,
                            pageWidth: globalThis.document.documentElement.scrollWidth,
                        };
                    });
                    assert(
                        Math.abs(
                            layout.title.y + layout.title.height / 2 - layout.button.y - layout.button.height / 2
                        ) <= 2,
                        JSON.stringify(layout)
                    );
                    assert(layout.button.x >= layout.title.right, JSON.stringify(layout));
                    assert(layout.helper.y - layout.header.bottom <= 9, JSON.stringify(layout));
                    assert(layout.text.y - layout.previous.bottom <= 9, JSON.stringify(layout));
                    assert.equal(layout.overflow, 'auto');
                    assert(layout.scrollHeight > layout.clientHeight);
                    assert(layout.text.height <= viewport.height * 0.36 + 1, JSON.stringify(layout));
                    assert(layout.pageWidth <= viewport.width);
                    await section.locator('pre').evaluate(element => {
                        element.scrollTop = element.scrollHeight;
                    });
                    assert(await section.locator('pre').evaluate(element => element.scrollTop > 0));
                    await section.locator('pre').evaluate(element => {
                        element.scrollTop = 0;
                    });
                }
                // Return to the beginning of each section for a useful visual review.
                await page.locator('#historyTranscriptPanel').evaluate(element => {
                    element.scrollTop = 0;
                    globalThis.document.scrollingElement.scrollTop = 0;
                });
                await page.screenshot({ path: `/tmp/pinefetch-history-text-${viewport.width}-${engine.name()}.png` });
            }
            await page.setViewportSize({ width: 1400, height: 960 });
            await page.locator('#historyTranscriptTab').focus();
            await page.keyboard.press('ArrowRight');
            assert.equal(await page.locator('#historySubtitlesTab').getAttribute('aria-selected'), 'true');
            await page.locator('#historySubtitleText').waitFor();
            assert((await page.locator('#historySubtitleTrackSelect').boundingBox()).height >= 32);
            assert.equal(await page.locator('#historySubtitlesPanel details').count(), 0);
            assert(await page.locator('#historySubtitleMeta').isVisible());
            assert.match(await page.locator('#historySubtitleMeta').textContent(), /video.en.vtt/);
            const subtitleHeader = await page.locator('#historySubtitleTitle').boundingBox();
            const subtitleCopy = await page.locator('#historySubtitleCopyBtn').boundingBox();
            assert(
                Math.abs(subtitleHeader.y + subtitleHeader.height / 2 - subtitleCopy.y - subtitleCopy.height / 2) <= 2
            );

            assert.match(await page.locator('#historySubtitleText').textContent(), /Creator <img/);
            assert.equal(await page.locator('#historySubtitleText img').count(), 0);
            assert.match(await page.locator('#historySubtitleNotice').textContent(), /file is missing/);
            await page.locator('#historySubtitleCopyBtn').click();
            await page.waitForFunction(() => globalThis.__pinefetchUi.copied.startsWith('WEBVTT'));
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failNextSubtitle = true;
            });
            await page.locator('#historySubtitleTrackSelect').selectOption('/fixture/video.de.srt');
            await page.waitForFunction(() =>
                globalThis.document.getElementById('historySubtitleStatus').textContent.includes('could not be loaded')
            );
            assert.doesNotMatch(await page.locator('#historySubtitleStatus').textContent(), /Synthetic private/);
            await page.evaluate(() => {
                globalThis.__pinefetchUi.holdSubtitle = true;
            });
            await page.locator('#historySubtitleRetryBtn').click();
            await page.waitForFunction(() => typeof globalThis.__pinefetchUi.releaseSubtitle === 'function');
            await page.locator('#historySubtitleTrackSelect').selectOption('/fixture/video.en.vtt');
            await page.locator('#historySubtitleText').waitFor();
            await page.evaluate(async () => {
                globalThis.__pinefetchUi.releaseSubtitle();
                await new Promise(resolve => setTimeout(resolve, 0));
            });
            assert.match(await page.locator('#historySubtitleText').textContent(), /Creator <img/);
            await page.locator('#historySubtitleTrackSelect').selectOption('/fixture/video.de.srt');
            await page.locator('#historySubtitleText').waitFor();
            assert.match(await page.locator('#historySubtitleText').textContent(), /Grüße/);
            assert.match(await page.locator('#historySubtitleNotice').textContent(), /starts at 61s/);
            const subtitleHeights = [];
            for (const viewport of [
                { width: 1400, height: 960 },
                { width: 1400, height: 1200 },
                { width: 1000, height: 720 },
            ]) {
                await page.setViewportSize(viewport);
                const layout = await page.locator('#historySubtitlesPanel').evaluate(panel => {
                    const text = globalThis.document.getElementById('historySubtitleText').getBoundingClientRect();
                    const meta = globalThis.document.getElementById('historySubtitleMeta').getBoundingClientRect();
                    return { text, meta, panel: panel.getBoundingClientRect() };
                });
                assert(layout.meta.y >= layout.text.bottom, JSON.stringify(layout));
                assert(layout.meta.y - layout.text.bottom <= 9, JSON.stringify(layout));
                assert(layout.meta.bottom <= layout.panel.bottom + 1, JSON.stringify(layout));
                subtitleHeights.push(layout.text.height);
                await page.screenshot({
                    path: `/tmp/pinefetch-subtitles-${viewport.width}x${viewport.height}-${engine.name()}.png`,
                });
            }
            assert(subtitleHeights[1] > subtitleHeights[0] + 150, JSON.stringify(subtitleHeights));
            assert(subtitleHeights[2] < subtitleHeights[0], JSON.stringify(subtitleHeights));
            await page.setViewportSize({ width: 1400, height: 960 });
            await page.screenshot({ path: `/tmp/pinefetch-subtitles-${engine.name()}.png` });
            await page.locator('#historySubtitlesTab').focus();
            await page.keyboard.press('ArrowRight');
            assert.equal(await page.locator('#historyOcrTab').getAttribute('aria-selected'), 'true');
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
            await page.locator('#historyDetailsHistoryBtn').click();
            await page.locator('#historyDetailsView').waitFor({ state: 'hidden' });
            await page.waitForFunction(
                () =>
                    globalThis.document.querySelector('.pinefetch-history-open-btn') ===
                    globalThis.document.activeElement
            );
            // The same saved text layout uses post terminology outside video-description sources.
            await page.evaluate(() => {
                globalThis.__pinefetchUi.detailSource = 'tiktok';
                globalThis.__pinefetchUi.detailCaptionCount = 1;
            });
            await page.locator('.pinefetch-history-open-btn').first().focus();
            await page.keyboard.press('Shift+F10');
            await page.locator('#historyShowMoreDataBtn').click();
            await page.locator('#historyDetailsContent').waitFor();
            await page.locator('#historyTranscriptTab').click();
            await page.locator('#historyCaptionText').waitFor();
            assert.equal(await page.locator('#historyCaptionTitle').textContent(), 'Post caption');
            assert.equal(await page.locator('#historyCaptionCopyBtn').getAttribute('aria-label'), 'Copy post caption');
            assert(await page.locator('#historyCaptionTrackField').isHidden());
            await page.locator('#historyCaptionCopyBtn').click();
            assert.equal(await page.evaluate(() => globalThis.__pinefetchUi.copied), 'Caption content.\n'.repeat(60));
            await page.locator('#historyDetailsHistoryBtn').click();
            await page.evaluate(() => {
                delete globalThis.__pinefetchUi.detailSource;
                delete globalThis.__pinefetchUi.detailCaptionCount;
            });
            await page.locator('.pinefetch-history-open-btn').nth(1).focus();
            await page.keyboard.press('Shift+F10');
            await page.locator('#historyShowMoreDataBtn').click();
            await page.locator('#historyDetailsContent').waitFor();
            assert.equal(await page.locator('#historyOverviewTab').getAttribute('aria-selected'), 'true');
            assert.equal(await page.locator('#historyTranscriptTab').isDisabled(), false);
            assert(await page.locator('#historyTranscriptTab').isVisible());
            await page.locator('#historyTranscriptTab').click();
            assert.equal(
                await page.locator('#historyTranscriptStatus').textContent(),
                'No transcript is saved for this video.'
            );
            assert(await page.locator('#historyCaptionsPanel').isHidden());
            await page.locator('#historySubtitlesTab').click();
            assert.match(await page.locator('#historySubtitleStatus').textContent(), /No creator subtitles/);
            assert(await page.locator('#historySubtitleCopyBtn').isDisabled());
            assert(await page.locator('#historySubtitleTrackField').isHidden());
            await page.locator('#historyDetailsHistoryBtn').click();
            await page.locator('#historyDetailsView').waitFor({ state: 'hidden' });

            // OCR is lazy, safe text, explicitly cancellable, and entry/request scoped.
            const waitOcrPage = async () => {
                await page.waitForFunction(
                    () =>
                        globalThis.document.querySelector('#historyOcrListViewport').getAttribute('aria-busy') !==
                            'true' &&
                        globalThis.document.querySelector('#historyOcrResultCount').textContent !== 'Loading frames…'
                );
            };
            const searchOcr = async query => {
                await page.locator('#historyOcrSearchInput').fill(query);
                await waitOcrPage();
            };
            const confirmFrameDelete = async () => {
                const dialog = page.getByRole('dialog', { name: 'Delete frame text?' });
                await dialog.waitFor();
                await dialog.getByRole('button', { name: 'Delete frame text', exact: true }).click();
            };
            const openOcr = async (id, start = false) => {
                // Allow focus and layout to settle before opening the keyboard menu.
                await page.evaluate(
                    () =>
                        new Promise(resolve =>
                            globalThis.requestAnimationFrame(() => globalThis.requestAnimationFrame(resolve))
                        )
                );
                await page.locator(`[data-history-id="${id}"] .pinefetch-history-open-btn`).focus();
                await page.evaluate(
                    () =>
                        new Promise(resolve =>
                            globalThis.requestAnimationFrame(() => globalThis.requestAnimationFrame(resolve))
                        )
                );
                await page.keyboard.press('Shift+F10');
                assert.equal(
                    await page
                        .locator(`[data-history-id="${id}"] .pinefetch-history-open-btn`)
                        .getAttribute('aria-expanded'),
                    'true'
                );
                await page.locator('#historyOpenOcrBtn').click();
                await page.locator('#historyDetailsContent').waitFor();
                if (start) {
                    await page.locator('#historyOcrStartBtn').waitFor();
                    await page.locator('#historyOcrStartBtn').click();
                }
                await page.waitForFunction(
                    () => globalThis.document.getElementById('historyOcrTab').getAttribute('aria-selected') === 'true'
                );
            };
            const closeDetails = async () => {
                await page.locator('#historyDetailsHistoryBtn').click();
                await page.locator('#historyDetailsView').waitFor({ state: 'hidden' });
            };
            const expectOcrStatus = status =>
                page.waitForFunction(
                    expected => globalThis.document.getElementById('historyOcrStatus').textContent === expected,
                    status
                );
            const ocrProgressText = async () =>
                (await page.locator('#historyOcrProgressText').textContent()).replace(/\s+/g, ' ').trim();
            const ocrCounterLayout = () =>
                page.evaluate(() =>
                    [
                        'historyOcrTime',
                        'historyOcrTimeTotal',
                        'historyOcrFrames',
                        'historyOcrFramesTotal',
                        'historyOcrProgressText',
                        'historyOcrCancelBtn',
                        'historyDetailsThumbnail',
                    ].map(id => {
                        const { x, y, width, height } = globalThis.document.getElementById(id).getBoundingClientRect();
                        return { id, x, y, width, height };
                    })
                );
            const pinnedOcrLayout = () =>
                page.evaluate(() =>
                    ['historyOcrPreview', 'historyOcrSearchInput', 'historyOcrMeta'].map(id => {
                        const { x, y, width, height } = globalThis.document.getElementById(id).getBoundingClientRect();
                        return { id, x, y, width, height };
                    })
                );
            const checkOcrLayout = async state => {
                while (await page.locator('.oj-toast-dismiss').count())
                    await page.locator('.oj-toast-dismiss').first().click();
                for (const viewport of [
                    { width: 1000, height: 960 },
                    { width: 1600, height: 1100 },
                    { width: 1000, height: 720 },
                ]) {
                    await page.setViewportSize(viewport);
                    const layout = await page.evaluate(() => {
                        const panel = globalThis.document.getElementById('historyOcrPanel');
                        const rect = selector => panel.querySelector(selector).getBoundingClientRect().toJSON();
                        const right = panel.querySelector('#historyOcrResults');
                        const footer = panel.querySelector('.pinefetch-ocr-analysis-info');
                        return {
                            panel: panel.getBoundingClientRect().toJSON(),
                            actions: rect('.pinefetch-ocr-actions'),
                            start: rect('#historyOcrStartBtn'),
                            cancel: rect('#historyOcrCancelBtn'),
                            copy: rect('#historyOcrCopyBtn'),
                            activeButton: panel.querySelector('#historyOcrStartBtn').hidden ? 'cancel' : 'start',
                            workspace: rect('.pinefetch-ocr-workspace'),
                            preview: rect('#historyOcrPreview'),
                            frame: rect('.pinefetch-ocr-frame-stage'),
                            time: rect('#historyOcrFrameTime'),
                            right: rect('#historyOcrResults'),
                            heading: rect('#historyOcrResultsTitle'),
                            search: rect('#historyOcrSearchInput'),
                            list: rect('#historyOcrListViewport'),
                            footer: rect('.pinefetch-ocr-analysis-info'),
                            rightSide: ['historyOcrResultsTitle', 'historyOcrSearchInput', 'historyOcrText'].every(id =>
                                right.contains(globalThis.document.getElementById(id))
                            ),
                            footerInfo: [
                                'historyOcrMeta',
                                'historyOcrNotice',
                                'historyOcrActionNotice',
                                'historyOcrPreviousResult',
                                'historyOcrAnalysis',
                            ].every(id => footer.contains(globalThis.document.getElementById(id))),
                            overflow: panel.scrollWidth > panel.clientWidth || panel.scrollHeight > panel.clientHeight,
                            scrollContainers: [...panel.querySelectorAll('*')]
                                .filter(el => /auto|scroll/.test(globalThis.getComputedStyle(el).overflowY))
                                .map(el => el.id),
                        };
                    });
                    assert(layout.rightSide && layout.footerInfo);
                    assert.equal(layout.overflow, false);
                    assert(layout.panel.bottom <= viewport.height, JSON.stringify(layout));
                    assert.deepEqual(layout.scrollContainers, ['historyOcrListViewport']);
                    assert(layout.workspace.top >= layout.actions.bottom);
                    assert(layout.right.left >= layout.preview.right);
                    assert(Math.abs(layout.right.top - layout.preview.top) < 1);
                    assert(Math.abs(layout.preview.width / (layout.preview.width + layout.right.width) - 0.55) < 0.01);
                    assert(layout.frame.height > 150);
                    assert(layout.time.top >= layout.frame.bottom);
                    assert(layout.time.bottom <= layout.panel.bottom);
                    assert(layout.search.top >= layout.heading.bottom);
                    assert(layout.list.top >= layout.search.bottom);
                    assert(layout.list.height > 60, JSON.stringify(layout));
                    assert(layout.footer.top >= layout.list.bottom);
                    assert(layout.footer.bottom <= layout.panel.bottom);
                    assert(Math.abs(layout.copy.right - layout.actions.right) < 1);
                    assert(layout.copy.height >= 36);
                    assert.equal(layout[layout.activeButton].height, layout.copy.height);
                    assert(layout[layout.activeButton].right < layout.copy.left);
                    assert(
                        await page
                            .locator('#historyOcrCopyBtn')
                            .evaluate(el => el.classList.contains('oj-button-primary'))
                    );
                    assert(
                        await page
                            .locator('#historyOcrStartBtn')
                            .evaluate(el => el.classList.contains('oj-button-secondary'))
                    );
                    assert.equal(await page.locator('#historyOcrCopyBtn').count(), 1);
                    assert.equal(await page.locator('#historyOcrStartBtn').count(), 1);
                    assert.equal(await page.locator('#historyOcrText').count(), 1);
                    assert.equal(
                        await page
                            .locator('#historyOcrSelectedText, #historyOcrEntryCount, #historyOcrPreviewTitle')
                            .count(),
                        0
                    );
                    assert.doesNotMatch(
                        await page.locator('#historyOcrPanel').textContent(),
                        /Extract text appearing in video frames using Apple Vision|Times are observations from sampled frames|Analysis runs in the background/
                    );
                    await page.screenshot({
                        path: `/tmp/pinefetch-ocr-review-${state}-${viewport.width}x${viewport.height}-${engine.name()}.png`,
                    });
                }
                await page.setViewportSize({ width: 1400, height: 960 });
            };
            await openOcr('history-1');
            await expectOcrStatus('Not checked yet');
            assert.equal(
                await page.evaluate(
                    () => globalThis.__pinefetchUi.commands.filter(call => call.command === 'start_history_ocr').length
                ),
                0
            );
            await closeDetails();
            await openOcr('history-1', true);
            await expectOcrStatus('Checking for OCR…');
            assert(await page.locator('#historyOcrDuplicatesBtn').isDisabled());
            assert.deepEqual(await page.evaluate(() => Object.keys(globalThis.__pinefetchUi.ocrProgress)), [
                'history-1',
            ]);
            assert.equal(await page.locator('#historyOcrProgress').getAttribute('value'), null);
            assert(await page.locator('#historyOcrPreparing').isVisible());
            assert.equal(await page.locator('#historyOcrProgressText').isVisible(), false);
            assert(await page.locator('#historyDetailsThumbnail').isVisible());
            assert(await page.locator('#historyDetailsThumbnail').getAttribute('src'));
            await page.evaluate(() => {
                const progress = globalThis.__pinefetchUi.ocrProgress['history-1'];
                progress.frames_checked = 3;
                progress.observed_at_ms = 3020;
                progress.total_duration_ms = 60000;
                progress.total_frames = 60;
                progress.total_frames_is_estimate = true;
                globalThis.__pinefetchUi.emit('history:ocr', progress);
            });
            assert.equal(await ocrProgressText(), 'Video time 00:00:03 / 00:01:00 · Frames 3 / 60');
            assert(
                Math.abs(Number(await page.locator('#historyOcrProgress').getAttribute('value')) - 3020 / 60000) < 1e-9
            );
            await page.setViewportSize({ width: 1000, height: 960 });
            await page.evaluate(() => {
                const progress = globalThis.__pinefetchUi.ocrProgress['history-1'];
                Object.assign(progress, {
                    frames_checked: 741,
                    observed_at_ms: 740000,
                    total_duration_ms: 1305621,
                    total_frames: 1306,
                });
                globalThis.__pinefetchUi.emit('history:ocr', progress);
            });
            assert.equal(await ocrProgressText(), 'Video time 00:12:20 / 00:21:45 · Frames 741 / 1306');
            assert(await page.locator('#historyDetailsThumbnail').isVisible());
            assert(await page.locator('#historyOcrPanel').evaluate(el => el.scrollWidth <= el.clientWidth));
            await page.evaluate(() => globalThis.document.fonts.ready);
            const counterLayout = await ocrCounterLayout();
            for (const frames of [9, 10, 99, 100, 999, 1000]) {
                await page.evaluate(framesChecked => {
                    const progress = globalThis.__pinefetchUi.ocrProgress['history-1'];
                    Object.assign(progress, { frames_checked: framesChecked, observed_at_ms: framesChecked * 1000 });
                    globalThis.__pinefetchUi.emit('history:ocr', progress);
                }, frames);
                assert.deepEqual(await ocrCounterLayout(), counterLayout, `Counters shifted at ${frames} frames`);
                assert.equal(
                    await ocrProgressText(),
                    `Video time ${[Math.floor(frames / 3600), Math.floor(frames / 60) % 60, frames % 60].map(part => String(part).padStart(2, '0')).join(':')} / 00:21:45 · Frames ${frames} / 1306`
                );
            }
            // Final extraction can reduce the total's digit count without moving
            // separators, controls or the thumbnail.
            await page.evaluate(() => {
                const progress = globalThis.__pinefetchUi.ocrProgress['history-1'];
                Object.assign(progress, { frames_checked: 999, total_frames: 999, total_frames_is_estimate: false });
                globalThis.__pinefetchUi.emit('history:ocr', progress);
            });
            await expectOcrStatus('Finishing OCR…');
            assert.deepEqual(await ocrCounterLayout(), counterLayout);
            await page.evaluate(() => {
                const progress = globalThis.__pinefetchUi.ocrProgress['history-1'];
                Object.assign(progress, {
                    frames_checked: 741,
                    observed_at_ms: 740000,
                    total_frames: 1306,
                    total_frames_is_estimate: true,
                });
                globalThis.__pinefetchUi.emit('history:ocr', progress);
            });
            await page.screenshot({ path: `/tmp/pinefetch-ocr-progress-${engine.name()}.png` });
            await checkOcrLayout('running');
            await page.setViewportSize({ width: 1400, height: 960 });
            await closeDetails();
            assert.equal(
                await page.evaluate(
                    () => globalThis.__pinefetchUi.commands.filter(call => call.command === 'cancel_history_ocr').length
                ),
                0
            );
            const backgroundRequests = await historyReadCount();
            assert.match(
                await page.locator('[data-history-id="history-1"] .pinefetch-history-ocr-indicator').textContent(),
                /processing/
            );
            await page.locator('#viewDownloadBtn').click();
            await page.evaluate(() => {
                const progress = globalThis.__pinefetchUi.ocrProgress['history-1'];
                Object.assign(progress, { frames_checked: 800, observed_at_ms: 800000 });
                globalThis.__pinefetchUi.emit('history:ocr', progress);
            });
            await page.locator('#viewHistoryBtn').click();
            await page.locator('#historyView').waitFor();
            assert.equal(await historyReadCount(), backgroundRequests);
            await page.locator('[data-history-id="history-2"] .pinefetch-history-open-btn').click();
            await page.locator('#historyDetailsContent').waitFor();
            assert.equal(await page.locator('#historyOverviewTab').getAttribute('aria-selected'), 'true');
            await page.locator('#historyOcrTab').click();
            await expectOcrStatus('Not checked yet');
            await page.locator('#historyOcrStartBtn').click();
            assert.match(await page.locator('#historyOcrNotice').textContent(), /Another OCR analysis is active/);
            assert.equal(
                await page.evaluate(
                    () => globalThis.__pinefetchUi.commands.filter(call => call.command === 'start_history_ocr').length
                ),
                1
            );
            assert.equal(
                await page.evaluate(() => globalThis.__pinefetchUi.ocrProgress['history-1'].frames_checked),
                800
            );
            await closeDetails();
            await openOcr('history-1');
            await expectOcrStatus('Checking for OCR…');
            assert.equal(await ocrProgressText(), 'Video time 00:13:20 / 00:21:45 · Frames 800 / 1306');
            await page.locator('#historyOverviewTab').click();
            await page.locator('#historyTranscriptTab').click();
            await page.locator('#historyOcrTab').click();
            await expectOcrStatus('Checking for OCR…');
            assert.equal(
                await page.evaluate(
                    () => globalThis.__pinefetchUi.commands.filter(call => call.command === 'start_history_ocr').length
                ),
                1
            );
            await page.locator('#historyOcrCancelBtn').click();
            await expectOcrStatus('OCR cancelled');
            await closeDetails();
            await openOcr('history-1', true);
            await page.evaluate(() => {
                const progress = globalThis.__pinefetchUi.ocrProgress['history-1'];
                Object.assign(progress, {
                    frames_checked: 4,
                    observed_at_ms: 3520,
                    total_duration_ms: 3560,
                    total_frames: 4,
                    total_frames_is_estimate: false,
                });
                globalThis.__pinefetchUi.emit('history:ocr', progress);
            });
            await expectOcrStatus('Finishing OCR…');
            assert.equal(await ocrProgressText(), 'Video time 00:00:03 / 00:00:03 · Frames 4 / 4');
            assert(Number(await page.locator('#historyOcrProgress').getAttribute('value')) < 1);
            await closeDetails();
            await openOcr('history-2');
            await expectOcrStatus('Not checked yet');
            await page.evaluate(() => {
                const ui = globalThis.__pinefetchUi;
                const progress = ui.ocrProgress['history-1'];
                ui.ocrRecords['history-1'] = {
                    ...ui.ocrRecords['history-1'],
                    last_attempt_status: 'completed',
                    result_created_at: 1700000000000,
                    text: null,
                    frames: [
                        {
                            id: 'frame-first',
                            full_text: '<img src=x onerror=alert(1)> 12!\nQuelle: Archiv',
                            timestamp_ms: 3020,
                        },
                        { id: 'frame-second', full_text: 'Second text', timestamp_ms: 6000 },
                        { id: 'frame-later', full_text: '<img src=x onerror=alert(1)> 12!', timestamp_ms: 9030 },
                    ],
                };
                delete ui.ocrProgress['history-1'];
                ui.emit('history:ocr', { ...progress, status: 'completed' });
            });
            assert.equal(await page.locator('#historyOcrStatus').textContent(), 'Not checked yet');
            await closeDetails();
            assert.match(
                await page.locator('[data-history-id="history-1"] .pinefetch-history-ocr-indicator').textContent(),
                /completed/
            );
            await openOcr('history-1');
            await expectOcrStatus('OCR complete');
            assert(await page.locator('#historyDetailsThumbnail').isVisible());
            await page.locator('#historyOcrText').waitFor();
            assert.equal(await page.locator('#historyOcrText img').count(), 0);
            assert.equal(await page.locator('#historyOcrText li').count(), 3);
            assert.deepEqual(await page.locator('#historyOcrText .pinefetch-ocr-select').allTextContents(), [
                '00:00:03',
                '00:00:06',
                '00:00:09',
            ]);
            await searchOcr('second');
            assert.equal(await page.locator('#historyOcrText li').count(), 1);
            assert.equal(await page.locator('#historyOcrResultCount').textContent(), '1 frame found');
            await searchOcr('');
            await page.setViewportSize({ width: 1000, height: 960 });
            assert(await page.locator('#historyOcrPanel').evaluate(el => el.scrollWidth <= el.clientWidth));
            await page.screenshot({ path: `/tmp/pinefetch-ocr-results-${engine.name()}.png` });
            await page.setViewportSize({ width: 1400, height: 960 });
            await searchOcr('No OCR matches');
            assert.equal(await page.locator('#historyOcrText li').count(), 0);
            assert.match(await page.locator('#historyOcrEmpty').textContent(), /No detected text matches/);
            await page.locator('#historyOcrCopyBtn').click();
            assert.match(await page.evaluate(() => globalThis.__pinefetchUi.copied), /^\[00:00:03.020\]\n<img/);
            await searchOcr('');
            assert(await page.locator('#historyOcrPreview').isVisible());
            await page.locator('#historyOcrFrame').waitFor();
            await page.locator('.pinefetch-ocr-select').first().focus();
            await page.keyboard.press('Enter');
            await page.locator('#historyOcrFrame').waitFor();
            assert(
                await page
                    .locator('.pinefetch-ocr-select')
                    .first()
                    .evaluate(el => el === globalThis.document.activeElement)
            );
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '00:00:03');
            assert.match(
                await page.locator('.pinefetch-ocr-result[aria-current="true"] .oj-text').textContent(),
                /^<img/
            );
            assert.equal(await page.locator('.pinefetch-ocr-result[aria-current="true"] .oj-text img').count(), 0);
            const frameCall = await page.evaluate(() =>
                globalThis.__pinefetchUi.commands.findLast(call => call.command === 'get_history_ocr_frame')
            );
            assert.equal(frameCall.args.id, 'history-1');
            assert.equal(frameCall.args.selection.frame_id, 'frame-first');
            assert(!('segment_index' in frameCall.args.selection));
            await page.evaluate(
                images => {
                    globalThis.__pinefetchUi.frameImages[9030] = images.late;
                    globalThis.__pinefetchUi.frameImages[6000] = images.current;
                    globalThis.__pinefetchUi.holdFrame = true;
                    globalThis.__pinefetchUi.ocrNodes = [...globalThis.document.querySelectorAll('#historyOcrText li')];
                },
                { late: videos[1].thumbnail, current: videos[0].thumbnail }
            );
            await page.locator('.pinefetch-ocr-select').nth(2).click();
            await page.waitForFunction(() => Boolean(globalThis.__pinefetchUi.releaseFrame));
            await page.locator('.pinefetch-ocr-select').nth(1).click();
            await page.locator('#historyOcrFrame').waitFor();
            await page.evaluate(() => globalThis.__pinefetchUi.releaseFrame());
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '00:00:06');
            assert.equal(await page.locator('#historyOcrFrame').getAttribute('src'), videos[0].thumbnail);
            assert(
                await page.evaluate(() =>
                    [...globalThis.document.querySelectorAll('#historyOcrText li')].every(
                        (node, index) => node === globalThis.__pinefetchUi.ocrNodes[index]
                    )
                )
            );
            assert.equal(
                await page.locator('.pinefetch-ocr-result[aria-current="true"] .oj-text').textContent(),
                'Second text'
            );
            assert.equal(await page.locator('#historyOcrResultCount').textContent(), '3 frames with text');
            assert(await page.locator('#historyOcrAnalyzed').textContent());
            assert(await page.locator('#historyOcrPreviousResult').isHidden());
            assert.equal(
                await page.locator('#historyOcrFrame').evaluate(el => globalThis.getComputedStyle(el).objectFit),
                'contain'
            );
            await checkOcrLayout('completed');
            await page.setViewportSize({ width: 1000, height: 960 });
            assert(await page.locator('#historyOcrPanel').evaluate(el => el.scrollWidth <= el.clientWidth));
            await page.screenshot({ path: `/tmp/pinefetch-ocr-frame-${engine.name()}.png` });
            await page.setViewportSize({ width: 1400, height: 960 });
            await page.evaluate(() => {
                globalThis.__pinefetchUi.holdFrame = true;
                globalThis.__pinefetchUi.releaseFrame = null;
            });
            await page.locator('.pinefetch-ocr-select').nth(2).click();
            await page.waitForFunction(() => Boolean(globalThis.__pinefetchUi.releaseFrame));
            await searchOcr('No OCR matches');
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '');
            assert.equal(await page.locator('.pinefetch-ocr-result[aria-current="true"]').count(), 0);
            assert(await page.locator('#historyOcrFrame').isHidden());
            await page.evaluate(() => globalThis.__pinefetchUi.releaseFrame());
            assert(await page.locator('#historyOcrFrame').isHidden());
            await searchOcr('second');
            await page.locator('#historyOcrFrame').waitFor();
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '00:00:06');
            await searchOcr('');
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '00:00:06');
            await page.clock.install();
            await page.evaluate(() => {
                globalThis.__pinefetchUi.holdFrame = true;
                globalThis.__pinefetchUi.releaseFrame = null;
            });
            await page.locator('.pinefetch-ocr-select').first().click();
            await page.waitForFunction(() => Boolean(globalThis.__pinefetchUi.releaseFrame));
            assert.equal(await page.locator('#historyOcrFrameStatus').textContent(), 'Loading selected frame…');
            await page.clock.fastForward(30001);
            await page.clock.resume();
            await page.waitForFunction(() =>
                globalThis.document.querySelector('#historyOcrFrameStatus').textContent.includes('too long')
            );
            assert.equal(await page.locator('#historyOcrPreview').getAttribute('aria-busy'), 'false');
            assert(await page.locator('#historyOcrFrame').isHidden());
            await page.evaluate(() => globalThis.__pinefetchUi.releaseFrame());
            assert(await page.locator('#historyOcrFrame').isHidden());
            await page.locator('.pinefetch-ocr-select').nth(1).click();
            await page.locator('#historyOcrFrame').waitFor();
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '00:00:06');
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failNextOcrDelete = true;
            });
            await page.locator('.pinefetch-ocr-delete').nth(1).focus();
            await page.getByRole('tooltip', { name: 'Delete frame text', exact: true }).waitFor();
            await page.keyboard.press('Escape');
            await page.keyboard.press('Enter');
            await confirmFrameDelete();
            await page.locator('#historyOcrActionNotice').waitFor();
            assert.equal(await page.locator('#historyOcrText li').count(), 3);
            assert.equal(
                await page.locator('.pinefetch-ocr-result[aria-current="true"] .oj-text').textContent(),
                'Second text'
            );
            await page.locator('.pinefetch-ocr-delete').nth(1).click();
            await confirmFrameDelete();
            await page.waitForFunction(() => globalThis.document.querySelectorAll('#historyOcrText li').length === 2);
            assert.equal(await page.locator('#historyOcrResultCount').textContent(), '2 frames with text');
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '00:00:09');
            await page.locator('#historyOcrCopyBtn').click();
            assert.doesNotMatch(await page.evaluate(() => globalThis.__pinefetchUi.copied), /Second text/);
            await searchOcr('second');
            assert.equal(await page.locator('#historyOcrResultCount').textContent(), '0 frames found');
            await searchOcr('');
            await page.locator('.pinefetch-ocr-select').nth(1).click();
            await page.locator('.pinefetch-ocr-delete').first().click();
            await confirmFrameDelete();
            await page.waitForFunction(() => globalThis.document.querySelectorAll('#historyOcrText li').length === 1);
            assert.equal(await page.locator('.pinefetch-ocr-select').textContent(), '00:00:09');
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '00:00:09');
            const groupDeletion = await page.evaluate(() =>
                globalThis.__pinefetchUi.commands.findLast(call => call.command === 'delete_history_ocr_frame_text')
            );
            assert.equal(groupDeletion.args.selection.frame_id, 'frame-first');
            assert(!('wholeSegment' in groupDeletion.args));
            assert.deepEqual(
                await page.evaluate(() =>
                    globalThis.__pinefetchUi.ocrRecords['history-1'].frames.map(frame => frame.timestamp_ms)
                ),
                [9030]
            );
            await page.locator('#historyOcrCopyBtn').click();
            assert.match(await page.evaluate(() => globalThis.__pinefetchUi.copied), /^\[00:00:09.030\]/);
            assert.doesNotMatch(
                await page.evaluate(() => globalThis.__pinefetchUi.copied),
                /00:00:03|00:00:04|00:00:05/
            );
            await closeDetails();
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failNextFrame = true;
            });
            await openOcr('history-1');
            await expectOcrStatus('OCR complete');
            assert.equal(await page.locator('#historyOcrText li').count(), 1);
            await page.waitForFunction(() =>
                globalThis.document.querySelector('#historyOcrFrameStatus').textContent.includes('unavailable')
            );
            assert(await page.locator('#historyOcrFrame').isHidden());
            await closeDetails();
            await openOcr('history-1', true);
            assert(await page.locator('.pinefetch-ocr-delete').isDisabled());
            await page.evaluate(() => {
                const ui = globalThis.__pinefetchUi;
                const progress = ui.ocrProgress['history-1'];
                ui.ocrRecords['history-1'].last_attempt_status = 'error';
                ui.ocrRecords['history-1'].last_error = 'The saved video changed.';
                delete ui.ocrProgress['history-1'];
                ui.emit('history:ocr', { ...progress, status: 'error' });
            });
            await expectOcrStatus('OCR failed');
            await page.locator('#historyOcrText').waitFor();
            assert(await page.locator('#historyOcrPreviousResult').isVisible());
            assert.match(await page.locator('#historyOcrNotice').textContent(), /saved video changed/);
            assert(await page.locator('#historyOcrCopyBtn').isEnabled());
            assert.equal(await page.locator('#historyOcrStartBtn').textContent(), 'Retry Analysis');
            await page.locator('#historyOcrStartBtn').click();
            await expectOcrStatus('Checking for OCR…');
            assert(await page.locator('#historyOcrCopyBtn').isEnabled());
            await page.locator('#historyOcrCancelBtn').click();
            await expectOcrStatus('OCR cancelled');
            assert(await page.locator('#historyOcrPreviousResult').isVisible());
            await checkOcrLayout('cancelled-retry');
            await page.locator('.pinefetch-ocr-delete').click();
            await confirmFrameDelete();
            await page.waitForFunction(
                () => globalThis.document.querySelector('#historyOcrResultCount').textContent === '0 frames with text'
            );
            assert(await page.locator('#historyOcrFrame').isHidden());
            assert(await page.locator('#historyOcrCopyBtn').isDisabled());
            assert.match(await page.locator('#historyOcrEmpty').textContent(), /No text was detected/);
            await closeDetails();
            await page.evaluate(() => {
                globalThis.__pinefetchUi.holdOcr = true;
            });
            await openOcr('history-1');
            await page.waitForFunction(() => Boolean(globalThis.__pinefetchUi.releaseOcr));
            await closeDetails();
            await openOcr('history-2');
            await expectOcrStatus('Not checked yet');
            await page.evaluate(() => globalThis.__pinefetchUi.releaseOcr());
            await page.evaluate(
                () =>
                    new Promise(resolve =>
                        globalThis.requestAnimationFrame(() => globalThis.requestAnimationFrame(resolve))
                    )
            );
            assert.equal(await page.locator('#historyOcrText').textContent(), '');
            assert.equal(await page.locator('#historyOcrStatus').textContent(), 'Not checked yet');
            assert.equal(await page.locator('#historyDetailsThumbnail').getAttribute('src'), null);
            await page.evaluate(() => {
                const ui = globalThis.__pinefetchUi;
                ui.ocrRecords['history-2'] = {
                    last_attempt_id: 'empty',
                    last_attempt_status: 'completed',
                    result_created_at: 1700000000000,
                    frames: [],
                    text: '',
                };
            });
            await page.locator('#historyOverviewTab').click();
            await page.locator('#historyOcrTab').click();
            await expectOcrStatus('OCR complete');
            assert.match(await page.locator('#historyOcrEmpty').textContent(), /No text was detected/);
            assert(await page.locator('#historyOcrCopyBtn').isDisabled());
            await page.evaluate(() => {
                const content = globalThis.__pinefetchUi.ocrRecords['history-2'];
                content.frames = Array.from({ length: 151 }, (_, index) => ({
                    id: `detected-${index}`,
                    full_text: `Detected entry ${index}`,
                    timestamp_ms: index * 1000,
                }));
                content.text = null;
            });
            await page.locator('#historyOverviewTab').click();
            await page.locator('#historyOcrTab').click();
            await page.waitForFunction(() => globalThis.document.querySelectorAll('#historyOcrText li').length === 100);
            const pinnedBeforeScrolling = await pinnedOcrLayout();
            await page.locator('#historyOcrListViewport').evaluate(el => {
                el.scrollTop = 650;
            });
            assert.deepEqual(await pinnedOcrLayout(), pinnedBeforeScrolling);
            await page.evaluate(() => {
                globalThis.__pinefetchUi.ocrNodes = [...globalThis.document.querySelectorAll('#historyOcrText li')];
            });
            await page.locator('.pinefetch-ocr-result .oj-text').nth(15).click();
            const scrollBeforeSelection = await page.locator('#historyOcrListViewport').evaluate(el => el.scrollTop);
            await page.locator('.pinefetch-ocr-result .oj-text').nth(16).click();
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '00:00:16');
            assert.equal(
                await page.locator('#historyOcrListViewport').evaluate(el => el.scrollTop),
                scrollBeforeSelection
            );
            assert(
                await page.evaluate(() =>
                    [...globalThis.document.querySelectorAll('#historyOcrText li')].every(
                        (node, index) => node === globalThis.__pinefetchUi.ocrNodes[index]
                    )
                )
            );
            await page.keyboard.press('ArrowDown');
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '00:00:17');
            await page.keyboard.press('ArrowUp');
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '00:00:16');
            assert.deepEqual(await pinnedOcrLayout(), pinnedBeforeScrolling);
            await page.locator('.pinefetch-ocr-select').nth(99).click();
            await page.keyboard.press('ArrowDown');
            await waitOcrPage();
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '00:01:40');
            assert.equal(await page.locator('#historyOcrText li').count(), 151);
            assert.deepEqual(await pinnedOcrLayout(), pinnedBeforeScrolling);
            await closeDetails();
            await openOcr('history-2');
            await expectOcrStatus('OCR complete');
            assert.equal(await page.locator('#historyOcrText li').count(), 100);
            await page.locator('#historyOcrLoadMoreBtn').click();
            await waitOcrPage();
            assert.equal(await page.locator('#historyOcrText li').count(), 151);
            await searchOcr('entry 150');
            assert.equal(await page.locator('#historyOcrText li').count(), 1);
            assert.equal(await page.locator('#historyOcrLoadMoreBtn').isVisible(), false);
            const filteredFrameTime = await page.locator('#historyOcrFrameTime').textContent();
            await page.locator('#historyOcrSearchInput').press('ArrowUp');
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), filteredFrameTime);
            await closeDetails();

            await openOcr('history-2', true);
            await expectOcrStatus('Checking for OCR…');
            assert.equal(await page.locator('#historyDetailsThumbnail').isVisible(), false);
            assert(
                await page.locator('#historyDetailsHeader').evaluate(el => el.classList.contains('pinefetch-no-media'))
            );
            await page.evaluate(() => {
                const progress = globalThis.__pinefetchUi.ocrProgress['history-2'];
                Object.assign(progress, { frames_checked: 1, observed_at_ms: 1310 });
                globalThis.__pinefetchUi.emit('history:ocr', progress);
            });
            assert.equal(await page.locator('#historyOcrProgress').getAttribute('value'), null);
            assert.equal(await ocrProgressText(), 'Video time 00:00:01 / Unknown · Frames 1 / Unknown');
            await page.locator('#historyOcrCancelBtn').click();
            await expectOcrStatus('OCR cancelled');
            await closeDetails();

            await openOcr('history-2');
            await expectOcrStatus('OCR cancelled');
            assert.equal(await page.locator('#historyOcrText li').count(), 100);
            const beforeNativeSearch = await page.evaluate(() => globalThis.__pinefetchUi.commands.length);
            await searchOcr('entry 150');
            assert.equal(await page.locator('#historyOcrText li').count(), 1);
            const nativeSearch = await page.evaluate(
                start =>
                    globalThis.__pinefetchUi.commands.slice(start).find(call => call.command === 'get_history_ocr'),
                beforeNativeSearch
            );
            assert.equal(nativeSearch.args.offset, 0);
            assert.equal(nativeSearch.args.limit, 100);
            assert.equal(nativeSearch.args.search, 'entry 150');
            assert(nativeSearch.args.resultRevision);
            await page.locator('#historyOcrCopyBtn').click();
            const unpagedCopy = await page.evaluate(() => globalThis.__pinefetchUi.copied);
            assert(unpagedCopy.includes('[00:00:00.000]\nDetected entry 0'));
            assert(unpagedCopy.endsWith('[00:02:30.000]\nDetected entry 150'));
            // A delayed page from an older query cannot overwrite the newest search.
            await page.evaluate(() => {
                globalThis.__pinefetchUi.holdOcr = true;
                globalThis.__pinefetchUi.releaseOcr = null;
            });
            await page.locator('#historyOcrSearchInput').fill('entry 120');
            await page.waitForFunction(() => Boolean(globalThis.__pinefetchUi.releaseOcr));
            await searchOcr('entry 150');
            await page.evaluate(() => globalThis.__pinefetchUi.releaseOcr());
            assert.equal(await page.locator('.pinefetch-ocr-result .oj-text').textContent(), 'Detected entry 150');
            await closeDetails();

            // A long saved result remains readable and fully copyable after an
            // interrupted retry, including observations beyond the rendered page.
            const longOcrText = `${'Vollständiger erkannter Text mit mehreren Zeilen und Umlauten. '.repeat(8)}\n${'SehrLangesWort'.repeat(20)}\nLetzte Zeile des ausgewählten Textes.`;
            await page.evaluate(text => {
                const content = globalThis.__pinefetchUi.ocrRecords['history-1'];
                content.frames = Array.from({ length: 801 }, (_, index) => ({
                    id: `saved-${index}`,
                    full_text: index === 0 ? text : `Saved observation ${index}`,
                    timestamp_ms: index * 1000,
                }));
                content.text = null;
            }, longOcrText);
            await openOcr('history-1', true);
            await expectOcrStatus('Checking for OCR…');
            assert.equal(await page.locator('#historyOcrText li').count(), 100);
            assert(await page.locator('#historyOcrPreviousResult').isVisible());
            await page.locator('#historyOcrCopyBtn').click();
            assert.match(await page.evaluate(() => globalThis.__pinefetchUi.copied), /Saved observation 800/);
            await page.evaluate(() => {
                const ui = globalThis.__pinefetchUi;
                const progress = ui.ocrProgress['history-1'];
                ui.ocrRecords['history-1'].last_attempt_status = 'interrupted';
                ui.ocrRecords['history-1'].last_error = 'The application stopped before analysis finished.';
                delete ui.ocrProgress['history-1'];
                ui.emit('history:ocr', { ...progress, status: 'interrupted' });
            });
            await expectOcrStatus('OCR interrupted');
            await page.waitForFunction(
                () =>
                    globalThis.document.querySelector('#historyOcrResultCount').textContent ===
                    '801 frames with text · 100 shown'
            );
            assert(await page.locator('#historyOcrNotice').isVisible());
            assert(await page.locator('#historyOcrPreviousResult').isVisible());
            assert.equal(await page.locator('#historyOcrStartBtn').textContent(), 'Retry Analysis');
            await searchOcr('Letzte Zeile');
            assert.equal(await page.locator('#historyOcrResultCount').textContent(), '1 frame found');
            await page.locator('.pinefetch-ocr-select').click();
            await page.locator('#historyOcrFrame').waitFor();
            assert.equal(
                await page.locator('.pinefetch-ocr-result[aria-current="true"] .oj-text').textContent(),
                longOcrText
            );
            await checkOcrLayout('interrupted-long-text');
            await page.locator('#historyOcrCopyBtn').click();
            const copiedSavedResult = await page.evaluate(() => globalThis.__pinefetchUi.copied);
            assert(copiedSavedResult.includes(longOcrText));
            assert.match(copiedSavedResult, /\[00:13:20\.000\]\nSaved observation 800/);
            await searchOcr('Saved observation 1');
            await page.locator('.pinefetch-ocr-select').first().click();
            await page.locator('#historyOcrFrame').waitFor();
            await checkOcrLayout('interrupted-retry');
            await searchOcr('');
            assert.equal(
                await page.locator('#historyOcrResultCount').textContent(),
                '801 frames with text · 100 shown'
            );
            await page.locator('#historyOcrLoadMoreBtn').click();
            await waitOcrPage();
            assert.equal(await page.locator('#historyOcrText li').count(), 200);
            await checkOcrLayout('large-result');
            const fixedLayout = await pinnedOcrLayout();
            await page.locator('#historyOcrListViewport').evaluate(el => {
                el.scrollTop = 1800;
            });
            assert.deepEqual(await pinnedOcrLayout(), fixedLayout);
            await page.screenshot({ path: `/tmp/pinefetch-ocr-review-scrolled-${engine.name()}.png` });
            for (const height of [960, 720]) {
                await page.setViewportSize({ width: 800, height });
                const stacked = await page.evaluate(() => {
                    const rect = selector => globalThis.document.querySelector(selector).getBoundingClientRect();
                    const frame = rect('.pinefetch-ocr-frame-stage');
                    const results = rect('#historyOcrResults');
                    const list = rect('#historyOcrListViewport');
                    const footer = rect('.pinefetch-ocr-analysis-info');
                    const panel = globalThis.document.querySelector('#historyOcrPanel');
                    return {
                        stacked: results.top >= frame.bottom && Math.abs(results.width - frame.width) < 1,
                        frameHeight: frame.height,
                        listHeight: list.height,
                        footerBottom: footer.bottom,
                        panelBottom: panel.getBoundingClientRect().bottom,
                        fits: panel.scrollWidth <= panel.clientWidth && panel.scrollHeight <= panel.clientHeight,
                    };
                });
                assert(stacked.stacked);
                assert(stacked.panelBottom <= height, JSON.stringify(stacked));
                assert(stacked.frameHeight > 70, JSON.stringify(stacked));
                assert(stacked.listHeight > 50, JSON.stringify(stacked));
                assert(stacked.footerBottom <= stacked.panelBottom + 1, JSON.stringify(stacked));
                assert(stacked.fits, JSON.stringify(stacked));
                await page.screenshot({ path: `/tmp/pinefetch-ocr-review-narrow-800x${height}-${engine.name()}.png` });
            }
            await page.setViewportSize({ width: 1400, height: 960 });
            await closeDetails();

            await page.setViewportSize({ width: 1400, height: 960 });
            await page.evaluate(() => {
                const content = globalThis.__pinefetchUi.ocrRecords['history-2'];
                Object.assign(content, {
                    last_attempt_status: 'completed',
                    last_error: null,
                    text: null,
                    frames: [
                        { id: 'precise-a', timestamp_ms: 8020, full_text: 'Same word\nQuelle: Archiv\nSame word' },
                        { id: 'precise-b', timestamp_ms: 8030, full_text: 'Same word\nDifferent block' },
                    ],
                });
            });
            await openOcr('history-2');
            await expectOcrStatus('OCR complete');
            assert.deepEqual(await page.locator('.pinefetch-ocr-select').allTextContents(), ['00:00:08', '00:00:08']);
            assert.equal(
                await page.locator('.pinefetch-ocr-result .oj-text').first().textContent(),
                'Same word\nQuelle: Archiv\nSame word'
            );
            await page.locator('.pinefetch-ocr-select').nth(1).click();
            const preciseSelection = await page.evaluate(() =>
                globalThis.__pinefetchUi.commands.findLast(call => call.command === 'get_history_ocr_frame')
            );
            assert.equal(preciseSelection.args.selection.frame_id, 'precise-b');
            // Cancel keeps all frame texts and the native delete command untouched.
            const deletesBeforeCancel = await page.evaluate(
                () =>
                    globalThis.__pinefetchUi.commands.filter(call => call.command === 'delete_history_ocr_frame_text')
                        .length
            );
            await page.locator('.pinefetch-ocr-delete').first().click();
            await page
                .getByRole('dialog', { name: 'Delete frame text?' })
                .getByRole('button', { name: 'Cancel', exact: true })
                .click();
            assert.equal(
                await page.evaluate(
                    () =>
                        globalThis.__pinefetchUi.commands.filter(
                            call => call.command === 'delete_history_ocr_frame_text'
                        ).length
                ),
                deletesBeforeCancel
            );
            assert.equal(await page.locator('#historyOcrText li').count(), 2);
            // Delete the whole selected frame, then reject its late image response.
            await page.evaluate(() => {
                globalThis.__pinefetchUi.holdFrame = true;
                globalThis.__pinefetchUi.releaseFrame = null;
            });
            await page.locator('.pinefetch-ocr-select').first().click();
            await page.waitForFunction(() => Boolean(globalThis.__pinefetchUi.releaseFrame));
            await page.locator('.pinefetch-ocr-delete').first().click();
            await confirmFrameDelete();
            await page.waitForFunction(
                () => globalThis.document.querySelector('#historyOcrResultCount').textContent === '1 frame with text'
            );
            await page.locator('#historyOcrFrame').waitFor();
            const survivingImage = await page.locator('#historyOcrFrame').getAttribute('src');
            await page.evaluate(() => globalThis.__pinefetchUi.releaseFrame());
            assert.equal(await page.locator('#historyOcrFrame').getAttribute('src'), survivingImage);
            assert.equal(await page.locator('#historyOcrText li').getAttribute('data-ocr-key'), 'precise-b');
            await closeDetails();
            // Bulk cleanup counts every stored frame even with no matching UI rows.
            await page.evaluate(() => {
                const ui = globalThis.__pinefetchUi;
                ui.ocrRecords['history-2'].frames = [
                    ...Array.from({ length: 350 }, (_, index) => ({
                        id: `duplicate-${index}`,
                        frame_key: index,
                        timestamp_ms: index * 1000,
                        full_text: 'Repeated OCR text',
                    })),
                    { id: 'unique-after', frame_key: 350, timestamp_ms: 350000, full_text: 'Later unique text' },
                ];
                ui.duplicateIds = Array.from({ length: 349 }, (_, index) => `duplicate-${index + 1}`);
            });
            await openOcr('history-2');
            await expectOcrStatus('OCR complete');
            assert.equal(await page.locator('#historyOcrText li').count(), 100);
            await searchOcr('not in these results');
            const duplicatesButton = page.getByRole('button', { name: 'Remove All Duplicates', exact: true });
            const bulkDialog = page.getByRole('dialog', { name: 'Remove all duplicate OCR frames?' });
            const removalCalls = () =>
                page.evaluate(
                    () =>
                        globalThis.__pinefetchUi.commands.filter(
                            call => call.command === 'remove_history_ocr_duplicates'
                        ).length
                );
            const initialRemovals = await removalCalls();
            await duplicatesButton.focus();
            await page.keyboard.press('Enter');
            await bulkDialog.waitFor();
            assert.match(await bulkDialog.textContent(), /349 duplicate OCR frames/);
            assert.equal(await page.evaluate(() => globalThis.document.activeElement.textContent), 'Cancel');
            await page.keyboard.press('Escape');
            await bulkDialog.waitFor({ state: 'hidden' });
            assert.equal(await removalCalls(), initialRemovals);
            await page.waitForFunction(() => globalThis.document.activeElement.id === 'historyOcrDuplicatesBtn');
            // Keep a surviving selection and retain the active search after cleanup.
            await searchOcr('Later unique');
            await page.locator('#historyOcrFrame').waitFor();
            await page.evaluate(() => {
                globalThis.__pinefetchUi.holdDuplicateRemoval = true;
            });
            await duplicatesButton.click();
            await bulkDialog.getByRole('button', { name: 'Remove duplicates', exact: true }).click();
            await page.waitForFunction(() => Boolean(globalThis.__pinefetchUi.releaseDuplicateRemoval));
            assert.equal(await page.locator('#historyOcrDuplicatesBtn').getAttribute('aria-busy'), 'true');
            for (const selector of [
                '#historyOcrDuplicatesBtn',
                '#historyOcrCopyBtn',
                '#historyOcrStartBtn',
                '#historyOcrSearchInput',
                '.pinefetch-ocr-delete',
            ])
                assert(await page.locator(selector).isDisabled());
            await page.evaluate(() => globalThis.__pinefetchUi.releaseDuplicateRemoval());
            await page.waitForFunction(() => globalThis.__pinefetchUi.ocrRecords['history-2'].frames.length === 2);
            await page.getByText('Removed 349 duplicate OCR frames.', { exact: true }).waitFor();
            await page.locator('#historyOcrFrame').waitFor();
            assert.equal(
                await page.locator('.pinefetch-ocr-result[aria-current="true"]').getAttribute('data-ocr-key'),
                'unique-after'
            );
            assert.equal(await page.locator('#historyOcrSearchInput').inputValue(), 'Later unique');
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '00:05:50');
            await page.locator('#historyOcrCopyBtn').click();
            assert.equal(
                await page.evaluate(() => globalThis.__pinefetchUi.copied),
                '[00:00:00.000]\nRepeated OCR text\n\n[00:05:50.000]\nLater unique text'
            );
            await searchOcr('Repeated');
            assert.equal(await page.locator('#historyOcrText li').count(), 1);
            await duplicatesButton.click();
            await page.getByText('No duplicate OCR frames found.', { exact: true }).waitFor();
            assert.equal(await removalCalls(), initialRemovals + 1);
            assert.equal(await bulkDialog.count(), 0);
            // A selected duplicate is replaced, and its late image is discarded.
            await closeDetails();
            await page.evaluate(() => {
                const ui = globalThis.__pinefetchUi;
                ui.ocrRecords['history-2'].frames = [
                    { id: 'keep-first', frame_key: 0, timestamp_ms: 0, full_text: 'Repeated text' },
                    { id: 'drop-selected', frame_key: 1, timestamp_ms: 1000, full_text: 'Repeated text' },
                ];
                ui.duplicateIds = ['drop-selected'];
            });
            await openOcr('history-2');
            await expectOcrStatus('OCR complete');
            await page.locator('#historyOcrFrame').waitFor();
            await page.evaluate(() => {
                globalThis.__pinefetchUi.holdFrame = true;
                globalThis.__pinefetchUi.releaseFrame = null;
            });
            await page.locator('.pinefetch-ocr-select').nth(1).click();
            await page.waitForFunction(() => Boolean(globalThis.__pinefetchUi.releaseFrame));
            // A write error keeps rows, selection and original revision available.
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failDuplicateRemoval = true;
            });
            await duplicatesButton.click();
            await bulkDialog.getByRole('button', { name: 'Remove duplicates', exact: true }).click();
            await page.locator('#historyOcrActionNotice').waitFor();
            assert.equal(await page.locator('#historyOcrText li').count(), 2);
            assert.equal(
                await page.locator('.pinefetch-ocr-result[aria-current="true"]').getAttribute('data-ocr-key'),
                'drop-selected'
            );
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failDuplicateRemoval = false;
            });
            // A newer result between preview and confirmation is rejected.
            await duplicatesButton.click();
            await bulkDialog.waitFor();
            await page.evaluate(() => {
                globalThis.__pinefetchUi.ocrRecords['history-2'].frames[1].full_text = 'Changed text';
            });
            await bulkDialog.getByRole('button', { name: 'Remove duplicates', exact: true }).click();
            await page.waitForFunction(() =>
                globalThis.document.querySelector('#historyOcrActionNotice').textContent.includes('results changed')
            );
            assert.equal(await page.locator('#historyOcrText li').count(), 2);
            await page.evaluate(() => {
                globalThis.__pinefetchUi.ocrRecords['history-2'].frames[1].full_text = 'Repeated text';
            });
            await duplicatesButton.click();
            await bulkDialog.getByRole('button', { name: 'Remove duplicates', exact: true }).click();
            await page.waitForFunction(() => globalThis.document.querySelectorAll('#historyOcrText li').length === 1);
            await page.locator('#historyOcrFrame').waitFor();
            await page.evaluate(() => globalThis.__pinefetchUi.releaseFrame());
            assert.equal(
                await page.locator('.pinefetch-ocr-result[aria-current="true"]').getAttribute('data-ocr-key'),
                'keep-first'
            );
            assert.equal(await page.locator('#historyOcrFrameTime').textContent(), '00:00:00');
            // A late preview after detail navigation must not open a dialog.
            await page.evaluate(() => {
                globalThis.__pinefetchUi.holdDuplicatePreview = true;
            });
            await duplicatesButton.click();
            await page.waitForFunction(() => Boolean(globalThis.__pinefetchUi.releaseDuplicatePreview));
            await closeDetails();
            await openOcr('history-1');
            await page.evaluate(() => globalThis.__pinefetchUi.releaseDuplicatePreview());
            assert.equal(await bulkDialog.count(), 0);
            await closeDetails();
            await page.evaluate(() => {
                const content = globalThis.__pinefetchUi.ocrRecords['history-2'];
                content.frames = [];
                content.text = '[00:00:08] Unstructured legacy text';
            });
            await openOcr('history-2');
            await expectOcrStatus('OCR complete');
            assert.equal(await page.locator('#historyOcrText li').count(), 1);
            assert(await page.locator('.pinefetch-ocr-select').isDisabled());
            assert(await page.locator('.pinefetch-ocr-delete').isDisabled());
            assert.match(
                await page.locator('#historyOcrResultCount').textContent(),
                /0 frames with text.*Legacy text available/
            );
            await searchOcr('no legacy matches');
            assert.equal(await page.locator('#historyOcrText li').count(), 0);
            await page.locator('#historyOcrCopyBtn').click();
            assert.equal(
                await page.evaluate(() => globalThis.__pinefetchUi.copied),
                '[00:00:08] Unstructured legacy text'
            );
            await closeDetails();

            // A transcript without a retained video fails before a native job
            // exists and preserves all previously saved OCR text.
            await page.evaluate(() => {
                globalThis.__pinefetchUi.ocrSourceError = 'No downloaded video is available for OCR.';
            });
            await openOcr('history-2');
            await expectOcrStatus('OCR complete');
            const savedOcrText = await page.locator('#historyOcrText').textContent();
            await page.locator('#historyOcrStartBtn').click();
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('#historyOcrNotice')
                    .textContent.includes('Keep video with transcript')
            );
            assert.equal(await page.locator('#historyOcrText').textContent(), savedOcrText);
            assert.equal(await page.locator('#historyOcrProgress').isVisible(), false);
            assert.deepEqual(await page.evaluate(() => Object.keys(globalThis.__pinefetchUi.ocrProgress)), []);
            assert.equal(await page.locator('#historyOcrStartBtn').isEnabled(), true);
            await page.evaluate(() => {
                globalThis.__pinefetchUi.ocrSourceError = 'OCR requires a supported video container.';
            });
            await page.locator('#historyOcrStartBtn').click();
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('#historyOcrNotice')
                    .textContent.includes('Text, audio and image files cannot be analyzed')
            );
            assert.equal(await page.locator('#historyOcrText').textContent(), savedOcrText);
            await page.evaluate(() => {
                globalThis.__pinefetchUi.ocrSourceError = null;
            });
            await closeDetails();

            // Account navigation is local; profile metrics and video discovery require explicit clicks.
            await openOcr('history-1');
            await page.locator('#historyOverviewTab').click();
            await page.locator('#historyDetailsAccountBreadcrumbBtn').click();
            await page.locator('#historyAccountView').waitFor();
            await page.waitForFunction(
                () => globalThis.document.querySelector('#historyAccountSavedList').childElementCount > 0
            );
            assert.equal(await page.evaluate(() => globalThis.__pinefetchUi.accountFetches), 0);
            assert.equal(await page.evaluate(() => globalThis.__pinefetchUi.profileFetches), 0);
            assert(await page.locator('#historyAccountOpenBtn').isDisabled());
            assert(await page.locator('#historyAccountProfileHint').isVisible());
            assert.match(await page.locator('#historyAccountVideosStateTitle').textContent(), /Discover more/);
            await page.waitForFunction(
                () => !globalThis.document.querySelector('#historyAccountSocialLoadBtn').disabled
            );
            await page.evaluate(async () => {
                const page = await globalThis.__TAURI__.core.invoke('get_history', { offset: 0, limit: 100 });
                const first = page.entries.find(entry => entry.id === 'history-1');
                globalThis.__pinefetchUi.setHistoryEntries([
                    ...page.entries,
                    ...Array.from({ length: 25 }, (_, index) => ({
                        ...first,
                        id: `account-extra-${index}`,
                        title: `Saved account video ${index}`,
                        filename: `${'Long filename with details '.repeat(12)}${index}.mp4`,
                    })),
                    { ...first, id: 'account-different-platform', source: 'tiktok' },
                    { ...first, id: 'account-similar-name', uploader: `${first.uploader} Extra` },
                ]);
                globalThis.__pinefetchUi.holdAccount = true;
            });
            await page.locator('#historyAccountSocialLoadBtn').click();
            assert(await page.locator('#historyAccountSocialLoadBtn').isDisabled());
            await page.locator('#historyAccountHistoryBtn').click();
            await page.locator('#historyView').waitFor();
            await page.locator('[data-history-id="history-1"] .pinefetch-history-open-btn').click();
            await page.locator('#historyDetailsContent').waitFor();
            await page.evaluate(() => globalThis.__pinefetchUi.releaseAccount());
            assert(await page.locator('#historyAccountView').isHidden());
            await page.locator('#historyDetailsAccountBreadcrumbBtn').click();
            await page.waitForFunction(
                () => globalThis.document.querySelector('#historyAccountSocialMetrics').childElementCount === 2
            );
            assert.equal(await page.evaluate(() => globalThis.__pinefetchUi.accountFetches), 1);
            assert.equal(await page.locator('#historyAccountSavedList button').count(), 20);
            await page.locator('#historyAccountMoreSavedBtn').click();
            await page.waitForFunction(
                () => globalThis.document.querySelector('#historyAccountSavedList').childElementCount === 27
            );
            assert(await page.locator('#historyAccountMoreSavedBtn').isHidden());
            assert.equal(await page.locator('#historyAccountSavedCount').textContent(), '27');
            const accountMetrics = page.locator('#historyAccountSocialMetrics');
            assert.match(await accountMetrics.innerText(), /Subscribers[\s\S]*12[,.]345/);
            assert.match(await accountMetrics.innerText(), /Posts[\s\S]*0/);
            assert(!(await accountMetrics.innerText().then(text => text.includes('—'))));
            assert.equal(await accountMetrics.locator('.pinefetch-social-metric').count(), 2);
            assert.equal(await page.locator('#historyAccountSocialUnavailableList dt').count(), 2);
            await page.locator('#historyAccountSocialUnavailableSummary').focus();
            await page.keyboard.press('Enter');
            assert(await page.locator('#historyAccountSocialUnavailableList').isVisible());
            assert(await page.locator('#historyAccountOpenBtn').isEnabled());
            assert(await page.locator('#historyAccountProfileHint').isHidden());
            await page.locator('#historyAccountOpenBtn').click();
            const savedFile = page.locator('#historyAccountSavedList .pinefetch-account-filename').first();
            assert.equal(await savedFile.getAttribute('title'), await savedFile.textContent());
            const savedButton = page.locator('#historyAccountSavedList button').first();
            assert.equal(await savedButton.getAttribute('aria-describedby'), await savedFile.getAttribute('id'));
            assert.equal(await page.locator('#historyAccountOpenBtn').getAttribute('aria-describedby'), null);
            await savedButton.hover();
            assert(
                await page
                    .locator('#historyAccountSavedList li')
                    .first()
                    .evaluate(el => globalThis.getComputedStyle(el).backgroundColor !== 'rgba(0, 0, 0, 0)')
            );
            await savedButton.focus();
            // macOS WebKit defaults to form-only Tab navigation; Option-Tab includes buttons.
            await page.keyboard.press(engine.name() === 'webkit' ? 'Alt+Tab' : 'Tab');
            const accountFocus = await page.evaluate(() => {
                const active = globalThis.document.activeElement;
                return {
                    tag: active.tagName,
                    label: active.getAttribute('aria-label'),
                    classes: active.className,
                    visible: active.matches(':focus-visible'),
                    outline: globalThis.getComputedStyle(active).outlineStyle,
                    inList: Boolean(active.closest('#historyAccountSavedList')),
                };
            });
            assert(
                accountFocus.inList && accountFocus.visible && accountFocus.outline !== 'none',
                JSON.stringify(accountFocus)
            );

            await page.evaluate(() => {
                globalThis.__pinefetchUi.failAccount = true;
            });
            await page.locator('#historyAccountSocialLoadBtn').click();
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('#historyAccountSocialStatus')
                    .textContent.includes('Previously saved data')
            );
            assert.match(await accountMetrics.innerText(), /12[,.]345/);
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failAccount = false;
            });
            await page.evaluate(() => {
                globalThis.__pinefetchUi.unavailableAccount = true;
            });
            await page.locator('#historyAccountSocialLoadBtn').click();
            await page.waitForFunction(
                () => globalThis.document.querySelector('#historyAccountSocialUnavailableList').childElementCount === 8
            );
            assert.equal(await accountMetrics.locator('.pinefetch-social-metric').count(), 0);
            assert.match(await page.locator('#historyAccountSocialStatus').textContent(), /No public metrics/);
            assert.match(
                await page.locator('#historyAccountSocialUnavailableList').innerText(),
                /Account identity could not be verified/
            );
            assert.doesNotMatch(
                await page.locator('#historyAccountSocialUnavailableList').innerText(),
                /Video identity/
            );
            await page.evaluate(() => {
                globalThis.__pinefetchUi.unavailableAccount = false;
            });
            await page.locator('#historyAccountSocialLoadBtn').click();
            await page.waitForFunction(
                () => globalThis.document.querySelector('#historyAccountSocialMetrics').childElementCount === 2
            );
            await page.evaluate(() => {
                globalThis.__pinefetchUi.holdProfileVideos = true;
                globalThis.__pinefetchUi.emptyProfileVideos = true;
            });
            await page.locator('#historyAccountVideosBtn').click();
            assert(await page.locator('#historyAccountVideosBtn').isDisabled());
            assert.equal(await page.locator('#historyAccountRemoteList').getAttribute('aria-busy'), 'true');
            assert(await page.locator('#historyAccountVideosLoading').isVisible());
            await page.evaluate(() => globalThis.__pinefetchUi.releaseProfileVideos());
            await page.waitForFunction(
                () =>
                    globalThis.document.querySelector('#historyAccountVideosStateTitle').textContent ===
                    'No public videos found'
            );
            assert(await page.locator('#historyAccountRemoteList').isHidden());
            assert(await page.locator('#historyAccountVideosLoading').isHidden());
            await page.evaluate(() => {
                globalThis.__pinefetchUi.emptyProfileVideos = false;
                globalThis.__pinefetchUi.failProfileVideos = true;
            });
            await page.locator('#historyAccountVideosBtn').click();
            await page.waitForFunction(() =>
                globalThis.document.querySelector('#historyAccountVideosStatus').classList.contains('oj-status-error')
            );
            assert.equal(await page.locator('#historyAccountVideosBtn').textContent(), 'Try again');
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failProfileVideos = false;
            });
            await page.locator('#historyAccountVideosBtn').click();
            await page.locator('#historyAccountRemoteList button').waitFor();
            assert.equal(await page.locator('#historyAccountRemoteList img').count(), 0);
            await page.locator('#historyAccountMoreVideosBtn').click();
            assert.equal(await page.locator('#historyAccountRemoteList button').count(), 1);
            assert(await page.locator('#historyAccountMoreVideosBtn').isHidden());
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failProfileVideos = true;
            });
            await page.locator('#historyAccountVideosBtn').click();
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('#historyAccountVideosStatus')
                    .textContent.includes('Previously loaded videos')
            );
            assert.equal(await page.locator('#historyAccountRemoteList button').count(), 1);
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failProfileVideos = false;
            });
            await page.locator('#historyAccountSavedList button').first().click();
            await page.locator('#historyDetailsContent').waitFor();
            assert.equal(await page.locator('#historyDetailsBreadcrumbAccount').textContent(), videos[0].uploader);
            assert.equal(await page.locator('#historyDetailsBreadcrumbCurrent').textContent(), videos[0].title);
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failAccountRead = true;
            });
            await page.locator('#historyDetailsAccountBreadcrumbBtn').click();
            await page.locator('#historyAccountRetryBtn').waitFor();
            assert(
                await page.locator('#historyAccountStatus').evaluate(el => el.classList.contains('oj-status-error'))
            );
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failAccountRead = false;
            });
            await page.locator('#historyAccountRetryBtn').click();
            await page.locator('#historyAccountView').waitFor();
            await page.waitForFunction(
                () => globalThis.document.querySelector('#historyAccountSocialMetrics').childElementCount === 2
            );
            const accountViewport = page.viewportSize();
            await page.setViewportSize({ width: 390, height: 844 });
            assert(await page.evaluate(() => globalThis.document.documentElement.scrollWidth <= globalThis.innerWidth));
            const clippedFile = page.locator('#historyAccountSavedList .pinefetch-account-filename').nth(2);
            assert(
                await clippedFile.evaluate(
                    el => el.scrollWidth > el.clientWidth && globalThis.getComputedStyle(el).textOverflow === 'ellipsis'
                )
            );
            await page.locator('#historyAccountSocialLoadBtn').scrollIntoViewIfNeeded();
            assert(await page.locator('#historyAccountSocialLoadBtn').isVisible());
            await page.screenshot({ path: `/tmp/pinefetch-account-${engine.name()}.png` });
            for (const width of [620, 800, 1000]) {
                await page.setViewportSize({ width, height: 800 });
                assert(
                    await page.evaluate(() => globalThis.document.documentElement.scrollWidth <= globalThis.innerWidth)
                );
            }
            await page.setViewportSize(accountViewport);
            await page.locator('#historyAccountVideosBtn').click();
            await page.locator('#historyAccountRemoteList button').click();
            await page.evaluate(() => globalThis.__pinefetchUi.setHistoryEntries(null));
            await page.locator('#downloadView').waitFor();
            await page.waitForFunction(
                () => globalThis.document.querySelector('#infoTitle').textContent !== 'No video loaded'
            );
            assert.equal(await page.locator('#urlInput').inputValue(), videos[0].url);
            await page.locator('#viewHistoryBtn').click();
            await page.locator('#historyAccountHistoryBtn').click();
            await page.locator('#historyView').waitFor();

            // Social Insights load from SQLite on tab open; only a click starts a fetch.
            await openOcr('history-1');
            await page.locator('#historyOcrTab').focus();
            await page.keyboard.press('ArrowRight');
            assert.equal(await page.locator('#historySocialTab').getAttribute('aria-selected'), 'true');
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('#historySocialStatus')
                    .textContent.includes('No Social Insights saved')
            );
            assert.equal(await page.evaluate(() => globalThis.__pinefetchUi.socialFetches), 0);
            await page.evaluate(() => {
                globalThis.__pinefetchUi.holdSocial = true;
            });
            await page.locator('#historySocialLoadBtn').click();
            await page.waitForFunction(() => Boolean(globalThis.__pinefetchUi.releaseSocial));
            assert(await page.locator('#historySocialLoadBtn').isDisabled());
            assert.equal(await page.locator('#historySocialLoadBtn').getAttribute('aria-busy'), 'true');
            assert.equal(await page.evaluate(() => globalThis.__pinefetchUi.socialFetches), 1);
            await closeDetails();
            await openOcr('history-2');
            await page.locator('#historySocialTab').click();
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('#historySocialStatus')
                    .textContent.includes('No Social Insights saved')
            );
            await page.evaluate(() => globalThis.__pinefetchUi.releaseSocial());
            assert.equal(await page.locator('#historySocialMetrics').textContent(), '');
            await closeDetails();
            await openOcr('history-1');
            await page.locator('#historySocialTab').click();
            await page.waitForFunction(
                () =>
                    globalThis.document.querySelectorAll('#historySocialMetrics .pinefetch-social-metric').length === 9
            );
            assert.equal(await page.evaluate(() => globalThis.__pinefetchUi.socialFetches), 1);
            const insightCards = page.locator('#historySocialMetrics .pinefetch-social-metric');
            assert.match(await insightCards.nth(0).textContent(), /12[,.]345.*Approximate/s);
            assert.match(await insightCards.nth(1).textContent(), /Likes0Public count/);
            assert.match(await insightCards.nth(3).textContent(), /Shares—Not supported/);
            assert.match(await page.locator('#historySocialMeta').textContent(), /Fetched.*YouTube/);
            const storedMeta = await page.locator('#historySocialMeta').textContent();
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failSocial = true;
            });
            await page.locator('#historySocialLoadBtn').click();
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('#historySocialStatus')
                    .textContent.includes('Previously saved data is still shown')
            );
            assert.equal(await page.locator('#historySocialMeta').textContent(), storedMeta);
            assert.match(await insightCards.nth(0).textContent(), /12[,.]345/);
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failSocial = false;
            });
            await page.locator('#historySocialLoadBtn').click();
            await page.waitForFunction(
                previous => globalThis.document.querySelector('#historySocialMeta').textContent !== previous,
                storedMeta
            );
            await page.locator('.oj-toast button').evaluateAll(buttons => buttons.forEach(button => button.click()));
            await page.screenshot({ path: `/tmp/pinefetch-social-insights-${engine.name()}.png` });
            await page.setViewportSize({ width: 800, height: 960 });
            assert(await page.locator('#historySocialPanel').evaluate(el => el.scrollWidth <= el.clientWidth));
            await page.setViewportSize({ width: 1400, height: 960 });
            for (const score of [-3, 0]) {
                await page.evaluate(value => {
                    globalThis.__pinefetchUi.socialScore = value;
                }, score);
                await page.locator('#historySocialLoadBtn').click();
                const card = page
                    .locator('#historySocialMetrics .pinefetch-social-metric')
                    .filter({ has: page.getByRole('heading', { name: 'Score', exact: true }) });
                await page.waitForFunction(
                    value =>
                        [
                            ...globalThis.document.querySelectorAll('#historySocialMetrics .pinefetch-social-metric'),
                        ].some(
                            card =>
                                card.querySelector('h3').textContent === 'Score' &&
                                card.querySelector('.oj-heading-2').textContent === String(value)
                        ),
                    score
                );
                assert.equal(await card.locator('.oj-heading-2').textContent(), String(score));
                assert.match(await card.innerText(), /Public score/);
            }
            await page.evaluate(() => {
                globalThis.__pinefetchUi.socialScore = null;
                globalThis.__pinefetchUi.unsupportedSocial = true;
            });
            await page.locator('#historySocialLoadBtn').click();
            await page.waitForFunction(() =>
                globalThis.document.querySelector('#historySocialStatus').textContent.includes('not supported')
            );
            assert.equal(
                await page
                    .locator('#historySocialMetrics .oj-heading-2')
                    .allTextContents()
                    .then(values => values.every(value => value === '—')),
                true
            );
            await page.evaluate(() => {
                globalThis.__pinefetchUi.unsupportedSocial = false;
            });
            await closeDetails();

            // Keywords use saved sources, filter all terms and ignore replies for another video.
            await openOcr('history-1');
            const socialFetchesBeforeKeywords = await page.evaluate(() => globalThis.__pinefetchUi.socialFetches);
            await page.locator('#historySocialTab').focus();
            await page.keyboard.press('ArrowRight');
            assert.equal(await page.locator('#historyKeywordsTab').getAttribute('aria-selected'), 'true');
            const keywordRows = page.locator('#historyKeywordsRows tr');
            const keywordSearch = page.locator('#historyKeywordsSearch');
            const keywordSource = page.locator('#historyKeywordsSourceTrigger');
            const keywordSourceMenu = page.locator('#historyKeywordsSourceMenu');
            await page.waitForFunction(
                () => globalThis.document.querySelectorAll('#historyKeywordsRows tr').length === 3
            );
            assert.deepEqual(await keywordRows.first().locator('th,td').allTextContents(), [
                'Nintendo',
                '3',
                '1',
                '2',
                '6',
            ]);
            assert.deepEqual(await keywordRows.locator('td:last-child').allTextContents(), ['6', '5', '5']);
            assert.equal(
                await page.locator('#historyKeywordsTable th[aria-sort]').getAttribute('aria-sort'),
                'descending'
            );
            await keywordSource.focus();
            await page.keyboard.press('ArrowDown');
            await keywordSourceMenu.waitFor();
            assert.equal(await keywordSource.getAttribute('aria-expanded'), 'true');
            assert.equal(await page.evaluate(() => globalThis.document.activeElement.dataset.ojValue), 'all');
            await page.keyboard.press('ArrowDown');
            await page.keyboard.press('Enter');
            await keywordSourceMenu.waitFor({ state: 'hidden' });
            assert.equal(await page.locator('#historyKeywordsSourceValue').textContent(), 'In all three sources');
            assert.equal(
                await keywordSourceMenu.locator('[aria-checked="true"]').getAttribute('data-oj-value'),
                'shared'
            );
            assert(await keywordSource.evaluate(node => node === globalThis.document.activeElement));
            assert.equal(await keywordRows.count(), 3);
            await selectHistoryFilter('historyKeywordsSource', 'ocr', 'In OCR');
            assert.equal(await keywordRows.count(), 3);
            await selectHistoryFilter('historyKeywordsSource', 'captions', 'In captions');
            assert.equal(await keywordRows.count(), 3);
            await selectHistoryFilter('historyKeywordsSource', 'transcript', 'In transcript');
            assert.equal(await keywordRows.count(), 3);
            await keywordSearch.fill('nInTeNdO');
            assert.equal(await keywordRows.count(), 1);
            await keywordSearch.fill('missing keyword');
            assert.match(await page.locator('#historyKeywordsEmpty').textContent(), /No keywords match/);
            assert(await page.locator('#historyKeywordsTable').isHidden());
            await keywordSearch.fill('');
            await selectHistoryFilter('historyKeywordsSource', 'all', 'All keywords');
            await keywordSource.focus();
            await page.keyboard.press('ArrowUp');
            await keywordSourceMenu.waitFor();
            assert.equal(await page.evaluate(() => globalThis.document.activeElement.dataset.ojValue), 'captions');
            await page.keyboard.press('Escape');
            await keywordSourceMenu.waitFor({ state: 'hidden' });
            assert(await keywordSource.evaluate(node => node === globalThis.document.activeElement));
            await keywordSource.click();
            await keywordSourceMenu.waitFor();
            await keywordSearch.click();
            await keywordSourceMenu.waitFor({ state: 'hidden' });
            assert.equal(await page.locator('#historyKeywordsSourceValue').textContent(), 'All keywords');
            await page.setViewportSize({ width: 600, height: 960 });
            assert(await page.locator('#historyKeywordsPanel').evaluate(el => el.scrollWidth <= el.clientWidth));
            await page.setViewportSize({ width: 1400, height: 960 });
            await page.evaluate(() => {
                const ui = globalThis.__pinefetchUi;
                ui.keywordRecords['history-1'].terms.push(
                    ...Array.from({ length: 250 }, (_, i) => ({
                        term: `Term${i}`,
                        transcript: 5,
                        ocr: 0,
                        captions: 0,
                        total: 5,
                        sources: 1,
                    }))
                );
                ui.keywordRecords['history-1'].terms.push({
                    term: '<img src=x onerror=alert(1)>',
                    transcript: 5,
                    ocr: 0,
                    captions: 0,
                    total: 5,
                    sources: 1,
                });
                ui.holdKeywords = true;
            });
            await page.locator('#historyKeywordsRefreshBtn').click();
            await page.waitForFunction(() => Boolean(globalThis.__pinefetchUi.releaseKeywords));
            assert(await keywordSearch.isDisabled());
            assert(await keywordSource.isDisabled());
            assert(await page.locator('#historyKeywordsRefreshBtn').isDisabled());
            await page.evaluate(() => globalThis.__pinefetchUi.releaseKeywords());
            await page.waitForFunction(
                () => globalThis.document.querySelectorAll('#historyKeywordsRows tr').length === 200
            );
            await page.locator('#historyKeywordsLoadMoreBtn').click();
            assert.equal(await keywordRows.count(), 254);
            await selectHistoryFilter('historyKeywordsSource', 'ocr', 'In OCR', true);
            assert.equal(await keywordRows.count(), 3);
            await selectHistoryFilter('historyKeywordsSource', 'all', 'All keywords');
            assert.equal(await keywordRows.count(), 200);
            await page.locator('#historyKeywordsLoadMoreBtn').click();
            assert.equal(await keywordRows.count(), 254);
            assert.equal(await page.locator('#historyKeywordsRows img').count(), 0);
            await keywordSearch.fill('term249');
            assert.equal(await keywordRows.count(), 1);
            await keywordSearch.fill('');
            assert.equal(await keywordRows.count(), 200);
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failKeywords = true;
            });
            await page.locator('#historyKeywordsRefreshBtn').click();
            await page.waitForFunction(() =>
                globalThis.document
                    .querySelector('#historyKeywordsStatus')
                    .textContent.includes('Previous results are still shown')
            );
            assert.equal(await keywordRows.count(), 200);
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failKeywords = false;
                globalThis.__pinefetchUi.holdKeywords = true;
                globalThis.__pinefetchUi.releaseKeywords = null;
            });
            await page.locator('#historyKeywordsRefreshBtn').click();
            await page.waitForFunction(() => Boolean(globalThis.__pinefetchUi.releaseKeywords));
            await closeDetails();
            await openOcr('history-2');
            await page.locator('#historyKeywordsTab').click();
            await page.locator('#historyKeywordsEmpty').waitFor();
            assert.equal(await page.locator('#historyKeywordsSourceValue').textContent(), 'All keywords');
            assert.match(await page.locator('#historyKeywordsEmpty').textContent(), /No saved text is available/);
            await page.evaluate(() => globalThis.__pinefetchUi.releaseKeywords());
            assert.equal(await keywordRows.count(), 0);
            assert.equal(
                await page.evaluate(() => globalThis.__pinefetchUi.socialFetches),
                socialFetchesBeforeKeywords
            );
            await closeDetails();

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
            // Authentication is owned by Settings and preserves the saved browser on access failure.
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failBrowserAccess = true;
            });
            await page.locator('#browserAuthBrowser').selectOption('chrome');
            await page.getByRole('button', { name: 'Connect Browser', exact: true }).click();
            await page.waitForFunction(() =>
                globalThis.document
                    .getElementById('browserAuthStatus')
                    .textContent.includes('saved connection has not changed')
            );
            assert.doesNotMatch(await page.locator('#browserAuthStatus').textContent(), /Synthetic private/);
            assert(await page.locator('#disconnectBrowserAuthBtn').isEnabled());
            const beforeImport = await page.evaluate(() => globalThis.__pinefetchUi.commands.length);
            await page.locator('#viewLinkDumpBtn').click();
            assert(await page.locator('#linkDumpServerPanel').isVisible());
            assert.equal(await page.getByRole('heading', { name: 'Browser Authentication', exact: true }).count(), 0);
            await page.locator('#viewSettingsBtn').click();
            assert.deepEqual(
                await page.evaluate(
                    after =>
                        globalThis.__pinefetchUi.commands
                            .slice(after)
                            .filter(call =>
                                ['get_config', 'check_youtube_cookie_access', 'get_link_dump_overview'].includes(
                                    call.command
                                )
                            ),
                    beforeImport
                ),
                []
            );
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failBrowserAccess = false;
            });
            await page.getByRole('button', { name: 'Connect Browser', exact: true }).click();
            await page.waitForFunction(() =>
                globalThis.document.getElementById('browserAuthStatus').textContent.includes('enabled for chrome')
            );
            await page.getByRole('button', { name: 'Disconnect', exact: true }).click();
            await page.waitForFunction(
                () =>
                    globalThis.document.getElementById('browserAuthStatus').textContent ===
                    'Browser authentication is off.'
            );
            assert(await page.locator('#disconnectBrowserAuthBtn').isDisabled());
            await page.screenshot({ path: `/tmp/pinefetch-browser-authentication-${engine.name()}.png` });
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

            const creatorSubtitles = page.locator('#saveCreatorSubtitles');
            assert.equal(await creatorSubtitles.isChecked(), false);
            await creatorSubtitles.focus();
            await page.keyboard.press('Space');
            await page.waitForFunction(() =>
                globalThis.__pinefetchUi.commands.some(
                    call => call.command === 'patch_config' && call.args.changes.save_creator_subtitles === true
                )
            );
            await page.waitForFunction(
                () =>
                    globalThis.document.getElementById('settingsSaveStatus').textContent ===
                    'Changes saved. They apply to new downloads.'
            );
            assert.equal(await creatorSubtitles.isChecked(), true);

            // Playlist paste/import expands into independent jobs without requiring Queue.
            await page.locator('#viewDownloadBtn').click();
            await selectPreset('audio_opus', 'Audio only (opus)', false);
            const playlistCalls = () =>
                page.evaluate(
                    () => globalThis.__pinefetchUi.commands.filter(call => call.command === 'enqueue_playlist').length
                );
            const playlistInput = page.locator('#urlInput');
            const playlistStatus = page.locator('#playlistImportStatus');
            let playlistQueueCount = await page.locator('.pinefetch-queue-item').count();
            const beforePlaylist = await playlistCalls();
            await page.locator('#outputDir').fill('/Users/demo/Downloads/Playlist target');
            await page.evaluate(() => {
                globalThis.__pinefetchUi.holdPlaylist = true;
            });
            const playlistLink = `${videos[0].url}&list=PL_test&t=90`;
            await playlistInput.fill(playlistLink);
            await page.waitForFunction(
                count =>
                    globalThis.__pinefetchUi.commands.filter(call => call.command === 'enqueue_playlist').length ===
                    count,
                beforePlaylist + 1
            );
            assert.match(await playlistStatus.textContent(), /Loading YouTube playlist/);
            assert(await page.locator('#startDownloadBtn').isDisabled());
            await playlistInput.press('Enter');
            await playlistInput.dispatchEvent('input');
            await page.waitForTimeout(750);
            assert.equal(await playlistCalls(), beforePlaylist + 1);
            assert.equal(await page.locator('.pinefetch-queue-item').count(), playlistQueueCount);
            await page.evaluate(() => globalThis.__pinefetchUi.releasePlaylist());
            playlistQueueCount += videos.length;
            await page.waitForFunction(
                count => globalThis.document.querySelectorAll('.pinefetch-queue-item').length === count,
                playlistQueueCount
            );
            assert.match(await playlistStatus.textContent(), /Added 2 playlist videos.*1 unavailable/);
            assert.equal(await playlistInput.inputValue(), '');
            assert(await page.locator('#startDownloadBtn').isEnabled());
            const playlistRequest = await page.evaluate(
                () =>
                    globalThis.__pinefetchUi.commands.filter(call => call.command === 'enqueue_playlist').at(-1).args
                        .request
            );
            assert.equal(playlistRequest.url, 'https://www.youtube.com/playlist?list=PL_test');
            assert.equal(playlistRequest.output_dir, '/Users/demo/Downloads/Playlist target');
            assert.equal(playlistRequest.audio_format, 'opus');
            assert.equal(playlistRequest.cut_start_time, null);
            assert.equal(
                await page.evaluate(() => globalThis.__pinefetchScreenshot.queuedUrls.slice(-2).join('\n')),
                videos.map(video => video.url).join('\n')
            );
            await page.screenshot({ path: `/tmp/pinefetch-playlist-import-${engine.name()}.png` });

            // A late import may add its accepted playlist but cannot clear newer composer input.
            await page.evaluate(() => {
                globalThis.__pinefetchUi.holdPlaylist = true;
            });
            await playlistInput.fill('https://youtube.com/playlist?list=PL_late');
            await page.waitForFunction(() => Boolean(globalThis.__pinefetchUi.releasePlaylist));
            await page.waitForFunction(
                count =>
                    globalThis.__pinefetchUi.commands.filter(call => call.command === 'enqueue_playlist').length ===
                    count,
                beforePlaylist + 2
            );
            await playlistInput.fill(videos[0].url);
            await page.evaluate(() => globalThis.__pinefetchUi.releasePlaylist());
            playlistQueueCount += videos.length;
            await page.waitForFunction(
                count => globalThis.document.querySelectorAll('.pinefetch-queue-item').length === count,
                playlistQueueCount
            );
            assert.equal(await playlistInput.inputValue(), videos[0].url);
            await playlistInput.press('Escape');

            // Failed and empty imports leave the link available for a deliberate retry.
            await page.evaluate(() => {
                globalThis.__pinefetchUi.failNextPlaylist = true;
            });
            await playlistInput.fill('https://youtube.com/playlist?list=PL_failed');
            await page.waitForFunction(() =>
                globalThis.document.querySelector('#playlistImportStatus').textContent.includes('Could not load')
            );
            assert.equal(await page.locator('.pinefetch-queue-item').count(), playlistQueueCount);
            assert.equal(await playlistInput.inputValue(), 'https://youtube.com/playlist?list=PL_failed');
            await page.locator('#startDownloadBtn').click();
            playlistQueueCount += videos.length;
            await page.waitForFunction(
                count => globalThis.document.querySelectorAll('.pinefetch-queue-item').length === count,
                playlistQueueCount
            );
            await page.evaluate(() => {
                globalThis.__pinefetchUi.playlistVideos = [];
            });
            await playlistInput.fill('https://youtube.com/playlist?list=PL_empty');
            await page.waitForFunction(() =>
                globalThis.document.querySelector('#playlistImportStatus').textContent.includes('No available videos')
            );
            assert.equal(await page.locator('.pinefetch-queue-item').count(), playlistQueueCount);
            assert.equal(await playlistInput.inputValue(), 'https://youtube.com/playlist?list=PL_empty');
            await playlistInput.press('Escape');
            await page.evaluate(entries => {
                const ui = globalThis.__pinefetchUi;
                ui.playlistVideos = entries;
                ui.clipboard = 'https://youtube.com/playlist?list=PL_clipboard';
            }, videos);
            await page.evaluate(() => globalThis.dispatchEvent(new globalThis.Event('focus')));
            playlistQueueCount += videos.length;
            await page.waitForFunction(
                count => globalThis.document.querySelectorAll('.pinefetch-queue-item').length === count,
                playlistQueueCount
            );
            assert.equal(await playlistInput.inputValue(), '');
            const afterPlaylists = await playlistCalls();
            await playlistInput.fill(videos[1].url);
            await page.waitForTimeout(750);
            assert.equal(await playlistCalls(), afterPlaylists);
            assert.equal(await page.locator('.pinefetch-queue-item').count(), playlistQueueCount);
            assert.equal(await playlistInput.inputValue(), videos[1].url);

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
                `[test:ui] ${engine.name()}: offline assets, preset restoration/keyboard/persistence, queue requests/progress, playlist auto-import/retry/deduplication/clipboard, history filters/creator/reset, breadcrumb hierarchy/keyboard/missing creator/retained History state, account navigation/pagination/insights/profile video selection, transcription model/save rollback, menus, tabs, full-container details, local keyword counts/filtering/pagination/late replies, OCR frame grouping/native pagination/search/full export/confirmed deletion/late replies, background navigation, confirmations and switches passed.`
            );
        } finally {
            await browser.close();
        }
    }
} finally {
    await new Promise(resolve => server.close(resolve));
}
