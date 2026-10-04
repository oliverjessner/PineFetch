import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { delimiter, join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';

for (const failedGate of ['check', 'build:check']) {
    test(`publishing stops when ${failedGate} fails, before any external release action`, async t => {
        const fixture = await mkdtemp(join(tmpdir(), 'pinefetch-publish-gate-'));
        t.after(() => rm(fixture, { recursive: true, force: true }));
        const bin = join(fixture, 'bin');
        const scripts = join(fixture, 'scripts');
        const log = join(fixture, 'commands.txt');
        await Promise.all([mkdir(bin), mkdir(scripts)]);
        await writeFile(join(scripts, 'publish.sh'), await readFile(new URL('../scripts/publish.sh', import.meta.url)));
        await writeFile(
            join(bin, 'npm'),
            `#!/bin/sh
printf '%s\\n' "npm $*" >> "$QA_GATE_LOG"
if [ "$*" = "run $QA_FAILED_GATE" ]; then exit 42; fi
exit 0
`,
            { mode: 0o755 }
        );
        // No real repository, signer, network client, or publishing command may run.
        for (const command of ['git', 'gh', 'codesign', 'hdiutil', 'shasum', 'curl', 'open']) {
            await writeFile(
                join(bin, command),
                `#!/bin/sh
printf '%s\\n' "forbidden: ${command} $*" >> "$QA_GATE_LOG"
exit 99
`,
                { mode: 0o755 }
            );
        }
        const result = spawnSync('sh', [join(scripts, 'publish.sh')], {
            cwd: fixture,
            encoding: 'utf8',
            timeout: 15000,
            env: {
                ...process.env,
                PATH: `${bin}${delimiter}${process.env.PATH}`,
                HOMEBREW_TAP_DIR: join(fixture, 'tap'),
                QA_GATE_LOG: log,
                QA_FAILED_GATE: failedGate,
            },
        });
        assert.ifError(result.error);
        assert.equal(result.status, 42, result.stderr);
        assert.deepEqual(
            (await readFile(log, 'utf8')).trim().split('\n'),
            failedGate === 'check'
                ? ['npm run sync:version', 'npm run check']
                : ['npm run sync:version', 'npm run check', 'npm run build:check']
        );
    });
}
