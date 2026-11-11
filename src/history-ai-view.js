import { codexStatus } from './codex-diagnostics.js';
import { renderAiMarkdown } from './ai-markdown.js';

// Match only fixed backend messages; CLI output may contain private data.
const codexFailures = new Map([
    [
        'Codex CLI rejected PineFetch’s AI settings. Update Codex CLI and PineFetch, then check Diagnostics again.',
        'Codex CLI rejected PineFetch’s AI settings. Update Codex CLI and PineFetch, then check Settings → Diagnostics again.',
    ],
    [
        'Codex usage limit reached. Check your ChatGPT or API usage limits and try again later.',
        'Codex usage limit reached. Check your ChatGPT or API usage limits and try again later.',
    ],
    [
        'Codex could not reach OpenAI. Check your connection, proxy, or VPN and try again.',
        'Codex could not reach OpenAI. Check your connection, proxy, or VPN and try again.',
    ],
    [
        'Codex access was denied. Check your ChatGPT or API account and model access, then try again.',
        'Codex access was denied. Check your ChatGPT or API account and model access, then try again.',
    ],
    [
        'Codex service is temporarily unavailable. Try again later.',
        'Codex service is temporarily unavailable. Try again later.',
    ],
    [
        'Codex did not supply valid supporting transcript evidence. The result was not saved.',
        'Codex could not verify its answer against the transcript. Nothing was saved. Try again.',
    ],
    [
        'Codex attempted to use a tool. The result was rejected; check your Codex configuration.',
        'Codex tried to use a tool and the result was rejected. Update Codex CLI and check Settings → Diagnostics before trying again.',
    ],
    [
        'PineFetch could not prepare private temporary storage for Codex.',
        'PineFetch could not prepare private temporary storage. Check free disk space and temporary-folder permissions, then try again.',
    ],
]);

const errorMessage = (error, action) => {
    const message = typeof error === 'string' ? error : error?.message || '';
    const codexFailure = codexFailures.get(message);
    if (codexFailure) return codexFailure;
    if (/cancel/i.test(message)) return `${action} cancelled. Your saved summary and messages are unchanged.`;
    if (/timeout|timed out/i.test(message)) return `${action} timed out. Try again when Codex is available.`;
    if (/transcript.*chang|revision|stale/i.test(message))
        return 'The transcript changed. Reopen this tab to load its current summary and conversation.';
    if (/json|schema|malformed|invalid.*output|empty.*answer|unsupported.*answer/i.test(message))
        return `${action} returned an incomplete or unsupported response. Nothing was saved. Try again.`;
    if (/too (?:long|large)|exceeds|maximum|context limit|size limit/i.test(message))
        return `${action} could not start because the transcript or question exceeds the supported limit. Use a shorter transcript or question.`;
    if (/policy|sandbox|isolation|restriction/i.test(message))
        return 'Codex could not enforce the required read-only execution restrictions. Update Codex CLI and check Settings → Diagnostics before trying again.';
    if (/auth|login|installed|executable|compatib/i.test(message))
        return `${action} could not start. Check Codex CLI in Settings → Diagnostics, then check again here.`;
    return `${action} failed. Your saved data is still available. Check Codex setup and try again.`;
};

const requestIdentity = () =>
    globalThis.crypto?.randomUUID?.() || `ai-${Date.now()}-${Math.random().toString(36).slice(2)}`;

