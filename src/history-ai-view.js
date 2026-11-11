import { codexStatus } from './codex-diagnostics.js';

const errorMessage = (error, action) => {
    const message = typeof error === 'string' ? error : error?.message || '';
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

export const createHistoryAiView = ({ root, api, appendLog = () => {} }) => {
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
            'empty-chat',
            'question',
            'send',
            'cancel-chat',
            'chat-status',
            'form',
            'source-notice',
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
    };
    let displayedMessages;
    let displayedPending;
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
    const renderMessages = () => {
        if (displayedMessages === state.messages && displayedPending === state.pendingQuestion) return;
        displayedMessages = state.messages;
        displayedPending = state.pendingQuestion;
        const nearEnd = ui.messages.scrollHeight - ui.messages.scrollTop - ui.messages.clientHeight < 80;
        ui.messages.replaceChildren();
        const messages = state.pendingQuestion
            ? [...state.messages, { role: 'user', content: state.pendingQuestion, pending: true }]
            : state.messages;
        for (const message of messages) {
            const item = document.createElement('li');
            item.className = 'oj-panel oj-panel-compact pinefetch-ai-message';
            const author = document.createElement('p');
            author.className = 'oj-label';
            author.textContent = message.role === 'user' ? 'You' : 'PineFetch';
            const content = document.createElement('p');
            content.className = 'pinefetch-ai-prose';
            content.textContent = message.content || '';
            item.append(author, content);
            if (message.pending) {
                const pending = document.createElement('p');
                pending.className = 'oj-helper';
                pending.textContent = 'Waiting for a transcript-based answer…';
                item.append(pending);
            }
            ui.messages.append(item);
        }
        ui['empty-chat'].hidden = messages.length > 0;
        if (nearEnd) ui.messages.scrollTop = ui.messages.scrollHeight;
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
            state.loaded && !state.transcript
                ? 'No saved transcript is available. Download this video using “Transcribe to text” or “Transcribe with timestamps”, then reopen its details. Summary and chat both need a non-empty transcript.'
                : '';
        ui['source-notice'].hidden = !ui['source-notice'].textContent;
        ui['load-status'].textContent =
            state.loadError || (state.reading ? 'Loading saved summary and conversation…' : '');
        ui['load-status'].classList.toggle('oj-status-error', Boolean(state.loadError));
        ui.reload.hidden = !state.loadError && !state.summaryReadError && !state.chatReadError;
        ui.reload.disabled = state.reading || Boolean(state.summaryRequest || state.chatRequest);
        ui.summary.hidden = !state.summary;
        ui['summary-text'].textContent = state.summary?.quick_summary || '';
        ui.takeaways.replaceChildren();
        for (const takeaway of state.summary?.key_takeaways || []) {
            const item = document.createElement('li');
            item.textContent = takeaway;
            ui.takeaways.append(item);
        }
        ui.analyze.textContent = state.summaryRequest
            ? 'Analyzing Video…'
            : state.summary
              ? 'Regenerate Analysis'
              : 'Analyze Video';
        ui.analyze.disabled = !available || Boolean(state.summaryRequest);
        updateButton(ui.analyze, Boolean(state.summaryRequest));
        ui['cancel-summary'].hidden = !state.summaryRequest;
        ui.copy.disabled = !state.summary;
        ui['summary-status'].textContent =
            state.summaryError ||
            (state.summaryRequest ? 'Generating Quick Summary and Key Takeaways…' : state.summaryNotice);
        ui['summary-status'].classList.toggle('oj-status-error', Boolean(state.summaryError));
        ui.question.disabled = !available || state.chatReadError;
        ui.send.disabled = !available || state.chatReadError || Boolean(state.chatRequest) || !ui.question.value.trim();
        ui.send.textContent = state.chatRequest ? 'Answering…' : 'Send';
        updateButton(ui.send, Boolean(state.chatRequest));
        ui['cancel-chat'].hidden = !state.chatRequest;
        ui['chat-status'].textContent =
            state.chatError || (state.chatRequest ? 'Reading the transcript and preparing an answer…' : '');
        ui['chat-status'].classList.toggle('oj-status-error', Boolean(state.chatError));
        renderMessages();
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
        ui.question.value = '';
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
            state.summaryNotice = 'Summary saved locally.';
        } catch (error) {
            appendLog('[AI summary] Analysis did not complete.', true);
            if (current(id, revision) && state.summaryRequest === requestId) {
                invalidateChangedTranscript(error);
                state.summaryError = errorMessage(error, 'Analysis');
            }
        } finally {
            if (current(id, revision) && state.summaryRequest === requestId) {
                state.summaryRequest = null;
                render();
            }
        }
    };
    const ask = async () => {
        const question = ui.question.value.trim();
        if (!ready() || state.chatReadError || state.chatRequest || !question) return;
        const id = state.id;
        const revision = state.revision;
        const requestId = requestIdentity();
        state.chatRequest = requestId;
        state.chatError = '';
        state.pendingQuestion = question;
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
            if (ui.question.value.trim() === question) ui.question.value = '';
        } catch (error) {
            appendLog('[Ask this Video] Answer did not complete.', true);
            if (current(id, revision) && state.chatRequest === requestId) {
                invalidateChangedTranscript(error);
                state.chatError = errorMessage(error, 'Answer');
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
        ui.check.addEventListener('click', () => void check());
        ui.reload.addEventListener('click', () => void load());
        ui.analyze.addEventListener('click', () => void analyze());
        ui['cancel-summary'].addEventListener('click', () => void cancel('summary'));
        ui['cancel-chat'].addEventListener('click', () => void cancel('chat'));
        ui.question.addEventListener('input', render);
        ui.form.addEventListener('submit', event => {
            event.preventDefault();
            void ask();
        });
        ui.question.addEventListener('keydown', event => {
            if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
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
    return Object.freeze({ reset, load, bindEvents, deactivate: () => reset(null) });
};
