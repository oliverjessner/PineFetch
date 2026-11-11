# Screenshots

Run `npm run screenshots` to regenerate the WebP images in `src/images/mockups/`.
The workflow opens the real frontend in an isolated Chromium browser at
1400 × 1080 with a dark theme and a fixed clock. It uses local fixture data;
no account, saved user data, live Codex request or download is used.

| Image                          | View                                                          |
| ------------------------------ | ------------------------------------------------------------- |
| `download.webp`                | Download preview and populated queue                          |
| `history.webp`                 | History and statistics                                        |
| `more_data.webp`               | Video Details overview                                        |
| `ai_summary.webp`              | Ask this Video chat alongside Quick Summary and Key Takeaways |
| `ocr.webp`                     | OCR text and selected frame                                   |
| `social_insights.webp`         | Social Insights                                               |
| `keywords.webp`                | Saved-text keyword counts                                     |
| `account.webp`                 | Creator account                                               |
| `tags.webp`                    | Tags overview                                                 |
| `tag_content.webp`             | Tagged content                                                |
| `tag_accounts.webp`            | Tagged accounts                                               |
| `settings_downloads.webp`      | Settings → Downloads                                          |
| `settings_files_metadata.webp` | Settings → Files & Metadata                                   |
| `settings_browser.webp`        | Settings → Browser                                            |
| `settings_browser_import.webp` | Settings → Browser Import                                     |
| `settings_diagnostics.webp`    | Settings → Diagnostics                                        |

Long Settings categories also produce numbered continuation images, such as
`settings_browser_import_2.webp`, so controls below the initial viewport are
included. Help disclosures remain in their default collapsed state.
`settings.webp` and `browser_import.webp` remain aliases for existing README links.

The AI fixture in `test/fixtures/screenshots/ai_summary.json` contains a cached
summary and conversation based on the existing synthetic transcript. Its saved
messages exercise Markdown and both chat bubble styles. The workflow checks the
transcript hash, summary, takeaways, message count and enabled composer before
capture. It also checks that every current Settings category has a capture and
waits for both diagnostics components to finish.

The command replaces images in the mockup directory. It fails on browser errors
or unsupported fixture commands rather than capturing a broken state.
