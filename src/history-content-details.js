// Keep technical information available without competing with the saved text.
export const renderContentMetadata = (list, rows) => {
    list.replaceChildren();
    for (const [label, value] of rows) {
        if (value === null || value === undefined || value === '') continue;
        const term = document.createElement('dt');
        term.textContent = label;
        const description = document.createElement('dd');
        description.textContent = `${value}`;
        list.append(term, description);
    }
    list.hidden = !list.childElementCount;
};

export const renderContentDetails = (disclosure, rows) => {
    const list = disclosure.querySelector('dl');
    renderContentMetadata(list, rows);
    disclosure.open = false;
    disclosure.hidden = list.hidden;
};
