import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createBrowserImportView } from '../src/browser-import-view.js';

test('Browser Import loads and caches its overview without authentication controls or config access', async () => {
    let overviewReads = 0;
    const view = createBrowserImportView({
        els: {},
        api: {
            available: true,
            getConfig: () => {
                assert.fail('Browser Import must not load browser authentication');
            },
            getLinkDumpOverview: async () => {
                overviewReads++;
                return { settings: {}, secrets: [] };
            },
        },
        appendLog: message => assert.fail(message),
    });
    assert.equal(view.hasOverview(), false);
    await view.syncLinkDumpOverview();
    assert.equal(view.hasOverview(), true);
    assert.equal(overviewReads, 1);
});

test('Link Dump overview failure remains retryable without involving authentication', async () => {
    let attempts = 0;
    const errors = [];
    const view = createBrowserImportView({
        els: {},
        api: {
            available: true,
            getLinkDumpOverview: async () => {
                if (!attempts++) throw new Error('Server unavailable');
                return { settings: {}, secrets: [] };
            },
        },
        appendLog: message => errors.push(message),
    });
    await view.syncLinkDumpOverview();
    assert.equal(view.hasOverview(), false);
    assert.match(errors[0], /Server unavailable/);
    await view.syncLinkDumpOverview();
    assert.equal(view.hasOverview(), true);
});
