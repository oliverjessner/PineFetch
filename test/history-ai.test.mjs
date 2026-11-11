import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHistoryAiView } from '../src/history-ai-view.js';
import { codexStatus, createCodexDiagnostics } from '../src/codex-diagnostics.js';

class Element extends EventTarget {
    constructor(tagName = 'div') {
        super();
        this.tagName = tagName.toUpperCase();
    }
    children = [];
    attributes = new Map();
    text = '';
    style = {
        setProperty(name, value) {
            this[name] = value;
        },
    };
    get textContent() {
        return this.text + this.children.map(child => child.textContent).join('');
    }
    set textContent(value) {
        this.text = value;
        this.children = [];
    }
    get lastElementChild() {
        return this.children.at(-1);
    }
    value = '';
    hidden = false;
    disabled = false;
    scrollHeight = 0;
    scrollTop = 0;
    clientHeight = 0;
    classes = new Set();
    classList = {
        add: name => this.classes.add(name),
        remove: name => this.classes.delete(name),
        toggle: (name, enabled) => (enabled ? this.classes.add(name) : this.classes.delete(name)),
    };
    append(...children) {
        this.children.push(...children);
    }
    replaceChildren(...children) {
        this.text = '';
        this.children = [...children];
    }
    removeChild(child) {
        this.children.splice(this.children.indexOf(child), 1);
    }
    focus() {
        this.focused = true;
    }
    getBoundingClientRect() {
        return { height: this.height || 0 };
    }
    setAttribute(name, value) {
        this.attributes.set(name, value);
    }
    click() {
        this.dispatchEvent(new Event('click'));
    }
}
const settle = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => {
    let resolve;
    let reject;
    const promise = new Promise((done, fail) => {
        resolve = done;
        reject = fail;
    });
    return { promise, resolve, reject };
};
const summary = {
    quick_summary: 'The transcript describes an experiment.',
    key_takeaways: ['The experiment was reproducible.'],
};
const conversation = [
    { role: 'user', content: 'What happened?' },
    { role: 'assistant', content: 'The experiment was repeated.' },
];
const keydown = (element, options = {}) => {
    const event = new Event('keydown', { cancelable: true });
    Object.assign(event, { key: 'Enter', ...options });
    element.dispatchEvent(event);
    return event;
};
const setup = (t, overrides = {}) => {
    const original = globalThis.document;
    globalThis.document = {
        createElement: tagName => new Element(tagName),
        createTextNode: textContent => ({ textContent }),
    };
    t.after(() => {
        globalThis.document = original;
    });
    const elements = new Map();
    const root = new Element();
    root.querySelector = selector => {
        if (!elements.has(selector)) elements.set(selector, new Element());
        return elements.get(selector);
    };
    const calls = [];
    const logs = [];
    const api = {
        available: true,
        getCodexDiagnostics: async () => ({ status: 'chatgpt', ready: true }),
        getVideoAi: async () => ({ transcript_available: true, summary: null, messages: [] }),
        analyzeVideo: async args => {
            calls.push(['analyze', args]);
            return summary;
        },
        askVideo: async args => {
            calls.push(['ask', args]);
            return { messages: conversation };
        },
        cancelVideoAi: async args => {
            calls.push(['cancel', args]);
        },
        ...overrides,
    };
    const readVideoAi = api.getVideoAi;
    api.getVideoAi = async args => ({ transcript_hash: 'a'.repeat(64), ...(await readVideoAi(args)) });
    const view = createHistoryAiView({ root, api, appendLog: (...args) => logs.push(args) });
    view.bindEvents();
    view.reset('video-a');
    const ui = name => root.querySelector(`[data-ai-${name}]`);
    const question = text => {
        ui('question').value = text;
        ui('question').dispatchEvent(new Event('input'));
    };
    const send = () => ui('form').dispatchEvent(new Event('submit', { cancelable: true }));
    return { view, ui, calls, logs, question, send };
};

