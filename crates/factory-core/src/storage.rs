//! SQLite persistence for tracked projects, stored as `bruk.db` in the Bruk folder.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension, params};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    bruk_home::{BrukHome, HomeError},
    chat::{ChatSession, ChatUsageDelta},
    github_intake::current_unix_seconds,
    model::{Project, ProjectId},
    preferences::{AppPreferences, ProviderKind},
    project_settings::ProjectSettings,
    secrets::{
        EnvScope, SecretId, SecretKind, SecretRecord, SecretSource, SecretState,
        SecretsMetadataSnapshot, Sharing, compute_preview, keychain_account_key,
        keychain_item_label, new_db_instance_id,
        redactor::SecretRedactor,
        store::{SecretStore, write_and_verify},
        validate_secret_value,
    },
    work_queue::ProjectWorkQueue,
    workflow_run::{RunExecutionState, WorkflowRunViewData},
};

/// Persisted record of a real workflow run stored in SQLite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedWorkflowRun {
    pub id: String,
    pub project_id: ProjectId,
    pub project_name: String,
    pub workflow_id: String,
    pub workflow_name: String,
    pub run_number: u32,
    pub status: RunExecutionState,
    pub provider: String,
    pub issue_number: Option<u32>,
    pub branch_name: String,
    pub total_tokens: u64,
    pub total_cost_cents: u64,
    pub duration_ms: u64,
    pub summary: String,
    pub view: WorkflowRunViewData,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

const PREFERENCES_KEY: &str = "app_preferences";
const SECRET_STORE_INSTANCE_ID_KEY: &str = "secret_store_instance_id";
const SECRET_STORE_LAST_PATH_KEY: &str = "secret_store_last_path";

