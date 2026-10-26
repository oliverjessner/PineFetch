import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

test('Whisper preparation invoked through a symlink executes the CLI instead of silently succeeding', t => {
    const root = mkdtempSync(join(tmpdir(), 'pinefetch-whisper-cli-'));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const scripts = join(root, 'scripts');
    mkdirSync(scripts);
    for (const name of [
        'prepare-whisper-runtime.mjs',
        'check-whisper-portability.mjs',
        'check-whisper-runtime.py',
        'whisper-runtime.version',
        'whisper-av.version',
        'whisper-requirements.txt',
    ]) {
        copyFileSync(new URL(`../scripts/${name}`, import.meta.url), join(scripts, name));
    }
    // No target can start a download, Python process or runtime publication.
    writeFileSync(join(scripts, 'whisper-python.json'), JSON.stringify({ version: '3.12.15', targets: {} }));
    const entry = join(root, 'prepare-whisper-link.mjs');
    symlinkSync(join(scripts, 'prepare-whisper-runtime.mjs'), entry, 'file');

    const result = spawnSync(process.execPath, [entry], {
        cwd: root,
        env: { ...process.env, NODE_OPTIONS: '' },
        encoding: 'utf8',
        timeout: 10000,
    });
    assert.equal(result.error, undefined);
    assert.equal(result.signal, null);
    assert.equal(result.status, 1);
    assert.equal(result.stdout, '');
    assert.equal(result.stderr.trim(), `Unsupported Whisper runtime target: ${process.platform}-${process.arch}`);
    assert.equal(existsSync(join(root, 'src-tauri')), false);
});
