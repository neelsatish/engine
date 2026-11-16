//! The briefing's memory: dumps and their items, in SQLite.
//!
//! One briefing per day. Unfinished items (queued, running, held, failed) carry over to the next day;
//! done items drop off at the end of their day and stay in the table as history.

use rusqlite::{Connection, OptionalExtension, Result, params};
use serde::Serialize;
use crate::schedule::Block;
use std::path::Path;

#[derive(Debug, Serialize, PartialEq)]
pub struct Item {
    pub id: i64,
    /// instant, answer, job, outward (an approval card) or task (something only the user can do).
    pub kind: String,
    pub title: String,
    /// What a card shows: an approval's exact action, or a task's follow-up.
    pub detail: String,
    /// queued, running, held, done, failed or dismissed.
    pub status: String,
    /// While running: the result is the current step, and this is the orb for it (thinking, viewing, ...).
    pub process: String,
    pub result: String,
    pub created: String,
    /// For a finished job: "checking", "good" or "concern" from the audit, or empty.
    pub verdict: String,
    pub verdict_reason: String,
    /// The dump it came from, and what the person said there (for the chat).
    pub dump: Option<i64>,
    pub said: String,
}

pub struct Store(Connection);

/// A job taken off the queue to run.
#[derive(Debug, PartialEq)]
pub struct Job {
    pub id: i64,
    pub dump: Option<i64>,
    pub prompt: String,
    /// A project name from Settings, or empty.
    pub project: String,
    /// For an approved action: the one outward tool it may call once ("*" for any). Empty for normal jobs.
    pub tool: String,
    /// The folder chosen in the dump box when the job was made, or empty.
    pub folder: String,
}

