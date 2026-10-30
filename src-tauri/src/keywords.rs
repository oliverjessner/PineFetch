//! Deterministic, local term counts over every saved text source for one video.
use crate::database::Database;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

pub(crate) const MAX_TEXT_BYTES: usize = 128 * 1024 * 1024;
const MAX_TERMS: usize = 50_000;
const MIN_OCCURRENCES: u64 = 5;
static STOP_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "aber alle allem allen aller alles als also am an andere anderen anderer anderes auch auf aus bei bin bis bist da dabei dadurch dafür daher damit dann das dass daß dein deine dem den denn der des die dies diese diesem diesen dieser dieses doch dort du durch ein eine einem einen einer eines er es etwas euch euer eure für ganz gegen gehabt haben hat hatte hatten hast hier hin hinter ich ihr ihre ihrem ihren ihrer ihres im in ins ist ja jede jedem jeden jeder jedes jetzt kann kein keine keinem keinen keiner keines können konnte machen man mehr mein meine mich mir mit muss nach nicht nichts noch nun nur ob oder ohne sehr sein seine seinem seinen seiner seines selbst sich sie sind so solche sollte sondern sonst über um und uns unser unsere unter vom von vor war waren warst was welche welchem welchen welcher welches wenn werde werden werdet weshalb wie wieder wir wird wirst wo zu zum zur zwischen the a an and are as at be been being but by can could did do does for from had has have he her here hers him his how if in into is it its just may me more most my no nor not of on once only or other our out over own same she should some such than that their theirs them then there these they this those through to too under until up very was we were what when where which while who whom why will with would you your yours about also because both each few much must now off ours shall since still themselves upon us want well were all any don dont doesnt isnt wasnt wouldnt couldve ive youre thats its let lets webvtt kind language captions www com http https".split_whitespace().collect()
});

#[derive(Debug, Serialize)]
pub(crate) struct KeywordTerm {
    pub(crate) term: String,
    pub(crate) transcript: u64,
    pub(crate) ocr: u64,
    pub(crate) captions: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub(crate) subtitles: u64,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) subtitle_languages: BTreeMap<String, u64>,
    pub(crate) total: u64,
    pub(crate) sources: u8,
}
#[derive(Debug, Default, Serialize)]
pub(crate) struct KeywordSources {
    pub(crate) transcript: bool,
    pub(crate) ocr_frames: u64,
    pub(crate) ocr_legacy: bool,
    pub(crate) captions: u64,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) subtitle_languages: BTreeMap<String, u64>,
}
fn is_zero(value: &u64) -> bool {
    *value == 0
}
#[derive(Debug, Serialize)]
pub(crate) struct HistoryKeywords {
    pub(crate) history_entry_id: String,
    pub(crate) sources: KeywordSources,
    pub(crate) terms: Vec<KeywordTerm>,
}

#[derive(Default)]
struct Miner {
    terms: HashMap<String, KeywordTerm>,
    bytes: usize,
}
impl Miner {
    #[cfg(test)]
    fn add(&mut self, text: &str, source: usize) -> Result<(), String> {
        self.add_text(text, source, None, None)
    }
    fn add_text(
        &mut self,
        text: &str,
        source: usize,
        language: Option<&str>,
        format: Option<&str>,
    ) -> Result<(), String> {
        #[cfg(test)]
        crate::db_contention_tests::analysis_started();
        self.bytes = self.bytes.saturating_add(text.len());
        if self.bytes > MAX_TEXT_BYTES {
            return Err("The saved texts exceed the 128 MiB keyword mining limit.".into());
        }
        let format = if matches!(source, 1 | 2) {
            Some("plain")
        } else {
            format
        };
        super::keyword_evidence::visit_tokens(text, format, None, |token| {
            let word = &text[token.start..token.end];
            let key = crate::unicode_search::key(word);
            if !eligible_word(&key) {
                return Ok(());
            }
            if !self.terms.contains_key(&key) && self.terms.len() >= MAX_TERMS {
                return Err("The saved texts exceed the 50,000 distinct keyword limit.".into());
            }
            let term = self.terms.entry(key).or_insert_with(|| KeywordTerm {
                term: word.into(),
                subtitles: 0,
                subtitle_languages: BTreeMap::new(),
                transcript: 0,
                ocr: 0,
                captions: 0,
                total: 0,
                sources: 0,
            });
            match source {
                0 => term.transcript += 1,
                1 => term.ocr += 1,
                2 => term.captions += 1,
                _ => {
                    term.subtitles += 1;
                    *term
                        .subtitle_languages
                        .entry(language.unwrap_or("und").into())
                        .or_default() += 1;
                }
            }
            term.total += 1;
            term.sources = u8::from(term.transcript > 0)
                + u8::from(term.ocr > 0)
                + u8::from(term.captions > 0)
                + u8::from(term.subtitles > 0);
            Ok(())
        })?;
        Ok(())
    }
    fn finish(self) -> Vec<KeywordTerm> {
        let mut terms: Vec<_> = self
            .terms
            .into_iter()
            .filter(|(_, term)| term.total >= MIN_OCCURRENCES)
            .collect();
        terms.sort_by(|(ak, a), (bk, b)| b.total.cmp(&a.total).then(ak.cmp(bk)));
        terms.into_iter().map(|(_, term)| term).collect()
    }
}

