import { searchMatches } from './unicode-search.js';

// Shared, DOM-free text rules. Source text is retained verbatim for copying.
export const textTimestamp = milliseconds => {
    const ms = Math.max(0, Math.round(Number(milliseconds) || 0));
    return `${[Math.floor(ms / 3600000), Math.floor(ms / 60000) % 60, Math.floor(ms / 1000) % 60].map(part => String(part).padStart(2, '0')).join(':')}.${String(ms % 1000).padStart(3, '0')}`;
};
export const timestampMilliseconds = value => {
    const parts = value.replace(',', '.').split(':').map(Number);
    if (parts.length < 2 || parts.length > 3 || parts.some(part => !Number.isFinite(part) || part < 0)) return null;
    return Math.round(parts.reduce((total, part) => total * 60 + part, 0) * 1000);
};
const clock = '(?:\\d+:)?\\d{2}:\\d{2}(?:[.,]\\d+)?';
const timing = new RegExp(`^\\s*\\[?(${clock})\\s*(?:-->|→|–|-)\\s*(${clock})\\]?\\s*(.*)$`);
const point = new RegExp(`^\\s*\\[(${clock})\\]\\s*(.*)$`);
export const textEntries = text => {
    const lines = String(text || '')
        .replace(/\r\n?/g, '\n')
        .split('\n');
    const rows = [];
    let active = null;
    for (const [index, line] of lines.entries()) {
        const range = line.match(timing);
        const single = range ? null : line.match(point);
        if (range || single) {
            const match = range || single;
            active = {
                id: `text-${rows.length}`,
                time: timestampMilliseconds(match[1]),
                end: range ? timestampMilliseconds(match[2]) : null,
                text: range ? (line.trimStart().startsWith('[') ? match[3] : '') : match[2],
            };
            rows.push(active);
        } else if (active) {
            // Numeric SRT cue identifiers and WebVTT settings are not speech.
            if (
                line.trim() &&
                timing.test(lines[index + 1] || '') &&
                (!lines[index - 1]?.trim() || /^\d+$/.test(line.trim()))
            )
                continue;
            active.text += `${active.text ? '\n' : ''}${line}`;
        }
    }
    if (rows.length) return rows.map(row => ({ ...row, text: row.text.trim() }));
    return String(text || '').trim() ? [{ id: 'text-0', time: null, end: null, text: String(text) }] : [];
};
export const textMatches = searchMatches;
export const entryAtTime = (entries, milliseconds) => {
    let selected = null;
    for (const entry of entries) {
        if (entry.time !== null && entry.time <= milliseconds && (!selected || entry.time >= selected.time))
            selected = entry;
    }
    return selected;
};