/// Errors returned when opening or mutating the local SQLite project database.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("project name cannot be empty")]
    EmptyProjectName,
    #[error("repository path cannot be empty")]
    EmptyProjectPath,
    #[error("a project is already registered for repository path '{0}'")]
    DuplicateRepositoryPath(PathBuf),
    #[error("{0}")]
    Secret(String),
    #[error("{0}")]
    Home(#[from] HomeError),
    #[error("failed to create database directory '{path}': {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("preferences serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Persistent SQLite store for tracking registered projects on this machine.
pub struct ProjectDatabase {
    conn: Connection,
    path: Option<PathBuf>,
}

impl ProjectDatabase {
    /// Opens (creating if needed) `bruk.db` in a Bruk folder, creating the folder layout too.
    pub fn open_home(home: &BrukHome) -> Result<Self, StorageError> {
        home.ensure_layout()?;
        Self::open(home.database_path())
    }

    /// Copies the database at `source` into a new file at `destination` without modifying
    /// `source`. Uses `VACUUM INTO`, so pending WAL contents are included.
    pub fn copy_database(source: &Path, destination: &Path) -> Result<(), StorageError> {
        if let Some(parent) = destination.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|source| StorageError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let conn = Connection::open_with_flags(
            source,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.execute(
            "VACUUM INTO ?1",
            params![destination.to_string_lossy().into_owned()],
        )?;
        // Clear secret_store_instance_id, secret_store_last_path, and secret_tombstones in the
        // copied database so a cloned DB cannot mutate or delete the original DB's Keychain items.
        if let Ok(dest_conn) = Connection::open(destination) {
            let _ = dest_conn.execute(
                "DELETE FROM app_settings WHERE key IN (?1, ?2)",
                params![SECRET_STORE_INSTANCE_ID_KEY, SECRET_STORE_LAST_PATH_KEY],
            );
            let _ = dest_conn.execute("DELETE FROM secret_tombstones", []);
            let _ = dest_conn.execute(
                "UPDATE secrets SET state = 'needs_value' WHERE detail_json LIKE '%\"source\":\"keychain\"%'",
                [],
            );
        }
        Ok(())
    }

    /// Opens (creating parent directories and schema if needed) a SQLite database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|source| StorageError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let conn = Connection::open(&path)?;
        let db = Self {
            conn,
            path: Some(path.clone()),
        };
        db.init_schema()?;
        log::info!("db.open path={}", path.display());
        Ok(db)
    }

    /// Opens an ephemeral in-memory SQLite database (useful for tests).
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn, path: None };
        db.init_schema()?;
        Ok(db)
    }

    /// Returns the filesystem path of the SQLite database, if file-backed.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn init_schema(&self) -> Result<(), StorageError> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA secure_delete = ON;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS projects (
                 id             INTEGER PRIMARY KEY AUTOINCREMENT,
                 name           TEXT NOT NULL,
                 root_path      TEXT NOT NULL UNIQUE,
                 default_branch TEXT NOT NULL DEFAULT 'main',
                 description    TEXT NOT NULL DEFAULT '',
                 created_at     INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE TABLE IF NOT EXISTS app_settings (
                 key        TEXT PRIMARY KEY,
                 value_json TEXT NOT NULL,
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE TABLE IF NOT EXISTS project_settings (
                 project_id INTEGER PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
                 value_json TEXT NOT NULL,
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE TABLE IF NOT EXISTS project_work_queues (
                 project_id INTEGER PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
                 value_json TEXT NOT NULL,
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE TABLE IF NOT EXISTS secrets (
                 id           TEXT PRIMARY KEY,
                 project_id   INTEGER REFERENCES projects(id) ON DELETE CASCADE,
                 name         TEXT NOT NULL,
                 kind         TEXT NOT NULL,
                 state        TEXT NOT NULL,
                 detail_json  TEXT NOT NULL,
                 created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at   INTEGER NOT NULL DEFAULT (unixepoch()),
                 last_used_at INTEGER
             );
             CREATE UNIQUE INDEX IF NOT EXISTS secrets_app_env_name
                 ON secrets (name COLLATE NOCASE)
                 WHERE kind = 'env' AND project_id IS NULL;
             CREATE UNIQUE INDEX IF NOT EXISTS secrets_project_env_name
                 ON secrets (project_id, name COLLATE NOCASE)
                 WHERE kind = 'env' AND project_id IS NOT NULL;
             CREATE TABLE IF NOT EXISTS secret_tombstones (
                 id          TEXT PRIMARY KEY,
                 account_key TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS workflow_runs (
                 id               TEXT PRIMARY KEY,
                 project_id       INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                 project_name     TEXT NOT NULL,
                 workflow_id      TEXT NOT NULL,
                 workflow_name    TEXT NOT NULL,
                 run_number       INTEGER NOT NULL,
                 status           TEXT NOT NULL,
                 provider         TEXT NOT NULL,
                 issue_number     INTEGER,
                 branch_name      TEXT NOT NULL DEFAULT '',
                 total_tokens     INTEGER NOT NULL DEFAULT 0,
                 total_cost_cents INTEGER NOT NULL DEFAULT 0,
                 duration_ms      INTEGER NOT NULL DEFAULT 0,
                 summary          TEXT NOT NULL DEFAULT '',
                 view_json        TEXT NOT NULL,
                 started_at       INTEGER NOT NULL DEFAULT (unixepoch()),
                 finished_at      INTEGER
             );
             CREATE TABLE IF NOT EXISTS chat_sessions (
                 id               TEXT PRIMARY KEY,
                 project_id       INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                 title            TEXT NOT NULL DEFAULT '',
                 provider         TEXT NOT NULL,
                 status           TEXT NOT NULL,
                 total_tokens     INTEGER NOT NULL DEFAULT 0,
                 total_cost_cents INTEGER NOT NULL DEFAULT 0,
                 value_json       TEXT NOT NULL,
                 created_at       INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at       INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE TABLE IF NOT EXISTS chat_usage (
                 id           INTEGER PRIMARY KEY AUTOINCREMENT,
                 chat_id      TEXT NOT NULL,
                 project_id   INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                 project_name TEXT NOT NULL,
                 chat_title   TEXT NOT NULL DEFAULT '',
                 provider     TEXT NOT NULL,
                 tokens       INTEGER NOT NULL DEFAULT 0,
                 cost_micros  INTEGER NOT NULL DEFAULT 0,
                 recorded_at  INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE INDEX IF NOT EXISTS chat_usage_by_chat ON chat_usage(chat_id);",
        )?;
        Ok(())
    }

    /// Loads all registered projects ordered by registration id.
    pub fn list_projects(&self) -> Result<Vec<Project>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, root_path, default_branch, description
             FROM projects
             ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let root_path: String = row.get(2)?;
            let default_branch: String = row.get(3)?;
            let description: String = row.get(4)?;
            Ok(Project {
                id: id as ProjectId,
                name,
                root_path: PathBuf::from(root_path),
                default_branch,
                description,
            })
        })?;

        let mut projects = Vec::new();
        for row in rows {
            projects.push(row?);
        }
        Ok(projects)
    }

    /// Validates and persists a newly registered project in SQLite.
    pub fn register_project(
        &self,
        name: &str,
        root_path: impl AsRef<Path>,
        default_branch: &str,
        description: &str,
    ) -> Result<Project, StorageError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StorageError::EmptyProjectName);
        }

        let raw_path = root_path.as_ref().to_string_lossy();
        let trimmed_path = raw_path.trim();
        if trimmed_path.is_empty() {
            return Err(StorageError::EmptyProjectPath);
        }

        let branch = default_branch.trim();
        let default_branch = if branch.is_empty() { "main" } else { branch };
        let description = description.trim();

        let insert_result = self.conn.execute(
            "INSERT INTO projects (name, root_path, default_branch, description)
             VALUES (?1, ?2, ?3, ?4)",
            params![name, trimmed_path, default_branch, description],
        );

        match insert_result {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == ErrorCode::ConstraintViolation =>
            {
                return Err(StorageError::DuplicateRepositoryPath(PathBuf::from(
                    trimmed_path,
                )));
            }
            Err(err) => return Err(StorageError::Sqlite(err)),
        }

        let id = self.conn.last_insert_rowid() as ProjectId;
        log::info!(
            "db.register_project id={id} name={name:?} branch={default_branch:?} path={trimmed_path}"
        );
        Ok(Project {
            id,
            name: name.to_owned(),
            root_path: PathBuf::from(trimmed_path),
            default_branch: default_branch.to_owned(),
            description: description.to_owned(),
        })
    }

    /// Removes a tracked project by id. Returns `true` if a row was deleted.
    ///
    /// Runs in one transaction that:
    /// 1. Inserts tombstones for every Project-scoped Keychain secret belonging to `id` (regardless of `state`),
    /// 2. Strips `id` from every App secret's `Sharing::Projects(...)` in `detail_json`,
    /// 3. Deletes the `projects` row (cascading `secrets`).
    pub fn delete_project(&self, id: ProjectId) -> Result<bool, StorageError> {
        let instance_id = self.ensure_db_instance_id()?;
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let outcome = (|| -> Result<bool, StorageError> {
            // 1. Insert tombstones for every Project-scoped Keychain secret belonging to `id`.
            let project_secrets = {
                let mut stmt = self.conn.prepare(
                    "SELECT id, project_id, name, kind, state, detail_json, created_at, updated_at, last_used_at
                     FROM secrets WHERE project_id = ?1",
                )?;
                let rows = stmt.query_map(params![id as i64], map_secret_row)?;
                let mut out = Vec::new();
                for r in rows {
                    out.push(r?);
                }
                out
            };
            for rec in project_secrets {
                if matches!(rec.source, SecretSource::Keychain) {
                    let account_key = keychain_account_key(&instance_id, &rec.id);
                    self.conn.execute(
                        "INSERT OR REPLACE INTO secret_tombstones (id, account_key) VALUES (?1, ?2)",
                        params![rec.id, account_key],
                    )?;
                }
            }

            // 2. Strip `id` from every App secret's `Sharing::Projects(...)` in `detail_json`.
            let app_secrets = {
                let mut stmt = self.conn.prepare(
                    "SELECT id, project_id, name, kind, state, detail_json, created_at, updated_at, last_used_at
                     FROM secrets WHERE kind = 'env' AND project_id IS NULL",
                )?;
                let rows = stmt.query_map([], map_secret_row)?;
                let mut out = Vec::new();
                for r in rows {
                    out.push(r?);
                }
                out
            };
            for mut rec in app_secrets {
                let mut changed = false;
                if let SecretKind::Env {
                    scope:
                        EnvScope::App {
                            shared_with: Sharing::Projects(ref mut set),
                        },
                    ..
                } = rec.kind
                    && set.remove(&id)
                {
                    changed = true;
                }
                if changed {
                    let detail = SecretDetailJson::from_record(&rec);
                    let detail_json = serde_json::to_string(&detail)?;
                    self.conn.execute(
                        "UPDATE secrets SET detail_json = ?1, updated_at = unixepoch() WHERE id = ?2",
                        params![detail_json, rec.id],
                    )?;
                }
            }

            // 3. Delete the project row (cascading project-scoped `secrets`).
            let affected = self
                .conn
                .execute("DELETE FROM projects WHERE id = ?1", params![id as i64])?;
            Ok(affected > 0)
        })();

        match outcome {
            Ok(deleted) => {
                self.conn.execute_batch("COMMIT")?;
                log::info!("db.delete_project id={id} deleted={deleted}");
                Ok(deleted)
            }
            Err(err) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                log::error!("db.delete_project_failed id={id} err={err}");
                Err(err)
            }
        }
    }

    /// Loads persisted application preferences from SQLite, if saved previously.
    /// Legacy pre-Demo-Mode rows are automatically migrated and re-saved cleanly.
    pub fn load_preferences(&self) -> Result<Option<AppPreferences>, StorageError> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = ?1",
                params![PREFERENCES_KEY],
                |row| row.get(0),
            )
            .optional()?;
        match raw {
            Some(json) => {
                let mut prefs: AppPreferences = serde_json::from_str(&json)?;
                if prefs.migrate_legacy_sample_defaults() {
                    self.save_preferences(&prefs)?;
                }
                Ok(Some(prefs))
            }
            None => Ok(None),
        }
    }

    /// Saves application preferences as JSON in SQLite.
    pub fn save_preferences(&self, preferences: &AppPreferences) -> Result<(), StorageError> {
        let json = serde_json::to_string(preferences)?;
        self.conn.execute(
            "INSERT INTO app_settings (key, value_json, updated_at)
             VALUES (?1, ?2, unixepoch())
             ON CONFLICT(key) DO UPDATE SET
                 value_json = excluded.value_json,
                 updated_at = unixepoch()",
            params![PREFERENCES_KEY, json],
        )?;
        Ok(())
    }

    /// Loads persisted per-project settings from SQLite, if saved previously.
    pub fn load_project_settings(
        &self,
        project_id: ProjectId,
    ) -> Result<Option<ProjectSettings>, StorageError> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM project_settings WHERE project_id = ?1",
                params![project_id as i64],
                |row| row.get(0),
            )
            .optional()?;
        match raw {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    /// Saves per-project settings as JSON in SQLite.
    pub fn save_project_settings(
        &self,
        project_id: ProjectId,
        settings: &ProjectSettings,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(settings)?;
        self.conn.execute(
            "INSERT INTO project_settings (project_id, value_json, updated_at)
             VALUES (?1, ?2, unixepoch())
             ON CONFLICT(project_id) DO UPDATE SET
                 value_json = excluded.value_json,
                 updated_at = unixepoch()",
            params![project_id as i64, json],
        )?;
        Ok(())
    }

    /// Loads a secret value associated with a project from the legacy `project_secrets` table,
    /// if that table still exists (pre-migration databases and tests).
    pub fn load_project_secret(
        &self,
        project_id: ProjectId,
        key: &str,
    ) -> Result<Option<String>, StorageError> {
        if !self.has_legacy_project_secrets_table()? {
            return Ok(None);
        }
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM project_secrets WHERE project_id = ?1 AND key = ?2",
                params![project_id as i64, key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    }

    /// Stores or updates a secret value in the legacy `project_secrets` table (kept for legacy
    /// migration tests; production code writes to `SecretStore` + `secrets` metadata table).
    pub fn save_project_secret(
        &self,
        project_id: ProjectId,
        key: &str,
        value: &str,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS project_secrets (
                 project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                 key        TEXT NOT NULL,
                 value      TEXT NOT NULL,
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 PRIMARY KEY (project_id, key)
             )",
            [],
        )?;
        self.conn.execute(
            "INSERT INTO project_secrets (project_id, key, value, updated_at)
             VALUES (?1, ?2, ?3, unixepoch())
             ON CONFLICT(project_id, key) DO UPDATE SET
                 value = excluded.value,
                 updated_at = unixepoch()",
            params![project_id as i64, key, value],
        )?;
        Ok(())
    }

    /// Deletes a secret value from the legacy `project_secrets` table, if it exists.
    pub fn delete_project_secret(
        &self,
        project_id: ProjectId,
        key: &str,
    ) -> Result<bool, StorageError> {
        if !self.has_legacy_project_secrets_table()? {
            return Ok(false);
        }
        let affected = self.conn.execute(
            "DELETE FROM project_secrets WHERE project_id = ?1 AND key = ?2",
            params![project_id as i64, key],
        )?;
        Ok(affected > 0)
    }

    /// Loads a persisted project work queue snapshot from SQLite, if saved previously.
    pub fn load_project_work_queue(
        &self,
        project_id: ProjectId,
    ) -> Result<Option<ProjectWorkQueue>, StorageError> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM project_work_queues WHERE project_id = ?1",
                params![project_id as i64],
                |row| row.get(0),
            )
            .optional()?;
        match raw {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    /// Saves a project work queue snapshot as JSON in SQLite.
    pub fn save_project_work_queue(
        &self,
        project_id: ProjectId,
        queue: &ProjectWorkQueue,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(queue)?;
        self.conn.execute(
            "INSERT INTO project_work_queues (project_id, value_json, updated_at)
             VALUES (?1, ?2, unixepoch())
             ON CONFLICT(project_id) DO UPDATE SET
                 value_json = excluded.value_json,
                 updated_at = unixepoch()",
            params![project_id as i64, json],
        )?;
        Ok(())
    }

    /// Returns the next sequential run number for `project_id`.
    pub fn next_run_number_for_project(&self, project_id: ProjectId) -> Result<u32, StorageError> {
        let max_no: Option<i64> = self.conn.query_row(
            "SELECT MAX(run_number) FROM workflow_runs WHERE project_id = ?1",
            params![project_id as i64],
            |row| row.get(0),
        )?;
        Ok((max_no.unwrap_or(0) + 1) as u32)
    }

    /// Inserts or updates a real workflow run snapshot and telemetry record in SQLite.
    pub fn save_workflow_run(&self, run: &PersistedWorkflowRun) -> Result<(), StorageError> {
        let status_json = serde_json::to_string(&run.status)?;
        let view_json = serde_json::to_string(&run.view)?;
        self.conn.execute(
            "INSERT INTO workflow_runs (
                 id, project_id, project_name, workflow_id, workflow_name, run_number,
                 status, provider, issue_number, branch_name, total_tokens, total_cost_cents,
                 duration_ms, summary, view_json, started_at, finished_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
             ON CONFLICT(id) DO UPDATE SET
                 status = excluded.status,
                 provider = excluded.provider,
                 branch_name = excluded.branch_name,
                 total_tokens = excluded.total_tokens,
                 total_cost_cents = excluded.total_cost_cents,
                 duration_ms = excluded.duration_ms,
                 summary = excluded.summary,
                 view_json = excluded.view_json,
                 finished_at = excluded.finished_at",
            params![
                run.id,
                run.project_id as i64,
                run.project_name,
                run.workflow_id,
                run.workflow_name,
                run.run_number as i64,
                status_json,
                run.provider,
                run.issue_number.map(i64::from),
                run.branch_name,
                run.total_tokens as i64,
                run.total_cost_cents as i64,
                run.duration_ms as i64,
                run.summary,
                view_json,
                run.started_at,
                run.finished_at,
            ],
        )?;
        Ok(())
    }

    /// Loads persisted workflow runs across all projects, newest first.
    pub fn list_all_workflow_runs(&self) -> Result<Vec<PersistedWorkflowRun>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, project_name, workflow_id, workflow_name, run_number,
                    status, provider, issue_number, branch_name, total_tokens, total_cost_cents,
                    duration_ms, summary, view_json, started_at, finished_at
             FROM workflow_runs
             ORDER BY started_at DESC, run_number DESC",
        )?;
        let rows = stmt.query_map([], map_workflow_run_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Loads persisted workflow runs for one project, newest first.
    pub fn list_workflow_runs_for_project(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<PersistedWorkflowRun>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, project_name, workflow_id, workflow_name, run_number,
                    status, provider, issue_number, branch_name, total_tokens, total_cost_cents,
                    duration_ms, summary, view_json, started_at, finished_at
             FROM workflow_runs
             WHERE project_id = ?1
             ORDER BY started_at DESC, run_number DESC",
        )?;
        let rows = stmt.query_map(params![project_id as i64], map_workflow_run_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Inserts or updates a chat session with its whole transcript.
    pub fn save_chat_session(&self, session: &ChatSession) -> Result<(), StorageError> {
        let provider = serde_json::to_string(&session.provider)?;
        let status = serde_json::to_string(&session.status)?;
        let json = serde_json::to_string(session)?;
        self.conn.execute(
            "INSERT INTO chat_sessions (
                 id, project_id, title, provider, status, total_tokens, total_cost_cents,
                 value_json, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET
                 title = excluded.title,
                 provider = excluded.provider,
                 status = excluded.status,
                 total_tokens = excluded.total_tokens,
                 total_cost_cents = excluded.total_cost_cents,
                 value_json = excluded.value_json,
                 updated_at = excluded.updated_at",
            params![
                session.id,
                session.project_id as i64,
                session.title,
                provider,
                status,
                session.total_tokens as i64,
                session.total_cost_cents() as i64,
                json,
                session.created_at,
                session.updated_at,
            ],
        )?;
        Ok(())
    }

    /// Loads every chat session across projects, most recently active first. Rows that no
    /// longer parse are skipped.
    pub fn list_chat_sessions(&self) -> Result<Vec<ChatSession>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT value_json FROM chat_sessions ORDER BY updated_at DESC, id DESC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            if let Ok(session) = serde_json::from_str::<ChatSession>(&row?) {
                out.push(session);
            }
        }
        Ok(out)
    }

    /// Deletes a chat session. Returns `true` if a row was deleted. Its usage records stay.
    pub fn delete_chat_session(&self, id: &str) -> Result<bool, StorageError> {
        let affected = self
            .conn
            .execute("DELETE FROM chat_sessions WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    /// Records the tokens and cost one turn of `session` used. Records outlive the chat, so the
    /// Dashboard keeps counting what a deleted chat consumed.
    pub fn record_chat_usage(
        &self,
        session: &ChatSession,
        usage: ChatUsageDelta,
        recorded_at: i64,
    ) -> Result<(), StorageError> {
        let provider = serde_json::to_string(&session.provider)?;
        self.conn.execute(
            "INSERT INTO chat_usage (
                 chat_id, project_id, project_name, chat_title, provider, tokens, cost_micros,
                 recorded_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                session.id,
                session.project_id as i64,
                session.project_name,
                session.display_title(),
                provider,
                usage.tokens as i64,
                usage.cost_micros as i64,
                recorded_at,
            ],
        )?;
        Ok(())
    }

    /// Usage of every chat that recorded any, deleted chats included, one row per chat
    /// (named as of its latest turn), most tokens first.
    pub fn list_chat_usage(&self) -> Result<Vec<PersistedChatUsage>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT t.chat_id, l.project_name, l.chat_title, l.provider, t.tokens, t.cost_micros,
                    t.turns, t.last_recorded_at,
                    EXISTS (SELECT 1 FROM chat_sessions s WHERE s.id = t.chat_id)
             FROM (
                 SELECT chat_id, MAX(id) AS last_id, SUM(tokens) AS tokens,
                        SUM(cost_micros) AS cost_micros, COUNT(*) AS turns,
                        MAX(recorded_at) AS last_recorded_at
                 FROM chat_usage
                 GROUP BY chat_id
             ) t
             JOIN chat_usage l ON l.id = t.last_id
             ORDER BY t.tokens DESC, t.chat_id",
        )?;
        let rows = stmt.query_map([], |row| {
            let provider_raw: String = row.get(3)?;
            Ok(PersistedChatUsage {
                chat_id: row.get(0)?,
                project_name: row.get(1)?,
                title: row.get(2)?,
                provider: serde_json::from_str(&provider_raw).unwrap_or(ProviderKind::Claude),
                tokens: row.get::<_, i64>(4)?.max(0) as u64,
                cost_micros: row.get::<_, i64>(5)?.max(0) as u64,
                turns: row.get::<_, i64>(6)?.max(0) as u32,
                last_recorded_at: row.get(7)?,
                deleted: !row.get::<_, bool>(8)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // -----------------------------------------------------------------------------------------
    // Secrets metadata, db_instance_id, path-change guard, two-store consistency & migration
    // -----------------------------------------------------------------------------------------

    /// Reads an arbitrary string value stored in `app_settings` by key.
    pub fn get_app_setting_raw(&self, key: &str) -> Result<Option<String>, StorageError> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(raw)
    }

    /// Writes an arbitrary string value into `app_settings` by key.
    pub fn set_app_setting_raw(&self, key: &str, value: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO app_settings (key, value_json, updated_at)
             VALUES (?1, ?2, unixepoch())
             ON CONFLICT(key) DO UPDATE SET
                 value_json = excluded.value_json,
                 updated_at = unixepoch()",
            params![key, value],
        )?;
        Ok(())
    }

    /// Returns this database's unique Keychain namespace UUID (`secret_store_instance_id`),
    /// minting and persisting a new UUID v4 on first access.
    pub fn ensure_db_instance_id(&self) -> Result<String, StorageError> {
        if let Some(existing) = self.get_app_setting_raw(SECRET_STORE_INSTANCE_ID_KEY)?
            && !existing.trim().is_empty()
        {
            return Ok(existing);
        }
        let id = new_db_instance_id();
        self.set_app_setting_raw(SECRET_STORE_INSTANCE_ID_KEY, &id)?;
        if let Some(path) = self.path() {
            let _ = self.set_app_setting_raw(SECRET_STORE_LAST_PATH_KEY, &path.to_string_lossy());
        }
        Ok(id)
    }

    /// Checks whether a file-backed database was opened at a different path than `secret_store_last_path`.
    /// Returns `Ok(Some((previous_path, current_path)))` if a manual copy/move is detected.
    pub fn check_database_path_change(&self) -> Result<Option<(PathBuf, PathBuf)>, StorageError> {
        let Some(current) = self.path() else {
            return Ok(None);
        };
        let current_str = current.to_string_lossy().into_owned();
        match self.get_app_setting_raw(SECRET_STORE_LAST_PATH_KEY)? {
            None => {
                self.set_app_setting_raw(SECRET_STORE_LAST_PATH_KEY, &current_str)?;
                Ok(None)
            }
            Some(prev) if prev == current_str => Ok(None),
            Some(prev) => {
                log::warn!("db.path_change_detected previous={prev} current={current_str}");
                Ok(Some((PathBuf::from(prev), current.to_path_buf())))
            }
        }
    }

    /// User confirmed this Bruk folder was **moved** (keep Keychain namespace & update path).
    pub fn confirm_database_moved(&self) -> Result<(), StorageError> {
        if let Some(current) = self.path() {
            self.set_app_setting_raw(SECRET_STORE_LAST_PATH_KEY, &current.to_string_lossy())?;
        }
        Ok(())
    }

    /// User confirmed this Bruk folder was **cloned** (mint new `db_instance_id`, clear tombstones,
    /// mark Keychain secrets `needs_value` so the original folder's Keychain items stay untouched).
    pub fn confirm_database_cloned(&self) -> Result<String, StorageError> {
        let new_id = new_db_instance_id();
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let res = (|| -> Result<(), StorageError> {
            self.set_app_setting_raw(SECRET_STORE_INSTANCE_ID_KEY, &new_id)?;
            if let Some(current) = self.path() {
                self.set_app_setting_raw(SECRET_STORE_LAST_PATH_KEY, &current.to_string_lossy())?;
            }
            self.conn.execute("DELETE FROM secret_tombstones", [])?;
            self.conn.execute(
                "UPDATE secrets SET state = 'needs_value', updated_at = unixepoch()
                 WHERE detail_json LIKE '%\"source\":\"keychain\"%'",
                [],
            )?;
            Ok(())
        })();
        match res {
            Ok(()) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(new_id)
            }
            Err(err) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }

    /// Lists all persisted secret metadata records ordered by kind, name.
    pub fn list_secrets(&self) -> Result<Vec<SecretRecord>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, name, kind, state, detail_json, created_at, updated_at, last_used_at
             FROM secrets
             ORDER BY kind ASC, name COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([], map_secret_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Reads one secret metadata record by id.
    pub fn get_secret(&self, id: &str) -> Result<Option<SecretRecord>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, name, kind, state, detail_json, created_at, updated_at, last_used_at
             FROM secrets WHERE id = ?1",
        )?;
        let row = stmt.query_row(params![id], map_secret_row).optional()?;
        Ok(row)
    }

    /// Low-level insert of a `SecretRecord` row (enforces cross-store uniqueness with `project_settings.env_vars`).
    pub fn insert_secret_row(&self, record: &SecretRecord) -> Result<(), StorageError> {
        record.validate().map_err(StorageError::Secret)?;
        self.check_cross_store_env_uniqueness(record, None)?;
        let detail_json = serde_json::to_string(&SecretDetailJson::from_record(record))?;
        let kind_str = if record.is_internal() {
            "internal"
        } else {
            "env"
        };
        let state_str = match record.state {
            SecretState::Ready => "ready",
            SecretState::NeedsValue => "needs_value",
        };
        let res = self.conn.execute(
            "INSERT INTO secrets (id, project_id, name, kind, state, detail_json, created_at, updated_at, last_used_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                record.id,
                record.project_id().map(|p| p as i64),
                record.name,
                kind_str,
                state_str,
                detail_json,
                record.created_at,
                record.updated_at,
                record.last_used_at,
            ],
        );
        match res {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == ErrorCode::ConstraintViolation =>
            {
                Err(StorageError::Secret(format!(
                    "a secret named '{}' already exists in this scope",
                    record.name
                )))
            }
            Err(err) => Err(StorageError::Sqlite(err)),
        }
    }

    /// Low-level update of an existing `SecretRecord` row.
    pub fn update_secret_row(&self, record: &SecretRecord) -> Result<(), StorageError> {
        record.validate().map_err(StorageError::Secret)?;
        self.check_cross_store_env_uniqueness(record, Some(&record.id))?;
        let detail_json = serde_json::to_string(&SecretDetailJson::from_record(record))?;
        let kind_str = if record.is_internal() {
            "internal"
        } else {
            "env"
        };
        let state_str = match record.state {
            SecretState::Ready => "ready",
            SecretState::NeedsValue => "needs_value",
        };
        let res = self.conn.execute(
            "UPDATE secrets
             SET project_id = ?2, name = ?3, kind = ?4, state = ?5, detail_json = ?6,
                 updated_at = ?7, last_used_at = ?8
             WHERE id = ?1",
            params![
                record.id,
                record.project_id().map(|p| p as i64),
                record.name,
                kind_str,
                state_str,
                detail_json,
                record.updated_at,
                record.last_used_at,
            ],
        );
        match res {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == ErrorCode::ConstraintViolation =>
            {
                Err(StorageError::Secret(format!(
                    "a secret named '{}' already exists in this scope",
                    record.name
                )))
            }
            Err(err) => Err(StorageError::Sqlite(err)),
        }
    }

    /// Deletes only the SQLite row (used when rolling back a failed Keychain create).
    pub fn delete_secret_row_only(&self, id: &str) -> Result<bool, StorageError> {
        let affected = self
            .conn
            .execute("DELETE FROM secrets WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    /// Two-store consistent **Create** for a Keychain-backed secret (§5):
    /// 1. Insert SQLite row as `needs_value` (verifying cross-store uniqueness)
    /// 2. Write Keychain item + read back & verify exact byte equality
    /// 3. Mark SQLite row `ready` (+ `preview` if char len >= 12)
    ///
    /// On Keychain error, rolls back the `needs_value` row.
    pub fn create_keychain_secret(
        &self,
        store: &dyn SecretStore,
        mut record: SecretRecord,
        value: &SecretString,
        project_name: Option<&str>,
    ) -> Result<SecretRecord, StorageError> {
        validate_secret_value(value.expose_secret()).map_err(StorageError::Secret)?;
        record.source = SecretSource::Keychain;
        record.state = SecretState::NeedsValue;
        record.preview = None;
        record.validate().map_err(StorageError::Secret)?;

        let instance_id = self.ensure_db_instance_id()?;
        self.insert_secret_row(&record)?;

        let account_key = keychain_account_key(&instance_id, &record.id);
        let label = keychain_item_label(&record, project_name);
        if let Err(err) = write_and_verify(store, &account_key, &label, value) {
            let _ = self.delete_secret_row_only(&record.id);
            return Err(StorageError::Secret(err.to_string()));
        }

        record.state = SecretState::Ready;
        record.preview = compute_preview(value.expose_secret());
        record.updated_at = current_unix_seconds();
        self.update_secret_row(&record)?;
        Ok(record)
    }

    /// Creates a manager-backed secret (1Password or generic Command Manager).
    pub fn create_manager_secret(
        &self,
        mut record: SecretRecord,
    ) -> Result<SecretRecord, StorageError> {
        record.state = SecretState::Ready;
        record.preview = None;
        record.validate().map_err(StorageError::Secret)?;
        self.insert_secret_row(&record)?;
        Ok(record)
    }

    /// Two-store consistent **Replace** for an existing Keychain secret (§5):
    /// Writes and verifies the Keychain item FIRST without flipping a working `ready` secret to
    /// `needs_value`. If the user cancels or Keychain fails, the existing secret stays intact.
    pub fn replace_keychain_secret(
        &self,
        store: &dyn SecretStore,
        id: &str,
        new_value: &SecretString,
        project_name: Option<&str>,
    ) -> Result<SecretRecord, StorageError> {
        validate_secret_value(new_value.expose_secret()).map_err(StorageError::Secret)?;
        let mut record = self
            .get_secret(id)?
            .ok_or_else(|| StorageError::Secret(format!("secret '{id}' not found")))?;
        let instance_id = self.ensure_db_instance_id()?;
        let account_key = keychain_account_key(&instance_id, &record.id);
        let label = keychain_item_label(&record, project_name);

        // Write & verify Keychain FIRST; SQLite row remains untouched on failure!
        write_and_verify(store, &account_key, &label, new_value)
            .map_err(|err| StorageError::Secret(err.to_string()))?;

        record.source = SecretSource::Keychain;
        record.state = SecretState::Ready;
        record.preview = compute_preview(new_value.expose_secret());
        record.updated_at = current_unix_seconds();
        self.update_secret_row(&record)?;
        Ok(record)
    }

    /// Updates metadata (name, sharing, exposure, description, manager source) and best-effort
    /// updates the Keychain `kSecAttrLabel` if Keychain-backed.
    pub fn update_secret_metadata(
        &self,
        store: &dyn SecretStore,
        mut record: SecretRecord,
        project_name: Option<&str>,
    ) -> Result<SecretRecord, StorageError> {
        record.updated_at = current_unix_seconds();
        self.update_secret_row(&record)?;
        if matches!(record.source, SecretSource::Keychain) {
            let instance_id = self.ensure_db_instance_id()?;
            let account_key = keychain_account_key(&instance_id, &record.id);
            let label = keychain_item_label(&record, project_name);
            let _ = store.update_label(&account_key, &label);
        }
        Ok(record)
    }

    /// Two-store consistent **Delete** (§5):
    /// In one SQLite transaction, deletes the `secrets` row and inserts `(id, account_key)` into
    /// `secret_tombstones` (even when `state == 'needs_value'`), then deletes the Keychain item
    /// and clears the tombstone.
    pub fn delete_secret(&self, store: &dyn SecretStore, id: &str) -> Result<bool, StorageError> {
        let Some(record) = self.get_secret(id)? else {
            return Ok(false);
        };
        let instance_id = self.ensure_db_instance_id()?;
        let needs_tombstone = matches!(record.source, SecretSource::Keychain);
        let account_key = keychain_account_key(&instance_id, &record.id);

        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let tx_res = (|| -> Result<bool, StorageError> {
            let affected = self
                .conn
                .execute("DELETE FROM secrets WHERE id = ?1", params![id])?;
            if affected > 0 && needs_tombstone {
                self.conn.execute(
                    "INSERT OR REPLACE INTO secret_tombstones (id, account_key) VALUES (?1, ?2)",
                    params![id, account_key],
                )?;
            }
            Ok(affected > 0)
        })();

        let deleted = match tx_res {
            Ok(d) => {
                self.conn.execute_batch("COMMIT")?;
                d
            }
            Err(err) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                return Err(err);
            }
        };

        if deleted && needs_tombstone && store.delete(&account_key).is_ok() {
            let _ = self.delete_tombstone(id);
        }

        Ok(deleted)
    }

    /// Lists outstanding Keychain deletion tombstones `(secret_id, account_key)`.
    pub fn list_tombstones(&self) -> Result<Vec<(String, String)>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, account_key FROM secret_tombstones")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Removes a cleared tombstone row.
    pub fn delete_tombstone(&self, id: &str) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM secret_tombstones WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Launch sweep retrying outstanding tombstones (§5).
    /// Skips when no tombstones exist, when store is unavailable/locked, when `store.is_memory()`
    /// (unless `allow_memory`), or when a database path change has not yet been confirmed.
    pub fn sweep_secret_tombstones(
        &self,
        store: &dyn SecretStore,
        allow_memory: bool,
    ) -> Result<usize, StorageError> {
        let tombstones = self.list_tombstones()?;
        if tombstones.is_empty() {
            return Ok(0);
        }
        if !store.status().is_available() || (!allow_memory && store.is_memory()) {
            return Ok(0);
        }
        if self.check_database_path_change()?.is_some() {
            return Ok(0);
        }
        let mut swept = 0usize;
        let total = tombstones.len();
        for (id, account_key) in tombstones {
            match store.delete(&account_key) {
                Ok(()) => {
                    self.delete_tombstone(&id)?;
                    swept += 1;
                }
                Err(err) => {
                    log::warn!("db.sweep_tombstone_failed secret_id={id} err={err}");
                }
            }
        }
        log::info!("db.sweep_tombstones total={total} swept={swept}");
        Ok(swept)
    }

    /// Flushes batched `last_used_at` timestamp updates into SQLite.
    pub fn flush_secret_last_used(
        &self,
        updates: &BTreeMap<SecretId, i64>,
    ) -> Result<(), StorageError> {
        if updates.is_empty() {
            return Ok(());
        }
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let res = (|| -> Result<(), StorageError> {
            let mut stmt = self
                .conn
                .prepare("UPDATE secrets SET last_used_at = ?2 WHERE id = ?1")?;
            for (id, ts) in updates {
                stmt.execute(params![id, ts])?;
            }
            Ok(())
        })();
        match res {
            Ok(()) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(err) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }

    /// Marks missing Keychain secrets as `needs_value` in SQLite.
    pub fn mark_secrets_needs_value(&self, ids: &[SecretId]) -> Result<(), StorageError> {
        if ids.is_empty() {
            return Ok(());
        }
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let res = (|| -> Result<(), StorageError> {
            let mut stmt = self.conn.prepare(
                "UPDATE secrets SET state = 'needs_value', updated_at = unixepoch() WHERE id = ?1",
            )?;
            for id in ids {
                stmt.execute(params![id])?;
            }
            Ok(())
        })();
        match res {
            Ok(()) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(err) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }

    /// Checks cross-store case-insensitive uniqueness between `secrets` and `project_settings.env_vars`.
    fn check_cross_store_env_uniqueness(
        &self,
        record: &SecretRecord,
        exclude_id: Option<&str>,
    ) -> Result<(), StorageError> {
        let SecretKind::Env { scope, .. } = &record.kind else {
            return Ok(());
        };
        if let EnvScope::Project(pid) = scope
            && let Some(ps) = self.load_project_settings(*pid)?
            && ps
                .env_vars
                .iter()
                .any(|v| !v.is_secret && v.key.eq_ignore_ascii_case(&record.name))
        {
            return Err(StorageError::Secret(format!(
                "a plain environment variable named '{}' already exists in this project",
                record.name
            )));
        }
        // Check secrets table uniqueness explicitly for clean error messages.
        let conflict: Option<String> = match scope {
            EnvScope::App { .. } => self
                .conn
                .query_row(
                    "SELECT id FROM secrets WHERE kind = 'env' AND project_id IS NULL AND name = ?1 COLLATE NOCASE",
                    params![record.name],
                    |row| row.get(0),
                )
                .optional()?,
            EnvScope::Project(pid) => self
                .conn
                .query_row(
                    "SELECT id FROM secrets WHERE kind = 'env' AND project_id = ?1 AND name = ?2 COLLATE NOCASE",
                    params![*pid as i64, record.name],
                    |row| row.get(0),
                )
                .optional()?,
        };
        if let Some(existing_id) = conflict
            && exclude_id != Some(existing_id.as_str())
        {
            return Err(StorageError::Secret(format!(
                "a secret named '{}' already exists in this scope",
                record.name
            )));
        }
        Ok(())
    }

    /// Saves `ProjectSettings` inside one transaction after checking that no plain `env_vars`
    /// collide case-insensitively with a Project Env `SecretRecord`.
    pub fn save_project_settings_checked(
        &self,
        project_id: ProjectId,
        settings: &ProjectSettings,
    ) -> Result<(), StorageError> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let res = (|| -> Result<(), StorageError> {
            let mut stmt = self
                .conn
                .prepare("SELECT name FROM secrets WHERE kind = 'env' AND project_id = ?1")?;
            let secret_names: Vec<String> = stmt
                .query_map(params![project_id as i64], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            for ev in &settings.env_vars {
                if !ev.is_secret && secret_names.iter().any(|s| s.eq_ignore_ascii_case(&ev.key)) {
                    return Err(StorageError::Secret(format!(
                        "a secret named '{}' already exists in this project",
                        ev.key
                    )));
                }
            }
            self.save_project_settings(project_id, settings)
        })();
        match res {
            Ok(()) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(err) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }

    /// Builds an immutable [`SecretsMetadataSnapshot`] for [`crate::secrets::SecretResolver`].
    pub fn build_secrets_metadata_snapshot(
        &self,
        preferences: &AppPreferences,
    ) -> Result<SecretsMetadataSnapshot, StorageError> {
        let db_instance_id = self.ensure_db_instance_id()?;
        let records = self.list_secrets()?;
        let projects = self.list_projects()?;
        let mut project_plain_env: BTreeMap<ProjectId, BTreeMap<String, String>> = BTreeMap::new();
        let mut project_excluded_app_secrets: BTreeMap<ProjectId, BTreeSet<SecretId>> =
            BTreeMap::new();

        for p in projects {
            if let Some(ps) = self.load_project_settings(p.id)? {
                let mut plain = BTreeMap::new();
                for ev in &ps.env_vars {
                    if !ev.is_secret {
                        plain.insert(ev.key.clone(), ev.value.clone());
                    }
                }
                preferences.advanced.apply_agent_env(&mut plain);
                project_plain_env.insert(p.id, plain);
                if !ps.excluded_app_secrets.is_empty() {
                    project_excluded_app_secrets.insert(p.id, ps.excluded_app_secrets);
                }
            }
        }

        Ok(SecretsMetadataSnapshot {
            db_instance_id,
            records,
            project_plain_env,
            project_excluded_app_secrets,
            settings: preferences.secrets.clone(),
        })
    }

    /// Whether the legacy `project_secrets` table still exists in this database.
    pub fn has_legacy_project_secrets_table(&self) -> Result<bool, StorageError> {
        let exists: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'project_secrets'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(exists.is_some())
    }

    /// Lists all rows in the legacy `project_secrets` table for migration.
    pub fn list_all_legacy_project_secrets(
        &self,
    ) -> Result<Vec<(ProjectId, String, String)>, StorageError> {
        if !self.has_legacy_project_secrets_table()? {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare("SELECT project_id, key, value FROM project_secrets")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)? as ProjectId,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Step 6 of migration: structured JSON scrubbing of `workflow_runs`, `chat_sessions`, and
    /// `chat_usage` preserving valid JSON so `list_chat_sessions` never drops chats.
    pub fn scrub_persisted_transcripts(
        &self,
        redactor: &SecretRedactor,
    ) -> Result<usize, StorageError> {
        if redactor.is_empty() {
            return Ok(0);
        }
        let mut scrubbed = 0usize;
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let res = (|| -> Result<(), StorageError> {
            // 1. workflow_runs
            let runs: Vec<(String, String, String)> = {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id, summary, view_json FROM workflow_runs")?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                rows.collect::<Result<_, _>>()?
            };
            for (id, summary, view_json) in runs {
                let new_summary = redactor.redact(&summary);
                let new_view_json = match serde_json::from_str::<serde_json::Value>(&view_json) {
                    Ok(mut val) => {
                        redactor.redact_json_value(&mut val);
                        serde_json::to_string(&val)?
                    }
                    Err(_) => view_json.clone(),
                };
                if new_summary != summary || new_view_json != view_json {
                    self.conn.execute(
                        "UPDATE workflow_runs SET summary = ?2, view_json = ?3 WHERE id = ?1",
                        params![id, new_summary, new_view_json],
                    )?;
                    scrubbed += 1;
                }
            }

            // 2. chat_sessions
            let chats: Vec<(String, String, String)> = {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id, title, value_json FROM chat_sessions")?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                rows.collect::<Result<_, _>>()?
            };
            for (id, title, value_json) in chats {
                let new_title = redactor.redact(&title);
                let new_value_json = match serde_json::from_str::<serde_json::Value>(&value_json) {
                    Ok(mut val) => {
                        redactor.redact_json_value(&mut val);
                        serde_json::to_string(&val)?
                    }
                    Err(_) => value_json.clone(),
                };
                if new_title != title || new_value_json != value_json {
                    self.conn.execute(
                        "UPDATE chat_sessions SET title = ?2, value_json = ?3 WHERE id = ?1",
                        params![id, new_title, new_value_json],
                    )?;
                    scrubbed += 1;
                }
            }

            // 3. chat_usage.chat_title
            let usage_rows: Vec<(i64, String)> = {
                let mut stmt = self.conn.prepare("SELECT id, chat_title FROM chat_usage")?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
                rows.collect::<Result<_, _>>()?
            };
            for (id, chat_title) in usage_rows {
                let new_title = redactor.redact(&chat_title);
                if new_title != chat_title {
                    self.conn.execute(
                        "UPDATE chat_usage SET chat_title = ?2 WHERE id = ?1",
                        params![id, new_title],
                    )?;
                    scrubbed += 1;
                }
            }
            Ok(())
        })();

        match res {
            Ok(()) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(scrubbed)
            }
            Err(err) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }

    /// Step 7 of migration: drops `project_secrets`, checkpoints WAL (`TRUNCATE`), and runs `VACUUM`.
    pub fn drop_legacy_project_secrets_and_vacuum(&self) -> Result<(), StorageError> {
        self.conn.execute_batch(
            "DROP TABLE IF EXISTS project_secrets;
             PRAGMA wal_checkpoint(TRUNCATE);
             VACUUM;",
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecretDetailJson {
    kind: SecretKind,
    source: SecretSource,
    #[serde(default)]
    description: String,
    #[serde(default)]
    preview: Option<String>,
}

impl SecretDetailJson {
    fn from_record(record: &SecretRecord) -> Self {
        Self {
            kind: record.kind.clone(),
            source: record.source.clone(),
            description: record.description.clone(),
            preview: record.preview.clone(),
        }
    }
}

fn map_secret_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SecretRecord> {
    let id: String = row.get(0)?;
    let _project_id: Option<i64> = row.get(1)?;
    let name: String = row.get(2)?;
    let _kind_col: String = row.get(3)?;
    let state_col: String = row.get(4)?;
    let detail_raw: String = row.get(5)?;
    let created_at: i64 = row.get(6)?;
    let updated_at: i64 = row.get(7)?;
    let last_used_at: Option<i64> = row.get(8)?;

    let detail: SecretDetailJson = serde_json::from_str(&detail_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(err))
    })?;
    let state = match state_col.as_str() {
        "ready" => SecretState::Ready,
        _ => SecretState::NeedsValue,
    };

    Ok(SecretRecord {
        id,
        name,
        kind: detail.kind,
        source: detail.source,
        state,
        description: detail.description,
        preview: detail.preview,
        created_at,
        updated_at,
        last_used_at,
    })
}

/// What one chat's turns used, summed from its usage records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedChatUsage {
    pub chat_id: String,
    pub project_name: String,
    pub title: String,
    pub provider: ProviderKind,
    pub tokens: u64,
    pub cost_micros: u64,
    /// Turns that reported usage.
    pub turns: u32,
    pub last_recorded_at: i64,
    /// The chat itself was deleted; its usage still counts.
    pub deleted: bool,
}

fn map_workflow_run_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PersistedWorkflowRun> {
    let status_raw: String = row.get(6)?;
    let view_raw: String = row.get(14)?;
    let status: RunExecutionState =
        serde_json::from_str(&status_raw).unwrap_or(RunExecutionState::Stopped);
    let view: WorkflowRunViewData =
        serde_json::from_str(&view_raw).unwrap_or_else(|_| WorkflowRunViewData::empty("", ""));
    Ok(PersistedWorkflowRun {
        id: row.get(0)?,
        project_id: row.get::<_, i64>(1)? as ProjectId,
        project_name: row.get(2)?,
        workflow_id: row.get(3)?,
        workflow_name: row.get(4)?,
        run_number: row.get::<_, i64>(5)? as u32,
        status,
        provider: row.get(7)?,
        issue_number: row.get::<_, Option<i64>>(8)?.map(|n| n as u32),
        branch_name: row.get(9)?,
        total_tokens: row.get::<_, i64>(10)? as u64,
        total_cost_cents: row.get::<_, i64>(11)? as u64,
        duration_ms: row.get::<_, i64>(12)? as u64,
        summary: row.get(13)?,
        view,
        started_at: row.get(15)?,
        finished_at: row.get(16)?,
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn persists_chat_sessions_and_drops_them_with_their_project() {
        use crate::{
            chat::{ChatEvent, ChatSessionStatus},
            project_settings::ProjectSandboxMode,
        };

        let temp = TempDir::new().unwrap();
        let db_path = temp.path().join("bruk.db");
        let db = ProjectDatabase::open(&db_path).unwrap();
        let project = db.register_project("api", "/work/api", "main", "").unwrap();

        let mut older = ChatSession::new(
            "chat-1",
            project.id,
            "api",
            ProviderKind::Claude,
            None,
            None,
            ProjectSandboxMode::ReadOnly,
            10,
        );
        older.push_user_message("Where is auth handled?", 10);
        let mut newer = ChatSession::new(
            "chat-2",
            project.id,
            "api",
            ProviderKind::Codex,
            Some("reviewer".to_owned()),
            Some("Reviewer".to_owned()),
            ProjectSandboxMode::WorkspaceWrite,
            20,
        );
        newer.push_user_message("Review my branch", 20);
        db.save_chat_session(&older).unwrap();
        db.save_chat_session(&newer).unwrap();

        // Updates replace the row in place.
        older.apply_event(&ChatEvent::TurnStarted, 30);
        older.apply_event(
            &ChatEvent::AssistantMessage {
                text: "In `src/auth.rs`.".to_owned(),
            },
            30,
        );
        older.apply_event(
            &ChatEvent::TurnFinished {
                status: ChatSessionStatus::Idle,
                error: None,
            },
            30,
        );
        db.save_chat_session(&older).unwrap();

        let reopened = ProjectDatabase::open(&db_path).unwrap();
        let chats = reopened.list_chat_sessions().unwrap();
        assert_eq!(
            chats.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["chat-1", "chat-2"]
        );
        assert_eq!(chats[0], older);
        assert_eq!(chats[1], newer);

        // Usage is recorded per turn and outlives the chat.
        let turn = |tokens, cost_micros| ChatUsageDelta {
            tokens,
            cost_micros,
        };
        reopened
            .record_chat_usage(&older, turn(1_000, 20_000), 31)
            .unwrap();
        reopened
            .record_chat_usage(&older, turn(500, 5_000), 32)
            .unwrap();
        reopened
            .record_chat_usage(&newer, turn(4_000, 0), 33)
            .unwrap();
        assert!(reopened.delete_chat_session("chat-2").unwrap());
        assert!(!reopened.delete_chat_session("chat-2").unwrap());
        let usage = reopened.list_chat_usage().unwrap();
        assert_eq!(
            usage,
            vec![
                PersistedChatUsage {
                    chat_id: "chat-2".to_owned(),
                    project_name: "api".to_owned(),
                    title: "Review my branch".to_owned(),
                    provider: ProviderKind::Codex,
                    tokens: 4_000,
                    cost_micros: 0,
                    turns: 1,
                    last_recorded_at: 33,
                    deleted: true,
                },
                PersistedChatUsage {
                    chat_id: "chat-1".to_owned(),
                    project_name: "api".to_owned(),
                    title: "Where is auth handled?".to_owned(),
                    provider: ProviderKind::Claude,
                    tokens: 1_500,
                    cost_micros: 25_000,
                    turns: 2,
                    last_recorded_at: 32,
                    deleted: false,
                },
            ]
        );

        assert!(reopened.delete_project(project.id).unwrap());
        assert!(reopened.list_chat_sessions().unwrap().is_empty());
        assert!(reopened.list_chat_usage().unwrap().is_empty());
    }

    #[test]
    fn creates_nested_dir_and_persists_projects_across_reopen() {
        let temp = TempDir::new().unwrap();
        let db_path = temp
            .path()
            .join("config")
            .join("software-factory")
            .join("factory.db");

        {
            let db = ProjectDatabase::open(&db_path).unwrap();
            assert_eq!(db.path(), Some(db_path.as_path()));
            let created = db
                .register_project(
                    "billing-api",
                    "/Users/dev/billing-api",
                    "",
                    "Payments service",
                )
                .unwrap();
            assert_eq!(created.id, 1);
            assert_eq!(created.name, "billing-api");
            assert_eq!(created.root_path, PathBuf::from("/Users/dev/billing-api"));
            assert_eq!(created.default_branch, "main");
        }

        // Reopen from disk to confirm durability.
        let reopened = ProjectDatabase::open(&db_path).unwrap();
        let projects = reopened.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "billing-api");
        assert_eq!(
            projects[0].root_path,
            PathBuf::from("/Users/dev/billing-api")
        );
    }

    #[test]
    fn rejects_blank_inputs_and_duplicate_paths() {
        let db = ProjectDatabase::open_in_memory().unwrap();

        assert!(matches!(
            db.register_project("   ", "/repo/a", "main", ""),
            Err(StorageError::EmptyProjectName)
        ));
        assert!(matches!(
            db.register_project("repo-a", "   ", "main", ""),
            Err(StorageError::EmptyProjectPath)
        ));

        db.register_project("repo-a", "/repo/a", "main", "")
            .unwrap();
        assert!(matches!(
            db.register_project("repo-a-dup", "/repo/a", "main", ""),
            Err(StorageError::DuplicateRepositoryPath(path)) if path == Path::new("/repo/a")
        ));
    }

    #[test]
    fn deletes_registered_project() {
        let db = ProjectDatabase::open_in_memory().unwrap();
        let project = db
            .register_project("web-app", "/repo/web-app", "main", "")
            .unwrap();

        assert!(db.delete_project(project.id).unwrap());
        assert!(!db.delete_project(project.id).unwrap());
        assert!(db.list_projects().unwrap().is_empty());
    }

    #[test]
    fn opens_bruk_db_inside_home_and_copies_databases() {
        let temp = TempDir::new().unwrap();
        let home = BrukHome::at(temp.path().join("Bruk"));
        {
            let db = ProjectDatabase::open_home(&home).unwrap();
            assert_eq!(db.path(), Some(temp.path().join("Bruk/bruk.db").as_path()));
            db.register_project("web-app", "/repo/web-app", "main", "")
                .unwrap();
        }
        assert!(home.projects_dir().is_dir());
        assert!(home.runs_dir().is_dir());

        let copy_path = temp.path().join("copy").join("bruk.db");
        ProjectDatabase::copy_database(home.database_path(), &copy_path).unwrap();
        let copy = ProjectDatabase::open(&copy_path).unwrap();
        assert_eq!(copy.list_projects().unwrap()[0].name, "web-app");
        // The source is untouched and still opens.
        let original = ProjectDatabase::open(home.database_path()).unwrap();
        assert_eq!(original.list_projects().unwrap().len(), 1);
    }

    #[test]
    fn persists_app_preferences_across_reopen() {
        let temp = TempDir::new().unwrap();
        let db_path = temp.path().join("factory.db");

        {
            let db = ProjectDatabase::open(&db_path).unwrap();
            assert_eq!(db.load_preferences().unwrap(), None);

            let mut prefs = AppPreferences::sample();
            prefs.appearance.mode = crate::AppearanceMode::Dark;
            prefs
                .make_default_openrouter_key("openrouter-acme")
                .unwrap();
            prefs
                .add_openrouter_key("Sandbox", "sk-or-v1-abcd1234ef56", "50", "openrouter/auto")
                .unwrap();
            db.save_preferences(&prefs).unwrap();
        }

        let reopened = ProjectDatabase::open(&db_path).unwrap();
        let loaded = reopened.load_preferences().unwrap().unwrap();
        assert_eq!(loaded.appearance.mode, crate::AppearanceMode::Dark);
        assert!(loaded.openrouter_key("openrouter-acme").unwrap().is_default);
        assert_eq!(loaded.openrouter_keys.len(), 3);
    }

    #[test]
    fn migrates_legacy_pre_demo_mode_preferences_by_clearing_sample_providers() {
        let db = ProjectDatabase::open_in_memory().unwrap();
        let mut legacy = AppPreferences::sample();
        legacy.schema_version = 0;
        legacy.appearance.mode = crate::AppearanceMode::Dark;
        legacy.general.default_projects_dir = "~/code".to_owned();
        db.save_preferences(&legacy).unwrap();

        let migrated = db.load_preferences().unwrap().unwrap();
        assert_eq!(
            migrated.schema_version,
            crate::preferences::PREFERENCES_SCHEMA_VERSION
        );
        assert!(!migrated.demo_mode);
        assert!(!migrated.claude.connected);
        assert!(migrated.codex.is_none());
        assert!(migrated.openrouter_keys.is_empty());
        assert!(migrated.extensions.entries.is_empty());
        assert_eq!(migrated.appearance.mode, crate::AppearanceMode::Dark);
        assert_eq!(migrated.general.default_projects_dir, "~/code");
    }

    #[test]
    fn migrates_old_default_projects_dir_into_bruk_folder_but_keeps_custom_dirs() {
        let home = BrukHome::at("/Users/dev/Bruk");
        let db = ProjectDatabase::open_in_memory().unwrap();

        let mut untouched = AppPreferences::empty();
        untouched.schema_version = 3;
        untouched.general.default_projects_dir = "~/src".to_owned();
        db.save_preferences(&untouched).unwrap();
        let migrated = db.load_preferences().unwrap().unwrap();
        assert_eq!(
            migrated.schema_version,
            crate::preferences::PREFERENCES_SCHEMA_VERSION
        );
        assert!(migrated.general.projects_dir_follows_home());
        assert_eq!(
            migrated.general.projects_dir(&home),
            Path::new("/Users/dev/Bruk/projects")
        );

        let mut custom = AppPreferences::empty();
        custom.schema_version = 3;
        custom.general.default_projects_dir = "/Volumes/work/repos".to_owned();
        db.save_preferences(&custom).unwrap();
        let kept = db.load_preferences().unwrap().unwrap();
        assert_eq!(kept.general.default_projects_dir, "/Volumes/work/repos");
        assert_eq!(
            kept.general.projects_dir(&home),
            Path::new("/Volumes/work/repos")
        );

        // Fresh preferences follow the Bruk folder from the start.
        assert!(AppPreferences::empty().general.projects_dir_follows_home());
    }

    #[test]
    fn migrates_v2_unconnected_codex_placeholder_while_preserving_other_settings() {
        let db = ProjectDatabase::open_in_memory().unwrap();
        let mut v2 = AppPreferences::empty();
        v2.schema_version = 2;
        v2.claude.custom_cli_path = Some("/opt/bin/claude".to_owned());
        v2.claude.default_model = "sonnet".to_owned();
        v2.add_codex_account("codex", "gpt-5.3-codex").unwrap();
        v2.add_openrouter_key("Personal", "sk-or-v1-abcd1234ef56", "25", "openrouter/auto")
            .unwrap();
        db.save_preferences(&v2).unwrap();

        let migrated = db.load_preferences().unwrap().unwrap();
        assert_eq!(
            migrated.schema_version,
            crate::preferences::PREFERENCES_SCHEMA_VERSION
        );
        assert!(migrated.codex.is_none());
        assert_eq!(
            migrated.claude.custom_cli_path.as_deref(),
            Some("/opt/bin/claude")
        );
        assert_eq!(migrated.claude.default_model, "sonnet");
        assert_eq!(migrated.openrouter_keys.len(), 1);
        assert!(migrated.claude_spawn_settings.is_empty());
        assert!(migrated.codex_spawn_settings.is_empty());
    }

    #[test]
    fn persists_claude_spawn_settings_across_reopen() {
        let temp = TempDir::new().unwrap();
        let db_path = temp.path().join("factory.db");

        {
            let db = ProjectDatabase::open(&db_path).unwrap();
            let mut prefs = AppPreferences::empty();
            prefs
                .claude_spawn_settings
                .set_from_text("cli.permission-mode", "acceptEdits")
                .unwrap();
            prefs
                .claude_spawn_settings
                .set_value(
                    "alwaysThinkingEnabled",
                    crate::ClaudeSettingValue::Bool(true),
                )
                .unwrap();
            prefs
                .claude_spawn_settings
                .set_from_text("permissions.allow", "Bash(cargo test*), Read(*)")
                .unwrap();
            prefs
                .claude_spawn_settings
                .set_env_var("DISABLE_TELEMETRY", "1")
                .unwrap();
            prefs
                .claude_spawn_settings
                .set_extra_cli_args("--max-turns 15")
                .unwrap();
            prefs
                .claude_spawn_settings
                .set_extra_settings_json(r#"{"customKey":"abc"}"#)
                .unwrap();
            db.save_preferences(&prefs).unwrap();
        }

        let reopened = ProjectDatabase::open(&db_path).unwrap();
        let loaded = reopened.load_preferences().unwrap().unwrap();
        assert_eq!(loaded.claude_spawn_settings.override_count(), 6);
        assert_eq!(
            loaded.claude_spawn_settings.get("cli.permission-mode"),
            Some(&crate::ClaudeSettingValue::String("acceptEdits".to_owned()))
        );
        assert_eq!(
            loaded.claude_spawn_settings.get("alwaysThinkingEnabled"),
            Some(&crate::ClaudeSettingValue::Bool(true))
        );
        assert_eq!(
            loaded
                .claude_spawn_settings
                .env_vars
                .get("DISABLE_TELEMETRY")
                .map(String::as_str),
            Some("1")
        );
        assert_eq!(
            loaded.claude_spawn_settings.extra_cli_args,
            "--max-turns 15"
        );
    }

    #[test]
    fn persists_codex_spawn_settings_across_reopen() {
        let temp = TempDir::new().unwrap();
        let db_path = temp.path().join("factory.db");

        {
            let db = ProjectDatabase::open(&db_path).unwrap();
            let mut prefs = AppPreferences::empty();
            prefs
                .codex_spawn_settings
                .set_from_text("cli.sandbox", "workspace-write")
                .unwrap();
            prefs
                .codex_spawn_settings
                .set_value(
                    "sandbox_workspace_write.network_access",
                    crate::CodexSettingValue::Bool(true),
                )
                .unwrap();
            prefs
                .codex_spawn_settings
                .set_from_text(
                    "sandbox_workspace_write.writable_roots",
                    "/tmp/cache, /opt/tools",
                )
                .unwrap();
            prefs
                .codex_spawn_settings
                .set_env_var("NO_COLOR", "1")
                .unwrap();
            prefs
                .codex_spawn_settings
                .set_extra_cli_args("--ephemeral")
                .unwrap();
            prefs
                .codex_spawn_settings
                .set_extra_config_overrides("custom_flag = true")
                .unwrap();
            db.save_preferences(&prefs).unwrap();
        }

        let reopened = ProjectDatabase::open(&db_path).unwrap();
        let loaded = reopened.load_preferences().unwrap().unwrap();
        assert_eq!(loaded.codex_spawn_settings.override_count(), 6);
        assert_eq!(
            loaded.codex_spawn_settings.get("cli.sandbox"),
            Some(&crate::CodexSettingValue::String(
                "workspace-write".to_owned()
            ))
        );
        assert_eq!(
            loaded
                .codex_spawn_settings
                .get("sandbox_workspace_write.network_access"),
            Some(&crate::CodexSettingValue::Bool(true))
        );
        assert_eq!(
            loaded
                .codex_spawn_settings
                .env_vars
                .get("NO_COLOR")
                .map(String::as_str),
            Some("1")
        );
        assert_eq!(loaded.codex_spawn_settings.extra_cli_args, "--ephemeral");
        assert_eq!(
            loaded.codex_spawn_settings.extra_config_overrides,
            "custom_flag = true"
        );
    }

    #[test]
    fn persists_project_settings_secrets_and_work_queue_across_reopen() {
        let temp = TempDir::new().unwrap();
        let db_path = temp.path().join("factory.db");

        let project_id = {
            let db = ProjectDatabase::open(&db_path).unwrap();
            let project = db
                .register_project("billing-api", "/Users/dev/billing-api", "main", "")
                .unwrap();

            let mut settings =
                ProjectSettings::empty("billing-api", "/Users/dev/billing-api", "main");
            settings.github_repo = "acme/billing-api".to_owned();
            settings
                .add_intake_filter(crate::GitHubFilterClause::new(
                    crate::GitHubFilterQualifier::Label,
                    "agent-ready",
                ))
                .unwrap();
            settings.add_intake_rule("bug", "Fix issue").unwrap();
            settings.set_fallback_workflow("Triage new issues");
            settings.set_github_token("ghp_super_secret_1234").unwrap();
            db.save_project_settings(project.id, &settings).unwrap();
            db.save_project_secret(project.id, "github_pat", "ghp_super_secret_1234")
                .unwrap();

            let mut queue = ProjectWorkQueue::empty("billing-api");
            queue.repo_slug = "acme/billing-api".to_owned();
            queue.tracker_name = "GitHub Issues".to_owned();
            db.save_project_work_queue(project.id, &queue).unwrap();
            project.id
        };

        let reopened = ProjectDatabase::open(&db_path).unwrap();
        let loaded_settings = reopened.load_project_settings(project_id).unwrap().unwrap();
        assert_eq!(loaded_settings.github_repo, "acme/billing-api");
        assert_eq!(loaded_settings.intake_rules.len(), 2);
        assert_eq!(loaded_settings.github_token_last4.as_deref(), Some("1234"));

        let secret = reopened
            .load_project_secret(project_id, "github_pat")
            .unwrap();
        assert_eq!(secret.as_deref(), Some("ghp_super_secret_1234"));

        let loaded_queue = reopened
            .load_project_work_queue(project_id)
            .unwrap()
            .unwrap();
        assert_eq!(loaded_queue.repo_slug, "acme/billing-api");

        // Deleting the project cascades and removes settings, secrets, and work queue.
        assert!(reopened.delete_project(project_id).unwrap());
        assert_eq!(reopened.load_project_settings(project_id).unwrap(), None);
        assert_eq!(
            reopened
                .load_project_secret(project_id, "github_pat")
                .unwrap(),
            None
        );
        assert_eq!(reopened.load_project_work_queue(project_id).unwrap(), None);
    }

    #[test]
    fn persists_and_lists_workflow_runs_with_cascade_delete() {
        let temp = TempDir::new().unwrap();
        let db_path = temp.path().join("factory.db");
        let project_id = {
            let db = ProjectDatabase::open(&db_path).unwrap();
            let project = db
                .register_project("billing-api", "/Users/dev/billing-api", "main", "")
                .unwrap();
            assert_eq!(db.next_run_number_for_project(project.id).unwrap(), 1);
            let run = PersistedWorkflowRun {
                id: "run-1".to_owned(),
                project_id: project.id,
                project_name: "billing-api".to_owned(),
                workflow_id: "fix-issue".to_owned(),
                workflow_name: "Fix issue".to_owned(),
                run_number: 1,
                status: RunExecutionState::Succeeded,
                provider: "Claude".to_owned(),
                issue_number: Some(42),
                branch_name: "agent/issue-42-run-1".to_owned(),
                total_tokens: 1500,
                total_cost_cents: 12,
                duration_ms: 4200,
                summary: "Completed 'Fix issue' (2 steps)".to_owned(),
                view: WorkflowRunViewData::empty("billing-api", "Fix issue"),
                started_at: 1_700_000_000,
                finished_at: Some(1_700_000_004),
            };
            db.save_workflow_run(&run).unwrap();
            assert_eq!(db.next_run_number_for_project(project.id).unwrap(), 2);
            project.id
        };

        let reopened = ProjectDatabase::open(&db_path).unwrap();
        let runs = reopened.list_workflow_runs_for_project(project_id).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].total_tokens, 1500);
        assert_eq!(runs[0].issue_number, Some(42));
        assert_eq!(reopened.list_all_workflow_runs().unwrap().len(), 1);

        assert!(reopened.delete_project(project_id).unwrap());
        assert!(reopened.list_all_workflow_runs().unwrap().is_empty());
    }
}
