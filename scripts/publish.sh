#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH= cd "$(dirname "$0")" && pwd)
REPO_DIR=$(CDPATH= cd "$SCRIPT_DIR/.." && pwd)
cd "$REPO_DIR"

# Publish and manual cleanup share one lock, including all preparation gates.
PUBLISH_LOCK="$REPO_DIR/.pinefetch-publish.lock"
if ! mkdir "$PUBLISH_LOCK"; then
    echo "Another publish or cleanup holds $PUBLISH_LOCK. Finish it first; inspect stale locks manually." >&2
    exit 1
fi
TMP_CHANGELOG=
TMP_RELEASE_NOTES=

cleanup() {
    if [ -n "$TMP_CHANGELOG" ]; then rm -f "$TMP_CHANGELOG"; fi
    if [ -n "$TMP_RELEASE_NOTES" ]; then rm -f "$TMP_RELEASE_NOTES"; fi
    rm -f "$PUBLISH_LOCK/$$.pid"
    rmdir "$PUBLISH_LOCK" || echo "Could not remove $PUBLISH_LOCK; inspect the remaining lock files." >&2
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
: > "$PUBLISH_LOCK/$$.pid"

# Version preparation is explicit; the following gates are read-only.
npm run sync:version
npm run check
npm run build:check

RELEASE_REPO="oliverjessner/PineFetch"
BUNDLE_DIR="src-tauri/target/release/bundle"
APP="$BUNDLE_DIR/macos/PineFetch.app"
DMG_DIR="$BUNDLE_DIR/dmg"
LOCAL_CHANGELOG="docs/changelog.md"
HOMEBREW_TAP_DIR=${HOMEBREW_TAP_DIR:-"$REPO_DIR/../homebrew-tap"}
TMP_CHANGELOG=$(mktemp)
TMP_RELEASE_NOTES=$(mktemp)

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "Missing required command: $1"
        exit 1
    fi
}

commit_homebrew_changes() {
    HOMEBREW_COMMIT_MESSAGE=$1
    set --
    for tap_file in Casks/pinefetch.rb README.md; do
        if [ -e "$HOMEBREW_TAP_DIR/$tap_file" ] ||
            git -C "$HOMEBREW_TAP_DIR" ls-files --error-unmatch -- "$tap_file" >/dev/null 2>&1; then
            set -- "$@" "$tap_file"
        fi
    done
    if [ "$#" -eq 0 ]; then
        return
    fi

    git -C "$HOMEBREW_TAP_DIR" add --all -- "$@"
    if ! git -C "$HOMEBREW_TAP_DIR" diff --cached --quiet -- "$@"; then
        # --only also leaves unrelated staged tap changes out of this commit.
        git -C "$HOMEBREW_TAP_DIR" commit --only -m "$HOMEBREW_COMMIT_MESSAGE" -- "$@"
    fi
}

require_command npm
require_command node
require_command git
require_command gh
require_command codesign
require_command hdiutil
require_command shasum
require_command curl

if [ ! -d "$HOMEBREW_TAP_DIR/.git" ]; then
    echo "Homebrew tap not found at $HOMEBREW_TAP_DIR"
    echo "Set HOMEBREW_TAP_DIR to the tap checkout before publishing."
    exit 1
fi

git -C "$HOMEBREW_TAP_DIR" fetch origin
TAP_BRANCH=$(git -C "$HOMEBREW_TAP_DIR" branch --show-current)
if [ -z "$TAP_BRANCH" ]; then
    echo "Homebrew tap is in detached HEAD state. Check out its release branch before publishing."
    exit 1
fi

if [ "$(git -C "$HOMEBREW_TAP_DIR" rev-parse HEAD)" != "$(git -C "$HOMEBREW_TAP_DIR" rev-parse "origin/$TAP_BRANCH")" ]; then
    echo "Homebrew tap is not synchronized with origin/$TAP_BRANCH. Pull or push its commits before publishing."
    exit 1
fi

VERSION=$(node -p 'require("./package.json").version || "0.0.0"')
TAG="v$VERSION"

if git rev-parse "$TAG" >/dev/null 2>&1 || gh release view "$TAG" --repo "$RELEASE_REPO" >/dev/null 2>&1; then
    echo "Release $TAG already exists. Increase the version before publishing."
    exit 1
fi

# Preserve existing PineFetch tap edits before the generated cask replaces them.
commit_homebrew_changes "Prepare PineFetch release $TAG"

if [ -n "$(git status --porcelain)" ]; then
    echo "Committing working tree before release..."
    git add -A
    if ! git diff --cached --quiet; then
        git commit -m "chore: release $TAG"
    fi
fi

echo "Cleaning previous builds..."
rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR"

echo "Building the app..."
npm run build -- --bundles app

if [ ! -d "$APP" ]; then
    echo "App not found at $APP"
    exit 1
fi

codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

mkdir -p "$DMG_DIR"
OUT="$DMG_DIR/PineFetch_${VERSION}_aarch64_adhoc.dmg"
hdiutil create -volname "PineFetch" -srcfolder "$APP" -ov -format UDZO "$OUT"
echo "Created $OUT"

git push origin HEAD

cp "$LOCAL_CHANGELOG" "$TMP_CHANGELOG"
awk -v version="$VERSION" '
    $0 == "# " version { capture=1 }
    capture && $0 ~ /^# / && $0 != "# " version { exit }
    capture { print }
' "$TMP_CHANGELOG" > "$TMP_RELEASE_NOTES"

if [ ! -s "$TMP_RELEASE_NOTES" ]; then
    cp "$TMP_CHANGELOG" "$TMP_RELEASE_NOTES"
fi

echo "Creating GitHub release $TAG on $RELEASE_REPO..."
gh release create "$TAG" "$OUT" \
    --repo "$RELEASE_REPO" \
    --title "PineFetch $TAG" \
    --notes-file "$TMP_RELEASE_NOTES"

echo "Updating Homebrew cask..."
DMG_SHA256=$(shasum -a 256 "$OUT" | awk '{ print $1 }')
node scripts/update-homebrew-tap.mjs "$HOMEBREW_TAP_DIR" "$VERSION" "$DMG_SHA256"

if command -v brew >/dev/null 2>&1; then
    brew style "$HOMEBREW_TAP_DIR/Casks/pinefetch.rb"
fi

commit_homebrew_changes "Update PineFetch cask to $TAG"
# Also publish a preparation commit if the generated cask was already current.
git -C "$HOMEBREW_TAP_DIR" push origin HEAD

echo "Release and Homebrew cask published successfully."

echo "Cleaning generated build files..."
if ! node "$SCRIPT_DIR/cleanup.mjs" --after-publish; then
    echo "Publishing succeeded, but local cleanup failed. Resolve the cleanup issue and run npm run cleanup; do not publish the release again." >&2
    exit 1
fi

echo "Opening the PineFetch release page"
if ! open -a "Google Chrome" "https://github.com/$RELEASE_REPO/releases/tag/$TAG"; then
    echo "The release was published successfully, but its page could not be opened." >&2
fi
