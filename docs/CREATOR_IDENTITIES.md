# Saved creator identities

Schema 14 stores `creator_source` and `creator_id` beside each History entry.
The ordered identity index serves both Account pages and the History context-menu
creator filter. Display names remain presentation and search text.

The metadata parser accepts YouTube's 24-character `UC` channel ID and numeric
user IDs for TikTok, Instagram, X/Twitter, Facebook, Twitch and Vimeo. X uses a
numeric `channel_id` before `uploader_id`; other numeric platforms use
`uploader_id`. Twitter and X share the canonical `x` scope. Mutable handles never
become stable IDs. Missing IDs, including unsupported extractors, stay unverified.

A successful download commits its metadata ID atomically with History. Opening an
Account is local and groups only matching verified platform IDs. An unverified
entry shows only itself and an explanation. Explicit profile discovery can obtain
and persist its ID; it does not bulk-assign old rows by name. IDs already stored
cannot be silently overwritten. YouTube profile fetches use the canonical channel
URL; mutable profile handles on other platforms are resolved again for explicit
network actions.

`creator_account_insights` uses `(source, creator_id)`. Unverified snapshots use an
entry-specific key, preventing cross-account sharing. The schema migration backs
up and preserves older `history_account_insights` snapshots without guessing an
association. Single-entry deletion checks only the affected keys using membership
indexes; ambiguous legacy records are retained conservatively. Clear History
clears both snapshot tables. Cleanup and History deletion share one transaction.

Tests cover rename and same-name separation, platform scope, missing IDs, metadata
validation, atomic completion, backed-up migration/rollback/reopen, index layout
validation, stable cursor ordering and targeted deletion with shared snapshots.
