import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import {
    existsSync,
    mkdirSync,
    mkdtempSync,
    readFileSync,
    readdirSync,
    rmSync,
    symlinkSync,
    writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { test } from 'node:test';
import {
    cleanupBuildArtifacts,
    cleanupPaths,
    formatCleanupReport,
    parseCleanupArgs,
    planCleanup,
    runCleanup,
    runtimePaths,
} from '../scripts/cleanup.mjs';

const gitEnv = {
    ...process.env,
    GIT_CONFIG_NOSYSTEM: '1',
    GIT_CONFIG_GLOBAL: '/dev/null',
    GIT_TERMINAL_PROMPT: '0',
};

function git(directory, ...args) {
    return execFileSync('git', ['-C', directory, ...args], {
        encoding: 'utf8',
        env: gitEnv,
        timeout: 10000,
        stdio: ['ignore', 'pipe', 'pipe'],
    });
}

function write(directory, filename, contents = 'generated') {
    const destination = resolve(directory, filename);
    const inside = relative(resolve(directory), destination);
    assert(!isAbsolute(filename) && inside && inside !== '..' && !inside.startsWith(`..${sep}`));
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, contents);
}

function temporaryDirectory(t, prefix) {
    const directory = mkdtempSync(join(tmpdir(), prefix));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    return directory;
}

function fixture(t) {
    const directory = temporaryDirectory(t, 'pinefetch-cleanup-test-');
    git(directory, 'init', '--quiet');
    write(directory, 'package.json', JSON.stringify({ name: 'pinefetch', version: '3.1.1' }));
    write(directory, 'src-tauri/Cargo.toml', '[package]\nname = "pinefetch"\nversion = "3.1.1"\n');
    git(directory, 'add', '--', 'package.json', 'src-tauri/Cargo.toml');
    for (const runtime of runtimePaths) {
        write(directory, `${runtime}/.keep`, 'source-owned keep\n');
        write(directory, `${runtime}/PLACEHOLDER`, 'source-owned placeholder\n');
        git(directory, 'add', '--', `${runtime}/.keep`, `${runtime}/PLACEHOLDER`);
    }
    return directory;
}

function populateOutputs(directory) {
    for (const filename of cleanupPaths) {
        write(directory, filename.endsWith('/Cargo.lock') ? filename : `${filename}/generated.bin`);
    }
    for (const runtime of runtimePaths) {
        write(directory, `${runtime}/bin/generated`, 'runtime binary');
        write(directory, `${runtime}/.inputs`, 'runtime cache stamp');
    }
}

test('cleanup accepts only its explicit preview, help and publish options', () => {
    assert.deepEqual(parseCleanupArgs([]), { dryRun: false, help: false, afterPublish: false });
    assert.deepEqual(parseCleanupArgs(['--dry-run']), { dryRun: true, help: false, afterPublish: false });
    assert.deepEqual(parseCleanupArgs(['--after-publish']), { dryRun: false, help: false, afterPublish: true });
    assert.equal(parseCleanupArgs(['--help']).help, true);
    assert.equal(parseCleanupArgs(['-h']).help, true);
    assert.throws(() => parseCleanupArgs(['--root', '/']), /unknown|option/i);
    assert.throws(() => parseCleanupArgs(['--dry-run', '--after-publish']));
});

test('cleanup removes its generated allowlist and runtime contents while preserving source and private files', t => {
    const directory = fixture(t);
    populateOutputs(directory);
    const protectedFiles = [
        'data/history.sqlite',
        'backups/history.sqlite',
        '.env',
        'node_modules/example/index.js',
        'docs/licenses/python-standalone/LICENSE.cpython.txt',
        'src-tauri/icons/icon.icns',
        'src-tauri/vendor/glib/src/lib.rs',
        'src-tauri/gen/manual-notes.txt',
        'src/images/mockups/history.webp',
        'screenshots/manual.png',
        'artifacts/private.sqlite',
        'artifacts/manual-export.txt',
        'downloads/original.mp4',
        'src-tauri/resources/custom-resource.txt',
    ];
    for (const filename of protectedFiles) write(directory, filename, 'keep');

    const report = cleanupBuildArtifacts({ directory });
    assert(report.entries.length > 0);
    assert(report.bytes > 0);
    for (const filename of cleanupPaths) assert.equal(existsSync(join(directory, filename)), false, filename);
    for (const runtime of runtimePaths) {
        assert.deepEqual(readdirSync(join(directory, runtime)).sort(), ['.keep', 'PLACEHOLDER']);
        assert.equal(readFileSync(join(directory, runtime, '.keep'), 'utf8'), 'source-owned keep\n');
        assert.equal(readFileSync(join(directory, runtime, 'PLACEHOLDER'), 'utf8'), 'source-owned placeholder\n');
    }
    for (const filename of protectedFiles)
        assert.equal(readFileSync(join(directory, filename), 'utf8'), 'keep', filename);
    assert.match(formatCleanupReport(report), /Removed/i);
    assert.equal(cleanupBuildArtifacts({ directory }).entries.length, 0);
});

