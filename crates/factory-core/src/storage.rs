//! SQLite persistence for tracked projects stored in the user's config folder.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, ErrorCode, OptionalExtension, params};
use thiserror::Error;

use crate::{
    model::{Project, ProjectId},
    preferences::AppPreferences,
    project_settings::ProjectSettings,
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

const APP_CONFIG_DIR: &str = "software-factory";
const DB_FILE_NAME: &str = "factory.db";
const DB_PATH_ENV: &str = "SOFTWARE_FACTORY_DB_PATH";
const PREFERENCES_KEY: &str = "app_preferences";

/// Errors returned when opening or mutating the local SQLite project database.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("project name cannot be empty")]
    EmptyProjectName,
    #[error("repository path cannot be empty")]
    EmptyProjectPath,
    #[error("a project is already registered for repository path '{0}'")]
    DuplicateRepositoryPath(PathBuf),
    #[error("could not determine user config directory")]
    ConfigDirUnavailable,
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

/// Resolves the default SQLite database path inside the user's config folder.
///
/// Resolution order:
/// 1. `SOFTWARE_FACTORY_DB_PATH` environment variable (explicit override)
/// 2. `$XDG_CONFIG_HOME/software-factory/factory.db` when `XDG_CONFIG_HOME` is set
/// 3. The platform user config directory (`dirs::config_dir()` ->
///    `~/Library/Application Support/software-factory/factory.db` on macOS)
pub fn default_db_path() -> Result<PathBuf, StorageError> {
    if let Ok(explicit) = env::var(DB_PATH_ENV) {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        let trimmed = xdg.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed)
                .join(APP_CONFIG_DIR)
                .join(DB_FILE_NAME));
        }
    }

    let config_dir = dirs::config_dir().ok_or(StorageError::ConfigDirUnavailable)?;
    Ok(config_dir.join(APP_CONFIG_DIR).join(DB_FILE_NAME))
}

/// Persistent SQLite store for tracking registered projects on this machine.
pub struct ProjectDatabase {
    conn: Connection,
    path: Option<PathBuf>,
}

impl ProjectDatabase {
    /// Opens (creating if needed) the SQLite database in the user's config folder.
    pub fn open_default() -> Result<Self, StorageError> {
        let path = default_db_path()?;
        Self::open(path)
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
            path: Some(path),
        };
        db.init_schema()?;
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
             CREATE TABLE IF NOT EXISTS project_secrets (
                 project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                 key        TEXT NOT NULL,
                 value      TEXT NOT NULL,
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 PRIMARY KEY (project_id, key)
             );
             CREATE TABLE IF NOT EXISTS project_work_queues (
                 project_id INTEGER PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
                 value_json TEXT NOT NULL,
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch())
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
             );",
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
        Ok(Project {
            id,
            name: name.to_owned(),
            root_path: PathBuf::from(trimmed_path),
            default_branch: default_branch.to_owned(),
            description: description.to_owned(),
        })
    }

    /// Removes a tracked project by id. Returns `true` if a row was deleted.
    pub fn delete_project(&self, id: ProjectId) -> Result<bool, StorageError> {
        let affected = self
            .conn
            .execute("DELETE FROM projects WHERE id = ?1", params![id as i64])?;
        Ok(affected > 0)
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

    /// Loads a secret value associated with a project (for example `"github_pat"`).
    pub fn load_project_secret(
        &self,
        project_id: ProjectId,
        key: &str,
    ) -> Result<Option<String>, StorageError> {
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

    /// Stores or updates a secret value associated with a project.
    pub fn save_project_secret(
        &self,
        project_id: ProjectId,
        key: &str,
        value: &str,
    ) -> Result<(), StorageError> {
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

    /// Deletes a secret value associated with a project.
    pub fn delete_project_secret(
        &self,
        project_id: ProjectId,
        key: &str,
    ) -> Result<bool, StorageError> {
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
    fn resolves_default_db_path_inside_config_folder() {
        let path = default_db_path().unwrap();
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("factory.db")
        );
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
