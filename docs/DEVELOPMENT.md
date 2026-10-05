# Development and quality checks

Component responsibilities and dependency boundaries are documented in
[ARCHITECTURE.md](ARCHITECTURE.md).

Database migrations, legacy imports, SQLite snapshots and file completion rules
are documented in [DATA-INTEGRITY.md](DATA-INTEGRITY.md). Their offline regression
tests run in the same Rust suite and `npm run check` gate described here.

PineFetch's documented release and Homebrew installation target macOS on Apple
silicon. The quality workflow uses `macos-15` (arm64). The existing Windows build
helper remains available; this workflow does not establish Windows or Linux
support.

## Prerequisites

- Node.js **26.10.0**, recorded in `.node-version` and `package.json`. Select this
  version with your Node version manager before installing dependencies.
- Rust **1.93.0**, with Clippy and rustfmt. Install
  [rustup](https://rustup.rs/); it reads `rust-toolchain.toml` automatically when
  commands run in this checkout. This pins the already tested compiler version.
- Xcode or the Xcode Command Line Tools, including the macOS SDK and Clang:

  ```sh
  xcode-select --install
  xcrun --find clang
  xcrun --sdk macosx --show-sdk-path
  ```

Tauri links Apple's native frameworks and WebKit on macOS. SQLite is compiled
from the bundled `rusqlite` source, so a C compiler is required. No Homebrew
SQLite or GTK installation is needed for the application checks. The existing
process tests also use the standard macOS `sh` and `sleep` commands.

The GitHub runner supplies Xcode, the SDK, and these native system tools; the
shared setup action verifies their availability. It installs the pinned Rust
toolchain and Node version explicitly. The npm cache only accelerates downloads;
there is no dependency on a warmed cache or a developer's runtime installations.

## Install and check

```sh
npm ci
npm run check
```

`check` stops on the first failure and returns a nonzero exit code. It checks
version consistency, ESLint, Clippy, Prettier, rustfmt, JavaScript tests, and Rust
tests. Neither it nor the individual check commands writes tracked files, repairs
lockfiles, synchronizes versions, starts a watcher, launches the desktop app, or
downloads platform content. Cargo may download locked dependencies and writes
ignored build output.

| Command                                     | Purpose                                                                 |
| ------------------------------------------- | ----------------------------------------------------------------------- |
| `npm run check:versions`                    | Read-only comparison of npm, Tauri, and Cargo versions                  |
| `npm run lint`                              | ESLint and Clippy                                                       |
| `npm run lint:js`                           | ESLint recommended correctness rules, with warnings treated as failures |
| `npm run lint:rust`                         | `cargo clippy --locked --all-targets -- -D warnings`                    |
| `npm run format:check`                      | Prettier and rustfmt, without modifications                             |
| `npm run format:check:js`                   | Check owned frontend, configuration, scripts, and documentation         |
| `npm run format:check:rust`                 | `cargo fmt --all -- --check`                                            |
| `npm test`                                  | Both test suites, once                                                  |
| `npm run test:js`                           | Native Node.js test runner; each file has a 30-second timeout           |
| `npm run test:rust`                         | `cargo test --locked --all-targets`                                     |
| `npm run format`                            | Explicit automatic formatting with Prettier and rustfmt                 |
| `npm run format:js` / `npm run format:rust` | Format only one language                                                |
| `npm run build:check`                       | Release compilation and linking, including embedded frontend assets     |

Run one existing test while investigating a failure:

```sh
node --test --test-timeout=30000 --test-name-pattern="timestamps" test/url-utils.test.mjs
cargo test --manifest-path src-tauri/Cargo.toml --locked --all-targets normalizes_youtube_watch_url
```

For a version change, run `npm run sync:version` explicitly, then run `npm run
check` and commit the updated manifests and lockfiles together. This preparation
updates application version metadata only, not dependency versions. It preserves
the Tauri configuration's formatting. It is never invoked by `check`.

## Test scope and exclusions

All existing application Rust tests run. They use in-memory SQLite databases,
uniquely named temporary paths, local mock endpoints, and local fake processes.
They do not open PineFetch's real database, use personal download directories, or
require a running desktop app. JavaScript tests exercise URL domain boundaries,
timestamps, normalization, TXT import counts, and existing deduplication rules.
Release-gate tests execute an isolated copy of `publish.sh` with mocked npm,
repository, signing, network, and publishing commands. They never publish.

The frontend was already written as browser ES modules. `src/package.json`
declares that existing module type for Node imports; the root package's module
configuration is unchanged. ESLint gives browser code browser globals and gives
Node scripts and tests only Node globals. No bundler or test framework is added.

Prettier follows the existing four-space indentation and single-quoted JavaScript,
with two-space Markdown/YAML indentation. Rust formatting uses rustfmt defaults.
Generated Tauri schemas, build output, npm dependencies, bundled runtimes,
binaries, and unchanged vendor code are excluded. `package-lock.json` and
`Cargo.lock` are generated files; their correctness is enforced by `npm ci`,
version checks, and locked Cargo commands rather than reformatting them.

The vendored GLib security backport retains upstream source formatting and tests.
It is not a workspace member, so application rustfmt/Clippy/test commands do not
rewrite or run its upstream suite. See [its provenance and separate release-mode
regression command](../src-tauri/vendor/README.md). Running that optional command
on macOS additionally requires native GLib development libraries. Default
application features are checked; `--all-features` is deliberately not used.

## Build check versus a distributable release

`npm run build:check` runs Cargo with `--locked --release --features
custom-protocol`. It compiles and links the application and embeds the checked-in
frontend. Committed runtime placeholder directories make this work on a clean
checkout without Python, FFmpeg, Deno, yt-dlp, signing credentials, or a prepared
runtime bundle. It does not synchronize versions or invoke runtime preparation.

This check does **not** validate Whisper/FFmpeg/Deno runtime contents, an app
bundle or DMG, installation, runtime downloads, signatures, or notarization.
Those remain release validation responsibilities. The existing `npm run build`
prepares the runtimes and creates distribution artifacts; it needs the external
tools specified in the existing runtime preparation scripts. Normal pull-request
CI neither runs that packaging path nor publishes artifacts or updates Homebrew.

## CI and required merge gates

`.github/workflows/quality.yml` runs on every pull request into `main`, every
push to `main`, and manual dispatch. Both jobs use the shared setup action,
`npm ci`, and the exact local npm commands. There are no path filters, optional
required jobs, or separate copies of the check rules. Runs have time limits and
superseded runs for the same pull request are canceled. External actions are
pinned to verified full commit SHAs with version comments. Workflow permissions
are `contents: read`; ordinary pull requests need no secrets.

Make these **two exact status check names** required for `main`:

- `Quality checks (macOS)`
- `Build (macOS)`

In **Settings → Rules → Rulesets**, create an active branch ruleset targeting
`main` (or use **Settings → Branches → Branch protection rules**):

1. Require a pull request before merging.
2. Require both status checks above to pass, selecting GitHub Actions as their
   source. Enable **Require branches to be up to date before merging**.
3. Apply the rules to administrators too and leave the bypass list empty; with
   classic protection, enable **Do not allow bypassing the above settings**.
4. Block force pushes and branch deletion. Do not allow direct updates that
   bypass the required pull request and checks.

The checks must run once before their names appear in the GitHub selector. A
workflow file alone does not enforce merge protection. This change does not
activate branch protection or modify any repository settings. Both individual
jobs are required, so failure or cancellation cannot be hidden by a successful
summary job. No merge queue is configured by this change; enabling one later
requires adding the corresponding workflow trigger before relying on it.

## Before publishing

Prepare the version and changelog in a pull request, then merge it through the
required checks. Publish from the resulting clean `main` checkout after both
`Quality checks (macOS)` and `Build (macOS)` have passed for that commit. Run
`npm run check` and `npm run build:check` locally before packaging as well.

`npm run publish` explicitly prepares versions, then runs those same two gates
before any commit, push, release creation, signing, or Homebrew action. A failed
gate terminates the script. Its existing commit/push and packaging behavior is
otherwise retained. With protected `main`, release-time working-tree changes
must go through a pull request rather than bypassing protection.

Check the resulting distribution's bundled runtimes, launch behavior, and DMG
installation separately. Passing the compile/link check is not a successful
distribution test. Personal signing credentials and notarization remain outside
normal CI. Do not run the real publishing script just to test the quality gates;
`npm run test:js` covers both failure paths with isolated mocks.
