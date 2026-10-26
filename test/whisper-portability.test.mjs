import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdir, mkdtemp, realpath, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import {
    checkWhisperPortability,
    checkWhisperRuntimeTree,
    normalizeWhisperMachO,
} from '../scripts/check-whisper-portability.mjs';

const arm64 = 0x0100000c;
const x64 = 0x01000007;
const encodeVersion = value => {
    const [major, minor = 0, patch = 0] = value.split('.').map(Number);
    return major * 65536 + minor * 256 + patch;
};

// Structural Mach-O fixtures exercise the checker on every CI host. They are
// deliberately not executable programs or proof of a real runtime launching.
const image = ({
    cpu = arm64,
    subtype = 0,
    fileType = 6,
    dependencies = [],
    rpaths = [],
    installName = null,
    minimum = '11.0',
    platform = 1,
    little = true,
    extra = [],
} = {}) => {
    const u32 = (buffer, offset, value) =>
        little ? buffer.writeUInt32LE(value, offset) : buffer.writeUInt32BE(value, offset);
    const stringCommand = (command, value, prefixSize) => {
        const text = Buffer.from(value);
        const result = Buffer.alloc(Math.ceil((prefixSize + text.length + 1) / 8) * 8);
        u32(result, 0, command);
        u32(result, 4, result.length);
        u32(result, 8, prefixSize);
        text.copy(result, prefixSize);
        return result;
    };
    const commands = [
        ...dependencies.map(dependency => stringCommand(0x0c, dependency, 24)),
        ...rpaths.map(path => stringCommand(0x8000001c, path, 12)),
        ...(installName ? [stringCommand(0x0d, installName, 24)] : []),
        ...extra,
    ];
    if (minimum !== null) {
        const version = Buffer.alloc(24);
        u32(version, 0, 0x32);
        u32(version, 4, 24);
        u32(version, 8, platform);
        u32(version, 12, encodeVersion(minimum));
        commands.push(version);
    }
    const header = Buffer.alloc(32);
    u32(header, 0, 0xfeedfacf);
    u32(header, 4, cpu);
    u32(header, 8, subtype);
    u32(header, 12, fileType);
    u32(header, 16, commands.length);
    u32(
        header,
        20,
        commands.reduce((length, command) => length + command.length, 0)
    );
    return Buffer.concat([header, ...commands]);
};

const universal = (slices, wide = false) => {
    const tableSize = 8 + slices.length * (wide ? 32 : 20);
    let position = Math.ceil(tableSize / 64) * 64;
    const offsets = slices.map(slice => {
        const offset = position;
        position = Math.ceil((position + slice.bytes.length) / 64) * 64;
        return offset;
    });
    const result = Buffer.alloc(position);
    result.writeUInt32BE(wide ? 0xcafebabf : 0xcafebabe, 0);
    result.writeUInt32BE(slices.length, 4);
    slices.forEach((slice, index) => {
        const entry = 8 + index * (wide ? 32 : 20);
        result.writeUInt32BE(slice.cpu, entry);
        if (wide) {
            result.writeBigUInt64BE(BigInt(offsets[index]), entry + 8);
            result.writeBigUInt64BE(BigInt(slice.bytes.length), entry + 16);
        } else {
            result.writeUInt32BE(offsets[index], entry + 8);
            result.writeUInt32BE(slice.bytes.length, entry + 12);
        }
        slice.bytes.copy(result, offsets[index]);
    });
    return result;
};

async function fixture(t) {
    const workspace = await mkdtemp(join(tmpdir(), 'pinefetch-whisper-portability-'));
    t.after(() => rm(workspace, { recursive: true, force: true }));
    const root = join(workspace, 'runtime');
    await mkdir(root);
    const put = async (name, bytes) => {
        const path = join(root, name);
        await mkdir(dirname(path), { recursive: true });
        await writeFile(path, bytes);
        return path;
    };
    await put(
        'bin/python3.12',
        image({ fileType: 2, dependencies: ['@rpath/libpython.dylib'], rpaths: ['@executable_path/../lib'] })
    );
    await put(
        'lib/libpython.dylib',
        image({ dependencies: ['/usr/lib/libSystem.B.dylib', '@loader_path/libhelper.dylib'] })
    );
    await put('lib/libhelper.dylib', image());
    await put(
        'lib/python3.12/site-packages/decoder.so',
        image({ fileType: 8, dependencies: ['@rpath/libhelper.dylib'] })
    );
    return {
        root,
        workspace,
        put,
        check: options => checkWhisperPortability(root, { arch: 'arm64', maxMacOSVersion: '11.0', ...options }),
    };
}

