//! The briefing's memory: dumps and their items, in SQLite.
//!
//! One briefing per day. Unfinished items (queued, running, held, failed) carry over to the next day;
//! done items drop off at the end of their day and stay in the table as history.

use rusqlite::{Connection, Result, params};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize, PartialEq)]
pub struct Item {
    pub id: i64,
    pub kind: String,
    pub title: String,
    /// queued, running, held, done, failed or dismissed.
    pub status: String,
    pub result: String,
    pub created: String,
}

pub struct Store(Connection);

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS dumps (
    id      INTEGER PRIMARY KEY,
    text    TEXT NOT NULL,
    created TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);
CREATE TABLE IF NOT EXISTS items (
    id      INTEGER PRIMARY KEY,
    dump_id INTEGER REFERENCES dumps(id),
    kind    TEXT NOT NULL,
    title   TEXT NOT NULL,
    detail  TEXT NOT NULL DEFAULT '',
    status  TEXT NOT NULL,
    result  TEXT NOT NULL DEFAULT '',
    created TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
    day     TEXT NOT NULL DEFAULT (date('now', 'localtime'))
);";

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path)?;
        db.execute_batch(SCHEMA)?;
        // An answer that was mid-flight when Engine quit will never finish.
        db.execute(
            "UPDATE items SET status = 'failed', result = 'Engine closed before this finished.' WHERE status = 'running'",
            [],
        )?;
        Ok(Self(db))
    }

    pub fn add_dump(&self, text: &str) -> Result<i64> {
        self.0.execute("INSERT INTO dumps (text) VALUES (?1)", [text])?;
        Ok(self.0.last_insert_rowid())
    }

    pub fn add_item(&self, dump: Option<i64>, kind: &str, title: &str, detail: &str, status: &str, result: &str) -> Result<i64> {
        self.0.execute(
            "INSERT INTO items (dump_id, kind, title, detail, status, result) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![dump, kind, title, detail, status, result],
        )?;
        Ok(self.0.last_insert_rowid())
    }

    pub fn set(&self, id: i64, status: &str, result: &str) -> Result<()> {
        self.0.execute("UPDATE items SET status = ?2, result = ?3 WHERE id = ?1", params![id, status, result])?;
        Ok(())
    }

    /// Removes an item from the briefing. Running items finish first.
    pub fn dismiss(&self, id: i64) -> Result<()> {
        self.0.execute("UPDATE items SET status = 'dismissed' WHERE id = ?1 AND status != 'running'", [id])?;
        Ok(())
    }

    /// Today's items plus anything unfinished from earlier days, oldest first.
    pub fn briefing(&self) -> Result<Vec<Item>> {
        let mut q = self.0.prepare(
            "SELECT id, kind, title, status, result, created FROM items
             WHERE (day = date('now', 'localtime') AND status != 'dismissed')
                OR (kind != 'instant' AND status IN ('queued', 'running', 'held', 'failed'))
             ORDER BY id",
        )?;
        q.query_map([], |r| {
            Ok(Item {
                id: r.get(0)?,
                kind: r.get(1)?,
                title: r.get(2)?,
                status: r.get(3)?,
                result: r.get(4)?,
                created: r.get(5)?,
            })
        })?
        .collect()
    }

    /// The local date for prompts, e.g. "Monday 2026-11-16".
    pub fn today(&self) -> Result<String> {
        let (weekday, date): (i64, String) = self.0.query_row(
            "SELECT CAST(strftime('%w', 'now', 'localtime') AS INTEGER), date('now', 'localtime')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let days = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
        Ok(format!("{} {date}", days[weekday as usize]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn briefing_carries_over_unfinished_items() {
        let s = Store::open(Path::new(":memory:")).unwrap();
        let dump = s.add_dump("a dump").unwrap();
        let add = |kind: &str, title: &str, status: &str| s.add_item(Some(dump), kind, title, "", status, "").unwrap();
        let yesterday = |id: i64| {
            s.0.execute("UPDATE items SET day = date('now', 'localtime', '-1 day') WHERE id = ?1", [id]).unwrap();
        };

        let old_done = add("answer", "old done", "done");
        let old_held = add("outward", "old held", "held");
        let old_open = add("instant", "old open", "failed");
        let gone = add("job", "dismissed", "queued");
        add("answer", "today done", "done");
        let running = add("answer", "today running", "running");
        for id in [old_done, old_held, old_open] {
            yesterday(id);
        }
        s.dismiss(gone).unwrap();
        s.dismiss(running).unwrap(); // ignored: still running

        let titles: Vec<_> = s.briefing().unwrap().into_iter().map(|i| i.title).collect();
        assert_eq!(titles, ["old held", "today done", "today running"]);

        s.set(running, "done", "a plan").unwrap();
        assert_eq!(s.briefing().unwrap()[2].result, "a plan");
        assert!(s.today().unwrap().contains(char::is_numeric));
    }
}
