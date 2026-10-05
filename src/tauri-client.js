// The only module that knows Tauri command/event names and the global bridge.
// The factory also accepts a fake bridge for contract tests without a desktop.
export const createTauriClient = tauri => {
    const invoke = tauri?.core?.invoke;
    const listen = tauri?.event?.listen;
    return Object.freeze({
        available: Boolean(invoke),
        eventsAvailable: Boolean(listen),
        readClipboardPlugin: () => (typeof tauri?.clipboard?.readText === 'function' ? tauri.clipboard.readText() : ''),
        cacheLastDownloadUrl: args => invoke('cache_last_download_url', args),
        cancelDownload: args => invoke('cancel_download', args),
        clearHistory: args => invoke('clear_history', args),
        createLinkDumpSecret: args => invoke('create_link_dump_secret', args),
        deleteLinkDumpSecret: args => invoke('delete_link_dump_secret', args),
        enqueueDownload: args => invoke('enqueue_download', args),
        getConfig: args => invoke('get_config', args),
        getDownloadPresets: args => invoke('get_download_presets', args),
        getHistory: args => invoke('get_history', args),
        getHistoryCaption: args => invoke('get_history_caption', args),
        getHistoryDetails: args => invoke('get_history_details', args),
        getHistoryStats: args => invoke('get_history_stats', args),
        getHistoryTranscript: args => invoke('get_history_transcript', args),
        getLinkDumpOverview: args => invoke('get_link_dump_overview', args),
        getQueue: args => invoke('get_queue', args),
        getQueueStatus: args => invoke('get_queue_status', args),
        getYtDlpInstalledVersion: args => invoke('get_yt_dlp_installed_version', args),
        initializeCli: args => invoke('initialize_cli', args),
        loadInfo: args => invoke('load_info', args),
        openExternalUrl: args => invoke('open_external_url', args),
        openFilePath: args => invoke('open_file_path', args),
        openFolder: args => invoke('open_folder', args),
        patchConfig: args => invoke('patch_config', args),
        pauseQueue: args => invoke('pause_queue', args),
        pickOutputDir: args => invoke('pick_output_dir', args),
        pickTxtFile: args => invoke('pick_txt_file', args),
        readClipboardText: args => invoke('read_clipboard_text', args),
        removeHistoryEntry: args => invoke('remove_history_entry', args),
        restartLinkDumpServer: args => invoke('restart_link_dump_server', args),
        resumeQueue: args => invoke('resume_queue', args),
        revokeLinkDumpSecret: args => invoke('revoke_link_dump_secret', args),
        setQueueAutoStart: args => invoke('set_queue_auto_start', args),
        startQueue: args => invoke('start_queue', args),
        updateLinkDumpSettings: args => invoke('update_link_dump_settings', args),
        onDownloadLog: handler => listen('download:log', handler),
        onDownloadProgress: handler => listen('download:progress', handler),
        onDownloadState: handler => listen('download:state', handler),
        onHistoryChanged: handler => listen('history:changed', handler),
        onLinkDumpServerStatus: handler => listen('link-dump:server-status', handler),
        onQueueStatus: handler => listen('queue:status', handler),
        onQueueUpdate: handler => listen('queue:update', handler),
    });
};

export const api = createTauriClient(globalThis.window?.__TAURI__);
