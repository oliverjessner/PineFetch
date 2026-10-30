import { textMatches } from './history-text.js';

// Keep ordinary source rows intact. Large untimed documents get bounded display
// sections; offsets refer to the unchanged original for cross-section matches.
export const textSections = text => {
    if (text.length <= 8192) return [{ id: 'text-0', time: null, text }];
    const result = [];
    let start = 0;
    let end = 0;
    const append = () => {
        result.push({
            id: `text-${result.length}`,
            time: null,
            text: text.slice(start, end),
            sourceStart: start,
            sourceEnd: end,
        });
        start = end;
    };
    if (/^\p{ASCII}*$/u.test(text)) {
        while (end < text.length) {
            end = Math.min(start + 1000, text.length);
            if (text[end - 1] === '\r' && text[end] === '\n') end--;
            append();
        }
    } else {
        for (const segment of new Intl.Segmenter('und', { granularity: 'grapheme' }).segment(text)) {
            if (end - start >= 1000) append();
            end = segment.index + segment.segment.length;
        }
        if (end > start) append();
    }
    return result;
};
export const sectionMatches = (row, matches) => {
    let low = 0;
    let high = matches.length;
    while (low < high) {
        const middle = (low + high) >>> 1;
        if (matches[middle].end <= row.sourceStart) low = middle + 1;
        else high = middle;
    }
    const ranges = [];
    for (let index = low; index < matches.length && matches[index].start < row.sourceEnd; index++) {
        ranges.push({
            start: Math.max(matches[index].start, row.sourceStart) - row.sourceStart,
            end: Math.min(matches[index].end, row.sourceEnd) - row.sourceStart,
        });
    }
    return ranges;
};
export const documentMatches = textMatches;
