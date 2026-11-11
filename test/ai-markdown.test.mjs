import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderAiMarkdown } from '../src/ai-markdown.js';

class Node {
    constructor(tagName = null, text = '') {
        this.tagName = tagName;
        this.text = text;
        this.children = [];
        this.attributes = new Map();
    }
    get textContent() {
        return this.text + this.children.map(child => child.textContent).join('');
    }
    set textContent(value) {
        this.text = value;
        this.children = [];
    }
    append(...nodes) {
        this.children.push(...nodes);
    }
    replaceChildren(...nodes) {
        this.text = '';
        this.children = nodes;
    }
    setAttribute(name, value) {
        this.attributes.set(name, value);
    }
}

const render = markdown => {
    const container = new Node('div');
    container.ownerDocument = {
        createElement: tagName => new Node(tagName),
        createTextNode: text => new Node(null, text),
    };
    renderAiMarkdown(container, markdown);
    return container;
};
const descendants = node => [node, ...node.children.flatMap(descendants)];
const tags = (node, tagName) => descendants(node).filter(child => child.tagName === tagName);

test('Markdown renders semantic paragraphs, emphasis, inline code and ordered and unordered lists', () => {
    const content = render(
        'A **clear** answer with *context* and `literal code`.\nA continued paragraph.\n\n- First insight\n- Second insight\n\n3. Third step\n4. Fourth step'
    );
    assert.deepEqual(
        content.children.map(child => child.tagName),
        ['p', 'ul', 'ol']
    );
    assert.equal(tags(content, 'strong')[0].textContent, 'clear');
    assert.equal(tags(content, 'em')[0].textContent, 'context');
    assert.equal(tags(content, 'code')[0].textContent, 'literal code');
    assert.equal(
        content.children[0].textContent,
        'A clear answer with context and literal code. A continued paragraph.'
    );
    assert.equal(tags(content, 'li').length, 4);
    assert.equal(tags(content, 'ol')[0].attributes.get('start'), '3');
});

test('Markdown preserves fenced code as literal text and supports nested lists and quotations', () => {
    const content = render(
        '# Findings\n\n- Main insight\n  - Supporting detail\n  - Another detail\n- Next insight\n\n> A **quoted** insight.\n\n```html\n<img src="https://example.com/track" onerror="run()">\n**literal**\n```'
    );
    assert.equal(tags(content, 'h4')[0].textContent, 'Findings');
    assert.equal(tags(content, 'ul').length, 2);
    assert.equal(tags(content, 'li').length, 4);
    assert.equal(tags(content, 'blockquote')[0].textContent, 'A quoted insight.');
    assert.equal(tags(content, 'pre')[0].className, 'oj-code-block');
    assert.equal(
        tags(content, 'pre')[0].textContent,
        '<img src="https://example.com/track" onerror="run()">\n**literal**'
    );
    assert.equal(tags(content, 'img').length, 0);
});

test('raw HTML is never interpreted, Markdown images never load, and only safe external links become anchors', () => {
    const content = render(
        '<script>alert(1)</script> <img src=x onerror=run()>\n\n![Tracking pixel](https://example.com/track)\n\n[Read more](https://example.com/notes) [Email](mailto:team@example.com) [Unsafe](javascript:alert) [Data](data:text/html,test) [Local](file:///private/data) [Relative](/local)'
    );
    const links = tags(content, 'a');
    assert.equal(links.length, 2);
    assert.deepEqual(
        links.map(link => link.attributes.get('href')),
        ['https://example.com/notes', 'mailto:team@example.com']
    );
    for (const link of links) {
        assert.equal(link.attributes.get('target'), '_blank');
        assert.equal(link.attributes.get('rel'), 'noopener noreferrer');
    }
    assert.equal(tags(content, 'script').length, 0);
    assert.equal(tags(content, 'img').length, 0);
    assert.match(content.textContent, /<script>alert\(1\)<\/script>/);
    assert.match(content.textContent, /Tracking pixel/);
    assert.match(content.textContent, /Unsafe.*Data.*Local.*Relative/);
    assert.equal(
        descendants(content).some(node => [...node.attributes.keys()].some(name => /^on/i.test(name))),
        false
    );
});

test('malformed Markdown remains readable and replacing a response removes previous DOM', () => {
    const content = render('An **unfinished marker and [unfinished link.\n\n`unfinished code');
    assert.match(content.textContent, /An \*\*unfinished marker and \[unfinished link\./);
    assert.match(content.textContent, /`unfinished code/);
    renderAiMarkdown(content, 'A replacement answer.');
    assert.equal(content.children.length, 1);
    assert.equal(content.textContent, 'A replacement answer.');
});