test('Ask this Video is available with a transcript and authenticated Codex before any summary exists', async t => {
    const { view, ui, calls, question, send } = setup(t);
    await view.load();
    await settle();
    assert.equal(ui('summary').hidden, true);
    assert.equal(ui('question').disabled, false);
    assert.equal(ui('analyze-label').textContent, 'Analyze Video');
    assert.deepEqual(calls, []);
    question('What happened?');
    assert.equal(ui('send').disabled, false);
    send();
    assert.equal(ui('question').value, '');
    assert.equal(ui('question').disabled, true);
    assert.equal(ui('send').classes.has('oj-button-loading'), true);
    await settle();
    assert.equal(calls.length, 1);
    assert.equal(calls[0][0], 'ask');
    assert.equal(calls[0][1].historyId, 'video-a');
    assert.equal(calls[0][1].question, 'What happened?');
    assert.equal(calls[0][1].transcriptHash, 'a'.repeat(64));
    assert.equal(ui('messages').children.length, 2);
    assert.equal(ui('summary').hidden, true);
    assert.equal(ui('question').disabled, false);
    assert.equal(ui('send').classes.has('oj-button-loading'), false);
});

test('summary and chat complete independently while only a pending answer locks the composer', async t => {
    const analysis = deferred();
    const answer = deferred();
    const { view, ui, question, send } = setup(t, {
        getVideoAi: async () => ({ transcript_available: true, summary: null, messages: conversation }),
        analyzeVideo: () => analysis.promise,
        askVideo: () => answer.promise,
    });
    await view.load();
    ui('analyze').click();
    assert.equal(ui('question').disabled, false);
    question('Follow-up');
    send();
    assert.equal(ui('cancel-summary').hidden, false);
    assert.equal(ui('cancel-chat').hidden, false);
    assert.equal(ui('question').value, '');
    assert.equal(ui('question').disabled, true);
    analysis.resolve(summary);
    await settle();
    assert.equal(ui('summary-text').textContent, summary.quick_summary);
    assert.equal(ui('question').value, '');
    assert.equal(ui('question').disabled, true);
    assert.equal(ui('messages').children.length, 3);
    assert.equal(ui('cancel-chat').hidden, false);
    answer.resolve({
        messages: [
            ...conversation,
            { role: 'user', content: 'Follow-up' },
            { role: 'assistant', content: 'More transcript evidence.' },
        ],
    });
    await settle();
    assert.equal(ui('messages').children.length, 4);
    assert.equal(ui('question').value, '');
    assert.equal(ui('question').disabled, false);
    question('My next question');
    const messages = ui('messages').children;
    ui('analyze').click();
    await settle();
    assert.equal(ui('messages').children, messages);
    assert.equal(ui('messages').children.length, 4);
    assert.equal(ui('question').value, 'My next question');
});

test('Enter sends, Shift+Enter keeps a newline, and IME confirmation never submits', async t => {
    const { view, ui, calls, question } = setup(t);
    await view.load();
    question('A multiline question');
    assert.equal(keydown(ui('question'), { shiftKey: true }).defaultPrevented, false);
    assert.equal(keydown(ui('question'), { isComposing: true }).defaultPrevented, false);
    assert.equal(keydown(ui('question'), { keyCode: 229 }).defaultPrevented, false);
    assert.equal(calls.length, 0);
    const icon = new Element('i');
    ui('send').append(icon);
    assert.equal(keydown(ui('question')).defaultPrevented, true);
    assert.equal(ui('send').children[0], icon);
    assert.equal(ui('send').attributes.get('aria-label'), 'Preparing answer');
    assert.equal(ui('send').attributes.get('aria-busy'), 'true');
    assert.equal(ui('send').classes.has('oj-button-loading'), true);
    assert.equal(ui('question').value, '');
    assert.equal(ui('question').disabled, true);
    await settle();
    assert.equal(calls.length, 1);
    assert.equal(ui('send').children[0], icon);
    assert.equal(ui('send').attributes.get('aria-label'), 'Send question');
    assert.equal(ui('send').attributes.get('aria-busy'), 'false');
    assert.equal(ui('send').classes.has('oj-button-loading'), false);
    assert.equal(ui('question').value, '');
    assert.equal(ui('question').disabled, false);
});

