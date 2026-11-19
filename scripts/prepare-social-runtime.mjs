import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { cp, mkdir, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const runtime = join(root, 'src-tauri/resources/social-runtime');
const nodeVersion = '26.10.0';
const platform = process.platform;
const arch = process.arch;
if (!['darwin', 'linux', 'win32'].includes(platform) || !['arm64', 'x64'].includes(arch))
    throw new Error(`Unsupported Social Insights runtime target: ${platform}/${arch}`);
const files = [
    'package.json',
    'package-lock.json',
    'native/social-insights.cjs',
    'native/social-browser.cjs',
    'scripts/prepare-social-runtime.mjs',
];
const inputs = await Promise.all(files.map(file => readFile(join(root, file))));
const key = createHash('sha256')
    .update(`${nodeVersion}/${platform}/${arch}`)
    .update(Buffer.concat(inputs))
    .digest('hex');
const nodeName = platform === 'win32' ? 'node.exe' : 'node';
const verify = directory =>
    execFileSync(
        join(directory, nodeName),
        [
            '-e',
            "require('./social-browser.cjs').verifyBrowserRuntime().then(version => console.log('[social-runtime] ' + version)).catch(() => process.exit(1))",
        ],
        { cwd: directory, encoding: 'utf8', timeout: 30000 }
    );
try {
    if (
        (await readFile(join(runtime, '.inputs'), 'utf8')) === key &&
        (await readFile(join(runtime, 'social-insights.cjs'))).equals(inputs[2]) &&
        (await readFile(join(runtime, 'social-browser.cjs'))).equals(inputs[3]) &&
        execFileSync(join(runtime, nodeName), ['--version'], { encoding: 'utf8' }).trim() === `v${nodeVersion}`
    ) {
        execFileSync(join(runtime, nodeName), ['-e', "require('fetchary'); require('./social-insights.cjs')"], {
            cwd: runtime,
            stdio: 'ignore',
        });
        process.stdout.write(verify(runtime));
        console.log('[social-runtime] Reusing bundled Node, Fetchary and Chromium.');
        process.exit(0);
    }
} catch {
    /* Build the missing or invalid runtime in isolation. */
}

await mkdir(runtime, { recursive: true });
const stage = await mkdtemp(join(root, 'src-tauri/resources/.social-runtime-'));
try {
    const target = platform === 'win32' ? 'win' : platform;
    const archive = `node-v${nodeVersion}-${target}-${arch}.${platform === 'win32' ? 'zip' : 'tar.gz'}`;
    const base = `https://nodejs.org/dist/v${nodeVersion}/`;
    const download = async name => {
        const response = await fetch(base + name, { signal: AbortSignal.timeout(120000) });
        if (!response.ok) throw new Error(`Node runtime download failed: HTTP ${response.status}`);
        return Buffer.from(await response.arrayBuffer());
    };
    const [checksums, bytes] = await Promise.all([download('SHASUMS256.txt'), download(archive)]);
    const expected = checksums
        .toString()
        .split('\n')
        .find(line => line.trim().endsWith(`  ${archive}`))
        ?.split(/\s+/)[0];
    if (!expected || createHash('sha256').update(bytes).digest('hex') !== expected)
        throw new Error('Node runtime checksum verification failed.');
    const archivePath = join(stage, archive);
    await writeFile(archivePath, bytes);
    execFileSync('tar', ['-xf', archivePath, '-C', stage]);
    const extracted = join(stage, `node-v${nodeVersion}-${target}-${arch}`);
    await cp(join(extracted, platform === 'win32' ? nodeName : `bin/${nodeName}`), join(stage, nodeName));
    await cp(join(extracted, 'LICENSE'), join(stage, 'NODE-LICENSE'));
    await rm(extracted, { recursive: true, maxRetries: 5, retryDelay: 100 });
    await rm(archivePath);
    await writeFile(join(stage, 'package.json'), inputs[0]);
    await writeFile(join(stage, 'package-lock.json'), inputs[1]);
    await writeFile(join(stage, 'social-insights.cjs'), inputs[2]);
    await writeFile(join(stage, 'social-browser.cjs'), inputs[3]);
    execFileSync(
        platform === 'win32' ? 'npm.cmd' : 'npm',
        ['ci', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund'],
        {
            cwd: stage,
            stdio: 'inherit',
            env: { ...process.env, PUPPETEER_SKIP_DOWNLOAD: 'true' },
        }
    );
    execFileSync(join(stage, nodeName), ['-e', "require('fetchary'); console.log('Fetchary library ready')"], {
        cwd: stage,
        stdio: 'inherit',
    });
    const requireRuntime = createRequire(join(stage, 'package.json'));
    const { install, Browser, detectBrowserPlatform } = requireRuntime('@puppeteer/browsers');
    const { PUPPETEER_REVISIONS } = requireRuntime('puppeteer-core');
    const buildId = PUPPETEER_REVISIONS['chrome-headless-shell'];
    const browserPlatform = detectBrowserPlatform();
    // Keep Chromium inside resources. Never depend on a personal browser/cache.
    let executable;
    try {
        const old = JSON.parse(await readFile(join(runtime, 'browser.json'), 'utf8'));
        if (old.build_id === buildId && old.target === `${platform}/${arch}`) {
            verify(runtime);
            await cp(join(runtime, 'browser'), join(stage, 'browser'), { recursive: true });
            executable = requireRuntime('@puppeteer/browsers').computeExecutablePath({
                cacheDir: join(stage, 'browser'),
                browser: Browser.CHROMEHEADLESSSHELL,
                buildId,
                platform: browserPlatform,
            });
        }
    } catch {
        /* No compatible prior browser. Download the locked revision. */
        await rm(join(stage, 'browser'), { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
    }
    if (!executable) {
        console.log(`[social-runtime] Downloading Chromium headless shell ${buildId}.`);
        const browser = await install({
            cacheDir: join(stage, 'browser'),
            browser: Browser.CHROMEHEADLESSSHELL,
            buildId,
            platform: browserPlatform,
        });
        executable = browser.executablePath;
    }
    await writeFile(
        join(stage, 'browser.json'),
        JSON.stringify({ target: `${platform}/${arch}`, build_id: buildId, executable: relative(stage, executable) })
    );
    process.stdout.write(verify(stage));
    await writeFile(
        join(stage, 'PLACEHOLDER'),
        'Generated by npm run prepare:social-runtime: portable Node 26, Fetchary and pinned Chromium headless shell.\n'
    );
    await writeFile(join(stage, '.inputs'), key);
    // Preserve the previous valid runtime until every preparation step succeeds.
    const previous = `${runtime}.previous`;
    await rm(previous, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
    await rename(runtime, previous);
    try {
        await rename(stage, runtime);
        process.stdout.write(verify(runtime));
    } catch (error) {
        await rm(runtime, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
        await rename(previous, runtime);
        throw error;
    }
    await rm(previous, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
    console.log(`[social-runtime] Bundled Node ${nodeVersion}, Fetchary and Chromium ${buildId}.`);
} finally {
    await rm(stage, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}
