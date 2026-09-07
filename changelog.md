# 1.9.0

- notifications
- cli

# 1.8.1

- Renamed the Link Dump API endpoints from `addYoutube` to `addVideo`.
- Added support for Instagram and TikTok videos sent through the PineFetch Link Dump browser extension.

# 1.8.0

- Reduced History pagination to 20 entries per page.
- Improved CSS and aligned PineFetch with `pinefetch-designsystem`.
- Improved menu rendering with a 500-line log limit, caching, and lazy loading.
- Separated transcription options from the other settings.
- Added automatic Homebrew Cask publishing, including checksum generation and tap commits.
- Spell-checked the changelog.

# 1.7.2

- Fixed missing audio in TikTok downloads.

# 1.7.1

- Added TXT imports for lists of TikTok links.

# 1.7.0

- Added transcription quality options.
- Added text transcripts with timestamps.
- Stored transcripts in the database.
- Added an option to keep videos alongside their transcripts.

# 1.6.2

- Updated dependencies through Dependabot.
- Added tracking for medium, source, and uploader metadata.
- Added a Clear button to the terminal.

# 1.6.0

- Added the History view with runtime and file-size tracking.

# 1.5.1

- Fixed an FFmpeg issue.

# 1.5.0

- Fixed timestamp handling.
- Fixed the bundled FFmpeg runtime for post-processing.

# 1.4.5

- Added History pagination.
- Added all available metadata to History.

# 1.4.3

- Fixed presets for links sent through PineFetch Link Dump.
- Improved History.
- Migrated History storage to SQLite.

# 1.4.0

- Added the [PineFetch Design System](https://github.com/oliverjessner/PineFetch-Designsystem).
- Added support for sending links to PineFetch through [PineFetch Link Dump](https://github.com/oliverjessner/PineFetch-Link-Dump).

# 1.3.0

- Added TXT import.

# 1.2.0

- Added downloads starting at a URL timestamp such as `t=3`.
- Added non-blocking rendering.

# 1.0.1

- Fixed code-quality issues with [ItWorksBut](https://github.com/oliverjessner/ItWorksBut).

# 1.0.0

- Added starting and stopping downloads.

# 0.2.0

## Highlights

- Added a context menu with actions to download again or cancel.
- Added Magic Import, which inserts a URL from the clipboard into an empty URL field.
- Added History.

## Improvements

- Fixed scrolling issues.
- Added support for pressing `Esc` to clear the URL input and loaded information.

# 0.1.0

## Highlights

- Added a video-to-text mode.
- Clicking a completed queue item opens the downloaded output file.
- Added a **Clear** button for the queue.

## Improvements

- General UI cleanup and polish.
- Added thumbnail previews in queue items.
- Added visual feedback for invalid URLs.
- Added keyboard shortcut: `Cmd + I` to trigger **Show info**.
- Pressing `Enter` queues the URL currently in the input field.
- The URL input is automatically focused after queuing.

**Note:** Please only download content you have the rights or permission to access.
