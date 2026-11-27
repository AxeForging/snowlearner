//! Practice history in SQLite: every attempt, plus small key/value meta
//! (e.g. the last day the recap was shown). Since #220 an attempt also keeps
//! how it was asked (exercise, question, tier said, try, mode), which the
//! question schedule replays; a history from before gains those columns with
//! defaults when it opens.

use crate::learn::deck::Answer;
use crate::learn::picker::{Exercise, Mode, PhraseStats};
use crate::learn::schedule::QuestionAttempt;
use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDate};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Attempt {
    pub at: DateTime<Local>,
    pub language: String,
    pub say: String,
    pub meaning: String,
    pub heard: String,
    pub score: f32,
    pub success: bool,
}

/// How an attempt was asked (#220). The default is a phrase attempt from
/// before these were kept: first try, nothing else known.
#[derive(Debug, Clone, PartialEq)]
pub struct Answered {
    pub exercise: Exercise,
    /// The question answered, in the question exercise.
    pub ask: Option<String>,
    /// The tier said on a right answer.
    pub tier: Option<Answer>,
    /// 1 for the first try at a question.
    pub try_number: u32,
    pub mode: Option<Mode>,
}

impl Default for Answered {
    fn default() -> Answered {
        Answered { exercise: Exercise::Phrases, ask: None, tier: None, try_number: 1, mode: None }
    }
}

/// Columns #220 added, with the type and default an old history gets.
const ANSWERED_COLUMNS: [(&str, &str); 5] = [
    ("exercise", "TEXT NOT NULL DEFAULT 'phrases'"),
    ("ask", "TEXT"),
    ("tier", "TEXT"),
    ("try_number", "INTEGER NOT NULL DEFAULT 1"),
    ("mode", "TEXT"),
];

fn tier_key(t: Answer) -> &'static str {
    match t {
        Answer::Short => "short",
        Answer::All | Answer::Complete => "complete",
        Answer::Polished => "polished",
    }
}

fn tier_of(s: &str) -> Option<Answer> {
    match s {
        "short" => Some(Answer::Short),
        "complete" => Some(Answer::Complete),
        "polished" => Some(Answer::Polished),
        _ => None,
    }
}

