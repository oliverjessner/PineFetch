import fs from 'node:fs';
import path from 'node:path';

const [tapDirArg, version, sha256] = process.argv.slice(2);

if (!tapDirArg || !version || !sha256) {
    console.error('Usage: node scripts/update-homebrew-tap.mjs <tap-dir> <version> <sha256>');
    process.exit(1);
}

if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
    console.error(`Invalid version: ${version}`);
    process.exit(1);
}

if (!/^[a-f0-9]{64}$/i.test(sha256)) {
    console.error('SHA256 must contain exactly 64 hexadecimal characters.');
    process.exit(1);
}

const tapDir = path.resolve(tapDirArg);
const caskDir = path.join(tapDir, 'Casks');
const caskPath = path.join(caskDir, 'pinefetch.rb');
const readmePath = path.join(tapDir, 'README.md');

if (!fs.existsSync(path.join(tapDir, '.git'))) {
    console.error(`Homebrew tap not found at ${tapDir}`);
    process.exit(1);
}

if (!fs.existsSync(readmePath)) {
    console.error(`Homebrew tap README not found at ${readmePath}`);
    process.exit(1);
}

const cask = `cask "pinefetch" do
  version "${version}"
  sha256 "${sha256.toLowerCase()}"

  url "https://github.com/oliverjessner/PineFetch/releases/download/v#{version}/PineFetch_#{version}_aarch64_adhoc.dmg",
      verified: "github.com/oliverjessner/PineFetch/"
  name "PineFetch"
  desc "Local-first yt-dlp desktop client"
  homepage "https://github.com/oliverjessner/PineFetch"

  depends_on arch: :arm64
  depends_on macos: :big_sur

  app "PineFetch.app"
  binary "#{appdir}/PineFetch.app/Contents/MacOS/PineFetch", target: "PineFetch"

  zap trash: [
    "~/Library/Application Support/com.pinefetch.app",
    "~/Library/Preferences/com.pinefetch.app.plist",
  ]
end
`;

let readme = fs.readFileSync(readmePath, 'utf8');
const packageRow =
    '| `pinefetch` | Cask | Local-first yt-dlp desktop client | `brew install --cask oliverjessner/tap/pinefetch` |';

if (!readme.includes('| `pinefetch` |')) {
    const rowAnchor =
        '| `bulkpixel` | Cask | Local-first batch image converter | `brew install --cask oliverjessner/tap/bulkpixel` |';
    if (!readme.includes(rowAnchor)) {
        console.error('Could not find the package table anchor in the Homebrew tap README.');
        process.exit(1);
    }
    readme = readme.replace(rowAnchor, `${rowAnchor}\n${packageRow}`);
}

if (!readme.includes('brew install --cask pinefetch')) {
    const installAnchor = 'brew install --cask bulkpixel';
    if (!readme.includes(installAnchor)) {
        console.error('Could not find the short-install anchor in the Homebrew tap README.');
        process.exit(1);
    }
    readme = readme.replace(installAnchor, `${installAnchor}\nbrew install --cask pinefetch`);
}

if (!readme.includes('## PineFetch Cask')) {
    readme = `${readme.trimEnd()}

## PineFetch Cask

PineFetch is installed as a macOS cask:

\`\`\`sh
brew tap oliverjessner/tap
brew install --cask oliverjessner/tap/pinefetch
\`\`\`

The app is currently not signed with an Apple Developer ID or notarized. If macOS blocks the first launch, right-click \`PineFetch.app\` in \`/Applications\`, select \`Open\`, and confirm the dialog.
`;
}

fs.mkdirSync(caskDir, { recursive: true });
fs.writeFileSync(caskPath, cask);
fs.writeFileSync(readmePath, readme);

console.log(`Updated ${caskPath}`);
console.log(`Updated ${readmePath}`);
