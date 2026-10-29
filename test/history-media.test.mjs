import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHistoryMediaView } from '../src/history-media-view.js';

// Model only the browser surface so native decode events and stalled loads can
// be exercised deterministically, including events after leaving a History row.
class MediaElement extends EventTarget {
    hidden = true;
    readyState = 0;
    videoWidth = 0;
    currentTime = 0;
    duration = 30;
    attributes = new Map();
    set src(value) {
        this.attributes.set('src', value);
    }
    get src() {
        return this.getAttribute('src');
    }
    getAttribute(name) {
        return this.attributes.get(name) ?? null;
    }
    removeAttribute(name) {
        this.attributes.delete(name);
    }
    pause() {}
    load() {}
}
const setup = (api = {}, appendLog = () => {}) => {
    const root = new EventTarget();
    const elements = Object.fromEntries(
        ['video', '[data-media-fallback]', '[data-media-status]'].map(selector => [selector, new MediaElement()])
    );
    root.querySelector = selector => elements[selector];
    const times = [];
    const media = createHistoryMediaView({
        root,
        appendLog,
        api: {
            getHistoryMedia: async ({ compatible } = {}) => {
                if (compatible) throw new Error('Synthetic conversion failure');
                return { path: '/saved/video.webm', lease_id: null };
            },
            releaseHistoryMedia: async () => {},
            fileSource: path => path,
            ...api,
        },
        onTime: time => times.push(time),
    });
    media.setEntry({ id: 'video', url: 'https://www.youtube.com/watch?v=jNQXAC9IVRw' });
    return {
        media,
        root,
        video: elements.video,
        frame: elements['[data-media-fallback]'],
        status: elements['[data-media-status]'],
        times,
    };
};
const decoded = video => {
    video.readyState = 2;
    video.videoWidth = 1920;
    video.dispatchEvent(new Event('loadeddata'));
};

test('loads only the video resolved from the History ID and retains it across tab changes', async t => {
    const calls = [];
    const { media, video } = setup({
        getHistoryMedia: async args => {
            calls.push(args);
            return { path: '/saved/Grüße #1.mp4', lease_id: null };
        },
        fileSource: path => `asset:${path}`,
    });
    t.after(media.pause);
    await media.activate();
    assert.deepEqual(calls, [{ id: 'video' }]);
    assert.equal(video.src, 'asset:/saved/Grüße #1.mp4');
    decoded(video);
    media.seek(4);
    media.pause();
    await media.activate();
    assert.equal(calls.length, 1);
    assert.equal(video.currentTime, 4);
    assert.equal(media.hasVideo(), true);
});

test('a missing local video shows a notice and allows retry after reopening the tab', async t => {
    let missing = true;
    const { media, video, status } = setup({
        getHistoryMedia: async () => {
            if (missing) throw new Error('The saved video is unavailable');
            return { path: '/saved/video.mp4', lease_id: null };
        },
    });
    t.after(media.pause);
    await media.activate();
    assert.equal(video.src, null);
    assert.equal(video.hidden, true);
    assert.equal(status.hidden, false);
    assert.match(status.textContent, /No saved video is available/);
    missing = false;
    media.pause();
    await media.activate();
    decoded(video);
    assert.equal(video.src, '/saved/video.mp4');
    assert.equal(status.hidden, true);
});

test('a stalled decoder prepares local MP4 once and preserves the selected time', async t => {
    t.mock.timers.enable({ apis: ['setTimeout'] });
    const calls = [];
    let finish;
    const { media, video, status } = setup({
        getHistoryMedia: args => {
            calls.push(args);
            return args.compatible
                ? new Promise(resolve => (finish = resolve))
                : Promise.resolve({ path: '/saved/video.webm', lease_id: null });
        },
    });
    t.after(media.pause);
    await media.activate();
    media.seek(18.018);
    t.mock.timers.tick(12000);
    assert.deepEqual(calls, [{ id: 'video' }, { id: 'video', compatible: true }]);
    assert.equal(video.hidden, true);
    assert.match(status.textContent, /Preparing local video/);
    // Preparation has its own native process deadline, not the decoder's 12s deadline.
    t.mock.timers.tick(60000);
    assert.equal(calls.length, 2);
    finish({ path: '/cache/playback.mp4', lease_id: null });
    await Promise.resolve();
    assert.equal(video.src, '/cache/playback.mp4');
    video.readyState = 1;
    video.dispatchEvent(new Event('loadedmetadata'));
    decoded(video);
    assert.equal(video.currentTime, 18.018);
    assert.equal(status.hidden, true);
    // An unusable derivative must not cause an endless conversion loop.
    video.dispatchEvent(new Event('error'));
    assert.equal(calls.length, 2);
    assert.equal(video.hidden, true);
    assert.match(status.textContent, /could not be loaded/);
});