test('standalone runtime closure resolves loader, executable and inherited runpaths without host tools', async t => {
    const f = await fixture(t);
    await symlink('python3.12', join(f.root, 'bin/python3'));
    await symlink('python3', join(f.root, 'bin/python'));
    const result = await f.check();
    assert.deepEqual(result, { machOFiles: 4, symlinks: 2, architectures: ['arm64'], minimumMacOS: '11.0.0' });
    const moved = join(f.workspace, 'relocated runtime');
    const { rename } = await import('node:fs/promises');
    await rename(f.root, moved);
    assert.deepEqual(
        await checkWhisperPortability(moved, { arch: 'arm64', executable: 'bin/python', maxMacOSVersion: '11.0' }),
        result
    );
});

test('runtime tree rejects venvs, absolute/internal-looking links, escapes, broken links and cycles', async t => {
    for (const scenario of ['venv', 'absolute', 'escape', 'chain', 'broken', 'cycle']) {
        await t.test(scenario, async child => {
            const f = await fixture(child);
            if (scenario === 'venv') await f.put('nested/pyvenv.cfg', 'home = /opt/homebrew/bin');
            else if (scenario === 'absolute') await symlink(join(f.root, 'bin/python3.12'), join(f.root, 'alias'));
            else if (scenario === 'escape') await symlink('../outside', join(f.root, 'alias'));
            else if (scenario === 'chain') {
                await symlink('alias2', join(f.root, 'alias1'));
                await symlink('../outside', join(f.root, 'alias2'));
            } else if (scenario === 'broken') await symlink('missing', join(f.root, 'alias'));
            else await symlink('.', join(f.root, 'cycle'));
            if (scenario === 'cycle') {
                // Internal directory aliases are safe: the walker does not follow
                // them, so they cannot recurse indefinitely or escape the tree.
                assert.equal((await checkWhisperRuntimeTree(f.root)).symlinks, 1);
                await symlink('loop', join(f.root, 'loop'));
            }
            await assert.rejects(checkWhisperRuntimeTree(f.root), /venv|symlink/);
        });
    }
});

test('external build-host dependencies and even absolute current-bundle paths fail relocation rules', async t => {
    const f = await fixture(t);
    for (const dependency of [
        '/opt/homebrew/lib/libpython.dylib',
        '/Users/builder/Python.framework/Python',
        join(f.root, 'lib/libhelper.dylib'),
        '/usr/lib/../../opt/homebrew/libfoo.dylib',
    ]) {
        await f.put('lib/libhelper.dylib', image({ dependencies: [dependency] }));
        await assert.rejects(f.check(), /absolute non-system library path/);
    }
});

test('a dylib build-time identity is not loaded unless an actual dependency references it', async t => {
    const f = await fixture(t);
    await f.put('lib/libhelper.dylib', image({ installName: '/DLC/av/.dylibs/libhelper.dylib' }));
    assert.equal((await f.check()).machOFiles, 4);
    await f.put('lib/libpython.dylib', image({ dependencies: ['/DLC/av/.dylibs/libhelper.dylib'] }));
    await assert.rejects(f.check(), /absolute non-system library path/);
});

test('a transitive dylib retains its caller runpath context instead of being re-rooted', async t => {
    const f = await fixture(t);
    await f.put(
        'lib/libpython.dylib',
        image({ dependencies: ['@loader_path/libhelper.dylib'], rpaths: ['@loader_path/../private'] })
    );
    await f.put('lib/libhelper.dylib', image({ dependencies: ['@rpath/libtransitive.dylib'] }));
    await f.put('private/libtransitive.dylib', image());
    // This dlopen root also inherits the private library path before using the
    // helper; loading the helper without either caller's stack would be invalid.
    await f.put(
        'lib/python3.12/site-packages/decoder.so',
        image({ fileType: 8, dependencies: ['@rpath/libhelper.dylib'], rpaths: ['@executable_path/../private'] })
    );
    assert.equal((await f.check()).machOFiles, 5);
});

