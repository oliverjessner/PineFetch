import { initTabs } from './vendor/oj/index.js';
import { searchContains } from './unicode-search.js';
import { createTagChip } from './tags.js';
import { formatHistorySource, formatLocalDateTime } from './formatters.js';

export const createHistoryTagsView = ({
    root,
    api,
    tags,
    createItem,
    openVideo,
    openAccount,
    toHistory,
    appendLog,
}) => {
    root.innerHTML = `<nav class="oj-breadcrumb" aria-label="Tag breadcrumb"><ol><li><button type="button" class="oj-button oj-button-ghost oj-button-compact" data-tags-history>History</button></li><li><button type="button" class="oj-button oj-button-ghost oj-button-compact" data-tags-overview>Tags</button></li><li aria-current="page" data-tags-crumb hidden></li></ol></nav>
    <div class="oj-cluster"><h2 id="historyTagsTitle" class="oj-heading-2 oj-grow" tabindex="-1">Tags</h2><button type="button" class="oj-button oj-button-ghost" data-tags-refresh>Refresh</button><button type="button" class="oj-button oj-button-primary" data-tags-create>Create Tag</button><button type="button" class="oj-button oj-button-secondary" data-tags-edit hidden>Edit Tag</button><button type="button" class="oj-button oj-button-danger" data-tags-delete hidden>Delete Tag</button></div>
    <div data-tags-description class="oj-text"></div><p data-tags-count class="oj-helper"></p><label class="oj-field"> <span data-tags-search-label>Search Tags</span><input id="historyTagsQuery" class="oj-input" type="search" autocomplete="off"></label>
    <p data-tags-status class="oj-status" role="status"></p><div class="pinefetch-tags-scroll" data-tags-scroll><div class="oj-stack" data-tags-definitions></div><div class="oj-tabs" data-oj-tabs data-tags-tabs hidden><div class="oj-tab-list" role="tablist" aria-label="Tagged archive"><button type="button" class="oj-tab" id="tagContentTab" role="tab" aria-controls="tagContentPanel" aria-selected="true">Content</button><button type="button" class="oj-tab" id="tagAccountsTab" role="tab" aria-controls="tagAccountsPanel">Accounts</button></div><section id="tagContentPanel" role="tabpanel" aria-labelledby="tagContentTab" class="oj-tab-panel"><div class="oj-stack" data-tags-content></div></section><section id="tagAccountsPanel" role="tabpanel" aria-labelledby="tagAccountsTab" class="oj-tab-panel" hidden><p class="oj-helper">Accounts are derived from tagged content. Verified creator IDs are grouped; unverified downloads remain separate.</p><div class="oj-stack" data-tags-accounts></div></section></div><p class="oj-helper" data-tags-empty></p><button type="button" class="oj-button oj-button-ghost" data-tags-more hidden>Load more</button></div>`;
    const el = name => root.querySelector(`[data-tags-${name}]`);
    initTabs(el('tabs'));
    const state = {
        definitions: [],
        id: null,
        tag: null,
        tab: 'content',
        query: '',
        accountQuery: '',
        overviewQuery: '',
        cursor: null,
        limit: 200,
        request: 0,
        loading: false,
        dirty: false,
        scroll: 0,
        busy: false,
    };
    const status = (text = '', error = false) => {
        el('status').textContent = text;
        el('status').classList.toggle('oj-status-error', error);
    };
    const controls = () => {
        el('create').hidden = Boolean(state.id);
        el('edit').hidden = !state.id;
        el('delete').hidden = !state.id;
        el('tabs').hidden = !state.id;
        el('definitions').hidden = Boolean(state.id);
        el('crumb').hidden = !state.id;
        el('search-label').textContent = !state.id
            ? 'Search Tags'
            : state.tab === 'content'
              ? 'Search tagged content'
              : 'Search tagged accounts';
        for (const button of [el('create'), el('edit'), el('delete'), el('refresh')])
            button.disabled =
                state.busy ||
                state.loading ||
                !api.available ||
                ([el('edit'), el('delete')].includes(button) && !state.tag);
        el('more').disabled = state.loading;
    };
    const renderDefinitions = () => {
        const query = el('scroll').scrollTop;
        const items = state.definitions.filter(
            tag => searchContains(tag.name, state.overviewQuery) || searchContains(tag.description, state.overviewQuery)
        );
        const fragment = document.createDocumentFragment();
        for (const tag of items.slice(0, state.limit)) {
            const row = document.createElement('article');
            row.className = 'pinefetch-tag-definition';
            row.dataset.tagId = tag.id;
            const open = document.createElement('button');
            open.type = 'button';
            open.className = 'oj-button oj-button-ghost pinefetch-tag-definition-title';
            open.append(createTagChip(tag));
            open.setAttribute('aria-label', `Open tag ${tag.name}`);
            open.addEventListener('click', () => void openTag(tag.id));
            const description = document.createElement('p');
            description.className = 'oj-text';
            description.textContent = tag.description;
            const count = document.createElement('p');
            count.className = 'oj-helper';
            count.textContent = `${tag.content_count.toLocaleString()} saved items · ${tag.account_count.toLocaleString()} accounts`;
            const edit = document.createElement('button');
            edit.type = 'button';
            edit.className = 'oj-button oj-button-ghost oj-button-compact';
            edit.textContent = 'Edit';
            edit.setAttribute('aria-label', `Edit tag ${tag.name}`);
            edit.addEventListener('click', () => void mutate(() => tags.edit(tag)));
            const remove = document.createElement('button');
            remove.type = 'button';
            remove.className = 'oj-button oj-button-ghost oj-button-compact';
            remove.textContent = 'Delete';
            remove.setAttribute('aria-label', `Delete tag ${tag.name}`);
            remove.addEventListener('click', () => void mutate(() => tags.removeDefinition(tag)));
            row.append(open, description, count, edit, remove);
            fragment.append(row);
        }
        el('definitions').replaceChildren(fragment);
        el('empty').hidden = items.length > 0;
        el('empty').textContent = state.definitions.length
            ? 'No tags match your search.'
            : 'Create a tag to organize saved downloads.';
        el('more').hidden = items.length <= state.limit;
        el('more').textContent = 'Show more tags';
        el('scroll').scrollTop = query;
    };
    const renderHeader = tag => {
        state.tag = tag;
        el('crumb').textContent = tag.name;
        root.querySelector('h2').replaceChildren(createTagChip(tag));
        el('description').textContent = tag.description;
        el('count').textContent =
            `${tag.content_count.toLocaleString()} saved items · ${tag.account_count.toLocaleString()} accounts`;
        root.querySelector('#tagContentTab').textContent = `Content (${tag.content_count.toLocaleString()})`;
        root.querySelector('#tagAccountsTab').textContent = `Accounts (${tag.account_count.toLocaleString()})`;
    };
    const load = async (append = false) => {
        const request = ++state.request;
        const id = state.id;
        state.loading = true;
        state.dirty = false;
        el('empty').hidden = true;
        status('Loading tags…');
        controls();
        const scroll = el('scroll').scrollTop;
        try {
            if (!id) {
                const definitions = await tags.refresh();
                if (request !== state.request) return;
                state.definitions = definitions;
                renderDefinitions();
                status();
                return;
            }
            const tab = state.tab;
            const [tag, page] = await Promise.all([
                append && state.tag?.id === id ? Promise.resolve(state.tag) : api.getTag({ id }),
                tab === 'content'
                    ? api.getTagContent({
                          id,
                          query: state.query,
                          searchField: 'title',
                          limit: 20,
                          ...(append && state.cursor ? { cursor: state.cursor } : { offset: 0 }),
                      })
                    : api.getTagAccounts({
                          id,
                          query: state.accountQuery,
                          limit: 20,
                          ...(append && state.cursor ? { cursor: state.cursor } : {}),
                      }),
            ]);
            if (request !== state.request) return;
            if (tab === 'content') {
                await tags.batch(page.entries.map(entry => entry.id));
                if (request !== state.request) return;
            }
            renderHeader(tag);
            const target = el(tab === 'content' ? 'content' : 'accounts');
            if (!append) target.replaceChildren();
            const existing = new Set([...target.children].map(row => row.dataset.historyId));
            for (const entry of page.entries) {
                const entryId = entry.id || entry.history_entry_id;
                if (existing.has(entryId)) continue;
                if (tab === 'content')
                    target.append(
                        createItem(entry, {
                            tagId: id,
                            reportError: error => {
                                if (state.id === id) status(String(error), true);
                            },
                            open: () => {
                                state.scroll = el('scroll').scrollTop;
                                openVideo(entry.id, state.tag);
                            },
                            account: () => {
                                state.scroll = el('scroll').scrollTop;
                                openAccount(entry.id, state.tag);
                            },
                        })
                    );
                else {
                    const row = document.createElement('article');
                    row.className = 'pinefetch-tag-account';
                    row.dataset.historyId = entryId;
                    const name = document.createElement('button');
                    name.type = 'button';
                    name.className = 'oj-button oj-button-ghost';
                    name.textContent = entry.uploader;
                    name.addEventListener('click', () => {
                        state.scroll = el('scroll').scrollTop;
                        openAccount(entryId, state.tag);
                    });
                    const meta = document.createElement('p');
                    meta.className = 'oj-helper';
                    meta.textContent = `${formatHistorySource(entry.source)} · ${entry.content_count.toLocaleString()} tagged items${entry.identity_verified ? '' : ' · Unverified creator'}${entry.latest_at ? ` · Last saved ${formatLocalDateTime(new Date(entry.latest_at))}` : ''}`;
                    row.append(name, meta);
                    target.append(row);
                }
            }
            state.cursor = page.next_cursor || null;
            el('more').hidden = !page.has_more;
            el('more').textContent = tab === 'content' ? 'Load more content' : 'Load more accounts';
            el('empty').hidden = target.childElementCount > 0;
            el('empty').textContent =
                tab === 'content'
                    ? 'No saved items match this tag and search.'
                    : 'No creator accounts match this tag and search.';
            status();
        } catch (error) {
            appendLog(`[Tags] ${error}`, true);
            if (request === state.request) {
                status(String(error), true);
                if (id && !state.tag) root.querySelector('h2').textContent = 'Tag unavailable';
            }
        } finally {
            if (request === state.request) {
                state.loading = false;
                controls();
                el('scroll').scrollTop = append ? scroll : 0;
            }
        }
    };
    const mutate = async action => {
        const owner = state.id;
        const generation = state.request;
        if (state.busy) return;
        state.busy = true;
        controls();
        try {
            await action();
        } catch (error) {
            if (owner === state.id && generation === state.request) status(String(error), true);
            appendLog(`[Tags] ${error}`, true);
        } finally {
            state.busy = false;
            controls();
        }
    };
    const openOverview = () => {
        state.id = null;
        state.tag = null;
        state.limit = 200;
        state.cursor = null;
        state.query = '';
        state.accountQuery = '';
        root.querySelector('h2').textContent = 'Tags';
        el('description').textContent = 'Organize saved content with your own tags.';
        el('count').textContent = '';
        root.querySelector('input').value = state.overviewQuery;
        controls();
        void load();
    };
    const openTag = async id => {
        el('content').replaceChildren();
        el('accounts').replaceChildren();
        state.tag = null;
        el('crumb').textContent = 'Tag';
        root.querySelector('h2').textContent = 'Loading tag…';
        el('description').textContent = '';
        el('count').textContent = '';
        state.id = id;
        state.tab = 'content';
        state.query = '';
        state.accountQuery = '';
        state.cursor = null;
        root.querySelector('input').value = '';
        const switching = root.querySelector('#tagContentTab').getAttribute('aria-selected') !== 'true';
        root.querySelector('#tagContentTab').click();
        controls();
        if (!switching) await load();
        root.querySelector('h2').focus({ preventScroll: true });
    };
    let timer;
    root.querySelector('input').addEventListener('input', () => {
        clearTimeout(timer);
        if (!state.id) {
            state.overviewQuery = root.querySelector('input').value;
            state.limit = 200;
            renderDefinitions();
            return;
        }
        const value = root.querySelector('input').value;
        if (state.tab === 'content') state.query = value;
        else state.accountQuery = value;
        state.request++;
        timer = setTimeout(() => void load(), 250);
    });
    el('tabs').addEventListener('oj:change', event => {
        if (event.target !== el('tabs')) return;
        clearTimeout(timer);
        state.tab = event.detail.tab.id === 'tagContentTab' ? 'content' : 'accounts';
        root.querySelector('input').value = state.tab === 'content' ? state.query : state.accountQuery;
        state.cursor = null;
        controls();
        void load();
    });
    el('more').addEventListener('click', () => {
        if (state.loading) return;
        if (!state.id) {
            state.limit += 200;
            renderDefinitions();
        } else void load(true);
    });
    el('refresh').addEventListener('click', () => void load());
    el('create').addEventListener('click', () => void mutate(() => tags.edit()));
    el('edit').addEventListener('click', () => void mutate(() => tags.edit(state.tag)));
    el('delete').addEventListener('click', () => void mutate(() => tags.removeDefinition(state.tag)));
    el('history').addEventListener('click', toHistory);
    el('overview').addEventListener('click', openOverview);
    return Object.freeze({
        openOverview,
        openTag,
        resume: () => {
            if (state.dirty) void load();
            else el('scroll').scrollTop = state.scroll;
        },
        deactivate: () => {
            clearTimeout(timer);
            if (state.loading) state.dirty = true;
            state.loading = false;
            state.request++;
        },
        changed: event => {
            if (event.deleted && event.tagId === state.id) {
                state.id = null;
                state.tag = null;
                if (!root.hidden) openOverview();
                else state.dirty = true;
                return;
            }
            state.dirty = true;
            if (!root.hidden) void load();
        },
        remember: () => {
            state.scroll = el('scroll').scrollTop;
        },
        current: () => state.tag,
    });
};
