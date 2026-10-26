import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
    chmodSync,
    copyFileSync,
    existsSync,
    mkdirSync,
    mkdtempSync,
    readFileSync,
    readdirSync,
    readlinkSync,
    renameSync,
    rmSync,
    statSync,
    symlinkSync,
    writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import test from 'node:test';
import { prepareWhisperRuntime } from '../scripts/prepare-whisper-runtime.mjs';

// Real archive hashing/extraction, filesystem publication and process execution;
// only the downloaded Python/package behavior is a controlled executable fixture.
const fixture = t => {
    const root = mkdtempSync(join(tmpdir(), 'pinefetch-whisper-standalone-'));
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
    const requirements = join(scripts, 'whisper-requirements.txt');
    const licenses = join(root, 'docs/licenses/python-standalone');
    mkdirSync(licenses, { recursive: true });
    writeFileSync(join(licenses, 'NOTICE'), 'Synthetic standalone-distribution fixture notice.\n');
    const controls = join(root, 'fixture-controls.json');
    const runtime = join(root, 'src-tauri/resources/whisper-runtime');
    mkdirSync(runtime, { recursive: true });
    const sourceFiles = { '.keep': 'source-owned fixture keep\n', PLACEHOLDER: 'source-owned fixture placeholder\n' };
    for (const [name, content] of Object.entries(sourceFiles)) writeFileSync(join(runtime, name), content);
    const archiveRoot = join(root, 'archive');
    const extracted = join(archiveRoot, 'python');
    mkdirSync(join(extracted, 'bin'), { recursive: true });
    mkdirSync(join(extracted, 'lib/python3.12'), { recursive: true });
    writeFileSync(join(extracted, 'LICENSE'), 'Synthetic archive fixture, not a Python distribution.\n');
    const executable = join(extracted, 'bin/python3.12');
    writeFileSync(
        executable,
        `#!${process.execPath}
const fs = require('node:fs');
const path = require('node:path');
const args = process.argv.slice(2);
const prefix = path.dirname(path.dirname(process.argv[1]));
const controlFile = ${JSON.stringify(controls)};
const control = fs.existsSync(controlFile) ? JSON.parse(fs.readFileSync(controlFile)) : {};
if (args[0] !== '-I' || args[1] !== '-B') throw new Error('Fixture Python must be isolated and must not write bytecode');
const isolated = args.slice(2);
if (isolated[0] === '-m' && isolated[1] === 'pip') {
    if (control.fail === 'install') throw new Error('Fixture package installation failed');
    const requirements = fs.readFileSync(isolated[isolated.indexOf('-r') + 1], 'utf8');
    const version = name => requirements.match(new RegExp('^' + name + '==([^\\\\s]+)', 'm'))?.[1];
    fs.writeFileSync(path.join(prefix, 'versions.json'), JSON.stringify([version('faster-whisper'), version('av'), '3.12.15']));
    fs.writeFileSync(path.join(prefix, 'generation'), control.generation || 'original');
} else if (path.basename(isolated[0]) === 'check-whisper-runtime.py') {
    const installed = JSON.parse(fs.readFileSync(path.join(prefix, 'versions.json')));
    if (JSON.stringify(installed) !== JSON.stringify(isolated.slice(1))) throw new Error('Fixture installed runtime versions do not match');
    const generation = fs.readFileSync(path.join(prefix, 'generation'), 'utf8');
    const final = prefix === ${JSON.stringify(runtime)};
    if (generation === 'replacement' && control.fail === 'stage-check' && !final) throw new Error('Fixture staged audio decode failed');
    if (generation === 'replacement' && control.fail === 'final-check' && final) throw new Error('Fixture relocated audio decode failed');
} else {
    throw new Error('Unexpected fixture Python arguments: ' + JSON.stringify(args));
}
`
    );
    chmodSync(executable, 0o755);
    symlinkSync('python3.12', join(extracted, 'bin/python3'));
    symlinkSync('python3', join(extracted, 'bin/python'));
    const archive = join(root, 'python-fixture.tar.gz');
    execFileSync('tar', ['-czf', archive, '-C', archiveRoot, 'python']);
    let bytes = readFileSync(archive);
    const manifest = join(scripts, 'whisper-python.json');
    const archiveUrl =
        'https://github.com/astral-sh/python-build-standalone/releases/download/fixture/python-fixture.tar.gz';
    writeFileSync(
        manifest,
        JSON.stringify({
            version: '3.12.15',
            release: '20261021',
            targets: {
                'linux-x64': {
                    url: archiveUrl,
                    sha256: createHash('sha256').update(bytes).digest('hex'),
                    executable: 'bin/python3.12',
                },
            },
        })
    );
    const downloads = [];
    const commands = [];
    const prepare = () =>
        prepareWhisperRuntime({
            root,
            platform: 'linux',
            arch: 'x64',
            fetchImpl: async url => {
                downloads.push(String(url));
                assert.equal(String(url), archiveUrl);
                return new Response(bytes, { status: 200 });
            },
            runImpl: (file, args, options) => {
                commands.push({ file, args: [...args], env: options?.env });
                return execFileSync(file, args, {
                    ...options,
                    stdio: 'pipe',
                    timeout: Math.min(options?.timeout || 30000, 30000),
                });
            },
        });
    const pythonCalls = () => commands.filter(command => basename(command.file).startsWith('python'));
    const pipCalls = () => pythonCalls().filter(command => command.args.includes('pip'));
    const checks = () =>
        pythonCalls().filter(command => command.args.some(arg => arg.endsWith('check-whisper-runtime.py')));
    const fingerprint = () => readFileSync(join(runtime, '.pinefetch-whisper-runtime'), 'utf8');
    const rebuild = (fail, generation = 'replacement') => {
        writeFileSync(controls, JSON.stringify({ fail, generation }));
        writeFileSync(requirements, `${readFileSync(requirements, 'utf8')}\n# Fixture rebuild ${fail}\n`);
    };
    const assertClean = () => {
        assert.deepEqual(readdirSync(join(root, 'src-tauri/resources')), ['whisper-runtime']);
        for (const [name, content] of Object.entries(sourceFiles))
            assert.equal(readFileSync(join(runtime, name), 'utf8'), content);
    };
    const repackArchive = (members = ['python']) => {
        execFileSync('tar', ['-czf', archive, '-C', archiveRoot, ...members]);
        bytes = readFileSync(archive);
        const value = JSON.parse(readFileSync(manifest, 'utf8'));
        value.targets['linux-x64'].sha256 = createHash('sha256').update(bytes).digest('hex');
        writeFileSync(manifest, JSON.stringify(value));
    };
    return {
        root,
        archiveRoot,
        runtime,
        requirements,
        manifest,
        downloads,
        commands,
        prepare,
        pythonCalls,
        pipCalls,
        checks,
        fingerprint,
        rebuild,
        repackArchive,
        assertClean,
    };
};

