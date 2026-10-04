import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const checkScript = fileURLToPath(new URL('../scripts/check-versions.mjs', import.meta.url));
const syncScript = fileURLToPath(new URL('../scripts/sync-tauri-version.mjs', import.meta.url));

const createFixture = async (t, manifestVersion = '1.2.3') => {
    const root = await mkdtemp(join(tmpdir(), 'pinefetch-versions-'));
    t.after(() => rm(root, { recursive: true, force: true }));
    await mkdir(join(root, 'src-tauri'));
    const files = {
        'package.json': JSON.stringify({ name: 'pinefetch', version: '1.2.3' }),
        'package-lock.json': JSON.stringify({
            version: manifestVersion,
            lockfileVersion: 3,
            packages: {
                '': { name: 'pinefetch', version: manifestVersion },
                'node_modules/example': { version: '8.7.6' },
            },
        }),
        'src-tauri/tauri.conf.json': `{\n    "version": "${manifestVersion}",\n    "bundle": { "icon": ["one.png", "two.png"] }\n}\n`,
        'src-tauri/Cargo.toml': `[package]\nname = "pinefetch"\nversion = "${manifestVersion}"\n`,
        'src-tauri/Cargo.lock': `version = 4\n\n[[package]]\nname = "example"\nversion = "8.7.6"\n\n[[package]]\nname = "pinefetch"\nversion = "${manifestVersion}"\n`,
    };
    await Promise.all(Object.entries(files).map(([file, contents]) => writeFile(join(root, file), contents)));
    return { root, files };
};

for (const manifestVersion of ['1.2.3', '1.2.2']) {
    test(`version check is read-only when versions ${manifestVersion === '1.2.3' ? 'match' : 'differ'}`, async t => {
        const { root, files } = await createFixture(t, manifestVersion);
        const result = spawnSync(process.execPath, [checkScript, root], { encoding: 'utf8', timeout: 10000 });
        assert.ifError(result.error);
        assert.equal(result.status, manifestVersion === '1.2.3' ? 0 : 1, result.stderr);
        if (manifestVersion !== '1.2.3') assert.match(result.stderr, /version differs from package.json/);
        for (const [file, contents] of Object.entries(files)) {
            assert.equal(await readFile(join(root, file), 'utf8'), contents, `${file} must not change`);
        }
    });
}

test('explicit version sync preserves Tauri formatting and dependency versions', async t => {
    const { root, files } = await createFixture(t, '1.2.2');
    const result = spawnSync(
        process.execPath,
        [
            syncScript,
            join(root, 'package.json'),
            join(root, 'src-tauri/tauri.conf.json'),
            join(root, 'src-tauri/Cargo.toml'),
            join(root, 'src-tauri/Cargo.lock'),
            join(root, 'package-lock.json'),
        ],
        { encoding: 'utf8', timeout: 10000 }
    );
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(
        await readFile(join(root, 'src-tauri/tauri.conf.json'), 'utf8'),
        files['src-tauri/tauri.conf.json'].replace('1.2.2', '1.2.3')
    );
    assert.equal(
        await readFile(join(root, 'src-tauri/Cargo.lock'), 'utf8'),
        files['src-tauri/Cargo.lock'].replace('1.2.2', '1.2.3')
    );
    const lock = JSON.parse(await readFile(join(root, 'package-lock.json'), 'utf8'));
    assert.equal(lock.version, '1.2.3');
    assert.equal(lock.packages[''].version, '1.2.3');
    assert.equal(lock.packages['node_modules/example'].version, '8.7.6');
    const checked = spawnSync(process.execPath, [checkScript, root], { encoding: 'utf8', timeout: 10000 });
    assert.ifError(checked.error);
    assert.equal(checked.status, 0, checked.stderr);
});
