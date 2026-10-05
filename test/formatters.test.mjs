import assert from 'node:assert/strict';
import test from 'node:test';
import { formatDuration, formatFileSize, formatHistorySource, formatUploadDate } from '../src/formatters.js';

test('shared duration labels preserve missing, zero, minute and hour displays', () => {
    for (const [input, expected] of [
        [null, '-'],
        [undefined, '-'],
        [0, '0m 00s'],
        [59.9, '0m 59s'],
        [61, '1m 01s'],
        [3600, '1h 00m'],
        [7261, '2h 01m'],
    ]) {
        assert.equal(formatDuration(input), expected);
    }
});

test('shared file sizes retain binary units and the existing display precision', () => {
    for (const [input, expected] of [
        [null, '0 B'],
        [-1, '0 B'],
        [NaN, '0 B'],
        [Infinity, '0 B'],
        [0, '0 B'],
        [1, '1 B'],
        [1023, '1023 B'],
        [1024, '1.00 KB'],
        [10240, '10.0 KB'],
        [102400, '100 KB'],
        [1024 ** 2, '1.00 MB'],
        [1024 ** 4, '1.00 TB'],
    ]) {
        assert.equal(formatFileSize(input), expected);
    }
});

test('history list and detail source labels preserve canonical and unknown names', () => {
    for (const [input, expected] of [
        [null, 'Unknown'],
        [' ', 'Unknown'],
        [' YOUTUBE ', 'YouTube'],
        ['tiktok', 'TikTok'],
        ['linkedin', 'LinkedIn'],
        ['reddit', 'Reddit'],
        ['x', 'X'],
        ['example', 'Example'],
        ['🌲', '🌲'],
    ]) {
        assert.equal(formatHistorySource(input), expected);
    }
});

test('history list and detail upload dates retain empty and unrecognized values', () => {
    for (const [input, expected] of [
        [null, null],
        ['', null],
        [' 20261005 ', '2026-10-05'],
        [20261005, '2026-10-05'],
        [' 2026-10-05 ', '2026-10-05'],
        ['Grüße 🌲', 'Grüße 🌲'],
    ]) {
        assert.equal(formatUploadDate(input), expected);
    }
});
