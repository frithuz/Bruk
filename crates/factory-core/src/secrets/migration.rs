//! Idempotent, crash-safe migration from legacy plaintext storage into OS-native secret stores.
//!
//! Implements §11 of the Bruk Secret Storage specification:
//! 1. `project_secrets` GitHub PATs -> named App `Internal { purpose: GitHubPat }`
//! 2. `ProjectEnvVar` with `is_secret == true` -> Project Env secret (`Exposure::AgentAndSteps`)
//! 3. Inline MCP tokens (including disabled servers, App & Project) -> `${NAME}` / `Bearer ${NAME}`
//! 4. Claude & Codex spawn-settings credential env vars -> `Internal { purpose: ProviderSpawnEnv }`
//! 5. OpenRouter keys -> App `Internal { purpose: OpenRouterKey }` in `NeedsValue`, updated `cap_note`
//! 6. Structured JSON transcript scrubbing (`workflow_runs`, `chat_sessions`, `chat_usage`)
//! 7. `DROP TABLE project_secrets`, `PRAGMA wal_checkpoint(TRUNCATE)`, `VACUUM`, `secrets_migration_v1`
//! 8. Legacy `factory.db` + `-wal`/`-shm` scanner & user-confirmed deletion helper
//! 9. Rotation notice metadata ([`MigrationReport`])

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use secrecy::SecretString;
use zeroize::Zeroize;

use crate::{
    github_intake::current_unix_seconds,
    mcp::{McpServerDef, McpTransport},
    model::ProjectId,
    project_settings::ProjectSettings,
    storage::{ProjectDatabase, StorageError},
};

use super::{
    EnvScope, Exposure, InternalPurpose, SecretKind, SecretRecord, SecretSource, SecretState,
    Sharing, compute_preview, contains_env_reference, keychain_account_key, keychain_item_label,
    looks_like_secret_key, new_secret_id,
    redactor::SecretRedactor,
    sanitize_env_segment,
    store::{SecretStore, StoreStatus, write_and_verify},
};

/// `app_settings` marker written once Step 7 completes.
pub const SECRETS_MIGRATION_V1_KEY: &str = "secrets_migration_v1";

/// Sidecar suffixes scanned alongside legacy `factory.db`.
pub const LEGACY_DATABASE_SIDE_EXTENSIONS: &[&str] = &["", "-wal", "-shm"];

/// Resolution chosen by the user when `bruk.db` was opened at a new filesystem path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathChangeResolution {
    Moved,
    Cloned,
}

/// Summary returned by [`migrate_plaintext_secrets`] so the UI can file a single Inbox item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MigrationReport {
    /// True if migration was skipped (already done, Demo Mode, or MemoryStore guard).
    pub skipped: bool,
    /// Non-empty if migration aborted before modifying anything (e.g. Keychain locked/unavailable).
    pub aborted_reason: Option<String>,
    /// Names of value-bearing credentials migrated to the OS Keychain (advised to rotate).
    pub migrated_names: Vec<String>,
    /// Names of OpenRouter keys created in `NeedsValue` (raw key was never previously stored).
    pub openrouter_needs_value: Vec<String>,
    /// Names of migrated Project Env vars kept at `AgentAndSteps` for review.
    pub agent_and_steps_for_review: Vec<String>,
    /// Number of persisted workflow runs / chats scrubbed in Step 6.
    pub scrubbed_records: usize,
    /// True once Step 7 (`DROP TABLE project_secrets`, checkpoint, `VACUUM`) completed.
    pub disk_cleanup_completed: bool,
}

impl MigrationReport {
    pub fn has_user_notice(&self) -> bool {
        !self.migrated_names.is_empty()
            || !self.openrouter_needs_value.is_empty()
            || !self.agent_and_steps_for_review.is_empty()
    }

    pub fn rotation_notice_body(&self) -> String {
        let mut lines = Vec::new();
        if !self.migrated_names.is_empty() {
            lines.push(format!(
                "Migrated {} credential(s) into the OS Keychain: {}. Because plaintext previously existed in SQLite and OS backups, consider rotating these credentials.",
                self.migrated_names.len(),
                self.migrated_names.join(", ")
            ));
        }
        if !self.openrouter_needs_value.is_empty() {
            lines.push(format!(
                "OpenRouter key(s) needing re-entry (raw keys were not saved previously): {}.",
                self.openrouter_needs_value.join(", ")
            ));
        }
        if !self.agent_and_steps_for_review.is_empty() {
            lines.push(format!(
                "Migrated project env secret(s) kept as 'Agent and steps' to preserve existing workflows (review in Project Settings › Environment & Secrets): {}.",
                self.agent_and_steps_for_review.join(", ")
            ));
        }
        lines.join("\n\n")
    }
}

