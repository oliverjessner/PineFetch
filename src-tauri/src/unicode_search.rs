//! Shared Unicode 17 full folding data; canonical normalization keeps accents.
use icu_normalizer::{ComposingNormalizer, DecomposingNormalizer};
use std::collections::HashMap;
use std::sync::LazyLock;

const DATA: &str = include_str!("../../vendor/unicode/17.0.0/CaseFolding.txt");
static FOLDINGS: LazyLock<HashMap<char, String>> = LazyLock::new(|| {
    DATA.lines()
        .filter_map(|line| {
            let mut fields = line.split('#').next()?.split(';').map(str::trim);
            let code = fields.next()?;
            if !matches!(fields.next()?, "C" | "F") {
                return None;
            }
            let mapping = fields.next()?;
            let character =
                char::from_u32(u32::from_str_radix(code, 16).expect("bundled Unicode scalar"))
                    .expect("bundled Unicode scalar");
            let mapping = mapping
                .split_whitespace()
                .map(|value| {
                    char::from_u32(u32::from_str_radix(value, 16).expect("bundled Unicode scalar"))
                        .expect("bundled Unicode scalar")
                })
                .collect();
            Some((character, mapping))
        })
        .collect()
});

pub(crate) fn key(text: &str) -> String {
    if text.is_ascii() {
        return text.to_ascii_lowercase();
    }
    let decomposed = DecomposingNormalizer::new_nfd().normalize(text);
    let mut folded = String::with_capacity(decomposed.len());
    for character in decomposed.chars() {
        if let Some(mapping) = FOLDINGS.get(&character) {
            folded.push_str(mapping);
        } else {
            folded.push(character);
        }
    }
    ComposingNormalizer::new_nfc()
        .normalize(&folded)
        .into_owned()
}

