import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
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
        assert.equal(existsSync(join(fixture, '.pinefetch-publish.lock')), false);
    });
}

const createPublishFixture = async (t, { currentCask = false, releaseStatus = 0, openStatus = 0 } = {}) => {
    const root = await mkdtemp(join(tmpdir(), 'pinefetch-publish-tap-'));
    t.after(() => rm(root, { recursive: true, force: true }));
    const repo = join(root, 'repo');
    const tap = join(root, 'tap');
    const bin = join(root, 'bin');
    const log = join(root, 'commands.txt');
    const sha256 = 'a'.repeat(64);
    const env = {
        ...process.env,
        PATH: `${bin}${delimiter}${process.env.PATH}`,
        HOMEBREW_TAP_DIR: tap,
        QA_PUBLISH_LOG: log,
        QA_RELEASE_STATUS: String(releaseStatus),
        QA_OPEN_STATUS: String(openStatus),
        GIT_CONFIG_NOSYSTEM: '1',
        GIT_CONFIG_GLOBAL: '/dev/null',
        GIT_ALLOW_PROTOCOL: 'file',
        GIT_TERMINAL_PROMPT: '0',
    };
    const git = (directory, ...args) => {
        const result = spawnSync('git', ['-C', directory, ...args], {
            env,
            encoding: 'utf8',
            timeout: 10000,
        });
        assert.ifError(result.error);
        assert.equal(result.status, 0, result.stderr);
        return result.stdout.trim();
    };
    await Promise.all([
        mkdir(join(repo, 'scripts'), { recursive: true }),
        mkdir(join(repo, 'docs'), { recursive: true }),
        mkdir(join(repo, 'src-tauri'), { recursive: true }),
        mkdir(join(tap, 'Casks'), { recursive: true }),
        mkdir(bin),
    ]);
    for (const directory of [repo, tap]) {
        git(directory, 'init', '-b', 'main');
        git(directory, 'config', 'user.name', 'Publish Test');
        git(directory, 'config', 'user.email', 'publish-test@example.invalid');
        git(directory, 'config', 'core.hooksPath', '/dev/null');
        git(directory, 'config', 'commit.gpgsign', 'false');
        git(root, 'init', '--bare', '-b', 'main', `${directory}.git`);
        git(directory, 'remote', 'add', 'origin', `${directory}.git`);
    }
    for (const file of ['publish.sh', 'update-homebrew-tap.mjs', 'cleanup.mjs']) {
        await writeFile(join(repo, 'scripts', file), await readFile(new URL(`../scripts/${file}`, import.meta.url)));
    }
    await Promise.all([
        writeFile(join(repo, 'package.json'), JSON.stringify({ name: 'pinefetch', version: '2.5.0' })),
        writeFile(join(repo, 'src-tauri/Cargo.toml'), '[package]\nname = "pinefetch"\nversion = "2.5.0"\n'),
        writeFile(join(repo, '.gitignore'), 'src-tauri/target/\n.pinefetch-publish.lock/\n'),
        writeFile(join(repo, 'docs/changelog.md'), '# 2.5.0\n\nFixture release.\n'),
        writeFile(join(tap, 'Casks/pinefetch.rb'), 'cask "pinefetch" do\n  version "2.4.0"\nend\n'),
        writeFile(join(tap, 'Casks/other.rb'), '# Other application\n'),
        writeFile(
            join(tap, 'README.md'),
            '# Tap\n\n| `bulkpixel` | Cask | Local-first batch image converter | `brew install --cask oliverjessner/tap/bulkpixel` |\n\nbrew install --cask bulkpixel\n'
        ),
    ]);
    if (currentCask) {
        const result = spawnSync(
            process.execPath,
            [join(repo, 'scripts/update-homebrew-tap.mjs'), tap, '2.5.0', sha256],
            {
                env,
                encoding: 'utf8',
                timeout: 10000,
            }
        );
        assert.ifError(result.error);
        assert.equal(result.status, 0, result.stderr);
    }
    for (const directory of [repo, tap]) {
        git(directory, 'add', '-A');
        git(directory, 'commit', '-m', 'Fixture baseline');
        git(directory, 'push', '-u', 'origin', 'main');
    }
    const base = git(tap, 'rev-parse', 'HEAD');
    const stub = (command, body) =>
        writeFile(
            join(bin, command),
            `#!/bin/sh\nset -eu\nprintf '%s\\n' "${command} $*" >> "$QA_PUBLISH_LOG"\n${body}\n`,
            { mode: 0o755 }
        );
    await Promise.all([
        stub(
            'npm',
            'if [ "$*" = "run build -- --bundles app" ]; then mkdir -p src-tauri/target/release/bundle/macos/PineFetch.app; fi'
        ),
        stub(
            'gh',
            'case "$1 $2" in "release view") exit 1 ;; "release create") exit "$QA_RELEASE_STATUS" ;; *) exit 99 ;; esac'
        ),
        stub('codesign', 'exit 0'),
        stub('hdiutil', 'for output_path do :; done\nprintf fixture-dmg > "$output_path"'),
        stub('shasum', `printf '%s\\n' '${sha256}  fixture.dmg'`),
        stub('curl', 'exit 99'),
        stub('brew', 'exit 0'),
        stub('open', 'exit "$QA_OPEN_STATUS"'),
    ]);
    const publish = () => {
        const result = spawnSync('sh', [join(repo, 'scripts/publish.sh')], {
            cwd: root,
            env,
            encoding: 'utf8',
            timeout: 15000,
        });
        assert.ifError(result.error);
        return result;
    };
    return { repo, tap, log, base, git, publish };
};

