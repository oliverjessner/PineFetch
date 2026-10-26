import { formatHistorySource } from './formatters.js';
import { createHistorySocialInsightsView } from './history-social-insights-view.js';
import { createPlatformIcon } from './platform-icons.js';

export const createHistoryAccountView = ({ els, api, appendLog, openVideo, navigateToHistory, prepareDownload }) => {
    const state = {
        id: null,
        request: 0,
        offset: 0,
        reading: false,
        profile: '',
        openingProfile: false,
        remoteOffset: 0,
        remoteMore: false,
        remoteBusy: false,
        discovery: 'idle',
    };
    const pendingVideos = new Set();
    let rowSequence = 0;
    const socialEls = Object.fromEntries(
        ['LoadBtn', 'Results', 'Status', 'Meta', 'Metrics', 'Unavailable', 'UnavailableSummary', 'UnavailableList'].map(
            key => [`historySocial${key}`, els[`historyAccountSocial${key}`]]
        )
    );
    const status = (element, message = '', error = false) => {
        element.textContent = message;
        element.hidden = !message;
        element.classList.toggle('oj-status-error', error);
    };
    const setProfile = url => {
        state.profile = url || '';
        els.historyAccountOpenBtn.disabled = !state.profile || !api.available || state.openingProfile;
        els.historyAccountOpenBtn.title =
            state.profile || 'Load Social Insights or browse videos to find this profile.';
        els.historyAccountProfileHint.hidden = Boolean(state.profile);
        if (state.profile) els.historyAccountOpenBtn.removeAttribute('aria-describedby');
        else els.historyAccountOpenBtn.setAttribute('aria-describedby', 'historyAccountProfileHint');
    };
    const social = createHistorySocialInsightsView({
        els: socialEls,
        profileMetrics: true,
        appendLog,
        api: {
            available: api.available,
            getHistorySocialInsights: async ({ id }) => (await api.getHistoryAccount({ id, offset: 0 })).insights,
            loadHistorySocialInsights: async ({ id }) => {
                const snapshot = await api.loadHistoryAccountInsights({ id });
                if (state.id === id) setProfile(snapshot.profile_url);
                return snapshot;
            },
        },
    });
    const row = ({ title, filename, type, action, label, activate, url }) => {
        const item = document.createElement('li');
        item.className = 'pinefetch-account-video';
        if (url) item.dataset.url = url;
        const badge = document.createElement('span');
        badge.className = 'oj-badge pinefetch-account-filetype';
        badge.textContent = type;
        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'pinefetch-account-video-button';
        button.setAttribute('aria-label', `${label}: ${title}`);
        button.disabled = !api.available;
        const content = document.createElement('span');
        content.className = 'pinefetch-account-video-content';
        const heading = document.createElement('strong');
        heading.className = 'oj-truncate pinefetch-account-video-title';
        heading.textContent = title;
        heading.title = title;
        content.append(heading);
        if (filename) {
            const file = document.createElement('span');
            file.className = 'oj-caption oj-truncate pinefetch-account-filename';
            file.id = `history-account-file-${++rowSequence}`;
            button.setAttribute('aria-describedby', file.id);
            file.textContent = filename;
            file.title = filename;
            content.append(file);
        }
        const affordance = document.createElement('span');
        affordance.className = 'pinefetch-account-row-action';
        affordance.setAttribute('aria-hidden', 'true');
        affordance.textContent = action;
        const icon = document.createElement('i');
        icon.className = 'fa-solid fa-arrow-right';
        affordance.append(icon);
        button.append(content, affordance);
        button.addEventListener('click', () => activate(button));
        item.append(badge, button);
        return item;
    };
    const savedRow = entry =>
        row({
            title: entry.title || entry.filename || entry.url,
            filename: entry.filename || '',
            type:
                (entry.filename || entry.output_path || '').match(/\.([a-z0-9]{1,8})$/i)?.[1].toUpperCase() ||
                { video: 'VIDEO', audio: 'AUDIO', transcript: 'TEXT' }[entry.medium] ||
                'FILE',
            action: 'Details',
            label: 'Open video details',
            activate: button => openVideo(entry.id, button),
        });
    const loadSaved = async (append = false) => {
        if (!state.id || state.reading || !api.available) return;
        const id = state.id,
            request = state.request;
        state.reading = true;
        els.historyAccountRetryBtn.hidden = true;
        els.historyAccountMoreSavedBtn.disabled = true;
        els.historyAccountMoreSavedBtn.textContent = 'Loading downloads…';
        els.historyAccountSavedList.setAttribute('aria-busy', 'true');
        status(els.historyAccountStatus, append ? 'Loading more saved downloads…' : 'Loading account…');
        try {
            const page = await api.getHistoryAccount({ id, offset: append ? state.offset : 0 });
            if (state.id !== id || state.request !== request) return;
            const total = page.saved.total_count.toLocaleString();
            els.historyAccountTitle.textContent = page.uploader;
            els.historyAccountBreadcrumbCurrent.textContent = page.uploader;
            els.historyAccountBreadcrumbCurrent.title = page.uploader;
            els.historyAccountMeta.textContent = formatHistorySource(page.source);
            const platformIcon = createPlatformIcon(page.source);
            if (platformIcon) els.historyAccountMeta.prepend(platformIcon);
            els.historyAccountSummary.textContent = `${total} saved ${page.saved.total_count === 1 ? 'download' : 'downloads'}`;
            els.historyAccountSavedCount.textContent = total;
            els.historyAccountSavedCount.setAttribute('aria-label', `${total} saved downloads`);
            if (!append) els.historyAccountSavedList.replaceChildren();
            els.historyAccountSavedList.append(...page.saved.entries.map(savedRow));
            state.offset = (append ? state.offset : 0) + page.saved.entries.length;
            els.historyAccountSavedEmpty.hidden = els.historyAccountSavedList.childElementCount > 0;
            els.historyAccountMoreSavedBtn.hidden = !page.saved.has_more;
            if (page.insights?.profile_url) setProfile(page.insights.profile_url);
            status(els.historyAccountStatus);
            if (!append) social.hydrate(id, page.insights);
        } catch (error) {
            appendLog(`[account] ${error}`, true);
            if (state.id === id && state.request === request) {
                status(
                    els.historyAccountStatus,
                    append
                        ? 'More saved downloads could not be loaded. Try again.'
                        : 'The account could not be loaded. Try again or return to History.',
                    true
                );
                els.historyAccountRetryBtn.hidden = append;
            }
        } finally {
            if (state.id === id && state.request === request) {
                state.reading = false;
                els.historyAccountMoreSavedBtn.disabled = false;
                els.historyAccountMoreSavedBtn.textContent = 'Load more saved videos';
                els.historyAccountSavedList.setAttribute('aria-busy', 'false');
            }
        }
    };
    const renderDiscovery = () => {
        const busy = state.remoteBusy || pendingVideos.has(state.id);
        const populated = els.historyAccountRemoteList.childElementCount > 0;
        for (const button of [els.historyAccountVideosBtn, els.historyAccountMoreVideosBtn]) {
            button.disabled = busy || !state.id || !api.available;
            button.setAttribute('aria-busy', String(busy));
        }
        els.historyAccountVideosBtn.classList.toggle('oj-button-loading', busy);
        els.historyAccountVideosBtn.textContent = busy
            ? 'Loading videos…'
            : state.discovery === 'error'
              ? 'Try again'
              : state.discovery === 'idle'
                ? 'Browse videos'
                : 'Refresh videos';
        els.historyAccountMoreVideosBtn.textContent = busy ? 'Loading videos…' : 'Load more videos';
        els.historyAccountMoreVideosBtn.hidden = !state.remoteMore;
        els.historyAccountRemoteList.setAttribute('aria-busy', String(busy));
        els.historyAccountVideosPlaceholder.hidden = populated;
        els.historyAccountVideosHint.hidden = !populated;
        els.historyAccountVideosLoading.hidden = !busy || populated;
        els.historyAccountVideosIcon.className = busy
            ? 'oj-spinner'
            : state.discovery === 'error'
              ? 'fa-solid fa-circle-exclamation'
              : 'fa-solid fa-cloud-arrow-down';
        const messages = busy
            ? ['Loading profile videos…', 'You can keep reviewing saved downloads while the public profile loads.']
            : state.discovery === 'error'
              ? [
                    'Profile videos are unavailable',
                    'Try again, or use Open profile to visit the account. Some platforms require sign-in.',
                ]
              : state.discovery === 'empty'
                ? [
                      'No public videos found',
                      'The profile returned no videos. You can refresh the list or open the profile.',
                  ]
                : [
                      'Discover more from this account',
                      'Browse public videos, then choose one to prepare a download. Your History stays unchanged.',
                  ];
        els.historyAccountVideosStateTitle.textContent = messages[0];
        els.historyAccountVideosStateDescription.textContent = messages[1];
    };
    const loadVideos = async (append = false) => {
        const id = state.id;
        if (!id || pendingVideos.has(id) || !api.available) return;
        const request = state.request;
        pendingVideos.add(id);
        state.remoteBusy = true;
        renderDiscovery();
        status(els.historyAccountVideosStatus, 'Loading profile videos…');
        try {
            const page = await api.loadHistoryAccountVideos({ id, offset: append ? state.remoteOffset : 0 });
            if (id !== state.id || request !== state.request) return;
            setProfile(page.profile_url);
            if (!append) els.historyAccountRemoteList.replaceChildren();
            const existing = new Set([...els.historyAccountRemoteList.children].map(item => item.dataset.url));
            for (const entry of page.entries) {
                if (existing.has(entry.url)) continue;
                existing.add(entry.url);
                els.historyAccountRemoteList.append(
                    row({
                        title: entry.title || entry.url,
                        type: 'WEB',
                        action: 'Download',
                        label: 'Prepare download',
                        url: entry.url,
                        activate: () => prepareDownload(entry.url),
                    })
                );
            }
            state.remoteOffset = page.next_offset;
            state.remoteMore = page.has_more;
            const count = els.historyAccountRemoteList.childElementCount;
            state.discovery = count ? 'loaded' : 'empty';
            status(
                els.historyAccountVideosStatus,
                count
                    ? `${count} public ${count === 1 ? 'video' : 'videos'} loaded.${page.has_more ? ' More are available.' : ''}`
                    : 'No public videos were returned by this profile.'
            );
        } catch (error) {
            appendLog(`[account videos] ${error}`, true);
            if (id === state.id && request === state.request) {
                state.discovery = 'error';
                status(
                    els.historyAccountVideosStatus,
                    `Profile videos could not be loaded.${els.historyAccountRemoteList.childElementCount ? ' Previously loaded videos are still shown.' : ' The platform may require sign-in or may not expose its video list.'}`,
                    true
                );
            }
        } finally {
            pendingVideos.delete(id);
            if (state.id === id) {
                state.remoteBusy = false;
                if (request !== state.request) status(els.historyAccountVideosStatus);
                renderDiscovery();
            }
        }
    };
    const open = id => {
        state.id = id;
        state.request += 1;
        state.reading = false;
        state.openingProfile = false;
        state.remoteBusy = false;
        state.offset = 0;
        state.remoteOffset = 0;
        state.remoteMore = false;
        state.discovery = 'idle';
        setProfile('');
        els.historyAccountOpenBtn.setAttribute('aria-busy', 'false');
        els.historyAccountTitle.textContent = 'Account';
        els.historyAccountBreadcrumbCurrent.textContent = 'Account';
        els.historyAccountBreadcrumbCurrent.removeAttribute('title');
        els.historyAccountMeta.textContent = 'Loading';
        els.historyAccountSummary.textContent = '';
        els.historyAccountSavedCount.textContent = '';
        els.historyAccountSavedList.replaceChildren();
        els.historyAccountRemoteList.replaceChildren();
        els.historyAccountSavedEmpty.hidden = true;
        els.historyAccountMoreSavedBtn.hidden = true;
        els.historyAccountRetryBtn.hidden = true;
        status(els.historyAccountVideosStatus);
        social.reset(id);
        renderDiscovery();
        void loadSaved();
        els.historyAccountTitle.focus({ preventScroll: true });
    };
    const bindEvents = () => {
        social.bindEvents();
        els.historyAccountHistoryBtn.addEventListener('click', navigateToHistory);
        els.historyAccountRetryBtn.addEventListener('click', () => void loadSaved());
        els.historyAccountMoreSavedBtn.addEventListener('click', () => void loadSaved(true));
        els.historyAccountVideosBtn.addEventListener('click', () => void loadVideos());
        els.historyAccountMoreVideosBtn.addEventListener('click', () => void loadVideos(true));
        els.historyAccountOpenBtn.addEventListener('click', async () => {
            if (!state.profile || state.openingProfile) return;
            const request = state.request;
            state.openingProfile = true;
            els.historyAccountOpenBtn.setAttribute('aria-busy', 'true');
            setProfile(state.profile);
            try {
                await api.openExternalUrl({ url: state.profile });
            } catch (error) {
                appendLog(`[account open] ${error}`, true);
                if (request === state.request)
                    status(els.historyAccountStatus, 'The profile could not be opened. Try again.', true);
            } finally {
                if (request === state.request) {
                    state.openingProfile = false;
                    els.historyAccountOpenBtn.setAttribute('aria-busy', 'false');
                    setProfile(state.profile);
                }
            }
        });
    };
    const leave = () => {
        state.id = null;
        state.request += 1;
        state.reading = false;
        social.reset(null);
    };
    return Object.freeze({ open, bindEvents, leave });
};
