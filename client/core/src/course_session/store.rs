use super::{
    ChallengeStatus, CommandRecoveryView, CourseProgress, CourseSessionError, RecentCourse,
    StepProgressRecord, UnitStateRecord, UnitStatus,
};
use crate::server::PlaybackInterruptionReason;
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

const RECENT_COURSE_LIMIT: i64 = 5;
const SCHEMA_VERSION: i64 = 12;

pub(super) fn load_recent_courses(
    connection: &Connection,
) -> Result<Vec<RecentCourse>, CourseSessionError> {
    connection.execute(
        "
        DELETE FROM recent_courses
        WHERE path NOT IN (
            SELECT path
            FROM recent_courses
            ORDER BY last_opened_at DESC, path ASC
            LIMIT ?1
        )
        ",
        [RECENT_COURSE_LIMIT],
    )?;
    let mut statement = connection.prepare(
        "
        SELECT path, id, title
        FROM recent_courses
        ORDER BY last_opened_at DESC, path ASC
        LIMIT ?1
        ",
    )?;
    let rows = statement.query_map([RECENT_COURSE_LIMIT], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    let mut recent = Vec::new();
    for row in rows {
        let (path, id, title) = row?;
        // Cheap availability check: opening the course still reports the real
        // package error. Loading every recent package here would read all Steps
        // and assets on each refresh.
        if Path::new(&path).join("manifest.json").is_file() {
            recent.push(RecentCourse { path, id, title });
        } else {
            connection.execute("DELETE FROM recent_courses WHERE path = ?1", [&path])?;
        }
    }
    Ok(recent)
}

pub(super) fn setting(
    connection: &Connection,
    key: &str,
) -> Result<Option<String>, CourseSessionError> {
    Ok(connection
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?)
}

pub(super) fn clear_resume_cursor(
    connection: &Connection,
    course_id: &str,
    now: i64,
) -> Result<(), CourseSessionError> {
    connection.execute(
        "
        UPDATE course_progress
        SET current_unit_path = NULL, current_step_id = NULL, updated_at = ?1
        WHERE id = ?2
        ",
        rusqlite::params![now, course_id],
    )?;
    clear_resume_action(connection, course_id)?;
    Ok(())
}

fn resume_action_key(course_id: &str) -> String {
    format!("resume_action:{course_id}")
}

pub(super) fn load_resume_action(
    connection: &Connection,
    course_id: &str,
    unit_path: &str,
    step_id: &str,
) -> Result<Option<usize>, CourseSessionError> {
    let Some(value) = setting(connection, &resume_action_key(course_id))? else {
        return Ok(None);
    };
    let mut parts = value.split('\u{1f}');
    let (Some(unit), Some(step), Some(action)) = (parts.next(), parts.next(), parts.next()) else {
        return Ok(None);
    };
    if unit != unit_path || step != step_id {
        return Ok(None);
    }
    Ok(action.parse().ok())
}

pub(super) fn persist_resume_action(
    connection: &Connection,
    course_id: &str,
    unit_path: &str,
    step_id: &str,
    action_index: usize,
) -> Result<(), CourseSessionError> {
    let value = format!("{unit_path}\u{1f}{step_id}\u{1f}{action_index}");
    upsert_setting(connection, &resume_action_key(course_id), Some(&value))
}

pub(super) fn clear_resume_action(
    connection: &Connection,
    course_id: &str,
) -> Result<(), CourseSessionError> {
    upsert_setting(connection, &resume_action_key(course_id), None)
}

pub(super) fn persist_selection(
    connection: &Connection,
    unit_path: Option<&str>,
) -> Result<(), CourseSessionError> {
    upsert_setting(connection, "active_unit_path", unit_path)
}

pub(super) fn upsert_setting(
    connection: &Connection,
    key: &str,
    value: Option<&str>,
) -> Result<(), CourseSessionError> {
    match value {
        Some(value) => {
            connection.execute(
                "
                INSERT INTO settings(key, value)
                VALUES (?1, ?2)
                ON CONFLICT(key) DO UPDATE SET value = excluded.value
                ",
                [key, value],
            )?;
        }
        None => {
            connection.execute("DELETE FROM settings WHERE key = ?1", [key])?;
        }
    }
    Ok(())
}

pub(super) fn load_progress(
    connection: &Connection,
    id: &str,
) -> Result<Option<CourseProgress>, CourseSessionError> {
    let row = connection
        .query_row(
            "
            SELECT
                version, updated_at,
                current_unit_path, current_step_id
            FROM course_progress
            WHERE id = ?1
            ",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((version, updated_at, current_unit_path, current_step_id)) = row else {
        return Ok(None);
    };
    Ok(Some(CourseProgress {
        version,
        updated_at,
        current_unit_path,
        current_step_id,
        progress: load_step_progress(connection, id)?,
        recovery: load_command_recovery(connection, id)?,
        unit_states: load_unit_states(connection, id)?,
    }))
}

pub(super) fn unit_status_key(status: UnitStatus) -> &'static str {
    match status {
        UnitStatus::NotStarted => "not_started",
        UnitStatus::InProgress => "in_progress",
        UnitStatus::Paused => "paused",
        UnitStatus::Completed => "completed",
    }
}

