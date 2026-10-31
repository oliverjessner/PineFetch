import { confirmDialog } from './vendor/oj/index.js';
import { createTagChip, tagPalette, tagColor } from './tag-ui.js';
import { createQuickTags } from './quick-tags.js';

export const createTagController = ({ api, appendLog, onChange = () => {}, openTag = () => {}, detailsRoot }) => {
    const assigned = new Map();
    const assignmentReads = new Map();
    let assignmentRead = 0;
    const editor = document.createElement('dialog');
    editor.id = 'tagEditorDialog';
    editor.className = 'oj-dialog pinefetch-tag-dialog';
    editor.setAttribute('aria-labelledby', 'tagEditorTitle');
    editor.innerHTML = `<form class="oj-stack" novalidate><h2 class="oj-heading-2" id="tagEditorTitle">Create Tag</h2><label class="oj-field">Name<input id="tagNameInput" class="oj-input" name="name" required autocomplete="off" aria-describedby="tagNameHelp"></label><p class="oj-helper" id="tagNameHelp">Required · up to 80 characters</p><label class="oj-field">Description<textarea id="tagDescriptionInput" class="oj-input" name="description" rows="3" aria-describedby="tagDescriptionHelp"></textarea></label><p class="oj-helper" id="tagDescriptionHelp">Optional · up to 500 characters</p><fieldset class="oj-stack"><legend class="oj-label">Color</legend><div class="oj-cluster" data-tag-palette></div><div class="oj-cluster"><label>Pick color <input id="tagColorPicker" type="color" value="#35c9a5"></label><label class="oj-field oj-grow">HEX color<input id="tagColorInput" class="oj-input" name="color" value="#35C9A5" autocomplete="off" spellcheck="false"></label></div></fieldset><div data-tag-preview aria-label="Tag preview"></div><p class="oj-status oj-status-error" role="status" data-tag-error></p><div class="oj-cluster"><button type="button" class="oj-button oj-button-ghost" data-tag-cancel>Cancel</button><button type="submit" class="oj-button oj-button-primary" data-tag-submit>Create Tag</button></div></form>`;
    document.body.append(editor);
    const element = selector => editor.querySelector(selector);
    let editorResolve;
    let editorTag = null;
    let editorRevision = 0;
    let saving = false;
    const preview = () => {
        element('[data-tag-preview]').replaceChildren(
            createTagChip({
                name: element('[name=name]').value.trim() || 'Tag preview',
                color: element('[name=color]').value,
            })
        );
    };
    for (const color of tagPalette) {
        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'pinefetch-tag-swatch';
        button.style.backgroundColor = color;
        button.setAttribute('aria-label', `Use ${color}`);
        button.addEventListener('click', () => {
            element('[name=color]').value = color;
            element('#tagColorPicker').value = color;
            preview();
        });
        element('[data-tag-palette]').append(button);
    }
    element('#tagColorPicker').addEventListener('input', event => {
        element('[name=color]').value = event.target.value.toUpperCase();
        preview();
    });
    editor.addEventListener('input', () => {
        preview();
        element('[data-tag-error]').textContent = '';
    });
    element('[name=color]').addEventListener('input', event => {
        if (/^#?(?:[\dA-F]{3}|[\dA-F]{6})$/i.test(event.target.value.trim()))
            element('#tagColorPicker').value = tagColor(event.target.value);
    });
    element('[data-tag-cancel]').addEventListener('click', () => editor.close());
    editor.addEventListener('close', () => {
        editorRevision++;
        editorResolve?.(null);
        editorResolve = null;
    });
    const refresh = () => api.listTags();
    const choices = () => api.getTagChoices();
    let quick;
    const changed = event => {
        onChange(event);
        quick?.sync(event);
    };
    element('form').addEventListener('submit', async event => {
        event.preventDefault();
        if (saving) return;
        const name = element('[name=name]').value.trim();
        const description = element('[name=description]').value.trim();
        const color = element('[name=color]').value.trim();
        const error = element('[data-tag-error]');
        error.textContent = '';
        if (!name || [...name].length > 80 || [...description].length > 500) {
            error.textContent = !name
                ? 'Enter a tag name.'
                : 'Use up to 80 characters for names and 500 for descriptions.';
            return;
        }
        const request = editorRevision;
        saving = true;
        for (const field of editor.querySelectorAll('input,textarea,fieldset')) field.disabled = true;
        const submit = element('[data-tag-submit]');
        submit.disabled = true;
        submit.setAttribute('aria-busy', 'true');
        try {
            const tag = editorTag
                ? await api.updateTag({ id: editorTag.id, tag: { name, description, color } })
                : await api.createTag({ tag: { name, description, color } });
            changed({ tagId: tag.id });
            if (request === editorRevision && editor.open) {
                const resolve = editorResolve;
                editorResolve = null;
                editor.close();
                resolve?.(tag);
            }
        } catch (failure) {
            appendLog(`[Tags] ${failure}`, true);
            if (request === editorRevision) error.textContent = String(failure);
        } finally {
            saving = false;
            for (const field of editor.querySelectorAll('input,textarea,fieldset')) field.disabled = false;
            submit.disabled = false;
            submit.setAttribute('aria-busy', 'false');
        }
    });
    const edit = (tag = null) => {
        if (saving || document.querySelector('dialog:modal')) return Promise.resolve(null);
        quick?.close();
        if (editor.open) return Promise.resolve(null);
        editorRevision++;
        editorTag = tag;
        element('#tagEditorTitle').textContent = tag ? 'Edit Tag' : 'Create Tag';
        element('[data-tag-submit]').textContent = tag ? 'Save Changes' : 'Create Tag';
        element('[name=name]').value = tag?.name || '';
        element('[name=description]').value = tag?.description || '';
        element('[name=color]').value = tag?.color || tagPalette[0];
        element('#tagColorPicker').value = tag?.color || tagPalette[0];
        element('[data-tag-error]').textContent = '';
        preview();
        editor.showModal();
        element('[name=name]').focus();
        return new Promise(resolve => {
            editorResolve = resolve;
        });
    };
    const removeDefinition = async tag => {
        quick?.close();
        if (document.querySelector('dialog:modal')) throw new Error('Close the open dialog before deleting a tag.');
        const current = await api.getTag({ id: tag.id });
        if (document.querySelector('dialog:modal')) throw new Error('Close the open dialog before deleting a tag.');
        if (
            !(await confirmDialog({
                title: 'Delete tag?',
                message: `Delete “${current.name}” and remove its assignments from ${current.content_count.toLocaleString()} saved items? Saved content, media and texts will remain.`,
                confirmLabel: 'Delete Tag',
                danger: true,
            }))
        )
            return false;
        await api.deleteTag({ id: tag.id });
        changed({ tagId: tag.id, deleted: true });
        return true;
    };
    const batch = async ids => {
        if (!ids.length) return;
        const request = ++assignmentRead;
        for (const id of ids) assignmentReads.set(id, request);
        const result = await api.getHistoryTagsBatch({ ids });
        for (const entry of result)
            if (assignmentReads.get(entry.history_entry_id) === request)
                assigned.set(entry.history_entry_id, entry.tags);
        return result;
    };
    const chips = (id, { compact = true } = {}) => {
        const root = document.createElement('div');
        root.className = 'oj-cluster pinefetch-tag-chips';
        root.dataset.tagHistoryId = id;
        const tags = assigned.get(id) || [];
        for (const tag of compact ? tags.slice(0, 3) : tags) root.append(createTagChip(tag, { open: openTag }));
        if (compact && tags.length > 3) {
            const button = document.createElement('button');
            button.type = 'button';
            button.className = 'oj-button oj-button-ghost oj-button-compact';
            button.textContent = `+${tags.length - 3}`;
            button.title = tags
                .slice(3)
                .map(tag => tag.name)
                .join(', ');
            button.setAttribute('aria-label', `Manage all ${tags.length} tags`);
            button.addEventListener('click', event => void manage(id, event.currentTarget));
            root.append(button);
        }
        return root;
    };
    const assignments = new Map();
    const assignEntry = async (id, tagId, add) => {
        const key = `${id}:${tagId}`;
        const previous = assignments.get(key);
        if (previous) {
            if (previous.add === add) return previous.promise;
            await previous.promise.catch(() => {});
            return assignEntry(id, tagId, add);
        }
        const operation = { add, promise: null };
        operation.promise = (async () => {
            await (add ? api.addHistoryTag({ id, tagId }) : api.removeHistoryTag({ id, tagId }));
            changed({ historyId: id, tagId });
        })();
        assignments.set(key, operation);
        try {
            await operation.promise;
        } finally {
            if (assignments.get(key) === operation) assignments.delete(key);
        }
    };
    quick = createQuickTags({
        api,
        choices,
        assign: assignEntry,
        onCreated: tag => changed({ tagId: tag.id }),
        appendLog,
    });
    const manage = (id, trigger = document.activeElement) => quick.open(id, trigger);
    let detailId = null,
        detailRequest = 0,
        detailLoading = false;
    const detailRemovals = new Set();
    const compactDetails = matchMedia('(max-width: 620px)');
    if (detailsRoot)
        detailsRoot.innerHTML =
            '<hr class="oj-divider"><div class="oj-cluster" data-detail-tags-row><span class="oj-label">Tags</span><span data-detail-tags></span><button type="button" class="oj-button oj-button-ghost oj-button-compact" data-detail-tags-overflow hidden></button><button type="button" class="oj-button oj-button-secondary oj-button-compact" data-detail-tags-add>+ Add Tag</button><span class="oj-helper oj-sr-only" role="status" data-detail-tag-status></span></div>';
    const detailStatus = (message = '', error = false) => {
        const status = detailsRoot?.querySelector('[data-detail-tag-status]');
        if (!status) return;
        status.textContent = message;
        status.classList.toggle('oj-sr-only', !error);
        status.classList.toggle('oj-status-error', error);
    };
    const renderDetails = () => {
        if (!detailsRoot) return;
        const list = detailsRoot.querySelector('[data-detail-tags]');
        const more = detailsRoot.querySelector('[data-detail-tags-overflow]');
        const add = detailsRoot.querySelector('[data-detail-tags-add]');
        detailsRoot.hidden = !detailId;
        const id = detailId;
        const rows = assigned.get(id) || [];
        const limit = compactDetails.matches ? 1 : 3;
        const busy = detailLoading || detailRemovals.has(detailId);
        detailsRoot.setAttribute('aria-busy', String(busy));
        list.replaceChildren();
        for (const tag of rows.slice(0, limit))
            list.append(
                createTagChip(tag, {
                    open: openTag,
                    disabled: busy,
                    remove: () => void removeDetailTag(id, tag.id),
                })
            );
        more.hidden = rows.length <= limit;
        const remaining = Math.max(0, rows.length - limit);
        more.textContent = `+${remaining}`;
        more.setAttribute('aria-label', `Show ${remaining} more ${remaining === 1 ? 'tag' : 'tags'}`);
        more.title = rows
            .slice(limit)
            .map(tag => tag.name)
            .join(', ');
        add.disabled = !detailId || detailLoading;
    };
    const removeDetailTag = async (id, tagId) => {
        if (!id || detailLoading || detailRemovals.has(id)) return;
        detailRemovals.add(id);
        const restoreFocus = detailsRoot.contains(document.activeElement);
        detailStatus();
        renderDetails();
        let failure;
        try {
            await assignEntry(id, tagId, false);
            assigned.set(
                id,
                (assigned.get(id) || []).filter(tag => tag.id !== tagId)
            );
        } catch (error) {
            failure = String(error);
            appendLog(`[Tags] ${error}`, true);
        } finally {
            detailRemovals.delete(id);
            if (id === detailId) {
                renderDetails();
                detailStatus(failure || '', Boolean(failure));
                if (restoreFocus && document.activeElement === document.body)
                    detailsRoot.querySelector('[data-detail-tags-add]').focus({ preventScroll: true });
            }
        }
    };
    detailsRoot?.querySelector('[data-detail-tags-add]').addEventListener('click', event => {
        if (detailId) void manage(detailId, event.currentTarget);
    });
    detailsRoot?.querySelector('[data-detail-tags-overflow]').addEventListener('click', event => {
        if (detailId) void quick.open(detailId, event.currentTarget, { assignedOnly: true });
    });
    compactDetails.addEventListener('change', renderDetails);
    const showDetails = async id => {
        detailId = id;
        const request = ++detailRequest;
        if (!detailsRoot) return;
        detailLoading = Boolean(id);
        detailStatus(id ? 'Loading tags…' : '');
        renderDetails();
        if (!id) return;
        try {
            const rows = await api.getHistoryTags({ id });
            if (request !== detailRequest || id !== detailId) return;
            assigned.set(id, rows);
            detailStatus();
        } catch (error) {
            if (request === detailRequest && id === detailId) detailStatus(String(error), true);
        } finally {
            if (request === detailRequest && id === detailId) {
                detailLoading = false;
                renderDetails();
            }
        }
    };
    return Object.freeze({
        closeQuick: () => quick.close(),
        assignEntry,
        showDetails,
        refreshDetails: () => (detailId ? showDetails(detailId) : undefined),
        refresh,
        edit,
        removeDefinition,
        batch,
        chips,
        manage,
        assigned: id => assigned.get(id) || [],
        choices,
        changed,
    });
};