test('retry uses the failed question while preserving an independently edited draft', async t => {
    let attempts = 0;
    const submitted = [];
    const retried = deferred();
    const { view, ui, question, send } = setup(t, {
        askVideo: async args => {
            submitted.push(args.question);
            if (!attempts++) throw new Error('timed out');
            return retried.promise;
        },
    });
    await view.load();
    question('Retry this question');
    send();
    await settle();
    assert.equal(ui('retry-chat').hidden, false);
    assert.equal(ui('question').value, '');
    assert.equal(ui('question').disabled, false);
    question('Keep my next question');
    ui('retry-chat').click();
    assert.equal(ui('question').value, 'Keep my next question');
    assert.equal(ui('question').disabled, true);
    assert.equal(ui('send').classes.has('oj-button-loading'), true);
    retried.resolve({ messages: conversation });
    await settle();
    assert.deepEqual(submitted, ['Retry this question', 'Retry this question']);
    assert.equal(ui('question').value, 'Keep my next question');
    assert.equal(ui('question').disabled, false);
    assert.equal(ui('send').classes.has('oj-button-loading'), false);
    assert.equal(ui('retry-chat').hidden, true);
    assert.equal(ui('chat-status').textContent, '');
});

test('Stop keeps the cleared composer locked until the answer request actually settles', async t => {
    const answer = deferred();
    const { view, ui, calls, question, send } = setup(t, {
        askVideo: () => answer.promise,
    });
    await view.load();
    question('Cancel this question');
    send();
    ui('cancel-chat').click();
    await settle();
    assert.equal(calls.filter(([command]) => command === 'cancel').length, 1);
    assert.equal(ui('question').value, '');
    assert.equal(ui('question').disabled, true);
    assert.equal(ui('send').classes.has('oj-button-loading'), true);
    assert.equal(ui('cancel-chat').hidden, false);
    answer.reject(new Error('cancelled'));
    await settle();
    assert.equal(ui('question').disabled, false);
    assert.equal(ui('send').classes.has('oj-button-loading'), false);
    assert.equal(ui('send').attributes.get('aria-busy'), 'false');
    assert.equal(ui('cancel-chat').hidden, true);
    assert.equal(ui('retry-chat').hidden, false);
});

test('new messages preserve an older reading position and existing message elements', async t => {
    const answer = deferred();
    const { view, ui, question, send } = setup(t, {
        getVideoAi: async () => ({ transcript_available: true, summary: null, messages: conversation }),
        askVideo: () => answer.promise,
    });
    await view.load();
    const firstMessage = ui('messages').children[0];
    ui('messages').scrollHeight = 2000;
    ui('messages').clientHeight = 300;
    ui('messages').scrollTop = 100;
    ui('messages').dispatchEvent(new Event('scroll'));
    question('Follow-up');
    send();
    assert.equal(ui('messages').scrollTop, 100);
    answer.resolve({
        messages: [...conversation, { role: 'user', content: 'Follow-up' }, { role: 'assistant', content: 'Answer' }],
    });
    await settle();
    assert.equal(ui('messages').scrollTop, 100);
    assert.equal(ui('messages').children[0], firstMessage);
    ui('show-summary').click();
    ui('show-chat').click();
    assert.equal(ui('messages').scrollTop, 100);
    assert.equal(ui('workspace').attributes.get('data-ai-active-panel'), 'chat');
});

test('new messages follow the bottom when the reader is already there', async t => {
    const { view, ui, question, send } = setup(t, {
        getVideoAi: async () => ({ transcript_available: true, summary: null, messages: conversation }),
    });
    await view.load();
    ui('messages').scrollHeight = 2000;
    ui('messages').clientHeight = 300;
    ui('messages').scrollTop = 1700;
    ui('messages').dispatchEvent(new Event('scroll'));
    question('What happened?');
    send();
    assert.equal(ui('messages').scrollTop, 2000);
    await settle();
    assert.equal(ui('messages').scrollTop, 2000);
});