const treeSnapshot = directory =>
    readdirSync(directory, { withFileTypes: true })
        .sort((a, b) => a.name.localeCompare(b.name))
        .map(entry => {
            const path = join(directory, entry.name);
            if (entry.isSymbolicLink()) return [entry.name, 'link', readlinkSync(path)];
            if (entry.isDirectory()) return [entry.name, 'directory', treeSnapshot(path)];
            return [entry.name, 'file', statSync(path).mode, readFileSync(path).toString('base64')];
        });

test(
    'Whisper preparation installs a checksummed standalone archive with isolated pinned wheels',
    { timeout: 60000 },
    async t => {
        const f = fixture(t);
        await f.prepare();
        assert.equal(f.downloads.length, 1);
        assert.equal(f.pipCalls().length, 1);
        const pip = f.pipCalls()[0];
        assert.deepEqual(pip.args.slice(0, 6), ['-I', '-B', '-m', 'pip', '--isolated', 'install']);
        for (const argument of [
            '--disable-pip-version-check',
            '--no-compile',
            '--only-binary=:all:',
            '--require-hashes',
        ])
            assert(pip.args.includes(argument));
        assert.equal(pip.args[pip.args.indexOf('-r') + 1], f.requirements);
        const requirements = readFileSync(f.requirements, 'utf8');
        assert.match(requirements, /^faster-whisper==1\.2\.1\b/m);
        assert.match(requirements, /^av==17\.1\.0\b/m);
        assert.match(requirements, /--hash=sha256:[a-f0-9]{64}/);
        for (const command of f.pythonCalls()) {
            assert(command.file.startsWith(join(f.root, 'src-tauri/resources')));
            assert.equal(command.args.includes('venv'), false);
            assert(command.env && command.env !== process.env);
            for (const name of Object.keys(command.env)) assert(!/^(PYTHON|DYLD_)/.test(name), name);
        }
        assert.equal(f.checks().length, 2, 'Stage and final relocated runtime must both execute the audio check');
        assert.notEqual(f.checks()[0].file, f.checks()[1].file);
        assert(f.checks()[1].file.startsWith(f.runtime));
        assert.equal(existsSync(join(f.runtime, 'pyvenv.cfg')), false);
        assert.equal(typeof JSON.parse(f.fingerprint()).fingerprint, 'string');
        f.assertClean();
    }
);

