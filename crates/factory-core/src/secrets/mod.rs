//! Local secret storage, scoping, resolution, external secret managers, redaction, and migration.
//!
//! Names (e.g. `${LINEAR_TOKEN}`) are the repository contract; bindings to OS keychains or
//! external managers live strictly in local user state and never enter git.

pub mod managers;
pub mod migration;
pub mod redactor;
pub mod resolver;
pub mod store;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::ProjectId;

pub use managers::{
    BUILT_IN_COMMAND_MANAGER_PRESETS, CancellationToken, CommandManagerPreset,
    CommandSecretManager, ManagerRegistry, ManagerStatus, OnePasswordManager, SecretManager,
    validate_op_reference,
};
pub use migration::{
    LEGACY_DATABASE_SIDE_EXTENSIONS, MigrationReport, PathChangeResolution,
    delete_legacy_database_files, find_legacy_database_files, migrate_plaintext_secrets,
};
pub use redactor::{SecretRedactor, StreamingRedactor};
pub use resolver::{
    ChatSecretsCache, ResolvedSecrets, ResolvedValue, RunSecretsCache, SecretResolver,
    SecretsCache, SecretsMetadataSnapshot, ValueOrigin, WorkflowSecretRequirements,
    collect_workflow_secret_requirements,
};
#[cfg(target_os = "macos")]
pub use store::MacOsKeychainStore;
pub use store::{
    BRUK_KEYCHAIN_SERVICE, MemoryStore, SecretError, SecretStore, StoreStatus, default_secret_store,
};

pub use factory_types::{
    EnvScope, Exposure, InternalPurpose, ManagerId, ONE_PASSWORD_MANAGER_ID, SecretFailure,
    SecretFix, SecretId, SecretKind, SecretProblem, SecretSource, SecretState, Sharing,
};

/// Generates a new random UUID v4 [`SecretId`].
pub fn new_secret_id() -> SecretId {
    Uuid::new_v4().to_string()
}

/// Generates a new random UUID v4 [`ManagerId`].
pub fn new_manager_id() -> ManagerId {
    Uuid::new_v4().to_string()
}

/// Generates a new random UUID v4 database instance identifier for Keychain namespacing.
pub fn new_db_instance_id() -> String {
    Uuid::new_v4().to_string()
}

/// Formats the Keychain account key `<db_instance_id>:<secret_id>`.
pub fn keychain_account_key(db_instance_id: &str, secret_id: &str) -> String {
    format!("{db_instance_id}:{secret_id}")
}

/// Formats the human-readable Keychain `kSecAttrLabel` for a secret record.
pub fn keychain_item_label(record: &SecretRecord, project_name: Option<&str>) -> String {
    match &record.kind {
        SecretKind::Env {
            scope: EnvScope::App { .. },
            ..
        } => format!("Bruk: {} (App)", record.name),
        SecretKind::Env {
            scope: EnvScope::Project(_),
            ..
        } => match project_name.filter(|s| !s.trim().is_empty()) {
            Some(proj) => format!("Bruk: {} ({proj})", record.name),
            None => format!("Bruk: {}", record.name),
        },
        SecretKind::Internal { purpose } => match purpose {
            InternalPurpose::GitHubPat => format!("Bruk: GitHub PAT — {}", record.name),
            InternalPurpose::OpenRouterKey => format!("Bruk: OpenRouter — {}", record.name),
            InternalPurpose::ManagerAuth { .. } => format!("Bruk: Manager Auth — {}", record.name),
            InternalPurpose::ProviderSpawnEnv => format!("Bruk: Provider Env — {}", record.name),
        },
    }
}

/// Persisted metadata for one secret stored in `bruk.db` (never contains secret values).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRecord {
    pub id: SecretId,
    /// Validated env name for `Env` secrets; human display label for `Internal` secrets.
    pub name: String,
    pub kind: SecretKind,
    pub source: SecretSource,
    pub state: SecretState,
    #[serde(default)]
    pub description: String,
    /// `"…<last4>"` ONLY for Keychain values with char len >= 12; `None` otherwise.
    #[serde(default)]
    pub preview: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub last_used_at: Option<i64>,
}

