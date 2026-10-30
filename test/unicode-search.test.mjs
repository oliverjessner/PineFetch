import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { searchKey, searchContains, searchMatches, trimSearchQuery } from '../src/unicode-search.js';
import { caseFoldings } from '../src/unicode-folding-data.js';
import { parseCaseFoldings, renderCaseFoldings, sourceFile, outputFile } from '../scripts/generate-search-folding.mjs';

const cases = JSON.parse(await readFile(new URL('./fixtures/unicode-search.json', import.meta.url)));

test('frontend uses the same canonical caseless search cases as SQLite', () => {
    for (const item of cases) {
        assert.equal(searchContains(item.text, item.query), item.match, JSON.stringify(item));
        if (trimSearchQuery(item.query))
            assert.equal(searchMatches(item.text, item.query).length > 0, item.match, JSON.stringify(item));
    }
    assert.equal(searchKey('Straße ÖSTERREICH'), 'strasse österreich');
    assert.equal(searchKey(searchKey('ᾲ Straße')), searchKey('ᾲ Straße'));
});

test('full folding conforms to every bundled Unicode mapping and generated data stays synchronized', async () => {
    const source = await readFile(sourceFile, 'utf8');
    assert.deepEqual(caseFoldings, parseCaseFoldings(source));
    assert.equal(await readFile(outputFile, 'utf8'), await renderCaseFoldings(source));
    for (const [character, folded] of Object.entries(caseFoldings)) {
        assert.equal(searchKey(character), searchKey(folded), character);
        assert.equal(searchKey(searchKey(character)), searchKey(character), character);
    }
});

test('highlights map expansions, combining accents and astral characters back to the original text', () => {
    for (const [text, query, slices] of [
        ['🌲 Straße Österreich', 'STRASSE', ['Straße']],
        ['🌲 Straße Österreich', 'ÖSTERREICH', ['Österreich']],
        ['Café CAFÉ', 'café', ['Café', 'CAFÉ']],
        ['Straße', 's', ['S', 'ß']],
        ['𐐀 🌲 𐐀', '𐐨', ['𐐀', '𐐀']],
        ['à̕', 'à̕', ['à̕']],
        ['a.* a.*', '.*', ['.*', '.*']],
        ['👨‍👩‍👧‍👦', '👧', ['👨‍👩‍👧‍👦']],
    ]) {
        const original = text;
        const matches = searchMatches(text, query);
        assert.deepEqual(
            matches.map(({ start, end }) => text.slice(start, end)),
            slices,
            JSON.stringify({ text, query })
        );
        assert(matches.every((match, index) => index === 0 || matches[index - 1].end <= match.start));
        assert.equal(text, original);
    }
    assert.deepEqual(searchMatches('anything', '\u0085\uFEFF　'), []);
});