test('assistant messages render Markdown and offer safe copy feedback without mutating the conversation', async t => {
    const original = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
    const copied = [];
    let failCopy = false;
    Object.defineProperty(globalThis, 'navigator', {
        configurable: true,
        value: {
            clipboard: {
                writeText: async text => {
                    if (failCopy) throw new Error('permission denied');
                    copied.push(text);
                },
            },
        },
    });
    t.after(() => {
        if (original) Object.defineProperty(globalThis, 'navigator', original);
        else delete globalThis.navigator;
    });
    const content = 'A **clear** answer.\n\n- First insight\n- Second insight';
    const { view, ui } = setup(t, {
        getVideoAi: async () => ({
            transcript_available: true,
            summary: null,
            messages: [{ role: 'assistant', content }],
        }),
    });
    await view.load();
    const message = ui('messages').children[0];
    assert.equal(message.className, 'pinefetch-ai-message pinefetch-ai-message-assistant');
    assert.deepEqual(
        message.children[1].children.map(child => child.tagName),
        ['P', 'UL']
    );
    const copy = message.children[2].children[0];
    copy.click();
    await settle();
    assert.deepEqual(copied, [content]);
    assert.equal(ui('chat-status').textContent, 'Answer copied.');
    failCopy = true;
    copy.click();
    await settle();
    assert.equal(ui('chat-status').textContent, 'Answer could not be copied. Try again.');
    assert.equal(ui('retry-chat').hidden, true);
    assert.equal(ui('messages').children[0], message);
});

test('all cached takeaways remain visible and the composer grows to its measured content', async t => {
    const legacy = { ...summary, key_takeaways: Array.from({ length: 12 }, (_, index) => `Insight ${index + 1}`) };
    const { view, ui, question } = setup(t, {
        getVideoAi: async () => ({ transcript_available: true, summary: legacy, messages: [] }),
    });
    await view.load();
    assert.equal(ui('takeaways').children.length, 12);
    ui('question').scrollHeight = 120;
    question('A longer\nmultiline\nquestion');
    assert.equal(ui('question').style.height, '120px');
    ui('show-summary').click();
    assert.equal(ui('show-summary').attributes.get('aria-pressed'), 'true');
    assert.equal(ui('workspace').attributes.get('data-ai-active-panel'), 'summary');
    view.reset('video-b');
    assert.equal(ui('workspace').attributes.get('data-ai-active-panel'), 'chat');
    assert.equal(ui('question').value, '');
});

test('composer growth reserves visible message space and feedback inside the actual chat height', async t => {
    const original = globalThis.getComputedStyle;
    globalThis.getComputedStyle = element => element.metrics || {};
    t.after(() => {
        if (original) globalThis.getComputedStyle = original;
        else delete globalThis.getComputedStyle;
    });
    const { view, ui, question } = setup(t);
    await view.load();
    ui('chat').clientHeight = 220;
    ui('chat').metrics = { rowGap: '8px' };
    ui('chat-header').height = 32;
    ui('chat-feedback').height = 24;
    ui('form').metrics = { rowGap: '8px' };
    ui('question').metrics = { minHeight: '44px' };
    ui('question').scrollHeight = 300;
    ui('messages').metrics = { lineHeight: '20px', fontSize: '14px', paddingTop: '8px', paddingBottom: '12px' };
    ui('messages').scrollHeight = 2000;
    ui('messages').clientHeight = 100;
    ui('messages').scrollTop = 50;
    question('A long draft');
    assert.equal(ui('question').style['--pinefetch-ai-composer-space'], '96px');
    assert.equal(ui('messages').scrollTop, 50);
    ui('chat-feedback').height = 0;
    question('A longer draft');
    assert.equal(ui('question').style['--pinefetch-ai-composer-space'], '128px');
    assert.equal(ui('messages').scrollTop, 50);
});

