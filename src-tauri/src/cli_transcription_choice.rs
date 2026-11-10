//! Keyboard selection from a fixed snapshot of saved transcriptions.
use std::io::{self, Write};
use std::time::Duration;

const ESCAPE_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_ESCAPE_BYTES: usize = 16;
const HIDE_CURSOR: &[u8] = b"\x1b[?25l";
const SHOW_CURSOR: &[u8] = b"\x1b[?25h\r\n";

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Choice {
    pub number: u32,
    pub id: String,
    pub title: String,
    pub url: String,
    pub upload_date: Option<String>,
    pub created_at: Option<String>,
}

pub(crate) trait KeyInput {
    fn read_byte(&mut self, timeout: Option<Duration>) -> io::Result<Option<u8>>;

    fn dimensions(&self) -> (usize, usize) {
        (24, 100)
    }
}

fn terminal_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

pub(crate) fn format_choices(choices: &[Choice]) -> String {
    if choices.is_empty() {
        return "No saved transcriptions.".into();
    }
    let mut output = String::from("#\tTITLE\tUPLOAD_DATE\tCREATED_AT\tLINK");
    for choice in choices {
        output.push_str(&format!(
            "\n{}\t{}\t{}\t{}\t{}",
            choice.number,
            terminal_text(&choice.title),
            terminal_text(choice.upload_date.as_deref().unwrap_or("—")),
            terminal_text(choice.created_at.as_deref().unwrap_or("—")),
            terminal_text(&choice.url)
        ));
    }
    output
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Previous,
    Next,
    First,
    Last,
    Select,
    Cancel,
    Ignore,
}

fn escape_key(input: &mut impl KeyInput) -> io::Result<Key> {
    let Some(prefix) = input.read_byte(Some(ESCAPE_TIMEOUT))? else {
        return Ok(Key::Cancel);
    };
    if prefix != b'[' && prefix != b'O' {
        return Ok(Key::Ignore);
    }
    let mut sequence = Vec::new();
    for _ in 0..MAX_ESCAPE_BYTES {
        let Some(byte) = input.read_byte(Some(ESCAPE_TIMEOUT))? else {
            return Ok(Key::Ignore);
        };
        if matches!(byte, 3 | 4 | 27) {
            return Ok(Key::Cancel);
        }
        sequence.push(byte);
        if (0x40..=0x7e).contains(&byte) {
            return Ok(match sequence.as_slice() {
                b"A" => Key::Previous,
                b"B" => Key::Next,
                b"H" => Key::First,
                b"F" => Key::Last,
                b"1~" | b"7~" if prefix == b'[' => Key::First,
                b"4~" | b"8~" if prefix == b'[' => Key::Last,
                _ => Key::Ignore,
            });
        }
    }
    Ok(Key::Ignore)
}

fn read_key(input: &mut impl KeyInput) -> io::Result<Key> {
    Ok(match input.read_byte(None)? {
        None | Some(3 | 4 | b'q' | b'Q') => Key::Cancel,
        Some(27) => return escape_key(input),
        Some(b'k' | b'K') => Key::Previous,
        Some(b'j' | b'J') => Key::Next,
        Some(b'\r' | b'\n') => Key::Select,
        _ => Key::Ignore,
    })
}

// A UTF-8 byte budget is conservative for wide terminal characters, and
// leaving the last column unused avoids terminal automatic line wrapping.
fn fit_line(value: &str, columns: usize) -> String {
    let budget = columns.saturating_sub(1);
    if value.len() <= budget {
        return value.into();
    }
    let suffix = match budget {
        0 => return String::new(),
        1 | 2 => ".",
        _ => "…",
    };
    let mut end = budget - suffix.len();
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{suffix}", &value[..end])
}

fn choice_label(choice: &Choice, selected: bool) -> String {
    let sanitized = terminal_text(&choice.title);
    let title = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = if title.is_empty() { "Untitled" } else { &title };
    let marker = if selected { '>' } else { ' ' };
    format!("{marker} {} {title}", choice.number)
}

fn selector_lines(choices: &[Choice], selected: usize, dimensions: (usize, usize)) -> Vec<String> {
    let (rows, columns) = (dimensions.0.max(1), dimensions.1);
    if rows == 1 {
        return vec![fit_line(&choice_label(&choices[selected], true), columns)];
    }
    let show_header = rows >= 3;
    let available = rows - 1 - usize::from(show_header);
    let visible_count = choices
        .len()
        .min(rows.saturating_sub(4).max(3))
        .min(available);
    let max_start = choices.len() - visible_count;
    let start = selected.saturating_sub(visible_count / 2).min(max_start);
    let mut lines = Vec::new();
    if show_header {
        lines.push(fit_line(
            "Choose a transcription (↑/↓ or j/k, Enter to select, Esc/q to cancel)",
            columns,
        ));
    }
    for (index, choice) in choices.iter().enumerate().skip(start).take(visible_count) {
        lines.push(fit_line(&choice_label(choice, index == selected), columns));
    }
    lines.push(fit_line(
        &format!("{}/{}", selected + 1, choices.len()),
        columns,
    ));
    lines
}

