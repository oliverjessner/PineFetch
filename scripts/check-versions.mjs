import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// The optional directory lets tests validate fixtures without touching the checkout.
const root = process.argv[2] ? resolve(process.argv[2]) : fileURLToPath(new URL('../', import.meta.url));
const files = [
    'package.json',
    'package-lock.json',
    'src-tauri/tauri.conf.json',
    'src-tauri/Cargo.toml',
    'src-tauri/Cargo.lock',
];
const [packageRaw, lockRaw, tauriRaw, cargoRaw, cargoLockRaw] = await Promise.all(
    files.map(file => readFile(resolve(root, file), 'utf8'))
);
const { version } = JSON.parse(packageRaw);
assert.match(version, /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/, 'Invalid package version');
const lock = JSON.parse(lockRaw);
const versions = [
    ['package-lock.json', lock.version],
    ['package-lock.json root package', lock.packages?.['']?.version],
    ['tauri.conf.json', JSON.parse(tauriRaw).version],
    ['Cargo.toml', cargoRaw.match(/^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1]],
    ['Cargo.lock', cargoLockRaw.match(/^\[\[package\]\]\s*\nname\s*=\s*"pinefetch"\s*\nversion\s*=\s*"([^"]+)"/m)?.[1]],
];
for (const [file, actual] of versions) {
    assert.equal(actual, version, `${file} version differs from package.json; run npm run sync:version explicitly`);
}
console.log(`[version] All manifests and lockfiles use ${version}`);
