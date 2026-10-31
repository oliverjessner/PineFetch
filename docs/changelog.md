# 3.0.0

- Added local tags with Unicode-safe unique names, descriptions, palette and HEX colors, and multi-tag assignment for all saved media.
- Added a searchable Tags workspace with compact cards, color indicators, usage counts, action menus, empty states and contextual retry.
- Placed Video Details tags in a full-width Overview row below metadata and descriptions, leaving only title, platform and thumbnail in the header.
- Added non-modal quick tagging and Quick Create with keyboard navigation, responsive popovers and protection against late replies.
- Added paginated tag Content and Accounts views with search and return navigation.
- Added tag deletion confirmations that remove assignments while preserving saved media and text.
- Unified History's Search in, Source and Tag controls in one Filters popover with an active count and Clear all filters.
- Refined History with compact cards, truncated metadata, tag chips, separate thumbnails, top-right removal buttons and a narrower statistics sidebar.
- Associated creator videos, filters and insights with stable platform-specific IDs while keeping unverified downloads separate.
- Added explicit profile verification to resolve missing creator identities and recheck mutable handles before using saved account insights.
- Scoped deletion cleanup to the affected creator while preserving shared insights and rolling back failed deletions.
- Added backed-up database migrations for indexed tag relationships, creator identities, archive pagination, queue persistence and transcript media references.
- Included saved creator subtitles in keyword counts with language filters and shared Unicode matching rules.
- Linked keywords to paginated source occurrences and cut-aware video timestamps while preserving full-text copying and rejecting stale replies.
- Limited text and OCR views to 200 entries per page with full-result search, complete exports and keyboard and timestamp navigation.
- Debounced OCR searches by 250 ms and immediately invalidated older requests.
- Unified search and highlighting across existing archives with Unicode case folding and normalization while preserving original text, timestamps and exact creator navigation.
- Reduced database lock times for keywords and statistics and serialized heavy analyses to keep downloads and settings responsive.
- Accelerated History, Account pages and platform statistics with indexed queries, grouped totals and background database reads.
- Made Load more use stable cursors so new downloads and deletions do not shift subsequent pages.
- Limited unused playback previews to 2 GiB and 30 days with automatic cleanup at startup and during use.
- Protected originals and active players during preview cleanup and recovered interrupted conversions safely.
- Replaced the browser “Connected” label with “Public access checked” while keeping YouTube sign-in and protected-content access explicitly unverified.
- Added actionable, privacy-safe browser diagnostics while preserving saved settings when checks fail.
- Redesigned Settings with left-aligned categories, integrated Browser Import, contextual help, automatic saving and brief save feedback.
- Preserved Browser Import drafts and pairing secrets across category changes and linked native Dock shortcuts to Settings.
- Persisted waiting downloads, order, job options, Auto-start and Pause across restarts using atomic saves and exclusive profile locks.
- Restored unfinished jobs as Interrupted with Queue again and crash-safe completion receipts to prevent duplicate processing.
- Kept transcript exports linked to their retained video with safe handling of repeated exports, older entries and audio-only downloads.
- Bundled a checksum-pinned standalone Python and Whisper runtime that works without Homebrew or a separate Python installation.
- Hardened runtime installation with native-library audits, decoding and inference checks after relocation, atomic replacement and rollback.
- Enforced the transcription runtime's macOS 14 minimum before downloading media.
- Replaced premature Unsupported warnings with Not checked and preserved manually requested metadata checks during concurrent requests.
- Added clear History deletion confirmations, duplicate-action protection, focus recovery and retry while preserving downloaded files.
- Removed unused code and consolidated clipboard feedback and account snapshot reads.
- Shared download and Whisper process handling with bounded draining, cancellation, timeouts and inherited-pipe cleanup.
- Limited captured subprocess output to 128 MiB and streaming lines to 8 MiB.
- Expanded regression coverage for persistence, tagging, Unicode search, browser checks, clipboard failures and process cleanup.

# 2.6.2

- building fixe for new rust deps
- Updated the WAL snapshot restore test to use rusqlite 0.40's database name API.

# 2.6.1

- Updated @puppeteer/browsers to 3.2.6, puppeteer-core to 25.16.0 and @tauri-apps/cli to 2.12.4.
- Fixed Rust compilation with rusqlite 0.40 by enabling the required unsigned integer conversions for OCR and saved insights.

# 2.6.0

