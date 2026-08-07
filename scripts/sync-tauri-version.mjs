import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const defaultPackageJsonPath = fileURLToPath(new URL('../package.json', import.meta.url));
const defaultTauriConfigPath = fileURLToPath(new URL('../src-tauri/tauri.conf.json', import.meta.url));
const packageJsonPath = process.argv[2] ? resolve(process.argv[2]) : defaultPackageJsonPath;
const tauriConfigPath = process.argv[3] ? resolve(process.argv[3]) : defaultTauriConfigPath;

const readJson = async path => JSON.parse(await readFile(path, 'utf8'));

const packageJson = await readJson(packageJsonPath);
const version = `${packageJson.version || ''}`.trim();

if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)) {
    throw new Error(`Invalid package.json version: ${version || '(empty)'}`);
}

const tauriConfigRaw = await readFile(tauriConfigPath, 'utf8');
const tauriConfig = JSON.parse(tauriConfigRaw);

if (tauriConfig.package?.version === version) {
    console.log(`[version] Tauri already uses ${version}`);
} else {
    const versionPattern = /("package"\s*:\s*\{[\s\S]*?"version"\s*:\s*")([^"]*)(")/;
    const match = tauriConfigRaw.match(versionPattern);
    if (!match) {
        throw new Error('Could not find package.version in tauri.conf.json');
    }

    const previousVersion = match[2] || '(missing)';
    const updatedConfig = tauriConfigRaw.replace(
        versionPattern,
        (_match, prefix, _currentVersion, suffix) => `${prefix}${version}${suffix}`
    );
    await writeFile(tauriConfigPath, updatedConfig, 'utf8');
    console.log(`[version] Synced Tauri ${previousVersion} -> ${version}`);
}