impl SecretRecord {
    /// Constructs a new Keychain-backed `SecretRecord` with a fresh UUID.
    pub fn new_keychain(name: impl Into<String>, kind: SecretKind, now: i64) -> Self {
        Self {
            id: new_secret_id(),
            name: name.into(),
            kind,
            source: SecretSource::Keychain,
            state: SecretState::Ready,
            description: String::new(),
            preview: None,
            created_at: now,
            updated_at: now,
            last_used_at: None,
        }
    }

    /// Constructs a new external-manager-backed `SecretRecord` with a fresh UUID.
    pub fn new_manager_ref(
        name: impl Into<String>,
        kind: SecretKind,
        manager: impl Into<ManagerId>,
        reference: impl Into<String>,
        now: i64,
    ) -> Self {
        Self {
            id: new_secret_id(),
            name: name.into(),
            kind,
            source: SecretSource::Manager {
                manager: manager.into(),
                reference: reference.into(),
            },
            state: SecretState::Ready,
            description: String::new(),
            preview: None,
            created_at: now,
            updated_at: now,
            last_used_at: None,
        }
    }

    /// Returns the masked display preview (`"…a1b2"` when available, else `"••••••••"`).
    pub fn display_preview(&self) -> String {
        match &self.source {
            SecretSource::Keychain => match self.state {
                SecretState::NeedsValue => "Needs value".to_owned(),
                SecretState::Ready => self
                    .preview
                    .clone()
                    .unwrap_or_else(|| "••••••••".to_owned()),
            },
            SecretSource::Manager { reference, .. } => reference.clone(),
        }
    }

    /// Returns the `ProjectId` if this is a Project-scoped Env secret, else `None`.
    pub fn project_id(&self) -> Option<ProjectId> {
        match &self.kind {
            SecretKind::Env {
                scope: EnvScope::Project(pid),
                ..
            } => Some(*pid),
            _ => None,
        }
    }

    /// Whether this is an App-scoped Env secret.
    pub fn is_app_env(&self) -> bool {
        matches!(
            self.kind,
            SecretKind::Env {
                scope: EnvScope::App { .. },
                ..
            }
        )
    }

    /// Whether this is a sealed Internal credential.
    pub fn is_internal(&self) -> bool {
        matches!(self.kind, SecretKind::Internal { .. })
    }

    /// Validates all model invariants for this record.
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("secret id cannot be empty".to_owned());
        }
        match &self.kind {
            SecretKind::Env { .. } => validate_env_name(&self.name)?,
            SecretKind::Internal { purpose } => {
                if self.name.trim().is_empty() {
                    return Err("secret name cannot be empty".to_owned());
                }
                // No circular manager credentials: ManagerAuth MUST be Keychain-backed.
                if matches!(purpose, InternalPurpose::ManagerAuth { .. })
                    && !matches!(self.source, SecretSource::Keychain)
                {
                    return Err(
                        "manager authentication secrets must be stored in the OS Keychain"
                            .to_owned(),
                    );
                }
            }
        }
        if let SecretSource::Manager { manager, reference } = &self.source {
            if manager.trim().is_empty() {
                return Err("secret manager id cannot be empty".to_owned());
            }
            if reference.trim().is_empty() {
                return Err("secret manager reference cannot be empty".to_owned());
            }
            if manager == ONE_PASSWORD_MANAGER_ID {
                validate_op_reference(reference)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Secret manager configuration (persisted in AppPreferences only — never in repository files)
// ---------------------------------------------------------------------------------------------

/// Authentication mode for the built-in 1Password (`op`) manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnePasswordAuthMode {
    /// Recommended for agents & unattended runs: uses `OP_SERVICE_ACCOUNT_TOKEN` from Keychain.
    #[default]
    ServiceAccount,
    /// Interactive Touch ID / desktop app integration.
    DesktopApp,
}

impl OnePasswordAuthMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::ServiceAccount => "Service account",
            Self::DesktopApp => "Desktop app",
        }
    }
}