test('leaving during preparation ignores the derivative reply and starts no decoder timer', async t => {
    t.mock.timers.enable({ apis: ['setTimeout'] });
    let finish;
    const { media, video } = setup({
        getHistoryMedia: ({ compatible }) =>
            compatible
                ? new Promise(resolve => (finish = resolve))
                : Promise.resolve({ path: '/saved/video.webm', lease_id: null }),
    });
    await media.activate();
    video.dispatchEvent(new Event('error'));
    media.setEntry(null);
    finish({ path: '/cache/old-playback.mp4', lease_id: null });
    await Promise.resolve();
    t.mock.timers.tick(12000);
    assert.equal(video.src, null);
    assert.equal(video.hidden, true);
});

test('conversion failures show a useful notice and retain the native error in the log', async () => {
    const errors = [];
    const { media, video, status } = setup({}, (message, isError) => errors.push({ message, isError }));
    await media.activate();
    video.dispatchEvent(new Event('error'));
    await Promise.resolve();
    assert.match(status.textContent, /could not be prepared/);
    assert.equal(video.hidden, true);
    assert.equal(errors.length, 1);
    assert.match(errors[0].message, /Synthetic conversion failure/);
    assert.equal(errors[0].isError, true);
});

test('metadata alone cannot suppress a selected OCR image; decoded video replaces it', async t => {
    const { media, video, frame, times } = setup();
    t.after(media.pause);
    await media.activate();
    media.seek(18.018);
    media.setFrame('data:image/jpeg;base64,selected');
    video.readyState = 1;
    video.dispatchEvent(new Event('loadedmetadata'));
    video.dispatchEvent(new Event('timeupdate'));
    assert.equal(media.hasVideo(), false);
    assert.equal(frame.hidden, false);
    assert.equal(video.currentTime, 18.018);
    assert.deepEqual(times, []);
    decoded(video);
    assert.equal(media.hasVideo(), true);
    assert.equal(frame.hidden, true);
    video.dispatchEvent(new Event('timeupdate'));
    assert.deepEqual(times, [18.018]);
});

test('a local decode error preserves an existing OCR image', async () => {
    const { media, video, frame, root } = setup();
    let unavailable = 0;
    root.addEventListener('pinefetch:media-unavailable', () => unavailable++);
    await media.activate();
    media.setFrame('data:image/jpeg;base64,selected');
    video.dispatchEvent(new Event('error'));
    assert.equal(media.hasVideo(), false);
    assert.equal(video.hidden, true);
    assert.equal(frame.hidden, false);
    assert.equal(frame.src, 'data:image/jpeg;base64,selected');
    assert.equal(unavailable, 1);
    // Late events from the abandoned decoder cannot cover the frame again.
    decoded(video);
    video.dispatchEvent(new Event('error'));
    assert.equal(frame.hidden, false);
    assert.equal(unavailable, 1);
});

test('a frame returned after playback failure replaces the local error message', async () => {
    const { media, video, frame, status } = setup();
    await media.activate();
    video.dispatchEvent(new Event('error'));
    assert.equal(status.hidden, false);
    assert.match(status.textContent, /Preparing local video/);
    media.setFrame('data:image/jpeg;base64,late');
    assert.equal(frame.hidden, false);
    assert.equal(status.hidden, true);
});

test('a decoder that never loads or fails releases the OCR fallback after a bounded wait', async t => {
    t.mock.timers.enable({ apis: ['setTimeout'] });
    const { media, video, frame, root } = setup();
    let unavailable = 0;
    root.addEventListener('pinefetch:media-unavailable', () => unavailable++);
    await media.activate();
    media.setFrame('data:image/jpeg;base64,selected');
    t.mock.timers.tick(12000);
    assert.equal(unavailable, 1);
    assert.equal(video.hidden, true);
    assert.equal(frame.hidden, false);
    assert.equal(media.hasVideo(), false);
});

test('leaving a pending load cancels timeout and ignores late native replies', async t => {
    t.mock.timers.enable({ apis: ['setTimeout'] });
    let resolve;
    const { media, video, root } = setup({
        getHistoryMedia: () => new Promise(done => (resolve = done)),
    });
    let unavailable = 0;
    root.addEventListener('pinefetch:media-unavailable', () => unavailable++);
    const loading = media.activate();
    media.setEntry(null);
    resolve({ path: '/old/video.webm', lease_id: null });
    await loading;
    video.dispatchEvent(new Event('error'));
    t.mock.timers.tick(12000);
    assert.equal(video.src, null);
    assert.equal(unavailable, 0);
});

