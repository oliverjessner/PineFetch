//! Interactive selection from a fixed snapshot of saved transcriptions.
use std::io::{BufRead, Write};

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Choice {
    pub number: u32,
    pub id: String,
    pub title: String,
    pub url: String,
    pub upload_date: Option<String>,
    pub created_at: Option<String>,
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

pub(crate) fn choose<R: BufRead, W: Write>(
    choices: &[Choice],
    input: &mut R,
    output: &mut W,
) -> Result<Option<String>, String> {
    writeln!(output, "{}", format_choices(choices))
        .map_err(|error| format!("Could not display transcription choices: {error}"))?;
    if choices.is_empty() {
        output
            .flush()
            .map_err(|error| format!("Could not display transcription choices: {error}"))?;
        return Ok(None);
    }

    let mut line = String::new();
    loop {
        write!(
            output,
            "Choose a transcription by its history number, or q to cancel: "
        )
        .and_then(|()| output.flush())
        .map_err(|error| format!("Could not display transcription prompt: {error}"))?;

        line.clear();
        if input
            .read_line(&mut line)
            .map_err(|error| format!("Could not read transcription selection: {error}"))?
            == 0
        {
            return Ok(None);
        }
        let value = line.trim();
        if ["q", "quit", "cancel"]
            .iter()
            .any(|command| value.eq_ignore_ascii_case(command))
        {
            return Ok(None);
        }
        if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
            if let Some(choice) = value
                .parse::<u32>()
                .ok()
                .and_then(|number| choices.iter().find(|choice| choice.number == number))
            {
                return Ok(Some(choice.id.clone()));
            }
        }
        writeln!(
            output,
            "Enter a history number from the list, or q to cancel."
        )
        .map_err(|error| format!("Could not display transcription prompt: {error}"))?;
    }
}

#[cfg(test)]
mod tests {
    use super::{choose, format_choices, Choice};
    use std::io::Cursor;

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
    fn selection_returns_the_id_from_the_displayed_snapshot() {
        let mut input = Cursor::new(b" 9 \r\n");
        let mut output = Vec::new();
        assert_eq!(
            choose(&choices(), &mut input, &mut output).unwrap(),
            Some("stable-history-a".into())
        );
        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with(&format_choices(&choices())));
        assert!(output.ends_with("Choose a transcription by its history number, or q to cancel: "));
    }

    #[test]
    fn invalid_selections_reprompt_until_a_valid_snapshot_number_is_entered() {
        let mut input = Cursor::new(b"0\n10\n-1\n+3\nhello\n\n9999999999999999999999999999\n3\n");
        let mut output = Vec::new();
        assert_eq!(
            choose(&choices(), &mut input, &mut output).unwrap(),
            Some("stable-history-b".into())
        );
        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output
                .matches("Enter a history number from the list, or q to cancel.")
                .count(),
            7
        );
        assert_eq!(
            output
                .matches("Choose a transcription by its history number")
                .count(),
            8
        );
        assert_eq!(output.matches("#\tTITLE").count(), 1);
    }

    #[test]
    fn cancellation_and_eof_never_select_an_entry() {
        for text in ["q\n", "QuIt\n", " CANCEL \r\n", "", "invalid\n"] {
            let mut input = Cursor::new(text.as_bytes());
            let mut output = Vec::new();
            assert_eq!(choose(&choices(), &mut input, &mut output).unwrap(), None);
        }
    }

    #[test]
    fn empty_choices_report_no_transcriptions_without_reading_or_prompting() {
        let mut input = Cursor::new(b"1\n");
        let mut output = Vec::new();
        assert_eq!(choose(&[], &mut input, &mut output).unwrap(), None);
        assert_eq!(input.position(), 0);
        assert_eq!(output.as_slice(), b"No saved transcriptions.\n");
    }
}
