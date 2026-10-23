import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { normalizeVideoInput, videoInputError, isValidHttpUrl, parseTxtImportLinks } from '../src/url-utils.js';

const cases = JSON.parse(readFileSync(new URL('./fixtures/video-inputs.json', import.meta.url), 'utf8'));

test('ID import is disabled by default and leaves existing full URLs unchanged', () => {
    for (const { input } of cases) {
        assert.equal(normalizeVideoInput(input), input.trim());
        assert.equal(normalizeVideoInput(input, false), input.trim());
    }
    assert.equal(normalizeVideoInput(null), '');
    assert.equal(videoInputError(''), '');
});

test('opt-in ID import resolves only the documented platform formats', () => {
    for (const { input, url } of cases) {
        const actual = normalizeVideoInput(input, true);
        if (url) {
            assert.equal(actual, url, input);
            assert.equal(isValidHttpUrl(actual), true, input);
            assert.equal(videoInputError(input, true), '', input);
        } else {
            assert.equal(actual, input.trim(), input);
            assert.equal(isValidHttpUrl(actual), false, input);
            assert.notEqual(videoInputError(input, true), '', input);
        }
    }
});

test('ambiguous IDs require a platform and disabled ID import explains how to enable it', () => {
    assert.match(videoInputError('dQw4w9WgXcQ'), /Enable ID import/);
    assert.match(videoInputError('7421234567890123456', true), /Specify the platform/);
    assert.match(videoInputError('ABC123xyz', true), /Specify the platform/);
    assert.match(videoInputError('youtube:short', true), /exactly 11/);
    assert.match(videoInputError('tiktok:not-a-number', true), /numeric/);
    assert.match(videoInputError('instagram:ab', true), /shortcode/);
});

test('TXT imports deduplicate IDs against equivalent URLs only when ID import is enabled', () => {
    const source = [
        '# A comment',
        'dQw4w9WgXcQ',
        'youtube:dQw4w9WgXcQ',
        'https://youtu.be/dQw4w9WgXcQ',
        'tiktok:7421234567890123456',
        'https://www.tiktok.com/@creator/video/7421234567890123456',
        'instagram:ABC123xyz',
        'https://www.instagram.com/reel/ABC123xyz/',
        '7421234567890123456',
        'ABC123xyz',
    ].join('\n');
    const disabled = parseTxtImportLinks(source);
    assert.equal(disabled.items.length, 3);
    assert.equal(disabled.invalidCount, 6);
    const enabled = parseTxtImportLinks(source, true);
    assert.equal(enabled.items.length, 3);
    assert.equal(enabled.invalidCount, 2);
    assert.equal(enabled.duplicateCount, 4);
    assert.deepEqual(
        enabled.items.map(item => item.url),
        [
            'https://www.youtube.com/watch?v=dQw4w9WgXcQ',
            'https://www.tiktok.com/@_/video/7421234567890123456',
            'https://www.instagram.com/p/ABC123xyz/',
        ]
    );
});
