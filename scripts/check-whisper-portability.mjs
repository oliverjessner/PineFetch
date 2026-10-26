import { execFileSync } from 'node:child_process';
import { open, readFile, readdir, readlink, realpath, stat } from 'node:fs/promises';
import { dirname, isAbsolute, join, relative, resolve, sep, posix } from 'node:path';
import { fileURLToPath } from 'node:url';

const architectures = new Map([
    ['arm64', 0x0100000c],
    ['x64', 0x01000007],
]);
const dylibCommands = new Set([0x0c, 0x80000018, 0x8000001f, 0x20, 0x80000023]);
const dynamicFileTypes = new Set([2, 6, 8]); // MH_EXECUTE, MH_DYLIB, MH_BUNDLE.
const machoMagic = new Set([0xfeedface, 0xcefaedfe, 0xfeedfacf, 0xcffaedfe]);
const fatMagic = new Set([0xcafebabe, 0xbebafeca, 0xcafebabf, 0xbfbafeca]);

const within = (root, path) => {
    const suffix = relative(root, path);
    return suffix === '' || (!isAbsolute(suffix) && suffix !== '..' && !suffix.startsWith(`..${sep}`));
};
const systemPath = path => {
    const normalized = posix.normalize(path);
    return (
        posix.isAbsolute(path) &&
        (normalized === '/usr/lib' ||
            normalized.startsWith('/usr/lib/') ||
            normalized === '/System/Library' ||
            normalized.startsWith('/System/Library/'))
    );
};
const fail = (path, reason) => {
    throw new Error(`Whisper runtime is not portable: ${path}: ${reason}`);
};

// Do not follow directory links while walking. Their physical targets are
// checked separately inside this tree, and realpath catches chains and cycles.
export async function checkWhisperRuntimeTree(directory) {
    const root = await realpath(directory);
    if (!(await stat(root)).isDirectory()) fail(directory, 'runtime root is not a directory');
    const files = [];
    let symlinks = 0;
    const walk = async path => {
        for (const entry of (await readdir(path, { withFileTypes: true })).sort((a, b) =>
            a.name.localeCompare(b.name)
        )) {
            const full = join(path, entry.name);
            const label = relative(root, full);
            if (entry.name.toLowerCase() === 'pyvenv.cfg')
                fail(label, 'venv configuration is not a standalone runtime');
            if (entry.isSymbolicLink()) {
                symlinks++;
                const target = await readlink(full);
                if (isAbsolute(target) || posix.isAbsolute(target)) fail(label, `absolute symlink to ${target}`);
                if (!within(root, resolve(path, target))) fail(label, `symlink escapes the runtime: ${target}`);
                let resolved;
                try {
                    resolved = await realpath(full);
                } catch (error) {
                    fail(label, `broken or cyclic symlink (${error.code})`);
                }
                if (!within(root, resolved)) fail(label, `symlink chain escapes the runtime: ${target}`);
            } else if (entry.isDirectory()) {
                await walk(full);
            } else if (entry.isFile()) {
                files.push(full);
            } else {
                fail(label, 'unsupported filesystem entry');
            }
        }
    };
    await walk(root);
    return { root, files, symlinks };
}

const versionText = value => `${value >>> 16}.${(value >>> 8) & 255}.${value & 255}`;
const versionNumber = value => {
    if (!/^\d{1,5}\.\d{1,3}(?:\.\d{1,3})?$/.test(value)) throw new Error(`Invalid macOS version: ${value}`);
    const [major, minor, patch = 0] = value.split('.').map(Number);
    if (major > 65535 || minor > 255 || patch > 255) throw new Error(`Invalid macOS version: ${value}`);
    return major * 65536 + minor * 256 + patch;
};