test('a summary failure leaves chat usable and does not overwrite its independent error state', async t => {
    const { view, ui, question, send } = setup(t, {
        analyzeVideo: async () => {
            throw new Error('malformed JSON');
        },
        askVideo: async () => {
            throw new Error('timed out');
        },
        getVideoAi: async () => ({ transcript_available: true, summary, messages: conversation }),
    });
    await view.load();
    question('Keep this draft');
    send();
    await settle();
    const chatError = ui('chat-status').textContent;
    ui('analyze').click();
    await settle();
    assert.match(ui('summary-status').textContent, /incomplete or unsupported/);
    assert.equal(ui('chat-status').textContent, chatError);
    assert.match(chatError, /timed out/);
    assert.equal(ui('question').disabled, false);
    assert.equal(ui('question').value, '');
    assert.equal(ui('retry-chat').hidden, false);
    assert.equal(ui('send').classes.has('oj-button-loading'), false);
    assert.equal(ui('summary-text').textContent, summary.quick_summary);
    assert.equal(ui('messages').children.length, 2);
});

const codexFailureCases = [
    [
        'configuration',
        'Codex CLI rejected PineFetch’s AI settings. Update Codex CLI and PineFetch, then check Diagnostics again.',
        'Codex CLI rejected PineFetch’s AI settings. Update Codex CLI and PineFetch, then check Settings → Diagnostics again.',
    ],
    [
        'usage limit',
        'Codex usage limit reached. Check your ChatGPT or API usage limits and try again later.',
        'Codex usage limit reached. Check your ChatGPT or API usage limits and try again later.',
    ],
    [
        'network',
        'Codex could not reach OpenAI. Check your connection, proxy, or VPN and try again.',
        'Codex could not reach OpenAI. Check your connection, proxy, or VPN and try again.',
    ],
    [
        'account access',
        'Codex access was denied. Check your ChatGPT or API account and model access, then try again.',
        'Codex access was denied. Check your ChatGPT or API account and model access, then try again.',
    ],
    [
        'service availability',
        'Codex service is temporarily unavailable. Try again later.',
        'Codex service is temporarily unavailable. Try again later.',
    ],
    [
        'supporting evidence',
        'Codex did not supply valid supporting transcript evidence. The result was not saved.',
        'Codex could not verify its answer against the transcript. Nothing was saved. Try again.',
    ],
    [
        'rejected tool use',
        'Codex attempted to use a tool. The result was rejected; check your Codex configuration.',
        'Codex tried to use a tool and the result was rejected. Update Codex CLI and check Settings → Diagnostics before trying again.',
    ],
    [
        'temporary storage',
        'PineFetch could not prepare private temporary storage for Codex.',
        'PineFetch could not prepare private temporary storage. Check free disk space and temporary-folder permissions, then try again.',
    ],
];

for (const [category, backendMessage, expectedMessage] of codexFailureCases) {
    test(`a ${category} failure shows a safe actionable cause without replacing saved data`, async t => {
        const { view, ui, logs, question } = setup(t, {
            analyzeVideo: async () => {
                throw backendMessage;
            },
            getVideoAi: async () => ({ transcript_available: true, summary, messages: conversation }),
        });
        await view.load();
        question('Keep this question');
        const messages = ui('messages').children;
        ui('analyze').click();
        await settle();
        assert.equal(ui('summary-status').textContent, expectedMessage);
        assert.deepEqual(logs, [[`[AI summary] ${expectedMessage}`, true]]);
        assert.equal(ui('summary-status').classes.has('oj-status-error'), true);
        assert.equal(ui('summary-text').textContent, summary.quick_summary);
        assert.equal(ui('messages').children, messages);
        assert.equal(ui('question').value, 'Keep this question');
        assert.equal(ui('question').disabled, false);
        assert.equal(ui('send').disabled, false);
        assert.equal(ui('chat-status').textContent, '');
        assert.equal(ui('analyze').disabled, false);
    });
}

