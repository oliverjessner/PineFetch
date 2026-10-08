import { copyFile, cp, readFile, rm, stat } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const destination = fileURLToPath(new URL('../src/vendor/oj/', import.meta.url));
const notices = ['LICENSE', 'README.md', 'THIRD-PARTY-NOTICES.md'];

export const syncDesignSystem = async () => {
    const packageRoot = dirname(require.resolve('oj-designsystem/package.json'));
    const distribution = join(packageRoot, 'dist');
    const { version } = JSON.parse(await readFile(join(packageRoot, 'package.json'), 'utf8'));

    // Keep the complete public distribution: CSS refers to local fonts/icons,
    // and the ESM entry and original licenses must travel with the application.
    await Promise.all(
        ['styles.css', 'index.js', 'assets', 'licenses', ...notices.map(file => `../${file}`)].map(file =>
            stat(join(distribution, file))
        )
    );
    await rm(destination, { recursive: true, force: true });
    await cp(distribution, destination, { recursive: true });
    await Promise.all(notices.map(file => copyFile(join(packageRoot, file), join(destination, file))));
    return version;
};

if (import.meta.main) {
    const version = await syncDesignSystem();
    console.log(`[design-system] Prepared local oj-designsystem ${version} assets.`);
}