pub(crate) fn eligible_word(key: &str) -> bool {
    (2..=64).contains(&key.chars().count())
        && key.chars().any(char::is_alphabetic)
        && !STOP_WORDS.contains(key)
}
#[derive(Debug, Serialize)]
pub(crate) struct TextSource {
    pub(crate) kind: usize,
    pub(crate) source_key: String,
    pub(crate) language: Option<String>,
    pub(crate) format: Option<String>,
    pub(crate) source_start_seconds: f64,
    pub(crate) timestamp_ms: Option<u64>,
    pub(crate) text: String,
}
impl TextSource {
    pub(crate) fn key(&self) -> String {
        crate::hashing::sha256_hex(format!("{}\0{}", self.kind, self.source_key).as_bytes())
    }
    pub(crate) fn revision(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(
            format!(
                "{:?}\0{:?}\0{}\0{:?}\0",
                self.language, self.format, self.source_start_seconds, self.timestamp_ms
            )
            .as_bytes(),
        );
        hash.update(self.text.as_bytes());
        format!("{:x}", hash.finalize())
    }
}
pub(crate) struct TextSnapshot {
    sources: KeywordSources,
    pub(crate) texts: Vec<TextSource>,
}
pub(crate) fn read_texts(
    conn: &rusqlite::Connection,
    id: &str,
    byte_limit: usize,
) -> Result<TextSnapshot, String> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM history_entries WHERE id=?1)",
            [id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err("This history entry no longer exists.".into());
    }
    let mut snapshot = TextSnapshot {
        sources: KeywordSources::default(),
        texts: Vec::new(),
    };
    let mut bytes = 0usize;
    for (kind, sql) in [
        (0,"SELECT text,CAST(rowid AS TEXT),NULL,NULL,0,NULL FROM transcriptions WHERE history_entry_id=?1 ORDER BY rowid"),
        (1,"SELECT text,'legacy',NULL,NULL,0,NULL FROM history_ocr WHERE history_entry_id=?1 AND text IS NOT NULL"),
        (1,"SELECT full_text,id,NULL,NULL,0,timestamp_ms FROM history_ocr_frames WHERE history_entry_id=?1 ORDER BY timestamp_ms,id"),
        (2,"SELECT text,media_path,NULL,NULL,0,NULL FROM captions WHERE history_entry_id=?1 ORDER BY created_at,media_path"),
        (3,"SELECT text,path,language,format,source_start_seconds,NULL FROM history_subtitles WHERE history_entry_id=?1 ORDER BY language,path"),
    ] {
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let text = row.get_ref(0).map_err(|e| e.to_string())?.as_str().map_err(|e| e.to_string())?;
            if text.trim().is_empty() { continue; }
            bytes = bytes.saturating_add(text.len());
            if bytes > byte_limit { return Err("The saved texts exceed the 128 MiB keyword mining limit.".into()); }
            let timestamp_ms: Option<u64> = row.get(5).map_err(|e| e.to_string())?;
            let language: Option<String> = row.get(2).map_err(|e| e.to_string())?;
            match kind {
                0 => snapshot.sources.transcript = true,
                1 if timestamp_ms.is_some() => snapshot.sources.ocr_frames += 1,
                1 => snapshot.sources.ocr_legacy = true,
                2 => snapshot.sources.captions += 1,
                _ => *snapshot.sources.subtitle_languages.entry(language.clone().unwrap_or_else(|| "und".into())).or_default() += 1,
            }
            snapshot.texts.push(TextSource { kind, source_key: row.get(1).map_err(|e| e.to_string())?, language, format: row.get(3).map_err(|e| e.to_string())?, source_start_seconds: row.get(4).map_err(|e| e.to_string())?, timestamp_ms, text: text.to_owned() });
        }
    }
    Ok(snapshot)
}