test('closure rejects missing, unanchored, escaping and non-Mach-O dependencies in Python extensions', async t => {
    const f = await fixture(t);
    await f.put('lib/not-a-library.dylib', 'plain text');
    for (const [dependency, expected] of [
        ['@rpath/missing.dylib', /unresolved dependency/],
        ['libhelper.dylib', /unanchored library search path/],
        ['@loader_path/../../../../../outside.dylib', /escapes the runtime/],
        ['@rpath/../../../../usr/lib/libSystem.B.dylib', /escapes the runtime/],
        ['@rpath/not-a-library.dylib', /not a compatible bundled dylib/],
    ]) {
        await f.put('lib/python3.12/site-packages/decoder.so', image({ fileType: 8, dependencies: [dependency] }));
        await assert.rejects(f.check(), expected);
    }
});

test('absolute and escaping runpaths are rejected even when no library currently uses them', async t => {
    const f = await fixture(t);
    for (const [path, expected] of [
        ['/opt/homebrew/lib', /absolute non-system library path/],
        ['@loader_path/../../../../usr/lib', /escapes the runtime/],
        ['@rpath/recursive', /unanchored library search path/],
    ]) {
        await f.put('lib/libhelper.dylib', image({ rpaths: [path] }));
        await assert.rejects(f.check(), expected);
    }
});

test('every bundled dynamic image must match architecture, macOS platform and deployment ceiling', async t => {
    const f = await fixture(t);
    for (const [options, expected] of [
        [{ cpu: x64 }, /arm64 Mach-O slice/],
        [{ subtype: 2 }, /arm64e-only/],
        [{ minimum: '13.5' }, /requires macOS 13.5.0, exceeds 11.0/],
        [{ minimum: null }, /missing macOS deployment version/],
        [{ platform: 2 }, /platform other than macOS/],
    ]) {
        await f.put('lib/libhelper.dylib', image(options));
        await assert.rejects(f.check(), expected);
    }
});

test('universal Mach-O and fat64 slices are parsed while static archives and objects stay outside runtime closure', async t => {
    const f = await fixture(t);
    for (const wide of [false, true]) {
        await f.put(
            'lib/libhelper.dylib',
            universal(
                [
                    { cpu: arm64, bytes: image() },
                    { cpu: x64, bytes: image({ cpu: x64, little: false }) },
                ],
                wide
            )
        );
        await f.put('lib/libpython.a', universal([{ cpu: arm64, bytes: Buffer.from('!<arch>\n') }], wide));
        await f.put('lib/build.o', image({ cpu: x64, fileType: 1, minimum: null }));
        const result = await f.check();
        assert.equal(result.machOFiles, 4);
        assert.deepEqual(result.architectures, ['arm64', 'x64']);
    }
});

test('malformed Mach-O command bounds, strings and universal slice descriptors fail closed', async t => {
    const f = await fixture(t);
    const zeroSize = image();
    zeroSize.writeUInt32LE(0, 36);
    const shortString = image({ dependencies: ['@rpath/libfoo.dylib'] });
    shortString.writeUInt32LE(500, 40);
    const unterminated = image({ dependencies: ['@rpath/libfoo.dylib'] });
    unterminated.fill(65, 56, 32 + unterminated.readUInt32LE(36));
    const overflow = image();
    overflow.writeUInt32LE(0xffffffff, 20);
    const fatMismatch = universal([{ cpu: x64, bytes: image() }]);
    const fatOffset = universal([{ cpu: arm64, bytes: image() }], true);
    fatOffset.writeBigUInt64BE(2n ** 63n, 16);
    for (const bytes of [
        Buffer.from([0xcf, 0xfa, 0xed, 0xfe]),
        zeroSize,
        shortString,
        unterminated,
        overflow,
        fatMismatch,
        fatOffset,
    ]) {
        await f.put('lib/libhelper.dylib', bytes);
        await assert.rejects(f.check(), /malformed Mach-O/);
    }
});

test('loader environment commands cannot bypass the checked library-resolution rules', async t => {
    const f = await fixture(t);
    const command = Buffer.alloc(16);
    command.writeUInt32LE(0x27, 0);
    command.writeUInt32LE(16, 4);
    await f.put('bin/python3.12', image({ fileType: 2, extra: [command] }));
    await assert.rejects(f.check(), /LC_DYLD_ENVIRONMENT/);
});