struct SelectorDisplay<'a, W: Write> {
    output: &'a mut W,
    line_count: usize,
}

impl<W: Write> SelectorDisplay<'_, W> {
    fn render(&mut self, lines: &[String], rows: usize) -> io::Result<()> {
        let previous = self.line_count.min(rows.max(1));
        if previous > 1 {
            write!(self.output, "\x1b[{}A", previous - 1)?;
        }
        let painted = previous.max(lines.len());
        for index in 0..painted {
            self.output.write_all(b"\x1b[2K\r")?;
            if let Some(line) = lines.get(index) {
                self.output.write_all(line.as_bytes())?;
            }
            if index + 1 < painted {
                self.output.write_all(b"\r\n")?;
            }
        }
        if painted > lines.len() {
            write!(self.output, "\x1b[{}A\r", painted - lines.len())?;
        }
        self.line_count = lines.len();
        self.output.flush()
    }
}

impl<W: Write> Drop for SelectorDisplay<'_, W> {
    fn drop(&mut self) {
        let _ = self.output.write_all(SHOW_CURSOR);
        let _ = self.output.flush();
    }
}

pub(crate) fn choose<I: KeyInput, W: Write>(
    choices: &[Choice],
    input: &mut I,
    output: &mut W,
) -> Result<Option<String>, String> {
    if choices.is_empty() {
        writeln!(output, "No saved transcriptions.")
            .and_then(|()| output.flush())
            .map_err(|error| format!("Could not display transcription choices: {error}"))?;
        return Ok(None);
    }
    let mut display = SelectorDisplay {
        output,
        line_count: 0,
    };
    display
        .output
        .write_all(HIDE_CURSOR)
        .map_err(|error| format!("Could not display transcription choices: {error}"))?;
    let mut selected = 0;
    let dimensions = input.dimensions();
    display
        .render(&selector_lines(choices, selected, dimensions), dimensions.0)
        .map_err(|error| format!("Could not display transcription choices: {error}"))?;
    loop {
        match read_key(input)
            .map_err(|error| format!("Could not read transcription selection: {error}"))?
        {
            Key::Select => return Ok(Some(choices[selected].id.clone())),
            Key::Cancel => return Ok(None),
            Key::Ignore => continue,
            Key::Previous => selected = (selected + choices.len() - 1) % choices.len(),
            Key::Next => selected = (selected + 1) % choices.len(),
            Key::First => selected = 0,
            Key::Last => selected = choices.len() - 1,
        }
        let dimensions = input.dimensions();
        display
            .render(&selector_lines(choices, selected, dimensions), dimensions.0)
            .map_err(|error| format!("Could not display transcription choices: {error}"))?;
    }
}

#[cfg(test)]
mod tests {
    use super::{choose, format_choices, selector_lines, Choice, KeyInput, ESCAPE_TIMEOUT};
    use std::io::{self, Cursor, Read, Write};
    use std::time::Duration;

    struct MockKeyInput {
        bytes: Cursor<Vec<u8>>,
        dimensions: (usize, usize),
        timeouts: Vec<Option<Duration>>,
        fail: bool,
    }

    impl MockKeyInput {
        fn new(bytes: &[u8]) -> Self {
            Self {
                bytes: Cursor::new(bytes.to_vec()),
                dimensions: (24, 100),
                timeouts: Vec::new(),
                fail: false,
            }
        }
    }

    impl KeyInput for MockKeyInput {
        fn read_byte(&mut self, timeout: Option<Duration>) -> io::Result<Option<u8>> {
            self.timeouts.push(timeout);
            if self.fail {
                return Err(io::Error::other("synthetic input failure"));
            }
            let mut byte = [0];
            Ok((self.bytes.read(&mut byte)? != 0).then_some(byte[0]))
        }

        fn dimensions(&self) -> (usize, usize) {
            self.dimensions
        }
    }