/// Configuration for the built-in 1Password CLI (`op`) secret manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnePasswordConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Path to `op` CLI binary (empty uses `op` on `PATH`).
    #[serde(default = "default_op_binary")]
    pub binary_path: String,
    #[serde(default)]
    pub auth_mode: OnePasswordAuthMode,
    /// Account shorthand used only in DesktopApp mode (NEVER passed in ServiceAccount mode).
    #[serde(default)]
    pub desktop_account: String,
    /// Reference to the Keychain-backed `Internal { purpose: ManagerAuth }` service-account token.
    #[serde(default)]
    pub service_account_secret_id: Option<SecretId>,
}

fn default_op_binary() -> String {
    "op".to_owned()
}

impl Default for OnePasswordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            binary_path: default_op_binary(),
            auth_mode: OnePasswordAuthMode::ServiceAccount,
            desktop_account: String::new(),
            service_account_secret_id: None,
        }
    }
}

/// Configuration for a user-defined generic CLI secret manager (`bws`, `vault`, `doppler`, `pass`, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandManagerConfig {
    pub id: ManagerId,
    pub name: String,
    /// Executable path or name on `PATH` (invoked directly via `Command::new`, never via shell).
    pub program: String,
    /// Arguments template containing `{ref}` placeholder.
    pub args: Vec<String>,
    /// Execution timeout in seconds (default 30).
    #[serde(default = "default_command_manager_timeout")]
    pub timeout_secs: u64,
    /// Optional Keychain-backed `Internal { purpose: ManagerAuth }` env vars (e.g. `BWS_ACCESS_TOKEN`).
    #[serde(default)]
    pub auth_env: BTreeMap<String, SecretId>,
    /// Optional substring in stderr that maps non-zero exit to `ReferenceNotFound`.
    #[serde(default)]
    pub not_found_pattern: Option<String>,
    #[serde(default = "default_true_bool")]
    pub enabled: bool,
}

fn default_command_manager_timeout() -> u64 {
    30
}

fn default_true_bool() -> bool {
    true
}

impl CommandManagerConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("manager id cannot be empty".to_owned());
        }
        if self.id == ONE_PASSWORD_MANAGER_ID {
            return Err("manager id '1password' is reserved".to_owned());
        }
        if self.name.trim().is_empty() {
            return Err("manager name cannot be empty".to_owned());
        }
        if self.program.trim().is_empty() {
            return Err("program path cannot be empty".to_owned());
        }
        if !self.args.iter().any(|arg| arg.contains("{ref}")) {
            return Err("arguments must include a '{ref}' placeholder".to_owned());
        }
        if self.timeout_secs == 0 || self.timeout_secs > 300 {
            return Err("timeout must be between 1 and 300 seconds".to_owned());
        }
        for env_name in self.auth_env.keys() {
            validate_env_name(env_name)?;
        }
        Ok(())
    }

    /// True if `{ref}` appears after a standalone `--` argument, allowing refs starting with `-`.
    pub fn ref_is_after_double_dash(&self) -> bool {
        let mut saw_double_dash = false;
        for arg in &self.args {
            if arg == "--" {
                saw_double_dash = true;
            } else if arg.contains("{ref}") {
                return saw_double_dash;
            }
        }
        false
    }
}

/// Application-wide secrets and secret-manager settings stored in `AppPreferences`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SecretsSettings {
    #[serde(default)]
    pub one_password: OnePasswordConfig,
    #[serde(default)]
    pub command_managers: Vec<CommandManagerConfig>,
}

impl SecretsSettings {
    pub fn manager_display_name(&self, manager_id: &str) -> String {
        if manager_id == ONE_PASSWORD_MANAGER_ID {
            return "1Password".to_owned();
        }
        self.command_managers
            .iter()
            .find(|m| m.id == manager_id)
            .map(|m| m.name.clone())
            .unwrap_or_else(|| manager_id.to_owned())
    }
}

// ---------------------------------------------------------------------------------------------
// Validation & preview helpers
// ---------------------------------------------------------------------------------------------