- Added the shared local video player to Subtitles with the same text/video layout as Transcript and OCR, timestamp seeking, playback highlighting and cut-aware subtitle timing.
- Transcript and OCR players now use only the saved video on disk, with local-file notices and OCR frame previews when playback is unavailable. Removed the remote YouTube embed fallback.
- Added automatic cached H.264/AAC playback previews when the platform cannot decode a saved video (including unsupported AV1/WebM files), preserving timestamp navigation and the original file.

- Added opt-in ID import (off by default): recognize 11-character YouTube IDs and explicit youtube:, tiktok: and instagram: inputs in Download, Magic import and TXT files, with platform hints for ambiguous IDs.

- Fixed blank History media previews when native video decoding stalls or fails: selected OCR frames stay visible until a video frame is decoded and remain available after playback errors.

- Added opt-in creator subtitle downloads in original languages and formats, with a Subtitles tab in Video Details, stored text, track selection, copying and original-timing notices for timestamp cuts.
- Integrated subtitle settings as schema 8 while preserving schema-7 browser authentication and existing settings.

- Moved YouTube browser authentication to Settings → Download and separated it from Browser Import and Link Dump initialization.

- updated deps
- Aligned the macOS Dock menu's muda dependency with Tauri at 0.20.0 so menu events share one dispatcher.
- Updated Link Dump secret generation to the getrandom 0.4 API.

# 2.5.1

- Removed the extra View account button from History details; accounts remain accessible through breadcrumbs and the Creator link in Overview.
- updating deps
- Fixed TikTok account insights to use the public uploader URL instead of an internal account ID, retry unavailable saved addresses on refresh and preserve previous insights when account identity cannot be verified.
- Replaced platform names in individual History items with available Font Awesome brand icons, keeping names as tooltips and screen-reader text and a visible name for platforms without an icon.
- Replaced History detail and account Back buttons with accessible History / Account / Post breadcrumbs, preserving list filters, loaded rows and scroll position and truncating long labels.
- Fixed Instagram Social Insights when browser background requests fail by retrying public metadata through Fetchary HTTP extraction; added compatibility with Fetchary 2.0.7's optional signed Reddit score while retaining older snapshots.

# 2.5.0

- Switched video and account Social Insights to Fetchary browser mode with bundled headless Chromium and isolated temporary profiles.

- Added an Account view from History video details, with paginated saved videos, explicit Fetchary account Social Insights saved in SQLite, and publicly accessible profile videos that can be opened in the download composer.
- Renamed History's source totals to “Downloads by platform” and added the same platform breakdown to `PineFetch stats`.

# 2.4.0

- Removed the source availability summary from the Keywords tab.

- Updated the Keywords Sources filter to use the same OJ choice menu as the History filters, including keyboard navigation and selected-option state.

- Added a Keywords tab with local word mining across saved transcripts, all OCR frames and captions, per-source counts, search and source filters. Only keywords with at least five total occurrences are included, sorted by Total descending; German and English articles are excluded regardless of letter case.
- Added reproducible offline screenshots for Social Insights and Keywords.

- Added a Social Insights tab: manually load public video metrics with Fetchary and save snapshots locally in SQLite.
- Fixed OCR source selection for transcriptions: use the retained video, require a supported video file and reject TXT/ANSI text animations in analysis and previews.
- Added “Remove All Duplicates” to OCR results, with a count preview, confirmation and atomic removal of consecutive identical frame texts.

# 2.3.3

- context menu

# 2.3.2

- new logo
- removed layout shift

# 2.3.0

- open in browser context menu in history
- the download path is now in the download menu
- search through transcriped texts
- "Show saved videos by this creator" context menu
- OCR analysis
- search amount displayed in history
- YouTube playlist support: pasting a playlist automatically queues its available videos in order

# 2.2.2

- moving to https://github.com/oliverjessner/oj-designsystem

# 2.2.1

- security fixes that are coming from dependabot
- adding a ci
- chore: strengthen data integrity and migration quality
- automatic screenshots

# 2.2.0

- storing the pinefetch version in the database
- cli --version
- visual fix for cards in history
- sha 256 for videos and captions
- better search with filters
- view captions and transcriptions in history
- storing the language of the transcription

# 2.1.0

- captions are stored in database
- compatibility list for download able videos / caption
- added captions for facebook, YouTube, TikTok, x, reddit
- downloadable thumbnails, via setting, default false,
- remove old legacy sqlite table
- allowing reddit, x and facebook for browser import and txt import

# 2.0.0

- history search
- ux overhaul
- code improvement
- project clean up
- updated deps
- improved readme

# 1.9.1

- captions
- import instagram urls

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