    fn choices() -> Vec<Choice> {
        vec![
            Choice {
                number: 3,
                id: "stable-history-b".into(),
                title: "Grüße 🌲".into(),
                url: "https://example.com/b".into(),
                upload_date: Some("2026-11-10".into()),
                created_at: Some("2026-11-10 12:34:56".into()),
            },
            Choice {
                number: 9,
                id: "stable-history-a".into(),
                title: "日本語の字幕".into(),
                url: "https://example.com/a".into(),
                upload_date: None,
                created_at: None,
            },
        ]
    }

    fn navigation_choices() -> Vec<Choice> {
        let mut entries = choices();
        entries.push(Choice {
            number: 51,
            id: "stable-history-c".into(),
            title: "Third transcription".into(),
            url: "https://example.com/c".into(),
            upload_date: None,
            created_at: None,
        });
        entries
    }

    #[test]
    fn renderer_preserves_unicode_dates_and_missing_values() {
        assert_eq!(
            format_choices(&choices()),
            concat!(
                "#\tTITLE\tUPLOAD_DATE\tCREATED_AT\tLINK",
                "\n3\tGrüße 🌲\t2026-11-10\t2026-11-10 12:34:56\thttps://example.com/b",
                "\n9\t日本語の字幕\t—\t—\thttps://example.com/a"
            )
        );
        assert_eq!(format_choices(&[]), "No saved transcriptions.");
    }

    #[test]
    fn renderer_sanitizes_controls_in_every_visible_field() {
        let choice = Choice {
            number: 3,
            id: "hidden-id".into(),
            title: "Title\twith\n\x1b[31mcontrols".into(),
            url: "https://example.com/\rurl".into(),
            upload_date: Some("date\u{009b}value".into()),
            created_at: Some("time\0value".into()),
        };
        assert_eq!(
            format_choices(&[choice]),
            concat!(
                "#\tTITLE\tUPLOAD_DATE\tCREATED_AT\tLINK",
                "\n3\tTitle with  [31mcontrols\tdate value\ttime value\thttps://example.com/ url"
            )
        );
    }

    #[test]
    fn arrows_jk_and_home_end_select_ids_from_the_displayed_snapshot() {
        let cases: &[(&[u8], &str)] = &[
            (b"\r", "stable-history-b"),
            (b"\n", "stable-history-b"),
            (b"\x1b[B\r", "stable-history-a"),
            (b"\x1b[A\r", "stable-history-c"),
            (b"j\r", "stable-history-a"),
            (b"k\r", "stable-history-c"),
            (b"jjj\r", "stable-history-b"),
            (b"kkk\r", "stable-history-b"),
            (b"\x1b[F\r", "stable-history-c"),
            (b"j\x1b[H\r", "stable-history-b"),
            (b"\x1b[4~\r", "stable-history-c"),
            (b"\x1b[8~\r", "stable-history-c"),
            (b"j\x1b[1~\r", "stable-history-b"),
            (b"j\x1b[7~\r", "stable-history-b"),
            (b"\x1bOB\r", "stable-history-a"),
            (b"\x1bOA\r", "stable-history-c"),
            (b"\x1bOF\r", "stable-history-c"),
            (b"j\x1bOH\r", "stable-history-b"),
        ];
        for &(keys, expected) in cases {
            let mut output = Vec::new();
            let selected = choose(
                &navigation_choices(),
                &mut MockKeyInput::new(keys),
                &mut output,
            )
            .unwrap();
            assert_eq!(selected.as_deref(), Some(expected), "{keys:?}");
            assert!(output.starts_with(super::HIDE_CURSOR));
            assert!(output.ends_with(super::SHOW_CURSOR));
        }
    }

    #[test]
    fn unknown_keys_and_numeric_text_do_not_select_a_numbered_row() {
        let mut input = MockKeyInput::new(b"9xyz\x1b[5~\x1b[6~\x1b[99~\x1bOP\x1b[1;5A\r");
        let mut output = Vec::new();
        assert_eq!(
            choose(&choices(), &mut input, &mut output).unwrap(),
            Some("stable-history-b".into())
        );
        assert_eq!(
            String::from_utf8(output)
                .unwrap()
                .matches("Choose a transcription")
                .count(),
            1
        );
    }

    #[test]
    fn escape_suffix_reads_are_timed_and_unknown_sequences_are_bounded() {
        let mut input = MockKeyInput::new(b"\x1b");
        assert_eq!(
            choose(&choices(), &mut input, &mut Vec::new()).unwrap(),
            None
        );
        assert_eq!(input.timeouts, vec![None, Some(ESCAPE_TIMEOUT)]);

        let mut bytes = b"\x1b[".to_vec();
        bytes.extend_from_slice(&[b';'; 40]);
        bytes.push(b'\r');
        let mut input = MockKeyInput::new(&bytes);
        assert_eq!(
            choose(&choices(), &mut input, &mut Vec::new()).unwrap(),
            Some("stable-history-b".into())
        );
        assert_eq!(
            input
                .timeouts
                .iter()
                .filter(|timeout| timeout.is_some())
                .count(),
            super::MAX_ESCAPE_BYTES + 1
        );
    }

