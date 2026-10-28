//! Parameterized filters and keyset pagination shared by History and Accounts.
use crate::database::Database;
use crate::history::{
    history_search_pattern, i64_to_millis, map_history_entry, optional_i64_to_millis,
};
use crate::models::{HistoryOcrSummary, HistoryPage};
use rusqlite::{params_from_iter, types::Value};
use serde::{Deserialize, Serialize};

#[cfg(test)]
#[derive(Clone, Copy, Default)]
pub(crate) struct QueryWork {
    pub(crate) count_steps: i32,
    pub(crate) page_steps: i32,
    pub(crate) sorts: i32,
}
#[cfg(test)]
thread_local! {
    static LAST_WORK: std::cell::Cell<QueryWork> = const { std::cell::Cell::new(QueryWork { count_steps:0,page_steps:0,sorts:0 }) };
}
#[cfg(test)]
pub(crate) fn last_query_work() -> QueryWork {
    LAST_WORK.with(|work| work.get())
}

pub(crate) const INDEX_SQL: &str = "
DROP INDEX idx_history_entries_completed_at;
DROP INDEX idx_history_account_lookup;
CREATE INDEX idx_history_order ON history_entries(COALESCE(completed_at,created_at) DESC,created_at DESC,id DESC);
CREATE INDEX idx_history_source_order ON history_entries(source COLLATE NOCASE,COALESCE(completed_at,created_at) DESC,created_at DESC,id DESC);
CREATE INDEX idx_history_creator_order ON history_entries(source COLLATE NOCASE,uploader COLLATE BINARY,COALESCE(completed_at,created_at) DESC,created_at DESC,id DESC);
";

#[derive(Serialize)]
pub(crate) struct HistoryFilter {
    pattern: Option<String>,
    field: String,
    source: Option<String>,
    exact_uploader: Option<String>,
}
impl HistoryFilter {
    pub(crate) fn new(
        query: Option<&str>,
        field: Option<&str>,
        source: Option<&str>,
        exact: bool,
    ) -> Result<Self, String> {
        let field = field
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .unwrap_or("title");
        if !matches!(field, "title" | "user" | "description" | "transcript") {
            return Err(format!("Unsupported history search field: {field}"));
        }
        Ok(Self {
            pattern: history_search_pattern(query),
            field: field.into(),
            source: source
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_ascii_lowercase),
            exact_uploader: if exact && field == "user" {
                query
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            } else {
                None
            },
        })
    }
    fn signature(&self) -> Result<String, String> {
        serde_json::to_vec(self)
            .map(|json| crate::hashing::sha256_hex(&json))
            .map_err(|e| e.to_string())
    }
    // Only fixed SQL fragments enter the query. Every user value stays bound.
    pub(crate) fn predicate(&self) -> (String, Vec<Value>) {
        let mut clauses = Vec::new();
        let mut values = Vec::new();
        if let Some(source) = &self.source {
            values.push(Value::Text(source.clone()));
            clauses.push(format!("history.source=?{} COLLATE NOCASE", values.len()));
        }
        if let Some(uploader) = &self.exact_uploader {
            values.push(Value::Text(uploader.clone()));
            clauses.push(format!("history.uploader=?{} COLLATE BINARY", values.len()));
        } else if let Some(pattern) = &self.pattern {
            values.push(Value::Text(pattern.clone()));
            let n = values.len();
            clauses.push(match self.field.as_str() {
                "title" => format!("history.title LIKE ?{n} ESCAPE '\\' COLLATE NOCASE"),
                "user" => format!("history.uploader LIKE ?{n} ESCAPE '\\' COLLATE NOCASE"),
                "description" => format!("EXISTS(SELECT 1 FROM captions WHERE captions.history_entry_id=history.id AND captions.text LIKE ?{n} ESCAPE '\\' COLLATE NOCASE)"),
                "transcript" => format!("EXISTS(SELECT 1 FROM transcriptions WHERE transcriptions.history_entry_id=history.id AND transcriptions.text LIKE ?{n} ESCAPE '\\' COLLATE NOCASE)"),
                _ => unreachable!("validated search field"),
            });
        }
        (
            if clauses.is_empty() {
                "1".into()
            } else {
                clauses.join(" AND ")
            },
            values,
        )
    }
}

// Opaque JSON strings preserve signed 64-bit SQLite keys across JavaScript.
// Counts describe the first page's view; refresh starts a new view and recounts.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    filter: String,
    completed: i64,
    created: i64,
    id: String,
    total: u64,
}