/// Validates an environment variable name against `^[A-Za-z_][A-Za-z0-9_]*$`.
pub fn validate_env_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("secret name cannot be empty".to_owned());
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("secret name cannot be empty".to_owned());
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(format!(
            "secret name '{name}' must start with a letter or '_'"
        ));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "secret name '{name}' can only contain letters, digits, and '_'"
        ));
    }
    Ok(())
}

/// Validates that a Keychain secret value is non-empty and not whitespace-only.
/// Multi-line values with internal newlines are allowed.
pub fn validate_secret_value(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("secret value cannot be empty or whitespace-only".to_owned());
    }
    Ok(())
}

/// Returns `Some("…<last4>")` ONLY when `value.chars().count() >= 12`; otherwise `None`.
pub fn compute_preview(value: &str) -> Option<String> {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < 12 {
        return None;
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    Some(format!("…{tail}"))
}

/// Heuristic indicating whether an env var or HTTP header key likely carries a credential.
/// Excludes known non-secret numeric/config keys like `MAX_*_TOKENS`, `*_TIMEOUT*`, `CACHE_KEY`, `SORT_KEY`.
pub fn looks_like_secret_key(key: &str) -> bool {
    let upper = key.trim().to_ascii_uppercase();
    if upper.is_empty() {
        return false;
    }
    // Explicit non-secret exclusions from spec §11.
    if upper.contains("TIMEOUT")
        || upper == "CACHE_KEY"
        || upper == "SORT_KEY"
        || upper.ends_with("_CACHE_KEY")
        || upper.ends_with("_SORT_KEY")
        || upper.contains("MAX_TOKENS")
        || (upper.starts_with("MAX_") && upper.ends_with("_TOKENS"))
        || upper.ends_with("_TOKEN_LIMIT")
        || upper.ends_with("_TOKEN_COUNT")
        || upper == "KEY_FORMAT"
        || upper == "PUBLIC_KEY_ID"
    {
        return false;
    }
    if upper == "AUTHORIZATION"
        || upper == "X-API-KEY"
        || upper == "X_API_KEY"
        || upper == "ANTHROPIC_CUSTOM_HEADERS"
    {
        return true;
    }
    const PATTERNS: &[&str] = &[
        "TOKEN",
        "SECRET",
        "API_KEY",
        "APIKEY",
        "ACCESS_KEY",
        "PRIVATE_KEY",
        "PASSWORD",
        "PASSWD",
        "CREDENTIAL",
        "AUTH",
        "BEARER",
        "PAT",
    ];
    PATTERNS.iter().any(|pat| {
        if *pat == "PAT" {
            upper == "PAT" || upper.ends_with("_PAT") || upper.starts_with("PAT_")
        } else {
            upper.contains(pat)
        }
    })
}

/// Whether a string contains a `${...}` reference.
pub fn contains_env_reference(value: &str) -> bool {
    !crate::mcp::env_references(value).is_empty()
}

/// Whether `name` looks like an ambient host credential variable that Codex's default
/// shell_environment_policy excludes (`KEY`, `SECRET`, or `TOKEN`, case-insensitive).
pub fn is_ambient_credential_var_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.contains("KEY") || upper.contains("SECRET") || upper.contains("TOKEN")
}

/// Normalizes an arbitrary string into an uppercase env var segment (`[A-Z0-9_]`).
pub fn sanitize_env_segment(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_underscore = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
            prev_underscore = false;
        } else if !out.is_empty() && !prev_underscore {
            out.push('_');
            prev_underscore = true;
        }
    }
    let trimmed = out.trim_matches('_').to_owned();
    if trimmed.is_empty() {
        "SECRET".to_owned()
    } else if trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("S_{trimmed}")
    } else {
        trimmed
    }
}

