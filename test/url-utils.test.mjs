import assert from 'node:assert/strict';
import { describe, test } from 'node:test';
import {
    detectPlatform,
    extractUrlStartTimestamp,
    isValidHttpUrl,
    normalizeTxtImportUrl,
    parseTxtImportLinks,
    resolveYouTubeThumbnail,
} from '../src/url-utils.js';

describe('HTTP URL validation and platform boundaries', () => {
    test('accepts HTTP(S) and rejects malformed URLs and other schemes', () => {
        for (const value of ['https://example.com/video', 'http://localhost:2255/path']) {
            assert.equal(isValidHttpUrl(value), true, value);
        }
        for (const value of [
            '',
            null,
            'youtube.com/watch?v=abc',
            '/relative',
            'file:///tmp/video',
            'ftp://example.com',
            'javascript:alert(1)',
        ]) {
            assert.equal(isValidHttpUrl(value), false, String(value));
        }
    });

    test('recognizes supported domains, aliases, subdomains, and a trailing DNS dot', () => {
        for (const [url, expected] of [
            ['https://WWW.YOUTUBE.COM./watch?v=abc123', 'youtube'],
            ['https://youtu.be/abc123', 'youtube'],
            ['https://m.facebook.com/watch', 'facebook'],
            ['https://fb.watch/example', 'facebook'],
            ['https://twitch.tv/channel', 'twitch'],
            ['https://x.com/user/status/123', 'x'],
            ['https://twitter.com/user/status/123', 'x'],
            ['https://redd.it/example', 'reddit'],
            ['https://www.redditmedia.com/example', 'reddit'],
            ['https://vm.tiktok.com/Example', 'tiktok'],
            ['https://instagr.am/p/ABC123', 'instagram'],
        ]) {
            assert.equal(detectPlatform(url), expected, url);
        }
    });

    test('rejects lookalike hosts, URLs whose path contains a domain, and invalid input', () => {
        for (const url of [
            'https://notyoutube.com',
            'https://youtube.com.evil.example',
            'https://example.com/youtube.com',
            'https://tiktok.com.evil.example',
            '',
            'not a URL',
        ]) {
            assert.equal(detectPlatform(url), null, url);
        }
    });
});

describe('start timestamps', () => {
    test('parses numeric, fractional, unit, colon, and fragment timestamps', () => {
        for (const [suffix, expected] of [
            ['?t=90', 90],
            ['?start=1.5', 1.5],
            ['?start_time=1h2m3s', 3723],
            ['?time_continue=1%3A30', 90],
            ['?t=01%3A02%3A03', 3723],
            ['#t=1m30s', 90],
            ['#75', 75],
            ['#1m', 60],
        ]) {
            assert.equal(extractUrlStartTimestamp(`https://example.com/video${suffix}`), expected, suffix);
        }
    });

    test('uses the first valid query timestamp before falling back to the fragment', () => {
        assert.equal(extractUrlStartTimestamp('https://example.com/?t=invalid&start=30#t=60'), 30);
        assert.equal(extractUrlStartTimestamp('https://example.com/?t=0#t=60'), 60);
    });

    test('returns null for zero, negative, malformed, missing, and infinite timestamps', () => {
        for (const suffix of [
            '',
            '?t=',
            '?t=0',
            '?t=-1',
            '?t=Infinity',
            '?t=1mgarbage',
            '?t=1::2',
            '?t=1:2:3:4',
            '?timestamp=30',
            '#t=invalid',
        ]) {
            assert.equal(extractUrlStartTimestamp(`https://example.com/${suffix}`), null, suffix);
        }
        assert.equal(extractUrlStartTimestamp('not a URL'), null);
    });
});