for (const currentCask of [false, true]) {
    test(`publishing commits only PineFetch tap edits and pushes them when cask is ${currentCask ? 'current' : 'outdated'}`, async t => {
        const fixture = await createPublishFixture(t, { currentCask });
        const { repo, tap, git, base } = fixture;
        const readme = `${await readFile(join(tap, 'README.md'), 'utf8')}\nPineFetch local release note.\n`;
        await writeFile(join(tap, 'README.md'), readme);
        if (!currentCask) await writeFile(join(tap, 'Casks/pinefetch.rb'), '# Locally prepared PineFetch cask\n');
        await writeFile(join(tap, 'Casks/other.rb'), '# Unrelated staged application update\n');
        git(tap, 'add', 'Casks/other.rb');
        await writeFile(join(tap, 'notes.txt'), 'Unrelated untracked note\n');

        const result = fixture.publish();
        assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
        assert.equal(existsSync(join(repo, 'src-tauri/target')), false);
        assert.equal(existsSync(join(repo, '.pinefetch-publish.lock')), false);
        assert.match(result.stdout, /Removed .*generated paths/);
        const commits = git(tap, 'rev-list', '--reverse', `${base}..HEAD`).split('\n');
        assert.equal(commits.length, currentCask ? 1 : 2);
        assert.equal(git(tap, 'show', '-s', '--format=%s', commits[0]), 'Prepare PineFetch release v2.5.0');
        assert.equal(git(`${tap}.git`, 'rev-parse', 'main'), git(tap, 'rev-parse', 'HEAD'));
        assert.equal(git(tap, 'diff', '--cached', '--name-only'), 'Casks/other.rb');
        assert.equal(await readFile(join(tap, 'Casks/other.rb'), 'utf8'), '# Unrelated staged application update\n');
        assert.equal(await readFile(join(tap, 'notes.txt'), 'utf8'), 'Unrelated untracked note\n');
        assert.match(await readFile(join(tap, 'README.md'), 'utf8'), /PineFetch local release note/);
        for (const commit of commits) {
            assert(
                git(tap, 'diff-tree', '--no-commit-id', '--name-only', '-r', commit)
                    .split('\n')
                    .every(file => ['Casks/pinefetch.rb', 'README.md'].includes(file))
            );
        }
        if (!currentCask) {
            assert.equal(git(tap, 'show', `${commits[0]}:Casks/pinefetch.rb`), '# Locally prepared PineFetch cask');
            assert.match(await readFile(join(tap, 'Casks/pinefetch.rb'), 'utf8'), /version "2\.5\.0"/);
        }
    });
}

test('publishing rejects unpushed tap commits even if their tree matches origin', async t => {
    const { tap, git, publish, log } = await createPublishFixture(t);
    git(tap, 'commit', '--allow-empty', '-m', 'Unrelated local commit');
    const head = git(tap, 'rev-parse', 'HEAD');
    await writeFile(join(tap, 'Casks/pinefetch.rb'), '# Pending PineFetch update\n');
    const result = publish();
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stdout, /not synchronized with origin\/main/);
    assert.equal(git(tap, 'rev-parse', 'HEAD'), head);
    assert.equal(await readFile(join(tap, 'Casks/pinefetch.rb'), 'utf8'), '# Pending PineFetch update\n');
    assert.doesNotMatch(await readFile(log, 'utf8'), /gh release create|npm run build --/);
});

