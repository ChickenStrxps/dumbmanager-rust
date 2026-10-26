//! SQLite persistence for itches and settings.
//!
//! - Every write runs in one `BEGIN IMMEDIATE` transaction: it fully happens
//!   or not at all, and concurrent writers wait (busy timeout) instead of
//!   interleaving.
//! - WAL journal with `synchronous=FULL`: a committed write survives a crash
//!   or power loss.
//! - The file is stamped with this app's `application_id`. A database that
//!   belongs to anything else (for example the original DumbManager's
//!   `manager.sqlite`) is refused before a single byte is written to it.
//! - Lists are ordered by a monotonic write revision, so "newest update
//!   first" holds even when two writes share a millisecond or the clock jumps.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{
    params, params_from_iter, Connection, OptionalExtension, Row, Transaction, TransactionBehavior,
};

use crate::error::{CoreError, CoreResult};
use crate::model::{
    check_transition, Itch, ItchEvent, ItchInput, ItchKind, ItchStatus, ListFilter,
};
use crate::settings::{Motion, Settings, SettingsInput, Theme};
use crate::snapshot::StatusCounts;
use crate::time;
use crate::validate;

/// `PRAGMA application_id` for DumbManager Rust databases ("DMRS").
pub const APPLICATION_ID: i32 = 0x444D_5253;
/// Current schema version (`PRAGMA user_version`).
pub const SCHEMA_VERSION: i32 = 2;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

const SCHEMA_V1: &str = "
CREATE TABLE itches (
    id          TEXT PRIMARY KEY NOT NULL,
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    project     TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('bug', 'improvement', 'idea')),
    status      TEXT NOT NULL CHECK (status IN ('open', 'stashed', 'ready', 'resolved')),
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    revision    INTEGER NOT NULL UNIQUE
) STRICT;
CREATE INDEX itches_status ON itches (status, revision);
CREATE INDEX itches_project ON itches (project, revision);

CREATE TABLE itch_events (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    itch_id     TEXT NOT NULL REFERENCES itches (id) ON DELETE CASCADE,
    at          TEXT NOT NULL,
    action      TEXT NOT NULL CHECK (action IN ('created', 'edited', 'status')),
    from_status TEXT,
    to_status   TEXT
) STRICT;
CREATE INDEX itch_events_itch ON itch_events (itch_id, seq);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;
";

const ITCH_COLUMNS: &str = "id, title, description, project, kind, status, created_at, updated_at";

const KEY_MOTION: &str = "motion";
const KEY_THEME: &str = "theme";
const KEY_DEFAULT_PROJECT: &str = "default_project";
const KEY_UPDATED_AT: &str = "updated_at";

fn itch_not_found() -> CoreError {
    CoreError::not_found("That itch no longer exists. Refresh the list.")
}

pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Store {
    /// Opens (or creates) the database at `path`. The parent folder must exist.
    pub fn open(path: &Path) -> CoreResult<Self> {
        if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(CoreError::invalid(
                "The database file is a link to another location. DumbManager Rust only opens its own database file.",
            ));
        }
        // Changing journal mode during simultaneous first opens can return
        // SQLITE_BUSY immediately without invoking SQLite's busy handler.
        // Reopen/recheck ownership on each bounded retry; never retry invalid
        // input, foreign ownership, or a schema error.
        let deadline = Instant::now() + BUSY_TIMEOUT;
        loop {
            match Self::try_open(path) {
                Err(CoreError::Busy(_)) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => return result,
            }
        }
    }

    fn try_open(path: &Path) -> CoreResult<Self> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        // Read-only ownership check first: nothing below may run on a foreign file.
        // Keep the ID/version/schema reads in one snapshot. Otherwise another
        // first opener can commit between the ID and table-count queries,
        // making a newly initialized database falsely appear foreign.
        {
            let read = conn.transaction()?;
            refuse_foreign(&read)?;
            check_schema_version(&read)?;
            read.commit()?;
        }
        let journal: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        if !journal.eq_ignore_ascii_case("wal") {
            return Err(CoreError::Storage(format!(
                "could not enable the write-ahead log (journal mode is {journal})."
            )));
        }
        conn.execute_batch("PRAGMA synchronous = FULL; PRAGMA foreign_keys = ON;")?;
        let mut store = Self {
            conn,
            path: path.to_path_buf(),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Folds the write-ahead log back into the main file, so the `.sqlite3`
    /// file alone is a complete copy (for example, for a manual backup).
    /// Committed data is already durable without this; the app calls it on
    /// a clean exit.
    pub fn checkpoint(&self) -> CoreResult<()> {
        let busy: i64 = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
        if busy != 0 {
            return Err(CoreError::Busy(
                "Another connection is using the database; the log was not folded in yet."
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn migrate(&mut self) -> CoreResult<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Re-check under the write lock: another process may have initialised it meanwhile.
        refuse_foreign(&tx)?;
        let version = check_schema_version(&tx)?;
        if version < 1 {
            tx.execute_batch(SCHEMA_V1)?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        }
        if version < 2 {
            crate::legacy::migrate(&tx)?;
        }
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn write_tx(&mut self) -> CoreResult<Transaction<'_>> {
        Ok(self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }

    // ----- itches -------------------------------------------------------

    pub fn get_itch(&self, id: &str) -> CoreResult<Itch> {
        let id = validate::itch_id(id)?;
        load_itch(&self.conn, &id)?.ok_or_else(itch_not_found)
    }

    /// Explicit headless CRUD deletion, with the removed record returned to
    /// the caller. Its local history is removed by the foreign-key cascade in
    /// the same transaction. No desktop IPC command exposes deletion in the
    /// first milestone, and no background operation calls this method.
    pub fn delete_itch(&mut self, id: &str) -> CoreResult<Itch> {
        let id = validate::itch_id(id)?;
        let tx = self.write_tx()?;
        let removed = load_itch(&tx, &id)?.ok_or_else(itch_not_found)?;
        tx.execute("DELETE FROM itches WHERE id = ?1", [&id])?;
        tx.commit()?;
        Ok(removed)
    }

    /// Itches matching `filter`, newest update first.
    pub fn list_itches(&self, filter: &ListFilter) -> CoreResult<Vec<Itch>> {
        let mut sql = format!("SELECT {ITCH_COLUMNS} FROM itches WHERE 1 = 1");
        let mut args: Vec<&str> = Vec::new();
        if let Some(status) = filter.status {
            sql.push_str(" AND status = ?");
            args.push(status.as_str());
        }
        if let Some(project) = &filter.project {
            sql.push_str(" AND project = ?");
            args.push(project.as_str());
        }
        sql.push_str(" ORDER BY revision DESC");
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(args), itch_from_row)?;
        let mut itches = Vec::new();
        for row in rows {
            let itch = row?;
            if filter.matches_text(&itch) {
                itches.push(itch);
            }
        }
        Ok(itches)
    }

    /// Creates or edits an itch (see [`ItchInput`] for the rules).
    /// Validation happens before the transaction starts; nothing is written on error.
    pub fn save_itch(&mut self, input: &ItchInput) -> CoreResult<Itch> {
        let id = match input.id.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(raw) => Some(validate::itch_id(raw)?),
        };
        let title = validate::title(input.title.as_deref().unwrap_or(""))?;
        let description = input
            .description
            .as_deref()
            .map(validate::description)
            .transpose()?;
        let project = match input.project.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(raw) => Some(validate::project(raw)?),
        };
        let kind = input.kind.as_deref().map(ItchKind::parse).transpose()?;

        let tx = self.write_tx()?;
        let now = time::now_iso();
        let saved = match id {
            None => {
                let kind = kind.ok_or_else(|| {
                    CoreError::invalid("Choose a kind: bug, improvement or idea.")
                })?;
                let project = match project {
                    Some(project) => project,
                    None => {
                        let fallback = load_settings(&tx)?.default_project;
                        if fallback.is_empty() {
                            validate::FALLBACK_PROJECT.to_owned()
                        } else {
                            fallback
                        }
                    }
                };
                let itch = Itch {
                    id: uuid::Uuid::new_v4().hyphenated().to_string(),
                    title,
                    description: description.unwrap_or_default(),
                    project,
                    kind,
                    status: ItchStatus::Open,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                };
                let revision = next_revision(&tx)?;
                tx.execute(
                    "INSERT INTO itches (id, title, description, project, kind, status, created_at, updated_at, revision)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        itch.id,
                        itch.title,
                        itch.description,
                        itch.project,
                        itch.kind.as_str(),
                        itch.status.as_str(),
                        itch.created_at,
                        itch.updated_at,
                        revision
                    ],
                )?;
                record_event(&tx, &itch.id, &now, "created", None, Some(itch.status))?;
                itch
            }
            Some(id) => {
                let existing = load_itch(&tx, &id)?.ok_or_else(itch_not_found)?;
                let mut next = existing.clone();
                next.title = title;
                if let Some(description) = description {
                    next.description = description;
                }
                if let Some(project) = project {
                    next.project = project;
                }
                if let Some(kind) = kind {
                    next.kind = kind;
                }
                if next == existing {
                    // Identical save (e.g. a retry): keep the timestamp and order.
                    return Ok(existing);
                }
                next.updated_at = now.clone();
                let revision = next_revision(&tx)?;
                tx.execute(
                    "UPDATE itches SET title = ?2, description = ?3, project = ?4, kind = ?5, updated_at = ?6, revision = ?7
                     WHERE id = ?1",
                    params![
                        next.id,
                        next.title,
                        next.description,
                        next.project,
                        next.kind.as_str(),
                        next.updated_at,
                        revision
                    ],
                )?;
                record_event(&tx, &next.id, &now, "edited", None, None)?;
                next
            }
        };
        crate::legacy::sync_native(&tx, &saved)?;
        tx.commit()?;
        Ok(saved)
    }

    /// Moves an itch to `status` following [`check_transition`]. Moving to the
    /// current status returns the itch unchanged.
    pub fn transition_itch(&mut self, id: &str, status: &str) -> CoreResult<Itch> {
        let id = validate::itch_id(id)?;
        let target = ItchStatus::parse(status)?;
        let tx = self.write_tx()?;
        let mut itch = load_itch(&tx, &id)?.ok_or_else(itch_not_found)?;
        if itch.status == target {
            return Ok(itch);
        }
        check_transition(itch.status, target)?;
        let from = itch.status;
        let now = time::now_iso();
        itch.status = target;
        itch.updated_at = now.clone();
        let revision = next_revision(&tx)?;
        tx.execute(
            "UPDATE itches SET status = ?2, updated_at = ?3, revision = ?4 WHERE id = ?1",
            params![itch.id, itch.status.as_str(), itch.updated_at, revision],
        )?;
        record_event(&tx, &itch.id, &now, "status", Some(from), Some(target))?;
        crate::legacy::sync_native(&tx, &itch)?;
        tx.commit()?;
        Ok(itch)
    }

    /// Local audit trail for one itch, oldest first.
    pub fn itch_history(&self, id: &str) -> CoreResult<Vec<ItchEvent>> {
        let id = validate::itch_id(id)?;
        let mut statement = self.conn.prepare(
            "SELECT at, action, from_status, to_status FROM itch_events WHERE itch_id = ?1 ORDER BY seq",
        )?;
        let rows = statement.query_map([&id], |row| {
            let from: Option<String> = row.get(2)?;
            let to: Option<String> = row.get(3)?;
            Ok(ItchEvent {
                at: row.get(0)?,
                action: row.get(1)?,
                from_status: from.as_deref().and_then(|s| ItchStatus::parse(s).ok()),
                to_status: to.as_deref().and_then(|s| ItchStatus::parse(s).ok()),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn counts(&self) -> CoreResult<StatusCounts> {
        let mut counts = StatusCounts::default();
        let mut statement = self
            .conn
            .prepare("SELECT status, COUNT(*) FROM itches GROUP BY status")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (status, count) = row?;
            let count = u64::try_from(count).unwrap_or(0);
            match ItchStatus::parse(&status) {
                Ok(ItchStatus::Open) => counts.open = count,
                Ok(ItchStatus::Stashed) => counts.stashed = count,
                Ok(ItchStatus::Ready) => counts.ready = count,
                Ok(ItchStatus::Resolved) => counts.resolved = count,
                Err(_) => {}
            }
            counts.total += count;
        }
        Ok(counts)
    }

    // ----- settings -----------------------------------------------------

    pub fn settings(&self) -> CoreResult<Settings> {
        load_settings(&self.conn)
    }

    /// Validates the whole input first, then writes every changed key in one
    /// transaction. An unchanged save writes nothing.
    pub fn save_settings(&mut self, input: &SettingsInput) -> CoreResult<Settings> {
        let motion = input.motion.as_deref().map(Motion::parse).transpose()?;
        let theme = input.theme.as_deref().map(Theme::parse).transpose()?;
        let default_project = input
            .default_project
            .as_deref()
            .map(validate::default_project)
            .transpose()?;

        let tx = self.write_tx()?;
        let current = load_settings(&tx)?;
        let mut next = current.clone();
        if let Some(motion) = motion {
            next.motion = motion;
        }
        if let Some(theme) = theme {
            next.theme = theme;
        }
        if let Some(default_project) = default_project {
            next.default_project = default_project;
        }
        if next == current {
            return Ok(current);
        }
        let now = time::now_iso();
        next.updated_at = Some(now.clone());
        for (key, value) in [
            (KEY_MOTION, next.motion.as_str().to_owned()),
            (KEY_THEME, next.theme.as_str().to_owned()),
            (KEY_DEFAULT_PROJECT, next.default_project.clone()),
            (KEY_UPDATED_AT, now),
        ] {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        tx.commit()?;
        Ok(next)
    }
}

fn check_schema_version(conn: &Connection) -> CoreResult<i32> {
    let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(CoreError::Storage(format!(
            "this database uses schema version {version}, newer than this build supports ({SCHEMA_VERSION}). Update DumbManager Rust; the file was left untouched."
        )));
    }
    Ok(version)
}

/// Refuses databases that are not empty and not stamped as ours.
fn refuse_foreign(conn: &Connection) -> CoreResult<()> {
    let application_id: i32 = conn.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    if application_id == APPLICATION_ID {
        return Ok(());
    }
    if application_id != 0 {
        return Err(CoreError::ForeignDatabase(
            "This database file belongs to a different application. DumbManager Rust left it untouched.".to_owned(),
        ));
    }
    let objects: i64 =
        conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))?;
    if objects > 0 {
        return Err(CoreError::ForeignDatabase(
            "This database file already holds another application's data. DumbManager Rust left it untouched.".to_owned(),
        ));
    }
    Ok(())
}

fn itch_from_row(row: &Row<'_>) -> rusqlite::Result<Itch> {
    let kind: String = row.get(4)?;
    let status: String = row.get(5)?;
    let bad = |column: usize, what: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            format!("unexpected {what} in the database").into(),
        )
    };
    Ok(Itch {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        project: row.get(3)?,
        kind: ItchKind::parse(&kind).map_err(|_| bad(4, "kind"))?,
        status: ItchStatus::parse(&status).map_err(|_| bad(5, "status"))?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

pub(crate) fn load_itch(conn: &Connection, id: &str) -> CoreResult<Option<Itch>> {
    Ok(conn
        .query_row(
            &format!("SELECT {ITCH_COLUMNS} FROM itches WHERE id = ?1"),
            [id],
            itch_from_row,
        )
        .optional()?)
}

pub(crate) fn next_revision(conn: &Connection) -> CoreResult<i64> {
    // sqlite_sequence survives explicit deletion and cascading event cleanup,
    // so subsequent mutations keep increasing even after the newest item is
    // deleted. Existing row revisions remain a lower bound as well.
    Ok(conn.query_row(
        "SELECT MAX(COALESCE((SELECT MAX(revision) FROM itches), 0),
                    COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'itch_events'), 0)) + 1",
        [],
        |row| row.get(0),
    )?)
}