// Layouts and constants follow Apple's mach-o/{loader,fat}.h. Parsing the load
// commands directly avoids depending on Xcode, otool, Python or the host linker.
function readMachO(bytes, label) {
    const malformed = reason => fail(label, `malformed Mach-O (${reason})`);
    const range = (offset, size, start = 0, end = bytes.length) => {
        if (
            !Number.isSafeInteger(offset) ||
            !Number.isSafeInteger(size) ||
            size < 0 ||
            offset < start ||
            offset > end - size
        )
            malformed('out-of-bounds offset or size');
    };
    const thin = (start, size, expectedCpu) => {
        range(start, size);
        range(start, 4, start, start + size);
        const magic = bytes.readUInt32BE(start);
        if (!machoMagic.has(magic)) {
            if (bytes.subarray(start, start + 8).toString('ascii') === '!<arch>\n') return null;
            malformed('fat slice has no Mach-O header');
        }
        const little = magic === 0xcefaedfe || magic === 0xcffaedfe;
        const headerSize = magic === 0xfeedfacf || magic === 0xcffaedfe ? 32 : 28;
        range(start, headerSize, start, start + size);
        const u32 = offset => (little ? bytes.readUInt32LE(offset) : bytes.readUInt32BE(offset));
        const cpu = u32(start + 4);
        if (expectedCpu !== undefined && cpu !== expectedCpu) malformed('fat architecture does not match its slice');
        const fileType = u32(start + 12);
        if (!dynamicFileTypes.has(fileType)) return null;
        const count = u32(start + 16);
        const commandBytes = u32(start + 20);
        const end = start + headerSize + commandBytes;
        range(start + headerSize, commandBytes, start, start + size);
        if (count > commandBytes / 8) malformed('invalid load-command count');
        const result = {
            cpu,
            subtype: u32(start + 8),
            fileType,
            is64: headerSize === 32,
            dependencies: [],
            rpaths: [],
            minimum: null,
        };
        let offset = start + headerSize;
        const commandString = (minimum, commandSize) => {
            if (commandSize < minimum) malformed('short string command');
            const stringOffset = u32(offset + 8);
            if (stringOffset < minimum || stringOffset >= commandSize) malformed('invalid command string offset');
            const stringEnd = bytes.indexOf(0, offset + stringOffset);
            if (stringEnd < 0 || stringEnd >= offset + commandSize) malformed('unterminated command string');
            let value;
            try {
                value = new TextDecoder('utf-8', { fatal: true }).decode(
                    bytes.subarray(offset + stringOffset, stringEnd)
                );
            } catch {
                malformed('invalid UTF-8 command string');
            }
            if (
                !value ||
                [...value].some(character => {
                    const code = character.charCodeAt(0);
                    return code < 32 || (code >= 127 && code <= 159);
                })
            )
                malformed('empty or invalid command string');
            return value;
        };
        for (let index = 0; index < count; index++) {
            range(offset, 8, start, end);
            const command = u32(offset);
            const commandSize = u32(offset + 4);
            if (commandSize < 8 || commandSize % (headerSize === 32 ? 8 : 4) !== 0) malformed('invalid command size');
            range(offset, commandSize, start, end);
            if (dylibCommands.has(command)) result.dependencies.push(commandString(24, commandSize));
            // LC_ID_DYLIB identifies this image; it is not a dependency lookup.
            // Wheels can retain a build-time ID while every actual LC_LOAD uses
            // a relocated @loader_path. Validate its encoding, not its address.
            else if (command === 0x0d) result.installName = commandString(24, commandSize);
            else if (command === 0x0e) {
                const linker = commandString(12, commandSize);
                if (!systemPath(linker)) fail(label, `non-system dynamic linker: ${linker}`);
            } else if (command === 0x8000001c) result.rpaths.push(commandString(12, commandSize));
            else if (command === 0x27) fail(label, 'LC_DYLD_ENVIRONMENT can change library resolution');
            else if (command === 0x32) {
                if (commandSize < 24 || u32(offset + 20) > (commandSize - 24) / 8)
                    malformed('short build-version command');
                if (u32(offset + 8) !== 1) fail(label, 'Mach-O targets a platform other than macOS');
                const minimum = u32(offset + 12);
                if (result.minimum !== null && result.minimum !== minimum) malformed('conflicting deployment versions');
                result.minimum = minimum;
            } else if (command === 0x24) {
                if (commandSize !== 16) malformed('invalid minimum-version command');
                const minimum = u32(offset + 8);
                if (result.minimum !== null && result.minimum !== minimum) malformed('conflicting deployment versions');
                result.minimum = minimum;
            } else if ([0x25, 0x2f, 0x30].includes(command)) {
                fail(label, 'Mach-O minimum version targets a platform other than macOS');
            }
            offset += commandSize;
        }
        if (offset !== end) malformed('load-command sizes do not match header');
        return result;
    };
    if (bytes.length < 4) return [];
    const magic = bytes.readUInt32BE(0);
    if (machoMagic.has(magic)) return [thin(0, bytes.length)].filter(Boolean);
    if (!fatMagic.has(magic)) return [];
    range(0, 8);
    const little = magic === 0xbebafeca || magic === 0xbfbafeca;
    const wide = magic === 0xcafebabf || magic === 0xbfbafeca;
    const u32 = offset => (little ? bytes.readUInt32LE(offset) : bytes.readUInt32BE(offset));
    const sizeNumber = offset => {
        const value = little ? bytes.readBigUInt64LE(offset) : bytes.readBigUInt64BE(offset);
        if (value > BigInt(Number.MAX_SAFE_INTEGER)) malformed('fat slice exceeds safe address range');
        return Number(value);
    };
    const count = u32(4);
    if (count === 0 || count > 64) malformed('invalid fat-architecture count');
    const entrySize = wide ? 32 : 20;
    const tableEnd = 8 + count * entrySize;
    range(8, count * entrySize);
    const occupied = [];
    const slices = [];
    for (let index = 0; index < count; index++) {
        const offset = 8 + index * entrySize;
        const sliceOffset = wide ? sizeNumber(offset + 8) : u32(offset + 8);
        const sliceSize = wide ? sizeNumber(offset + 16) : u32(offset + 12);
        range(sliceOffset, sliceSize, tableEnd);
        if (sliceSize === 0 || occupied.some(([start, end]) => sliceOffset < end && sliceOffset + sliceSize > start))
            malformed('empty or overlapping fat slice');
        occupied.push([sliceOffset, sliceOffset + sliceSize]);
        const slice = thin(sliceOffset, sliceSize, u32(offset));
        if (slice) slices.push(slice);
    }
    return slices;
}

