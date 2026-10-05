import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { detectPlatform, extractUrlStartTimestamp, isValidHttpUrl, normalizeTxtImportUrl } from '../src/url-utils.js';

const cases = JSON.parse(readFileSync(new URL('./fixtures/url-contracts.json', import.meta.url), 'utf8'));

test('frontend HTTP validation follows the shared download boundary', () => {
    for (const { input, valid } of cases.validation) assert.equal(isValidHttpUrl(input), valid, input);
});

test('frontend platform detection follows the shared host and alias boundaries', () => {
    for (const { input, platform } of cases.platforms) assert.equal(detectPlatform(input), platform, input);
});

test('frontend timestamps follow the shared numeric and query precedence rules', () => {
    for (const { input, seconds } of cases.timestamps) assert.equal(extractUrlStartTimestamp(input), seconds, input);
});

test('TXT import retains its URLs, timestamp keys and supported-platform subset', () => {
    for (const { input, frontend } of cases.imports) assert.deepEqual(normalizeTxtImportUrl(input), frontend, input);
});