describe('TXT import normalization', () => {
    test('keeps YouTube URLs and timestamps while producing equivalent deduplication keys', () => {
        const first = normalizeTxtImportUrl('  https://youtu.be/abc123?t=1m  ');
        const second = normalizeTxtImportUrl('https://www.youtube.com/watch?v=abc123&start=60');
        assert.deepEqual(first, { url: 'https://youtu.be/abc123?t=1m', key: 'youtube:abc123:60' });
        assert.equal(second.key, first.key);
        assert.equal(normalizeTxtImportUrl('https://youtube.com/shorts/abc123?t=120').key, 'youtube:abc123:120');
        assert.equal(normalizeTxtImportUrl('https://youtube.com/live/abc123').key, 'youtube:abc123:');
    });

    test('normalizes TikTok tracking parameters and supports short links', () => {
        assert.deepEqual(normalizeTxtImportUrl('http://www.tiktok.com/@creator/video/123456789?track=1#fragment'), {
            url: 'https://www.tiktok.com/@creator/video/123456789',
            key: 'tiktok:123456789',
        });
        assert.deepEqual(normalizeTxtImportUrl('https://vm.tiktok.com/Example/?track=1'), {
            url: 'https://vm.tiktok.com/Example/',
            key: 'tiktok-short:Example',
        });
        assert.equal(normalizeTxtImportUrl('https://www.tiktok.com/t/Example/').key, 'tiktok-short:Example');
    });

    test('normalizes Instagram posts, reels, TV routes, and username prefixes', () => {
        for (const route of ['p', 'reel', 'tv']) {
            assert.deepEqual(normalizeTxtImportUrl(`https://instagr.am/creator/${route}/ABC_123/?track=1`), {
                url: `https://www.instagram.com/${route}/ABC_123/`,
                key: 'instagram:ABC_123',
            });
        }
    });

    test('rejects unsupported platforms, missing content IDs, unsafe schemes, and lookalikes', () => {
        for (const url of [
            '',
            null,
            'invalid',
            'ftp://youtube.com/watch?v=abc123',
            'https://youtube.com/watch',
            'https://youtube.com.evil.example/watch?v=abc123',
            'https://tiktok.com/@creator',
            'https://instagram.com/p/a/',
            'https://twitch.tv/channel',
        ]) {
            assert.equal(normalizeTxtImportUrl(url), null, String(url));
        }
    });

    test('handles empty, whitespace-only, comment-only, and invalid-only files', () => {
        for (const input of ['', null, '   ']) {
            assert.deepEqual(parseTxtImportLinks(input), {
                items: [],
                invalidCount: 0,
                duplicateCount: 0,
                ignoredCount: 1,
                isEmpty: true,
            });
        }
        assert.deepEqual(parseTxtImportLinks('# comment\n\n  # another'), {
            items: [],
            invalidCount: 0,
            duplicateCount: 0,
            ignoredCount: 3,
            isEmpty: false,
        });
        assert.deepEqual(parseTxtImportLinks('invalid\nhttps://example.com/video'), {
            items: [],
            invalidCount: 2,
            duplicateCount: 0,
            ignoredCount: 0,
            isEmpty: false,
        });
    });

    test('counts mixed line endings and deduplicates content while retaining distinct YouTube start times', () => {
        const input =
            '# batch\r\n \r\nhttps://youtu.be/abc123?t=60\rhttps://www.youtube.com/watch?v=abc123&start=1m\n' +
            'https://youtube.com/watch?v=abc123&t=120\nhttps://www.tiktok.com/@creator/video/123456789?track=1\n' +
            'http://m.tiktok.com/@other/video/123456789#fragment\nhttps://www.instagram.com/reel/ABC_123/?track=1\n' +
            'https://instagr.am/p/ABC_123/\ninvalid\nhttps://youtube.com.evil.example/watch?v=abc123\nhttps://twitch.tv/name\n';
        const result = parseTxtImportLinks(input);
        assert.deepEqual(
            result.items.map(item => item.key),
            ['youtube:abc123:60', 'youtube:abc123:120', 'tiktok:123456789', 'instagram:ABC_123']
        );
        assert.equal(result.items[0].url, 'https://youtu.be/abc123?t=60');
        assert.equal(result.invalidCount, 3);
        assert.equal(result.duplicateCount, 3);
        assert.equal(result.ignoredCount, 3);
        assert.equal(result.isEmpty, false);
    });
});

test('thumbnail resolution extracts video IDs without accepting lookalike domains', () => {
    for (const url of [
        'https://youtu.be/abc123',
        'https://youtube.com/watch?v=abc123',
        'https://youtube.com/embed/abc123',
    ]) {
        assert.equal(resolveYouTubeThumbnail(url), 'https://i.ytimg.com/vi/abc123/mqdefault.jpg');
    }
    for (const url of ['', 'invalid', 'https://youtube.com/', 'https://youtube.com.evil.example/watch?v=abc123']) {
        assert.equal(resolveYouTubeThumbnail(url), null);
    }
});