const prefix = (path, token) => path === token || path.startsWith(`${token}/`);

async function readRuntimeImages({ root, files }) {
    const images = new Map();
    for (const file of files) {
        const handle = await open(file, 'r');
        let magic;
        try {
            const header = Buffer.alloc(4);
            const { bytesRead } = await handle.read(header, 0, 4, 0);
            if (bytesRead === 4) magic = header.readUInt32BE(0);
        } finally {
            await handle.close();
        }
        if (!machoMagic.has(magic) && !fatMagic.has(magic)) continue;
        const slices = readMachO(await readFile(file), relative(root, file));
        if (slices.length > 0) images.set(file, slices);
    }
    return images;
}

// Build-time normalization only: never repair LC_LOAD dependencies or infer
// missing libraries from the developer machine. The static checker must run
// afterwards, and a real relocated runtime smoke remains a separate check.
export async function normalizeWhisperMachO(directory, { runImpl = execFileSync } = {}) {
    const tree = await checkWhisperRuntimeTree(directory);
    const images = await readRuntimeImages(tree);
    const report = { filesPatched: 0, removedRpaths: 0, normalizedIds: 0 };
    const run = async (tool, args, file) => {
        try {
            await runImpl(tool, args, { encoding: 'utf8', stdio: 'pipe', timeout: 30000 });
        } catch (error) {
            throw new Error(`Whisper Mach-O normalization failed: ${relative(tree.root, file)}: ${error.message}`, {
                cause: error,
            });
        }
    };
    for (const [file, slices] of images) {
        const obsoleteRpaths = new Set(
            slices.flatMap(slice => slice.rpaths).filter(path => posix.isAbsolute(path) && !systemPath(path))
        );
        const normalizeId = slices.some(
            slice => slice.installName && posix.isAbsolute(slice.installName) && !systemPath(slice.installName)
        );
        if (obsoleteRpaths.size === 0 && !normalizeId) continue;
        for (const path of obsoleteRpaths) {
            await run('/usr/bin/install_name_tool', ['-delete_rpath', path, file], file);
            report.removedRpaths++;
        }
        if (normalizeId) {
            await run('/usr/bin/install_name_tool', ['-id', `@rpath/${posix.basename(file)}`, file], file);
            report.normalizedIds++;
        }
        await run('/usr/bin/codesign', ['--force', '--sign', '-', file], file);
        report.filesPatched++;
    }
    return report;
}

