# PineFetch CLI

Before adding your first download, configure the output folder and yt-dlp in the app's **Settings**.

## Accessing the CLI

### Manual macOS installation

Run the executable inside the installed app:

```bash
/Applications/PineFetch.app/Contents/MacOS/PineFetch --help
```

For the shorter command, add this alias to your shell configuration, such as `~/.zshrc`:

```bash
alias PineFetch='/Applications/PineFetch.app/Contents/MacOS/PineFetch'
```

Open a new terminal after saving the alias. Adjust the path if the app is installed elsewhere.

### Windows

Run `PineFetch.exe` from its installation directory. In PowerShell:

```powershell
.\PineFetch.exe --help
```

For an executable elsewhere, use its full path:

```powershell
& 'C:\path\to\PineFetch.exe' stats
```

Add the installation directory to your user `PATH` to use `PineFetch` from any terminal.

## Add a download

```bash
PineFetch queue add --link 'https://www.youtube.com/watch?v=VIDEO_ID' --preset 'best'
```

Replace the example URL with the link you want to download. Links must use `http://` or `https://` and be supported by yt-dlp.

| Option              | Required | Description                                                            |
| ------------------- | -------- | ---------------------------------------------------------------------- |
| `--link <URL>`      | Yes      | Link to download. Quote URLs to preserve shell characters such as `&`. |
| `--preset <PRESET>` | No       | Download preset; defaults to `best`.                                   |

### Presets

| Preset                 | Result                                            |
| ---------------------- | ------------------------------------------------- |
| `best`                 | Best available video and audio.                   |
| `max`                  | Video up to 1080p with audio.                     |
| `mp3`                  | Audio in MP3 format.                              |
| `opus`                 | Audio in Opus format.                             |
| `text`                 | Plain-text transcript.                            |
| `text with timestamps` | Transcript with segment start and end timestamps. |

Quote presets containing spaces:

```bash
PineFetch queue add --link 'https://www.youtube.com/watch?v=VIDEO_ID' --preset 'text with timestamps'
```

Omit `--preset` to use `best`:

```bash
PineFetch queue add --link 'https://www.youtube.com/watch?v=VIDEO_ID'
```

Downloads use the app's saved output folder, timestamp settings, transcription quality, and video-retention setting for transcripts. The current queue mode determines when downloads start:

- **Auto-start on:** new items begin downloading as the worker reaches them.
- **Auto-start off:** when no queue run is active, items wait until you click **Start queue** in the app. Items added during an active run join that run.

## List the queue

```bash
PineFetch queue list
```

Lists waiting downloads in processing order, with their number, preset, and link. Numbers start at `1`. Active and completed downloads are excluded.

An empty queue prints `No waiting downloads.`

## Remove a queue item

```bash
PineFetch queue remove 1
```

Removes the first waiting download. Replace `1` with another number from `queue list` to remove that item.

Numbers refer to the queue at the moment the command runs. They can change as downloads start or other items are removed. This command does not cancel an active download, delete downloaded files, or remove history entries. A missing or invalid position returns an error.

## View history

```bash
PineFetch history list
```

Shows the latest **25 successful downloads**, newest first, with their title and source link. If a title is unavailable, the filename is used when available.

**CLI history is read-only. There are no history delete, remove, or clear commands.** Listing history does not change existing entries or downloaded files.

An empty history prints `History is empty.`

## View statistics

```bash
PineFetch stats
```

Shows the same totals as the right side of the app's History view:

- **Downloaded videos:** the number of stored history entries.
- **Total data:** the combined recorded file size.
- **Total runtime:** the combined recorded media duration.

Statistics cover the entire history, not just the latest 25 entries. Missing file-size or duration metadata does not contribute to the corresponding total.

## Help and exit codes

```bash
PineFetch --help
```

`PineFetch -h` and `PineFetch help` also display help. Help and invalid commands do not launch the desktop app.

| Exit code | Meaning                                                                   |
| --------- | ------------------------------------------------------------------------- |
| `0`       | Command completed successfully.                                           |
| `1`       | Execution failed, such as an unavailable app or a nonexistent queue item. |
| `2`       | Invalid command or arguments.                                             |

Successful output goes to stdout; errors go to stderr.

## Connection and troubleshooting

The CLI uses a separate authenticated connection on the local machine. It works even when the **Link Dump** server is disabled and does not require a Link Dump connection secret.

- **Command not found:** check your Homebrew installation, shell alias, or Windows `PATH`, or invoke the executable by its full path.
- **Default output directory not set:** choose and save an output folder in the app's Settings.
- **App did not become ready:** the CLI waits up to 15 seconds after launching PineFetch. Open the app manually, let it finish loading, and retry.
- **Unknown preset:** use one of the six preset names above; quote `text with timestamps` as one argument.

## Development

Start the app and development web server from the repository root:

```bash
npm run dev
```

Then use the debug executable in another terminal:

```bash
src-tauri/target/debug/pinefetch --help
src-tauri/target/debug/pinefetch queue list
```

The development executable needs the development web server to display its desktop window. Packaged builds include the UI.