test('checker CLI reports the checked closure and exits nonzero for an incompatible runtime', async t => {
    const f = await fixture(t);
    const checker = fileURLToPath(new URL('../scripts/check-whisper-portability.mjs', import.meta.url));
    const command = () =>
        spawnSync(process.execPath, [checker, f.root, '--arch', 'arm64', '--max-macos', '11.0'], {
            encoding: 'utf8',
            timeout: 10000,
        });
    const passed = command();
    assert.equal(passed.status, 0, passed.stderr);
    assert.match(passed.stdout, /4 Mach-O files/);
    await f.put('lib/libhelper.dylib', image({ minimum: '12.0' }));
    const failed = command();
    assert.equal(failed.status, 1);
    assert.match(failed.stderr, /requires macOS 12.0.0/);
    const alias = join(f.workspace, 'checker-alias.mjs');
    await symlink(checker, alias);
    const aliased = spawnSync(process.execPath, [alias, f.root, '--arch', 'arm64', '--max-macos', '11.0'], {
        encoding: 'utf8',
        timeout: 10000,
    });
    assert.equal(aliased.status, 1, 'invocation through a symlink must run the check rather than silently succeed');
    assert.match(aliased.stderr, /requires macOS 12.0.0/);
});

test('Mach-O normalizer leaves portable images untouched and signs only images it patches', async t => {
    const f = await fixture(t);
    const calls = [];
    const runImpl = (command, args, options) => calls.push({ command, args, options });
    assert.deepEqual(await normalizeWhisperMachO(f.root, { runImpl }), {
        filesPatched: 0,
        removedRpaths: 0,
        normalizedIds: 0,
    });
    assert.deepEqual(calls, []);
    const file = await f.put(
        'lib/libhelper.dylib',
        image({
            installName: '/DLC/av/.dylibs/libhelper.dylib',
            rpaths: ['/tmp/vendor/lib', '@loader_path', '/usr/lib'],
        })
    );
    const canonicalFile = await realpath(file);
    assert.deepEqual(await normalizeWhisperMachO(f.root, { runImpl }), {
        filesPatched: 1,
        removedRpaths: 1,
        normalizedIds: 1,
    });
    assert.deepEqual(
        calls.map(({ command, args }) => [command, args]),
        [
            ['/usr/bin/install_name_tool', ['-delete_rpath', '/tmp/vendor/lib', canonicalFile]],
            ['/usr/bin/install_name_tool', ['-id', '@rpath/libhelper.dylib', canonicalFile]],
            ['/usr/bin/codesign', ['--force', '--sign', '-', canonicalFile]],
        ]
    );
    assert(calls.every(call => call.options.timeout === 30000 && call.options.stdio === 'pipe'));
});

test('Mach-O normalizer never rewrites load dependencies to hide a missing external library', async t => {
    const f = await fixture(t);
    await f.put(
        'lib/libhelper.dylib',
        image({ dependencies: ['/opt/homebrew/lib/missing.dylib'], rpaths: ['/tmp/vendor/lib'] })
    );
    const calls = [];
    await normalizeWhisperMachO(f.root, { runImpl: (command, args) => calls.push({ command, args }) });
    assert.equal(calls.length, 2);
    assert(calls.every(call => !call.args.includes('-change')));
    await assert.rejects(f.check(), /absolute non-system library path: \/opt\/homebrew\/lib\/missing.dylib/);
});

test('Mach-O normalizer stops on tool failures and validates the tree and native headers before patching', async t => {
    const f = await fixture(t);
    await f.put('lib/libpython.dylib', image({ rpaths: ['/tmp/vendor/lib'] }));
    const calls = [];
    await assert.rejects(
        normalizeWhisperMachO(f.root, {
            runImpl: (command, args) => {
                calls.push({ command, args });
                throw new Error('controlled tool failure');
            },
        }),
        /Whisper Mach-O normalization failed: lib\/libpython.dylib: controlled tool failure/
    );
    assert.equal(calls.length, 1);
    assert.equal(calls[0].args[0], '-delete_rpath');
    await f.put('lib/libhelper.dylib', Buffer.from([0xcf, 0xfa, 0xed, 0xfe]));
    await assert.rejects(
        normalizeWhisperMachO(f.root, { runImpl: () => assert.fail('must validate before mutation') }),
        /malformed Mach-O/
    );
});