export const createHistoryAiView = ({ root, api, appendLog = () => {}, onTranscribe = null }) => {
    const element = name => root.querySelector(`[data-ai-${name}]`);
    const ui = Object.fromEntries(
        [
            'setup',
            'setup-section',
            'check',
            'load-status',
            'reload',
            'summary',
            'summary-text',
            'takeaways',
            'summary-status',
            'analyze',
            'cancel-summary',
            'copy',
            'chat',
            'messages',
            'question',
            'send',
            'cancel-chat',
            'chat-status',
            'form',
            'source-notice',
            'transcript-controls',
            'transcribe',
            'workspace',
            'show-chat',
            'show-summary',
            'summary-panel',
            'summary-empty',
            'chat-empty',
            'analyze-label',
            'retry-chat',
            'retry-summary',
            'chat-header',
            'chat-feedback',
        ].map(name => [name, element(name)])
    );
    const state = {
        id: null,
        revision: 0,
        loaded: false,
        reading: false,
        transcript: false,
        transcriptHash: null,
        diagnostics: null,
        checking: false,
        summary: null,
        messages: [],
        summaryRequest: null,
        chatRequest: null,
        summaryError: '',
        chatError: '',
        loadError: '',
        summaryNotice: '',
        pendingQuestion: '',
        summaryReadError: false,
        chatReadError: false,
        failedQuestion: '',
        chatNotice: '',
        chatCopyError: '',
        activePanel: 'chat',
    };
    let displayedMessages;
    let displayedPending;
    let displayedSummary;
    let messageKeys = [];
    let chatAtEnd = true;
    let transcriptRefreshPending = false;
    let transcriptRefreshRequest = 0;
    const current = (id, revision) => id === state.id && revision === state.revision;
    const ready = () =>
        Boolean(
            api.available &&
            state.id &&
            state.loaded &&
            state.transcript &&
            state.transcriptHash &&
            codexStatus(state.diagnostics).ready
        );
    const invalidateChangedTranscript = error => {
        const message = typeof error === 'string' ? error : error?.message || '';
        if (!/transcript.*chang|revision|stale/i.test(message)) return;
        state.loaded = false;
        state.transcript = false;
        state.transcriptHash = null;
        state.summary = null;
        state.messages = [];
        state.loadError =
            'The transcript changed. Reload to start a conversation or analysis for the current transcript.';
    };
    const updateButton = (button, busy) => {
        button.classList.toggle('oj-button-loading', busy);
        button.setAttribute('aria-busy', String(busy));
    };
    const resizeQuestion = () => {
        const scrollTop = ui.messages.scrollTop;
        const nearEnd =
            ui.messages.clientHeight > 0
                ? ui.messages.scrollHeight - scrollTop - ui.messages.clientHeight < 80
                : chatAtEnd;
        if (ui.chat.clientHeight > 0 && typeof getComputedStyle === 'function') {
            const chatStyle = getComputedStyle(ui.chat);
            const questionStyle = getComputedStyle(ui.question);
            const messagesStyle = getComputedStyle(ui.messages);
            const number = value => Number.parseFloat(value) || 0;
            const minimum = number(questionStyle.minHeight);
            const messageReserve = Math.max(
                minimum,
                (number(messagesStyle.lineHeight) || number(messagesStyle.fontSize) * 1.5) +
                    number(messagesStyle.paddingTop) +
                    number(messagesStyle.paddingBottom)
            );
            const feedbackHeight = ui['chat-feedback'].getBoundingClientRect().height;
            const formGap = feedbackHeight > 0 ? number(getComputedStyle(ui.form).rowGap) : 0;
            const allowance =
                ui.chat.clientHeight -
                number(chatStyle.paddingTop) -
                number(chatStyle.paddingBottom) -
                ui['chat-header'].getBoundingClientRect().height -
                number(chatStyle.rowGap) * 2 -
                feedbackHeight -
                formGap -
                messageReserve;
            ui.question.style.setProperty('--pinefetch-ai-composer-space', `${Math.max(minimum, allowance)}px`);
        }
        ui.question.style.height = 'auto';
        if (ui.question.scrollHeight > 0) ui.question.style.height = `${ui.question.scrollHeight}px`;
        chatAtEnd = nearEnd;
        if (nearEnd) ui.messages.scrollTop = ui.messages.scrollHeight;
        else ui.messages.scrollTop = scrollTop;
    };
    const selectPanel = (panel, { focus = false } = {}) => {
        state.activePanel = panel;
        ui.workspace.setAttribute('data-ai-active-panel', panel);
        ui['show-chat'].setAttribute('aria-pressed', String(panel === 'chat'));
        ui['show-summary'].setAttribute('aria-pressed', String(panel === 'summary'));
        if (panel === 'chat') {
            resizeQuestion();
            if (chatAtEnd) ui.messages.scrollTop = ui.messages.scrollHeight;
            if (focus && !ui.question.disabled) ui.question.focus();
        }
    };
    const createMessage = message => {
        const item = document.createElement('li');
        const assistant = message.role !== 'user';
        item.className = `pinefetch-ai-message pinefetch-ai-message-${assistant ? 'assistant' : 'user'}`;
        const author = document.createElement('p');
        author.className = 'oj-helper pinefetch-ai-message-author';
        if (assistant) {
            const logo = document.createElement('img');
            logo.className = 'pinefetch-ai-message-logo';
            logo.setAttribute('src', 'images/logo.png');
            logo.setAttribute('alt', '');
            logo.setAttribute('aria-hidden', 'true');
            author.append(logo, document.createTextNode('PineFetch'));
        } else author.textContent = 'You';
        const content = document.createElement('div');
        content.className = 'oj-prose pinefetch-ai-prose pinefetch-ai-message-content';
        if (assistant) renderAiMarkdown(content, message.content);
        else content.textContent = message.content || '';
        item.append(author, content);
        if (assistant) {
            const actions = document.createElement('div');
            actions.className = 'pinefetch-ai-message-actions';
            const copy = document.createElement('button');
            copy.type = 'button';
            copy.className = 'oj-icon-button oj-button-ghost oj-button-compact pinefetch-ai-message-copy';
            copy.setAttribute('aria-label', 'Copy answer');
            copy.setAttribute('title', 'Copy answer');
            const icon = document.createElement('i');
            icon.className = 'fa-regular fa-copy';
            icon.setAttribute('aria-hidden', 'true');
            copy.append(icon);
            const id = state.id;
            const revision = state.revision;
            copy.addEventListener('click', async () => {
                try {
                    await navigator.clipboard.writeText(message.content || '');
                    if (!current(id, revision)) return;
                    icon.className = 'fa-solid fa-check';
                    copy.setAttribute('aria-label', 'Answer copied');
                    copy.setAttribute('title', 'Copied');
                    state.chatCopyError = '';
                    state.chatNotice = 'Answer copied.';
                } catch {
                    if (!current(id, revision)) return;
                    state.chatCopyError = 'Answer could not be copied. Try again.';
                }
                render();
            });
            actions.append(copy);
            item.append(actions);
        }
        if (message.pending) {
            const pending = document.createElement('p');
            pending.className = 'oj-helper pinefetch-ai-message-pending';
            pending.textContent = 'Waiting for an answer…';
            item.append(pending);
        }
        return item;
    };
    const renderMessages = () => {
        if (displayedMessages === state.messages && displayedPending === state.pendingQuestion) return;
        displayedMessages = state.messages;
        displayedPending = state.pendingQuestion;
        const scrollTop = ui.messages.scrollTop;
        const nearEnd =
            ui.messages.clientHeight > 0
                ? ui.messages.scrollHeight - scrollTop - ui.messages.clientHeight < 80
                : chatAtEnd;
        const messages = state.pendingQuestion
            ? [...state.messages, { role: 'user', content: state.pendingQuestion, pending: true }]
            : state.messages;
        const keys = messages.map(message => JSON.stringify([message.role, message.content, Boolean(message.pending)]));
        let unchanged = 0;
        while (unchanged < keys.length && keys[unchanged] === messageKeys[unchanged]) unchanged += 1;
        while (ui.messages.children.length > unchanged) ui.messages.removeChild(ui.messages.lastElementChild);
        for (const message of messages.slice(unchanged)) ui.messages.append(createMessage(message));
        messageKeys = keys;
        ui['chat-empty'].hidden = messages.length > 0;
        chatAtEnd = nearEnd;
        if (nearEnd) ui.messages.scrollTop = ui.messages.scrollHeight;
        else ui.messages.scrollTop = scrollTop;
    };
    const render = () => {
        const codex = codexStatus(state.diagnostics);
        const available = ready();
        ui.setup.textContent = !api.available
            ? 'Open the PineFetch desktop app to use the locally installed Codex CLI.'
            : state.checking
              ? 'Checking Codex CLI…'
              : state.diagnostics && !codex.ready
                ? 'Codex is not ready. Check Settings → Diagnostics.'
                : '';
        ui['setup-section'].hidden = api.available && (!state.diagnostics || codex.ready);
        ui.check.disabled = state.checking || !api.available;
        ui['source-notice'].textContent =
            state.loaded && !state.transcript ? 'A saved transcript is required for chat and summaries.' : '';
        ui['source-notice'].hidden = !ui['source-notice'].textContent;
        ui['transcript-controls'].hidden = !ui['source-notice'].textContent;
        ui.transcribe.disabled = !api.available || !state.id || !onTranscribe;
        ui['load-status'].textContent =
            state.loadError || (state.reading ? 'Loading saved summary and conversation…' : '');
        ui['load-status'].classList.toggle('oj-status-error', Boolean(state.loadError));
        ui.reload.hidden = !state.loadError && !state.summaryReadError && !state.chatReadError;
        ui.reload.disabled = state.reading || Boolean(state.summaryRequest || state.chatRequest);
        ui.summary.hidden = !state.summary;
        ui['summary-empty'].hidden = Boolean(state.summary);
        ui['summary-panel'].setAttribute('data-ai-has-summary', String(Boolean(state.summary)));
        if (displayedSummary !== state.summary) {
            displayedSummary = state.summary;
            ui['summary-text'].textContent = state.summary?.quick_summary || '';
            ui.takeaways.replaceChildren();
            for (const takeaway of state.summary?.key_takeaways || []) {
                const item = document.createElement('li');
                item.textContent = takeaway;
                ui.takeaways.append(item);
            }
        }
        ui['analyze-label'].textContent = state.summaryRequest
            ? 'Analyzing…'
            : state.summary
              ? 'Regenerate'
              : 'Analyze Video';
        ui.analyze.setAttribute('aria-label', state.summary ? 'Regenerate Summary' : 'Analyze Video');
        ui.analyze.setAttribute('title', state.summary ? 'Regenerate Summary' : 'Analyze Video');
        ui.analyze.disabled = !available || Boolean(state.summaryRequest);
        updateButton(ui.analyze, Boolean(state.summaryRequest));
        ui['cancel-summary'].hidden = !state.summaryRequest;
        ui.copy.disabled = !state.summary;
        ui['summary-status'].textContent =
            state.summaryError ||
            (state.summaryRequest ? 'Generating Quick Summary and Key Takeaways…' : state.summaryNotice);
        ui['summary-status'].classList.toggle('oj-status-error', Boolean(state.summaryError));
        ui['retry-summary'].hidden = !state.summaryError || state.summaryReadError;
        ui['retry-summary'].disabled = !available || Boolean(state.summaryRequest);
        ui.question.disabled = !available || state.chatReadError || Boolean(state.chatRequest);
        ui.send.disabled = !available || state.chatReadError || Boolean(state.chatRequest) || !ui.question.value.trim();
        ui.send.setAttribute('aria-label', state.chatRequest ? 'Preparing answer' : 'Send question');
        ui.send.setAttribute('title', state.chatRequest ? 'Preparing answer' : 'Send question');
        updateButton(ui.send, Boolean(state.chatRequest));
        ui['cancel-chat'].hidden = !state.chatRequest;
        ui['chat-status'].textContent =
            state.chatError ||
            (state.chatRequest
                ? 'Reading the transcript and preparing an answer…'
                : state.chatCopyError || state.chatNotice);
        ui['chat-status'].classList.toggle('oj-status-error', Boolean(state.chatError || state.chatCopyError));
        ui['retry-chat'].hidden = !state.failedQuestion || !state.chatError || state.chatReadError;
        ui['retry-chat'].disabled = !available || Boolean(state.chatRequest);
        ui.messages.setAttribute('aria-busy', String(Boolean(state.chatRequest)));
        renderMessages();
        resizeQuestion();
    };
    const cancel = async (lane, { quiet = false } = {}) => {
        const key = `${lane}Request`;
        const requestId = state[key];
        const id = state.id;
        const revision = state.revision;
        if (!requestId || !id) return;
        try {
            await api.cancelVideoAi({ historyId: id, requestId });
            // Only the operation's settlement releases its lane. A cancellation
            // may race the final database commit; never enable a duplicate run.
        } catch {
            if (!quiet && current(id, revision) && state[key] === requestId) {
                state[`${lane}Error`] = 'Cancellation could not be confirmed. Wait for the current request to finish.';
                render();
            }
        }
    };
    const reset = id => {
        void cancel('summary', { quiet: true });
        void cancel('chat', { quiet: true });
        state.id = id;
        state.revision += 1;
        transcriptRefreshPending = false;
        transcriptRefreshRequest += 1;
        state.loaded = false;
        state.reading = false;
        state.transcript = false;
        state.transcriptHash = null;
        state.diagnostics = null;
        state.checking = false;
        state.summary = null;
        state.messages = [];
        state.summaryRequest = null;
        state.chatRequest = null;
        state.pendingQuestion = '';
        state.summaryError = '';
        state.chatError = '';
        state.loadError = '';
        state.summaryNotice = '';
        state.summaryReadError = false;
        state.chatReadError = false;
        state.failedQuestion = '';
        state.chatNotice = '';
        state.chatCopyError = '';
        messageKeys = [];
        ui.messages.replaceChildren();
        ui.messages.scrollTop = 0;
        chatAtEnd = true;
        ui.question.value = '';
        resizeQuestion();
        selectPanel('chat');
        render();
    };
    const check = async () => {
        if (!state.id || state.checking) return;
        const id = state.id;
        const revision = state.revision;
        state.checking = true;
        render();
        try {
            const diagnostics = await api.getCodexDiagnostics();
            if (current(id, revision)) state.diagnostics = diagnostics;
        } catch {
            if (current(id, revision)) state.diagnostics = { status: 'failed' };
        } finally {
            if (current(id, revision)) {
                state.checking = false;
                render();
            }
        }
    };
    const refreshTranscript = async () => {
        // A newly saved source can unlock chat without resetting messages,
        // summaries, reading position or the user's current draft.
        if (!state.id || !api.available) return;
        transcriptRefreshPending = true;
        if (state.reading) return;
        transcriptRefreshPending = false;
        if (!state.loaded) return;
        const id = state.id;
        const revision = state.revision;
        const request = ++transcriptRefreshRequest;
        try {
            const saved = await api.getVideoAi({ historyId: id });
            if (!current(id, revision) || request !== transcriptRefreshRequest) return;
            state.transcript = saved.transcript_available === true;
            state.transcriptHash = saved.transcript_hash || null;
            if (state.transcript && state.transcriptHash) {
                if (/transcript.*(revision|could not be loaded)/i.test(state.loadError)) state.loadError = '';
            } else if (state.transcript)
                state.loadError =
                    'The saved transcript revision could not be verified. Retry Loading before using AI actions.';
        } catch {
            if (current(id, revision) && request === transcriptRefreshRequest)
                state.loadError = 'The new transcript could not be loaded. Retry Loading to enable AI actions.';
        }
        if (current(id, revision) && request === transcriptRefreshRequest) render();
    };
    const load = async () => {
        if (!state.id || !api.available || state.reading) return;
        if (!state.diagnostics) void check();
        if (state.loaded && !state.loadError && !state.summaryReadError && !state.chatReadError) return;
        if (state.summaryRequest || state.chatRequest) return;
        const id = state.id;
        const revision = state.revision;
        state.reading = true;
        state.loadError = '';
        render();
        try {
            const saved = await api.getVideoAi({ historyId: id });
            if (!current(id, revision)) return;
            state.transcript = saved.transcript_available === true;
            state.transcriptHash = saved.transcript_hash || null;
            state.summary = saved.summary || null;
            state.messages = saved.messages || [];
            state.summaryReadError = Boolean(saved.summary_error);
            state.chatReadError = Boolean(saved.chat_error);
            state.summaryError = state.summaryReadError
                ? 'The saved summary could not be loaded. Regenerate Analysis to replace it. Ask this Video is independent.'
                : '';
            state.chatError = state.chatReadError
                ? 'The saved conversation could not be loaded. Retry Loading before sending a question. You can still generate a summary.'
                : '';
            state.loaded = true;
            if (state.transcript && !state.transcriptHash)
                state.loadError =
                    'The saved transcript revision could not be verified. Retry Loading before using AI actions.';
            if (saved.invalidated)
                state.summaryNotice =
                    'Results from earlier transcript revisions are hidden. Chat and analysis use the current saved transcript.';
        } catch {
            if (current(id, revision))
                state.loadError =
                    'Saved AI data could not be loaded. Retry to load this video’s transcript and conversation.';
        } finally {
            if (current(id, revision)) {
                state.reading = false;
                render();
                if (transcriptRefreshPending) void refreshTranscript();
            }
        }
    };
    const analyze = async () => {
        if (!ready() || state.summaryRequest) return;
        const id = state.id;
        const revision = state.revision;
        const requestId = requestIdentity();
        state.summaryRequest = requestId;
        state.summaryError = '';
        state.summaryNotice = '';
        render();
        try {
            const summary = await api.analyzeVideo({ historyId: id, requestId, transcriptHash: state.transcriptHash });
            if (!current(id, revision) || state.summaryRequest !== requestId) return;
            state.summary = summary;
            state.summaryReadError = false;
        } catch (error) {
            const message = errorMessage(error, 'Analysis');
            appendLog(`[AI summary] ${message}`, true);
            if (current(id, revision) && state.summaryRequest === requestId) {
                invalidateChangedTranscript(error);
                state.summaryError = message;
            }
        } finally {
            if (current(id, revision) && state.summaryRequest === requestId) {
                state.summaryRequest = null;
                render();
            }
        }
    };
    const ask = async (question = ui.question.value.trim()) => {
        if (!ready() || state.chatReadError || state.chatRequest || !question) return;
        const id = state.id;
        const revision = state.revision;
        const requestId = requestIdentity();
        state.chatRequest = requestId;
        state.chatError = '';
        state.failedQuestion = '';
        state.chatNotice = '';
        state.chatCopyError = '';
        state.pendingQuestion = question;
        if (ui.question.value.trim() === question) ui.question.value = '';
        render();
        try {
            const response = await api.askVideo({
                historyId: id,
                requestId,
                transcriptHash: state.transcriptHash,
                question,
            });
            if (!current(id, revision) || state.chatRequest !== requestId) return;
            state.messages = response.messages;
        } catch (error) {
            const message = errorMessage(error, 'Answer');
            appendLog(`[Ask this Video] ${message}`, true);
            if (current(id, revision) && state.chatRequest === requestId) {
                invalidateChangedTranscript(error);
                state.chatError = message;
                state.failedQuestion = question;
            }
        } finally {
            if (current(id, revision) && state.chatRequest === requestId) {
                state.chatRequest = null;
                state.pendingQuestion = '';
                render();
            }
        }
    };
    const bindEvents = () => {
        ui.transcribe.addEventListener('click', () => {
            if (state.id && !ui.transcribe.disabled) onTranscribe?.(state.id);
        });
        ui.check.addEventListener('click', () => void check());
        ui.reload.addEventListener('click', () => void load());
        ui.analyze.addEventListener('click', () => void analyze());
        ui['retry-summary'].addEventListener('click', () => void analyze());
        ui['retry-chat'].addEventListener('click', () => void ask(state.failedQuestion));
        ui['show-chat'].addEventListener('click', () => selectPanel('chat', { focus: true }));
        ui['show-summary'].addEventListener('click', () => selectPanel('summary'));
        ui['cancel-summary'].addEventListener('click', () => void cancel('summary'));
        ui['cancel-chat'].addEventListener('click', () => void cancel('chat'));
        ui.question.addEventListener('input', () => {
            render();
        });
        ui.messages.addEventListener('scroll', () => {
            if (ui.messages.clientHeight > 0)
                chatAtEnd = ui.messages.scrollHeight - ui.messages.scrollTop - ui.messages.clientHeight < 80;
        });
        if (typeof ResizeObserver !== 'undefined') {
            const measurements = new Map();
            const observer = new ResizeObserver(entries => {
                let changed = false;
                for (const { target, contentRect } of entries) {
                    if (!contentRect.width) continue;
                    const size = `${contentRect.width}:${target === ui.chat ? contentRect.height : ''}`;
                    if (measurements.get(target) === size) continue;
                    measurements.set(target, size);
                    changed = true;
                }
                if (changed) resizeQuestion();
            });
            observer.observe(ui.question);
            observer.observe(ui.chat);
        }
        ui.form.addEventListener('submit', event => {
            event.preventDefault();
            void ask();
        });
        ui.question.addEventListener('keydown', event => {
            if (event.key === 'Enter' && !event.shiftKey && !event.isComposing && event.keyCode !== 229) {
                event.preventDefault();
                void ask();
            }
        });
        ui.copy.addEventListener('click', async () => {
            if (!state.summary) return;
            const id = state.id;
            const revision = state.revision;
            const summary = state.summary;
            try {
                await navigator.clipboard.writeText(
                    `Quick Summary\n${summary.quick_summary}\n\nKey Takeaways\n${summary.key_takeaways.map(item => `• ${item}`).join('\n')}`
                );
                if (current(id, revision) && summary === state.summary) state.summaryNotice = 'Summary copied.';
            } catch {
                if (current(id, revision)) state.summaryError = 'Summary could not be copied. Try again.';
            }
            if (current(id, revision)) render();
        });
    };
    reset(null);
    return Object.freeze({ reset, load, refreshTranscript, bindEvents, deactivate: () => reset(null) });
};