test('a safe chat failure is logged without disturbing a summary still being generated', async t => {
    const analysis = deferred();
    const backendMessage = 'Codex could not reach OpenAI. Check your connection, proxy, or VPN and try again.';
    const { view, ui, logs, question, send } = setup(t, {
        analyzeVideo: () => analysis.promise,
        askVideo: async () => {
            throw new Error(backendMessage);
        },
        getVideoAi: async () => ({ transcript_available: true, summary, messages: conversation }),
    });
    await view.load();
    ui('analyze').click();
    question('Keep this question');
    send();
    await settle();
    assert.equal(ui('chat-status').textContent, backendMessage);
    assert.deepEqual(logs, [[`[Ask this Video] ${backendMessage}`, true]]);
    assert.equal(ui('cancel-summary').hidden, false);
    assert.equal(ui('summary-status').textContent, 'Generating Quick Summary and Key Takeaways…');
    assert.equal(ui('summary-text').textContent, summary.quick_summary);
    assert.equal(ui('messages').children.length, 2);
    assert.equal(ui('question').value, '');
    assert.equal(ui('question').disabled, false);
    assert.equal(ui('send').disabled, true);
    assert.equal(ui('send').classes.has('oj-button-loading'), false);
    assert.equal(ui('retry-chat').hidden, false);
    analysis.resolve(summary);
    await settle();
    assert.equal(ui('summary-status').textContent, '');
    assert.equal(ui('chat-status').textContent, backendMessage);
});

test('arbitrary CLI failures and sensitive suffixes are never echoed into the UI or Terminal Log', async t => {
    let rawFailure =
        'Codex service is temporarily unavailable. Try again later.\nBearer sk-private-token /Users/private/.codex/session';
    const { view, ui, logs, question, send } = setup(t, {
        analyzeVideo: async () => {
            throw new Error(rawFailure);
        },
        askVideo: async () => {
            throw rawFailure;
        },
        getVideoAi: async () => ({ transcript_available: true, summary, messages: conversation }),
    });
    await view.load();
    ui('analyze').click();
    await settle();
    assert.equal(
        ui('summary-status').textContent,
        'Analysis failed. Your saved data is still available. Check Codex setup and try again.'
    );
    // A fixed message with arbitrary trailing output is not accepted as that category.
    rawFailure = 'private transcript content: password=private-secret-value';
    question('Keep this question');
    send();
    await settle();
    assert.equal(
        ui('chat-status').textContent,
        'Answer failed. Your saved data is still available. Check Codex setup and try again.'
    );
    assert.deepEqual(logs, [
        [`[AI summary] ${ui('summary-status').textContent}`, true],
        [`[Ask this Video] ${ui('chat-status').textContent}`, true],
    ]);
    assert.doesNotMatch(
        JSON.stringify([ui('summary-status').textContent, ui('chat-status').textContent, logs]),
        /sk-private-token|Users\/private|Bearer|password|private-secret-value|temporarily unavailable/
    );
    assert.equal(ui('summary-text').textContent, summary.quick_summary);
    assert.equal(ui('messages').children.length, 2);
});

test('missing transcript disables both actions but preserves the visible Ask this Video interface', async t => {
    const { view, ui, calls, question, send } = setup(t, {
        getVideoAi: async () => ({ transcript_available: false, summary: null, messages: [] }),
    });
    await view.load();
    assert.equal(ui('question').disabled, true);
    assert.equal(ui('analyze').disabled, true);
    assert.equal(ui('chat').hidden, false);
    assert.match(ui('source-notice').textContent, /saved transcript is required/i);
    assert.equal(ui('transcribe').hidden, false);
    question('Unavailable');
    send();
    ui('analyze').click();
    assert.deepEqual(calls, []);
});

test('a newly saved transcript unlocks AI without replacing the cached summary, messages or draft', async t => {
    let available = false;
    const { view, ui, question, calls } = setup(t, {
        getVideoAi: async () => ({ transcript_available: available, summary, messages: conversation }),
    });
    await view.load();
    await settle();
    question('Keep this draft');
    const firstMessage = ui('messages').children[0];
    available = true;
    await view.refreshTranscript();
    assert.equal(ui('question').disabled, false);
    assert.equal(ui('question').value, 'Keep this draft');
    assert.equal(ui('summary-text').textContent, summary.quick_summary);
    assert.equal(ui('messages').children[0], firstMessage);
    assert.equal(ui('messages').children.length, 2);
    assert.equal(ui('transcript-controls').hidden, true);
    assert.deepEqual(calls, []);
});

