// This bridge exists only in the isolated screenshot browser, never in the app.
// Titles and thumbnails are cached YouTube oEmbed data; all other values are demos.
export const installScreenshotBridge = ({ videos, version }) => {
    const listeners = new Map();
    const queue = [];
    let ready = false;
    const config = {
        default_output_dir: '/Users/demo/Downloads/PineFetch',
        selected_preset_key: 'best',
        faster_whisper_model: 'base',
        magic_import_enabled: true,
        cut_at_timestamp_enabled: true,
        save_captions: true,
        save_thumbnails: true,
        notifications_enabled: true,
        download_video_with_transcript: true,
        yt_dlp_path: '',
    };
    const presets = [
        ['best', 'bestvideo+bestaudio/best', false, null, false, false, '_best'],
        ['1080', 'bv*[height<=1080]+ba/b[height<=1080]', false, null, false, false, '__max'],
        ['audio_mp3', 'ba/b', true, 'mp3', false, false, null],
        ['audio_opus', 'ba/b', true, 'opus', false, false, null],
        ['text', 'ba/b', true, 'mp3', true, false, null],
        ['text_timestamps', 'ba/b', true, 'mp3', true, true, '_timestamps'],
    ].map(([key, format, extract_audio, audio_format, transcribe_text, transcribe_timestamps, filename_suffix]) => ({
        key,
        format,
        extract_audio,
        audio_format,
        transcribe_text,
        transcribe_timestamps,
        filename_suffix,
    }));
    const history = videos.map((video, index) => ({
        ...video,
        id: `history-${index + 1}`,
        source: 'youtube',
        platform: 'youtube',
        medium: 'video',
        filename: `${video.title}.mp4`,
        output_path: `${config.default_output_dir}/${video.title}.mp4`,
        completed_at: Date.parse('2026-09-20T09:30:00Z') - index * 3600000,
        upload_date: '20260919',
        duration_seconds: video.duration,
        file_size_bytes: (120 + index * 24) * 1024 * 1024,
        pinefetch_version: version,
    }));
    const queueStatus = { auto_start: false, worker_running: false, paused: false };
    const emit = (name, payload) => {
        for (const handler of listeners.get(name) || []) handler({ payload: structuredClone(payload) });
    };
    globalThis.__pinefetchScreenshot = {
        get ready() {
            return ready;
        },
        get queuedUrls() {
            return queue.map(job => job.url);
        },
    };
    globalThis.__TAURI__ = {
        event: {
            listen: async (name, handler) => {
                if (!listeners.has(name)) listeners.set(name, new Set());
                listeners.get(name).add(handler);
                return () => listeners.get(name).delete(handler);
            },
        },
        core: {
            invoke: async (command, args = {}) => {
                switch (command) {
                    case 'get_download_presets':
                        return structuredClone(presets);
                    case 'get_config':
                        return structuredClone(config);
                    case 'get_queue_status':
                        return structuredClone(queueStatus);
                    case 'initialize_cli':
                        ready = true;
                        return;
                    case 'load_info': {
                        const video = videos.find(video => video.url === args.url);
                        if (!video) throw new Error(`Missing screenshot fixture: ${args.url}`);
                        return structuredClone(video);
                    }
                    case 'enqueue_download': {
                        const id = `screenshot-${queue.length + 1}`;
                        queue.push({ ...args.request, id });
                        emit('queue:update', queue);
                        return id;
                    }
                    case 'cache_last_download_url':
                        config.last_download_url = args.url;
                        return;
                    case 'get_history':
                        return {
                            entries: structuredClone(history.slice(args.offset, args.offset + args.limit)),
                            has_more: false,
                        };
                    case 'get_history_stats':
                        return {
                            video_count: history.length,
                            total_file_size_bytes: history.reduce((sum, entry) => sum + entry.file_size_bytes, 0),
                            total_duration_seconds: history.reduce((sum, entry) => sum + entry.duration_seconds, 0),
                            source_counts: [{ source: 'youtube', count: history.length }],
                        };
                    case 'get_history_details': {
                        const entry = history.find(entry => entry.id === args.id);
                        if (!entry) throw new Error(`Missing history fixture: ${args.id}`);
                        return {
                            entry: structuredClone(entry),
                            output_file_available: true,
                            file_extension: 'mp4',
                            transcript: { language: 'de', transcription_type: 'text', file_available: true },
                            captions: [
                                { media_path: entry.output_path, caption_path: `${entry.output_path}.caption.txt` },
                            ],
                        };
                    }
                    case 'get_link_dump_overview':
                        return {
                            settings: { server_enabled: true, host: '127.0.0.1', port: 2255 },
                            server_status: { status: 'running', url: 'http://127.0.0.1:2255' },
                            secrets: [
                                {
                                    id: 'demo-connection',
                                    name: 'Chrome on MacBook',
                                    status: 'active',
                                    created_at: '2026-09-19 10:00:00',
                                    last_used_at: '2026-09-20 09:30:00',
                                },
                            ],
                        };
                    case 'get_yt_dlp_installed_version':
                        return { version: '2026.09.19', path: '/demo/yt-dlp' };
                    default:
                        throw new Error(`Unsupported screenshot command: ${command}`);
                }
            },
        },
    };
};