test('preview reports the planned paths and sizes without deleting files or creating a lock', t => {
    const directory = fixture(t);
    write(directory, 'src-tauri/target/debug/output', '123456');
    write(directory, 'coverage/result.json', '1234');
    const plan = planCleanup(directory);
    const report = runCleanup({ directory, dryRun: true });
    assert.deepEqual(report.entries, plan.entries);
    assert.equal(report.bytes, plan.bytes);
    assert.equal(report.bytes, 10);
    assert.equal(report.dryRun, true);
    assert.equal(readFileSync(join(directory, 'src-tauri/target/debug/output'), 'utf8'), '123456');
    assert.equal(readFileSync(join(directory, 'coverage/result.json'), 'utf8'), '1234');
    assert.equal(existsSync(join(directory, '.pinefetch-publish.lock')), false);
    assert.match(formatCleanupReport(report), /Would remove/i);
    assert.match(formatCleanupReport(report), /src-tauri\/target/);
});

test('a tracked file in a fixed output blocks every deletion during preflight', t => {
    const directory = fixture(t);
    write(directory, 'src-tauri/target/debug/output', 'keep on refusal');
    write(directory, 'coverage/important.txt', 'tracked');
    git(directory, 'add', '--', 'coverage/important.txt');
    assert.throws(() => cleanupBuildArtifacts({ directory }), /tracked/i);
    assert.equal(readFileSync(join(directory, 'src-tauri/target/debug/output'), 'utf8'), 'keep on refusal');
    assert.equal(readFileSync(join(directory, 'coverage/important.txt'), 'utf8'), 'tracked');
});

test('a tracked file nested in generated runtime contents blocks every deletion', t => {
    const directory = fixture(t);
    write(directory, 'src-tauri/target/debug/output', 'keep on refusal');
    const filename = `${runtimePaths[0]}/bin/source-owned.txt`;
    write(directory, filename, 'tracked');
    git(directory, 'add', '--', filename);
    assert.throws(() => cleanupBuildArtifacts({ directory }), /tracked/i);
    assert.equal(readFileSync(join(directory, 'src-tauri/target/debug/output'), 'utf8'), 'keep on refusal');
    assert.equal(readFileSync(join(directory, filename), 'utf8'), 'tracked');
});

test('cleanup refuses symlink output roots, runtime roots and ancestors before deleting anything', t => {
    for (const filename of [
        'src-tauri/target',
        runtimePaths[0],
        'src-tauri/resources',
        `${runtimePaths[0]}/generated-link`,
    ]) {
        const directory = fixture(t);
        const outside = temporaryDirectory(t, 'pinefetch-cleanup-outside-');
        write(directory, 'coverage/keep-on-refusal', 'keep');
        write(outside, 'private.txt', 'outside');
        const destination = join(directory, filename);
        rmSync(destination, { recursive: true, force: true });
        mkdirSync(dirname(destination), { recursive: true });
        symlinkSync(outside, destination, 'dir');
        assert.throws(() => cleanupBuildArtifacts({ directory }), /symlink|symbolic/i, filename);
        assert.equal(readFileSync(join(directory, 'coverage/keep-on-refusal'), 'utf8'), 'keep', filename);
        assert.equal(readFileSync(join(outside, 'private.txt'), 'utf8'), 'outside', filename);
    }
});

test('symlinks inside disposable directories are removed without following their targets', t => {
    const directory = fixture(t);
    const outside = temporaryDirectory(t, 'pinefetch-cleanup-linked-data-');
    write(outside, 'private.sqlite', 'outside private data');
    write(directory, 'src-tauri/target/debug/output');
    write(directory, `${runtimePaths[0]}/bin/generated`);
    symlinkSync(outside, join(directory, 'src-tauri/target/debug/private-data'), 'dir');
    symlinkSync(outside, join(directory, runtimePaths[0], 'bin/private-data'), 'dir');
    cleanupBuildArtifacts({ directory });
    assert.equal(existsSync(join(directory, 'src-tauri/target')), false);
    assert.equal(existsSync(join(directory, runtimePaths[0], 'bin')), false);
    assert.equal(readFileSync(join(outside, 'private.sqlite'), 'utf8'), 'outside private data');
});

test('cleanup requires PineFetch package metadata and the exact Git checkout root', t => {
    const directory = fixture(t);
    write(directory, 'src-tauri/target/output', 'keep');
    write(directory, 'package.json', JSON.stringify({ name: 'another-app' }));
    assert.throws(() => cleanupBuildArtifacts({ directory }), /PineFetch|pinefetch/i);
    assert.equal(readFileSync(join(directory, 'src-tauri/target/output'), 'utf8'), 'keep');

    write(directory, 'package.json', JSON.stringify({ name: 'pinefetch' }));
    write(directory, 'src-tauri/Cargo.toml', '[package]\nname = "another-app"\n');
    assert.throws(() => cleanupBuildArtifacts({ directory }), /PineFetch|pinefetch/i);

    write(directory, 'nested/package.json', JSON.stringify({ name: 'pinefetch' }));
    write(directory, 'nested/src-tauri/Cargo.toml', '[package]\nname = "pinefetch"\n');
    assert.throws(() => cleanupBuildArtifacts({ directory: join(directory, 'nested') }), /checkout|root/i);
});

