const formatDuration = seconds => {
    if (!seconds && seconds !== 0) return '-';

    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    const hrs = Math.floor(mins / 60);

    if (hrs > 0) return `${hrs}h ${String(mins % 60).padStart(2, '0')}m`;
    return `${mins}m ${String(secs).padStart(2, '0')}s`;
};

const formatFileSize = bytes => {
    const size = Number(bytes);
    if (!Number.isFinite(size) || size <= 0) return '0 B';

    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    const unitIndex = Math.min(Math.floor(Math.log(size) / Math.log(1024)), units.length - 1);
    const value = size / 1024 ** unitIndex;
    const precision = unitIndex === 0 || value >= 100 ? 0 : value >= 10 ? 1 : 2;
    return `${value.toFixed(precision)} ${units[unitIndex]}`;
};

const formatHistorySource = source => {
    const name = `${source || 'unknown'}`.trim().toLowerCase();
    const known = {
        youtube: 'YouTube',
        tiktok: 'TikTok',
        instagram: 'Instagram',
        facebook: 'Facebook',
        twitch: 'Twitch',
        linkedin: 'LinkedIn',
        reddit: 'Reddit',
        x: 'X',
    };
    return known[name] || (name ? name.charAt(0).toUpperCase() + name.slice(1) : 'Unknown');
};

const formatUploadDate = value => {
    const raw = `${value || ''}`.trim();
    if (!raw) return null;
    if (/^\d{8}$/.test(raw)) return `${raw.slice(0, 4)}-${raw.slice(4, 6)}-${raw.slice(6, 8)}`;
    return raw;
};

const formatLocalDateTime = date => {
    return date.toLocaleString([], {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
    });
};

export { formatDuration, formatFileSize, formatHistorySource, formatUploadDate, formatLocalDateTime };