test(
    'Whisper cache reuse executes runtime checks and rejects legacy stamps or stale installed versions',
    { timeout: 60000 },
    async t => {
        const f = fixture(t);
        await f.prepare();
        const checks = f.checks().length;
        await f.prepare();
        assert.equal(f.downloads.length, 1);
        assert.equal(f.pipCalls().length, 1);
        assert.equal(f.checks().length, checks + 1);
        writeFileSync(join(f.runtime, '.pinefetch-whisper-runtime'), 'faster-whisper=1.2.1\npython=3.12.14\n');
        await f.prepare();
        assert.equal(f.downloads.length, 2);
        assert.equal(f.pipCalls().length, 2);
        writeFileSync(join(f.runtime, 'versions.json'), JSON.stringify(['1.2.1', '19.0.1', '3.12.15']));
        await f.prepare();
        assert.equal(f.downloads.length, 3);
        assert.equal(f.pipCalls().length, 3);
        assert.deepEqual(JSON.parse(readFileSync(join(f.runtime, 'versions.json'), 'utf8')), [
            '1.2.1',
            '17.1.0',
            '3.12.15',
        ]);
        f.assertClean();
    }
);

test(
    'Whisper checksum mismatch rejects the download before extraction and preserves the existing runtime',
    { timeout: 60000 },
    async t => {
        const f = fixture(t);
        await f.prepare();
        const previous = treeSnapshot(f.runtime);
        const oldCommands = f.commands.length;
        const manifest = JSON.parse(readFileSync(f.manifest, 'utf8'));
        manifest.targets['linux-x64'].sha256 = '0'.repeat(64);
        writeFileSync(f.manifest, JSON.stringify(manifest));
        await assert.rejects(f.prepare(), /checksum|sha.?256/i);
        assert.equal(
            f.commands.slice(oldCommands).some(command => basename(command.file) === 'tar'),
            false
        );
        assert.deepEqual(treeSnapshot(f.runtime), previous);
        f.assertClean();
    }
);

test('Whisper restores a previous runtime after interrupted directory publication', { timeout: 60000 }, async t => {
    const f = fixture(t);
    await f.prepare();
    const previous = treeSnapshot(f.runtime);
    renameSync(f.runtime, `${f.runtime}.previous`);
    mkdirSync(f.runtime);
    writeFileSync(join(f.runtime, 'incomplete-replacement'), 'Not a usable runtime');
    const result = await f.prepare();
    assert.equal(result.reused, true);
    assert.equal(f.downloads.length, 1);
    assert.equal(f.pipCalls().length, 1);
    assert.deepEqual(treeSnapshot(f.runtime), previous);
    f.assertClean();
});

test(
    'Whisper rejects an archive member outside python/ before extraction or Python execution',
    { timeout: 60000 },
    async t => {
        const f = fixture(t);
        await f.prepare();
        const previous = treeSnapshot(f.runtime);
        const before = f.commands.length;
        writeFileSync(join(f.archiveRoot, 'outside-root'), 'This entry must not be extracted');
        f.repackArchive(['python', 'outside-root']);
        await assert.rejects(f.prepare(), /unsafe member path/i);
        assert.equal(
            f.commands.slice(before).some(command => command.args.includes('-xzf')),
            false
        );
        assert.equal(
            f.commands.slice(before).some(command => basename(command.file).startsWith('python')),
            false
        );
        assert.deepEqual(treeSnapshot(f.runtime), previous);
        f.assertClean();
    }
);

test('Whisper rejects an absolute runtime symlink before package installation', { timeout: 60000 }, async t => {
    const f = fixture(t);
    await f.prepare();
    const previous = treeSnapshot(f.runtime);
    const before = f.commands.length;
    symlinkSync('/outside-fixture/not-a-bundled-library', join(f.archiveRoot, 'python/lib/host-library'));
    f.repackArchive();
    await assert.rejects(f.prepare(), /absolute symlink|not portable/i);
    assert.equal(
        f.commands.slice(before).some(command => basename(command.file).startsWith('python')),
        false
    );
    assert.deepEqual(treeSnapshot(f.runtime), previous);
    f.assertClean();
});

for (const [mode, message] of [
    ['install', /installation failed/],
    ['stage-check', /staged audio decode failed/],
    ['final-check', /relocated audio decode failed/],
]) {
    test(
        `Whisper ${mode} failure preserves the previous runtime and removes staged replacements`,
        { timeout: 60000 },
        async t => {
            const f = fixture(t);
            await f.prepare();
            const previous = treeSnapshot(f.runtime);
            f.rebuild(mode);
            await assert.rejects(f.prepare(), message);
            assert.deepEqual(treeSnapshot(f.runtime), previous);
            f.assertClean();
        }
    );
}

test('Whisper first-build decode failure never publishes a ready marker', { timeout: 60000 }, async t => {
    const f = fixture(t);
    f.rebuild('stage-check');
    await assert.rejects(f.prepare(), /staged audio decode failed/);
    assert.equal(existsSync(join(f.runtime, '.pinefetch-whisper-runtime')), false);
    assert.equal(existsSync(join(f.runtime, 'bin/python3.12')), false);
    f.assertClean();
});