test('manual cleanup releases its own publish lock after success', t => {
    const directory = fixture(t);
    write(directory, 'src-tauri/target/output');
    const report = runCleanup({ directory });
    assert.equal(report.dryRun, false);
    assert.equal(existsSync(join(directory, 'src-tauri/target')), false);
    assert.equal(existsSync(join(directory, '.pinefetch-publish.lock')), false);
});

test('manual cleanup also releases its own lock when preflight refuses a tracked output', t => {
    const directory = fixture(t);
    write(directory, 'coverage/important.txt', 'tracked');
    git(directory, 'add', '--', 'coverage/important.txt');
    assert.throws(() => runCleanup({ directory }), /tracked/i);
    assert.equal(existsSync(join(directory, '.pinefetch-publish.lock')), false);
    assert.equal(readFileSync(join(directory, 'coverage/important.txt'), 'utf8'), 'tracked');
});

test('active, stale and incomplete publish locks prevent cleanup and remain untouched', t => {
    for (const owner of [`${process.pid}.pid`, '999999999.pid', null, 'unexpected.txt']) {
        const directory = fixture(t);
        write(directory, 'src-tauri/target/output', 'keep');
        const lock = join(directory, '.pinefetch-publish.lock');
        mkdirSync(lock);
        if (owner) write(directory, `.pinefetch-publish.lock/${owner}`, 'owner');
        const before = readdirSync(lock);
        assert.throws(() => runCleanup({ directory }), /lock|publish/i);
        assert.throws(() => runCleanup({ directory, dryRun: true }), /lock|publish/i);
        assert.equal(readFileSync(join(directory, 'src-tauri/target/output'), 'utf8'), 'keep');
        assert.deepEqual(readdirSync(lock), before);
        if (owner) assert.equal(readFileSync(join(lock, owner), 'utf8'), 'owner');
    }
});

test('cleanup refuses a symlink publish lock without changing its external destination', t => {
    const directory = fixture(t);
    const outside = temporaryDirectory(t, 'pinefetch-cleanup-lock-outside-');
    write(directory, 'src-tauri/target/output', 'keep');
    write(outside, 'private.txt', 'outside');
    symlinkSync(outside, join(directory, '.pinefetch-publish.lock'), 'dir');
    assert.throws(() => runCleanup({ directory }), /symlink|symbolic/i);
    assert.equal(readFileSync(join(directory, 'src-tauri/target/output'), 'utf8'), 'keep');
    assert.deepEqual(readdirSync(outside), ['private.txt']);
});

test('after-publish cleanup requires the parent publisher lock and does not release it', t => {
    const directory = fixture(t);
    write(directory, 'src-tauri/target/output', 'generated');
    assert.throws(() => runCleanup({ directory, afterPublish: true }), /lock|publish|parent/i);
    write(directory, `.pinefetch-publish.lock/${process.pid}.pid`, 'owner');
    assert.throws(() => runCleanup({ directory, afterPublish: true }), /lock|publish|parent/i);
    assert.equal(readFileSync(join(directory, 'src-tauri/target/output'), 'utf8'), 'generated');

    rmSync(join(directory, '.pinefetch-publish.lock'), { recursive: true });
    write(directory, `.pinefetch-publish.lock/${process.ppid}.pid`, 'owner');
    runCleanup({ directory, afterPublish: true });
    assert.equal(existsSync(join(directory, 'src-tauri/target')), false);
    assert.deepEqual(readdirSync(join(directory, '.pinefetch-publish.lock')), [`${process.ppid}.pid`]);
    assert.equal(readFileSync(join(directory, '.pinefetch-publish.lock', `${process.ppid}.pid`), 'utf8'), 'owner');
});

test('an active Whisper preparation lock blocks planning, preview and deletion without altering files', t => {
    const directory = fixture(t);
    write(directory, 'src-tauri/target/debug/output', 'keep build');
    write(directory, `${runtimePaths[0]}/bin/prepared`, 'keep runtime');
    write(directory, 'src-tauri/resources/.whisper-runtime.lock/owner.json', 'preparing');
    const operations = [
        () => planCleanup(directory),
        () => cleanupBuildArtifacts({ directory }),
        () => runCleanup({ directory, dryRun: true }),
        () => runCleanup({ directory }),
    ];
    for (const operation of operations) {
        assert.throws(operation, /Whisper runtime preparation is locked/);
        assert.equal(readFileSync(join(directory, 'src-tauri/target/debug/output'), 'utf8'), 'keep build');
        assert.equal(readFileSync(join(directory, runtimePaths[0], 'bin/prepared'), 'utf8'), 'keep runtime');
        assert.equal(
            readFileSync(join(directory, 'src-tauri/resources/.whisper-runtime.lock/owner.json'), 'utf8'),
            'preparing'
        );
        assert.equal(existsSync(join(directory, '.pinefetch-publish.lock')), false);
    }
});