/// Detects whether pasted secret text has leading/trailing whitespace or trailing newlines.
pub fn has_surrounding_whitespace(value: &str) -> bool {
    !value.is_empty() && value.trim() != value
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn validates_env_names_and_values() {
        assert!(validate_env_name("LINEAR_TOKEN").is_ok());
        assert!(validate_env_name("_PRIVATE_1").is_ok());
        assert!(validate_env_name("").is_err());
        assert!(validate_env_name("1INVALID").is_err());
        assert!(validate_env_name("BAD-NAME").is_err());
        assert!(validate_env_name("BAD NAME").is_err());

        assert!(validate_secret_value("abc").is_ok());
        assert!(validate_secret_value("line1\nline2").is_ok());
        assert!(validate_secret_value("").is_err());
        assert!(validate_secret_value("   \n\t ").is_err());
    }

    #[test]
    fn short_secret_preview_returns_none_below_12_chars() {
        assert_eq!(compute_preview("12345678901"), None);
        assert_eq!(compute_preview("123456789012"), Some("…9012".to_owned()));
        assert_eq!(
            compute_preview("ghp_1234567890abcd"),
            Some("…abcd".to_owned())
        );
    }

    #[test]
    fn manager_auth_rejects_manager_source_to_prevent_cycles() {
        let record = SecretRecord {
            id: new_secret_id(),
            name: "1Password SA".to_owned(),
            kind: SecretKind::Internal {
                purpose: InternalPurpose::ManagerAuth {
                    manager: ONE_PASSWORD_MANAGER_ID.to_owned(),
                },
            },
            source: SecretSource::Manager {
                manager: ONE_PASSWORD_MANAGER_ID.to_owned(),
                reference: "op://Eng/SA/token".to_owned(),
            },
            state: SecretState::Ready,
            description: String::new(),
            preview: None,
            created_at: 0,
            updated_at: 0,
            last_used_at: None,
        };
        assert!(record.validate().is_err());
    }

    #[test]
    fn secret_key_heuristic_excludes_known_non_secrets() {
        assert!(looks_like_secret_key("LINEAR_TOKEN"));
        assert!(looks_like_secret_key("Authorization"));
        assert!(looks_like_secret_key("OPENAI_API_KEY"));
        assert!(looks_like_secret_key("ANTHROPIC_CUSTOM_HEADERS"));
        assert!(looks_like_secret_key("GITHUB_PAT"));

        assert!(!looks_like_secret_key("MAX_OUTPUT_TOKENS"));
        assert!(!looks_like_secret_key("MAX_TOKENS"));
        assert!(!looks_like_secret_key("REQUEST_TIMEOUT_MS"));
        assert!(!looks_like_secret_key("CACHE_KEY"));
        assert!(!looks_like_secret_key("SORT_KEY"));
        assert!(!looks_like_secret_key("NODE_ENV"));
    }

    #[test]
    fn serde_roundtrips_all_secret_enums_and_variants() {
        let scopes = [
            EnvScope::App {
                shared_with: Sharing::AllProjects,
            },
            EnvScope::App {
                shared_with: Sharing::NoProjects,
            },
            EnvScope::App {
                shared_with: Sharing::Projects([1, 2].into_iter().collect()),
            },
            EnvScope::Project(42),
        ];
        for scope in scopes {
            let json = serde_json::to_string(&scope).unwrap();
            let back: EnvScope = serde_json::from_str(&json).unwrap();
            assert_eq!(back, scope);
        }

        let fixes = [
            SecretFix::AddSecret {
                name: "FOO".to_owned(),
                project_id: Some(7),
            },
            SecretFix::EditSecret("sec-1".to_owned()),
            SecretFix::UnlockManager("1password".to_owned()),
            SecretFix::OpenManagerSettings("bws".to_owned()),
            SecretFix::OpenSecretsSettings,
            SecretFix::ResumeQueuedWork { project_id: 3 },
            SecretFix::DeleteLegacyDatabases {
                paths: vec![PathBuf::from("/tmp/factory.db")],
            },
            SecretFix::None,
        ];
        for fix in fixes {
            let json = serde_json::to_string(&fix).unwrap();
            let back: SecretFix = serde_json::from_str(&json).unwrap();
            assert_eq!(back, fix);
        }
    }
}
