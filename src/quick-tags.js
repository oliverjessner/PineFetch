import { createTagChip, tagPalette } from './tag-ui.js';
import { searchContains } from './unicode-search.js';
import { toast } from './vendor/oj/index.js';

// One anchored surface, with no modal, backdrop or focus trap. Its native
// popover layer escapes panel clipping, including inside an existing dialog.
export const createQuickTags = ({ api, choices, assign, onCreated, appendLog }) => {
    const panel = document.createElement('div');
    panel.id = 'historyTagsPopover';
    panel.className = 'pinefetch-quick-tags oj-stack';
    panel.tabIndex = -1;
    panel.hidden = true;
    panel.setAttribute('popover', 'manual');
    panel.setAttribute('role', 'dialog');
    panel.setAttribute('aria-modal', 'false');
    panel.setAttribute('aria-labelledby', 'quickTagsTitle');
    panel.innerHTML = `<section class="oj-stack" data-quick-select><div class="oj-cluster"><h3 id="quickTagsTitle" class="oj-heading-3 oj-grow">Tags</h3><button type="button" class="oj-button oj-button-ghost oj-button-compact" data-quick-close aria-label="Close tag selector">×</button></div><label class="oj-field"><span class="oj-sr-only">Search tags</span><input id="historyTagsSearch" type="search" class="oj-input" autocomplete="off" placeholder="Search tags…" aria-controls="quickTagsOptions"></label><div id="quickTagsOptions" class="oj-stack" role="group" aria-label="Tags for this item" data-quick-options></div><button type="button" class="oj-button oj-button-ghost" data-quick-create>+ Create new tag</button></section>
    <form class="oj-stack" data-quick-form hidden novalidate><button type="button" class="oj-button oj-button-ghost oj-button-compact" data-quick-back>← Back to tags</button><h3 id="quickCreateTitle" class="oj-heading-3">Create Tag</h3><label class="oj-field">Name<input id="quickTagName" class="oj-input" required autocomplete="off"></label><fieldset><legend class="oj-label">Color</legend><div class="oj-cluster" data-quick-palette></div></fieldset><div data-quick-preview aria-label="Tag preview"></div><div class="oj-cluster"><button type="button" class="oj-button oj-button-ghost" data-quick-cancel>Cancel</button><button type="submit" class="oj-button oj-button-primary" data-quick-submit>Create & Add</button></div></form><p class="oj-status" role="status" data-quick-status></p>`;
    document.body.append(panel);
    const el = name => panel.querySelector(`[data-quick-${name}]`);
    const search = panel.querySelector('#historyTagsSearch');
    const name = panel.querySelector('#quickTagName');
    const state = {
        open: false,
        card: null,
        details: null,
        id: null,
        trigger: null,
        epoch: 0,
        read: 0,
        definitions: [],
        assigned: [],
        loading: false,
        busy: false,
        mode: 'select',
        color: tagPalette[0],
        created: null,
        assignedOnly: false,
    };
    const creations = new Map();
    const partials = new Map();
    let savedAttributes = [];
    const restoreTrigger = () => {
        for (const [attribute, value] of savedAttributes)
            if (value === null) state.trigger?.removeAttribute(attribute);
            else state.trigger?.setAttribute(attribute, value);
    };
    const setTrigger = trigger => {
        state.trigger = trigger;
        savedAttributes = ['aria-controls', 'aria-expanded', 'aria-haspopup'].map(attribute => [
            attribute,
            trigger.getAttribute(attribute),
        ]);
        trigger.setAttribute('aria-controls', panel.id);
        trigger.setAttribute('aria-expanded', 'true');
        trigger.setAttribute('aria-haspopup', 'dialog');
    };
    const current = (epoch, id) => state.open && state.epoch === epoch && state.id === id;
    const status = (message = '', error = false) => {
        el('status').textContent = message;
        el('status').classList.toggle('oj-status-error', error);
    };
    const position = () => {
        if (!state.open) return;
        if (!state.trigger?.isConnected || !state.trigger.getClientRects().length) {
            const replacement =
                state.details?.querySelector('[data-detail-tags-add]') ||
                state.card?.querySelector('.pinefetch-history-open-btn');
            if (replacement?.isConnected && replacement.getClientRects().length) {
                restoreTrigger();
                setTrigger(replacement);
            }
        }
        const trigger = state.trigger;
        if (!trigger?.isConnected || !trigger.getClientRects().length) {
            close();
            return;
        }
        const box = trigger.getBoundingClientRect();
        const width = Math.min(320, Math.max(0, innerWidth - 16));
        panel.style.width = `${width}px`;
        const below = innerHeight - box.bottom - 14;
        const above = box.top - 14;
        const down = below >= Math.min(330, above);
        const available = Math.max(80, Math.min(innerHeight - 16, down ? below : above));
        panel.style.maxHeight = `${available}px`;
        const height = Math.min(panel.scrollHeight, available);
        panel.style.left = `${Math.max(8, Math.min(box.right - width, innerWidth - width - 8))}px`;
        panel.style.top = `${Math.max(8, Math.min(down ? box.bottom + 6 : box.top - height - 6, innerHeight - height - 8))}px`;
    };
    const observer = new MutationObserver(position);
    const close = ({ restoreFocus = false } = {}) => {
        if (!state.open) return;
        const trigger = state.trigger;
        state.open = false;
        state.epoch++;
        state.read++;
        observer.disconnect();
        if (typeof panel.hidePopover === 'function' && panel.matches(':popover-open')) panel.hidePopover();
        panel.hidden = true;
        restoreTrigger();
        if (restoreFocus && trigger?.isConnected && trigger.getClientRects().length)
            trigger.focus({ preventScroll: true });
    };
    const preview = () =>
        el('preview').replaceChildren(createTagChip({ name: name.value.trim() || 'Tag preview', color: state.color }));
    const render = () => {
        panel.setAttribute('aria-labelledby', state.mode === 'create' ? 'quickCreateTitle' : 'quickTagsTitle');
        el('select').hidden = state.mode !== 'select';
        el('form').hidden = state.mode !== 'create';
        panel.querySelector('#quickTagsTitle').textContent = state.assignedOnly ? 'Assigned tags' : 'Tags';
        search.placeholder = state.assignedOnly ? 'Search assigned tags…' : 'Search tags…';
        el('create').hidden = state.assignedOnly;
        search.disabled = false;
        el('create').disabled = state.loading || state.busy;
        el('options').replaceChildren();
        const selected = new Set(state.assigned.map(tag => tag.id));
        const available = (state.assignedOnly ? state.assigned : state.definitions).filter(tag =>
            searchContains(tag.name, search.value)
        );
        for (const tag of available.slice(0, 200)) {
            const button = document.createElement('button');
            button.type = 'button';
            button.className = 'oj-button oj-button-ghost pinefetch-tag-choice';
            button.dataset.tagId = tag.id;
            button.setAttribute('role', 'checkbox');
            button.setAttribute('aria-checked', String(selected.has(tag.id)));
            button.setAttribute('aria-label', `${selected.has(tag.id) ? 'Remove' : 'Add'} tag ${tag.name}`);
            button.disabled = state.busy || state.loading;
            const check = document.createElement('span');
            check.className = 'pinefetch-quick-tag-check';
            check.textContent = selected.has(tag.id) ? '✓' : '';
            check.setAttribute('aria-hidden', 'true');
            button.append(createTagChip(tag), check);
            button.addEventListener('click', () => void toggle(tag, !selected.has(tag.id)));
            el('options').append(button);
        }
        if (!available.length) {
            const empty = document.createElement('p');
            empty.className = 'oj-helper';
            empty.textContent = state.loading
                ? 'Loading tags…'
                : state.assignedOnly
                  ? 'No assigned tags match.'
                  : 'No tags match. Create a new tag below.';
            el('options').append(empty);
        }
        if (available.length > 200) {
            const hint = document.createElement('p');
            hint.className = 'oj-helper';
            hint.textContent = 'Search to narrow the available tags.';
            el('options').append(hint);
        }
        for (const field of el('form').querySelectorAll('input,fieldset'))
            field.disabled = state.busy || Boolean(state.created);
        el('back').disabled = state.busy;
        el('cancel').disabled = state.busy;
        el('submit').disabled = state.busy;
        el('submit').setAttribute('aria-busy', String(state.busy));
        el('submit').textContent = state.created ? 'Retry Add' : 'Create & Add';
        for (const swatch of el('palette').children)
            swatch.setAttribute('aria-pressed', String(swatch.dataset.color === state.color));
        preview();
        position();
    };
    const load = async (epoch = state.epoch, id = state.id) => {
        const request = ++state.read;
        state.loading = true;
        render();
        try {
            const [definitions, assigned] = await Promise.all([
                state.assignedOnly ? Promise.resolve([]) : choices(),
                api.getHistoryTags({ id }),
            ]);
            if (!current(epoch, id) || request !== state.read) return;
            state.definitions = definitions;
            state.assigned = assigned;
            state.loading = false;
            render();
        } catch (error) {
            if (current(epoch, id) && request === state.read) {
                state.loading = false;
                status(String(error), true);
                render();
            }
        }
    };
    const toggle = async (tag, add) => {
        if (state.busy || state.loading) return;
        const epoch = state.epoch,
            id = state.id;
        const focused = panel.contains(document.activeElement);
        state.busy = true;
        status('Saving…');
        render();
        try {
            await assign(id, tag.id, add);
            if (!current(epoch, id)) return;
            state.assigned = add
                ? [...state.assigned.filter(item => item.id !== tag.id), tag]
                : state.assigned.filter(item => item.id !== tag.id);
            status('Saved.');
        } catch (error) {
            appendLog(`[Quick tags] ${error}`, true);
            if (current(epoch, id)) status(String(error), true);
        } finally {
            if (current(epoch, id)) {
                state.busy = false;
                render();
                if (focused && (panel.contains(document.activeElement) || document.activeElement === document.body)) {
                    const option = [...el('options').querySelectorAll('[role=checkbox]')].find(
                        button => button.dataset.tagId === tag.id
                    );
                    (option || search).focus({ preventScroll: true });
                }
            }
        }
    };
    const applyCreation = async (operation, epoch, id) => {
        if (!current(epoch, id)) return;
        state.busy = false;
        state.created = operation.error ? operation.tag : null;
        if (operation.error) {
            state.mode = 'create';
            status(
                operation.tag
                    ? `Tag created, but it could not be added. Retry Add to try this assignment again. ${operation.error}`
                    : operation.error,
                true
            );
            render();
        } else {
            state.mode = 'select';
            search.value = '';
            status('Tag created and added.');
            await load(epoch, id);
            if (current(epoch, id)) search.focus({ preventScroll: true });
        }
    };
    el('form').addEventListener('submit', async event => {
        event.preventDefault();
        const id = state.id,
            epoch = state.epoch;
        if (state.busy || creations.has(id)) return;
        const draft = { name: name.value.trim(), description: '', color: state.color };
        if (!draft.name || [...draft.name].length > 80) {
            status(!draft.name ? 'Enter a tag name.' : 'Use up to 80 characters for the name.', true);
            name.focus();
            return;
        }
        const operation = { tag: state.created, draft, error: null, promise: null };
        state.busy = true;
        status(state.created ? 'Adding tag…' : 'Creating tag…');
        render();
        creations.set(id, operation);
        operation.promise = (async () => {
            try {
                if (!operation.tag) {
                    operation.tag = await api.createTag({ tag: draft });
                    onCreated(operation.tag);
                }
                await assign(id, operation.tag.id, true);
                partials.delete(id);
            } catch (error) {
                operation.error = String(error);
                appendLog(`[Quick create] ${error}`, true);
                if (operation.tag) partials.set(id, operation);
                if (!current(epoch, id) && operation.tag)
                    toast(
                        'Tag created, but it could not be added to the original item. Reopen its tag selector to retry.',
                        { type: 'error' }
                    );
            } finally {
                creations.delete(id);
            }
        })();
        await operation.promise;
        await applyCreation(operation, epoch, id);
    });
    for (const color of tagPalette) {
        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'pinefetch-tag-swatch';
        button.style.backgroundColor = color;
        button.dataset.color = color;
        button.setAttribute('aria-label', `Use ${color}`);
        button.addEventListener('click', () => {
            state.color = color;
            render();
        });
        el('palette').append(button);
    }
    const select = () => {
        const created = state.created;
        state.mode = 'select';
        state.created = null;
        partials.delete(state.id);
        if (created) search.value = created.name;
        status(created ? 'Tag remains available. Select it to add it.' : '');
        render();
        search.focus({ preventScroll: true });
        if (created) void load();
    };
    el('create').addEventListener('click', () => {
        state.mode = 'create';
        state.created = null;
        state.color = tagPalette[0];
        name.value = search.value.trim();
        status();
        render();
        name.focus();
    });
    el('back').addEventListener('click', select);
    el('cancel').addEventListener('click', select);
    el('close').addEventListener('click', () => close({ restoreFocus: true }));
    name.addEventListener('input', () => {
        status();
        preview();
        position();
    });
    search.addEventListener('input', () => {
        render();
    });
    document.addEventListener(
        'pointerdown',
        event => {
            if (state.open && !panel.contains(event.target) && !state.trigger?.contains(event.target)) close();
        },
        true
    );
    document.addEventListener(
        'focusin',
        event => {
            // WebKit focuses the parent dialog when a popover button is pressed.
            // Outside pointer presses are handled separately, before that fallback focus.
            if (
                state.open &&
                event.target !== panel.closest('dialog') &&
                !panel.contains(event.target) &&
                !state.trigger?.contains(event.target)
            )
                close();
        },
        true
    );
    document.addEventListener(
        'keydown',
        event => {
            if (!state.open) return;
            if (event.key === 'Escape') {
                event.preventDefault();
                event.stopImmediatePropagation();
                close({ restoreFocus: true });
                return;
            }
            if (state.mode !== 'select' || !panel.contains(event.target)) return;
            const buttons = [...el('options').querySelectorAll('button:not(:disabled)')];
            const index = buttons.indexOf(event.target);
            if (
                ['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key) &&
                (event.target === search || index >= 0)
            ) {
                event.preventDefault();
                const next =
                    event.key === 'Home'
                        ? 0
                        : event.key === 'End'
                          ? buttons.length - 1
                          : index < 0
                            ? event.key === 'ArrowUp'
                                ? buttons.length - 1
                                : 0
                            : (index + (event.key === 'ArrowUp' ? -1 : 1) + buttons.length) % buttons.length;
                buttons[next]?.focus();
            } else if (event.key === 'Enter' && event.target === search && buttons.length) {
                event.preventDefault();
                buttons[0].click();
            }
        },
        true
    );
    window.addEventListener('resize', position);
    document.addEventListener(
        'scroll',
        event => {
            if (!panel.contains(event.target)) position();
        },
        true
    );
    return Object.freeze({
        close,
        sync: event => {
            if (state.open && !state.busy && (!event.historyId || event.historyId === state.id)) void load();
        },
        open: async (id, trigger, { assignedOnly = false } = {}) => {
            if (state.open && state.id === id && state.trigger === trigger && state.assignedOnly === assignedOnly) {
                close({ restoreFocus: true });
                return;
            }
            close();
            if (!trigger?.isConnected) return;
            const modal = document.querySelector('dialog:modal');
            if (modal && !modal.contains(trigger)) return;
            (trigger.closest('dialog[open]') || document.body).append(panel);
            state.open = true;
            state.id = id;
            setTrigger(trigger);
            state.card = trigger.closest('[data-history-id]');
            state.details = trigger.closest('#historyDetailsTags');
            state.assignedOnly = assignedOnly;
            state.epoch++;
            const epoch = state.epoch;
            state.mode = 'select';
            state.color = tagPalette[0];
            state.created = null;
            state.definitions = [];
            state.assigned = [];
            state.busy = false;
            search.value = '';
            status();
            panel.hidden = false;
            if (typeof panel.showPopover === 'function') panel.showPopover();
            observer.observe(document.body, {
                subtree: true,
                childList: true,
                attributes: true,
                attributeFilter: ['hidden', 'open', 'aria-hidden'],
            });
            render();
            search.disabled = false;
            search.focus({ preventScroll: true });
            const pending = creations.get(id);
            const partial = partials.get(id);
            if (!assignedOnly && (pending || partial)) {
                const operation = pending || partial;
                state.mode = 'create';
                state.busy = Boolean(pending);
                state.created = operation.tag;
                state.color = operation.draft.color;
                name.value = operation.draft.name;
                status(
                    pending
                        ? 'Creating and adding tag…'
                        : `Tag created, but it could not be added. Retry Add. ${operation.error}`,
                    !pending
                );
                render();
                (pending ? panel : el('submit')).focus({ preventScroll: true });
                if (pending) void pending.promise.then(() => applyCreation(pending, epoch, id));
            }
            await load(epoch, id);
            if (current(epoch, id) && state.mode === 'select') search.focus({ preventScroll: true });
        },
    });
};
