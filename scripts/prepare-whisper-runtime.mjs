import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { cp, mkdir, mkdtemp, readFile, readdir, realpath, rename, rm, stat, writeFile } from 'node:fs/promises';
import { isAbsolute, join, posix } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
    checkWhisperPortability,
    checkWhisperRuntimeTree,
    normalizeWhisperMachO,
} from './check-whisper-portability.mjs';

const repositoryRoot = fileURLToPath(new URL('../', import.meta.url));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const exists = async path => {
    try {
        await stat(path);
        return true;
    } catch (error) {
        if (error.code === 'ENOENT') return false;
        throw error;
    }
};

// Do not let the build host's Python, user site, pip config or dynamic-loader
// overrides turn a self-contained distribution back into a host-dependent one.
export function whisperEnvironment(home, platform = process.platform) {
    const environment = {
        HOME: home,
        USERPROFILE: home,
        TMPDIR: home,
        TMP: home,
        TEMP: home,
        LANG: 'en_US.UTF-8',
        PIP_CONFIG_FILE: platform === 'win32' ? 'nul' : '/dev/null',
        PATH: platform === 'win32' ? join(process.env.SystemRoot || 'C:\\Windows', 'System32') : '/usr/bin:/bin',
    };
    if (platform === 'win32') {
        environment.SystemRoot = process.env.SystemRoot || 'C:\\Windows';
        environment.WINDIR = environment.SystemRoot;
    }
    return environment;
}

