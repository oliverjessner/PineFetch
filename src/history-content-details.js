// Keep technical information available without competing with the saved text.
export const renderContentDetails = (disclosure, rows) => {
    const list = disclosure.querySelector('dl');
    list.replaceChildren();
    for (const [label, value] of rows) {
        if (value === null || value === undefined || value === '') continue;
        const term = document.createElement('dt');
        term.textContent = label;
        const description = document.createElement('dd');
        description.textContent = `${value}`;
        list.append(term, description);
    }
    disclosure.open = false;
    disclosure.hidden = !list.childElementCount;
};
