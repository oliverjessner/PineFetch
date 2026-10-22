import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import http from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Real yt-dlp against isolated local fixtures. Never reads browser cookies or
// contacts a platform; verifies the external tool's subtitle metadata contract.
const directory = await mkdtemp(join(tmpdir(), 'pinefetch-creator-subtitles-'));
const original = 'WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nCreator terminology 🌲\n';
const requests = [];
const server = http.createServer((request, response) => {
    requests.push(request.url);
    if (request.url === '/original.vtt') {
        response.writeHead(200, { 'Content-Type': 'text/vtt' });
        response.end(original);
    } else if (request.url === '/video.mp4') {
        response.writeHead(200, { 'Content-Type': 'video/mp4' });
        response.end('synthetic media fixture');
    } else {
        response.writeHead(404);
        response.end();
    }
});
try {
    server.listen(0, '127.0.0.1');
    await once(server, 'listening');
    const origin = `http://127.0.0.1:${server.address().port}`;
    const fixture = join(directory, 'info.json');
    const media = join(directory, 'clip.mp4');
    await writeFile(
        fixture,
        JSON.stringify({
            id: 'fixture',
            title: 'Creator subtitles',
            extractor: 'generic',
            extractor_key: 'Generic',
            url: `${origin}/video.mp4`,
            webpage_url: `${origin}/video.mp4`,
            ext: 'mp4',
            subtitles: {
                en: [{ url: `${origin}/original.vtt`, ext: 'vtt' }],
                live_chat: [{ url: `${origin}/chat.json3`, ext: 'json3' }],
            },
            automatic_captions: { de: [{ url: `${origin}/automatic.vtt`, ext: 'vtt' }] },
        })
    );
    const run = (template = 'clip.%(ext)s') =>
        new Promise((resolve, reject) => {
            const child = spawn(
                process.env.PINEFETCH_TEST_YT_DLP || 'yt-dlp',
                [
                    '--ignore-config',
                    '--no-cache-dir',
                    '--no-playlist',
                    '--write-subs',
                    '--no-write-auto-subs',
                    '--sub-langs',
                    'all,-live_chat',
                    '--sub-format',
                    'best',
                    '--socket-timeout',
                    '5',
                    '--retries',
                    '0',
                    '--load-info-json',
                    fixture,
                    '-o',
                    join(directory, template),
                    '--print',
                    'after_move:pinefetch_subtitles:%(.{filepath,requested_subtitles})j',
                ],
                { timeout: 20000 }
            );
            let stdout = '';
            let stderr = '';
            child.stdout.setEncoding('utf8').on('data', text => {
                stdout += text;
            });
            child.stderr.setEncoding('utf8').on('data', text => {
                stderr += text;
            });
            child.on('error', reject);
            child.on('close', code =>
                code === 0 ? resolve(stdout) : reject(new Error(`yt-dlp exited ${code}: ${stderr}`))
            );
        });
    const output = await run();
    const line = output.split('\n').find(line => line.startsWith('pinefetch_subtitles:'));
    assert(line, 'yt-dlp must emit subtitle metadata after moving sidecars');
    const metadata = JSON.parse(line.slice('pinefetch_subtitles:'.length));
    assert.equal(metadata.filepath, media);
    assert.deepEqual(Object.keys(metadata.requested_subtitles), ['en']);
    assert.equal(metadata.requested_subtitles.en.ext, 'vtt');
    assert.equal(await readFile(join(directory, 'clip.en.vtt'), 'utf8'), original);
    assert.deepEqual([...new Set(requests)].sort(), ['/original.vtt', '/video.mp4']);
    assert((await readdir(directory)).includes('clip.mp4'));
    assert.equal(await readFile(media, 'utf8'), 'synthetic media fixture');
    // Existing subtitles can be reused and still appear in after_move metadata.
    assert((await run()).includes('pinefetch_subtitles:'));
    const noSubtitles = JSON.parse(await readFile(fixture, 'utf8'));
    noSubtitles.subtitles = {};
    await writeFile(fixture, JSON.stringify(noSubtitles));
    const absentOutput = await run('without-subtitles.%(ext)s');
    assert.equal(await readFile(join(directory, 'without-subtitles.mp4'), 'utf8'), 'synthetic media fixture');
    assert(absentOutput.includes('pinefetch_subtitles:'));
    assert(!requests.some(path => /automatic|chat/.test(path)));
    console.log(
        '[test:creator-subtitles] Real yt-dlp preserved original timing/text, excluded automatic captions/live chat, and reported reusable subtitle sidecars.'
    );
} finally {
    await new Promise(resolve => server.close(resolve));
    await rm(directory, { recursive: true, force: true });
}