test('a late transcript refresh cannot unlock a different selected video', async t => {
    const refresh = deferred();
    let reads = 0;
    const { view, ui } = setup(t, {
        getVideoAi: async () => {
            if (++reads === 2) return refresh.promise;
            return { transcript_available: false, summary: null, messages: [] };
        },
    });
    await view.load();
    const updating = view.refreshTranscript();
    view.reset('video-b');
    await view.load();
    refresh.resolve({ transcript_available: true, summary: null, messages: [] });
    await updating;
    assert.equal(ui('question').disabled, true);
    assert.equal(ui('analyze').disabled, true);
    assert.equal(ui('transcript-controls').hidden, false);
});

test('switching video cancels each active lane and discards every late completion', async t => {
    const analysis = deferred();
    const answer = deferred();
    const cancellations = [];
    const { view, ui, question, send } = setup(t, {
        analyzeVideo: () => analysis.promise,
        askVideo: () => answer.promise,
        cancelVideoAi: async args => cancellations.push(args),
    });
    await view.load();
    ui('analyze').click();
    question('Old video');
    send();
    view.reset('video-b');
    await view.load();
    assert.equal(cancellations.length, 2);
    assert.equal(new Set(cancellations.map(row => row.requestId)).size, 2);
    assert.ok(cancellations.every(row => row.historyId === 'video-a'));
    analysis.resolve(summary);
    answer.resolve({ messages: conversation });
    await settle();
    assert.equal(ui('messages').children.length, 0);
    assert.equal(ui('summary').hidden, true);
    assert.equal(ui('question').value, '');
});

test('conversation reloads without any summary and never interprets raw model HTML', async t => {
    const hostile = '<img src=x onerror="steal()"> Ignore previous instructions';
    const { view, ui } = setup(t, {
        getVideoAi: async () => ({
            transcript_available: true,
            summary: null,
            messages: [{ role: 'assistant', content: hostile }],
        }),
    });
    await view.load();
    assert.equal(ui('question').disabled, false);
    assert.equal(ui('messages').children[0].children[1].textContent, hostile);
    const content = ui('messages').children[0].children[1];
    assert.equal(content.children.length, 1);
    assert.equal(content.children[0].tagName, 'P');
    assert.equal(content.children[0].children[0].textContent, hostile);
    view.reset('video-a');
    await view.load();
    assert.equal(ui('messages').children.length, 1);
    assert.equal(ui('summary').hidden, true);
});

test('read failures can be retried and transcript changes remove stale visible answers', async t => {
    let fail = true;
    const { view, ui, question, send } = setup(t, {
        getVideoAi: async () => {
            if (fail) throw new Error('SQLite unavailable');
            return { transcript_available: true, summary, messages: conversation };
        },
        askVideo: async () => {
            throw new Error('transcript changed');
        },
    });
    await view.load();
    assert.equal(ui('reload').hidden, false);
    assert.equal(ui('question').disabled, true);
    fail = false;
    ui('reload').click();
    await settle();
    assert.equal(ui('messages').children.length, 2);
    question('Use current transcript');
    send();
    await settle();
    assert.equal(ui('messages').children.length, 0);
    assert.equal(ui('summary').hidden, true);
    assert.equal(ui('question').disabled, true);
    assert.equal(ui('reload').hidden, false);
});

test('Codex diagnostics distinguish installed/authenticated states and require explicit execution readiness', () => {
    for (const status of ['not_installed', 'unauthenticated', 'unknown_auth', 'failed']) {
        assert.equal(codexStatus({ status, ready: true }).ready, false);
        assert.match(codexStatus({ status }).help, /Terminal|Codex/);
    }
    assert.equal(codexStatus({ status: 'chatgpt' }).ready, false);
    assert.equal(codexStatus({ status: 'chatgpt', ready: true }).ready, true);
    assert.equal(codexStatus({ status: 'api_key', ready: true }).ready, true);
    assert.equal(codexStatus({ status: 'api_key', ready: true }).label, 'Authenticated using API key');
    assert.match(codexStatus({ status: 'chatgpt', ready: false }).help, /compatible/);
});

