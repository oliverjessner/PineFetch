//! Local tag definitions, many-to-many membership and derived creator accounts.
use crate::database::{self, Database};
use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE tags (
 id TEXT PRIMARY KEY NOT NULL,
 name TEXT NOT NULL CHECK(length(name)>0 AND length(name)<=80),
 name_key TEXT NOT NULL UNIQUE,
 description TEXT NOT NULL DEFAULT '' CHECK(length(description)<=500),
 color TEXT NOT NULL CHECK(length(color)=7 AND color GLOB '#[0-9A-F][0-9A-F][0-9A-F][0-9A-F][0-9A-F][0-9A-F]'),
 created_at INTEGER NOT NULL CHECK(created_at>=0),
 updated_at INTEGER NOT NULL CHECK(updated_at>=created_at)
);
CREATE TABLE history_entry_tags (
 history_entry_id TEXT NOT NULL REFERENCES history_entries(id) ON DELETE CASCADE,
 tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
 created_at INTEGER NOT NULL CHECK(created_at>=0),
 PRIMARY KEY(history_entry_id,tag_id)
);
CREATE INDEX idx_history_entry_tags_tag ON history_entry_tags(tag_id,history_entry_id);
";
#[derive(Debug, Serialize, Clone)]
pub(crate) struct Tag {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) color: String,
    pub(crate) created_at: u64,
    pub(crate) updated_at: u64,
}
#[derive(Debug, Serialize)]
pub(crate) struct TagSummary {
    #[serde(flatten)]
    pub(crate) tag: Tag,
    pub(crate) content_count: u64,
    pub(crate) account_count: u64,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TagInput {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    pub(crate) color: String,
}
impl TagInput {
    fn validated(self) -> Result<(Self, String), String> {
        let name = self.name.trim().to_string();
        let description = self.description.trim().replace("\r\n", "\n");
        if name.is_empty() {
            return Err("Enter a tag name.".into());
        }
        if name.chars().count() > 80 || name.chars().any(char::is_control) {
            return Err(
                "Tag names must contain at most 80 characters and no control characters.".into(),
            );
        }
        if description.chars().count() > 500
            || description
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        {
            return Err("Tag descriptions must contain at most 500 characters.".into());
        }
        let color = self
            .color
            .trim()
            .strip_prefix('#')
            .unwrap_or(self.color.trim());
        let color = if color.len() == 3 {
            color.chars().flat_map(|c| [c, c]).collect::<String>()
        } else {
            color.into()
        };
        if color.len() != 6 || !color.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Choose a valid HEX color, such as #35C9A5.".into());
        }
        let key = crate::unicode_search::key(&name);
        Ok((
            Self {
                name,
                description,
                color: format!("#{}", color.to_ascii_uppercase()),
            },
            key,
        ))
    }
}
pub(crate) fn valid_id(id: &str) -> Result<String, String> {
    Uuid::parse_str(id)
        .map(|id| id.to_string())
        .map_err(|_| "Invalid tag ID. Reload tags and try again.".into())
}
fn map_tag(row: &rusqlite::Row<'_>) -> rusqlite::Result<Tag> {
    Ok(Tag {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        color: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}
fn load(conn: &Connection, id: &str) -> Result<Tag, String> {
    conn.query_row(
        "SELECT id,name,description,color,created_at,updated_at FROM tags WHERE id=?1",
        [id],
        map_tag,
    )
    .optional()
    .map_err(|e| format!("Tags could not be read: {e}"))?
    .ok_or("This tag no longer exists.".into())
}
pub(crate) fn require(conn: &Connection, id: &str) -> Result<(), String> {
    conn.query_row("SELECT 1 FROM tags WHERE id=?1", [id], |_| Ok(()))
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("This tag no longer exists.".into())
}
fn require_history(conn: &Connection, id: &str) -> Result<(), String> {
    conn.query_row(
        "SELECT 1 FROM history_entries WHERE id=?1",
        [id],
        |_| Ok(()),
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or("This History entry no longer exists.".into())
}
fn unique(conn: &Connection, key: &str, except: &str) -> Result<(), String> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM tags WHERE name_key=?1 AND id!=?2)",
            params![key, except],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists {
        return Err("A tag with this name already exists.".into());
    }
    Ok(())
}
pub(crate) fn create(db: &Database, input: TagInput) -> Result<Tag, String> {
    let (input, key) = input.validated()?;
    let id = Uuid::new_v4().to_string();
    let now = crate::history::millis_to_i64(crate::files::current_timestamp_millis());
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    unique(&tx, &key, "")?;
    tx.execute("INSERT INTO tags(id,name,name_key,description,color,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6)",params![id,input.name,key,input.description,input.color,now]).map_err(|e|format!("Tag could not be created: {e}"))?;
    let tag = load(&tx, &id)?;
    tx.commit()
        .map_err(|e| format!("Tag could not be saved: {e}"))?;
    Ok(tag)
}
pub(crate) fn update(db: &Database, id: &str, input: TagInput) -> Result<Tag, String> {
    let id = valid_id(id)?;
    let (input, key) = input.validated()?;
    let now = crate::history::millis_to_i64(crate::files::current_timestamp_millis());
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    require(&tx, &id)?;
    unique(&tx, &key, &id)?;
    tx.execute("UPDATE tags SET name=?1,name_key=?2,description=?3,color=?4,updated_at=MAX(updated_at,?5) WHERE id=?6",params![input.name,key,input.description,input.color,now,id]).map_err(|e|format!("Tag could not be updated: {e}"))?;
    let tag = load(&tx, &id)?;
    tx.commit()
        .map_err(|e| format!("Tag could not be saved: {e}"))?;
    Ok(tag)
}
pub(crate) fn delete(db: &Database, id: &str) -> Result<(), String> {
    let id = valid_id(id)?;
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    require(&tx, &id)?;
    tx.execute("DELETE FROM tags WHERE id=?1", [id])
        .map_err(|e| format!("Tag could not be deleted: {e}"))?;
    tx.commit().map_err(|e| format!("Tag deletion failed: {e}"))
}
pub(crate) fn assign(
    db: &Database,
    history_id: &str,
    tag_id: &str,
    add: bool,
) -> Result<(), String> {
    let tag_id = valid_id(tag_id)?;
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    // Removing after a concurrent deletion is an idempotent success. Adds must
    // verify both references, including an assignment already present.
    if add {
        require(&tx, &tag_id)?;
        require_history(&tx, history_id)?;
        tx.execute("INSERT INTO history_entry_tags(history_entry_id,tag_id,created_at) VALUES(?1,?2,?3) ON CONFLICT DO NOTHING",params![history_id,tag_id,crate::history::millis_to_i64(crate::files::current_timestamp_millis())]).map_err(|e|format!("Tag could not be assigned: {e}"))?;
    } else {
        tx.execute(
            "DELETE FROM history_entry_tags WHERE history_entry_id=?1 AND tag_id=?2",
            params![history_id, tag_id],
        )
        .map_err(|e| format!("Tag could not be removed: {e}"))?;
    }
    tx.commit()
        .map_err(|e| format!("Tag assignment could not be saved: {e}"))
}
// Keep unknown creator IDs entry-scoped, exactly as Account navigation does.
// This scalar only trims tiny labels; URL inspection and network access are absent.
pub(crate) fn register(conn: &Connection) -> Result<(), String> {
    conn.create_scalar_function(
        "pinefetch_tag_source",
        4,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let creator_source: Option<String> = ctx.get(0)?;
            let creator_id: Option<String> = ctx.get(1)?;
            let source: Option<String> = ctx.get(2)?;
            let uploader: Option<String> = ctx.get(3)?;
            if uploader.is_none_or(|name| name.trim().is_empty()) {
                return Ok(None::<String>);
            }
            let source = if creator_id.is_some() {
                creator_source.or(source)
            } else {
                source
            };
            Ok(source
                .filter(|source| !source.trim().is_empty())
                .map(|source| crate::creator_identity::canonical_source(&source)))
        },
    )
    .map_err(|e| e.to_string())
}
const ACCOUNTS: &str = "members AS (SELECT link.tag_id,history.id AS history_id,history.uploader,
 pinefetch_tag_source(history.creator_source,history.creator_id,history.source,history.uploader) AS source,
 CASE WHEN history.creator_id IS NOT NULL AND history.creator_source IS NOT NULL THEN history.creator_id ELSE 'entry:'||history.id END AS identity,
 COALESCE(history.completed_at,history.created_at) AS saved_at FROM history_entry_tags link JOIN history_entries history ON history.id=link.history_entry_id";