pub(crate) fn read_page(
    db: &Database,
    limit: u32,
    offset: u32,
    filter: HistoryFilter,
    after: Option<&str>,
) -> Result<HistoryPage, String> {
    #[cfg(test)]
    let mut work = QueryWork::default();
    let signature = filter.signature()?;
    let cursor = after
        .map(|raw| {
            if raw.len() > 4096 {
                return Err("History cursor is too large. Reload the list.".to_string());
            }
            let cursor: Cursor = serde_json::from_str(raw)
                .map_err(|_| "Invalid History cursor. Reload the list.")?;
            if cursor.version != 1 || cursor.filter != signature || offset != 0 {
                return Err("History cursor does not match this view. Reload the list.".into());
            }
            Ok(cursor)
        })
        .transpose()?;
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("History read failed: {e}"))?;
    let (predicate, mut values) = filter.predicate();
    let total = if let Some(cursor) = &cursor {
        cursor.total
    } else {
        let count_sql = if predicate == "1" {
            "SELECT COUNT(*) FROM history_entries".to_owned()
        } else {
            format!("SELECT COUNT(*) FROM history_entries AS history WHERE {predicate}")
        };
        let mut count_stmt = tx
            .prepare(&count_sql)
            .map_err(|e| format!("History read failed: {e}"))?;
        let total: i64 = count_stmt
            .query_row(params_from_iter(&values), |r| r.get(0))
            .map_err(|e| format!("History read failed: {e}"))?;
        #[cfg(test)]
        {
            work.count_steps = count_stmt.get_status(rusqlite::StatementStatus::VmStep);
        }
        total.max(0) as u64
    };
    let keys = "SELECT history.id,COALESCE(history.completed_at,history.created_at) AS sort_time,history.created_at FROM history_entries AS history";
    // The bundled SQLite does not seek this expression-led row comparison.
    // Use three disjoint index ranges, each limited before merging. Omitting
    // their local limits/order lets SQLite sort entire equal-timestamp groups.
    let (ranges, candidates) = if let Some(cursor) = cursor {
        let n = values.len();
        let (t, c, id, page_limit) = (n + 1, n + 2, n + 3, n + 4);
        values.extend([
            Value::Integer(cursor.completed),
            Value::Integer(cursor.created),
            Value::Text(cursor.id),
        ]);
        (
            format!("same_created AS MATERIALIZED (
                {keys} WHERE {predicate} AND COALESCE(history.completed_at,history.created_at)=?{t} AND history.created_at=?{c} AND history.id<?{id}
                ORDER BY history.id DESC LIMIT ?{page_limit}),
                same_completed AS MATERIALIZED (
                {keys} WHERE {predicate} AND COALESCE(history.completed_at,history.created_at)=?{t} AND history.created_at<?{c}
                ORDER BY history.created_at DESC,history.id DESC LIMIT ?{page_limit}),
                older_completed AS MATERIALIZED (
                {keys} WHERE {predicate} AND COALESCE(history.completed_at,history.created_at)<?{t}
                ORDER BY sort_time DESC,history.created_at DESC,history.id DESC LIMIT ?{page_limit}),"),
            "SELECT * FROM same_created UNION ALL SELECT * FROM same_completed UNION ALL SELECT * FROM older_completed".to_owned(),
        )
    } else {
        (String::new(), format!("{keys} WHERE {predicate}"))
    };
    let n = values.len();
    let sql=format!("WITH {ranges} page AS MATERIALIZED (
        {candidates} ORDER BY sort_time DESC,created_at DESC,id DESC LIMIT ?{} OFFSET ?{}
        ) SELECT history.id,history.url,history.title,history.uploader,history.filename,history.thumbnail,history.upload_date,history.timestamp,history.duration_seconds,history.file_size_bytes,history.sha256,history.medium,history.source,history.platform,history.output_path,history.pinefetch_version,history.created_at,history.completed_at,
        ocr.last_attempt_id,ocr.last_attempt_at,ocr.last_attempt_status,ocr.result_created_at,page.sort_time
        FROM page JOIN history_entries AS history ON history.id=page.id
        LEFT JOIN history_ocr AS ocr ON ocr.history_entry_id=history.id
        ORDER BY page.sort_time DESC,page.created_at DESC,page.id DESC",n+1,n+2);
    values.extend([
        Value::Integer(i64::from(limit.max(1)) + 1),
        Value::Integer(i64::from(offset)),
    ]);
    let mut stmt = tx
        .prepare(&sql)
        .map_err(|e| format!("History read failed: {e}"))?;
    let mut rows = stmt
        .query(params_from_iter(&values))
        .map_err(|e| format!("History read failed: {e}"))?;
    let mut entries = Vec::new();
    let mut ocr_statuses = Vec::new();
    let mut next_cursor = None;
    let mut has_more = false;
    while let Some(row) = rows
        .next()
        .map_err(|e| format!("History read failed: {e}"))?
    {
        if entries.len() >= limit.max(1) as usize {
            has_more = true;
            break;
        }
        let entry = map_history_entry(row).map_err(|e| format!("History read failed: {e}"))?;
        let attempt_id: Option<String> = row.get(18).map_err(|e| e.to_string())?;
        if let Some(attempt_id) = attempt_id {
            ocr_statuses.push(HistoryOcrSummary {
                history_id: entry.id.clone(),
                attempt_id,
                attempt_at: i64_to_millis(row.get(19).map_err(|e| e.to_string())?),
                status: row.get(20).map_err(|e| e.to_string())?,
                result_created_at: optional_i64_to_millis(row.get(21).map_err(|e| e.to_string())?),
            });
        }
        next_cursor = Some(
            serde_json::to_string(&Cursor {
                version: 1,
                filter: signature.clone(),
                completed: row.get(22).map_err(|e| e.to_string())?,
                created: row.get(16).map_err(|e| e.to_string())?,
                id: entry.id.clone(),
                total,
            })
            .map_err(|e| e.to_string())?,
        );
        entries.push(entry);
    }
    drop(rows);
    #[cfg(test)]
    {
        work.page_steps = stmt.get_status(rusqlite::StatementStatus::VmStep);
        work.sorts = stmt.get_status(rusqlite::StatementStatus::Sort);
        LAST_WORK.with(|last| last.set(work));
    }
    drop(stmt);
    tx.commit()
        .map_err(|e| format!("History read failed: {e}"))?;
    Ok(HistoryPage {
        entries,
        ocr_statuses,
        has_more,
        total_count: total,
        next_cursor: if has_more { next_cursor } else { None },
    })
}
