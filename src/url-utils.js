const timestampParamNames = new Set(['t', 'start', 'start_time', 'time_continue']);

// ID import is opt-in. Resolve IDs to ordinary platform URLs before metadata,
// queueing or TXT deduplication; never pass extractor pseudo-URLs to yt-dlp.
const normalizeVideoInput = (value, idImportEnabled = false) => {
    const input = `${value || ''}`.trim();
    if (!idImportEnabled) return input;
    if (/^[A-Za-z0-9_-]{11}$/.test(input)) return `https://www.youtube.com/watch?v=${input}`;
    const match = /^(youtube|tiktok|instagram):(.+)$/i.exec(input);
    if (!match) return input;
    const [, platform, id] = match;
    switch (platform.toLowerCase()) {
        case 'youtube':
            return /^[A-Za-z0-9_-]{11}$/.test(id) ? `https://www.youtube.com/watch?v=${id}` : input;
        case 'tiktok':
            // yt-dlp uses @_/video/<id> when no creator handle is known.
            return /^[0-9]{6,25}$/.test(id) ? `https://www.tiktok.com/@_/video/${id}` : input;
        case 'instagram':
            return /^[A-Za-z0-9_-]{3,128}$/.test(id) ? `https://www.instagram.com/p/${id}/` : input;
        default:
            return input;
    }
};

const videoInputError = (value, idImportEnabled = false) => {
    const input = `${value || ''}`.trim();
    if (!input || isValidHttpUrl(normalizeVideoInput(input, idImportEnabled))) return '';
    if (!/^(?:[A-Za-z]+:)?[A-Za-z0-9_-]+$/.test(input)) return 'Enter a valid http:// or https:// video URL.';
    if (!idImportEnabled) return 'Enable ID import in Settings → Download to use video IDs, or enter a full URL.';
    if (/^youtube:/i.test(input))
        return 'A YouTube ID must contain exactly 11 letters, numbers, underscores or hyphens.';
    if (/^tiktok:/i.test(input)) return 'Use tiktok: followed by a numeric video ID (6–25 digits).';
    if (/^instagram:/i.test(input)) return 'Use instagram: followed by a valid post shortcode (3–128 characters).';
    return 'Specify the platform: youtube:<ID>, tiktok:<ID>, or instagram:<shortcode>.';
};

const normalizeYouTubePlaylistUrl = input => {
    if (!isValidHttpUrl(input) || detectPlatform(input) !== 'youtube') return null;
    try {
        const parsed = new URL(input.trim());
        const host = parsed.hostname.replace(/\.$/, '').toLowerCase();
        const youtube = host === 'youtube.com' || host.endsWith('.youtube.com');
        const short = host === 'youtu.be';
        if (parsed.username || parsed.password || (parsed.port && !['80', '443'].includes(parsed.port))) return null;
        if (
            !(youtube && ['/playlist', '/watch'].includes(parsed.pathname)) &&
            !(short && /^\/[^/]+$/.test(parsed.pathname))
        )
            return null;
        const list = parsed.searchParams.get('list');
        if (!list || !/^[A-Za-z0-9_-]+$/.test(list)) return null;
        return `https://www.youtube.com/playlist?list=${list}`;
    } catch {
        return null;
    }
};

const normalizePositiveTimestamp = seconds => {
    const value = Number(seconds);
    return Number.isFinite(value) && value > 0 ? value : null;
};

const parseDecimalNumber = value => {
    if (!/^[+-]?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?$/.test(value)) return null;
    const number = Number(value);
    return Number.isFinite(number) ? number : null;
};

const parseTimestampValue = raw => {
    const value = `${raw || ''}`.trim().toLowerCase();
    if (!value) return null;

    const numeric = parseDecimalNumber(value);
    if (numeric !== null) return normalizePositiveTimestamp(numeric);

    if (value.includes(':')) {
        const parts = value.split(':');
        if (parts.length < 2 || parts.length > 3) return null;

        let total = 0;
        for (const part of parts) {
            if (!part) return null;
            const amount = parseDecimalNumber(part);
            if (amount === null || amount < 0) return null;
            total = total * 60 + amount;
        }
        return normalizePositiveTimestamp(total);
    }

    const matches = [...value.matchAll(/([\d.]+)([hms])/g)];
    if (!matches.length || matches.map(match => match[0]).join('') !== value) return null;

    let total = 0;
    for (const [, rawAmount, unit] of matches) {
        const amount = parseDecimalNumber(rawAmount);
        if (amount === null) return null;
        const multiplier = unit === 'h' ? 3600 : unit === 'm' ? 60 : 1;
        total += amount * multiplier;
    }
    return normalizePositiveTimestamp(total);
};

