// OJ owns opening, positioning and keyboard navigation. This adapter owns
// the selected value and safe radio-item markup for PineFetch's choice menus.
export const createDropdownChoice = ({ menu, value }) => {
    let choices = new Map();
    let selectedValue = '';

    const setValue = nextValue => {
        selectedValue = choices.has(nextValue) ? nextValue : (choices.keys().next().value ?? '');
        if (choices.has(selectedValue)) value.textContent = choices.get(selectedValue);
        for (const item of menu.children) {
            item.setAttribute('aria-checked', String(item.dataset.ojValue === selectedValue));
        }
        return selectedValue;
    };

    const setOptions = options => {
        const document = menu.ownerDocument;
        const restoreFocus = !menu.hidden && menu.contains(document.activeElement);
        const focusedValue = document.activeElement?.closest('[data-oj-value]')?.dataset.ojValue;
        choices = new Map(options.map(option => [option.value, option.label]));
        const fragment = document.createDocumentFragment();
        for (const [key, label] of choices) {
            const item = document.createElement('button');
            item.type = 'button';
            item.className = 'oj-menu-item pinefetch-choice-menu-item';
            item.setAttribute('role', 'menuitemradio');
            item.dataset.ojValue = key;
            item.tabIndex = -1;
            const check = document.createElement('i');
            check.className = 'fa-solid fa-check pinefetch-choice-check';
            check.setAttribute('aria-hidden', 'true');
            const text = document.createElement('span');
            text.textContent = label;
            item.append(check, text);
            fragment.appendChild(item);
        }
        menu.replaceChildren(fragment);
        setValue(selectedValue);
        if (restoreFocus) {
            const items = Array.from(menu.children);
            const next =
                items.find(item => item.dataset.ojValue === focusedValue) ||
                items.find(item => item.dataset.ojValue === selectedValue);
            next?.focus({ preventScroll: true });
        }
    };

    return Object.freeze({
        getValue: () => selectedValue,
        hasValue: key => choices.has(key),
        setValue,
        setOptions,
    });
};