test('switching tabs while the decoder is loading cancels its timeout and allows retry', async t => {
    t.mock.timers.enable({ apis: ['setTimeout'] });
    const { media, video, root } = setup();
    let unavailable = 0;
    root.addEventListener('pinefetch:media-unavailable', () => unavailable++);
    await media.activate();
    media.pause();
    decoded(video);
    t.mock.timers.tick(12000);
    assert.equal(unavailable, 0);
    assert.equal(media.hasVideo(), false);
    await media.activate();
    decoded(video);
    assert.equal(media.hasVideo(), true);
});

test('a loaded preview stays pinned while paused and releases only after its source is detached', async () => {
    const released = [];
    const { media, video } = setup({
        getHistoryMedia: async () => ({ path: '/cache/preview.mp4', lease_id: 'loaded-preview' }),
        releaseHistoryMedia: async args => {
            assert.equal(video.src, null, 'detach the decoder before releasing its file');
            released.push(args);
        },
    });
    await media.activate();
    decoded(video);
    media.pause();
    await media.activate();
    assert.deepEqual(released, []);
    media.setEntry(null);
    media.setEntry(null);
    assert.deepEqual(released, [{ leaseId: 'loaded-preview' }]);
});

test('leaving a pending decoder load releases the preview and late decode events cannot retain it', async () => {
    const released = [];
    const { media, video } = setup({
        getHistoryMedia: async () => ({ path: '/cache/preview.mp4', lease_id: 'loading-preview' }),
        releaseHistoryMedia: async args => released.push(args.leaseId),
    });
    await media.activate();
    media.pause();
    decoded(video);
    media.setEntry(null);
    assert.deepEqual(released, ['loading-preview']);
});

test('obsolete replies release their own native reference without releasing the current player', async () => {
    let finish;
    const released = [];
    const { media, video } = setup({
        getHistoryMedia: ({ id }) =>
            id === 'video'
                ? new Promise(resolve => {
                      finish = resolve;
                  })
                : Promise.resolve({ path: '/cache/new.mp4', lease_id: 'current' }),
        releaseHistoryMedia: async args => released.push(args.leaseId),
    });
    const previous = media.activate();
    media.setEntry({ id: 'new' });
    await media.activate();
    decoded(video);
    finish({ path: '/cache/old.mp4', lease_id: 'obsolete' });
    await previous;
    assert.deepEqual(released, ['obsolete']);
    assert.equal(video.src, '/cache/new.mp4');
    media.setEntry(null);
    assert.deepEqual(released, ['obsolete', 'current']);
});

test('replacement, failed decoding and obsolete conversion replies release each reference exactly once', async () => {
    const released = [];
    let finish;
    const { media, video } = setup({
        getHistoryMedia: ({ compatible }) =>
            compatible
                ? new Promise(resolve => {
                      finish = resolve;
                  })
                : Promise.resolve({ path: '/cache/first.mp4', lease_id: 'first' }),
        releaseHistoryMedia: async args => released.push(args.leaseId),
    });
    await media.activate();
    video.dispatchEvent(new Event('error'));
    assert.deepEqual(released, ['first']);
    media.setEntry(null);
    finish({ path: '/cache/late.mp4', lease_id: 'late' });
    await Promise.resolve();
    assert.deepEqual(released, ['first', 'late']);
    media.setEntry({ id: 'new' });
    await media.activate();
    video.dispatchEvent(new Event('error'));
    finish({ path: '/cache/second.mp4', lease_id: 'second' });
    await Promise.resolve();
    video.dispatchEvent(new Event('error'));
    video.dispatchEvent(new Event('error'));
    assert.deepEqual(released, ['first', 'late', 'first', 'second']);
    media.setEntry(null);
});

test('an unusable file URL releases the reference and release failures cannot leak backend details', async () => {
    const logs = [];
    let released = 0;
    const { media } = setup(
        {
            getHistoryMedia: async () => ({ path: '/cache/preview.mp4', lease_id: 'unusable' }),
            fileSource: () => '',
            releaseHistoryMedia: async () => {
                released++;
                throw new Error('/private/cache/backend-detail');
            },
        },
        text => logs.push(text)
    );
    await media.activate();
    await Promise.resolve();
    assert.equal(released, 1);
    assert(logs.some(text => text.includes('Could not release')));
    assert(logs.every(text => !text.includes('backend-detail')));
    media.setEntry(null);
});