const extractUrlStartTimestamp = url => {
    try {
        const parsed = new URL(url);
        for (const [name, value] of parsed.searchParams.entries()) {
            if (!timestampParamNames.has(name)) continue;
            const seconds = parseTimestampValue(value);
            if (seconds) return seconds;
        }

        if (parsed.hash) {
            const fragment = parsed.hash.slice(1);
            const fragmentParams = new URLSearchParams(fragment);
            for (const [name, value] of fragmentParams.entries()) {
                if (!timestampParamNames.has(name)) continue;
                const seconds = parseTimestampValue(value);
                if (seconds) return seconds;
            }
            return parseTimestampValue(fragment);
        }
    } catch {
        return null;
    }
    return null;
};

const platformDomains = Object.freeze([
    ['youtube.com', 'youtube'],
    ['facebook.com', 'facebook'],
    ['twitch.tv', 'twitch'],
    ['x.com', 'x'],
    ['twitter.com', 'x'],
    ['reddit.com', 'reddit'],
    ['redditmedia.com', 'reddit'],
    ['tiktok.com', 'tiktok'],
    ['instagram.com', 'instagram'],
    ['instagr.am', 'instagram'],
]);

const platformForHostname = hostname => {
    const host = normalizeHostname(hostname).replace(/^www\./, '');
    for (const [domain, platform] of platformDomains) {
        if (matchesDomain(host, domain)) return platform;
    }
    switch (host) {
        case 'youtu.be':
            return 'youtube';
        case 'fb.watch':
            return 'facebook';
        case 'redd.it':
            return 'reddit';
        default:
            return null;
    }
};

const detectPlatform = url => {
    try {
        return platformForHostname(new URL(url).hostname);
    } catch {
        return null;
    }
};

const isValidHttpUrl = value => {
    try {
        for (const character of `${value}`) {
            const code = character.charCodeAt(0);
            if (code < 32 || (code >= 127 && code <= 159)) return false;
        }
        const parsed = new URL(value);
        return parsed.protocol === 'http:' || parsed.protocol === 'https:';
    } catch {
        return false;
    }
};

const normalizeHostname = hostname => `${hostname || ''}`.replace(/\.$/, '').toLowerCase();
const matchesDomain = (host, domain) => host === domain || host.endsWith(`.${domain}`);

const isYouTubeHostname = hostname => platformForHostname(hostname) === 'youtube';
const isTikTokHostname = hostname => platformForHostname(hostname) === 'tiktok';
const isInstagramHostname = hostname => platformForHostname(hostname) === 'instagram';

const getYouTubeVideoIdFromParsedUrl = parsed => {
    const host = normalizeHostname(parsed.hostname).replace(/^www\./, '');
    const pathParts = parsed.pathname.split('/').filter(Boolean);

    if (host === 'youtu.be') return pathParts[0] || null;
    if (host !== 'youtube.com' && !host.endsWith('.youtube.com')) return null;

    const route = (pathParts[0] || '').toLowerCase();
    if (route === 'watch') return parsed.searchParams.get('v')?.trim() || null;
    if (route === 'shorts' || route === 'embed' || route === 'v' || route === 'live') {
        return pathParts[1] || null;
    }

    return null;
};

const extractYouTubeVideoId = url => {
    try {
        const parsed = new URL(url);
        if (!isYouTubeHostname(parsed.hostname)) return null;
        return getYouTubeVideoIdFromParsedUrl(parsed);
    } catch {
        return null;
    }
};

const resolveYouTubeThumbnail = url => {
    const videoId = extractYouTubeVideoId(url);
    return videoId ? `https://i.ytimg.com/vi/${videoId}/mqdefault.jpg` : null;
};

