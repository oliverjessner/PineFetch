import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createTauriClient } from '../src/tauri-client.js';

const commands = {
    cacheLastDownloadUrl: 'cache_last_download_url',
    cancelDownload: 'cancel_download',
    clearHistory: 'clear_history',
    createLinkDumpSecret: 'create_link_dump_secret',
    deleteLinkDumpSecret: 'delete_link_dump_secret',
    enqueueDownload: 'enqueue_download',
    getConfig: 'get_config',
    getDownloadPresets: 'get_download_presets',
    getHistory: 'get_history',
    getHistoryCaption: 'get_history_caption',
    getHistoryDetails: 'get_history_details',
    getHistoryStats: 'get_history_stats',
    getHistoryTranscript: 'get_history_transcript',
    getLinkDumpOverview: 'get_link_dump_overview',
    getQueue: 'get_queue',
    getQueueStatus: 'get_queue_status',
    getYtDlpInstalledVersion: 'get_yt_dlp_installed_version',
    initializeCli: 'initialize_cli',
    loadInfo: 'load_info',
    openExternalUrl: 'open_external_url',
    openFilePath: 'open_file_path',
    openFolder: 'open_folder',
    patchConfig: 'patch_config',
    pauseQueue: 'pause_queue',
    pickOutputDir: 'pick_output_dir',
    pickTxtFile: 'pick_txt_file',
    readClipboardText: 'read_clipboard_text',
    removeHistoryEntry: 'remove_history_entry',
    restartLinkDumpServer: 'restart_link_dump_server',
    resumeQueue: 'resume_queue',
    revokeLinkDumpSecret: 'revoke_link_dump_secret',
    setQueueAutoStart: 'set_queue_auto_start',
    startQueue: 'start_queue',
    updateLinkDumpSettings: 'update_link_dump_settings',
};

test('every command forwards its existing name, payload and result unchanged', async () => {
    const calls = [];
    const response = { synthetic: true };
    const client = createTauriClient({
        core: {
            invoke: async (command, args) => {
                calls.push({ command, args });
                return response;
            },
        },
    });
    const payload = { id: 'synthetic-job', mediaPath: '/synthetic/Grüße 🌲.mp4', changes: { yt_dlp_path: null } };
    for (const [method, command] of Object.entries(commands)) {
        assert.equal(await client[method](payload), response);
        assert.equal(calls.at(-1).command, command);
        assert.equal(calls.at(-1).args, payload);
    }
    await client.getConfig();
    assert.equal(calls.at(-1).args, undefined);
    assert.equal(client.available, true);
    assert.equal(client.eventsAvailable, false);
});

test('subscriptions preserve names, callbacks and unsubscribe functions', async () => {
    const events = {
        onDownloadLog: 'download:log',
        onDownloadProgress: 'download:progress',
        onDownloadState: 'download:state',
        onHistoryChanged: 'history:changed',
        onLinkDumpServerStatus: 'link-dump:server-status',
        onQueueStatus: 'queue:status',
        onQueueUpdate: 'queue:update',
    };
    const handler = () => {};
    const unsubscribe = () => {};
    const calls = [];
    const client = createTauriClient({
        event: {
            listen: async (...args) => {
                calls.push(args);
                return unsubscribe;
            },
        },
    });
    for (const [method, event] of Object.entries(events)) {
        assert.equal(await client[method](handler), unsubscribe);
        assert.deepEqual(calls.at(-1), [event, handler]);
    }
});

test('backend failures reach callers and missing bridges keep browser fallback available', async () => {
    const error = new Error('synthetic persistence failure');
    const client = createTauriClient({
        core: {
            invoke: async () => {
                throw error;
            },
        },
    });
    await assert.rejects(client.enqueueDownload({ request: {} }), candidate => candidate === error);
    const browser = createTauriClient(undefined);
    assert.equal(browser.available, false);
    assert.equal(browser.eventsAvailable, false);
    assert.equal(browser.readClipboardPlugin(), '');
    const clipboard = createTauriClient({ clipboard: { readText: async () => 'synthetic clipboard' } });
    assert.equal(await clipboard.readClipboardPlugin(), 'synthetic clipboard');
});
