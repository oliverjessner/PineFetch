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
const setup = (api = {}) => {
    const root = new EventTarget();
    const elements = Object.fromEntries(
        ['video', 'iframe', '[data-media-fallback]', '[data-media-status]'].map(selector => [
            selector,
            new MediaElement(),
        ])
    );
    root.querySelector = selector => elements[selector];
    const times = [];
    const media = createHistoryMediaView({
        root,
        api: { getHistoryMedia: async () => '/saved/video.webm', fileSource: path => path, ...api },
        onTime: time => times.push(time),
    });
    media.setEntry({ id: 'video', url: 'https://www.youtube.com/watch?v=jNQXAC9IVRw' });
    return {
        media,
        root,
        video: elements.video,
        embed: elements.iframe,
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

test('a local decode error preserves an existing OCR image instead of hiding it behind YouTube', async () => {
    const { media, video, frame, embed, root } = setup();
    let unavailable = 0;
    root.addEventListener('pinefetch:media-unavailable', () => unavailable++);
    await media.activate();
    media.setFrame('data:image/jpeg;base64,selected');
    video.dispatchEvent(new Event('error'));
    assert.equal(media.hasVideo(), false);
    assert.equal(video.hidden, true);
    assert.equal(frame.hidden, false);
    assert.equal(embed.hidden, true);
    assert.equal(frame.src, 'data:image/jpeg;base64,selected');
    assert.equal(unavailable, 1);
    // Late events from the abandoned decoder cannot cover the frame again.
    decoded(video);
    video.dispatchEvent(new Event('error'));
    assert.equal(frame.hidden, false);
    assert.equal(unavailable, 1);
});

test('a frame returned after playback failure replaces the remote embed', async () => {
    const { media, video, frame, embed } = setup();
    await media.activate();
    video.dispatchEvent(new Event('error'));
    assert.equal(embed.hidden, false);
    media.setFrame('data:image/jpeg;base64,late');
    assert.equal(frame.hidden, false);
    assert.equal(embed.hidden, true);
    assert.equal(embed.getAttribute('src'), null);
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
    const { media, video, embed, root } = setup({
        getHistoryMedia: () => new Promise(done => (resolve = done)),
    });
    let unavailable = 0;
    root.addEventListener('pinefetch:media-unavailable', () => unavailable++);
    const loading = media.activate();
    media.setEntry(null);
    resolve('/old/video.webm');
    await loading;
    video.dispatchEvent(new Event('error'));
    t.mock.timers.tick(12000);
    assert.equal(video.src, null);
    assert.equal(embed.src, null);
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
