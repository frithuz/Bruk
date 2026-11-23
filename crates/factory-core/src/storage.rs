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
};

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
}