/// Runs the idempotent, crash-safe plaintext-to-Keychain migration on `db`.
///
/// - Never runs when `demo_mode` is true or when `allow_memory_store` is false and `store.is_memory()`.
/// - Aborts without modifying anything if `store.status()` is `Locked` or `Unavailable`.
pub fn migrate_plaintext_secrets(
    db: &ProjectDatabase,
    store: &dyn SecretStore,
    demo_mode: bool,
    allow_memory_store: bool,
) -> Result<MigrationReport, StorageError> {
    let mut report = MigrationReport::default();

    if demo_mode || (!allow_memory_store && store.is_memory()) {
        report.skipped = true;
        return Ok(report);
    }

    if db.get_app_setting_raw(SECRETS_MIGRATION_V1_KEY)?.is_some() {
        report.skipped = true;
        return Ok(report);
    }

    match store.status() {
        StoreStatus::Available => {}
        StoreStatus::Locked => {
            report.aborted_reason = Some(
                "OS Keychain is locked. Unlock your Keychain so Bruk can migrate existing credentials out of plaintext SQLite.".to_owned(),
            );
            return Ok(report);
        }
        StoreStatus::Unavailable(msg) => {
            report.aborted_reason = Some(format!(
                "OS Keychain is unavailable ({msg}). Bruk aborted credential migration without modifying your database."
            ));
            return Ok(report);
        }
    }

    let db_instance_id = db.ensure_db_instance_id()?;
    let now = current_unix_seconds();
    let projects = db.list_projects()?;
    let project_names: BTreeMap<ProjectId, String> =
        projects.iter().map(|p| (p.id, p.name.clone())).collect();

    let mut prefs = db.load_preferences()?.unwrap_or_default();
    let mut prefs_dirty = false;

    // Collect all migrated (label, raw_value) pairs for Step 6 transcript scrubbing.
    let mut migrated_raw_pairs: Vec<(String, String)> = Vec::new();
    let mut had_value_bearing_failure = false;

    // Track existing Env secret & plain var names case-insensitively across App + Projects.
    let mut all_taken_lower: BTreeSet<String> = db
        .list_secrets()?
        .into_iter()
        .filter(|r| !r.is_internal())
        .map(|r| r.name.to_ascii_lowercase())
        .collect();
    let mut project_settings_map: BTreeMap<ProjectId, ProjectSettings> = BTreeMap::new();
    for p in &projects {
        if let Some(ps) = db.load_project_settings(p.id)? {
            for ev in &ps.env_vars {
                all_taken_lower.insert(ev.key.to_ascii_lowercase());
            }
            project_settings_map.insert(p.id, ps);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Step 1: GitHub PATs from `project_secrets` -> named App Internal { purpose: GitHubPat }
    // -----------------------------------------------------------------------------------------
    if db.has_legacy_project_secrets_table()? {
        for (project_id, key, mut raw_val) in db.list_all_legacy_project_secrets()? {
            if key != "github_pat" {
                raw_val.zeroize();
                continue;
            }
            let trimmed = raw_val.trim().to_owned();
            raw_val.zeroize();
            if trimmed.is_empty() {
                let _ = db.delete_project_secret(project_id, &key);
                continue;
            }
            let proj_name = project_names
                .get(&project_id)
                .cloned()
                .unwrap_or_else(|| format!("project-{project_id}"));
            let name = format!("GitHub PAT ({proj_name})");
            let record = SecretRecord {
                id: new_secret_id(),
                name: name.clone(),
                kind: SecretKind::Internal {
                    purpose: InternalPurpose::GitHubPat,
                },
                source: SecretSource::Keychain,
                state: SecretState::NeedsValue,
                description: format!("Migrated GitHub PAT for {proj_name}"),
                preview: None,
                created_at: now,
                updated_at: now,
                last_used_at: None,
            };
            let secret_val = SecretString::from(trimmed.clone());
            match atomic_migrate_keychain_secret(
                db,
                store,
                &db_instance_id,
                &record,
                &secret_val,
                Some(&proj_name),
                || {
                    if let Some(ps) = project_settings_map.get_mut(&project_id) {
                        ps.github_pat_id = Some(record.id.clone());
                        ps.github_token_last4 = None;
                        db.save_project_settings(project_id, ps)?;
                    }
                    db.delete_project_secret(project_id, &key)?;
                    Ok(())
                },
            ) {
                Ok(()) => {
                    report.migrated_names.push(name);
                    migrated_raw_pairs.push(("internal".to_owned(), trimmed));
                }
                Err(_) => {
                    had_value_bearing_failure = true;
                }
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Step 2: ProjectEnvVar with `is_secret == true` -> Project Env secret (AgentAndSteps)
    // -----------------------------------------------------------------------------------------
    for p in &projects {
        let Some(ps) = project_settings_map.get(&p.id).cloned() else {
            continue;
        };
        let secret_vars: Vec<_> = ps
            .env_vars
            .iter()
            .filter(|v| v.is_secret)
            .cloned()
            .collect();
        if secret_vars.is_empty() {
            continue;
        }
        for ev in secret_vars {
            let trimmed = ev.value.trim().to_owned();
            if trimmed.is_empty() {
                if let Some(cur) = project_settings_map.get_mut(&p.id) {
                    cur.env_vars.retain(|v| v.id != ev.id);
                    db.save_project_settings(p.id, cur)?;
                }
                continue;
            }
            let var_name = if super::validate_env_name(&ev.key).is_ok() {
                ev.key.clone()
            } else {
                sanitize_env_segment(&ev.key)
            };
            let record = SecretRecord {
                id: new_secret_id(),
                name: var_name.clone(),
                kind: SecretKind::Env {
                    scope: EnvScope::Project(p.id),
                    exposure: Exposure::AgentAndSteps,
                },
                source: SecretSource::Keychain,
                state: SecretState::NeedsValue,
                description: "Migrated from project secret variable".to_owned(),
                preview: None,
                created_at: now,
                updated_at: now,
                last_used_at: None,
            };
            let secret_val = SecretString::from(trimmed.clone());
            match atomic_migrate_keychain_secret(
                db,
                store,
                &db_instance_id,
                &record,
                &secret_val,
                Some(&p.name),
                || {
                    if let Some(cur) = project_settings_map.get_mut(&p.id) {
                        cur.env_vars.retain(|v| v.id != ev.id);
                        db.save_project_settings(p.id, cur)?;
                    }
                    Ok(())
                },
            ) {
                Ok(()) => {
                    all_taken_lower.insert(var_name.to_ascii_lowercase());
                    report
                        .migrated_names
                        .push(format!("{var_name} ({})", p.name));
                    report
                        .agent_and_steps_for_review
                        .push(format!("{var_name} ({})", p.name));
                    migrated_raw_pairs.push((var_name, trimmed));
                }
                Err(_) => {
                    had_value_bearing_failure = true;
                }
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Step 3: Inline MCP tokens (including disabled servers, App & Project)
    // -----------------------------------------------------------------------------------------
    // 3a: User-all-projects MCP servers -> App Env secrets (AllProjects, McpOnly)
    let mut user_mcp_servers = prefs.extensions.mcp_servers.clone();
    let mut user_mcp_changed = false;
    for server in &mut user_mcp_servers {
        let extracted = extract_mcp_server_secrets(server, &mut all_taken_lower);
        for item in extracted {
            let record = SecretRecord {
                id: new_secret_id(),
                name: item.secret_name.clone(),
                kind: SecretKind::Env {
                    scope: EnvScope::App {
                        shared_with: Sharing::AllProjects,
                    },
                    exposure: Exposure::McpOnly,
                },
                source: SecretSource::Keychain,
                state: SecretState::NeedsValue,
                description: format!("Migrated from MCP server '{}'", server.name),
                preview: None,
                created_at: now,
                updated_at: now,
                last_used_at: None,
            };
            let secret_val = SecretString::from(item.raw_value.clone());
            let mut updated_server = server.clone();
            apply_mcp_replacement(
                &mut updated_server,
                &item.field_key,
                &item.replacement_value,
                item.is_header,
            );
            match atomic_migrate_keychain_secret(
                db,
                store,
                &db_instance_id,
                &record,
                &secret_val,
                None,
                || {
                    *server = updated_server.clone();
                    Ok(())
                },
            ) {
                Ok(()) => {
                    user_mcp_changed = true;
                    report.migrated_names.push(item.secret_name.clone());
                    migrated_raw_pairs.push((item.secret_name, item.raw_value));
                }
                Err(_) => {
                    had_value_bearing_failure = true;
                }
            }
        }
    }
    if user_mcp_changed {
        prefs.extensions.mcp_servers = user_mcp_servers;
        db.save_preferences(&prefs)?;
        prefs_dirty = true;
    }

    // 3b: Project-extra MCP servers -> Project Env secrets (McpOnly)
    for p in &projects {
        let Some(mut ps) = project_settings_map.get(&p.id).cloned() else {
            continue;
        };
        let mut changed = false;
        for server in &mut ps.mcp_servers {
            let extracted = extract_mcp_server_secrets(server, &mut all_taken_lower);
            for item in extracted {
                let record = SecretRecord {
                    id: new_secret_id(),
                    name: item.secret_name.clone(),
                    kind: SecretKind::Env {
                        scope: EnvScope::Project(p.id),
                        exposure: Exposure::McpOnly,
                    },
                    source: SecretSource::Keychain,
                    state: SecretState::NeedsValue,
                    description: format!("Migrated from MCP server '{}' ({})", server.name, p.name),
                    preview: None,
                    created_at: now,
                    updated_at: now,
                    last_used_at: None,
                };
                let secret_val = SecretString::from(item.raw_value.clone());
                let mut updated_server = server.clone();
                apply_mcp_replacement(
                    &mut updated_server,
                    &item.field_key,
                    &item.replacement_value,
                    item.is_header,
                );
                match atomic_migrate_keychain_secret(
                    db,
                    store,
                    &db_instance_id,
                    &record,
                    &secret_val,
                    Some(&p.name),
                    || {
                        *server = updated_server.clone();
                        Ok(())
                    },
                ) {
                    Ok(()) => {
                        changed = true;
                        report
                            .migrated_names
                            .push(format!("{} ({})", item.secret_name, p.name));
                        migrated_raw_pairs.push((item.secret_name, item.raw_value));
                    }
                    Err(_) => {
                        had_value_bearing_failure = true;
                    }
                }
            }
        }
        if changed {
            db.save_project_settings(p.id, &ps)?;
            project_settings_map.insert(p.id, ps);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Step 4: Claude & Codex spawn-settings credential env vars -> Internal { ProviderSpawnEnv }
    // -----------------------------------------------------------------------------------------
    let claude_keys: Vec<(String, String)> = prefs
        .claude_spawn_settings
        .env_vars
        .iter()
        .filter(|(k, v)| {
            looks_like_secret_key(k) && !v.trim().is_empty() && !contains_env_reference(v)
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, raw_val) in claude_keys {
        let record = SecretRecord {
            id: new_secret_id(),
            name: format!("Claude {k}"),
            kind: SecretKind::Internal {
                purpose: InternalPurpose::ProviderSpawnEnv,
            },
            source: SecretSource::Keychain,
            state: SecretState::NeedsValue,
            description: format!("Migrated from Claude spawn settings ({k})"),
            preview: None,
            created_at: now,
            updated_at: now,
            last_used_at: None,
        };
        let secret_val = SecretString::from(raw_val.clone());
        match atomic_migrate_keychain_secret(
            db,
            store,
            &db_instance_id,
            &record,
            &secret_val,
            None,
            || {
                prefs.claude_spawn_settings.env_vars.remove(&k);
                prefs
                    .claude_spawn_settings
                    .secret_env_vars
                    .insert(k.clone(), record.id.clone());
                db.save_preferences(&prefs)?;
                Ok(())
            },
        ) {
            Ok(()) => {
                prefs_dirty = true;
                report.migrated_names.push(format!("Claude {k}"));
                migrated_raw_pairs.push(("internal".to_owned(), raw_val));
            }
            Err(_) => {
                had_value_bearing_failure = true;
            }
        }
    }

    let codex_keys: Vec<(String, String)> = prefs
        .codex_spawn_settings
        .env_vars
        .iter()
        .filter(|(k, v)| {
            looks_like_secret_key(k) && !v.trim().is_empty() && !contains_env_reference(v)
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, raw_val) in codex_keys {
        let record = SecretRecord {
            id: new_secret_id(),
            name: format!("Codex {k}"),
            kind: SecretKind::Internal {
                purpose: InternalPurpose::ProviderSpawnEnv,
            },
            source: SecretSource::Keychain,
            state: SecretState::NeedsValue,
            description: format!("Migrated from Codex spawn settings ({k})"),
            preview: None,
            created_at: now,
            updated_at: now,
            last_used_at: None,
        };
        let secret_val = SecretString::from(raw_val.clone());
        match atomic_migrate_keychain_secret(
            db,
            store,
            &db_instance_id,
            &record,
            &secret_val,
            None,
            || {
                prefs.codex_spawn_settings.env_vars.remove(&k);
                prefs
                    .codex_spawn_settings
                    .secret_env_vars
                    .insert(k.clone(), record.id.clone());
                db.save_preferences(&prefs)?;
                Ok(())
            },
        ) {
            Ok(()) => {
                prefs_dirty = true;
                report.migrated_names.push(format!("Codex {k}"));
                migrated_raw_pairs.push(("internal".to_owned(), raw_val));
            }
            Err(_) => {
                had_value_bearing_failure = true;
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Step 5: OpenRouter keys -> App Internal { OpenRouterKey } in NeedsValue (does NOT block Step 7)
    // -----------------------------------------------------------------------------------------
    for or_key in &mut prefs.openrouter_keys {
        if or_key.secret_id.is_none() {
            let record = SecretRecord {
                id: new_secret_id(),
                name: or_key.name.clone(),
                kind: SecretKind::Internal {
                    purpose: InternalPurpose::OpenRouterKey,
                },
                source: SecretSource::Keychain,
                state: SecretState::NeedsValue,
                description: format!("OpenRouter API key '{}'", or_key.name),
                preview: None,
                created_at: now,
                updated_at: now,
                last_used_at: None,
            };
            db.insert_secret_row(&record)?;
            or_key.secret_id = Some(record.id);
            or_key.cap_note = "Key needs to be re-entered in Preferences › Providers or Secrets (previous versions did not store the raw key).".to_owned();
            report.openrouter_needs_value.push(or_key.name.clone());
            prefs_dirty = true;
        }
    }
    if prefs_dirty {
        db.save_preferences(&prefs)?;
    }

    // -----------------------------------------------------------------------------------------
    // Step 6: Structured JSON transcript scrubbing (valid JSON preserved!)
    // -----------------------------------------------------------------------------------------
    if !migrated_raw_pairs.is_empty() {
        let redactor = SecretRedactor::from_raw_pairs(migrated_raw_pairs.clone());
        if !redactor.is_empty() {
            report.scrubbed_records = db.scrub_persisted_transcripts(&redactor)?;
        }
        for (_, mut v) in migrated_raw_pairs {
            v.zeroize();
        }
    }

    // -----------------------------------------------------------------------------------------
    // Step 7: Disk cleanup once all value-bearing items succeed
    // -----------------------------------------------------------------------------------------
    if !had_value_bearing_failure {
        db.drop_legacy_project_secrets_and_vacuum()?;
        db.set_app_setting_raw(SECRETS_MIGRATION_V1_KEY, &now.to_string())?;
        report.disk_cleanup_completed = true;
    }

    Ok(report)
}

/// Per-item atomic migration rule (§11):
/// insert `needs_value` -> write & verify Keychain -> in one SQLite operation mark `ready` + clear plaintext.
fn atomic_migrate_keychain_secret(
    db: &ProjectDatabase,
    store: &dyn SecretStore,
    db_instance_id: &str,
    record: &SecretRecord,
    value: &SecretString,
    project_name: Option<&str>,
    clear_source: impl FnOnce() -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    db.insert_secret_row(record)?;
    let account_key = keychain_account_key(db_instance_id, &record.id);
    let label = keychain_item_label(record, project_name);
    if let Err(err) = write_and_verify(store, &account_key, &label, value) {
        // Roll back the `needs_value` row and leave plaintext untouched for retry.
        let _ = db.delete_secret_row_only(&record.id);
        return Err(StorageError::Secret(err.to_string()));
    }
    let preview = {
        use secrecy::ExposeSecret;
        compute_preview(value.expose_secret())
    };
    let mut ready = record.clone();
    ready.state = SecretState::Ready;
    ready.preview = preview;
    db.update_secret_row(&ready)?;
    clear_source()?;
    Ok(())
}

struct ExtractedMcpSecret {
    field_key: String,
    is_header: bool,
    secret_name: String,
    raw_value: String,
    replacement_value: String,
}

fn extract_mcp_server_secrets(
    server: &McpServerDef,
    taken_lower: &mut BTreeSet<String>,
) -> Vec<ExtractedMcpSecret> {
    let mut out = Vec::new();
    match &server.transport {
        McpTransport::Stdio { env, .. } => {
            for (key, value) in env {
                if !looks_like_secret_key(key)
                    || value.trim().is_empty()
                    || contains_env_reference(value)
                {
                    continue;
                }
                let secret_name = unique_migrated_name(&server.name, key, taken_lower);
                out.push(ExtractedMcpSecret {
                    field_key: key.clone(),
                    is_header: false,
                    replacement_value: format!("${{{secret_name}}}"),
                    secret_name,
                    raw_value: value.clone(),
                });
            }
        }
        McpTransport::Http { headers, .. } => {
            for (key, value) in headers {
                if !looks_like_secret_key(key)
                    || value.trim().is_empty()
                    || contains_env_reference(value)
                {
                    continue;
                }
                let secret_name = unique_migrated_name(&server.name, key, taken_lower);
                // Preserve `Bearer ${NAME}` when Authorization starts with `Bearer ` (Codex compatibility).
                let (raw_value, replacement_value) = if key.eq_ignore_ascii_case("authorization")
                    && let Some(token_part) = value.trim().strip_prefix("Bearer ")
                    && !token_part.trim().is_empty()
                {
                    (
                        token_part.trim().to_owned(),
                        format!("Bearer ${{{secret_name}}}"),
                    )
                } else {
                    (value.clone(), format!("${{{secret_name}}}"))
                };
                out.push(ExtractedMcpSecret {
                    field_key: key.clone(),
                    is_header: true,
                    secret_name,
                    raw_value,
                    replacement_value,
                });
            }
        }
    }
    out
}

fn apply_mcp_replacement(
    server: &mut McpServerDef,
    field_key: &str,
    replacement: &str,
    is_header: bool,
) {
    match (&mut server.transport, is_header) {
        (McpTransport::Stdio { env, .. }, false) => {
            env.insert(field_key.to_owned(), replacement.to_owned());
        }
        (McpTransport::Http { headers, .. }, true) => {
            headers.insert(field_key.to_owned(), replacement.to_owned());
        }
        _ => {}
    }
}

fn unique_migrated_name(
    server_name: &str,
    key: &str,
    taken_lower: &mut BTreeSet<String>,
) -> String {
    let base = format!(
        "{}_{}",
        sanitize_env_segment(server_name),
        sanitize_env_segment(key)
    );
    if taken_lower.insert(base.to_ascii_lowercase()) {
        return base;
    }
    let mut suffix = 2u32;
    loop {
        let candidate = format!("{base}_{suffix}");
        if taken_lower.insert(candidate.to_ascii_lowercase()) {
            return candidate;
        }
        suffix += 1;
    }
}

// ---------------------------------------------------------------------------------------------
// Step 8: Legacy database & WAL/SHM scanner + user-confirmed deletion
// ---------------------------------------------------------------------------------------------

/// Scans all `legacy_database_paths` for `factory.db` and any `-wal` / `-shm` sidecars.
pub fn find_legacy_database_files(legacy_database_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for db_path in legacy_database_paths {
        let base = db_path.as_os_str().to_string_lossy();
        for ext in LEGACY_DATABASE_SIDE_EXTENSIONS {
            let candidate = PathBuf::from(format!("{base}{ext}"));
            if candidate.is_file() && !found.contains(&candidate) {
                found.push(candidate);
            }
        }
    }
    found
}

/// Deletes user-confirmed legacy `factory.db` files and their `-wal` / `-shm` sidecars.
pub fn delete_legacy_database_files(paths: &[PathBuf]) -> Result<usize, std::io::Error> {
    let mut removed = 0usize;
    for path in paths {
        if path.is_file() {
            fs::remove_file(path)?;
            removed += 1;
        }
        // Also remove any sidecars if the caller passed only the main `.db` path.
        if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && ext == "db"
        {
            let base = path.as_os_str().to_string_lossy();
            for suffix in ["-wal", "-shm"] {
                let side = Path::new(&format!("{base}{suffix}")).to_path_buf();
                if side.is_file() {
                    fs::remove_file(&side)?;
                    removed += 1;
                }
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use secrecy::{ExposeSecret, SecretString};
    use tempfile::TempDir;

    use super::*;
    use crate::{
        AppPreferences, ChatEntry, ChatSession, ChatWorkspace, OpenRouterKey, ProjectDatabase,
        ProjectSandboxMode, ProjectSettings, ProviderKind,
        mcp::{McpServerDef, McpTransport},
        secrets::{
            EnvScope, Exposure, MemoryStore, SecretError, SecretKind, SecretRecord, SecretState,
            Sharing,
        },
    };

    #[test]
    fn full_plaintext_migration_is_idempotent_scrubs_transcripts_and_preserves_bearer_refs() {
        let temp = TempDir::new().unwrap();
        let db_path = temp.path().join("bruk.db");
        let db = ProjectDatabase::open(&db_path).unwrap();
        let store = MemoryStore::new();

        let project = db
            .register_project("api", "/tmp/api", "main", "git@github.com:acme/api.git")
            .unwrap();

        // 1) Legacy GitHub PAT in project_secrets
        db.save_project_secret(project.id, "github_pat", "ghp_legacy_secret_token_9876")
            .unwrap();

        // 2) ProjectSettings with secret ProjectEnvVar + MCP servers (inline secret, existing Bearer ${REF}, disabled server)
        let mut settings = ProjectSettings::empty("api", "/tmp/api", "main");
        settings
            .add_env_var("STRIPE_SECRET_KEY", "sk_live_legacy_stripe_123456", true)
            .unwrap();
        settings
            .add_env_var("PUBLIC_API_URL", "https://api.example.com", false)
            .unwrap();
        settings.upsert_mcp_server(McpServerDef {
            name: "linear".to_owned(),
            transport: McpTransport::Http {
                url: "https://mcp.linear.app/mcp".to_owned(),
                headers: [
                    (
                        "Authorization".to_owned(),
                        "Bearer lin_api_inline_secret_abc12345".to_owned(),
                    ),
                    (
                        "X-Keep-Ref".to_owned(),
                        "Bearer ${EXISTING_REF_VAR}".to_owned(),
                    ),
                ]
                .into_iter()
                .collect(),
                sse: false,
            },
            disabled: false,
        });
        settings.upsert_mcp_server(McpServerDef {
            name: "disabled-srv".to_owned(),
            transport: McpTransport::Stdio {
                command: "npx".to_owned(),
                args: vec!["-y".to_owned(), "disabled-mcp".to_owned()],
                env: [(
                    "API_KEY".to_owned(),
                    "disabled_inline_secret_xyz9876".to_owned(),
                )]
                .into_iter()
                .collect(),
            },
            disabled: true,
        });
        db.save_project_settings(project.id, &settings).unwrap();

        // 3) Preferences with Claude & Codex spawn credential env vars + OpenRouter key
        let mut prefs = AppPreferences::empty();
        prefs
            .claude_spawn_settings
            .set_env_var("ANTHROPIC_API_KEY", "sk-ant-legacy-spawn-key-9999")
            .unwrap();
        prefs
            .codex_spawn_settings
            .set_env_var("OPENAI_API_KEY", "sk-openai-legacy-spawn-8888")
            .unwrap();
        prefs.openrouter_keys.push(OpenRouterKey {
            id: "or-1".to_owned(),
            name: "Team".to_owned(),
            key_prefix: "sk-or-v1-".to_owned(),
            key_last4: "1234".to_owned(),
            secret_id: None,
            is_default: true,
            monthly_cap_cents: Some(5000),
            monthly_used_cents: 0,
            cap_note: "No cap".to_owned(),
            default_model: "openrouter/auto".to_owned(),
            used_by_projects: Vec::new(),
            test_state: crate::ConnectionTestState::Untested,
        });
        db.save_preferences(&prefs).unwrap();

        // 4) Chat session containing a leaked secret in JSON
        let mut chat = ChatSession::new(
            "chat-1".to_owned(),
            project.id,
            "api".to_owned(),
            ProviderKind::Claude,
            None,
            None,
            ProjectSandboxMode::WorkspaceWrite,
            100,
        );
        chat.workspace = ChatWorkspace::Checkout;
        chat.entries.push(ChatEntry::Assistant {
            text: "Leaked sk_live_legacy_stripe_123456 here".to_owned(),
            streaming: false,
        });
        db.save_chat_session(&chat).unwrap();

        // Demo mode MUST skip migration completely (zero data loss, zero Keychain writes).
        let demo_report = migrate_plaintext_secrets(&db, &store, true, true).unwrap();
        assert!(demo_report.skipped);
        assert!(store.is_empty());

        // Real migration (allowing MemoryStore in test).
        let report = migrate_plaintext_secrets(&db, &store, false, true).unwrap();
        assert!(!report.skipped);
        assert!(report.disk_cleanup_completed);
        assert!(report.migrated_names.len() >= 6);
        assert_eq!(report.openrouter_needs_value, vec!["Team".to_owned()]);
        assert_eq!(
            report.agent_and_steps_for_review,
            vec!["STRIPE_SECRET_KEY (api)".to_owned()]
        );
        assert!(report.scrubbed_records >= 1);
        assert!(report.has_user_notice());

        // Second run is a clean no-op (idempotent marker).
        let second = migrate_plaintext_secrets(&db, &store, false, true).unwrap();
        assert!(second.skipped);

        // Verify project settings updated cleanly.
        let reloaded_ps = db.load_project_settings(project.id).unwrap().unwrap();
        assert!(reloaded_ps.github_pat_id.is_some());
        assert_eq!(reloaded_ps.env_vars.len(), 1);
        assert_eq!(reloaded_ps.env_vars[0].key, "PUBLIC_API_URL");
        let linear = reloaded_ps
            .mcp_servers
            .iter()
            .find(|s| s.name == "linear")
            .unwrap();
        if let McpTransport::Http { headers, .. } = &linear.transport {
            assert!(headers["Authorization"].starts_with("Bearer ${"));
            assert_eq!(headers["X-Keep-Ref"], "Bearer ${EXISTING_REF_VAR}");
        } else {
            panic!("expected http transport");
        }
        let disabled = reloaded_ps
            .mcp_servers
            .iter()
            .find(|s| s.name == "disabled-srv")
            .unwrap();
        if let McpTransport::Stdio { env, .. } = &disabled.transport {
            assert!(env["API_KEY"].starts_with("${"));
        } else {
            panic!("expected stdio transport");
        }

        // Verify spawn settings moved secrets out of plaintext env_vars.
        let reloaded_prefs = db.load_preferences().unwrap().unwrap();
        assert!(
            !reloaded_prefs
                .claude_spawn_settings
                .env_vars
                .contains_key("ANTHROPIC_API_KEY")
        );
        assert!(
            reloaded_prefs
                .claude_spawn_settings
                .secret_env_vars
                .contains_key("ANTHROPIC_API_KEY")
        );
        assert!(
            !reloaded_prefs
                .codex_spawn_settings
                .env_vars
                .contains_key("OPENAI_API_KEY")
        );
        assert!(
            reloaded_prefs
                .codex_spawn_settings
                .secret_env_vars
                .contains_key("OPENAI_API_KEY")
        );
        assert!(reloaded_prefs.openrouter_keys[0].secret_id.is_some());

        // Verify Step 6 JSON scrubbing kept chat JSON valid & redacted the leaked value.
        let chats = db.list_chat_sessions().unwrap();
        assert_eq!(chats.len(), 1);
        if let ChatEntry::Assistant { text, .. } = &chats[0].entries[0] {
            assert!(!text.contains("sk_live_legacy_stripe_123456"));
            assert!(text.contains("[secret:"));
        } else {
            panic!("expected assistant entry");
        }

        // Verify legacy project_secrets table was dropped.
        assert!(
            db.load_project_secret(project.id, "github_pat")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn failed_keychain_replace_preserves_working_ready_secret() {
        let temp = TempDir::new().unwrap();
        let db = ProjectDatabase::open(temp.path().join("bruk.db")).unwrap();
        let store = MemoryStore::new();

        let rec = SecretRecord::new_keychain(
            "API_TOKEN",
            SecretKind::Env {
                scope: EnvScope::App {
                    shared_with: Sharing::AllProjects,
                },
                exposure: Exposure::McpOnly,
            },
            100,
        );
        let original = SecretString::from("sk_original_working_value_12345".to_owned());
        let saved = db
            .create_keychain_secret(&store, rec, &original, None)
            .unwrap();
        assert_eq!(saved.state, SecretState::Ready);

        // Simulate Keychain write failure during replace.
        store.inject_next_set_error(SecretError::Locked);
        let replacement = SecretString::from("sk_replacement_failed_value_67890".to_owned());
        assert!(
            db.replace_keychain_secret(&store, &saved.id, &replacement, None)
                .is_err()
        );

        // Secret MUST remain Ready with the original working value intact!
        let after = db.get_secret(&saved.id).unwrap().unwrap();
        assert_eq!(after.state, SecretState::Ready);
        let db_id = db.ensure_db_instance_id().unwrap();
        let account_key = crate::secrets::keychain_account_key(&db_id, &saved.id);
        let stored = store.get(&account_key).unwrap().unwrap();
        assert_eq!(stored.expose_secret(), "sk_original_working_value_12345");
    }

    #[test]
    fn delete_project_writes_tombstones_strips_sharing_and_copy_database_resets_instance() {
        let temp = TempDir::new().unwrap();
        let src_path = temp.path().join("src.db");
        let copy_path = temp.path().join("copy.db");
        let db = ProjectDatabase::open(&src_path).unwrap();
        let store = MemoryStore::new();

        let p1 = db.register_project("p1", "/tmp/p1", "main", "").unwrap();
        let p2 = db.register_project("p2", "/tmp/p2", "main", "").unwrap();

        let proj_sec = SecretRecord::new_keychain(
            "P1_SECRET",
            SecretKind::Env {
                scope: EnvScope::Project(p1.id),
                exposure: Exposure::McpOnly,
            },
            100,
        );
        let saved_p1 = db
            .create_keychain_secret(
                &store,
                proj_sec,
                &SecretString::from("p1_secret_value_12345678".to_owned()),
                Some("p1"),
            )
            .unwrap();

        let mut shared_set = BTreeSet::new();
        shared_set.insert(p1.id);
        shared_set.insert(p2.id);
        let app_sec = SecretRecord::new_keychain(
            "SHARED_SECRET",
            SecretKind::Env {
                scope: EnvScope::App {
                    shared_with: Sharing::Projects(shared_set),
                },
                exposure: Exposure::McpOnly,
            },
            100,
        );
        let saved_app = db
            .create_keychain_secret(
                &store,
                app_sec,
                &SecretString::from("shared_secret_value_12345678".to_owned()),
                None,
            )
            .unwrap();

        // Copy database before deleting p1: copy must reset db_instance_id and mark Keychain rows needs_value.
        ProjectDatabase::copy_database(&src_path, &copy_path).unwrap();
        let copied_db = ProjectDatabase::open(&copy_path).unwrap();
        assert_ne!(
            db.ensure_db_instance_id().unwrap(),
            copied_db.ensure_db_instance_id().unwrap()
        );
        let copied_sec = copied_db.get_secret(&saved_app.id).unwrap().unwrap();
        assert_eq!(copied_sec.state, SecretState::NeedsValue);

        // Delete p1 in src db: tombstone written, p1 secret gone, p1 stripped from Sharing::Projects.
        db.delete_project(p1.id).unwrap();
        assert!(db.get_secret(&saved_p1.id).unwrap().is_none());
        assert_eq!(db.list_tombstones().unwrap().len(), 1);
        let updated_app = db.get_secret(&saved_app.id).unwrap().unwrap();
        match updated_app.kind {
            SecretKind::Env {
                scope:
                    EnvScope::App {
                        shared_with: Sharing::Projects(set),
                    },
                ..
            } => {
                assert!(!set.contains(&p1.id));
                assert!(set.contains(&p2.id));
            }
            other => panic!("unexpected kind: {other:?}"),
        }

        // Sweep tombstones deletes the Keychain entry.
        let swept = db.sweep_secret_tombstones(&store, true).unwrap();
        assert_eq!(swept, 1);
        assert!(db.list_tombstones().unwrap().is_empty());
    }

    #[test]
    fn finds_and_deletes_legacy_database_and_wal_shm_sidecars() {
        let temp = TempDir::new().unwrap();
        let legacy = temp.path().join("factory.db");
        let wal = temp.path().join("factory.db-wal");
        let shm = temp.path().join("factory.db-shm");
        fs::write(&legacy, b"db").unwrap();
        fs::write(&wal, b"wal").unwrap();
        fs::write(&shm, b"shm").unwrap();

        let found = find_legacy_database_files(std::slice::from_ref(&legacy));
        assert_eq!(found.len(), 3);
        let removed = delete_legacy_database_files(&found).unwrap();
        assert_eq!(removed, 3);
        assert!(!legacy.exists());
        assert!(!wal.exists());
        assert!(!shm.exists());
    }
}
