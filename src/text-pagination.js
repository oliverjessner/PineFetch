export const createTextPagination = (viewport, onChange, size = 200) => {
    let start = 0;
    let total = 0;
    const controls = document.createElement('div');
    controls.className = 'oj-cluster pinefetch-text-pagination';
    controls.setAttribute('role', 'group');
    controls.setAttribute('aria-label', 'Text pages');
    const previous = document.createElement('button');
    const next = document.createElement('button');
    const label = document.createElement('span');
    label.className = 'oj-helper';
    label.setAttribute('role', 'status');
    for (const [button, text, direction] of [
        [previous, 'Previous entries', -1],
        [next, 'Next entries', 1],
    ]) {
        button.type = 'button';
        button.className = 'oj-button oj-button-ghost oj-button-compact';
        button.textContent = text;
        button.addEventListener('click', () => {
            start = Math.max(0, Math.min(Math.max(0, Math.ceil(total / size) - 1) * size, start + direction * size));
            viewport.scrollTop = 0;
            onChange();
        });
    }
    controls.append(previous, label, next);
    viewport.append(controls);
    controls.hidden = true;
    return {
        get start() {
            return start;
        },
        get size() {
            return size;
        },
        reset: () => {
            start = 0;
        },
        show: index => {
            if (index < 0 || (index >= start && index < start + size)) return false;
            start = Math.floor(index / size) * size;
            return true;
        },
        window: rows => {
            total = rows.length;
            start = Math.min(start, Math.max(0, Math.ceil(total / size) - 1) * size);
            controls.hidden = total <= size;
            previous.disabled = start === 0;
            next.disabled = start + size >= total;
            label.textContent = `${(start + 1).toLocaleString()}–${Math.min(start + size, total).toLocaleString()} of ${total.toLocaleString()}`;
            return rows.slice(start, start + size);
        },
    };
};