    #[test]
    fn cancellation_and_eof_restore_the_cursor_without_selecting() {
        for keys in [
            b"q".as_slice(),
            b"Q",
            b"\x03",
            b"\x04",
            b"\x1b",
            b"",
            b"\x1b[",
        ] {
            let mut output = Vec::new();
            assert_eq!(
                choose(&choices(), &mut MockKeyInput::new(keys), &mut output).unwrap(),
                None
            );
            assert!(output.ends_with(super::SHOW_CURSOR));
        }
    }

    #[test]
    fn viewport_scrolls_centers_and_fits_narrow_unicode_terminals() {
        let entries: Vec<_> = (0..12)
            .map(|index| Choice {
                number: 100 + index,
                id: format!("row-{index}"),
                title: format!("日本語\t🌲\n  transcription {index}"),
                url: String::new(),
                upload_date: None,
                created_at: None,
            })
            .collect();
        let centered = selector_lines(&entries, 6, (8, 100));
        assert!(centered[1].starts_with("  104 "));
        assert!(centered[3].starts_with("> 106 "));
        assert_eq!(centered.last().unwrap(), "7/12");
        let last = selector_lines(&entries, 11, (8, 100));
        assert!(last[1].starts_with("  108 "));
        assert!(last[4].starts_with("> 111 "));
        assert_eq!(last.last().unwrap(), "12/12");

        for dimensions in [(24, 100), (6, 18), (3, 8), (2, 4), (1, 1)] {
            let lines = selector_lines(&entries, 6, dimensions);
            assert!(lines.len() <= dimensions.0);
            for line in lines {
                assert!(line.len() <= dimensions.1.saturating_sub(1));
                assert!(!line.contains('\t'));
                assert!(!line.contains('\n'));
            }
        }
        let mut input = MockKeyInput::new(b"\x1b[F\r");
        input.dimensions = (6, 18);
        let mut output = Vec::new();
        assert_eq!(
            choose(&entries, &mut input, &mut output).unwrap(),
            Some("row-11".into())
        );
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("\x1b[4A"));
        assert!(output.contains("12/12"));
    }

    #[test]
    fn empty_choices_report_no_transcriptions_without_reading_or_hiding_the_cursor() {
        let mut input = MockKeyInput::new(b"j\r");
        let mut output = Vec::new();
        assert_eq!(choose(&[], &mut input, &mut output).unwrap(), None);
        assert_eq!(input.bytes.position(), 0);
        assert!(input.timeouts.is_empty());
        assert_eq!(output.as_slice(), b"No saved transcriptions.\n");
    }

    struct FailOnceWriter {
        bytes: Vec<u8>,
        writes: usize,
        fail_write: Option<usize>,
        fail_flush: bool,
    }

    impl Write for FailOnceWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            if self.fail_write == Some(self.writes) {
                return Err(io::Error::other("synthetic output failure"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.fail_flush {
                self.fail_flush = false;
                return Err(io::Error::other("synthetic flush failure"));
            }
            Ok(())
        }
    }

    #[test]
    fn input_and_output_errors_attempt_to_restore_the_cursor() {
        let mut input = MockKeyInput::new(b"j\r");
        input.fail = true;
        let mut output = Vec::new();
        assert!(choose(&choices(), &mut input, &mut output)
            .unwrap_err()
            .contains("synthetic input failure"));
        assert!(output.ends_with(super::SHOW_CURSOR));

        for (fail_write, fail_flush) in [(Some(1), false), (Some(2), false), (None, true)] {
            let mut output = FailOnceWriter {
                bytes: Vec::new(),
                writes: 0,
                fail_write,
                fail_flush,
            };
            assert!(choose(&choices(), &mut MockKeyInput::new(b"\r"), &mut output).is_err());
            assert!(output.bytes.ends_with(super::SHOW_CURSOR));
        }
    }

    #[test]
    fn unwinding_during_input_restores_the_cursor() {
        struct PanickingInput;
        impl KeyInput for PanickingInput {
            fn read_byte(&mut self, _timeout: Option<Duration>) -> io::Result<Option<u8>> {
                panic!("synthetic input panic");
            }
        }
        let mut output = Vec::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = choose(&choices(), &mut PanickingInput, &mut output);
        }));
        assert!(result.is_err());
        assert!(output.ends_with(super::SHOW_CURSOR));
    }
}
