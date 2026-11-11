// Model output is untrusted. Build semantic DOM without interpreting HTML,
// loading remote images, or allowing executable link protocols.
const safeHref = value => {
    for (const character of value) {
        const code = character.charCodeAt(0);
        if (code <= 32 || code === 127) return null;
    }
    try {
        const url = new URL(value);
        return ['https:', 'http:', 'mailto:'].includes(url.protocol) ? url.href : null;
    } catch {
        return null;
    }
};

const appendText = (parent, value, document) => {
    if (value) parent.append(document.createTextNode(value));
};

const appendInline = (parent, source, document, depth = 0) => {
    if (depth > 8) {
        appendText(parent, source, document);
        return;
    }
    const tokens = /\\[\\`*_[\]{}()#+.!>~-]|`+|!?\[|\*\*|__|~~|\*|_|\n/g;
    let offset = 0;
    for (let token = tokens.exec(source); token; token = tokens.exec(source)) {
        const index = token.index;
        if (index < offset) continue;
        let prefix = source.slice(offset, index);
        const marker = token[0];
        if (marker === '\n' && / {2,}$/.test(prefix)) {
            prefix = prefix.replace(/ {2,}$/, '');
            appendText(parent, prefix, document);
            parent.append(document.createElement('br'));
            offset = index + 1;
            continue;
        }
        appendText(parent, prefix, document);
        offset = index + marker.length;
        if (marker.startsWith('\\')) {
            appendText(parent, marker.slice(1), document);
        } else if (marker === '\n') {
            appendText(parent, ' ', document);
        } else if (marker.startsWith('`')) {
            const end = source.indexOf(marker, offset);
            if (end === -1) appendText(parent, marker, document);
            else {
                const code = document.createElement('code');
                code.textContent = source.slice(offset, end).replace(/\n/g, ' ');
                parent.append(code);
                offset = end + marker.length;
            }
        } else if (marker === '[' || marker === '![') {
            const link = source
                .slice(index)
                .match(/^(!?)\[([^\]\n]+)\]\(\s*(<[^>\n]+>|[^)\s]+)(?:\s+["'][^)\n]*["'])?\s*\)/);
            if (!link) appendText(parent, marker, document);
            else {
                const href = safeHref(link[3].replace(/^<|>$/g, ''));
                if (link[1] || !href) appendInline(parent, link[2], document, depth + 1);
                else {
                    const anchor = document.createElement('a');
                    anchor.setAttribute('href', href);
                    anchor.setAttribute('target', '_blank');
                    anchor.setAttribute('rel', 'noopener noreferrer');
                    appendInline(anchor, link[2], document, depth + 1);
                    parent.append(anchor);
                }
                offset = index + link[0].length;
            }
        } else {
            const insideWord = marker.includes('_') && /\w/.test(source[index - 1] || '');
            const end = insideWord ? -1 : source.indexOf(marker, offset);
            if (end <= offset) appendText(parent, marker, document);
            else {
                const tag = marker.length === 2 ? (marker === '~~' ? 's' : 'strong') : 'em';
                const emphasis = document.createElement(tag);
                appendInline(emphasis, source.slice(offset, end), document, depth + 1);
                parent.append(emphasis);
                offset = end + marker.length;
            }
        }
        tokens.lastIndex = offset;
    }
    appendText(parent, source.slice(offset), document);
};

const listItem = line => line.match(/^( *)([-+*]|\d+[.)])\s+(.+)$/);
const fence = line => line.match(/^ {0,3}(`{3,}|~{3,})(.*)$/);
const heading = line => line.match(/^ {0,3}(#{1,6})\s+(.+?)\s*#*$/);
const rule = line => /^ {0,3}(?:\*\s*){3,}$|^ {0,3}(?:-\s*){3,}$|^ {0,3}(?:_\s*){3,}$/.test(line);
const beginsBlock = line => fence(line) || heading(line) || listItem(line) || /^\s*>/.test(line) || rule(line);

const appendBlocks = (container, lines, document, depth = 0) => {
    for (let index = 0; index < lines.length;) {
        const line = lines[index];
        if (!line.trim()) {
            index += 1;
            continue;
        }
        const opening = fence(line);
        if (opening) {
            const content = [];
            const closing = new RegExp(`^ {0,3}${opening[1][0]}{${opening[1].length},}\\s*$`);
            index += 1;
            while (index < lines.length && !closing.test(lines[index])) content.push(lines[index++]);
            if (index < lines.length) index += 1;
            const pre = document.createElement('pre');
            pre.className = 'oj-code-block';
            const code = document.createElement('code');
            code.textContent = content.join('\n');
            pre.append(code);
            container.append(pre);
            continue;
        }
        const title = heading(line);
        if (title) {
            const element = document.createElement(`h${Math.min(6, title[1].length + 3)}`);
            appendInline(element, title[2], document);
            container.append(element);
            index += 1;
            continue;
        }
        if (rule(line)) {
            container.append(document.createElement('hr'));
            index += 1;
            continue;
        }
        if (/^\s*>/.test(line) && depth < 8) {
            const quoted = [];
            while (index < lines.length && /^\s*>/.test(lines[index]))
                quoted.push(lines[index++].replace(/^\s*> ?/, ''));
            const quote = document.createElement('blockquote');
            appendBlocks(quote, quoted, document, depth + 1);
            container.append(quote);
            continue;
        }
        const firstItem = listItem(line);
        if (firstItem && depth < 8) {
            const indentation = firstItem[1].length;
            const ordered = /^\d/.test(firstItem[2]);
            const list = document.createElement(ordered ? 'ol' : 'ul');
            if (ordered && Number.parseInt(firstItem[2], 10) !== 1)
                list.setAttribute('start', String(Number.parseInt(firstItem[2], 10)));
            while (index < lines.length) {
                const item = listItem(lines[index]);
                if (!item || item[1].length !== indentation || /^\d/.test(item[2]) !== ordered) break;
                const li = document.createElement('li');
                appendInline(li, item[3], document);
                index += 1;
                const nested = [];
                while (index < lines.length && lines[index].trim()) {
                    const next = listItem(lines[index]);
                    const spaces = lines[index].match(/^ */)[0].length;
                    if (spaces <= indentation || (next && next[1].length <= indentation)) break;
                    nested.push(lines[index++].slice(Math.min(spaces, indentation + 2)));
                }
                if (nested.length) appendBlocks(li, nested, document, depth + 1);
                list.append(li);
            }
            container.append(list);
            continue;
        }
        const paragraph = [line];
        index += 1;
        while (index < lines.length && lines[index].trim() && !beginsBlock(lines[index]))
            paragraph.push(lines[index++]);
        const p = document.createElement('p');
        appendInline(p, paragraph.join('\n'), document);
        container.append(p);
    }
};

export const renderAiMarkdown = (container, markdown) => {
    const document = container.ownerDocument || globalThis.document;
    container.replaceChildren();
    appendBlocks(
        container,
        String(markdown || '')
            .replace(/\r\n?/g, '\n')
            .split('\n'),
        document
    );
};