/// An approval card or a needs-you task, waiting on the user.
#[derive(Debug, PartialEq)]
pub struct Held {
    pub kind: String,
    pub dump: Option<i64>,
    pub title: String,
    pub detail: String,
    pub tool: String,
    pub project: String,
    pub folder: String,
}

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
);
CREATE TABLE IF NOT EXISTS blocks (
    id      INTEGER PRIMARY KEY,
    title   TEXT NOT NULL,
    start   TEXT NOT NULL,
    end     TEXT NOT NULL,
    kind    TEXT NOT NULL DEFAULT 'other',
    note    TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS blocks_start ON blocks(start);";

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path)?;
        db.execute_batch(SCHEMA)?;
        if db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? < 1 {
            db.execute_batch("ALTER TABLE items ADD COLUMN process TEXT NOT NULL DEFAULT ''; PRAGMA user_version = 1;")?;
        }
        if db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? < 2 {
            db.execute_batch("ALTER TABLE items ADD COLUMN project TEXT NOT NULL DEFAULT ''; PRAGMA user_version = 2;")?;
        }
        if db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? < 3 {
            db.execute_batch(
                "ALTER TABLE items ADD COLUMN verdict TEXT NOT NULL DEFAULT '';
                 ALTER TABLE items ADD COLUMN verdict_reason TEXT NOT NULL DEFAULT '';
                 PRAGMA user_version = 3;",
            )?;
        }
        if db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? < 4 {
            // For an approval card, the tool the agent would use; for an approved job, the one tool it may use once.
            db.execute_batch("ALTER TABLE items ADD COLUMN tool TEXT NOT NULL DEFAULT ''; PRAGMA user_version = 4;")?;
        }
        if db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? < 5 {
            db.execute_batch(
                "ALTER TABLE items ADD COLUMN folder TEXT NOT NULL DEFAULT '';
                 ALTER TABLE blocks ADD COLUMN fired INTEGER NOT NULL DEFAULT 0;
                 PRAGMA user_version = 5;",
            )?;
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

    /// Queues a job for an agent. `project` is a project name from Settings, or empty; `tool` is set only for an
    /// approved action (see `Job::tool`).
    pub fn add_job(&self, dump: Option<i64>, title: &str, detail: &str, project: &str, tool: &str, folder: &str) -> Result<i64> {
        self.0.execute(
            "INSERT INTO items (dump_id, kind, title, detail, status, project, tool, folder)
             VALUES (?1, 'job', ?2, ?3, 'queued', ?4, ?5, ?6)",
            params![dump, title, detail, project, tool, folder],
        )?;
        Ok(self.0.last_insert_rowid())
    }

    /// Holds an approval card (`kind` "outward") or a needs-you task (`kind` "task") for the user.
    #[allow(clippy::too_many_arguments)]
    pub fn add_held(
        &self,
        kind: &str,
        dump: Option<i64>,
        title: &str,
        detail: &str,
        tool: &str,
        project: &str,
        folder: &str,
    ) -> Result<i64> {
        self.0.execute(
            "INSERT INTO items (dump_id, kind, title, detail, status, tool, project, folder)
             VALUES (?1, ?2, ?3, ?4, 'held', ?5, ?6, ?7)",
            params![dump, kind, title, detail, tool, project, folder],
        )?;
        Ok(self.0.last_insert_rowid())
    }

    /// Takes a held card off the briefing as `status` (done or dismissed) and returns it. None if it wasn't held.
    pub fn take_held(&self, id: i64, status: &str) -> Result<Option<Held>> {
        self.0
            .query_row(
                "UPDATE items SET status = ?2 WHERE id = ?1 AND status = 'held'
                 RETURNING kind, dump_id, title, detail, tool, project, folder",
                params![id, status],
                |r| {
                    Ok(Held {
                        kind: r.get(0)?,
                        dump: r.get(1)?,
                        title: r.get(2)?,
                        detail: r.get(3)?,
                        tool: r.get(4)?,
                        project: r.get(5)?,
                        folder: r.get(6)?,
                    })
                },
            )
            .optional()
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

    /// Marks the oldest queued job running and returns it.
    pub fn claim_job(&self) -> Result<Option<Job>> {
        self.0
            .query_row(
                "UPDATE items SET status = 'running', result = 'Starting', process = 'thinking'
                 WHERE id = (SELECT id FROM items WHERE kind = 'job' AND status = 'queued' ORDER BY id LIMIT 1)
                 RETURNING id, dump_id, detail, project, tool, folder",
                [],
                |r| {
                    Ok(Job {
                        id: r.get(0)?,
                        dump: r.get(1)?,
                        prompt: r.get(2)?,
                        project: r.get(3)?,
                        tool: r.get(4)?,
                        folder: r.get(5)?,
                    })
                },
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

    /// Records the audit of a finished job: "checking" while it runs, then "good" or "concern".
    pub fn set_verdict(&self, id: i64, verdict: &str, reason: &str) -> Result<()> {
        self.0.execute(
            "UPDATE items SET verdict = ?2, verdict_reason = ?3 WHERE id = ?1",
            params![id, verdict, reason],
        )?;
        Ok(())
    }

    /// Puts a failed job back in the queue.
    pub fn retry(&self, id: i64) -> Result<()> {
        self.0.execute(
            "UPDATE items SET status = 'queued', result = '', process = '', verdict = '', verdict_reason = ''
             WHERE id = ?1 AND kind = 'job' AND status = 'failed'",
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
            "SELECT i.id, i.kind, i.title, i.status, i.process, i.result, i.created, i.verdict, i.verdict_reason,
                    i.detail, i.dump_id, COALESCE(d.text, '')
             FROM items i LEFT JOIN dumps d ON d.id = i.dump_id
             WHERE (i.day = date('now', 'localtime') AND i.status != 'dismissed')
                OR (i.kind != 'instant' AND i.status IN ('queued', 'running', 'held', 'failed'))
             ORDER BY i.id",
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
                verdict: r.get(7)?,
                verdict_reason: r.get(8)?,
                detail: r.get(9)?,
                dump: r.get(10)?,
                said: r.get(11)?,
            })
        })?
        .collect()
    }

    /// Blocks that overlap [from, to), earliest first. Times are "YYYY-MM-DDTHH:MM".
    pub fn blocks(&self, from: &str, to: &str) -> Result<Vec<Block>> {
        let mut q = self.0.prepare(
            "SELECT id, title, start, end, kind, note FROM blocks WHERE start < ?2 AND end > ?1 ORDER BY start, id",
        )?;
        q.query_map(params![from, to], |r| {
            Ok(Block { id: r.get(0)?, title: r.get(1)?, start: r.get(2)?, end: r.get(3)?, kind: r.get(4)?, note: r.get(5)? })
        })?
        .collect()
    }

    /// Adds a block (id 0) or updates one, and returns it with its id. The caller tidies it first.
    pub fn save_block(&self, mut b: Block) -> Result<Block> {
        if b.id == 0 {
            self.0.execute(
                "INSERT INTO blocks (title, start, end, kind, note) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![b.title, b.start, b.end, b.kind, b.note],
            )?;
            b.id = self.0.last_insert_rowid();
        } else {
            self.0.execute(
                "UPDATE blocks SET title = ?2, start = ?3, end = ?4, kind = ?5, note = ?6 WHERE id = ?1",
                params![b.id, b.title, b.start, b.end, b.kind, b.note],
            )?;
        }
        Ok(b)
    }

    pub fn title(&self, id: i64) -> Result<Option<String>> {
        self.0.query_row("SELECT title FROM items WHERE id = ?1", [id], |r| r.get(0)).optional()
    }

    pub fn delete_block(&self, id: i64) -> Result<()> {
        self.0.execute("DELETE FROM blocks WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Today as a block range: ("YYYY-MM-DDT00:00", tomorrow "T00:00"), in local time.
    pub fn today_range(&self) -> Result<(String, String)> {
        self.0.query_row(
            "SELECT date('now', 'localtime') || 'T00:00', date('now', 'localtime', '+1 day') || 'T00:00'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// Now in local time, "YYYY-MM-DDTHH:MM", with the weekday in front for prompts ("Monday 2026-11-16T15:00").
    pub fn now(&self) -> Result<(String, String)> {
        let (weekday, now): (i64, String) = self.0.query_row(
            "SELECT CAST(strftime('%w', 'now', 'localtime') AS INTEGER), strftime('%Y-%m-%dT%H:%M', 'now', 'localtime')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let days = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
        Ok((now.clone(), format!("{} {now}", days[weekday as usize])))
    }

    /// `t` ("YYYY-MM-DDTHH:MM") plus `minutes`, in the same format.
    pub fn add_minutes(&self, t: &str, minutes: i64) -> Result<String> {
        self.0.query_row("SELECT strftime('%Y-%m-%dT%H:%M', ?1, ?2)", params![t, format!("{minutes:+} minutes")], |r| r.get(0))
    }

    /// Reminders that are due and haven't been shown yet; marks them shown.
    pub fn take_due_reminders(&self, now: &str) -> Result<Vec<Block>> {
        let mut q = self.0.prepare(
            "UPDATE blocks SET fired = 1 WHERE kind = 'reminder' AND fired = 0 AND start <= ?1
             RETURNING id, title, start, end, kind, note",
        )?;
        q.query_map([now], |r| {
            Ok(Block { id: r.get(0)?, title: r.get(1)?, start: r.get(2)?, end: r.get(3)?, kind: r.get(4)?, note: r.get(5)? })
        })?
        .collect()
    }

    /// The day after `date` ("YYYY-MM-DD").
    pub fn day_after(&self, date: &str) -> Result<String> {
        self.0.query_row("SELECT date(?1, '+1 day')", [date], |r| r.get(0))
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
        let first = s.add_job(None, "first", "do one", "my app", "", "C:\\work").unwrap();
        let second = s.add_job(None, "second", "do two", "", "", "").unwrap();

        let job = Job {
            id: first,
            dump: None,
            prompt: "do one".into(),
            project: "my app".into(),
            tool: String::new(),
            folder: "C:\\work".into(),
        };
        assert_eq!(s.claim_job().unwrap(), Some(job));
        s.set_activity(first, "viewing", "Reading plan.md").unwrap();
        let item = &s.briefing().unwrap()[0];
        assert_eq!((item.process.as_str(), item.result.as_str()), ("viewing", "Reading plan.md"));

        assert!(s.cancel(first).unwrap());
        s.finish(first, "failed", "killed").unwrap(); // the run ending after Cancel changes nothing
        assert_eq!(s.claim_job().unwrap().map(|j| j.id), Some(second));
        assert_eq!(s.claim_job().unwrap(), None);

        s.finish(second, "failed", "oops").unwrap();
        s.retry(second).unwrap();
        assert_eq!(s.claim_job().unwrap().map(|j| j.id), Some(second));
        let titles: Vec<_> = s.briefing().unwrap().into_iter().map(|i| i.title).collect();
        assert_eq!(titles, ["second"]);

        s.finish(second, "done", "did it").unwrap();
        s.set_verdict(second, "concern", "Skipped part two.").unwrap();
        let item = &s.briefing().unwrap()[0];
        assert_eq!((item.verdict.as_str(), item.verdict_reason.as_str()), ("concern", "Skipped part two."));
    }

    #[test]
    fn blocks_by_range() {
        let s = Store::open(Path::new(":memory:")).unwrap();
        let block = |t: &str, a: &str, b: &str| Block { title: t.into(), start: a.into(), end: b.into(), kind: "study".into(), ..Default::default() };
        let mon = s.save_block(block("Mon", "2026-11-16T09:00", "2026-11-16T10:00")).unwrap();
        s.save_block(block("Late", "2026-11-16T23:30", "2026-11-17T00:30")).unwrap();
        s.save_block(block("Wed", "2026-11-18T09:00", "2026-11-18T10:00")).unwrap();
        let titles = |from, to| s.blocks(from, to).unwrap().into_iter().map(|b| b.title).collect::<Vec<_>>();
        assert_eq!(titles("2026-11-17T00:00", "2026-11-18T00:00"), ["Late"]); // spans midnight
        assert_eq!(titles("2026-11-16T00:00", "2026-11-23T00:00"), ["Mon", "Late", "Wed"]);

        s.save_block(Block { title: "Monday".into(), ..mon.clone() }).unwrap();
        assert_eq!(titles("2026-11-16T00:00", "2026-11-16T12:00"), ["Monday"]);
        s.delete_block(mon.id).unwrap();
        assert!(titles("2026-11-16T00:00", "2026-11-16T12:00").is_empty());
    }

    #[test]
    fn reminders_fire_once() {
        let s = Store::open(Path::new(":memory:")).unwrap();
        assert_eq!(s.add_minutes("2026-11-16T23:58", 5).unwrap(), "2026-11-17T00:03");
        let r = |t: &str, at: &str| Block { title: t.into(), start: at.into(), end: s.add_minutes(at, 5).unwrap(), kind: "reminder".into(), ..Default::default() };
        s.save_block(r("Stretch", "2026-11-16T10:00")).unwrap();
        s.save_block(r("Call", "2026-11-16T18:00")).unwrap();
        let due: Vec<_> = s.take_due_reminders("2026-11-16T12:00").unwrap().into_iter().map(|b| b.title).collect();
        assert_eq!(due, ["Stretch"]);
        assert!(s.take_due_reminders("2026-11-16T12:00").unwrap().is_empty()); // only once
        assert!(s.now().unwrap().0.contains('T'));
    }

    #[test]
    fn held_cards_are_taken_once() {
        let s = Store::open(Path::new(":memory:")).unwrap();
        let card = s.add_held("outward", None, "Send reply to Sam", "To: Sam\n\nYes!", "mcp__x__send", "", "").unwrap();
        let item = &s.briefing().unwrap()[0];
        assert_eq!((item.kind.as_str(), item.status.as_str(), item.detail.as_str()), ("outward", "held", "To: Sam\n\nYes!"));

        let held = s.take_held(card, "done").unwrap().unwrap();
        assert_eq!((held.title.as_str(), held.tool.as_str()), ("Send reply to Sam", "mcp__x__send"));
        assert_eq!(s.take_held(card, "done").unwrap(), None); // a second click does nothing
    }
}
