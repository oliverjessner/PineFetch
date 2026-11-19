# Cleanup after publishing

Preview generated files and their file-content sizes without changing anything:

```sh
npm run cleanup -- --dry-run
```

Remove the listed generated files:

```sh
npm run cleanup
```

A successful `npm run publish` runs the same cleanup after the GitHub release has been created and the Homebrew tap changes have been pushed. A failed publish skips cleanup and leaves the remaining build files available for diagnosis. If publishing succeeds but cleanup fails, the error identifies the local cleanup failure separately. Resolve that issue and run `npm run cleanup`; publishing again is unnecessary.

## Removed files

Cleanup uses an explicit allowlist inside the PineFetch checkout:

| Path                                                        | Contents                                                                      |
| ----------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `src-tauri/target`                                          | Cargo build outputs, generated resource copies, and local app and DMG bundles |
| `src-tauri/gen/schemas`                                     | Generated Tauri schemas                                                       |
| `src-tauri/vendor/glib/target`                              | Build outputs from testing the vendored GLib library separately               |
| `src-tauri/vendor/glib/Cargo.lock`                          | The generated lockfile for those standalone GLib tests                        |
| `coverage`                                                  | Generated coverage reports                                                    |
| `src-tauri/resources/whisper-runtime.previous`              | A previous generated Whisper runtime                                          |
| `src-tauri/resources/social-runtime.previous`               | A previous generated social runtime                                           |
| Generated children of `src-tauri/resources/whisper-runtime` | Prepared standalone Python and Whisper files                                  |
| Generated children of `src-tauri/resources/ffmpeg-runtime`  | Prepared FFmpeg executables and libraries                                     |
| Generated children of `src-tauri/resources/deno-runtime`    | The prepared Deno executable                                                  |
| Generated children of `src-tauri/resources/ocr-runtime`     | The compiled OCR helper                                                       |
| Generated children of `src-tauri/resources/social-runtime`  | Prepared Node, Fetchary, production dependencies, and Chromium files          |

Runtime directories retain their source-controlled `.keep` and `PLACEHOLDER` files.

PineFetch stores its current local `.app` and `.dmg` inside `src-tauri/target`, so cleanup removes them too. Manual cleanup also removes unpublished local bundles and files needed for retries. Published GitHub releases and assets remain available; cleanup does not change them or verify publication before a manual run.

The next build recreates missing outputs and runtimes. It can take longer and may download runtime files again.

## Preserved files and checks

Source directories, `docs/licenses`, icons, vendored source code, and `node_modules` are outside the allowlist. Databases, backups, manually saved screenshots, and other artifacts outside the listed output paths remain untouched. Treat the listed output directories as disposable. Cleanup does not scan global Cargo, npm, Homebrew, or system caches, or installed PineFetch app data.

Before removing any files, the script verifies the PineFetch package identity and Git checkout root, checks every destination, and refuses tracked files or symlinks at cleanup targets or their parents. Symlinks inside a removable generated directory are removed as links without following their destinations.

Publish and manual cleanup share `.pinefetch-publish.lock`. An existing, incomplete, or stale lock stops cleanup; it is never removed automatically. Check that no publish or cleanup is running before resolving a leftover lock. An existing `src-tauri/resources/.whisper-runtime.lock` also stops cleanup to protect runtime preparation.

The dry run uses the same checks, refuses existing locks, and creates no lock or other files.
