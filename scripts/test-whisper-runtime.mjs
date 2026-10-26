import { spawnSync } from 'node:child_process';
import { access, cp, mkdir, mkdtemp, readFile, realpath, rm, stat, writeFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import { homedir } from 'node:os';
import { isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkWhisperPortability } from './check-whisper-portability.mjs';
import { whisperEnvironment } from './prepare-whisper-runtime.mjs';

const repository = await realpath(fileURLToPath(new URL('../', import.meta.url)));
const help = `Usage: npm run test:whisper-runtime -- [--runtime PATH] [--model CT2_DIRECTORY] [--keep]

macOS real-runtime acceptance: relocate the bundled Python, decode PCM and
transcribe generated speech with CPU/int8. sandbox-exec denies networking and
reads from the real home/repository, Homebrew and external Python frameworks.
This is a restricted-host simulation, not a clean operating-system installation.

--runtime PATH  Runtime directory (default: src-tauri/resources/whisper-runtime)
--model PATH    Existing local CTranslate2 tiny model; copied before sandboxing.
                Without this option, download tiny into an isolated temporary cache.
--keep          Keep runtime, model, child logs and summary.json after success.
                Failed runs always retain their evidence directory.
`;

function parseArguments(args) {
    const options = { runtime: join(repository, 'src-tauri/resources/whisper-runtime'), model: null, keep: false };
    const used = new Set();
    for (let index = 0; index < args.length; index++) {
        const arg = args[index];
        if (arg === '--help' || arg === '-h') return { help: true };
        if (used.has(arg)) throw new Error(`Repeated option: ${arg}`);
        used.add(arg);
        if (arg === '--keep') options.keep = true;
        else if (arg === '--runtime' || arg === '--model') {
            const value = args[++index];
            if (!value || value.startsWith('--')) throw new Error(`Missing value for ${arg}`);
            options[arg.slice(2)] = resolve(value);
        } else throw new Error(`Unknown option: ${arg}`);
    }
    return options;
}

const within = (root, path) => {
    const suffix = relative(root, path);
    return suffix === '' || (!isAbsolute(suffix) && suffix !== '..' && !suffix.startsWith(`..${sep}`));
};

async function main() {
    const options = parseArguments(process.argv.slice(2));
    if (options.help) {
        process.stdout.write(help);
        return;
    }
    if (process.platform !== 'darwin')
        throw new Error('This acceptance check requires macOS and sandbox-exec; it cannot be skipped as passed.');
    for (const tool of ['/usr/bin/sandbox-exec', '/usr/bin/say']) {
        await access(tool, constants.X_OK).catch(() => {
            throw new Error(`Required macOS acceptance tool is unavailable: ${tool}`);
        });
    }
    const manifest = JSON.parse(await readFile(join(repository, 'scripts/whisper-python.json'), 'utf8'));
    const target = manifest.targets[`darwin-${process.arch}`];
    if (!target) throw new Error(`No pinned Whisper runtime is declared for darwin-${process.arch}`);
    const versions = await Promise.all(
        ['whisper-runtime.version', 'whisper-av.version'].map(async name =>
            (await readFile(join(repository, 'scripts', name), 'utf8')).trim()
        )
    );
    const source = await realpath(options.runtime);
    if (!(await stat(source)).isDirectory()) throw new Error('--runtime must point to a bundled runtime directory');
    const work = await realpath(await mkdtemp('/private/tmp/pinefetch-whisper-acceptance-'));
    let passed = false;
    const summary = {
        status: 'running',
        method: 'real relocated runtime under restricted-host simulation; not a clean OS',
        platform: process.platform,
        arch: process.arch,
        source_runtime: source,
        evidence_directory: work,
    };
    try {
        const moved = join(work, 'Moved Whisper Runtime');
        const home = join(work, 'Isolated Home');
        const realHome = await realpath(homedir());
        const denied = [...new Set(['/opt/homebrew', '/usr/local', '/Library/Frameworks', repository, realHome])];
        if (denied.some(prefix => within(prefix, work)))
            throw new Error('Temporary evidence directory falls inside a denied host path');
        await mkdir(home);
        const environment = whisperEnvironment(home);
        environment.HF_HOME = join(home, 'huggingface');
        const run = async (phase, file, args, env = environment, timeout = 120000) => {
            const result = spawnSync(file, args, {
                cwd: work,
                env,
                encoding: 'utf8',
                timeout,
                maxBuffer: 16 * 1024 * 1024,
                stdio: 'pipe',
            });
            await writeFile(join(work, `${phase}.stdout.log`), result.stdout || '');
            await writeFile(join(work, `${phase}.stderr.log`), result.stderr || result.error?.message || '');
            if (result.error || result.signal || result.status !== 0)
                throw new Error(
                    `${phase} failed (${result.signal || result.status || 'start error'}): ${String(result.stderr || result.error?.message || 'No diagnostic output.').trim()}`,
                    { cause: result.error }
                );
            return result.stdout || '';
        };
        await cp(source, moved, { recursive: true, verbatimSymlinks: true, force: false, errorOnExist: true });
        summary.relocated_runtime = moved;
        summary.portability = await checkWhisperPortability(moved, {
            executable: target.executable,
            arch: process.arch,
            maxMacOSVersion: target.minimumMacOS || '14.0',
        });
        const python = join(moved, target.executable);
        const checker = join(work, 'check-whisper-runtime.py');
        const inference = join(work, 'check-whisper-inference.py');
        await cp(join(repository, 'scripts/check-whisper-runtime.py'), checker);
        await cp(join(repository, 'scripts/check-whisper-inference.py'), inference);
        let model = options.model;
        if (!model) {
            const output = await run(
                'model-download',
                python,
                ['-I', '-B', inference, 'download', join(work, 'model-cache')],
                environment,
                300000
            );
            model = JSON.parse(output.trim().split(/\r?\n/).at(-1)).model_path;
        }
        model = await realpath(model);
        for (const name of ['model.bin', 'config.json', 'tokenizer.json']) {
            if (!(await stat(join(model, name))).isFile())
                throw new Error(`Local CTranslate2 model is missing ${name}`);
        }
        const localModel = join(work, 'Offline Tiny Model');
        await cp(model, localModel, { recursive: true, dereference: true, force: false, errorOnExist: true });
        summary.model = {
            source: options.model ? 'provided local directory' : 'tiny downloaded before sandboxing',
            copied_to: localModel,
        };
        const voices = await run('speech-voices', '/usr/bin/say', ['-v', '?'], environment, 30000);
        if (!/^Samantha\s/m.test(voices))
            throw new Error(
                'Samantha speech voice is unavailable; install that voice before running this acceptance check.'
            );
        const audio = join(work, 'local-media-speech.aiff');
        const speech = 'PineFetch stores video and audio locally. The media stays local on this computer.';
        await run('speech-generation', '/usr/bin/say', ['-v', 'Samantha', '-o', audio, speech], environment, 30000);
        summary.speech = { voice: 'Samantha', text: speech, audio };
        const probes = [];
        for (const prefix of denied) {
            try {
                if ((await stat(prefix)).isDirectory()) probes.push(prefix);
            } catch (error) {
                if (error.code !== 'ENOENT') throw error;
            }
        }
        const profile = join(work, 'offline.sb');
        await writeFile(
            profile,
            `(version 1)\n(allow default)\n(deny network*)\n(deny file-read*\n${denied.map(path => `    (subpath ${JSON.stringify(path)})`).join('\n')}\n)\n`
        );
        const probeFile = join(work, 'sandbox-probes.json');
        await writeFile(probeFile, JSON.stringify(probes));
        const offline = { ...environment, HF_HUB_OFFLINE: '1', TRANSFORMERS_OFFLINE: '1' };
        summary.sandbox = { required: true, network: 'denied', denied_read_prefixes: denied };
        summary.decode = (
            await run(
                'offline-decode',
                '/usr/bin/sandbox-exec',
                ['-f', profile, python, '-I', '-B', checker, ...versions, manifest.version],
                offline
            )
        ).trim();
        const output = await run(
            'offline-inference',
            '/usr/bin/sandbox-exec',
            [
                '-f',
                profile,
                python,
                '-I',
                '-B',
                inference,
                'infer',
                '--model',
                localModel,
                '--audio',
                audio,
                '--sandbox-probes',
                probeFile,
            ],
            offline
        );
        summary.inference = JSON.parse(output.trim().split(/\r?\n/).at(-1));
        summary.status = 'passed';
        passed = true;
    } catch (error) {
        summary.status = 'failed';
        summary.error = error.message;
        throw error;
    } finally {
        await writeFile(join(work, 'summary.json'), `${JSON.stringify(summary, null, 4)}\n`);
        if (passed && !options.keep) {
            await rm(work, { recursive: true, force: true });
            summary.evidence_directory = null;
            summary.temporary_files_removed = true;
        }
        process.stdout.write(`${JSON.stringify(summary, null, 4)}\n`);
        if (summary.evidence_directory)
            process.stderr.write(`Whisper acceptance evidence: ${join(work, 'summary.json')}\n`);
    }
}

main().catch(error => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
});
