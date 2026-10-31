import { initTabs, initDropdowns } from './vendor/oj/index.js';
import { searchContains } from './unicode-search.js';
import { createTagChip, tagColor } from './tag-ui.js';
import { formatHistorySource, formatLocalDateTime } from './formatters.js';

export const createTagsView = ({ root, api, tags, createItem, openVideo, openAccount, appendLog }) => {
    root.innerHTML = `<nav class="oj-breadcrumb" aria-label="Tag breadcrumb" data-tags-breadcrumb hidden><ol><li><button type="button" class="oj-button oj-button-ghost oj-button-compact" data-tags-overview>Tags</button></li><li aria-current="page" data-tags-crumb></li></ol></nav>
    <div class="oj-cluster"><h2 id="tagsTitle" class="oj-heading-2 oj-grow" tabindex="-1">Tags</h2><button type="button" class="oj-button oj-button-primary" data-tags-create>Create Tag</button><button type="button" class="oj-button oj-button-secondary" data-tags-edit hidden>Edit Tag</button><button type="button" class="oj-button oj-button-danger" data-tags-delete hidden>Delete Tag</button></div>
    <div data-tags-description class="oj-text"></div><p data-tags-count class="oj-helper"></p><label class="oj-field" data-tags-search> <span data-tags-search-label>Search Tags</span><input id="tagsQuery" class="oj-input" type="search" autocomplete="off" placeholder="Search tags..."></label>
    <div class="oj-cluster" data-tags-feedback hidden><p data-tags-status class="oj-status" role="status"></p><button type="button" class="oj-button oj-button-secondary oj-button-compact" data-tags-retry hidden>Retry</button></div><div class="pinefetch-tags-scroll" data-tags-scroll><div class="oj-stack" data-tags-definitions></div><div class="oj-tabs" data-oj-tabs data-tags-tabs hidden><div class="oj-tab-list" role="tablist" aria-label="Tagged archive"><button type="button" class="oj-tab" id="tagContentTab" role="tab" aria-controls="tagContentPanel" aria-selected="true">Content</button><button type="button" class="oj-tab" id="tagAccountsTab" role="tab" aria-controls="tagAccountsPanel">Accounts</button></div><section id="tagContentPanel" role="tabpanel" aria-labelledby="tagContentTab" class="oj-tab-panel"><div class="oj-stack" data-tags-content></div></section><section id="tagAccountsPanel" role="tabpanel" aria-labelledby="tagAccountsTab" class="oj-tab-panel" hidden><p class="oj-helper">Accounts are derived from tagged content. Verified creator IDs are grouped; unverified downloads remain separate.</p><div class="oj-stack" data-tags-accounts></div></section></div><section class="oj-empty-state" data-tags-empty-state hidden><h3 class="oj-heading-3">No tags yet</h3><p class="oj-helper">Create your first tag to organize saved content.</p><button type="button" class="oj-button oj-button-primary" data-tags-empty-create>Create Tag</button></section><section class="oj-empty-state" data-tags-no-matches hidden><h3 class="oj-heading-3">No matching tags</h3><p class="oj-helper">Try another search term.</p><button type="button" class="oj-button oj-button-secondary" data-tags-clear-search>Clear search</button></section><p class="oj-helper" data-tags-empty></p><button type="button" class="oj-button oj-button-ghost" data-tags-more hidden>Load more</button></div>`;
    const el = name => root.querySelector(`[data-tags-${name}]`);
    initTabs(el('tabs'));
    const state = {
        definitions: [],
        initialized: false,
        overviewLoaded: false,
        overviewScroll: 0,
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
    let cleanupMenus = () => {};
    let activeMenu = null;
    const closeMenu = () => {
        if (activeMenu?.getAttribute('aria-expanded') === 'true') activeMenu.click();
        activeMenu = null;
    };
    const status = (text = '', error = false) => {
        el('feedback').hidden = !text;
        el('status').textContent = text;
        el('status').classList.toggle('oj-status-error', error);
        el('retry').hidden = !error;
    };
    const controls = () => {
        el('create').hidden = Boolean(state.id) || (state.overviewLoaded && !state.definitions.length);
        el('edit').hidden = !state.id;
        el('delete').hidden = !state.id;
        el('tabs').hidden = !state.id;
        el('definitions').hidden = Boolean(state.id);
        el('breadcrumb').hidden = !state.id;
        el('count').hidden = !state.id;
        el('no-matches').hidden =
            Boolean(state.id) || !state.definitions.length || Boolean(el('definitions').childElementCount);
        root.querySelector('input').placeholder = !state.id ? 'Search tags...' : 'Search…';
        el('search').hidden = !state.id && !state.definitions.length;
        el('empty-state').hidden = Boolean(state.id) || !state.overviewLoaded || Boolean(state.definitions.length);
        el('search-label').textContent = !state.id
            ? 'Search Tags'
            : state.tab === 'content'
              ? 'Search tagged content'
              : 'Search tagged accounts';
        for (const button of [
            el('create'),
            el('empty-create'),
            el('edit'),
            el('delete'),
            el('retry'),
            ...root.querySelectorAll('[data-oj-dropdown-trigger]'),
        ])
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
            row.className = 'oj-panel oj-panel-compact oj-panel-interactive pinefetch-tag-definition';
            row.dataset.tagId = tag.id;
            const open = document.createElement('button');
            open.type = 'button';
            open.className = 'oj-button oj-button-ghost pinefetch-tag-definition-open';
            open.setAttribute('aria-label', `Open tag ${tag.name}`);
            open.title = [tag.name, tag.description].filter(Boolean).join('\n');
            open.addEventListener('click', () => void openTag(tag.id));
            const dot = document.createElement('span');
            dot.className = 'oj-status-dot pinefetch-tag-definition-dot';
            dot.style.backgroundColor = tagColor(tag.color);
            dot.setAttribute('aria-hidden', 'true');
            const text = document.createElement('div');
            text.className = 'pinefetch-tag-definition-text';
            const name = document.createElement('span');
            name.className = 'oj-truncate pinefetch-tag-definition-name';
            name.textContent = tag.name;
            name.title = tag.name;
            text.append(name);
            if (tag.description) {
                const description = document.createElement('span');
                description.className = 'oj-caption oj-truncate';
                description.textContent = tag.description;
                description.title = tag.description;
                text.append(description);
            }
            const counts = document.createElement('div');
            counts.className = 'oj-caption pinefetch-tag-definition-counts';
            for (const value of [
                `${tag.content_count.toLocaleString()} saved items`,
                `${tag.account_count.toLocaleString()} accounts`,
            ]) {
                const count = document.createElement('span');
                count.textContent = value;
                counts.append(count);
            }
            const actions = document.createElement('div');
            actions.className = 'oj-dropdown pinefetch-tag-definition-actions';
            actions.setAttribute('data-oj-dropdown', '');
            actions.innerHTML = `<button type="button" class="oj-button oj-button-ghost oj-button-icon" data-oj-dropdown-trigger><span aria-hidden="true">⋯</span></button><div class="oj-menu" data-oj-dropdown-menu role="menu" hidden><button type="button" class="oj-menu-item" role="menuitem" data-oj-value="edit">Edit Tag</button><button type="button" class="oj-menu-item oj-menu-item-danger" role="menuitem" data-oj-value="delete">Delete Tag</button></div>`;
            const trigger = actions.querySelector('button');
            trigger.setAttribute('aria-label', `Actions for tag ${tag.name}`);
            actions.querySelector('[role=menu]').setAttribute('aria-label', `Actions for tag ${tag.name}`);
            const closeOtherMenu = () => {
                if (activeMenu !== trigger) closeMenu();
            };
            trigger.addEventListener('click', closeOtherMenu, { capture: true });
            trigger.addEventListener(
                'keydown',
                event => {
                    if (['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(event.key)) closeOtherMenu();
                },
                { capture: true }
            );
            actions.addEventListener('oj:open', () => {
                activeMenu = trigger;
            });
            actions.addEventListener('oj:close', () => {
                if (activeMenu === trigger) activeMenu = null;
            });
            actions.addEventListener('oj:select', event => {
                const action = event.detail.value === 'edit' ? () => tags.edit(tag) : () => tags.removeDefinition(tag);
                // OJ closes the menu and restores its trigger before a dialog opens.
                queueMicrotask(() => void mutate(action, trigger));
            });
            const chevron = document.createElement('span');
            chevron.textContent = '›';
            chevron.setAttribute('aria-hidden', 'true');
            chevron.className = 'pinefetch-tag-navigation-mark';
            row.append(open, dot, text, counts, actions, chevron);
            fragment.append(row);
        }
        closeMenu();
        cleanupMenus();
        el('definitions').replaceChildren(fragment);
        cleanupMenus = initDropdowns(el('definitions'));
        controls();
        el('no-matches').hidden = items.length > 0 || !state.definitions.length;
        el('empty').hidden = true;
        el('more').hidden = items.length <= state.limit;
        el('more').textContent = 'Show more tags';
        el('scroll').scrollTop = query;
    };
    const renderHeader = tag => {
        state.tag = tag;
        el('crumb').textContent = tag.name;
        root.querySelector('h2').replaceChildren(createTagChip(tag));
        el('description').textContent = tag.description;
        el('description').hidden = !tag.description;
        el('count').textContent =
            `${tag.content_count.toLocaleString()} saved items · ${tag.account_count.toLocaleString()} accounts`;
        root.querySelector('#tagContentTab').textContent = `Content (${tag.content_count.toLocaleString()})`;
        root.querySelector('#tagAccountsTab').textContent = `Accounts (${tag.account_count.toLocaleString()})`;
    };
    const load = async (append = false, restoreScroll = null) => {
        closeMenu();
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
                state.overviewLoaded = true;
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
                el('scroll').scrollTop = restoreScroll ?? (append ? scroll : 0);
            }
        }
    };
    const mutate = async (action, opener = document.activeElement) => {
        const openerTag = opener?.closest('[data-tag-id]')?.dataset.tagId;
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
            const focused = document.activeElement;
            if (
                !root.hidden &&
                !document.querySelector('dialog:modal') &&
                (!focused || !root.contains(focused) || !focused.getClientRects().length)
            ) {
                const target =
                    opener?.isConnected && !opener.disabled && opener.getClientRects().length
                        ? opener
                        : root.querySelector(
                              `[data-tag-id="${CSS.escape(openerTag || '')}"] [data-oj-dropdown-trigger]`
                          ) || root.querySelector('h2');
                target.focus({ preventScroll: true });
            }
        }
    };
    const openOverview = () => {
        closeMenu();
        state.initialized = true;
        if (!state.id && !root.hidden) state.overviewScroll = el('scroll').scrollTop;
        state.id = null;
        state.tag = null;
        state.cursor = null;
        state.query = '';
        state.accountQuery = '';
        root.querySelector('h2').textContent = 'Tags';
        el('description').textContent = 'Organize and explore your saved content.';
        el('description').hidden = false;
        el('count').textContent = '';
        root.querySelector('input').value = state.overviewQuery;
        controls();
        void load(false, state.overviewScroll);
    };
    const openTag = async id => {
        closeMenu();
        if (!state.id && !root.hidden) state.overviewScroll = el('scroll').scrollTop;
        state.initialized = true;
        el('content').replaceChildren();
        el('accounts').replaceChildren();
        state.tag = null;
        el('crumb').textContent = 'Tag';
        root.querySelector('h2').textContent = 'Loading tag…';
        el('description').textContent = '';
        el('description').hidden = true;
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
        if (state.id === id && !root.hidden) root.querySelector('h2').focus({ preventScroll: true });
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
    el('retry').addEventListener('click', () => void load());
    el('clear-search').addEventListener('click', () => {
        root.querySelector('input').value = '';
        root.querySelector('input').dispatchEvent(new Event('input'));
        root.querySelector('input').focus();
    });
    el('create').addEventListener('click', event => void mutate(() => tags.edit(), event.currentTarget));
    el('empty-create').addEventListener('click', event => void mutate(() => tags.edit(), event.currentTarget));
    el('edit').addEventListener('click', event => void mutate(() => tags.edit(state.tag), event.currentTarget));
    el('delete').addEventListener(
        'click',
        event => void mutate(() => tags.removeDefinition(state.tag), event.currentTarget)
    );
    el('overview').addEventListener('click', openOverview);
    return Object.freeze({
        openOverview,
        openTag,
        resume: () => {
            if (!state.initialized) openOverview();
            else if (state.dirty) void load(false, state.scroll);
            else el('scroll').scrollTop = state.scroll;
        },
        deactivate: () => {
            closeMenu();
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
                else {
                    state.initialized = false;
                    state.dirty = true;
                }
                return;
            }
            state.dirty = true;
            if (!root.hidden) void load();
        },
        remember: () => {
            state.scroll = el('scroll').scrollTop;
            if (!state.id) state.overviewScroll = state.scroll;
        },
        current: () => state.tag,
    });
};
