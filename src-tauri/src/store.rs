//! The briefing's memory: dumps and their items, in SQLite.
//!
//! One briefing per day. Unfinished items (queued, running, held, failed) carry over to the next day;
//! done items drop off at the end of their day and stay in the table as history.

use rusqlite::{Connection, OptionalExtension, Result, params};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize, PartialEq)]
pub struct Item {
    pub id: i64,
    pub kind: String,
    pub title: String,
    /// queued, running, held, done, failed or dismissed.
    pub status: String,
    /// While running: the result is the current step, and this is the orb for it (thinking, viewing, ...).
    pub process: String,
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
        if db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? < 1 {
            db.execute_batch("ALTER TABLE items ADD COLUMN process TEXT NOT NULL DEFAULT ''; PRAGMA user_version = 1;")?;
        }
        // An answer or job that was mid-flight when Engine quit will never finish.
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

    /// Ends a running item. Does nothing if it was cancelled meanwhile.
    pub fn finish(&self, id: i64, status: &str, result: &str) -> Result<()> {
        self.0.execute(
            "UPDATE items SET status = ?2, result = ?3, process = '' WHERE id = ?1 AND status = 'running'",
            params![id, status, result],
        )?;
        Ok(())
    }

    /// Records what a running job is doing now.
    pub fn set_activity(&self, id: i64, process: &str, label: &str) -> Result<()> {
        self.0.execute(
            "UPDATE items SET process = ?2, result = ?3 WHERE id = ?1 AND status = 'running'",
            params![id, process, label],
        )?;
        Ok(())
    }

    /// Marks the oldest queued job running and returns its id, dump and instructions.
    pub fn claim_job(&self) -> Result<Option<(i64, Option<i64>, String)>> {
        self.0
            .query_row(
                "UPDATE items SET status = 'running', result = 'Starting', process = 'thinking'
                 WHERE id = (SELECT id FROM items WHERE kind = 'job' AND status = 'queued' ORDER BY id LIMIT 1)
                 RETURNING id, dump_id, detail",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
    }

    /// Stops tracking a running job; the caller kills its process. False if it wasn't running.
    pub fn cancel(&self, id: i64) -> Result<bool> {
        let n = self.0.execute(
            "UPDATE items SET status = 'dismissed' WHERE id = ?1 AND kind = 'job' AND status = 'running'",
            [id],
        )?;
        Ok(n > 0)
    }

    /// Puts a failed job back in the queue.
    pub fn retry(&self, id: i64) -> Result<()> {
        self.0.execute(
            "UPDATE items SET status = 'queued', result = '', process = '' WHERE id = ?1 AND kind = 'job' AND status = 'failed'",
            [id],
        )?;
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
            "SELECT id, kind, title, status, process, result, created FROM items
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
                process: r.get(4)?,
                result: r.get(5)?,
                created: r.get(6)?,
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

        s.finish(running, "done", "a plan").unwrap();
        assert_eq!(s.briefing().unwrap()[2].result, "a plan");
        s.finish(running, "failed", "late").unwrap(); // no longer running: ignored
        assert_eq!(s.briefing().unwrap()[2].status, "done");
        assert!(s.today().unwrap().contains(char::is_numeric));
    }

    #[test]
    fn jobs_queue_run_and_cancel() {
        let s = Store::open(Path::new(":memory:")).unwrap();
        let first = s.add_item(None, "job", "first", "do one", "queued", "").unwrap();
        let second = s.add_item(None, "job", "second", "do two", "queued", "").unwrap();

        assert_eq!(s.claim_job().unwrap(), Some((first, None, "do one".into())));
        s.set_activity(first, "viewing", "Reading plan.md").unwrap();
        let item = &s.briefing().unwrap()[0];
        assert_eq!((item.process.as_str(), item.result.as_str()), ("viewing", "Reading plan.md"));

        assert!(s.cancel(first).unwrap());
        s.finish(first, "failed", "killed").unwrap(); // the run ending after Cancel changes nothing
        assert_eq!(s.claim_job().unwrap().map(|j| j.0), Some(second));
        assert_eq!(s.claim_job().unwrap(), None);

        s.finish(second, "failed", "oops").unwrap();
        s.retry(second).unwrap();
        assert_eq!(s.claim_job().unwrap().map(|j| j.0), Some(second));
        let titles: Vec<_> = s.briefing().unwrap().into_iter().map(|i| i.title).collect();
        assert_eq!(titles, ["second"]);
    }
}
