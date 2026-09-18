export const createBrowserImportView = ({ els, invoke, appendLog, appendTextSpans }) => {
    const linkDumpExtensionRepoUrl = 'https://github.com/oliverjessner/PineFetch-Link-Dump';
    const state = { linkDump: null, generatedLinkDumpSecret: null };
    let linkDumpSyncPromise = null;

    const formatDateTime = value => {
        if (!value) return '-';
        const date = new Date(`${value.replace(' ', 'T')}Z`);
        if (Number.isNaN(date.getTime())) return value;
        return date.toLocaleString([], {
            year: 'numeric',
            month: 'short',
            day: 'numeric',
            hour: '2-digit',
            minute: '2-digit',
        });
    };

    const statusLabel = value => {
        const normalized = `${value || ''}`.trim().toLowerCase();
        return normalized ? normalized.charAt(0).toUpperCase() + normalized.slice(1) : 'Stopped';
    };

    const setLinkDumpStatusText = (message, isError = false) => {
        if (!els.linkDumpServerStatusText) return;
        els.linkDumpServerStatusText.textContent = message || '';
        els.linkDumpServerStatusText.classList.toggle('pf-status-error', Boolean(message && isError));
        els.linkDumpServerStatusText.classList.toggle('pf-status-success', Boolean(message && !isError));
    };

    const setLinkDumpSecretStatus = (message, isError = false) => {
        if (!els.linkDumpSecretStatus) return;
        els.linkDumpSecretStatus.textContent = message || '';
        els.linkDumpSecretStatus.classList.toggle('pf-status-error', Boolean(message && isError));
        els.linkDumpSecretStatus.classList.toggle('pf-status-success', Boolean(message && !isError));
    };

    const applyLinkDumpServerStatus = serverStatus => {
        if (!serverStatus || !els.linkDumpServerStatusBadge) return;
        if (state.linkDump) {
            state.linkDump = { ...state.linkDump, server_status: serverStatus };
        }
        const status = `${serverStatus.status || 'stopped'}`.toLowerCase();
        els.linkDumpServerStatusBadge.textContent = statusLabel(status);
        els.linkDumpServerStatusBadge.classList.toggle('pf-badge-danger', status === 'error');
        els.linkDumpServerStatusBadge.classList.toggle('pf-badge-warning', status === 'stopped');
        els.linkDumpServerStatusBadge.classList.toggle('pf-badge-muted', status !== 'running' && status !== 'error');
        els.linkDumpServerStatusBadge.classList.toggle('pf-badge', true);

        if (status === 'running') {
            setLinkDumpStatusText('Browser extensions can send YouTube, TikTok, and Instagram links to this PineFetch instance.');
        } else if (status === 'error') {
            setLinkDumpStatusText(serverStatus.error_message || 'Link Dump Server could not start.', true);
        } else {
            setLinkDumpStatusText('Link Dump Server is stopped.', false);
        }
    };

    const renderLinkDumpSecrets = secrets => {
        if (!els.linkDumpSecretList) return;
        if (state.linkDump) {
            state.linkDump = { ...state.linkDump, secrets };
        }
        const visibleSecrets = Array.isArray(secrets)
            ? secrets.filter(connection => `${connection.status || ''}`.toLowerCase() !== 'deleted')
            : [];
        els.linkDumpSecretHint.hidden = visibleSecrets.length > 0;
        const fragment = document.createDocumentFragment();

        visibleSecrets.forEach(connection => {
            const item = document.createElement('div');
            item.className = 'pf-link-dump-secret-item';

            const content = document.createElement('div');
            content.className = 'pf-link-dump-secret-content';

            const title = document.createElement('div');
            title.className = 'pf-link-dump-secret-title';
            title.textContent = connection.name || 'Link Dump Connection';

            const meta = document.createElement('div');
            meta.className = 'pf-link-dump-secret-meta';
            appendTextSpans(meta, [
                `Created ${formatDateTime(connection.created_at)}`,
                `Last used ${formatDateTime(connection.last_used_at)}`,
            ]);

            content.append(title, meta);

            const actions = document.createElement('div');
            actions.className = 'pf-row pf-link-dump-secret-actions';

            const badge = document.createElement('span');
            const status = `${connection.status || 'active'}`.toLowerCase();
            badge.className = `pf-badge ${
                status === 'active' ? '' : status === 'revoked' ? 'pf-badge-warning' : 'pf-badge-muted'
            }`;
            badge.textContent = statusLabel(status);
            actions.appendChild(badge);

            if (status === 'active') {
                const revokeBtn = document.createElement('button');
                revokeBtn.className = 'pf-btn pf-btn-ghost';
                revokeBtn.type = 'button';
                revokeBtn.textContent = 'Revoke';
                revokeBtn.onclick = () => {
                    if (!window.confirm('Revoke this connection? Extensions using this secret will no longer be able to send links.')) {
                        return;
                    }
                    void revokeLinkDumpSecret(connection.id);
                };
                actions.appendChild(revokeBtn);
            }

            if (status !== 'deleted') {
                const deleteBtn = document.createElement('button');
                deleteBtn.className = 'pf-btn pf-btn-danger';
                deleteBtn.type = 'button';
                deleteBtn.textContent = 'Delete';
                deleteBtn.onclick = () => {
                    if (!window.confirm('Delete this connection? Extensions using this secret will no longer be able to send links.')) {
                        return;
                    }
                    void deleteLinkDumpSecret(connection.id);
                };
                actions.appendChild(deleteBtn);
            }

            item.append(content, actions);
            fragment.appendChild(item);
        });
        els.linkDumpSecretList.replaceChildren(fragment);
    };

    const renderLinkDumpOverview = overview => {
        state.linkDump = overview;
        const settings = overview?.settings || {};
        const serverStatus = overview?.server_status || {};
        if (els.linkDumpServerUrl) {
            els.linkDumpServerUrl.value =
                serverStatus.url || `http://${settings.host || '127.0.0.1'}:${settings.port || 2255}`;
        }
        if (els.linkDumpPort) {
            els.linkDumpPort.value = settings.port || 2255;
        }
        if (els.linkDumpServerEnabled) {
            els.linkDumpServerEnabled.checked = settings.server_enabled !== false;
        }
        applyLinkDumpServerStatus(serverStatus);
        renderLinkDumpSecrets(overview?.secrets || []);
    };

    const syncLinkDumpOverview = () => {
        if (!invoke) return Promise.resolve();
        if (linkDumpSyncPromise) return linkDumpSyncPromise;

        linkDumpSyncPromise = (async () => {
            try {
                renderLinkDumpOverview(await invoke('get_link_dump_overview'));
            } catch (err) {
                setLinkDumpStatusText(`Link Dump settings unavailable: ${err}`, true);
                appendLog(`[link-dump] ${err}`, true);
            }
        })().finally(() => {
            linkDumpSyncPromise = null;
        });
        return linkDumpSyncPromise;
    };

    const openLinkDumpExtensionRepo = async event => {
        event.preventDefault();
        const url = els.linkDumpExtensionRepoLink?.href || linkDumpExtensionRepoUrl;
        if (invoke) {
            try {
                await invoke('open_external_url', { url });
                return;
            } catch (err) {
                appendLog(`[link-dump] Could not open extension repository: ${err}`, true);
            }
        }
        window.open(url, '_blank', 'noopener,noreferrer');
    };

    const saveLinkDumpServer = async () => {
        if (!invoke) return;
        const port = Number(els.linkDumpPort.value);
        if (!Number.isInteger(port) || port < 1 || port > 65535) {
            setLinkDumpStatusText('Port must be between 1 and 65535.', true);
            return;
        }

        try {
            const overview = await invoke('update_link_dump_settings', {
                patch: {
                    server_enabled: Boolean(els.linkDumpServerEnabled.checked),
                    port,
                },
            });
            renderLinkDumpOverview(overview);
            appendLog('[link-dump] server settings saved', false);
        } catch (err) {
            setLinkDumpStatusText(`${err}`, true);
            appendLog(`[link-dump] ${err}`, true);
        }
    };

    const restartLinkDumpServer = async () => {
        if (!invoke) return;
        try {
            applyLinkDumpServerStatus(await invoke('restart_link_dump_server'));
            appendLog('[link-dump] server restarted', false);
        } catch (err) {
            setLinkDumpStatusText(`${err}`, true);
            appendLog(`[link-dump] ${err}`, true);
        }
    };

    const generateLinkDumpSecret = async () => {
        if (!invoke) return;
        try {
            const generated = await invoke('create_link_dump_secret', {
                name: els.linkDumpSecretName.value.trim() || null,
            });
            state.generatedLinkDumpSecret = generated.secret;
            els.generatedLinkDumpSecret.value = generated.secret;
            els.generatedLinkDumpSecretPanel.hidden = false;
            els.linkDumpSecretName.value = '';
            setLinkDumpSecretStatus('Secret generated.');
            renderLinkDumpOverview(await invoke('get_link_dump_overview'));
        } catch (err) {
            setLinkDumpSecretStatus(`${err}`, true);
            appendLog(`[link-dump] ${err}`, true);
        }
    };

    const copyGeneratedLinkDumpSecret = async () => {
        if (!state.generatedLinkDumpSecret) return;
        try {
            await navigator.clipboard.writeText(state.generatedLinkDumpSecret);
            state.generatedLinkDumpSecret = null;
            els.generatedLinkDumpSecret.value = '';
            els.generatedLinkDumpSecretPanel.hidden = true;
            setLinkDumpSecretStatus('Secret copied.');
        } catch (err) {
            setLinkDumpSecretStatus(`Copy failed: ${err}`, true);
            appendLog(`[copy] ${err}`, true);
        }
    };

    const revokeLinkDumpSecret = async id => {
        if (!invoke) return;
        try {
            const secrets = await invoke('revoke_link_dump_secret', { id });
            renderLinkDumpSecrets(secrets);
            setLinkDumpSecretStatus('Connection revoked.');
        } catch (err) {
            setLinkDumpSecretStatus(`${err}`, true);
            appendLog(`[link-dump] ${err}`, true);
        }
    };

    const deleteLinkDumpSecret = async id => {
        if (!invoke) return;
        try {
            const secrets = await invoke('delete_link_dump_secret', { id });
            renderLinkDumpSecrets(secrets);
            setLinkDumpSecretStatus('Connection deleted.');
        } catch (err) {
            setLinkDumpSecretStatus(`${err}`, true);
            appendLog(`[link-dump] ${err}`, true);
        }
    };

    const bindEvents = () => {
        els.saveLinkDumpServerBtn.addEventListener('click', () => {
            void saveLinkDumpServer();
        });
        els.restartLinkDumpServerBtn.addEventListener('click', () => {
            void restartLinkDumpServer();
        });
        els.generateLinkDumpSecretBtn.addEventListener('click', () => {
            void generateLinkDumpSecret();
        });
        els.copyGeneratedLinkDumpSecretBtn.addEventListener('click', () => {
            void copyGeneratedLinkDumpSecret();
        });
        els.linkDumpExtensionRepoLink.addEventListener('click', event => {
            void openLinkDumpExtensionRepo(event);
        });
    };

    return Object.freeze({
        applyLinkDumpServerStatus,
        bindEvents,
        hasOverview: () => Boolean(state.linkDump),
        syncLinkDumpOverview,
    });
};