pub(crate) fn record_event(
    conn: &Connection,
    itch_id: &str,
    at: &str,
    action: &str,
    from: Option<ItchStatus>,
    to: Option<ItchStatus>,
) -> CoreResult<()> {
    conn.execute(
        "INSERT INTO itch_events (itch_id, at, action, from_status, to_status) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![itch_id, at, action, from.map(ItchStatus::as_str), to.map(ItchStatus::as_str)],
    )?;
    Ok(())
}

/// Reads settings; a damaged or unknown stored value falls back to its default
/// instead of breaking the app.
pub(crate) fn load_settings(conn: &Connection) -> CoreResult<Settings> {
    let mut statement = conn.prepare("SELECT key, value FROM settings")?;
    let stored: HashMap<String, String> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let get = |key: &str| stored.get(key).map(String::as_str);
    Ok(Settings {
        motion: get(KEY_MOTION)
            .and_then(|v| Motion::parse(v).ok())
            .unwrap_or_default(),
        theme: get(KEY_THEME)
            .and_then(|v| Theme::parse(v).ok())
            .unwrap_or_default(),
        default_project: get(KEY_DEFAULT_PROJECT)
            .and_then(|v| validate::default_project(v).ok())
            .unwrap_or_default(),
        updated_at: get(KEY_UPDATED_AT).map(str::to_owned),
    })
}
