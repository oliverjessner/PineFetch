# 3.0.0

- Polished History cards with a coordinated thumbnail/remove group, tighter metadata-to-tag spacing, grouped and truncated metadata with full-value tooltips, and compact OJ tag components. Reused design-system spacing, typography, colors and focus styles across standard and narrow layouts.

- Moved Video Details tags beside the platform in the shared header, visible across all tabs. Kept Add Tag next to up to three chips (one in narrow windows), with an assigned-tags overflow for removal and protection against stale replies during video changes.

- Promoted Tags to its own full-width main section, removed the nested All Content / Tags navigation, and retained collection, search, pagination and contextual Back navigation across sections. Added a purposeful empty state for the first tag.
- Simplified History to search, Search in and a Filters popover for Source and Tag, with an active-filter count and individual or combined reset. Retained the narrower statistics sidebar and existing combined query behavior.

- Replaced modal tag assignment with an anchored, searchable popover: toggle checked tags without closing it, or create and add a tag inline. Quick creation retains saved tags on assignment failures and offers Retry Add.
- Separated quick tagging from full tag management in the dedicated Tags section. Preserved the full Create/Edit dialog, added safeguards against nested modals, and improved keyboard focus, narrow-window placement and delayed-request handling without changing tag storage.

- Added locally persisted tags with Unicode-safe unique names, descriptions, a color palette, custom HEX colors and immediate multi-tag assignment for all saved media types.
- Added centralized tag management, compact tag chips, Video Details controls and a Manage Tags context action. Tag deletion confirms affected items and preserves saved media and text.
- Added dedicated tag Content and Accounts pages with search, pagination and return navigation, plus an independent History tag filter that combines with existing filters.
- Added a backed-up schema-15 migration with indexed many-to-many membership, transactional writes, batched tag loading and account counts based on current verified creator IDs; unverified creators remain entry-scoped.

- Associate saved creator videos and account insights with stable, platform-scoped IDs from download metadata. Creator filters also use those identities; names and handles no longer merge unrelated accounts or split renamed creators.
- Keep older downloads without a stable ID isolated and explain their unverified status. Explicit profile loading can resolve membership; mutable profile handles are checked again, while YouTube uses its canonical channel URL.
- Added a backed-up schema-14 migration, indexed identity pagination and separate identity-scoped account snapshots. Preserve legacy snapshots without guessing their creator.
- Restrict deletion cleanup to the removed entry's identity and legacy name keys. Keep shared insights until the final member is removed, preserve unrelated snapshots and roll back entry deletion if cleanup fails. An isolated 200,000-entry / 20,000-insight archive completed targeted deletion in about 0.9 ms.

- Include saved creator subtitles in local keyword counts, with separate language columns and filters and shared Unicode token rules for counts and source occurrences.
- Link words and source counts to paginated occurrences, original saved source text and available video timestamps, adjusting subtitle playback for retained cuts. Full copying preserves original text; source revisions and view generations reject changed data and late replies.

- Limit text and loaded OCR views to 200 visible entries per page, with full-result search, unchanged copy exports and keyboard/time navigation across pages. Large untimed documents retain original offsets and Unicode-safe highlights across display sections.
- Debounce OCR searches by 250 ms and invalidate older replies immediately. A 25,000-row browser fixture rendered 200 entries in about 23 ms in both tested engines.

- Bound captured subprocess output to a shared 128 MiB stdout/stderr budget while reading. Oversized output fails explicitly and terminates the process group, with timeout, inherited-pipe and exact-budget regressions.

- Shortened shared database lock windows by snapshotting keyword text before counting and sorting, preserving source order, limits and complete OCR/caption coverage.
- Moved legacy statistics URL classification and source-count sorting outside the shared connection lock, with one consistent read transaction for totals, groups and fallback data.
- Serialized heavy analyses on a separate gate so queued work cannot reserve the database connection or accumulate large text snapshots; added lock measurements and concurrent settings/completion-write regressions.