export async function checkWhisperPortability(
    directory,
    { executable = 'bin/python3.12', arch = process.arch, maxMacOSVersion } = {}
) {
    if (!architectures.has(arch)) throw new Error(`Unsupported Whisper runtime architecture: ${arch}`);
    const maximum = maxMacOSVersion === undefined ? null : versionNumber(maxMacOSVersion);
    const tree = await checkWhisperRuntimeTree(directory);
    const { root, symlinks } = tree;
    const entry = await realpath(resolve(root, executable));
    if (!within(root, entry)) fail(executable, 'Python executable escapes the runtime');
    const images = new Map();
    const observedArchitectures = new Set();
    let minimum = 0;
    for (const [file, slices] of await readRuntimeImages(tree)) {
        const label = relative(root, file);
        for (const slice of slices) {
            for (const [name, cpu] of architectures) if (cpu === slice.cpu) observedArchitectures.add(name);
            for (const path of [...slice.dependencies, ...slice.rpaths]) {
                if (posix.isAbsolute(path) && !systemPath(path))
                    fail(label, `absolute non-system library path: ${path}`);
            }
        }
        const matching = slices.filter(slice => slice.cpu === architectures.get(arch));
        if (matching.length !== 1 || !matching[0].is64) fail(label, `expected exactly one ${arch} Mach-O slice`);
        const image = matching[0];
        if (arch === 'arm64' && (image.subtype & 0x00ffffff) === 2)
            fail(label, 'arm64e-only image is not a generic arm64 runtime');
        if (image.minimum === null || image.minimum === 0) fail(label, 'missing macOS deployment version');
        if (maximum !== null && image.minimum > maximum)
            fail(label, `requires macOS ${versionText(image.minimum)}, exceeds ${maxMacOSVersion}`);
        minimum = Math.max(minimum, image.minimum);
        images.set(file, image);
    }
    if (images.get(entry)?.fileType !== 2) fail(executable, 'Python entry point is not a bundled Mach-O executable');

    const expand = (path, loader, entryPoint) => {
        let expanded;
        if (prefix(path, '@loader_path')) expanded = resolve(dirname(loader), `.${path.slice('@loader_path'.length)}`);
        else if (prefix(path, '@executable_path'))
            expanded = resolve(dirname(entryPoint), `.${path.slice('@executable_path'.length)}`);
        if (expanded !== undefined) {
            if (!within(root, expanded))
                fail(relative(root, loader), `relative library path escapes the runtime: ${path}`);
            return expanded;
        }
        if (systemPath(path)) return posix.normalize(path);
        fail(relative(root, loader), `unanchored library search path: ${path}`);
    };
    const runpaths = (file, entryPoint) =>
        images.get(file).rpaths.map(path => {
            const expanded = expand(path, file, entryPoint);
            if (!systemPath(expanded) && !within(root, expanded))
                fail(relative(root, file), `runpath escapes the runtime: ${path}`);
            return expanded;
        });
    const resolveDependency = async (dependency, file, entryPoint, paths) => {
        if (systemPath(dependency)) return null; // May live only in the macOS dyld shared cache.
        const candidates = prefix(dependency, '@rpath')
            ? paths.map(path => ({
                  path: resolve(path, `.${dependency.slice('@rpath'.length)}`),
                  system: systemPath(path),
              }))
            : [{ path: expand(dependency, file, entryPoint), system: false }];
        for (const { path: candidate, system } of candidates) {
            if (system) {
                if (!systemPath(candidate))
                    fail(relative(root, file), `dependency escapes its system runpath: ${dependency}`);
                return null;
            }
            if (!within(root, candidate)) fail(relative(root, file), `dependency escapes the runtime: ${dependency}`);
            let actual;
            try {
                actual = await realpath(candidate);
            } catch (error) {
                if (error.code === 'ENOENT' || error.code === 'ENOTDIR') continue;
                throw error;
            }
            if (!within(root, actual))
                fail(relative(root, file), `dependency symlink escapes the runtime: ${dependency}`);
            if (images.get(actual)?.fileType !== 6)
                fail(relative(root, file), `dependency is not a compatible bundled dylib: ${dependency}`);
            return actual;
        }
        fail(relative(root, file), `unresolved dependency: ${dependency}`);
    };
    const visited = new Set();
    const reached = new Set();
    const visit = async (file, entryPoint, inherited, chain = new Set()) => {
        if (chain.has(file)) return;
        const paths = [...new Set([...runpaths(file, entryPoint), ...inherited])];
        const key = JSON.stringify([file, entryPoint, paths]);
        if (visited.has(key)) return;
        visited.add(key);
        reached.add(file);
        const nextChain = new Set([...chain, file]);
        for (const dependency of images.get(file).dependencies) {
            const target = await resolveDependency(dependency, file, entryPoint, paths);
            if (target) await visit(target, entryPoint, paths, nextChain);
        }
    };
    await visit(entry, entry, []);
    // Python dlopens extension bundles that are not ordinary executable edges.
    // Audit them too, with the interpreter's inherited runpaths.
    const inherited = runpaths(entry, entry);
    for (const [file, image] of images) {
        if (image.fileType === 2) await visit(file, file, []);
        else if (image.fileType === 8) await visit(file, entry, inherited);
    }
    // A dependent dylib inherits its caller's runpath stack. Re-rooting it
    // without that caller would reject valid transitive @rpath dependencies.
    // Still inspect orphan dylibs: they may be loaded later through ctypes.
    for (const file of images.keys()) if (!reached.has(file)) await visit(file, entry, inherited);
    return {
        machOFiles: images.size,
        symlinks,
        architectures: [...observedArchitectures].sort(),
        minimumMacOS: versionText(minimum),
    };
}

const invokedPath = process.argv[1] ? await realpath(process.argv[1]).catch(() => null) : null;
if (invokedPath === (await realpath(fileURLToPath(import.meta.url)))) {
    try {
        const [directory, ...args] = process.argv.slice(2);
        if (!directory || args.length % 2 !== 0)
            throw new Error('Usage: check-whisper-portability.mjs RUNTIME [--arch arm64|x64] [--max-macos X.Y]');
        const options = {};
        for (let index = 0; index < args.length; index += 2) {
            if (args[index] === '--arch') options.arch = args[index + 1];
            else if (args[index] === '--max-macos') options.maxMacOSVersion = args[index + 1];
            else throw new Error(`Unknown option: ${args[index]}`);
        }
        const report = await checkWhisperPortability(directory, options);
        console.log(
            `[Whisper portability] ${report.machOFiles} Mach-O files, ${report.symlinks} internal symlinks; architectures ${report.architectures.join(', ')}; macOS >= ${report.minimumMacOS}`
        );
    } catch (error) {
        console.error(error.message);
        process.exitCode = 1;
    }
}
