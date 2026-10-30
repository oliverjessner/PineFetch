import assert from 'node:assert/strict';
import { test } from 'node:test';
import { textEntries, textTimestamp, timestampMilliseconds, textMatches, entryAtTime } from '../src/history-text.js';

test('reads real Whisper ranges, SRT and WebVTT without changing the source text', () => {
    const whisper = '[00:00:01.250 → 00:00:02.000] Grüße 🌲\n[00:00:03.000 → 00:00:04.000] Second';
    const expected = [
        { id: 'text-0', time: 1250, end: 2000, text: 'Grüße 🌲' },
        { id: 'text-1', time: 3000, end: 4000, text: 'Second' },
    ];
    assert.deepEqual(textEntries(whisper), expected);
    const srt = '1\r\n00:00:01,250 --> 00:00:02,000\r\nGrüße 🌲\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000\r\nSecond';
    assert.deepEqual(textEntries(srt), expected);
    assert.deepEqual(
        textEntries('WEBVTT\n\n00:01.250 --> 00:02.000\nGrüße 🌲\n\n00:03.000 --> 00:04.000\nSecond'),
        expected
    );
    assert.equal(whisper, '[00:00:01.250 → 00:00:02.000] Grüße 🌲\n[00:00:03.000 → 00:00:04.000] Second');
});
test('keeps untimed transcripts and unsupported subtitle formats readable without inventing timecodes', () => {
    for (const source of [
        'Plain text\nSecond line',
        '<tt><body>Original TTML text</body></tt>',
        'Dialogue: Original ASS text',
    ]) {
        assert.deepEqual(textEntries(source), [{ id: 'text-0', time: null, end: null, text: source }]);
    }
    assert.deepEqual(textEntries(''), []);
    assert.deepEqual(textEntries('[00:00:05.230]\nFrame text'), [
        { id: 'text-0', time: 5230, end: null, text: 'Frame text' },
    ]);
});
test('uses precise shared timestamps and finds the preceding playable entry', () => {
    assert.equal(timestampMilliseconds('01:02:03,456'), 3723456);
    assert.equal(timestampMilliseconds('02:03.456'), 123456);
    assert.equal(timestampMilliseconds('broken'), null);
    assert.equal(textTimestamp(3723456), '01:02:03.456');
    const entries = textEntries('[00:00:01]\nFirst\n[00:00:03]\nSecond');
    assert.equal(entryAtTime(entries, 0), null);
    assert.equal(entryAtTime(entries, 1250).text, 'First');
    assert.equal(entryAtTime(entries, 3000).text, 'Second');
});
test('search treats regex characters literally and returns Unicode-safe highlighting ranges', () => {
    assert.deepEqual(textMatches('Grüße 🌲 grüße', 'GRÜẞE'), [
        { start: 0, end: 5 },
        { start: 9, end: 14 },
    ]);
    assert.deepEqual(textMatches('a.* a.*', '.*'), [
        { start: 1, end: 3 },
        { start: 5, end: 7 },
    ]);
    assert.deepEqual(textMatches('anything', '  '), []);
});

test('large untimed sections preserve Unicode, CRLF and matches across display boundaries', async () => {
    const { textSections, sectionMatches } = await import('../src/large-text.js');
    for (const source of ['a'.repeat(999) + '\r\n' + 'b'.repeat(10000), '🌲O\u0308'.repeat(4000)]) {
        const sections = textSections(source);
        assert.equal(sections.map(section => section.text).join(''), source);
        const boundaries = new Set(
            [...new Intl.Segmenter('und', { granularity: 'grapheme' }).segment(source)].map(item => item.index)
        );
        for (const section of sections) assert.ok(boundaries.has(section.sourceStart));
    }
    const source = 'a'.repeat(998) + 'Straße' + 'b'.repeat(10000);
    const sections = textSections(source);
    const matches = textMatches(source, 'STRASSE');
    const highlights = sections.flatMap(section =>
        sectionMatches(section, matches).map(range => section.text.slice(range.start, range.end))
    );
    assert.equal(highlights.join(''), 'Straße');
});