pub(crate) fn mine(db: &Database, id: &str) -> Result<HistoryKeywords, String> {
    db.analyze(|| {
        let snapshot = db.read_snapshot(|conn| read_texts(conn, id, MAX_TEXT_BYTES))?;
        let mut miner = Miner::default();
        for source in snapshot.texts {
            miner.add_text(
                &source.text,
                source.kind,
                source.language.as_deref(),
                source.format.as_deref(),
            )?;
        }
        Ok(HistoryKeywords {
            history_entry_id: id.into(),
            sources: snapshot.sources,
            terms: miner.finish(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};
    #[test]
    fn terms_match_across_sources_preserve_names_and_exclude_stop_words_numbers_and_timestamps() {
        let mut miner = Miner::default();
        miner
            .add(
                "[00:00:10 → 00:00:15] Der Nintendo DSi: Nintendo Nintendo, DSi DSi DSi Pokémon und AI AI AI AI AI in 4K 4K 4K 4K 4K!",
                0,
            )
            .unwrap();
        miner.add("NINTENDO\nDSi Pokémon", 1).unwrap();
        miner.add("Nintendo Pokémon Pokémon Pokémon.", 2).unwrap();
        let terms = miner.finish();
        let term = terms.iter().find(|t| t.term == "Nintendo").unwrap();
        assert_eq!(
            (
                term.transcript,
                term.ocr,
                term.captions,
                term.total,
                term.sources
            ),
            (3, 1, 1, 5, 3)
        );
        assert!(terms.iter().any(|t| t.term == "AI"));
        assert!(terms.iter().any(|t| t.term == "4K"));
        assert!(!terms
            .iter()
            .any(|t| ["Der", "und", "in", "00", "10", "15"].contains(&t.term.as_str())));
        assert!(terms.iter().all(|term| term.total >= 5));
        let mut different = Miner::default();
        different
            .add(&"Game games Spiel Spiele ".repeat(5), 0)
            .unwrap();
        assert_eq!(different.finish().len(), 4);
    }
    #[test]
    fn german_and_english_articles_are_never_counted_in_any_source() {
        let mut miner = Miner::default();
        for source in 0..3 {
            miner
                .add(
                    &"DER die Das DEM den DES EIN eine Einem EINEN einer EINES THE a AN Nintendo "
                        .repeat(2),
                    source,
                )
                .unwrap();
        }
        // Repetition across all three sources must not let articles qualify.
        assert_eq!(miner.terms.len(), 1);
        let terms = miner.finish();
        assert_eq!(terms.len(), 1);
        assert_eq!(terms[0].term, "Nintendo");
        assert_eq!(
            (terms[0].transcript, terms[0].ocr, terms[0].captions),
            (2, 2, 2)
        );
    }

    #[test]
    fn five_total_occurrences_qualify_and_sort_by_total_descending_then_alphabetically() {
        let mut miner = Miner::default();
        miner.add("Below Below Below Below Solo Solo Solo Solo Solo Frequent Frequent Frequent Frequent Frequent Frequent Frequent Alpha Alpha Alpha Beta Beta Beta Shared Shared", 0).unwrap();
        miner.add("Alpha Alpha Beta Beta Shared Shared", 1).unwrap();
        miner.add("Shared", 2).unwrap();
        let terms = miner.finish();
        assert_eq!(
            terms
                .iter()
                .map(|term| (term.term.as_str(), term.total))
                .collect::<Vec<_>>(),
            [
                ("Frequent", 7),
                ("Alpha", 5),
                ("Beta", 5),
                ("Shared", 5),
                ("Solo", 5)
            ]
        );
        let shared = terms.iter().find(|t| t.term == "Shared").unwrap();
        assert_eq!((shared.transcript, shared.ocr, shared.captions), (2, 2, 1));
        let solo = terms.iter().find(|t| t.term == "Solo").unwrap();
        assert_eq!((solo.transcript, solo.sources), (5, 1));
    }

    #[test]
    fn screenshot_counts_match_the_shared_text_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../test/fixtures/screenshots/keywords.json"
        ))
        .unwrap();
        let mut miner = Miner::default();
        miner
            .add(fixture["transcript"].as_str().unwrap(), 0)
            .unwrap();
        for (source, name) in [(1, "ocr"), (2, "captions")] {
            for text in fixture[name].as_array().unwrap() {
                miner.add(text.as_str().unwrap(), source).unwrap();
            }
        }
        assert_eq!(
            serde_json::to_value(miner.finish()).unwrap(),
            fixture["result"]["terms"]
        );
    }
    #[test]
    fn all_saved_frames_and_caption_sources_are_counted_without_writes_or_file_access() {
        let conn = Connection::open_in_memory().unwrap();
        crate::database::initialize(&conn).unwrap();
        conn.execute_batch("INSERT INTO history_entries(id,url,created_at) VALUES ('video','https://example.com',1),('empty','https://example.com/empty',1);
            INSERT INTO transcriptions(history_entry_id,text,type) VALUES ('video','Nintendo Nintendo language','text');
            INSERT INTO history_ocr(history_entry_id,text,last_attempt_id,last_attempt_status,last_attempt_at,result_revision) VALUES ('video','Legacy Nintendo','original','completed',1,'original-revision');
            INSERT INTO captions(history_entry_id,media_path,caption_path,text,created_at) VALUES ('video','missing.mp4','missing.txt','Nintendo captions',1),('video','other.mp4','other.txt','Nintendo second',2);").unwrap();
        for index in 0..251 {
            conn.execute("INSERT INTO history_ocr_frames(id,history_entry_id,timestamp_ms,full_text,blocks_json) VALUES (?1,'video',?2,?3,'[]')", params![format!("frame-{index}"),index*1000, if index==250 {"RareLastFrame RareLastFrame RareLastFrame RareLastFrame RareLastFrame Nintendo"} else {"Nintendo"}]).unwrap();
        }
        let db = Database::new(conn);
        let result = mine(&db, "video").unwrap();
        let term = result.terms.iter().find(|t| t.term == "Nintendo").unwrap();
        assert_eq!((term.transcript, term.ocr, term.captions), (2, 252, 2));
        assert_eq!(result.sources.ocr_frames, 251);
        assert_eq!(result.sources.captions, 2);
        assert!(result.sources.ocr_legacy);
        assert!(result.terms.iter().any(|t| t.term == "RareLastFrame"));
        assert!(mine(&db, "empty").unwrap().terms.is_empty());
        assert!(mine(&db, "missing").is_err());
        let revision: String = db
            .lock()
            .unwrap()
            .query_row("SELECT result_revision FROM history_ocr", [], |r| r.get(0))
            .unwrap();
        assert_eq!(revision, "original-revision");
        db.lock()
            .unwrap()
            .execute("DELETE FROM history_ocr_frames WHERE id='frame-250'", [])
            .unwrap();
        assert!(!mine(&db, "video")
            .unwrap()
            .terms
            .iter()
            .any(|t| t.term == "RareLastFrame"));
    }
    #[test]
    fn snapshot_byte_budget_precedes_copying_and_errors_leave_the_connection_usable() {
        let conn = Connection::open_in_memory().unwrap();
        crate::database::initialize(&conn).unwrap();
        conn.execute_batch("INSERT INTO history_entries(id,url,created_at) VALUES('video','https://example.test',1); INSERT INTO transcriptions(history_entry_id,text,type) VALUES('video','Äpfel','text'); INSERT INTO history_ocr(history_entry_id,text,last_attempt_id,last_attempt_status,last_attempt_at) VALUES('video','X','fixture','completed',0);").unwrap();
        let db = Database::new(conn);
        let snapshot = db
            .read_snapshot(|conn| read_texts(conn, "video", 7))
            .unwrap();
        assert_eq!(
            snapshot
                .texts
                .iter()
                .map(|source| source.text.len())
                .sum::<usize>(),
            7
        );
        let error = db
            .read_snapshot(|conn| read_texts(conn, "video", 6))
            .err()
            .unwrap();
        assert!(error.contains("128 MiB"));
        assert!(db.lock().unwrap().is_autocommit());
        assert!(mine(&db, "missing").is_err());
        assert!(db.lock().unwrap().is_autocommit());
    }

    #[test]
    fn distinct_term_budget_still_applies_after_unlocking() {
        let conn = Connection::open_in_memory().unwrap();
        crate::database::initialize(&conn).unwrap();
        conn.execute("INSERT INTO history_entries(id,url,created_at) VALUES('video','https://example.test',1)",[]).unwrap();
        let text = (0..=MAX_TERMS)
            .map(|index| format!("Term{index} "))
            .collect::<String>();
        conn.execute(
            "INSERT INTO transcriptions(history_entry_id,text,type) VALUES('video',?1,'text')",
            [text],
        )
        .unwrap();
        let db = Database::new(conn);
        assert!(mine(&db, "video").unwrap_err().contains("50,000"));
        assert!(db.lock().unwrap().is_autocommit());
    }
}
