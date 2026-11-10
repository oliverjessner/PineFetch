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
PineFetch history list 100
```

Shows successful downloads, newest first, with their title, upload date, history creation time and source link. Without a count, it shows the latest **25** entries. Pass a positive integer, such as `100`, to show that many entries, or all available entries if fewer exist:

```text
#   TITLE           UPLOAD_DATE           CREATED_AT            LINK
1   Example video   2026-11-09 18:00:00   2026-11-10 12:34:56   https://example.com/video
2   Example audio   2026-11-08            2026-11-10 12:30:00   https://example.com/audio
```

`UPLOAD_DATE` uses the saved upload timestamp when available. If the source supplied only a date, it shows `YYYY-MM-DD` without adding a time. `CREATED_AT` shows when the history entry was created. Full timestamps use `YYYY-MM-DD HH:MM:SS` in the app computer's local time zone. Missing or invalid values show `—`. If a title is unavailable, the filename is used when available.

An empty history prints `History is empty.`

### Read an entry

```bash
PineFetch history read 1
```

Prints the first history entry's saved metadata as formatted JSON, including its ID, title, uploader, source link, creator identity, file details, duration, size and checksum when recorded. The output also includes assigned tags, output-file availability and summaries of related saved transcripts, captions and creator subtitles. Original stored date values remain in `entry`; the additional `dates` object contains readable upload, creation and completion dates in local time.

### Find and choose a transcription

```bash
PineFetch history transcription
PineFetch history transcription --choose
```

Without an argument, lists the latest **25 entries with a saved transcription**, newest first, using the same title, date and link columns as `history list`. Entries without a saved transcription are excluded before the limit is applied. The displayed numbers are their global history positions, so gaps are expected when other entries have no transcription.

Add `--choose` to select a displayed history number and print its complete saved transcript. The selection stays tied to the displayed entry even if another download completes while you choose. Enter `q` or end input to cancel without printing a transcript.

The chooser requires interactive standard input and standard error. Its list and prompt go to stderr; only the exact transcript text goes to stdout, so you can save it directly:

```bash
PineFetch history transcription --choose > transcript.txt
```

Piped input is not supported by `--choose`. Use the numbered command below for scripts.

### Read a transcription by history number

```bash
PineFetch history transcription 1
```

Prints the saved transcript text for the first history entry without a heading or added newline. Timestamped transcripts retain their saved timestamps. The text comes from the local database and remains available even if the exported transcript file is missing.

The direct `transcription <NUMBER>` command uses the same global history numbers as both lists and works without an interactive terminal.

Global history numbers start at `1` and use the same newest-first order across history commands. They are resolved when each command runs and can shift as downloads complete or entries are deleted in the app. The interactive chooser preserves the entries shown in its own list while you select. Use `history list 100` to view positions beyond the default 25. A nonexistent entry or an entry without a saved transcription returns an error with exit code `1`. Counts and entry numbers must be positive integers; invalid arguments return exit code `2`.

**CLI history is read-only. There are no history delete, remove, or clear commands.** These commands do not change existing entries or downloaded files.

## View statistics

```bash
PineFetch stats
```

Shows the same totals as the right side of the app's History view:

- **Downloaded videos:** the number of stored history entries.
- **Total data:** the combined recorded file size.
- **Total runtime:** the combined recorded media duration.
- **Downloads by platform:** completed History entries per platform, for example `YouTube: 3` and `TikTok: 5`.

Statistics cover the entire history, regardless of the count used for `history list`. Missing file-size or duration metadata does not contribute to the corresponding total.

## Help and exit codes

```bash
PineFetch --help
PineFetch --version
```

The help header and `PineFetch --version` show the installed PineFetch version. `PineFetch -h` and `PineFetch help` also display help; `PineFetch -V` and `PineFetch version` also display the version. These commands and invalid commands do not launch the desktop app.

| Exit code | Meaning                                                                                                  |
| --------- | -------------------------------------------------------------------------------------------------------- |
| `0`       | Command completed successfully.                                                                          |
| `1`       | Execution failed, such as an unavailable app, nonexistent queue/history entry, or missing transcription. |
| `2`       | Invalid command or arguments, including a count or entry number that is not a positive integer.          |

Successful output goes to stdout; errors go to stderr. The interactive transcription chooser also sends its list and prompt to stderr, keeping stdout available for the transcript alone. Cancellation exits successfully without transcript output. Attempting `--choose` without interactive stdin and stderr returns an error with exit code `1`.

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
