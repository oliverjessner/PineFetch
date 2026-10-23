import { formatLocalDateTime, formatHistorySource } from './formatters.js';
import { toast } from './vendor/oj/index.js';

const labels = {
    views: 'Views',
    likes: 'Likes',
    comments: 'Comments',
    shares: 'Shares',
    saves: 'Saves',
    followers: 'Followers',
    following: 'Following',
    posts: 'Posts',
    subscribers: 'Subscribers',
    score: 'Score',
};
const reasons = {
    unsupported: 'Not supported',
    not_exposed: 'Not exposed publicly',
    login_required: 'Login required',
    blocked: 'Access blocked',
    extraction_error: 'Could not extract',
    identity_mismatch: 'Video identity could not be verified',
};
const statuses = {
    available: 'Available public metrics',
    partial: 'Some public metrics are unavailable.',
    unavailable: 'No public metrics were exposed.',
    unsupported: 'Social Insights are not supported for this platform or URL.',
    login_required: 'This platform requires a signed-in session. Public metrics are unavailable.',
    blocked: 'The platform blocked this request. Try refreshing later.',
    extraction_error: 'The platform data could not be read. Try refreshing later.',
};

export const createHistorySocialInsightsView = ({ els, api, appendLog, profileMetrics = false }) => {
    const pending = new Map();
    const state = { id: null, request: 0, content: undefined, reading: false, error: '' };
    const render = () => {
        const content = state.content;
        const loading = pending.has(state.id);
        els.historySocialLoadBtn.disabled = !api.available || !state.id || state.reading || loading;
        els.historySocialLoadBtn.textContent = loading
            ? 'Loading Social Insights…'
            : content
              ? 'Refresh Social Insights'
              : 'Load Social Insights';
        els.historySocialLoadBtn.setAttribute('aria-busy', String(loading));
        els.historySocialLoadBtn.classList.toggle('oj-button-loading', loading);
        els.historySocialStatus.classList.toggle('oj-status-error', Boolean(state.error));
        els.historySocialResults.setAttribute('aria-busy', String(state.reading || loading));
        els.historySocialStatus.textContent =
            state.error ||
            (state.reading
                ? 'Loading saved insights…'
                : loading
                  ? 'Fetching public metrics…'
                  : content
                    ? statuses[content.data.status] || 'Saved Social Insights'
                    : 'No Social Insights saved. Load them when you need them.');
        els.historySocialMeta.textContent = content
            ? `Fetched ${formatLocalDateTime(new Date(content.fetched_at))} · ${content.data.platform ? formatHistorySource(content.data.platform) : 'Unknown platform'}`
            : '';
        els.historySocialMetrics.replaceChildren();
        if (profileMetrics) {
            els.historySocialUnavailable.hidden = true;
            els.historySocialUnavailableList.replaceChildren();
        }
        if (!content) return;
        const missingReason = name => {
            const reason = content.data.unavailable[name];
            return reason === 'identity_mismatch' && profileMetrics
                ? 'Account identity could not be verified'
                : reasons[reason] || 'Unavailable';
        };
        const entries = Object.entries(labels).filter(
            ([name]) =>
                (name !== 'score' || Number.isSafeInteger(content.data.metrics[name])) &&
                (!profileMetrics ||
                    ['followers', 'following', 'posts', 'subscribers'].includes(name) ||
                    Number.isSafeInteger(content.data.metrics[name]))
        );
        if (profileMetrics)
            entries.sort(
                ([left], [right]) =>
                    Number(Number.isSafeInteger(content.data.metrics[right])) -
                    Number(Number.isSafeInteger(content.data.metrics[left]))
            );
        let missing = 0;
        for (const [name, label] of entries) {
            const count = content.data.metrics[name];
            const available = Number.isSafeInteger(count) && (count >= 0 || name === 'score');
            if (profileMetrics && !available) {
                missing += 1;
                const term = document.createElement('dt');
                term.textContent = label;
                const detail = document.createElement('dd');
                detail.textContent = missingReason(name);
                els.historySocialUnavailableList.append(term, detail);
                continue;
            }
            const card = document.createElement('div');
            card.className = profileMetrics ? 'oj-card pinefetch-social-metric' : 'pinefetch-social-metric';
            const heading = document.createElement('h3');
            heading.className = 'oj-label';
            heading.textContent = label;
            const value = document.createElement('p');
            value.className = 'oj-heading-2 oj-mono';
            value.textContent = available ? count.toLocaleString() : '—';
            const detail = document.createElement('p');
            detail.className = 'oj-helper';
            detail.textContent =
                count === null || count === undefined
                    ? missingReason(name)
                    : content.data.sources[name]?.approximate
                      ? 'Approximate public count'
                      : name === 'score'
                        ? 'Public score'
                        : 'Public count';
            card.append(heading, value, detail);
            els.historySocialMetrics.append(card);
        }
        if (profileMetrics) {
            els.historySocialUnavailable.hidden = missing === 0;
            els.historySocialUnavailableSummary.textContent = `${missing} ${missing === 1 ? 'metric' : 'metrics'} unavailable`;
        }
    };
    const reset = id => {
        state.id = id;
        state.request += 1;
        state.content = undefined;
        state.reading = false;
        state.error = '';
        render();
    };
    const load = async () => {
        if (!state.id || state.reading || state.content !== undefined || !api.available) return;
        const id = state.id;
        const request = ++state.request;
        state.reading = true;
        state.error = '';
        render();
        try {
            const content = await api.getHistorySocialInsights({ id });
            if (id !== state.id || request !== state.request) return;
            state.content = content || null;
        } catch (error) {
            appendLog(`[Social Insights read] ${error}`, true);
            if (id === state.id && request === state.request)
                state.error = 'Saved Social Insights could not be loaded. Reopen this tab to try again.';
        } finally {
            if (id === state.id && request === state.request) {
                state.reading = false;
                render();
            }
        }
    };
    const bindEvents = () => {
        els.historySocialLoadBtn.addEventListener('click', async () => {
            const id = state.id;
            if (!id || els.historySocialLoadBtn.disabled || pending.has(id)) return;
            const request = ++state.request;
            state.error = '';
            const operation = api.loadHistorySocialInsights({ id });
            pending.set(id, operation);
            render();
            try {
                const content = await operation;
                if (id !== state.id || request !== state.request) return;
                state.content = content;
                toast('Social Insights saved.', { type: 'success' });
            } catch (error) {
                appendLog(`[Social Insights fetch] ${error}`, true);
                if (id === state.id && request === state.request) {
                    const message = /runtime|reinstall/i.test(String(error))
                        ? 'Social Insights tools are unavailable. Rebuild or reinstall PineFetch.'
                        : /account identity/i.test(String(error))
                          ? 'The account identity could not be verified.'
                          : /verified account|account URL/i.test(String(error))
                            ? 'The platform did not expose a verified account address for this video.'
                            : /profile browsing is unavailable/i.test(String(error))
                              ? 'Account insights are unavailable for this platform.'
                              : /already loading/i.test(String(error))
                                ? 'This account is already loading. Please wait and try again.'
                                : /platform blocked/i.test(String(error))
                                  ? 'The platform blocked this request. Try again later.'
                                  : /requires sign-in/i.test(String(error))
                                    ? 'The platform requires sign-in to expose these metrics.'
                                    : 'Social Insights could not be loaded. Check your connection and try again.';
                    state.error = `${message}${state.content ? ' Previously saved data is still shown.' : ''}`;
                }
            } finally {
                pending.delete(id);
                if (id === state.id) {
                    // Reopening this entry while its request was pending reads
                    // the committed snapshot, without starting another fetch.
                    if (request !== state.request) {
                        state.request += 1;
                        state.reading = false;
                        state.content = undefined;
                        void load();
                    }
                    render();
                }
            }
        });
    };
    return Object.freeze({ reset, load, bindEvents });
};