fn parse_unit_status(value: &str) -> UnitStatus {
    match value {
        "completed" => UnitStatus::Completed,
        "paused" => UnitStatus::Paused,
        "in_progress" => UnitStatus::InProgress,
        _ => UnitStatus::NotStarted,
    }
}

pub(super) fn challenge_status_key(status: ChallengeStatus) -> &'static str {
    match status {
        ChallengeStatus::NotStarted => "not_started",
        ChallengeStatus::Failed => "failed",
        ChallengeStatus::Passed => "passed",
        ChallengeStatus::Skipped => "skipped",
    }
}

fn parse_challenge_status(value: &str) -> ChallengeStatus {
    match value {
        "failed" => ChallengeStatus::Failed,
        "passed" => ChallengeStatus::Passed,
        "skipped" => ChallengeStatus::Skipped,
        _ => ChallengeStatus::NotStarted,
    }
}

pub(super) fn migrate(connection: &Connection) -> Result<(), CourseSessionError> {
    connection.execute_batch(
        "
        PRAGMA foreign_keys = ON;
        PRAGMA journal_mode = WAL;
        PRAGMA busy_timeout = 5000;
        ",
    )?;
    let schema_version =
        connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?;
    if schema_version > SCHEMA_VERSION {
        return Err(CourseSessionError::SchemaTooNew(schema_version));
    }
    // shortcut: any schema change resets all course state, including settings
    // and recent courses. Add per-version migrations only when preserving
    // learner progress becomes a requirement.
    if schema_version != 0 && schema_version != SCHEMA_VERSION {
        log::warn!(
            target: "cynapse.session",
            "course state schema {schema_version} does not match {SCHEMA_VERSION}; \
             resetting all course state"
        );
        connection.execute_batch(
            "
            DROP TABLE IF EXISTS command_executions;
            DROP TABLE IF EXISTS command_confirmations;
            DROP TABLE IF EXISTS step_states;
            DROP TABLE IF EXISTS attempt_steps;
            DROP TABLE IF EXISTS unit_environment_checks;
            DROP TABLE IF EXISTS unit_states;
            DROP TABLE IF EXISTS terminal_records;
            DROP TABLE IF EXISTS course_progress;
            DROP TABLE IF EXISTS course_attempts;
            DROP TABLE IF EXISTS recent_courses;
            DROP TABLE IF EXISTS settings;
            ",
        )?;
    }
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS recent_courses (
            path TEXT PRIMARY KEY,
            id TEXT NOT NULL,
            title TEXT NOT NULL,
            last_opened_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS course_progress (
            id TEXT PRIMARY KEY,
            course_path TEXT NOT NULL,
            version TEXT NOT NULL,
            current_unit_path TEXT,
            current_step_id TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS unit_states (
            course_id TEXT NOT NULL REFERENCES course_progress(id) ON DELETE CASCADE,
            unit_path TEXT NOT NULL,
            status TEXT NOT NULL,
            current_step_id TEXT,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY(course_id, unit_path)
        );

        CREATE TABLE IF NOT EXISTS step_states (
            course_id TEXT NOT NULL REFERENCES course_progress(id) ON DELETE CASCADE,
            unit_path TEXT NOT NULL,
            step_id TEXT NOT NULL,
            started INTEGER NOT NULL DEFAULT 0,
            completed INTEGER NOT NULL DEFAULT 0,
            challenge_status TEXT NOT NULL DEFAULT 'not_started',
            challenge_attempts INTEGER NOT NULL DEFAULT 0,
            challenge_checked_at INTEGER,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY(course_id, unit_path, step_id)
        );

        DROP TABLE IF EXISTS terminal_records;

        CREATE TABLE IF NOT EXISTS command_executions (
            course_id TEXT NOT NULL REFERENCES course_progress(id) ON DELETE CASCADE,
            unit_path TEXT NOT NULL,
            step_id TEXT NOT NULL,
            action_index INTEGER NOT NULL,
            command TEXT NOT NULL,
            cwd TEXT NOT NULL,
            timeout_ms INTEGER NOT NULL,
            status TEXT NOT NULL,
            output TEXT,
            output_truncated INTEGER NOT NULL DEFAULT 0,
            exit_code INTEGER,
            interruption TEXT,
            started_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY(course_id, unit_path, step_id, action_index)
        );

        PRAGMA user_version = 12;
        ",
    )?;
    Ok(())
}

pub(super) fn recover_running_commands(
    connection: &Connection,
    now: i64,
) -> Result<(), CourseSessionError> {
    connection.execute(
        "
        UPDATE unit_states
        SET status = 'paused', updated_at = ?1
        WHERE (course_id, unit_path) IN (
            SELECT course_id, unit_path
            FROM command_executions
            WHERE status = 'running'
        )
        ",
        [now],
    )?;
    connection.execute(
        "
        UPDATE command_executions
        SET status = 'paused', interruption = 'forced_termination',
            updated_at = ?1
        WHERE status = 'running'
        ",
        [now],
    )?;
    Ok(())
}

fn load_command_recovery(
    connection: &Connection,
    id: &str,
) -> Result<Option<CommandRecoveryView>, CourseSessionError> {
    Ok(connection
        .query_row(
            "
            SELECT
                command.unit_path, command.step_id, command.action_index,
                command.command, command.output, command.output_truncated,
                command.exit_code, command.interruption
            FROM command_executions command
            JOIN course_progress progress ON progress.id = command.course_id
            WHERE command.course_id = ?1
              AND command.status = 'paused'
              AND command.unit_path = progress.current_unit_path
              AND command.step_id = progress.current_step_id
            LIMIT 1
            ",
            [id],
            |row| {
                Ok(CommandRecoveryView {
                    unit_path: row.get(0)?,
                    step_id: row.get(1)?,
                    action_index: row.get::<_, i64>(2)? as usize,
                    command: row.get(3)?,
                    reason: PlaybackInterruptionReason::from_storage(
                        &row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    ),
                    output: row.get(4)?,
                    output_truncated: row.get(5)?,
                    exit_code: row.get(6)?,
                })
            },
        )
        .optional()?)
}

fn load_step_progress(
    connection: &Connection,
    id: &str,
) -> Result<Vec<StepProgressRecord>, CourseSessionError> {
    let mut statement = connection.prepare(
        "
        SELECT unit_path, step_id, started, completed,
               challenge_status, challenge_attempts, challenge_checked_at
        FROM step_states
        WHERE course_id = ?1
        ORDER BY unit_path, step_id
        ",
    )?;
    let rows = statement.query_map([id], |row| {
        Ok(StepProgressRecord {
            unit_path: row.get(0)?,
            step_id: row.get(1)?,
            started: row.get(2)?,
            completed: row.get(3)?,
            challenge_status: parse_challenge_status(row.get::<_, String>(4)?.as_str()),
            challenge_attempts: row.get::<_, i64>(5)? as u32,
            challenge_checked_at: row.get::<_, Option<i64>>(6)?.map(|value| value as u64),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn load_unit_states(
    connection: &Connection,
    id: &str,
) -> Result<Vec<UnitStateRecord>, CourseSessionError> {
    let mut statement = connection.prepare(
        "
        SELECT unit_path, status, current_step_id, updated_at
        FROM unit_states
        WHERE course_id = ?1
        ORDER BY unit_path
        ",
    )?;
    let rows = statement.query_map([id], |row| {
        Ok(UnitStateRecord {
            unit_path: row.get(0)?,
            status: parse_unit_status(&row.get::<_, String>(1)?),
            current_step_id: row.get(2)?,
            updated_at: row.get::<_, i64>(3)? as u64,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}