export async function prepareWhisperRuntime({
    root = repositoryRoot,
    platform = process.platform,
    arch = process.arch,
    fetchImpl = globalThis.fetch,
    runImpl = execFileSync,
} = {}) {
    const scripts = join(root, 'scripts');
    const inputs = [
        'whisper-python.json',
        'whisper-requirements.txt',
        'whisper-runtime.version',
        'whisper-av.version',
        'check-whisper-runtime.py',
        'check-whisper-portability.mjs',
        'prepare-whisper-runtime.mjs',
    ];
    const bytes = await Promise.all(inputs.map(name => readFile(join(scripts, name))));
    const manifest = JSON.parse(bytes[0]);
    const targetName = platform + '-' + arch;
    const target = manifest.targets[targetName];
    if (!target) throw new Error('Unsupported Whisper runtime target: ' + targetName);
    const whisperVersion = bytes[2].toString().trim();
    const avVersion = bytes[3].toString().trim();
    for (const version of [manifest.version, whisperVersion, avVersion]) {
        if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error('Invalid pinned Whisper/Python version: ' + version);
    }
    if (
        !/^https:\/\/github\.com\/astral-sh\/python-build-standalone\/releases\/download\//.test(target.url) ||
        !/^[a-f0-9]{64}$/.test(target.sha256) ||
        isAbsolute(target.executable) ||
        target.executable.split(/[\\/]/).some(part => part === '..')
    ) {
        throw new Error('Invalid standalone Python artifact manifest for ' + targetName);
    }
    for (const pin of ['faster-whisper==' + whisperVersion, 'av==' + avVersion]) {
        if (
            !bytes[1]
                .toString()
                .split(/\r?\n/)
                .some(line => line.startsWith(pin + ' '))
        )
            throw new Error('Whisper requirements do not contain the expected pin: ' + pin);
    }

    const licenses = join(root, 'docs/licenses/python-standalone');
    const licenseFiles = (await readdir(licenses)).sort();
    const licenseBytes = await Promise.all(licenseFiles.map(name => readFile(join(licenses, name))));
    const fingerprint = hash(Buffer.concat([Buffer.from(targetName), ...bytes, ...licenseBytes]));
    const resources = join(root, 'src-tauri/resources');
    const runtime = join(resources, 'whisper-runtime');
    const previous = runtime + '.previous';
    const lock = join(resources, '.whisper-runtime.lock');
    await mkdir(resources, { recursive: true });
    try {
        await mkdir(lock);
    } catch (error) {
        if (error.code === 'EEXIST')
            throw new Error(
                'Whisper preparation is already locked at ' +
                    lock +
                    '. Check for another build before removing a stale lock.',
                { cause: error }
            );
        throw error;
    }
    let work;
    try {
        work = await mkdtemp(join(resources, '.whisper-runtime-'));
        const home = join(work, 'home');
        await mkdir(home);
        const environment = whisperEnvironment(home, platform);
        const options = { cwd: home, env: environment, encoding: 'utf8', timeout: 60000, maxBuffer: 16 * 1024 * 1024 };
        const verify = async directory => {
            const report =
                platform === 'darwin'
                    ? await checkWhisperPortability(directory, {
                          executable: target.executable,
                          arch,
                          maxMacOSVersion: target.minimumMacOS,
                      })
                    : await checkWhisperRuntimeTree(directory);
            const output = runImpl(
                join(directory, target.executable),
                ['-I', '-B', join(scripts, 'check-whisper-runtime.py'), whisperVersion, avVersion, manifest.version],
                options
            );
            if (output) process.stdout.write(output);
            return report;
        };
        const isCurrent = async directory => {
            try {
                const marker = JSON.parse(await readFile(join(directory, '.pinefetch-whisper-runtime'), 'utf8'));
                if (marker.fingerprint !== fingerprint) return false;
                await verify(directory);
                return true;
            } catch {
                return false;
            }
        };

        // Recover a build interrupted between the two atomic directory renames.
        if (await exists(previous)) {
            if ((await exists(runtime)) && (await isCurrent(runtime))) await rm(previous, { recursive: true });
            else {
                await rm(runtime, { recursive: true, force: true });
                await rename(previous, runtime);
            }
        }
        if (await isCurrent(runtime)) {
            console.log('Reusing bundled whisper runtime at: ' + runtime);
            return { runtime, reused: true };
        }

        console.log('Preparing standalone Python ' + manifest.version + ' for ' + targetName);
        const response = await fetchImpl(target.url, { signal: AbortSignal.timeout(120000) });
        if (!response.ok) throw new Error('Standalone Python download failed: HTTP ' + response.status);
        const archiveBytes = Buffer.from(await response.arrayBuffer());
        if (hash(archiveBytes) !== target.sha256) throw new Error('Standalone Python checksum verification failed.');
        const archive = join(work, 'python.tar.gz');
        await writeFile(archive, archiveBytes);
        // Only the checksum-pinned upstream archive is extracted, and every
        // member must stay below its single python/ root.
        const members = runImpl('tar', ['-tzf', archive], { ...options, env: process.env })
            .trim()
            .split(/\r?\n/);
        if (
            members.some(
                name =>
                    !name.startsWith('python/') ||
                    name.includes('\\') ||
                    name.split('/').includes('..') ||
                    posix.normalize(name).startsWith('../')
            )
        )
            throw new Error('Standalone Python archive contains an unsafe member path.');
        runImpl('tar', ['-xzf', archive, '-C', work], { ...options, env: process.env });
        const staged = join(work, 'python');
        await checkWhisperRuntimeTree(staged);
        runImpl(
            join(staged, target.executable),
            [
                '-I',
                '-B',
                '-m',
                'pip',
                '--isolated',
                'install',
                '--disable-pip-version-check',
                '--no-compile',
                '--only-binary=:all:',
                '--require-hashes',
                '--index-url',
                'https://pypi.org/simple',
                '-r',
                join(scripts, 'whisper-requirements.txt'),
            ],
            { ...options, timeout: 600000, stdio: 'inherit' }
        );
        if (platform === 'darwin') {
            const normalized = await normalizeWhisperMachO(staged, { runImpl });
            console.log(
                'Removed ' +
                    normalized.removedRpaths +
                    ' external build runpaths; normalized ' +
                    normalized.normalizedIds +
                    ' library identities.'
            );
        }
        await cp(licenses, join(staged, 'licenses/python-standalone'), { recursive: true });
        // These tracked resource sentinels must stay byte-for-byte unchanged.
        for (const name of ['.keep', 'PLACEHOLDER']) {
            await cp(join(runtime, name), join(staged, name));
        }
        await verify(staged);
        await writeFile(
            join(staged, '.pinefetch-whisper-runtime'),
            JSON.stringify(
                {
                    fingerprint,
                    target: targetName,
                    python: manifest.version,
                    release: manifest.release,
                    fasterWhisper: whisperVersion,
                    av: avVersion,
                },
                null,
                4
            ) + '\n'
        );

        const hadRuntime = await exists(runtime);
        if (hadRuntime) await rename(runtime, previous);
        try {
            await rename(staged, runtime);
            // A passing staging-path test is insufficient: validate again after
            // moving to the exact resource path that Tauri will package.
            const report = await verify(runtime);
            if (platform === 'darwin')
                console.log('Checked ' + report.machOFiles + ' Mach-O files for bundle-local dependencies.');
        } catch (error) {
            await rm(runtime, { recursive: true, force: true });
            if (hadRuntime) await rename(previous, runtime);
            throw error;
        }
        await rm(previous, { recursive: true, force: true });
        console.log('Bundled standalone Whisper runtime ready at: ' + runtime);
        return { runtime, reused: false };
    } finally {
        if (work) await rm(work, { recursive: true, force: true });
        await rm(lock, { recursive: true, force: true });
    }
}

if (
    process.argv[1] &&
    (await realpath(process.argv[1]).catch(() => null)) === (await realpath(fileURLToPath(import.meta.url)))
) {
    prepareWhisperRuntime().catch(error => {
        console.error(error.message);
        process.exitCode = 1;
    });
}