- Unified History, creator search, descriptions, transcripts, OCR, subtitles and keyword filtering with locale-independent Unicode case folding and canonical normalization, including umlauts, ß/SS and composed/decomposed text.
- Kept highlights aligned with the original text when normalization or case folding changes character lengths, preserving copied text and timestamp navigation.
- Applied the same comparisons to existing archives without a schema migration; preserved indexed exact creator navigation and stable cursor pagination.

- Automatically trim unused local playback previews to 2 GiB and remove previews unused for 30 days, with cleanup at startup and once per minute.
- Protect loaded players with shared file leases, release obsolete replies and closed views, and clear old-page references on reload without blocking playback during conversion.
- Preserve originals, linked files and foreign cache contents; recover interrupted preview conversions safely and recreate evicted previews when needed.

- Replaced the browser “Connected” label with “Public access checked” and explicitly kept YouTube sign-in and access to protected videos unverified.
- Added private-data-safe browser diagnostics for profile and permission issues, locked databases, cookie decryption, empty cookie stores, network failures, sign-in challenges and rate limits, with actionable retry guidance.
- Kept saved browser settings unchanged after failed checks, rejected incomplete success reports, and detected cookie-decryption warnings even when a public request succeeds.

- Integrated Browser Import into Settings, including local server controls, extension pairing and saved connection management; native Dock shortcuts open the new category directly.
- Left-aligned the Settings workspace, switches, fields and actions, and kept import drafts and generated secrets available when switching categories.

- Redesigned Settings with a vertical category sidebar for Downloads, Files & Metadata, Browser and the existing Diagnostics tools, with compact navigation at smaller widths.
- Kept all existing controls and automatic persistence, added concise descriptions with expandable help, and replaced permanent save confirmations with brief feedback after successful saves.
- Distinguished saved browser choices from checked connections, retained permission and cookie guidance in Troubleshooting, and added responsive, keyboard, persistence and failure-state regressions.

- Accelerated History and saved Account pages with matching order/source/creator indexes and focused, parameterized search queries.
- Made Load more continue from the last saved position, retaining the initial count and avoiding shifted pages after new downloads or deletions. History reads now run outside the UI thread.
- Grouped platform totals in SQLite and limited legacy URL fallback to entries without a source. Added a backed-up schema-13 migration, large-archive measurements and pagination regressions.

- Persisted waiting downloads, their order and complete job options, plus Auto-start and Pause, across application restarts.
- Restored unfinished jobs as Interrupted with preserved output paths and an explicit Queue again action. Completed receipts prevent automatic reprocessing after a crash.
- Saved queue changes atomically before acknowledgement, protected each profile with an operating-system lock and added restart, process-exit, storage-failure and browser regressions.

- Kept every transcript export linked to its actual retained video, including repeated exports with unique filenames, so History playback and OCR keep using the correct source.
- Added atomic media-reference persistence and a backed-up schema-11 migration. Older transcripts retain conservative source lookup; new audio-only transcripts never select a neighboring video.

- Replaced the Homebrew-linked Whisper virtual environment with checksum-pinned standalone CPython, its standard library and hash-pinned binary dependencies.
- Added native dependency auditing, wheel relocation, isolated decoding checks and atomic runtime replacement with rollback; Python no longer needs to be installed separately.
- Added an explicit moved-runtime decode and real model inference check with Homebrew access blocked. Documented the existing macOS 14 minimum of the transcription dependencies and reject older systems before downloading media.

- Replaced premature Unsupported link warnings with a neutral Not checked state and guidance to check other HTTP(S) links manually.
- Preserved explicitly requested metadata checks while an older request is running, without enabling automatic previews for additional sites.

- Added confirmation before removing a History entry, naming its saved texts, subtitles, OCR results and insights while preserving downloaded files.
- Clarified Clear History's deletion scope, prevented duplicate deletion actions and preserved keyboard focus and retry after failures.

- Removed unused application CSS, obsolete DOM references and an unused field from internal Link Dump secret validation.
- Consolidated History clipboard feedback and reused account snapshots to avoid a duplicate initial database read, preserving retry and stale-response handling.
- Shared the download and Whisper streaming-process lifecycle, with bounded output draining, process cleanup and an 8 MiB line limit.
- Added regression coverage for clipboard failures, account snapshot reuse, streaming cancellation, inherited pipes and malformed process output.

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