pub(crate) fn trim_query(text: &str) -> &str {
    text.trim_matches(|character: char| character.is_whitespace() || character == '\u{feff}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{self, Database};
    use crate::history::search_history_page_from_db;
    use crate::history_query::{read_page, HistoryFilter};
    use rusqlite::{params, Connection};
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Case {
        text: Option<String>,
        query: String,
        #[serde(rename = "match")]
        matches: bool,
    }
    fn cases() -> Vec<Case> {
        serde_json::from_str(include_str!("../../test/fixtures/unicode-search.json")).unwrap()
    }
    fn database() -> Database {
        let connection = Connection::open_in_memory().unwrap();
        database::initialize(&connection).unwrap();
        Database::new(connection)
    }
    fn archive() -> Database {
        let db = database();
        {
            let conn = db.lock().unwrap();
            for (index, case) in cases().iter().enumerate() {
                let id = format!("case-{index:02}");
                conn.execute("INSERT INTO history_entries(id,url,title,uploader,source,created_at) VALUES(?1,'https://example.test/video',?2,?2,'youtube',?3)", params![id, case.text, index as i64]).unwrap();
                if let Some(text) = &case.text {
                    conn.execute("INSERT INTO captions(history_entry_id,media_path,text,created_at) VALUES(?1,'/fixture/video.mp4',?2,0)", params![id, text]).unwrap();
                    conn.execute("INSERT INTO transcriptions(id,history_entry_id,text,\"type\") VALUES(?1,?1,?2,'text')", params![id, text]).unwrap();
                }
            }
        }
        db
    }

    #[test]
    fn full_folding_conforms_to_all_bundled_unicode_mappings() {
        assert!(DATA.starts_with("# CaseFolding-17.0.0.txt"));
        for (&character, mapping) in FOLDINGS.iter() {
            assert_eq!(key(&character.to_string()), key(mapping), "{character}");
            assert_eq!(
                key(&key(&character.to_string())),
                key(&character.to_string()),
                "{character}"
            );
        }
        assert_eq!(key("Straße O\u{308}STERREICH"), "strasse österreich");
    }

    #[test]
    fn sqlite_uses_shared_search_cases_and_rebinding_cannot_reuse_a_previous_query() {
        let db = database();
        let conn = db.lock().unwrap();
        let mut statement = conn.prepare("SELECT pinefetch_contains(?1,?2)").unwrap();
        for case in cases() {
            let found: bool = statement
                .query_row(params![case.text, case.query], |row| row.get(0))
                .unwrap();
            assert_eq!(found, case.matches, "{:?}: {}", case.text, case.query);
        }
    }

    #[test]
    fn history_fields_share_unicode_rules_and_preserve_saved_text_and_exact_creator_identity() {
        let db = archive();
        for field in ["title", "user", "description", "transcript"] {
            for (index, case) in cases().iter().enumerate() {
                let page = search_history_page_from_db(
                    &db,
                    100,
                    0,
                    Some(&case.query),
                    Some(field),
                    None,
                    false,
                )
                .unwrap();
                assert_eq!(
                    page.entries
                        .iter()
                        .any(|entry| entry.id == format!("case-{index:02}")),
                    case.matches,
                    "{field}: {}",
                    case.query
                );
            }
        }
        let exact = search_history_page_from_db(
            &db,
            100,
            0,
            Some("Österreich"),
            Some("user"),
            Some("youtube"),
            true,
        )
        .unwrap();
        assert_eq!(
            exact
                .entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["case-08", "case-00"]
        );
        let exact = search_history_page_from_db(
            &db,
            100,
            0,
            Some("O\u{308}sterreich"),
            Some("user"),
            Some("youtube"),
            true,
        )
        .unwrap();
        assert_eq!(exact.entries.len(), 1);
        assert_eq!(exact.entries[0].id, "case-01");
        let raw: String = db
            .lock()
            .unwrap()
            .query_row(
                "SELECT title FROM history_entries WHERE id='case-01'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(raw, "O\u{308}sterreich");
    }

    #[test]
    fn equivalent_search_queries_continue_the_same_ordered_cursor_pages() {
        let db = archive();
        let filter =
            |query| HistoryFilter::new(Some(query), Some("title"), Some("youtube"), false).unwrap();
        let first = read_page(&db, 2, 0, filter("österreich"), None).unwrap();
        let second = read_page(
            &db,
            2,
            0,
            filter("O\u{308}STERREICH"),
            first.next_cursor.as_deref(),
        )
        .unwrap();
        let last = read_page(
            &db,
            2,
            0,
            filter("ÖSTERREICH"),
            second.next_cursor.as_deref(),
        )
        .unwrap();
        let ids = first
            .entries
            .into_iter()
            .chain(second.entries)
            .chain(last.entries)
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, ["case-24", "case-08", "case-02", "case-01", "case-00"]);
        assert!(!last.has_more && last.next_cursor.is_none());
    }

    #[test]
    fn ocr_filters_unicode_before_counting_and_pagination() {
        let db = archive();
        {
            let conn = db.lock().unwrap();
            conn.execute("INSERT INTO history_ocr(history_entry_id,last_attempt_id,last_attempt_status,last_attempt_at,result_format,result_revision) VALUES('case-00','fixture','completed',0,'frames','unicode')", []).unwrap();
            for (index, case) in cases().iter().enumerate() {
                if let Some(text) = case.text.as_deref().filter(|text| !text.is_empty()) {
                    conn.execute("INSERT INTO history_ocr_frames(id,history_entry_id,timestamp_ms,full_text,blocks_json) VALUES(?1,'case-00',?2,?3,'[]')", params![format!("frame-{index:02}"), index as i64, text]).unwrap();
                }
            }
        }
        let first = crate::ocr_store::page(&db, "case-00", 2, 0, "ÖSTERREICH", Some("unicode"))
            .unwrap()
            .unwrap();
        let second =
            crate::ocr_store::page(&db, "case-00", 2, 2, "O\u{308}sterreich", Some("unicode"))
                .unwrap()
                .unwrap();
        assert_eq!((first.matched_count, second.matched_count), (5, 5));
        assert_eq!(
            first
                .frames
                .iter()
                .map(|frame| frame.id.as_str())
                .collect::<Vec<_>>(),
            ["frame-00", "frame-01"]
        );
        assert_eq!(
            second
                .frames
                .iter()
                .map(|frame| frame.id.as_str())
                .collect::<Vec<_>>(),
            ["frame-02", "frame-08"]
        );
        assert!(first.has_more && second.has_more);
    }
}
