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