fn accounts_cte(tagged: bool) -> String {
    format!("WITH {ACCOUNTS}{}), accounts AS (SELECT tag_id,source,identity,COUNT(*) AS content_count,MIN(history_id) AS history_id,MAX(saved_at) AS latest_at FROM members WHERE source IS NOT NULL GROUP BY tag_id,source,identity)",if tagged {" WHERE link.tag_id=?1"} else {""})
}
pub(crate) fn list(db: &Database) -> Result<Vec<TagSummary>, String> {
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let sql=format!("{}, uses AS (SELECT tag_id,COUNT(*) AS content_count FROM history_entry_tags GROUP BY tag_id), creators AS (SELECT tag_id,COUNT(*) AS account_count FROM accounts GROUP BY tag_id) SELECT tags.id,tags.name,tags.description,tags.color,tags.created_at,tags.updated_at,COALESCE(uses.content_count,0),COALESCE(creators.account_count,0) FROM tags LEFT JOIN uses ON uses.tag_id=tags.id LEFT JOIN creators ON creators.tag_id=tags.id ORDER BY tags.name_key,tags.id",accounts_cte(false));
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let result = stmt
        .query_map([], |r| {
            Ok(TagSummary {
                tag: map_tag(r)?,
                content_count: r.get(6)?,
                account_count: r.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| format!("Tags could not be loaded: {e}"));
    result
}
pub(crate) fn get(db: &Database, id: &str) -> Result<TagSummary, String> {
    let id = valid_id(id)?;
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let tag = load(&tx, &id)?;
    let content_count = tx
        .query_row(
            "SELECT COUNT(*) FROM history_entry_tags WHERE tag_id=?1",
            [&id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let account_count = tx
        .query_row(
            &format!("{} SELECT COUNT(*) FROM accounts", accounts_cte(true)),
            [&id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(TagSummary {
        tag,
        content_count,
        account_count,
    })
}
#[derive(Debug, Serialize)]
pub(crate) struct EntryTags {
    pub(crate) history_entry_id: String,
    pub(crate) tags: Vec<Tag>,
}
pub(crate) fn batch(db: &Database, ids: &[String]) -> Result<Vec<EntryTags>, String> {
    if ids.len() > 500 {
        return Err("Load tags for at most 500 entries at once.".into());
    }
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut ids = ids.to_vec();
    ids.sort();
    ids.dedup();
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql=format!("SELECT tags.id,tags.name,tags.description,tags.color,tags.created_at,tags.updated_at,link.history_entry_id FROM history_entry_tags link JOIN tags ON tags.id=link.tag_id WHERE link.history_entry_id IN ({placeholders}) ORDER BY link.history_entry_id,tags.name_key,tags.id");
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let count: u64 = tx
        .query_row(
            &format!("SELECT COUNT(*) FROM history_entries WHERE id IN ({placeholders})"),
            params_from_iter(&ids),
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if count != ids.len() as u64 {
        return Err("A History entry no longer exists. Reload the list.".into());
    }
    let mut output: Vec<_> = ids
        .iter()
        .map(|id| EntryTags {
            history_entry_id: id.clone(),
            tags: Vec::new(),
        })
        .collect();
    {
        let mut stmt = tx.prepare(&sql).map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query(params_from_iter(&ids))
            .map_err(|e| e.to_string())?;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let id: String = row.get(6).map_err(|e| e.to_string())?;
            let index = ids
                .binary_search(&id)
                .map_err(|_| "Unexpected tag membership")?;
            output[index]
                .tags
                .push(map_tag(row).map_err(|e| e.to_string())?);
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(output)
}
#[derive(Debug, Serialize)]
pub(crate) struct TaggedAccount {
    pub(crate) history_entry_id: String,
    pub(crate) uploader: String,
    pub(crate) source: String,
    pub(crate) content_count: u64,
    pub(crate) identity_verified: bool,
    pub(crate) latest_at: i64,
}
#[derive(Debug, Serialize)]
pub(crate) struct AccountPage {
    pub(crate) entries: Vec<TaggedAccount>,
    pub(crate) total_count: u64,
    pub(crate) next_cursor: Option<String>,
    pub(crate) has_more: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountCursor {
    version: u8,
    tag_id: String,
    query: String,
    source: String,
    identity: String,
    total: u64,
}
pub(crate) fn accounts(
    db: &Database,
    id: &str,
    query: &str,
    limit: u32,
    after: Option<&str>,
) -> Result<AccountPage, String> {
    let id = valid_id(id)?;
    let query = crate::unicode_search::key(query.trim());
    let limit = limit.clamp(1, 100);
    let cursor = after
        .map(|raw| {
            if raw.len() > 4096 {
                return Err("Tag account cursor is too large.".to_string());
            }
            let cursor: AccountCursor =
                serde_json::from_str(raw).map_err(|_| "Invalid tag account cursor.")?;
            if cursor.version != 1 || cursor.tag_id != id || cursor.query != query {
                return Err("Tag account cursor does not match this view.".into());
            }
            Ok(cursor)
        })
        .transpose()?;
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    require(&tx, &id)?;
    let cte = accounts_cte(true);
    let from = "FROM accounts a JOIN history_entries h ON h.id=a.history_id";
    let predicate = "(?2='' OR pinefetch_contains(h.uploader,?2))";
    let total_count = if let Some(cursor) = &cursor {
        cursor.total
    } else {
        tx.query_row(
            &format!("{cte} SELECT COUNT(*) {from} WHERE {predicate}"),
            params![id, query],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?
    };
    let mut values = vec![Value::Text(id.clone()), Value::Text(query.clone())];
    let continuation = if let Some(cursor) = cursor {
        values.extend([Value::Text(cursor.source), Value::Text(cursor.identity)]);
        " AND (a.source,a.identity)>(?3,?4)"
    } else {
        ""
    };
    let sql=format!("{cte} SELECT a.history_id,h.uploader,a.source,a.content_count,h.creator_id IS NOT NULL,a.latest_at,a.identity {from} WHERE {predicate}{continuation} ORDER BY a.source,a.identity LIMIT ?{}",values.len()+1);
    values.push(Value::Integer(i64::from(limit) + 1));
    let mut entries = Vec::new();
    let mut next_cursor = None;
    let mut has_more = false;
    {
        let mut stmt = tx.prepare(&sql).map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query(params_from_iter(values))
            .map_err(|e| e.to_string())?;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            if entries.len() >= limit as usize {
                has_more = true;
                break;
            }
            let source: String = row.get(2).map_err(|e| e.to_string())?;
            next_cursor = Some(
                serde_json::to_string(&AccountCursor {
                    version: 1,
                    tag_id: id.clone(),
                    query: query.clone(),
                    source: source.clone(),
                    identity: row.get(6).map_err(|e| e.to_string())?,
                    total: total_count,
                })
                .map_err(|e| e.to_string())?,
            );
            entries.push(TaggedAccount {
                history_entry_id: row.get(0).map_err(|e| e.to_string())?,
                uploader: row.get(1).map_err(|e| e.to_string())?,
                source,
                content_count: row.get(3).map_err(|e| e.to_string())?,
                identity_verified: row.get(4).map_err(|e| e.to_string())?,
                latest_at: row.get(5).map_err(|e| e.to_string())?,
            });
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(AccountPage {
        entries,
        total_count,
        next_cursor: if has_more { next_cursor } else { None },
        has_more,
    })
}
