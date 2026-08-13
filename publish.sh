#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH= cd "$(dirname "$0")" && pwd)
cd "$SCRIPT_DIR"

RELEASE_REPO="oliverjessner/PineFetch"
BUNDLE_DIR="src-tauri/target/release/bundle"
APP="$BUNDLE_DIR/macos/PineFetch.app"
DMG_DIR="$BUNDLE_DIR/dmg"
LOCAL_CHANGELOG="changelog.md"
HOMEBREW_TAP_DIR=${HOMEBREW_TAP_DIR:-"$SCRIPT_DIR/../homebrew-tap"}
TMP_CHANGELOG=$(mktemp)
TMP_RELEASE_NOTES=$(mktemp)

cleanup() {
    rm -f "$TMP_CHANGELOG" "$TMP_RELEASE_NOTES"
}

trap cleanup EXIT

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "Missing required command: $1"
        exit 1
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

if [ -n "$(git -C "$HOMEBREW_TAP_DIR" status --porcelain)" ]; then
    echo "Homebrew tap has uncommitted changes. Commit or stash them before publishing."
    exit 1
fi

git -C "$HOMEBREW_TAP_DIR" fetch origin
TAP_BRANCH=$(git -C "$HOMEBREW_TAP_DIR" branch --show-current)
if [ -z "$TAP_BRANCH" ]; then
    echo "Homebrew tap is in detached HEAD state. Check out its release branch before publishing."
    exit 1
fi

if ! git -C "$HOMEBREW_TAP_DIR" diff --quiet "HEAD..origin/$TAP_BRANCH"; then
    echo "Homebrew tap is not synchronized with origin/$TAP_BRANCH. Pull or push its commits before publishing."
    exit 1
fi

npm run sync:version
VERSION=$(node -p 'require("./package.json").version || "0.0.0"')
TAG="v$VERSION"

if git rev-parse "$TAG" >/dev/null 2>&1 || gh release view "$TAG" --repo "$RELEASE_REPO" >/dev/null 2>&1; then
    echo "Release $TAG already exists. Increase the version before publishing."
    exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "Committing working tree before release..."
    git add -A
    git commit -m "chore: release $TAG"
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

git -C "$HOMEBREW_TAP_DIR" add Casks/pinefetch.rb README.md

if git -C "$HOMEBREW_TAP_DIR" diff --cached --quiet; then
    echo "Homebrew cask already up to date."
else
    git -C "$HOMEBREW_TAP_DIR" commit -m "Update PineFetch cask to $TAG"
    git -C "$HOMEBREW_TAP_DIR" push origin HEAD
fi

echo "Release and Homebrew cask published successfully."

echo "Opening the PineFetch release page"
open -a "Google Chrome" "https://github.com/$RELEASE_REPO/releases/tag/$TAG"