test('setup failure keeps both lanes disabled and Check Again enables chat without analysis', async t => {
    let connected = false;
    const { view, ui } = setup(t, {
        getCodexDiagnostics: async () => ({ status: connected ? 'chatgpt' : 'unauthenticated', ready: connected }),
    });
    await view.load();
    assert.equal(ui('question').disabled, true);
    assert.match(ui('setup').textContent, /Settings → Diagnostics/);
    assert.equal(ui('setup-section').hidden, false);
    connected = true;
    ui('check').click();
    await settle();
    assert.equal(ui('question').disabled, false);
    assert.equal(ui('summary').hidden, true);
    assert.equal(ui('setup-section').hidden, true);
});

test('an unreadable saved summary does not block chat and an unreadable chat does not block analysis', async t => {
    let brokenChat = false;
    const { view, ui, question, send, calls } = setup(t, {
        getVideoAi: async () => ({
            transcript_available: true,
            summary: null,
            messages: [],
            summary_error: brokenChat ? null : 'corrupt stored summary',
            chat_error: brokenChat ? 'corrupt stored chat' : null,
        }),
    });
    await view.load();
    assert.equal(ui('question').disabled, false);
    assert.match(ui('summary-status').textContent, /saved summary could not be loaded/);
    question('I can ask without a saved summary');
    send();
    await settle();
    assert.equal(calls.at(-1)[0], 'ask');
    brokenChat = true;
    view.reset('video-b');
    await view.load();
    assert.equal(ui('question').disabled, true);
    assert.equal(ui('analyze').disabled, false);
    assert.match(ui('chat-status').textContent, /saved conversation could not be loaded/);
    ui('analyze').click();
    await settle();
    assert.equal(calls.at(-1)[0], 'analyze');
    assert.match(ui('chat-status').textContent, /saved conversation could not be loaded/);
});

test('an explicit API-key login also enables transcript-only chat without introducing a key input', async t => {
    const { view, ui } = setup(t, {
        getCodexDiagnostics: async () => ({ status: 'api_key', ready: true }),
    });
    await view.load();
    assert.equal(ui('question').disabled, false);
    assert.equal(ui('setup').textContent, '');
    assert.equal(ui('setup-section').hidden, true);
});

test('unsupported transcript size remains actionable and does not interfere with saved chat', async t => {
    const { view, ui } = setup(t, {
        getVideoAi: async () => ({ transcript_available: true, summary: null, messages: conversation }),
        analyzeVideo: async () => {
            throw new Error('transcript exceeds bounded context limit');
        },
    });
    await view.load();
    ui('analyze').click();
    await settle();
    assert.match(ui('summary-status').textContent, /exceeds the supported limit/);
    assert.equal(ui('question').disabled, false);
    assert.equal(ui('messages').children.length, 2);
});

test('Diagnostics Check Again deduplicates running checks and recovers after failure', async () => {
    const elements = new Map();
    const root = new Element();
    root.querySelector = selector => {
        if (!elements.has(selector)) elements.set(selector, new Element());
        return elements.get(selector);
    };
    const probe = deferred();
    let calls = 0;
    const view = createCodexDiagnostics({
        root,
        api: {
            available: true,
            getCodexDiagnostics: async () => {
                if (!calls++) return probe.promise;
                return { status: 'api_key', ready: true, version: '0.164.0' };
            },
        },
    });
    const first = view.refresh();
    assert.equal(view.refresh(), first);
    probe.reject(new Error('private credential-path output'));
    await first;
    assert.equal(root.querySelector('[data-codex-status]').textContent, 'Check failed — try again');
    assert.doesNotMatch(root.querySelector('[data-codex-status]').textContent, /credential/);
    await view.refresh();
    assert.equal(root.querySelector('[data-codex-status]').textContent, 'Connected (API key)');
    assert.equal(root.querySelector('[data-codex-version]').textContent, '· v0.164.0');
    assert.equal(root.querySelector('[data-codex-version]').hidden, false);
});
