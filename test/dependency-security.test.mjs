import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';

test('locked minimatch packages exclude GHSA-g3hv-75wg-m249', async () => {
    const lock = JSON.parse(await readFile(new URL('../package-lock.json', import.meta.url), 'utf8'));

    // Check nested copies and npm aliases as well as the hoisted dependency.
    for (const [path, dependency] of Object.entries(lock.packages)) {
        if (!/(?:^|\/)node_modules\/minimatch$/.test(path) && dependency.name !== 'minimatch') continue;

        const version = /^(\d+)\.(\d+)\.(\d+)(?:-([^+]+))?(?:\+.+)?$/.exec(dependency.version);
        assert.ok(version, `${path}: unrecognized minimatch version ${dependency.version}`);
        const [major, minor, patch] = version.slice(1, 4).map(Number);
        // https://github.com/advisories/GHSA-g3hv-75wg-m249: >=7.0.0, <11.0.1.
        const vulnerable =
            major >= 7 &&
            (major < 11 || (major === 11 && minor === 0 && (patch === 0 || (patch === 1 && Boolean(version[4])))));

        assert.ok(
            !vulnerable,
            `${path}@${dependency.version} is affected by GHSA-g3hv-75wg-m249; update to minimatch >=11.0.1`
        );
    }
});