fn mode_key(m: Mode) -> &'static str {
    match m {
        Mode::Repeat => "repeat",
        Mode::Recall => "recall",
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DayPhrase {
    pub say: String,
    pub meaning: String,
    pub attempts: u32,
    pub successes: u32,
    pub best_score: f32,
}

pub struct History {
    conn: Connection,
}

impl History {
    /// Opens (or creates) the history. A damaged file is kept next to it as
    /// `<name>.corrupt` and a fresh history starts, rather than the app not
    /// starting at all.
    pub fn open(path: &Path) -> Result<History> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        match Self::open_intact(path) {
            Err(e) if damaged(&e) => {
                let aside = path.with_extension("sqlite3.corrupt");
                for side in ["", "-wal", "-shm"] {
                    let (mut from, mut to) = (path.as_os_str().to_owned(), aside.as_os_str().to_owned());
                    from.push(side);
                    to.push(side);
                    if std::path::Path::new(&from).exists() {
                        std::fs::rename(&from, &to).with_context(|| format!("moving aside {}", path.display()))?;
                    }
                }
                eprintln!("History was damaged ({e:#}); kept it as {} and started a new one.", aside.display());
                Self::open_intact(path)
            }
            opened => opened,
        }
    }

    fn open_intact(path: &Path) -> Result<History> {
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(Damaged(check).into());
        }
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS attempts (
                 id INTEGER PRIMARY KEY,
                 at TEXT NOT NULL,
                 day TEXT NOT NULL,
                 language TEXT NOT NULL,
                 say TEXT NOT NULL,
                 meaning TEXT NOT NULL,
                 heard TEXT NOT NULL,
                 score REAL NOT NULL,
                 success INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS attempts_day ON attempts(language, day);
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        let have: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('attempts')")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for (name, kind) in ANSWERED_COLUMNS {
            if !have.iter().any(|h| h == name) {
                conn.execute_batch(&format!("ALTER TABLE attempts ADD COLUMN {name} {kind};"))?;
            }
        }
        Ok(History { conn })
    }

    /// Records a phrase attempt asked the way attempts were before #220.
    pub fn record(&self, a: &Attempt) -> Result<()> {
        self.record_answer(a, &Answered::default())
    }

    /// Records an attempt with how it was asked.
    pub fn record_answer(&self, a: &Attempt, how: &Answered) -> Result<()> {
        self.conn.execute(
            "INSERT INTO attempts (at, day, language, say, meaning, heard, score, success,
                                   exercise, ask, tier, try_number, mode)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                a.at.to_rfc3339(),
                a.at.date_naive().to_string(),
                a.language,
                a.say,
                a.meaning,
                a.heard,
                a.score,
                a.success,
                how.exercise.key(),
                how.ask,
                how.tier.map(tier_key),
                how.try_number,
                how.mode.map(mode_key),
            ],
        )?;
        Ok(())
    }

    /// Every answer to a question in `language`, oldest first, for the
    /// question schedule (`learn::schedule::cards`).
    pub fn question_log(&self, language: &str) -> Result<Vec<QuestionAttempt>> {
        let mut stmt = self.conn.prepare(
            "SELECT ask, day, success, tier, try_number, mode FROM attempts
             WHERE language = ?1 AND exercise = 'questions' AND ask IS NOT NULL ORDER BY id",
        )?;
        let rows = stmt.query_map(params![language], |r| {
            let day: String = r.get(1)?;
            let tier: Option<String> = r.get(3)?;
            let mode: Option<String> = r.get(5)?;
            Ok((r.get::<_, String>(0)?, day, r.get::<_, bool>(2)?, tier, r.get::<_, u32>(4)?, mode))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (ask, day, success, tier, try_number, mode) = row?;
            let Ok(day) = day.parse::<NaiveDate>() else { continue };
            let mode = if mode.as_deref() == Some("recall") { Mode::Recall } else { Mode::Repeat };
            out.push(QuestionAttempt { ask, day, success, tier: tier.as_deref().and_then(tier_of), try_number, mode });
        }
        Ok(out)
    }

    /// Phrases practiced on `day`, in the order they were first tried.
    pub fn day_summary(&self, language: &str, day: NaiveDate) -> Result<Vec<DayPhrase>> {
        let mut stmt = self.conn.prepare(
            "SELECT say, meaning, COUNT(*), SUM(success), MAX(score)
             FROM attempts WHERE language = ?1 AND day = ?2
             GROUP BY say ORDER BY MIN(id)",
        )?;
        let rows = stmt.query_map(params![language, day.to_string()], |r| {
            Ok(DayPhrase {
                say: r.get(0)?,
                meaning: r.get(1)?,
                attempts: r.get(2)?,
                successes: r.get(3)?,
                best_score: r.get::<_, f64>(4)? as f32,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn stats(&self, language: &str, today: NaiveDate) -> Result<HashMap<String, PhraseStats>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.say,
                    SUM(CASE WHEN a.day = ?2 THEN a.success ELSE 0 END),
                    SUM(a.success),
                    (SELECT b.success FROM attempts b
                      WHERE b.language = ?1 AND b.say = a.say ORDER BY b.id DESC LIMIT 1)
             FROM attempts a WHERE a.language = ?1 GROUP BY a.say",
        )?;
        let rows = stmt.query_map(params![language, today.to_string()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                PhraseStats {
                    successes_today: r.get(1)?,
                    successes_total: r.get(2)?,
                    last_failed: !r.get::<_, bool>(3)?,
                },
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

/// `PRAGMA quick_check` found damage.
#[derive(Debug)]
struct Damaged(String);

impl std::fmt::Display for Damaged {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "integrity check: {}", self.0)
    }
}

impl std::error::Error for Damaged {}

/// Only real damage moves the file aside — never a lock or a full disk.
fn damaged(e: &anyhow::Error) -> bool {
    use rusqlite::ErrorCode::{DatabaseCorrupt, NotADatabase};
    e.chain().any(|c| {
        c.is::<Damaged>()
            || matches!(
                c.downcast_ref::<rusqlite::Error>().and_then(rusqlite::Error::sqlite_error_code),
                Some(DatabaseCorrupt | NotADatabase)
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(day: u32, hour: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 9, day, hour, 0, 0).unwrap()
    }

    fn attempt(day: u32, hour: u32, lang: &str, say: &str, score: f32, success: bool) -> Attempt {
        Attempt {
            at: at(day, hour),
            language: lang.into(),
            say: say.into(),
            meaning: format!("m:{say}"),
            heard: say.to_lowercase(),
            score,
            success,
        }
    }

    fn db() -> (tempfile::TempDir, History) {
        let dir = tempfile::tempdir().unwrap();
        let h = History::open(&dir.path().join("nested/h.sqlite3")).unwrap();
        (dir, h)
    }

    #[test]
    fn day_summary_groups_attempts_per_phrase_in_first_seen_order() {
        let (_d, h) = db();
        h.record(&attempt(27, 9, "en", "I'm hungry", 0.4, false)).unwrap();
        h.record(&attempt(27, 10, "en", "Good morning", 1.0, true)).unwrap();
        h.record(&attempt(27, 11, "en", "I'm hungry", 0.9, true)).unwrap();

        let s = h.day_summary("en", at(27, 0).date_naive()).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].say, "I'm hungry");
        assert_eq!((s[0].attempts, s[0].successes), (2, 1));
        assert!((s[0].best_score - 0.9).abs() < 1e-6);
        assert_eq!(s[1].say, "Good morning");
    }

    #[test]
    fn day_summary_excludes_other_days_and_languages() {
        let (_d, h) = db();
        h.record(&attempt(26, 23, "en", "yesterday", 1.0, true)).unwrap();
        h.record(&attempt(27, 9, "es", "Hola", 1.0, true)).unwrap();
        assert!(h.day_summary("en", at(27, 0).date_naive()).unwrap().is_empty());
    }

    #[test]
    fn stats_split_today_from_all_time() {
        let (_d, h) = db();
        h.record(&attempt(20, 9, "en", "Hi", 1.0, true)).unwrap();
        h.record(&attempt(27, 9, "en", "Hi", 1.0, true)).unwrap();
        h.record(&attempt(27, 10, "en", "Hi", 0.1, false)).unwrap();
        let s = h.stats("en", at(27, 0).date_naive()).unwrap();
        assert_eq!(s["Hi"], PhraseStats { successes_today: 1, successes_total: 2, last_failed: true });
        h.record(&attempt(27, 11, "en", "Hi", 1.0, true)).unwrap();
        assert!(!h.stats("en", at(27, 0).date_naive()).unwrap()["Hi"].last_failed, "fixed by the latest try");
    }

    #[test]
    fn a_question_answer_keeps_how_it_was_asked() {
        let (_d, h) = db();
        let how = Answered {
            exercise: Exercise::Questions,
            ask: Some("Ready?".into()),
            tier: Some(Answer::Short),
            try_number: 2,
            mode: Some(Mode::Recall),
        };
        h.record_answer(&attempt(27, 9, "en", "Yes, I'm ready.", 1.0, true), &how).unwrap();
        h.record(&attempt(27, 10, "en", "Hello.", 1.0, true)).unwrap();
        let log = h.question_log("en").unwrap();
        assert_eq!(
            log,
            vec![QuestionAttempt {
                ask: "Ready?".into(),
                day: at(27, 0).date_naive(),
                success: true,
                tier: Some(Answer::Short),
                try_number: 2,
                mode: Mode::Recall,
            }],
            "phrase attempts stay out of the question log"
        );
        assert_eq!(h.stats("en", at(27, 0).date_naive()).unwrap().len(), 2, "both still count for the phrases");
    }

    #[test]
    fn a_history_from_before_the_question_columns_opens_and_keeps_its_stats() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE attempts (id INTEGER PRIMARY KEY, at TEXT NOT NULL, day TEXT NOT NULL,
                     language TEXT NOT NULL, say TEXT NOT NULL, meaning TEXT NOT NULL, heard TEXT NOT NULL,
                     score REAL NOT NULL, success INTEGER NOT NULL);
                 INSERT INTO attempts (at, day, language, say, meaning, heard, score, success)
                     VALUES ('2026-09-27T09:00:00-03:00', '2026-09-27', 'en', 'Water.', 'Água.', 'water', 1.0, 1);",
            )
            .unwrap();
        }
        let h = History::open(&path).unwrap();
        let s = h.stats("en", at(27, 0).date_naive()).unwrap();
        assert_eq!(s["Water."], PhraseStats { successes_today: 1, successes_total: 1, last_failed: false });
        assert!(h.question_log("en").unwrap().is_empty(), "an old attempt is a phrase attempt");
        h.record_answer(
            &attempt(27, 11, "en", "Water.", 1.0, true),
            &Answered { exercise: Exercise::Questions, ask: Some("Anything to drink?".into()), ..Answered::default() },
        )
        .unwrap();
        drop(h);
        let h = History::open(&path).expect("opens again once migrated");
        assert_eq!(h.question_log("en").unwrap().len(), 1);
        assert_eq!(h.stats("en", at(27, 0).date_naive()).unwrap()["Water."].successes_total, 2);
    }

    #[test]
    fn meta_upserts_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.sqlite3");
        {
            let h = History::open(&path).unwrap();
            assert_eq!(h.meta("last_summary").unwrap(), None);
            h.set_meta("last_summary", "2026-09-26").unwrap();
            h.set_meta("last_summary", "2026-09-27").unwrap();
        }
        let h = History::open(&path).unwrap();
        assert_eq!(h.meta("last_summary").unwrap().as_deref(), Some("2026-09-27"));
    }

    #[test]
    fn a_corrupted_history_is_set_aside_and_a_fresh_one_opens() {
        // What stderr written into the file did on Windows: page 2 overwritten with log text.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        History::open(&path).unwrap().record(&attempt(27, 10, "en", "Hello.", 1.0, true)).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        let junk = b"Global hotkeys unavailable: could not register Ctrl+Alt+G (taken by another app?)\r\n";
        for (i, b) in bytes[4096..8192].iter_mut().enumerate() {
            *b = junk[i % junk.len()];
        }
        std::fs::write(&path, &bytes).unwrap();

        let h = History::open(&path).expect("a broken history must not keep the app from starting");
        h.record(&attempt(27, 11, "en", "Hi", 1.0, true)).unwrap();
        assert_eq!(h.day_summary("en", at(27, 0).date_naive()).unwrap().len(), 1, "starts over");
        assert_eq!(std::fs::read(dir.path().join("history.sqlite3.corrupt")).unwrap(), bytes, "old file kept as is");
    }

    #[test]
    fn a_file_that_is_not_a_database_is_set_aside_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        std::fs::write(&path, "Hotkey unavailable: Ctrl+Alt+G\r\n".repeat(200)).unwrap();
        History::open(&path).unwrap().record(&attempt(27, 11, "en", "Hi", 1.0, true)).unwrap();
        assert!(dir.path().join("history.sqlite3.corrupt").exists());
    }

    #[test]
    fn a_healthy_history_is_never_moved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        History::open(&path).unwrap().record(&attempt(27, 10, "en", "Hello.", 1.0, true)).unwrap();
        let h = History::open(&path).unwrap();
        assert_eq!(h.day_summary("en", at(27, 0).date_naive()).unwrap().len(), 1);
        assert!(!dir.path().join("history.sqlite3.corrupt").exists());
    }
}
