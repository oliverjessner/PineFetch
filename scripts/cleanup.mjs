import { execFileSync } from 'node:child_process';
import {
    lstatSync,
    mkdirSync,
    readFileSync,
    readdirSync,
    realpathSync,
    rmSync,
    rmdirSync,
    unlinkSync,
    writeFileSync,
} from 'node:fs';
import { isAbsolute, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const publishLock = '.pinefetch-publish.lock';

// Only generated output belongs here. In particular, never remove an entire
// artifacts, resources, vendor, or docs directory.
export const cleanupPaths = Object.freeze([
    'src-tauri/target',
    'src-tauri/gen/schemas',
    'src-tauri/vendor/glib/target',
    'src-tauri/vendor/glib/Cargo.lock',
    'coverage',
    'src-tauri/resources/whisper-runtime.previous',
    'src-tauri/resources/social-runtime.previous',
]);

export const runtimePaths = Object.freeze([
    'src-tauri/resources/whisper-runtime',
    'src-tauri/resources/ffmpeg-runtime',
    'src-tauri/resources/deno-runtime',
    'src-tauri/resources/ocr-runtime',
    'src-tauri/resources/social-runtime',
]);

export function parseCleanupArgs(args) {
    const options = { dryRun: false, help: false, afterPublish: false };
    for (const arg of args) {
        if (arg === '--dry-run') options.dryRun = true;
        else if (arg === '--help' || arg === '-h') options.help = true;
        else if (arg === '--after-publish') options.afterPublish = true;
        else throw new Error(`Unknown cleanup option: ${arg}`);
    }
    if (options.dryRun && options.afterPublish) throw new Error('--after-publish cannot be combined with --dry-run.');
    return options;
}

function projectRoot(directory) {
    const absolute = realpathSync(directory);
    const metadata = JSON.parse(readFileSync(join(absolute, 'package.json'), 'utf8'));
    const cargo = readFileSync(join(absolute, 'src-tauri/Cargo.toml'), 'utf8');
    const packageSection = cargo.split(/(?=^\[)/m).find(section => section.startsWith('[package]'));
    if (metadata.name !== 'pinefetch' || !/^name\s*=\s*"pinefetch"\s*$/m.test(packageSection ?? '')) {
        throw new Error('Cleanup requires the PineFetch project root.');
    }
    const checkout = execFileSync('git', ['rev-parse', '--show-toplevel'], {
        cwd: absolute,
        encoding: 'utf8',
        timeout: 10000,
    }).trim();
    if (realpathSync(checkout) !== absolute) throw new Error('Cleanup requires the root of the PineFetch checkout.');
    return absolute;
}

function safeTarget(directory, relative) {
    const parts = relative.split('/');
    const destination = resolve(directory, ...parts);
    if (
        isAbsolute(relative) ||
        parts.some(part => !part || part === '.' || part === '..') ||
        !destination.startsWith(directory + sep)
    ) {
        throw new Error(`Cleanup path must stay inside the checkout: ${relative}`);
    }
    let current = directory;
    for (const part of parts) {
        current = join(current, part);
        let stat;
        try {
            stat = lstatSync(current);
        } catch (error) {
            if (error.code === 'ENOENT') return null;
            throw error;
        }
        if (stat.isSymbolicLink()) throw new Error(`Refusing cleanup through a symlink: ${current}`);
        if (current !== destination && !stat.isDirectory())
            throw new Error(`Cleanup parent is not a directory: ${current}`);
    }
    return destination;
}

function sizeOf(filename) {
    const stat = lstatSync(filename);
    if (!stat.isDirectory()) return stat.size;
    return readdirSync(filename).reduce((bytes, name) => bytes + sizeOf(join(filename, name)), 0);
}

function assertPreparationIdle(directory) {
    if (safeTarget(directory, 'src-tauri/resources/.whisper-runtime.lock')) {
        throw new Error(
            'Whisper runtime preparation is locked. Finish preparation before cleanup; inspect stale locks manually.'
        );
    }
}

function assertPublishIdle(directory) {
    if (safeTarget(directory, publishLock)) {
        throw new Error(
            'Another publish or cleanup holds .pinefetch-publish.lock. Finish it first; inspect stale locks manually.'
        );
    }
}

function assertPublishOwner(directory, pid) {
    const lock = safeTarget(directory, publishLock);
    const owner = `${pid}.pid`;
    if (!lock || !lstatSync(lock).isDirectory()) throw new Error('The publish lock is missing.');
    const entries = readdirSync(lock);
    if (entries.length !== 1 || entries[0] !== owner || !safeTarget(directory, `${publishLock}/${owner}`)) {
        throw new Error('Cleanup must be invoked by the owner of the publish lock.');
    }
    if (!lstatSync(join(lock, owner)).isFile()) throw new Error('The publish lock owner is invalid.');
}

function acquireCleanupLock(directory) {
    assertPublishIdle(directory);
    const lock = join(directory, publishLock);
    try {
        mkdirSync(lock);
    } catch (error) {
        if (error.code === 'EEXIST')
            throw new Error('Another publish or cleanup has just acquired .pinefetch-publish.lock.');
        throw error;
    }
    const owner = join(lock, `${process.pid}.pid`);
    try {
        writeFileSync(owner, '', { flag: 'wx' });
    } catch (error) {
        rmdirSync(lock);
        throw error;
    }
    return () => {
        assertPublishOwner(directory, process.pid);
        unlinkSync(owner);
        rmdirSync(lock);
    };
}

export function planCleanup(directory = root) {
    const absolute = projectRoot(directory);
    assertPreparationIdle(absolute);
    const candidates = [...cleanupPaths];
    for (const relative of runtimePaths) {
        const runtime = safeTarget(absolute, relative);
        if (!runtime) continue;
        if (!lstatSync(runtime).isDirectory()) throw new Error(`Runtime path is not a directory: ${relative}`);
        for (const name of readdirSync(runtime).sort()) {
            // These source-owned markers keep Tauri resource paths present.
            if (name !== '.keep' && name !== 'PLACEHOLDER') candidates.push(`${relative}/${name}`);
        }
    }
    const tracked = execFileSync('git', ['--literal-pathspecs', 'ls-files', '-z', '--', ...candidates], {
        cwd: absolute,
        encoding: 'utf8',
        timeout: 10000,
        maxBuffer: 8 * 1024 ** 2,
    })
        .split('\0')
        .filter(Boolean);
    if (tracked.length) throw new Error(`Refusing to remove tracked files: ${tracked.join(', ')}`);
    // Validate every target and ancestor before deleting the first file.
    const entries = candidates.flatMap(relative => {
        const filename = safeTarget(absolute, relative);
        return filename ? [{ path: relative, bytes: sizeOf(filename) }] : [];
    });
    return { root: absolute, entries, bytes: entries.reduce((bytes, entry) => bytes + entry.bytes, 0) };
}

export function cleanupBuildArtifacts({ directory = root, dryRun = false } = {}) {
    const plan = planCleanup(directory);
    if (!dryRun) {
        for (const entry of plan.entries) {
            const filename = safeTarget(plan.root, entry.path);
            if (filename) rmSync(filename, { recursive: true, force: true });
        }
    }
    return { ...plan, dryRun };
}

export function runCleanup({ directory = root, dryRun = false, afterPublish = false } = {}) {
    const absolute = projectRoot(directory);
    if (afterPublish) {
        if (dryRun) throw new Error('--after-publish cannot be combined with --dry-run.');
        // publish.sh invokes Node directly while retaining its own lock. The
        // internal mode cannot bypass a lock belonging to another process.
        assertPublishOwner(absolute, process.ppid);
        return cleanupBuildArtifacts({ directory: absolute });
    }
    if (dryRun) {
        assertPublishIdle(absolute);
        return cleanupBuildArtifacts({ directory: absolute, dryRun: true });
    }
    const unlock = acquireCleanupLock(absolute);
    try {
        return cleanupBuildArtifacts({ directory: absolute });
    } finally {
        unlock();
    }
}

export function formatCleanupReport(result) {
    return [
        `${result.dryRun ? 'Would remove' : 'Removed'} ${result.entries.length} generated paths (${(result.bytes / 1024 ** 2).toFixed(1)} MiB of file contents).`,
        ...result.entries.map(entry => `  ${entry.path}`),
        'Local app/DMG bundles and generated runtime contents are included. Source files, runtime placeholders, installed dependencies and application data are preserved.',
    ].join('\n');
}

export function main(args = process.argv.slice(2)) {
    const options = parseCleanupArgs(args);
    if (options.help) {
        console.log(
            'Usage: npm run cleanup -- [--dry-run]\n\nRemove allowlisted local build output, including all target app/DMG bundles and generated runtime contents. Manual cleanup also removes unpublished local bundles.\n--dry-run previews paths and file sizes without changing files.\nSuccessful publishes run cleanup automatically after the GitHub release and Homebrew tap push. Failed publishes keep build output. See docs/CLEANUP.md.'
        );
        return;
    }
    console.log(formatCleanupReport(runCleanup(options)));
}

if (import.meta.main) {
    try {
        main();
    } catch (error) {
        console.error(error.message);
        process.exitCode = 1;
    }
}
