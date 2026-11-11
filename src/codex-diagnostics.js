// Readiness is explicit: a successful CLI exit or API-key login is not a
// confirmed authentication with the required execution restrictions.
export const codexStatus = diagnostics => {
    const status = diagnostics?.status;
    const states = {
        not_installed: [
            'Not installed',
            'Install Codex CLI, then run codex login in Terminal and sign in with ChatGPT.',
        ],
        unauthenticated: [
            'Installed, not authenticated',
            'Run codex login in Terminal and sign in with ChatGPT, then check again.',
        ],
        chatgpt: ['Connected with ChatGPT', 'Your existing ChatGPT login is used. No OpenAI API key is required.'],
        api_key: [
            'Authenticated using API key',
            'Your existing Codex API-key authentication is used. To use ChatGPT instead, run codex logout, then codex login in Terminal.',
        ],
        unknown_auth: [
            'Unknown authentication method',
            'ChatGPT authentication could not be confirmed. Run codex login status in Terminal, then sign in with ChatGPT using codex login.',
        ],
        failed: [
            'Diagnostics failed',
            'Check your Codex installation in Terminal with codex --version and codex login status, then check again.',
        ],
    };
    const [label, help] = states[status] || states.failed;
    const authenticated = status === 'chatgpt' || status === 'api_key';
    const ready = authenticated && diagnostics?.ready === true;
    return {
        ready,
        label,
        help:
            authenticated && !ready
                ? 'Update Codex CLI to a compatible version, then check again. The required execution restrictions must be supported.'
                : help,
    };
};

export const createCodexDiagnostics = ({ root, api }) => {
    const status = root.querySelector('[data-codex-status]');
    const statusGroup = root.querySelector('[data-codex-status-group]');
    const indicator = root.querySelector('[data-codex-indicator]');
    const version = root.querySelector('[data-codex-version]');
    const check = root.querySelector('[data-codex-check]');
    let request = 0;
    let pending = null;
    const render = (diagnostics, busy = false) => {
        const { ready } = codexStatus(diagnostics);
        const states = {
            not_installed: 'Not installed — install Codex',
            unauthenticated: 'Sign in — run codex login',
            chatgpt: ready ? 'Connected' : 'Update Codex CLI',
            api_key: ready ? 'Connected (API key)' : 'Update Codex CLI',
            unknown_auth: 'Check login — run codex login status',
            failed: 'Check failed — try again',
        };
        status.textContent = busy
            ? 'Checking…'
            : !api.available
              ? 'Open PineFetch desktop'
              : states[diagnostics?.status] || states.failed;
        version.textContent = !busy && diagnostics?.version ? `· v${diagnostics.version}` : '';
        version.hidden = !version.textContent;
        indicator.className = busy ? 'oj-spinner' : 'oj-status-dot';
        statusGroup.classList.toggle('oj-status-success', !busy && ready);
        statusGroup.classList.toggle('oj-status-warning', !busy && !ready && diagnostics?.status !== 'failed');
        statusGroup.classList.toggle('oj-status-error', !busy && diagnostics?.status === 'failed');
    };
    const refresh = () => {
        if (pending) return pending;
        const revision = ++request;
        render(null, true);
        root.setAttribute('aria-busy', 'true');
        check.disabled = true;
        check.setAttribute('aria-busy', 'true');
        check.classList.add('oj-button-loading');
        pending = Promise.resolve().then(async () => {
            try {
                const diagnostics = api.available ? await api.getCodexDiagnostics() : { status: 'failed' };
                if (revision !== request) return;
                render(diagnostics);
            } catch {
                if (revision !== request) return;
                render({ status: 'failed' });
            } finally {
                if (revision === request) {
                    root.setAttribute('aria-busy', 'false');
                    check.disabled = false;
                    check.setAttribute('aria-busy', 'false');
                    check.classList.remove('oj-button-loading');
                    pending = null;
                }
            }
        });
        return pending;
    };
    return Object.freeze({
        refresh,
        bindEvents: () => check.addEventListener('click', () => void refresh()),
    });
};
