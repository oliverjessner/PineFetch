import { confirmDialog } from './vendor/oj/index.js';
import { searchContains } from './unicode-search.js';

export const tagPalette = ['#35C9A5', '#60A5FA', '#A78BFA', '#F472B6', '#FB923C', '#FACC15', '#F87171', '#94A3B8'];
export const tagColor = color => {
    const value = typeof color === 'string' ? color.trim().replace(/^#/, '') : '';
    if (/^[\dA-F]{3}$/i.test(value))
        return (
            '#' +
            value
                .split('')
                .map(c => c + c)
                .join('')
                .toUpperCase()
        );
    return /^[\dA-F]{6}$/i.test(value) ? '#' + value.toUpperCase() : tagPalette[0];
};
export const createTagChip = (tag, { open, remove, disabled = false } = {}) => {
    const chip = document.createElement('span');
    chip.className = 'pinefetch-tag-chip';
    chip.style.setProperty('--tag-color', tagColor(tag.color));
    const dot = document.createElement('span');
    dot.className = 'pinefetch-tag-dot';
    dot.setAttribute('aria-hidden', 'true');
    const label = document.createElement(open ? 'button' : 'span');
    label.className = 'pinefetch-tag-name';
    label.textContent = tag.name;
    label.title = tag.name;
    if (open) {
        label.type = 'button';
        label.addEventListener('click', event => {
            event.stopPropagation();
            open(tag);
        });
    }
    chip.append(dot, label);
    if (remove) {
        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'pinefetch-tag-remove';
        button.textContent = '×';
        button.disabled = disabled;
        button.setAttribute('aria-label', `Remove tag ${tag.name}`);
        button.addEventListener('click', () => remove(tag));
        chip.append(button);
    }
    return chip;
};
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
    const changed = event => onChange(event);
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
        if (saving) return Promise.resolve(null);
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
        const current = await api.getTag({ id: tag.id });
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
            button.addEventListener('click', () => void manage(id));
            root.append(button);
        }
        return root;
    };
    const picker = document.createElement('dialog');
    picker.id = 'historyTagsDialog';
    picker.className = 'oj-dialog pinefetch-tag-dialog';
    picker.setAttribute('aria-labelledby', 'historyTagsDialogTitle');
    picker.innerHTML = `<div class="oj-stack"><div class="oj-cluster"><h2 class="oj-heading-2 oj-grow" id="historyTagsDialogTitle">Manage Tags</h2><button type="button" class="oj-button oj-button-ghost" data-tags-close>Done</button></div><p class="oj-helper">Changes are saved immediately.</p><h3 class="oj-heading-3">Assigned tags</h3><div class="oj-cluster" data-tags-assigned></div><label class="oj-field">Search available tags<input id="historyTagsSearch" type="search" class="oj-input" autocomplete="off"></label><div class="oj-stack" data-tags-available></div><button type="button" class="oj-button oj-button-secondary" data-tags-create>Create New Tag</button><p role="status" class="oj-status" data-tags-status></p></div>`;
    document.body.append(picker);
    let pickerDefinitions = [];
    let pickerReturnFocus = null;
    let pickerId = null,
        pickerRequest = 0,
        pickerTags = [],
        pickerBusy = false,
        pickerLimit = 100;
    const pick = name => picker.querySelector(`[data-tags-${name}]`);
    const renderPicker = () => {
        pick('assigned').replaceChildren();
        for (const tag of pickerTags)
            pick('assigned').append(
                createTagChip(tag, { remove: tag => void assign(tag, false), disabled: pickerBusy })
            );
        if (!pickerTags.length) pick('assigned').textContent = 'No tags assigned.';
        const query = picker.querySelector('input').value;
        const selectedIds = new Set(pickerTags.map(tag => tag.id));
        const available = pickerDefinitions.filter(tag => !selectedIds.has(tag.id) && searchContains(tag.name, query));
        pick('available').replaceChildren();
        for (const tag of available.slice(0, pickerLimit)) {
            const button = document.createElement('button');
            button.type = 'button';
            button.className = 'oj-button oj-button-ghost pinefetch-tag-choice';
            button.disabled = pickerBusy;
            button.setAttribute('aria-label', `Add tag ${tag.name}`);
            button.append(createTagChip(tag));
            button.addEventListener('click', () => void assign(tag, true));
            pick('available').append(button);
        }
        if (!available.length) {
            const empty = document.createElement('p');
            empty.className = 'oj-helper';
            empty.textContent = query
                ? 'No available tags match this search.'
                : 'All available tags are assigned. Create a new tag to add another.';
            pick('available').append(empty);
        }
        if (available.length > pickerLimit) {
            const more = document.createElement('button');
            more.type = 'button';
            more.className = 'oj-button oj-button-ghost';
            more.textContent = 'Show more tags';
            more.addEventListener('click', () => {
                pickerLimit += 100;
                renderPicker();
            });
            pick('available').append(more);
        }
        pick('create').disabled = pickerBusy;
    };
    const loadPicker = async () => {
        const request = ++pickerRequest;
        const id = pickerId;
        pick('status').textContent = 'Loading tags…';
        try {
            const [definitions, tags] = await Promise.all([choices(), api.getHistoryTags({ id })]);
            if (request !== pickerRequest || !picker.open) return;
            pickerDefinitions = definitions;
            pickerTags = tags;
            assigned.set(id, tags);
            pick('status').textContent = '';
            renderPicker();
        } catch (error) {
            if (request === pickerRequest) pick('status').textContent = String(error);
        }
    };
    const assign = async (tag, add) => {
        if (pickerBusy) return;
        const request = pickerRequest;
        const id = pickerId;
        pickerBusy = true;
        renderPicker();
        pick('status').textContent = 'Saving…';
        try {
            await assignEntry(id, tag.id, add);
            if (request !== pickerRequest || !picker.open) return;
            pickerTags = add
                ? [...pickerTags, tag].sort((a, b) => a.name.localeCompare(b.name))
                : pickerTags.filter(current => current.id !== tag.id);
            assigned.set(id, pickerTags);
            pick('status').textContent = 'Saved.';
        } catch (error) {
            appendLog(`[Tags] ${error}`, true);
            if (request === pickerRequest) pick('status').textContent = String(error);
        } finally {
            if (request === pickerRequest) {
                pickerBusy = false;
                renderPicker();
            }
        }
    };
    const manage = async (id, returnFocus = document.activeElement) => {
        if (picker.open) return;
        pickerBusy = false;
        pickerReturnFocus = returnFocus;
        pickerRequest++;
        pickerId = id;
        pickerTags = [];
        pickerLimit = 100;
        picker.querySelector('input').value = '';
        pick('assigned').replaceChildren();
        pick('available').replaceChildren();
        pick('create').disabled = true;
        picker.showModal();
        await loadPicker();
    };
    picker.addEventListener('close', () => {
        pickerRequest++;
        if (document.querySelector('dialog[open]')) return;
        const visible = element => element?.isConnected && element.getClientRects().length;
        const replacement = [...document.querySelectorAll('[data-history-id]')]
            .find(row => row.dataset.historyId === pickerId && visible(row))
            ?.querySelector('.pinefetch-history-open-btn');
        const target = visible(pickerReturnFocus) ? pickerReturnFocus : replacement;
        if (visible(target)) target.focus({ preventScroll: true });
    });
    pick('close').addEventListener('click', () => picker.close());
    picker.querySelector('input').addEventListener('input', () => {
        pickerLimit = 100;
        renderPicker();
    });
    pick('create').addEventListener('click', async () => {
        const id = pickerId;
        const request = pickerRequest;
        const tag = await edit();
        if (tag && picker.open && id === pickerId && request === pickerRequest) await assign(tag, true);
    });
    const assignments = new Map();
    const assignEntry = async (id, tagId, add) => {
        const key = `${id}:${tagId}`;
        if (assignments.has(key)) return assignments.get(key);
        const pending = (async () => {
            await (add ? api.addHistoryTag({ id, tagId }) : api.removeHistoryTag({ id, tagId }));
            changed({ historyId: id, tagId });
        })();
        assignments.set(key, pending);
        try {
            await pending;
        } finally {
            assignments.delete(key);
        }
    };
    let detailId = null,
        detailRequest = 0,
        detailBusy = false;
    if (detailsRoot)
        detailsRoot.innerHTML =
            '<div class="oj-cluster"><h3 class="oj-heading-3 oj-grow">Tags</h3><button type="button" class="oj-button oj-button-secondary oj-button-compact">+ Add Tag</button></div><div class="oj-cluster" data-detail-tags></div><p class="oj-helper" role="status" data-detail-tag-status></p>';
    detailsRoot?.querySelector('button').addEventListener('click', () => {
        if (detailId) void manage(detailId);
    });
    const showDetails = async id => {
        detailId = id;
        const request = ++detailRequest;
        if (!detailsRoot) return;
        const list = detailsRoot.querySelector('[data-detail-tags]');
        const status = detailsRoot.querySelector('[data-detail-tag-status]');
        if (!id) {
            list.replaceChildren();
            status.textContent = '';
            return;
        }
        status.textContent = 'Loading tags…';
        try {
            const rows = await api.getHistoryTags({ id });
            if (request !== detailRequest) return;
            assigned.set(id, rows);
            list.replaceChildren();
            for (const tag of rows)
                list.append(
                    createTagChip(tag, {
                        open: openTag,
                        disabled: detailBusy,
                        remove: async () => {
                            if (detailBusy) return;
                            detailBusy = true;
                            for (const button of list.querySelectorAll('button')) button.disabled = true;
                            let failure;
                            try {
                                await assignEntry(id, tag.id, false);
                            } catch (error) {
                                failure = String(error);
                                appendLog(`[Tags] ${error}`, true);
                            } finally {
                                detailBusy = false;
                                if (id === detailId) {
                                    await showDetails(id);
                                    if (failure && id === detailId) status.textContent = failure;
                                }
                            }
                        },
                    })
                );
            if (!rows.length) list.textContent = 'No tags assigned.';
            status.textContent = '';
        } catch (error) {
            if (request === detailRequest) status.textContent = String(error);
        }
    };
    return Object.freeze({
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