test('a failed release keeps build artifacts and the prepared tap commit local', async t => {
    const { repo, tap, base, git, publish } = await createPublishFixture(t, { releaseStatus: 42 });
    await writeFile(join(tap, 'Casks/pinefetch.rb'), '# Pending PineFetch update\n');
    await writeFile(join(tap, 'Casks/other.rb'), '# Unrelated staged change\n');
    git(tap, 'add', 'Casks/other.rb');
    const result = publish();
    assert.equal(result.status, 42, result.stderr);
    assert.notEqual(git(tap, 'rev-parse', 'HEAD'), base);
    assert.equal(git(`${tap}.git`, 'rev-parse', 'main'), base);
    assert.equal(git(tap, 'diff', '--cached', '--name-only'), 'Casks/other.rb');
    assert.equal(git(tap, 'show', 'HEAD:Casks/pinefetch.rb'), '# Pending PineFetch update');
    assert.equal(existsSync(join(repo, 'src-tauri/target/release/bundle/dmg/PineFetch_2.5.0_aarch64_adhoc.dmg')), true);
    assert.equal(existsSync(join(repo, '.pinefetch-publish.lock')), false);
});

test('a failed tap push keeps the published release build available for diagnosis', async t => {
    const { repo, tap, publish, log } = await createPublishFixture(t);
    await writeFile(join(`${tap}.git`, 'hooks/pre-receive'), '#!/bin/sh\nexit 42\n', { mode: 0o755 });
    const result = publish();
    assert.notEqual(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(await readFile(log, 'utf8'), /gh release create/);
    assert.equal(existsSync(join(repo, 'src-tauri/target/release/bundle/dmg/PineFetch_2.5.0_aarch64_adhoc.dmg')), true);
    assert.equal(existsSync(join(repo, '.pinefetch-publish.lock')), false);
    assert.doesNotMatch(result.stdout, /Removed .*generated paths/);
});

test('a browser launch failure does not turn a completed publish and cleanup into a failed release', async t => {
    const { repo, publish } = await createPublishFixture(t, { openStatus: 42 });
    const result = publish();
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stderr, /release was published successfully, but its page could not be opened/);
    assert.equal(existsSync(join(repo, 'src-tauri/target')), false);
    assert.equal(existsSync(join(repo, '.pinefetch-publish.lock')), false);
});

test('a cleanup failure after publishing is reported separately and preserves all cleanup candidates', async t => {
    const { repo, tap, publish, git } = await createPublishFixture(t);
    await mkdir(join(repo, 'coverage'));
    await writeFile(join(repo, 'coverage/source-owned.txt'), 'Tracked source must survive.\n');
    git(repo, 'add', 'coverage/source-owned.txt');
    git(repo, 'commit', '-m', 'Protect a tracked cleanup candidate');
    const result = publish();
    assert.equal(result.status, 1, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stderr, /Publishing succeeded, but local cleanup failed/);
    assert.equal(git(`${tap}.git`, 'rev-parse', 'main'), git(tap, 'rev-parse', 'HEAD'));
    assert.equal(await readFile(join(repo, 'coverage/source-owned.txt'), 'utf8'), 'Tracked source must survive.\n');
    assert.equal(existsSync(join(repo, 'src-tauri/target/release/bundle/dmg/PineFetch_2.5.0_aarch64_adhoc.dmg')), true);
    assert.equal(existsSync(join(repo, '.pinefetch-publish.lock')), false);
});

test('publishing refuses an existing cleanup lock before running preparation or release commands', async t => {
    const { repo, publish, log } = await createPublishFixture(t);
    const lock = join(repo, '.pinefetch-publish.lock');
    await mkdir(lock);
    await writeFile(join(lock, '12345.pid'), 'Existing owner\n');
    const result = publish();
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /Another publish or cleanup/);
    assert.equal(await readFile(join(lock, '12345.pid'), 'utf8'), 'Existing owner\n');
    assert.equal(existsSync(log), false);
});