const getYouTubeImportTimestampKey = url => {
    const seconds = extractUrlStartTimestamp(url);
    return Number.isFinite(Number(seconds)) && Number(seconds) > 0 ? `${Number(seconds)}` : '';
};

const normalizeYouTubeUrl = value => {
    const trimmed = `${value || ''}`.trim();
    if (!trimmed) return null;

    try {
        const parsed = new URL(trimmed);
        if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return null;
        if (!isYouTubeHostname(parsed.hostname)) return null;

        const videoId = getYouTubeVideoIdFromParsedUrl(parsed);
        if (!videoId) return null;

        parsed.hostname = normalizeHostname(parsed.hostname);
        const url = parsed.toString();
        const timestampKey = getYouTubeImportTimestampKey(url);
        return {
            url,
            key: `youtube:${videoId}:${timestampKey}`,
        };
    } catch {
        return null;
    }
};

const normalizeTikTokUrl = value => {
    const trimmed = `${value || ''}`.trim();
    if (!trimmed) return null;

    try {
        const parsed = new URL(trimmed);
        if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return null;
        if (!isTikTokHostname(parsed.hostname)) return null;

        const host = normalizeHostname(parsed.hostname);
        const pathParts = parsed.pathname.split('/').filter(Boolean);
        const contentId = pathParts.map(part => part.replace(/\.html$/i, '')).find(part => /^\d{6,}$/.test(part));
        const shortCode =
            host === 'vm.tiktok.com' || host === 'vt.tiktok.com'
                ? pathParts[0] || null
                : pathParts[0]?.toLowerCase() === 't'
                  ? pathParts[1] || null
                  : null;

        if (!contentId && !shortCode) return null;

        parsed.protocol = 'https:';
        parsed.hostname = host;
        parsed.search = '';
        parsed.hash = '';
        const url = parsed.toString();
        return {
            url,
            key: contentId ? `tiktok:${contentId}` : `tiktok-short:${shortCode}`,
        };
    } catch {
        return null;
    }
};

const normalizeInstagramUrl = value => {
    const trimmed = `${value || ''}`.trim();
    if (!trimmed) return null;

    try {
        const parsed = new URL(trimmed);
        if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return null;
        if (!isInstagramHostname(parsed.hostname)) return null;

        const pathParts = parsed.pathname.split('/').filter(Boolean);
        const routeIndex = ['p', 'reel', 'tv'].includes(pathParts[0]?.toLowerCase()) ? 0 : 1;
        const route = pathParts[routeIndex]?.toLowerCase();
        const contentCode = pathParts[routeIndex + 1];
        if (!['p', 'reel', 'tv'].includes(route) || !/^[a-zA-Z0-9_-]{3,128}$/.test(contentCode || '')) {
            return null;
        }

        return {
            url: `https://www.instagram.com/${route}/${contentCode}/`,
            key: `instagram:${contentCode}`,
        };
    } catch {
        return null;
    }
};

const normalizeTxtImportUrl = value =>
    normalizeYouTubeUrl(value) || normalizeTikTokUrl(value) || normalizeInstagramUrl(value);

const parseTxtImportLinks = (content, idImportEnabled = false) => {
    const rawContent = `${content || ''}`;
    const seenKeys = new Set();
    const result = {
        items: [],
        invalidCount: 0,
        duplicateCount: 0,
        ignoredCount: 0,
        isEmpty: rawContent.trim().length === 0,
    };

    rawContent.split(/\r\n|\n|\r/).forEach(rawLine => {
        const line = rawLine.trim();
        if (!line || line.startsWith('#')) {
            result.ignoredCount += 1;
            return;
        }

        const normalized = normalizeTxtImportUrl(normalizeVideoInput(line, idImportEnabled));
        if (!normalized) {
            result.invalidCount += 1;
            return;
        }

        if (seenKeys.has(normalized.key)) {
            result.duplicateCount += 1;
            return;
        }

        seenKeys.add(normalized.key);
        result.items.push(normalized);
    });

    return result;
};

export {
    detectPlatform,
    extractUrlStartTimestamp,
    isValidHttpUrl,
    normalizeVideoInput,
    normalizeTxtImportUrl,
    normalizeYouTubePlaylistUrl,
    parseTxtImportLinks,
    resolveYouTubeThumbnail,
    videoInputError,
};
