import { readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const defaultPackageJsonPath = fileURLToPath(new URL('../package.json', import.meta.url));
const defaultTauriConfigPath = fileURLToPath(new URL('../src-tauri/tauri.conf.json', import.meta.url));
const packageJsonPath = process.argv[2] ? resolve(process.argv[2]) : defaultPackageJsonPath;
const tauriConfigPath = process.argv[3] ? resolve(process.argv[3]) : defaultTauriConfigPath;
const cargoTomlPath = process.argv[4] ? resolve(process.argv[4]) : join(dirname(tauriConfigPath), 'Cargo.toml');
const cargoLockPath = process.argv[5] ? resolve(process.argv[5]) : join(dirname(tauriConfigPath), 'Cargo.lock');
const packageLockPath = process.argv[6]
    ? resolve(process.argv[6])
    : join(dirname(packageJsonPath), 'package-lock.json');

const readJson = async path => JSON.parse(await readFile(path, 'utf8'));

const packageJson = await readJson(packageJsonPath);
const version = `${packageJson.version || ''}`.trim();

if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)) {
    throw new Error(`Invalid package.json version: ${version || '(empty)'}`);
}

const packageLock = await readJson(packageLockPath);
if (!packageLock.packages?.['']) {
    throw new Error('Could not find root package in package-lock.json');
}
if (packageLock.version !== version || packageLock.packages[''].version !== version) {
    packageLock.version = version;
    packageLock.packages[''].version = version;
    await writeFile(packageLockPath, `${JSON.stringify(packageLock, null, 4)}\n`, 'utf8');
    console.log(`[version] Synced npm lockfile to ${version}`);
}

const tauriConfigRaw = await readFile(tauriConfigPath, 'utf8');
const tauriConfig = JSON.parse(tauriConfigRaw);

if (tauriConfig.version === version) {
    console.log(`[version] Tauri already uses ${version}`);
} else {
    if (typeof tauriConfig.version !== 'string') {
        throw new Error('Could not find version in tauri.conf.json');
    }

    const previousVersion = tauriConfig.version || '(missing)';
    // Preserve formatting so explicit version preparation does not break the format gate.
    const updatedConfig = tauriConfigRaw.replace(
        /(^[ \t]*"version"\s*:\s*")([^"]+)(")/m,
        (_match, prefix, _currentVersion, suffix) => `${prefix}${version}${suffix}`
    );
    if (updatedConfig === tauriConfigRaw) {
        throw new Error('Could not replace version in tauri.conf.json');
    }
    await writeFile(tauriConfigPath, updatedConfig, 'utf8');
    console.log(`[version] Synced Tauri ${previousVersion} -> ${version}`);
}

const syncCargoVersion = async (path, pattern, label) => {
    const raw = await readFile(path, 'utf8');
    const match = raw.match(pattern);
    if (!match) {
        throw new Error(`Could not find ${label} package version in ${path}`);
    }
    if (match[2] === version) {
        console.log(`[version] ${label} already uses ${version}`);
        return;
    }
    await writeFile(
        path,
        raw.replace(pattern, (_match, prefix, _currentVersion, suffix) => `${prefix}${version}${suffix}`),
        'utf8'
    );
    console.log(`[version] Synced ${label} ${match[2]} -> ${version}`);
};

await syncCargoVersion(cargoTomlPath, /(^\[package\][\s\S]*?^version\s*=\s*")([^"]+)(")/m, 'Cargo.toml');
await syncCargoVersion(
    cargoLockPath,
    /(^\[\[package\]\]\s*\nname\s*=\s*"pinefetch"\s*\nversion\s*=\s*")([^"]+)(")/m,
    'Cargo.lock'
);
