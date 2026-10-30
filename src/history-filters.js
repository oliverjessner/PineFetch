// Secondary History filters use one native, non-modal surface. Query state and
// persistence stay in history-view; the existing source dropdown is reused here.
export const createHistoryFilters = ({ els, values, clear }) => {
    const panel = els.historyFiltersPopover;
    const trigger = els.historyFiltersBtn;
    const open = () => panel.matches(':popover-open');
    const close = () => {
        if (open()) panel.hidePopover();
    };
    const position = () => {
        if (!open()) return;
        const box = trigger.getBoundingClientRect();
        const width = Math.min(300, innerWidth - 16);
        panel.style.width = `${width}px`;
        panel.style.maxHeight = `${Math.max(1, innerHeight - 16)}px`;
        panel.style.left = `${Math.max(8, Math.min(box.right - width, innerWidth - width - 8))}px`;
        const height = panel.getBoundingClientRect().height;
        panel.style.top = `${Math.max(8, Math.min(box.bottom + 6, innerHeight - height - 8))}px`;
    };
    panel.addEventListener('toggle', () => {
        trigger.setAttribute('aria-expanded', String(open()));
        if (open()) {
            position();
            if (!panel.contains(document.activeElement)) els.historySourceTrigger.focus({ preventScroll: true });
        }
    });
    panel.addEventListener('keydown', event => {
        if (event.key !== 'Escape' || event.defaultPrevented) return;
        // OJ handles its source menu's Escape at document level. Let that menu
        // close first; the next Escape dismisses this containing surface.
        if (!els.historySourceMenu.hidden) return;
        event.preventDefault();
        event.stopPropagation();
        close();
        trigger.focus({ preventScroll: true });
    });
    window.addEventListener('resize', position);
    document.addEventListener('scroll', position, true);
    els.historyClearSourceFilter.addEventListener('click', () => {
        els.historySourceTrigger.focus({ preventScroll: true });
        clear({ source: true });
    });
    els.historyClearFiltersBtn.addEventListener('click', () => {
        els.historySourceTrigger.focus({ preventScroll: true });
        clear({ source: true, tag: true });
    });
    trigger.setAttribute('aria-expanded', 'false');
    return Object.freeze({
        close,
        update: () => {
            const { source, tagId } = values();
            const count = Number(Boolean(source)) + Number(Boolean(tagId));
            els.historyFiltersValue.textContent = count ? `Filters · ${count}` : 'Filters';
            trigger.setAttribute('aria-label', count ? `Filters, ${count} active` : 'Filters');
            trigger.classList.toggle('pinefetch-filters-active', count > 0);
            els.historyClearSourceFilter.hidden = !source;
            els.historyClearTagFilter.hidden = !tagId;
            els.historyClearFiltersBtn.disabled = count === 0;
            position();
        },
    });
};
