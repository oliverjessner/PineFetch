# Tags

Open **History → Tags** to create, search, edit or delete your tags. Each tag has a
name, an optional description, a color, and counts of its saved items and derived
creator accounts. Unused tags stay visible. Lists are alphabetical by the stored
Unicode-normalized name key.

## Create and edit

Choose **Create Tag**, enter a name and optionally a description, then pick a color
from the palette or enter a custom HEX value. The live preview shows the result.
Names support up to 80 Unicode code points, descriptions up to 500. Surrounding
whitespace is trimmed. Name comparisons use PineFetch's locale-independent Unicode
normalization and full case folding: `Straße` and `STRASSE`, or composed/decomposed
umlauts, cannot become duplicate tags. Display spelling is retained.

Colors are stored as uppercase `#RRGGBB`; three-digit HEX shorthand is expanded.
Chips use a colored indicator and a readable neutral background, so names remain
readable for bright or dark custom colors. Hover a truncated name to see it fully.
Editing preserves the tag ID and every assignment.

## Assign tags

In **Video Details → Overview → Tags**, use **+ Add Tag**, or choose **Manage Tags**
from an item's context menu in History. Search available tags, add multiple tags,
or create a new tag and assign it immediately. Already assigned tags are excluded
from the available choices. Remove an individual assignment using its × control.
Changes are shown only after SQLite confirms them. Normal History chips update in
place without resetting already loaded pages or their scroll position. Errors preserve existing data
and remain visible for retry; no separate Save action is required.

Video, audio and transcript-only History entries can all be tagged. Discovered
remote profile videos must first be downloaded and saved in History. A normal
History row shows up to three tags, with a `+N` button for the rest. Assignments have
no per-tag or per-item limit.

## Browse and filter

Open a tag to browse its **Content** or **Accounts** tab. Content uses History's
newest-first order and saved-item cards. Load more to continue, search saved titles,
open item details or a creator, or remove that tag from an item. **Accounts** lists
one row per derived creator, its platform, tagged-item count and latest saved time.
Account search matches the displayed creator name. It opens the existing local
Account view through an actual associated History entry.

**Back to [tag]** returns from item or account details to the tag's selected tab,
query and scroll position. Tag mutations refresh membership and may restart the
current page so removed items and counts are accurate. The History breadcrumb
returns to ordinary History. Its single-tag filter combines with existing search,
platform and creator filters. **Clear tag filter** clears only that constraint.

## Creator identity

Tag accounts use the current schema-14 creator model, not the older display-name
model. Matching verified platform IDs are grouped, including renamed creators.
Equal display names do not merge different verified creators or platforms.
Unverified downloads remain separate, even if names match. Rows missing a usable
creator name or stored platform source are excluded from Accounts but remain in
Content. Accounts and their counts are derived from tagged items; independent
account tags are not created. See [CREATOR_IDENTITIES.md](CREATOR_IDENTITIES.md).
Tag browsing works offline and does not start a live profile lookup. Unavailable
thumbnail images do not prevent tagging.

## Persistence and deletion

Schema 15 adds `tags` and `history_entry_tags` without changing earlier migrations.
Definitions use UUIDs, an authoritative backend-generated unique `name_key`, and
PineFetch epoch-millisecond timestamps. SQLite foreign keys and a composite primary
key prevent orphaned or duplicate assignments. Writes use the existing writer
transaction; migration uses the existing backup, schema/integrity validation and
rollback workflow. The app version is unchanged.

Deleting a tag removes its assignments, never History entries, original files,
transcripts, captions, subtitles or OCR. Deleting History entries cascades only
their memberships. **Clear History** removes all memberships while preserving tag
definitions. Confirmation shows the tag's current affected-item count.

## Query and validation notes

`get_history_tags_batch` loads the visible History page in a bounded batch instead
of querying per card. Definition choices omit usage aggregation; overview and
individual tag summaries calculate counts in SQL. Tag membership has indexes in
both directions. Tagged content reuses History cursors and includes the tag in
cursor validation. Sparse tags start from indexed membership IDs; dense tags seek
History order and probe membership, avoiding a full member sort for every page.
Account results group in SQLite once per page and use a tag/query-bound continuation cursor.
Heavy aggregate requests share the existing analysis gate; choices, assignments
and normal History pages do not wait for that gate.
The frontend never fetches the entire saved-content archive.

Run `npm run check:fast`, `npm run check`, `npm run test:tags-ui`, `npm run test:ui`
and `npm run build:check`. Native tests use isolated databases and cover backed-up
migration/rollback/reopen, validation, uniqueness, CRUD, foreign-key cleanup,
failures, grouping, cursor/filter correctness and a synthetic 200,000-entry archive.
Browser tests use isolated transports and exercise both Chromium and WebKit,
including acknowledgement, failures, navigation, keyboard controls and late replies.
These checks do not modify a personal PineFetch profile or install the app.

An isolated debug-build run over 200,000 History entries and a tag with 100,000
assignments measured roughly 10 ms for a content page, 0.5 ms for a sparse tag,
400 ms for overview/account aggregation and 520 ms for deleting 100,000
assignments. These are local observations rather than cross-machine guarantees.
Content continuations check a bounded number of ordering candidates; account
aggregation still scales with the number of tagged entries.
