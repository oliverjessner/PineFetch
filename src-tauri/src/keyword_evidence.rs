//! Count and locate the same Unicode tokens in immutable, local text snapshots.
use crate::database::Database;
use crate::keywords::{self, TextSource, MAX_TEXT_BYTES};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

static WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{L}\p{N}][\p{L}\p{N}\p{M}]*").unwrap());
static CLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\[)(\d+(?::\d{2}){1,2}(?:[.,]\d+)?)").unwrap());
static MARKUP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<[^>]*>|\{[^}]*\}|&[^;\s]+;|\\[Nnh]").unwrap());
static JSON_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""utf8"\s*:\s*"((?:[^"\\]|\\.)*)""#).unwrap());

pub(crate) struct Token {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) time: Option<u64>,
}
fn milliseconds(clock: &str) -> Option<u64> {
    let mut seconds = 0.;
    let normalized = clock.replace(',', ".");
    for part in normalized.split(':') {
        seconds = seconds * 60. + part.parse::<f64>().ok()?;
    }
    (seconds.is_finite() && seconds >= 0. && seconds < u64::MAX as f64 / 1000.)
        .then(|| (seconds * 1000.).round() as u64)
}
fn visit_plain(
    text: &str,
    base: usize,
    time: Option<u64>,
    markup: bool,
    visit: &mut impl FnMut(Token) -> Result<(), String>,
) -> Result<(), String> {
    let mut position = 0;
    // Exclude formatting instructions, preserving original byte positions.
    let mut part = |start: usize, end: usize| {
        for word in WORD.find_iter(&text[start..end]) {
            visit(Token {
                start: base + start + word.start(),
                end: base + start + word.end(),
                time,
            })?;
        }
        Ok::<_, String>(())
    };
    if markup {
        for ignored in MARKUP.find_iter(text) {
            part(position, ignored.start())?;
            position = ignored.end();
        }
    }
    part(position, text.len())
}
pub(crate) fn visit_tokens(
    text: &str,
    format: Option<&str>,
    fixed_time: Option<u64>,
    mut visit: impl FnMut(Token) -> Result<(), String>,
) -> Result<(), String> {
    if format == Some("json3") {
        for captures in JSON_TEXT.captures_iter(text) {
            let value = captures.get(1).unwrap();
            visit_plain(value.as_str(), value.start(), None, true, &mut visit)?;
        }
        return Ok(());
    }
    let cue_format = matches!(format, Some("srt" | "vtt"));
    let ass = matches!(format, Some("ass" | "ssa"));
    let mut time = fixed_time;
    let mut active = !cue_format;
    let mut base = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let clock = CLOCK.captures(trimmed);
        let timed_line =
            (trimmed.starts_with('[') && trimmed.contains(']')) || trimmed.contains("-->");
        if timed_line && fixed_time.is_none() && format != Some("plain") {
            if let Some(clock) = &clock {
                time = milliseconds(&clock[1]);
            }
        }
        if format != Some("plain")
            && trimmed.contains("-->")
            && !trimmed.starts_with('[')
            && clock.is_some()
        {
            active = true;
        } else if cue_format && trimmed.is_empty() {
            active = false;
        } else if ass {
            if let Some(dialogue) = line.strip_prefix("Dialogue:") {
                let fields: Vec<_> = dialogue.splitn(10, ',').collect();
                if fields.len() == 10 {
                    let speech = fields[9];
                    visit_plain(
                        speech,
                        base + line.len() - speech.len(),
                        milliseconds(fields[1].trim()),
                        true,
                        &mut visit,
                    )?;
                }
            }
        } else if active {
            let start = if format != Some("plain") && clock.is_some() && trimmed.starts_with('[') {
                line.find(']').map(|i| i + 1).unwrap_or(0)
            } else {
                0
            };
            visit_plain(
                &line[start..],
                base + start,
                time,
                format.is_some_and(|format| !matches!(format, "plain" | "transcript" | "txt")),
                &mut visit,
            )?;
        }
        base += line.len();
    }
    Ok(())
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OccurrenceQuery {
    pub(crate) term: String,
    pub(crate) source: Option<String>,
    pub(crate) language: Option<String>,
    pub(crate) offset: u32,
}
#[derive(Debug, Serialize)]
pub(crate) struct Occurrence {
    pub(crate) source: &'static str,
    pub(crate) source_key: String,
    pub(crate) revision: String,
    pub(crate) language: Option<String>,
    pub(crate) timestamp_ms: Option<u64>,
    pub(crate) source_start_seconds: f64,
    pub(crate) offset_utf16: usize,
    pub(crate) end_utf16: usize,
    pub(crate) snippet: String,
}
#[derive(Debug, Serialize)]
pub(crate) struct OccurrencePage {
    pub(crate) entries: Vec<Occurrence>,
    pub(crate) total: u64,
    pub(crate) next_offset: u32,
    pub(crate) has_more: bool,
}
fn source_name(kind: usize) -> &'static str {
    ["transcript", "ocr", "captions", "subtitles"][kind]
}
fn matches_source(source: &TextSource, query: &OccurrenceQuery) -> bool {
    query
        .source
        .as_deref()
        .is_none_or(|kind| source_name(source.kind) == kind)
        && query
            .language
            .as_deref()
            .is_none_or(|language| source.language.as_deref() == Some(language))
}
fn snapshot(db: &Database, id: &str) -> Result<keywords::TextSnapshot, String> {
    db.read_snapshot(|conn| keywords::read_texts(conn, id, MAX_TEXT_BYTES))
}
pub(crate) fn occurrences(
    db: &Database,
    id: &str,
    query: OccurrenceQuery,
) -> Result<OccurrencePage, String> {
    let key = crate::unicode_search::key(query.term.trim());
    if !keywords::eligible_word(&key) || WORD.find(&key).is_none_or(|word| word.as_str() != key) {
        return Err("Choose one complete keyword.".into());
    }
    if query
        .source
        .as_deref()
        .is_some_and(|source| !["transcript", "ocr", "captions", "subtitles"].contains(&source))
        || query.language.as_ref().is_some_and(|lang| lang.len() > 64)
    {
        return Err("Invalid keyword occurrence filter.".into());
    }
    db.analyze(|| {
        let snapshot = snapshot(db, id)?;
        let mut entries = Vec::new();
        let mut total = 0u64;
        for source in snapshot
            .texts
            .iter()
            .filter(|source| matches_source(source, &query))
        {
            let source_key = source.key();
            let revision = source.revision();
            let mut previous_byte = 0;
            let mut previous_utf16 = 0;
            visit_tokens(
                &source.text,
                if matches!(source.kind, 1 | 2) {
                    Some("plain")
                } else {
                    source.format.as_deref()
                },
                source.timestamp_ms,
                |token| {
                    if crate::unicode_search::key(&source.text[token.start..token.end]) != key {
                        return Ok(());
                    }
                    total += 1;
                    if total <= u64::from(query.offset) || entries.len() >= 100 {
                        return Ok(());
                    }
                    previous_utf16 += source.text[previous_byte..token.start]
                        .encode_utf16()
                        .count();
                    previous_byte = token.start;
                    let start = source.text[..token.start]
                        .char_indices()
                        .rev()
                        .nth(60)
                        .map(|(index, _)| index)
                        .unwrap_or(0);
                    let end = source.text[token.end..]
                        .char_indices()
                        .nth(60)
                        .map(|(index, _)| token.end + index)
                        .unwrap_or(source.text.len());
                    entries.push(Occurrence {
                        source: source_name(source.kind),
                        source_key: source_key.clone(),
                        revision: revision.clone(),
                        language: source.language.clone(),
                        timestamp_ms: token.time,
                        source_start_seconds: source.source_start_seconds,
                        offset_utf16: previous_utf16,
                        end_utf16: previous_utf16
                            + source.text[token.start..token.end].encode_utf16().count(),
                        snippet: source.text[start..end].into(),
                    });
                    Ok(())
                },
            )?;
        }
        let next_offset = query.offset.saturating_add(entries.len() as u32);
        Ok(OccurrencePage {
            entries,
            total,
            next_offset,
            has_more: u64::from(next_offset) < total,
        })
    })
}
pub(crate) fn source(
    db: &Database,
    id: &str,
    key: &str,
    revision: &str,
) -> Result<TextSource, String> {
    if key.len() != 64 || revision.len() != 64 {
        return Err("Invalid keyword source.".into());
    }
    db.analyze(|| {
        let mut source = snapshot(db, id)?
            .texts
            .into_iter()
            .find(|source| source.key() == key)
            .ok_or("The saved source changed. Refresh keywords and choose the occurrence again.")?;
        if source.revision() != revision {
            return Err(
                "The saved text changed. Refresh keywords and choose the occurrence again.".into(),
            );
        }
        source.source_key = key.into();
        Ok(source)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};
    fn query(source: Option<&str>, language: Option<&str>, offset: u32) -> OccurrenceQuery {
        OccurrenceQuery {
            term: "STRASSE".into(),
            source: source.map(Into::into),
            language: language.map(Into::into),
            offset,
        }
    }
    fn db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        crate::database::initialize(&conn).unwrap();
        conn.execute_batch("INSERT INTO history_entries(id,url,created_at) VALUES('video','https://example.test',1),('other','https://example.test/other',2); INSERT INTO transcriptions(history_entry_id,text,type) VALUES('video','[00:00:02.500] Straße Straße','text with timestamps'); INSERT INTO history_ocr(history_entry_id,text,last_attempt_id,last_attempt_status,last_attempt_at) VALUES('video','[00:03:00] Straße','fixture','completed',1);").unwrap();
        conn.execute(
            "INSERT INTO history_subtitles VALUES('video','missing.de.srt','de','srt',?1,8.5)",
            ["1\r\n00:00:10,500 --> 00:00:12,000 align:start\r\n🌲 Straße Straße Straße\r\n"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO history_subtitles VALUES('video','missing.en.vtt','en','vtt',?1,0)",
            ["WEBVTT\n\n00:01.000 --> 00:02.000\nSTRASSE STRASSE\n"],
        )
        .unwrap();
        Database::new(conn)
    }
    #[test]
    fn counts_languages_and_occurrences_use_identical_unicode_tokens_and_real_offsets() {
        let db = db();
        let mined = keywords::mine(&db, "video").unwrap();
        let term = mined
            .terms
            .iter()
            .find(|term| crate::unicode_search::key(&term.term) == "strasse")
            .unwrap();
        assert_eq!(
            (term.total, term.transcript, term.ocr, term.subtitles),
            (8, 2, 1, 5)
        );
        assert_eq!(term.subtitle_languages["de"], 3);
        assert_eq!(term.subtitle_languages["en"], 2);
        let all = occurrences(&db, "video", query(None, None, 0)).unwrap();
        assert_eq!(all.total, term.total);
        assert!(all
            .entries
            .iter()
            .find(|item| item.source == "ocr")
            .unwrap()
            .timestamp_ms
            .is_none());
        assert_eq!(all.entries[0].timestamp_ms, Some(2500));
        let german = occurrences(&db, "video", query(Some("subtitles"), Some("de"), 0)).unwrap();
        assert_eq!(german.total, 3);
        assert!(
            german
                .entries
                .iter()
                .all(|item| item.language.as_deref() == Some("de")
                    && item.timestamp_ms == Some(10500))
        );
        let item = &german.entries[0];
        let text = source(&db, "video", &item.source_key, &item.revision).unwrap();
        assert_eq!(text.source_start_seconds, 8.5);
        let units: Vec<_> = text.text.encode_utf16().collect();
        assert_eq!(
            String::from_utf16(&units[item.offset_utf16..item.end_utf16]).unwrap(),
            "Straße"
        );
        assert!(source(&db, "other", &item.source_key, &item.revision).is_err());
        db.lock()
            .unwrap()
            .execute(
                "UPDATE history_subtitles SET text='changed' WHERE language='de'",
                [],
            )
            .unwrap();
        assert!(source(&db, "video", &item.source_key, &item.revision)
            .unwrap_err()
            .contains("changed"));
        assert!(occurrences(&db, "missing", query(None, None, 0)).is_err());
    }
    #[test]
    fn every_saved_frame_is_pageable_with_fixed_ocr_time_and_stable_counts() {
        let db = db();
        for i in 0..251 {
            db.lock().unwrap().execute("INSERT INTO history_ocr_frames(id,history_entry_id,timestamp_ms,full_text,blocks_json) VALUES(?1,'video',?2,'[00:59:59] Straße','[]')",params![format!("frame-{i:03}"),i*1000]).unwrap();
        }
        let mut offset = 0;
        let mut entries = Vec::new();
        loop {
            let page = occurrences(&db, "video", query(Some("ocr"), None, offset)).unwrap();
            assert_eq!(page.total, 252);
            assert!(page.entries.len() <= 100);
            offset = page.next_offset;
            entries.extend(page.entries);
            if !page.has_more {
                break;
            }
        }
        assert_eq!(entries.len(), 252);
        assert_eq!(entries.last().unwrap().timestamp_ms, Some(250000));
    }
    #[test]
    fn literal_clock_text_never_invents_an_ocr_caption_or_untimed_transcript_timestamp() {
        for format in [None, Some("plain")] {
            let mut found = Vec::new();
            visit_tokens("12:30 Straße", format, None, |token| {
                if &"12:30 Straße"[token.start..token.end] == "Straße" {
                    found.push(token.time);
                }
                Ok(())
            })
            .unwrap();
            assert_eq!(found, [None]);
        }
        let mut words = Vec::new();
        visit_tokens(
            "[12:30 annotation] Straße",
            Some("plain"),
            Some(500),
            |token| {
                words.push((
                    &"[12:30 annotation] Straße"[token.start..token.end],
                    token.time,
                ));
                Ok(())
            },
        )
        .unwrap();
        assert!(words.contains(&("annotation", Some(500))));
    }
    #[test]
    fn formatting_and_cue_identifiers_are_excluded_without_changing_source_positions() {
        for (format,text) in [
            ("srt","12\n00:00:01,000 --> 00:00:02,000 align:start\n<span class=\"hidden\">Visible</span>\n"),
            ("ass","[Events]\nFormat: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\i1}Visible{\\i0}"),
            ("ttml","<tt><body><p begin=\"1s\" hidden=\"metadata\">Visible</p></body></tt>"),
            ("json3",r#"{"events":[{"segs":[{"utf8":"Visible"}],"metadata":"hidden"}]}"#),
        ] {
            let mut words=Vec::new();
            visit_tokens(text,Some(format),None,|token|{words.push(&text[token.start..token.end]);Ok(())}).unwrap();
            assert_eq!(words,["Visible"],"{format}");
        }
    }
}
