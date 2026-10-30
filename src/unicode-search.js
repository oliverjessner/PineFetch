import { caseFoldings } from './unicode-folding-data.js';

const foldingPattern = new RegExp(
    `[${Object.keys(caseFoldings)
        .map(character => `\\u{${character.codePointAt(0).toString(16)}}`)
        .join('')}]`,
    'gu'
);
const ascii = /^\p{ASCII}*$/u;
const graphemes = new Intl.Segmenter('und', { granularity: 'grapheme' });

// Locale-independent canonical caseless matching. Accents remain significant.
export const searchKey = value => {
    const text = String(value ?? '');
    if (ascii.test(text)) return text.toLowerCase();
    return text
        .normalize('NFD')
        .replace(foldingPattern, character => caseFoldings[character])
        .normalize('NFC');
};
export const trimSearchQuery = query =>
    String(query ?? '').replace(/^[\p{White_Space}\uFEFF]+|[\p{White_Space}\uFEFF]+$/gu, '');
export const searchContains = (text, query) =>
    text != null && searchKey(text).includes(searchKey(trimSearchQuery(query)));

// Return UTF-16 offsets into the unchanged source, even when folding expands ß
// or normalization combines a letter and accents. Never split a grapheme.
export const searchMatches = (value, query) => {
    const text = String(value ?? '');
    const needle = searchKey(trimSearchQuery(query));
    if (!needle) return [];
    const folded = searchKey(text);
    const found = [];
    for (let start = folded.indexOf(needle); start !== -1; start = folded.indexOf(needle, start + needle.length)) {
        found.push({ start, end: start + needle.length });
    }
    if (!found.length || ascii.test(text)) return found;
    const matches = [];
    let index = 0;
    let offset = 0;
    let sourceStart = null;
    for (const { segment, index: sourceIndex } of graphemes.segment(text)) {
        const end = offset + (ascii.test(segment) ? segment.length : searchKey(segment).length);
        while (index < found.length && found[index].start < end) {
            sourceStart ??= sourceIndex;
            if (found[index].end > end) break;
            const match = { start: sourceStart, end: sourceIndex + segment.length };
            const previous = matches.at(-1);
            if (previous && previous.end > match.start) previous.end = Math.max(previous.end, match.end);
            else matches.push(match);
            sourceStart = null;
            index++;
        }
        if (index === found.length) break;
        offset = end;
    }
    return matches;
};
