//! Complete catalog, persistence model, validation, and command builder for custom
//! OpenAI Codex CLI spawn settings.
//!
//! Every user-configurable setting discovered from `codex --help` / `codex exec --help`
//! (CLI launch flags), the official Codex `config.toml` JSON schema (passed via repeatable
//! `-c <key=value>` TOML overrides), and Codex environment variables is modeled here as an
//! optional override over the user's existing `~/.codex/config.toml` defaults. Unset options
//! are omitted so Bruk never clobbers the user's Codex configuration unless explicitly
//! overridden in Preferences.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::claude_settings::{CliFlagEncoding, split_shell_words as split_claude_shell_words};

/// Config key always forced to `false` on every spawned `codex` process so a run cannot
/// check for updates or prompt/drift the CLI version out from under subsequent runs.
pub const CODEX_DISABLE_UPDATE_CHECK_KEY: &str = "check_for_update_on_startup";

/// Value always assigned to [`CODEX_DISABLE_UPDATE_CHECK_KEY`] when spawning `codex`.
pub const CODEX_DISABLE_UPDATE_CHECK_VALUE: &str = "false";

/// Full `-c` argument pair value (`check_for_update_on_startup=false`) always enforced by Bruk.
pub const CODEX_DISABLE_UPDATE_CHECK_ARG: &str = "check_for_update_on_startup=false";

/// Applies mandatory Bruk runtime config guards to any `codex` [`Command`].
///
/// Always appends `-c check_for_update_on_startup=false` so startup update checks and prompts
/// cannot interfere with automated runs or nudge the installed Codex CLI version mid-flight.
pub fn apply_mandatory_codex_spawn_args(cmd: &mut Command) {
    cmd.arg("-c").arg(CODEX_DISABLE_UPDATE_CHECK_ARG);
}

/// Creates a new [`Command`] for spawning the `codex` binary with the given subcommand args
/// followed by mandatory Bruk spawn guards (`-c check_for_update_on_startup=false`).
pub fn new_codex_subcommand(binary_path: impl AsRef<Path>, subcommand_args: &[&str]) -> Command {
    let mut cmd = Command::new(binary_path.as_ref());
    cmd.args(subcommand_args);
    apply_mandatory_codex_spawn_args(&mut cmd);
    cmd
}

/// Logical grouping for Codex spawn settings displayed in the Preferences UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexSettingCategory {
    CliFlags,
    ModelsAndReasoning,
    ApprovalsAndPermissions,
    SandboxAndShell,
    ContextAndMemory,
    AgentsAndTools,
    McpPluginsAndHooks,
    AuthAndTelemetry,
    Features,
    InterfaceAndTui,
    EnvironmentAndRaw,
}

impl CodexSettingCategory {
    pub const ALL: &'static [Self] = &[
        Self::CliFlags,
        Self::ModelsAndReasoning,
        Self::ApprovalsAndPermissions,
        Self::SandboxAndShell,
        Self::ContextAndMemory,
        Self::AgentsAndTools,
        Self::McpPluginsAndHooks,
        Self::AuthAndTelemetry,
        Self::Features,
        Self::InterfaceAndTui,
        Self::EnvironmentAndRaw,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::CliFlags => "Launch & CLI",
            Self::ModelsAndReasoning => "Models & Reasoning",
            Self::ApprovalsAndPermissions => "Approvals & Permissions",
            Self::SandboxAndShell => "Sandbox & Shell",
            Self::ContextAndMemory => "Context & Memory",
            Self::AgentsAndTools => "Agents, Tools & Web",
            Self::McpPluginsAndHooks => "MCP, Plugins & Hooks",
            Self::AuthAndTelemetry => "Auth, Storage & Telemetry",
            Self::Features => "Feature Flags",
            Self::InterfaceAndTui => "Interface & TUI",
            Self::EnvironmentAndRaw => "Environment & Raw",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::CliFlags => "cli-flags",
            Self::ModelsAndReasoning => "models",
            Self::ApprovalsAndPermissions => "approvals",
            Self::SandboxAndShell => "sandbox",
            Self::ContextAndMemory => "context-memory",
            Self::AgentsAndTools => "agents-tools",
            Self::McpPluginsAndHooks => "mcp-plugins-hooks",
            Self::AuthAndTelemetry => "auth-telemetry",
            Self::Features => "features",
            Self::InterfaceAndTui => "interface-tui",
            Self::EnvironmentAndRaw => "env-raw",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::CliFlags => {
                "Command-line flags passed directly to the codex binary on every spawn."
            }
            Self::ModelsAndReasoning => {
                "Model provider, reasoning effort, summaries, verbosity, compaction, and service tier."
            }
            Self::ApprovalsAndPermissions => {
                "Approval policy, auto-review guardian tuning, permissions profiles, and app tool controls."
            }
            Self::SandboxAndShell => {
                "Sandbox mode, workspace-write network/roots, shell env policy, and browser/computer access."
            }
            Self::ContextAndMemory => {
                "System instructions, AGENTS.md discovery, history persistence, and memory pipelines."
            }
            Self::AgentsAndTools => {
                "Multi-agent limits, subagent defaults, web search, skills, profiles, and code mode."
            }
            Self::McpPluginsAndHooks => {
                "MCP server definitions, OAuth settings, plugins, marketplaces, and lifecycle hooks."
            }
            Self::AuthAndTelemetry => {
                "Credential storage, login restrictions, SQLite/log directories, analytics, and OpenTelemetry."
            }
            Self::Features => {
                "Codex [features] capability toggles passed via -c features.<name>=true|false."
            }
            Self::InterfaceAndTui => {
                "File opener, notifications, TUI theme, animations, rich rendering, and product notices."
            }
            Self::EnvironmentAndRaw => {
                "Process environment variables, extra raw CLI flags, and custom -c config.toml overrides."
            }
        }
    }
}

/// Where a setting is applied when spawning Codex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSettingTarget {
    /// Passed as a CLI flag directly on the `codex` command line.
    CliFlag(CliFlagEncoding),
    /// Passed as `-c <path>=<toml_value>` overriding `~/.codex/config.toml`.
    ConfigToml { path: &'static str },
}

/// Typed schema and UI hint for one Codex setting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CodexSettingKind {
    Bool {
        default_hint: Option<bool>,
    },
    Choice {
        options: &'static [&'static str],
        allow_custom: bool,
        default_hint: Option<&'static str>,
    },
    String {
        placeholder: &'static str,
        allow_empty: bool,
        default_hint: Option<&'static str>,
    },
    Integer {
        min: Option<i64>,
        max: Option<i64>,
        placeholder: &'static str,
        default_hint: Option<i64>,
    },
    Number {
        min: Option<f64>,
        max: Option<f64>,
        placeholder: &'static str,
        default_hint: Option<&'static str>,
    },
    StringList {
        placeholder: &'static str,
    },
    /// Validated JSON object or array serialized to a TOML inline table or array for `-c`.
    Json {
        placeholder: &'static str,
        expect_array: bool,
    },
}

/// Static specification of one user-configurable Codex spawn setting.
#[derive(Debug, Clone, Copy)]
pub struct CodexSettingSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub display_key: &'static str,
    pub description: &'static str,
    pub category: CodexSettingCategory,
    pub target: CodexSettingTarget,
    pub kind: CodexSettingKind,
}

impl CodexSettingSpec {
    /// Human-readable hint describing the CLI default value when unset.
    pub fn default_hint_label(&self) -> Option<String> {
        match self.kind {
            CodexSettingKind::Bool {
                default_hint: Some(true),
            } => Some("CLI default: On".to_owned()),
            CodexSettingKind::Bool {
                default_hint: Some(false),
            } => Some("CLI default: Off".to_owned()),
            CodexSettingKind::Choice {
                default_hint: Some(val),
                ..
            } => Some(format!("CLI default: {val}")),
            CodexSettingKind::String {
                default_hint: Some(val),
                ..
            } => Some(format!("CLI default: {val}")),
            CodexSettingKind::Integer {
                default_hint: Some(val),
                ..
            } => Some(format!("CLI default: {val}")),
            CodexSettingKind::Number {
                default_hint: Some(val),
                ..
            } => Some(format!("CLI default: {val}")),
            _ => None,
        }
    }

    /// Returns whether this setting matches a case-insensitive search query.
    pub fn matches_query(&self, query: &str) -> bool {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return true;
        }
        let needle = trimmed.to_ascii_lowercase();
        self.label.to_ascii_lowercase().contains(&needle)
            || self.display_key.to_ascii_lowercase().contains(&needle)
            || self.id.to_ascii_lowercase().contains(&needle)
            || self.description.to_ascii_lowercase().contains(&needle)
            || self.category.label().to_ascii_lowercase().contains(&needle)
    }
}

/// Quick-pick preset for a well-known Codex CLI environment variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodexEnvPreset {
    pub name: &'static str,
    pub description: &'static str,
    pub placeholder: &'static str,
    pub suggested_value: &'static str,
}

impl CodexEnvPreset {
    /// Returns whether this environment variable preset matches a case-insensitive search query.
    pub fn matches_query(&self, query: &str) -> bool {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return true;
        }
        let needle = trimmed.to_ascii_lowercase();
        self.name.to_ascii_lowercase().contains(&needle)
            || self.description.to_ascii_lowercase().contains(&needle)
            || "environment".contains(&needle)
            || "env".contains(&needle)
    }
}

/// Typed value stored for an overridden Codex setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum CodexSettingValue {
    Bool(bool),
    String(String),
    Integer(i64),
    /// Validated finite decimal number stored as text to preserve `Eq` and exact formatting.
    Number(String),
    StringList(Vec<String>),
    /// Validated JSON object or array payload (serialized to TOML for `-c` overrides).
    Json(String),
}

impl CodexSettingValue {
    /// Concise single-line display summary for the Preferences UI.
    pub fn summary(&self) -> String {
        match self {
            Self::Bool(true) => "On".to_owned(),
            Self::Bool(false) => "Off".to_owned(),
            Self::String(s) if s.is_empty() => "(empty)".to_owned(),
            Self::String(s) => truncate_preview(s, 56),
            Self::Integer(n) => n.to_string(),
            Self::Number(n) => n.clone(),
            Self::StringList(items) if items.is_empty() => "(empty list)".to_owned(),
            Self::StringList(items) => truncate_preview(&items.join(", "), 56),
            Self::Json(raw) => truncate_preview(raw, 56),
        }
    }

    /// Editable text representation for inline text editors.
    pub fn to_edit_text(&self) -> String {
        match self {
            Self::Bool(b) => b.to_string(),
            Self::String(s) => s.clone(),
            Self::Integer(n) => n.to_string(),
            Self::Number(n) => n.clone(),
            Self::StringList(items) => items.join(", "),
            Self::Json(raw) => raw.clone(),
        }
    }
}

fn truncate_preview(raw: &str, max_chars: usize) -> String {
    let single_line = raw.replace('\n', " ");
    let chars: Vec<char> = single_line.chars().collect();
    if chars.len() <= max_chars {
        single_line
    } else {
        let head: String = chars[..max_chars.saturating_sub(1)].iter().collect();
        format!("{head}…")
    }
}

/// Validation and serialization errors for [`CodexSpawnSettings`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CodexSettingsError {
    #[error("unknown Codex setting '{0}'")]
    UnknownSetting(String),
    #[error("invalid value for '{key}': {reason}")]
    InvalidValue { key: String, reason: String },
    #[error("environment variable name cannot be empty")]
    EmptyEnvName,
    #[error(
        "environment variable '{0}' must start with a letter or '_' and contain only letters, digits, and '_'"
    )]
    InvalidEnvName(String),
    #[error(
        "'{0}' is always enforced by Bruk (set to false) so runs cannot check for or prompt Codex updates; updates must be user-directed"
    )]
    ReservedConfigKey(String),
    #[error("invalid extra CLI arguments: {0}")]
    InvalidExtraCliArgs(String),
    #[error("invalid extra -c config override: {0}")]
    InvalidExtraConfigOverrides(String),
}

/// Persisted custom settings applied whenever Bruk spawns the `codex` CLI.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CodexSpawnSettings {
    /// Overrides for CLI launch flags (keyed by [`CodexSettingSpec::id`]).
    #[serde(default)]
    pub cli_flags: BTreeMap<String, CodexSettingValue>,
    /// Overrides for `config.toml` keys passed via `-c <path>=<toml_value>`.
    #[serde(default)]
    pub config_overrides: BTreeMap<String, CodexSettingValue>,
    /// Environment variables applied to the spawned `codex` process.
    #[serde(default)]
    pub env_vars: BTreeMap<String, String>,
    /// Secret-backed environment variables mapped to `Internal { purpose: ProviderSpawnEnv }` ids.
    #[serde(default)]
    pub secret_env_vars: BTreeMap<String, crate::secrets::SecretId>,
    /// Extra raw CLI arguments appended verbatim (shell-word split) when spawning Codex.
    #[serde(default)]
    pub extra_cli_args: String,
    /// Extra raw `key = value` TOML config overrides (one per line) emitted via `-c key=value`.
    #[serde(default)]
    pub extra_config_overrides: String,
}

impl CodexSpawnSettings {
    /// Looks up the current override value for a catalog setting by `spec_id`.
    pub fn get(&self, spec_id: &str) -> Option<&CodexSettingValue> {
        self.cli_flags
            .get(spec_id)
            .or_else(|| self.config_overrides.get(spec_id))
    }

    /// Returns `true` when no custom flags, config overrides, env vars, or raw overrides are set.
    pub fn is_empty(&self) -> bool {
        self.override_count() == 0
    }

    /// Total number of active custom overrides across flags, config keys, env vars, and raw fields.
    pub fn override_count(&self) -> usize {
        self.cli_flags.len()
            + self.config_overrides.len()
            + self.env_vars.len()
            + self.secret_env_vars.len()
            + usize::from(!self.extra_cli_args.trim().is_empty())
            + usize::from(!self.extra_config_overrides.trim().is_empty())
    }

    /// Number of active custom overrides belonging to `category`.
    pub fn override_count_in_category(&self, category: CodexSettingCategory) -> usize {
        if category == CodexSettingCategory::EnvironmentAndRaw {
            return self.env_vars.len()
                + self.secret_env_vars.len()
                + usize::from(!self.extra_cli_args.trim().is_empty())
                + usize::from(!self.extra_config_overrides.trim().is_empty());
        }
        CODEX_SETTING_SPECS
            .iter()
            .filter(|spec| spec.category == category && self.get(spec.id).is_some())
            .count()
    }

    /// Clears all custom spawn overrides back to CLI defaults.
    pub fn reset_all(&mut self) {
        self.cli_flags.clear();
        self.config_overrides.clear();
        self.env_vars.clear();
        self.secret_env_vars.clear();
        self.extra_cli_args.clear();
        self.extra_config_overrides.clear();
    }

    /// Removes the override for `spec_id` so it inherits the CLI default.
    pub fn clear(&mut self, spec_id: &str) -> bool {
        self.cli_flags.remove(spec_id).is_some() || self.config_overrides.remove(spec_id).is_some()
    }

    /// Validates and stores a typed override value for `spec_id`.
    pub fn set_value(
        &mut self,
        spec_id: &str,
        value: CodexSettingValue,
    ) -> Result<(), CodexSettingsError> {
        let spec = find_codex_setting_spec(spec_id)
            .ok_or_else(|| CodexSettingsError::UnknownSetting(spec_id.to_owned()))?;
        validate_value_for_spec(spec, &value)?;
        match spec.target {
            CodexSettingTarget::CliFlag(_) => {
                self.cli_flags.insert(spec.id.to_owned(), value);
            }
            CodexSettingTarget::ConfigToml { .. } => {
                self.config_overrides.insert(spec.id.to_owned(), value);
            }
        }
        Ok(())
    }

    /// Parses, validates, and stores an override value from user text input.
    pub fn set_from_text(&mut self, spec_id: &str, raw: &str) -> Result<(), CodexSettingsError> {
        let spec = find_codex_setting_spec(spec_id)
            .ok_or_else(|| CodexSettingsError::UnknownSetting(spec_id.to_owned()))?;
        let parsed = parse_text_for_spec(spec, raw)?;
        self.set_value(spec_id, parsed)
    }

    /// Validates and sets a process environment variable to inject on spawn.
    pub fn set_env_var(&mut self, key: &str, value: &str) -> Result<(), CodexSettingsError> {
        let key = key.trim();
        validate_env_var_name(key)?;
        self.secret_env_vars.remove(key);
        self.env_vars.insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    /// Binds an environment variable to a Keychain/manager `Internal { purpose: ProviderSpawnEnv }` secret.
    pub fn set_secret_env_var(
        &mut self,
        key: &str,
        secret_id: crate::secrets::SecretId,
    ) -> Result<(), CodexSettingsError> {
        let key = key.trim();
        validate_env_var_name(key)?;
        self.env_vars.remove(key);
        self.secret_env_vars.insert(key.to_owned(), secret_id);
        Ok(())
    }

    /// Removes a configured environment variable override (plain or secret-backed).
    pub fn remove_env_var(&mut self, key: &str) -> bool {
        let k = key.trim();
        let a = self.env_vars.remove(k).is_some();
        let b = self.secret_env_vars.remove(k).is_some();
        a || b
    }

    /// Validates and stores extra raw CLI arguments (shell-word split at spawn time).
    pub fn set_extra_cli_args(&mut self, raw: &str) -> Result<(), CodexSettingsError> {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            split_shell_words(trimmed)?;
        }
        self.extra_cli_args = trimmed.to_owned();
        Ok(())
    }

    /// Validates and stores extra raw `-c` config overrides (one `key = value` per line).
    pub fn set_extra_config_overrides(&mut self, raw: &str) -> Result<(), CodexSettingsError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            self.extra_config_overrides.clear();
            return Ok(());
        }
        let parsed = parse_extra_config_overrides(trimmed)?;
        let normalized = parsed
            .into_iter()
            .map(|(k, v)| format!("{k} = {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        self.extra_config_overrides = normalized;
        Ok(())
    }

    /// Builds ordered `(key, toml_value)` pairs from user-configured `config_overrides` and
    /// `extra_config_overrides`. Excludes the mandatory `check_for_update_on_startup=false` guard
    /// (which [`build_codex_spawn_plan`] always enforces separately).
    pub fn build_user_config_overrides(&self) -> Result<Vec<(String, String)>, CodexSettingsError> {
        let mut pairs: BTreeMap<String, String> = BTreeMap::new();

        for spec in CODEX_SETTING_SPECS {
            let CodexSettingTarget::ConfigToml { path } = spec.target else {
                continue;
            };
            let Some(value) = self.config_overrides.get(spec.id) else {
                continue;
            };
            if path == CODEX_DISABLE_UPDATE_CHECK_KEY {
                continue;
            }
            let toml_val = setting_value_to_toml(spec, value)?;
            pairs.insert(path.to_owned(), toml_val);
        }

        let extra_raw = self.extra_config_overrides.trim();
        if !extra_raw.is_empty() {
            for (key, value) in parse_extra_config_overrides_lenient_guard(extra_raw)? {
                if key == CODEX_DISABLE_UPDATE_CHECK_KEY {
                    continue;
                }
                pairs.insert(key, value);
            }
        }

        Ok(pairs.into_iter().collect())
    }

    /// Builds the CLI flag arguments from `cli_flags` and `extra_cli_args`.
    pub fn build_cli_flag_args(&self) -> Result<Vec<String>, CodexSettingsError> {
        let mut args = Vec::new();

        for spec in CODEX_SETTING_SPECS {
            let CodexSettingTarget::CliFlag(encoding) = spec.target else {
                continue;
            };
            let Some(value) = self.cli_flags.get(spec.id) else {
                continue;
            };
            encode_cli_flag(&mut args, encoding, value);
        }

        let extra = self.extra_cli_args.trim();
        if !extra.is_empty() {
            args.extend(split_shell_words(extra)?);
        }

        Ok(args)
    }
}

/// Per-invocation options merged with persistent [`CodexSpawnSettings`] when spawning `codex`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodexSpawnOptions {
    /// Optional working directory for the spawned `codex` process.
    pub working_dir: Option<PathBuf>,
    /// Optional per-run model override (takes precedence over `default_model`).
    pub model_override: Option<String>,
    /// Run non-interactively via `codex exec`.
    pub exec_mode: bool,
    /// Emit JSONL event stream on stdout (`--json`, implies `exec_mode`).
    pub json_events: bool,
    /// Optional positional prompt argument passed last.
    pub prompt: Option<String>,
}

/// Fully resolved invocation plan ready to preview or convert into a [`std::process::Command`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexSpawnPlan {
    pub binary_path: PathBuf,
    pub working_dir: Option<PathBuf>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// Environment variable names explicitly stripped from the spawned child (`env_remove`).
    pub env_remove: BTreeSet<String>,
    /// Environment variable names whose values are masked as `[secret:NAME]` in command previews.
    pub masked_env_keys: BTreeSet<String>,
    /// User-configured `config.toml` overrides (emitted as `-c key=value`).
    pub config_overrides: Vec<(String, String)>,
}

impl CodexSpawnPlan {
    /// Builds a [`std::process::Command`] configured with the binary, working dir, args, and env.
    pub fn to_command(&self) -> Command {
        let mut cmd = Command::new(&self.binary_path);
        if let Some(dir) = &self.working_dir {
            cmd.current_dir(dir);
        }
        cmd.args(&self.args);
        for key in &self.env_remove {
            cmd.env_remove(key);
        }
        cmd.envs(&self.env);
        cmd
    }

    /// Renders a shell-safe command preview string for display in Preferences.
    pub fn preview_command_line(&self) -> String {
        let mut parts = Vec::new();
        for (k, v) in &self.env {
            if self.masked_env_keys.contains(k) {
                parts.push(format!("{k}=[secret:{k}]"));
            } else {
                parts.push(format!("{k}={}", shell_quote(v)));
            }
        }
        parts.push(shell_quote(&self.binary_path.display().to_string()));
        for arg in &self.args {
            parts.push(shell_quote(arg));
        }
        parts.join(" ")
    }

    /// Pretty-printed `config.toml` override lines passed via `-c`, if any user overrides are set.
    pub fn config_overrides_pretty(&self) -> Option<String> {
        if self.config_overrides.is_empty() {
            return None;
        }
        let lines = self
            .config_overrides
            .iter()
            .map(|(k, v)| format!("{k} = {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        Some(lines)
    }
}

/// Builds a complete [`CodexSpawnPlan`] combining the resolved binary, default model,
/// user-configured [`CodexSpawnSettings`], and per-invocation [`CodexSpawnOptions`].
pub fn build_codex_spawn_plan(
    binary_path: impl AsRef<Path>,
    default_model: Option<&str>,
    settings: &CodexSpawnSettings,
    options: &CodexSpawnOptions,
) -> Result<CodexSpawnPlan, CodexSettingsError> {
    let mut args = Vec::new();

    if options.exec_mode || options.json_events {
        args.push("exec".to_owned());
    }

    // Always disable startup update checks so runs cannot prompt or drift CLI versions.
    args.push("-c".to_owned());
    args.push(CODEX_DISABLE_UPDATE_CHECK_ARG.to_owned());

    if options.json_events {
        args.push("--json".to_owned());
    }

    let model = options
        .model_override
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| default_model.map(str::trim).filter(|s| !s.is_empty()));
    if let Some(model_val) = model {
        args.push("--model".to_owned());
        args.push(model_val.to_owned());
    }

    let config_overrides = settings.build_user_config_overrides()?;
    for (key, value) in &config_overrides {
        args.push("-c".to_owned());
        args.push(format!("{key}={value}"));
    }

    args.extend(settings.build_cli_flag_args()?);

    if let Some(prompt) = options
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        args.push(prompt.to_owned());
    }

    let mut env = settings.env_vars.clone();
    let mut masked_env_keys = BTreeSet::new();
    for key in settings.secret_env_vars.keys() {
        masked_env_keys.insert(key.clone());
        env.entry(key.clone())
            .or_insert_with(|| format!("[secret:{key}]"));
    }

    Ok(CodexSpawnPlan {
        binary_path: binary_path.as_ref().to_path_buf(),
        working_dir: options.working_dir.clone(),
        args,
        env,
        env_remove: BTreeSet::new(),
        masked_env_keys,
        config_overrides,
    })
}

/// Convenience helper returning a ready-to-spawn [`std::process::Command`] for `codex`.
pub fn build_codex_command(
    binary_path: impl AsRef<Path>,
    default_model: Option<&str>,
    settings: &CodexSpawnSettings,
    options: &CodexSpawnOptions,
) -> Result<Command, CodexSettingsError> {
    let plan = build_codex_spawn_plan(binary_path, default_model, settings, options)?;
    Ok(plan.to_command())
}

/// Finds a setting specification by its canonical [`CodexSettingSpec::id`].
pub fn find_codex_setting_spec(id: &str) -> Option<&'static CodexSettingSpec> {
    CODEX_SETTING_SPECS.iter().find(|spec| spec.id == id)
}

/// Returns all setting specifications belonging to `category`.
pub fn codex_setting_specs_for_category(
    category: CodexSettingCategory,
) -> Vec<&'static CodexSettingSpec> {
    CODEX_SETTING_SPECS
        .iter()
        .filter(|spec| spec.category == category)
        .collect()
}

fn split_shell_words(input: &str) -> Result<Vec<String>, CodexSettingsError> {
    split_claude_shell_words(input)
        .map_err(|err| CodexSettingsError::InvalidExtraCliArgs(err.to_string()))
}

fn validate_env_var_name(name: &str) -> Result<(), CodexSettingsError> {
    if name.is_empty() {
        return Err(CodexSettingsError::EmptyEnvName);
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(CodexSettingsError::EmptyEnvName);
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(CodexSettingsError::InvalidEnvName(name.to_owned()));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(CodexSettingsError::InvalidEnvName(name.to_owned()));
    }
    Ok(())
}

fn validate_config_key_path(key: &str) -> Result<(), CodexSettingsError> {
    if key.is_empty() {
        return Err(CodexSettingsError::InvalidExtraConfigOverrides(
            "config key cannot be empty".to_owned(),
        ));
    }
    for segment in key.split('.') {
        if segment.is_empty() {
            return Err(CodexSettingsError::InvalidExtraConfigOverrides(format!(
                "invalid dotted key '{key}' (empty path segment)"
            )));
        }
        let quoted = segment.starts_with('"') && segment.ends_with('"') && segment.len() >= 2;
        if !quoted
            && !segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        {
            return Err(CodexSettingsError::InvalidExtraConfigOverrides(format!(
                "invalid key segment '{segment}' in '{key}' (use letters, digits, '_', '-', or quotes)"
            )));
        }
    }
    Ok(())
}

fn parse_extra_config_overrides(raw: &str) -> Result<Vec<(String, String)>, CodexSettingsError> {
    let pairs = parse_extra_config_overrides_lenient_guard(raw)?;
    for (key, _) in &pairs {
        if key == CODEX_DISABLE_UPDATE_CHECK_KEY {
            return Err(CodexSettingsError::ReservedConfigKey(key.clone()));
        }
    }
    Ok(pairs)
}

fn parse_extra_config_overrides_lenient_guard(
    raw: &str,
) -> Result<Vec<(String, String)>, CodexSettingsError> {
    let mut out = Vec::new();
    for (idx, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((lhs, rhs)) = trimmed.split_once('=') else {
            return Err(CodexSettingsError::InvalidExtraConfigOverrides(format!(
                "line {} must be in 'key = value' format",
                idx + 1
            )));
        };
        let key = lhs.trim();
        let val = rhs.trim();
        validate_config_key_path(key)?;
        if val.is_empty() {
            return Err(CodexSettingsError::InvalidExtraConfigOverrides(format!(
                "line {}: value for '{key}' cannot be empty",
                idx + 1
            )));
        }
        out.push((key.to_owned(), val.to_owned()));
    }
    Ok(out)
}

fn validate_value_for_spec(
    spec: &CodexSettingSpec,
    value: &CodexSettingValue,
) -> Result<(), CodexSettingsError> {
    let err = |reason: String| CodexSettingsError::InvalidValue {
        key: spec.display_key.to_owned(),
        reason,
    };

    match (spec.kind, value) {
        (CodexSettingKind::Bool { .. }, CodexSettingValue::Bool(_)) => Ok(()),
        (
            CodexSettingKind::Choice {
                options,
                allow_custom,
                ..
            },
            CodexSettingValue::String(s),
        ) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Err(err("value cannot be empty".to_owned()));
            }
            if !allow_custom && !options.contains(&trimmed) {
                return Err(err(format!("expected one of: {}", options.join(", "))));
            }
            Ok(())
        }
        (CodexSettingKind::String { allow_empty, .. }, CodexSettingValue::String(s)) => {
            if !allow_empty && s.trim().is_empty() {
                return Err(err("value cannot be empty".to_owned()));
            }
            Ok(())
        }
        (CodexSettingKind::Integer { min, max, .. }, CodexSettingValue::Integer(n)) => {
            if let Some(lo) = min
                && *n < lo
            {
                return Err(err(format!("must be at least {lo}")));
            }
            if let Some(hi) = max
                && *n > hi
            {
                return Err(err(format!("must be at most {hi}")));
            }
            Ok(())
        }
        (CodexSettingKind::Number { min, max, .. }, CodexSettingValue::Number(raw)) => {
            let parsed: f64 = raw
                .trim()
                .parse()
                .map_err(|_| err("must be a valid number".to_owned()))?;
            if !parsed.is_finite() {
                return Err(err("must be a finite number".to_owned()));
            }
            if let Some(lo) = min
                && parsed < lo
            {
                return Err(err(format!("must be at least {lo}")));
            }
            if let Some(hi) = max
                && parsed > hi
            {
                return Err(err(format!("must be at most {hi}")));
            }
            Ok(())
        }
        (CodexSettingKind::StringList { .. }, CodexSettingValue::StringList(_)) => Ok(()),
        (CodexSettingKind::Json { expect_array, .. }, CodexSettingValue::Json(raw)) => {
            let parsed: serde_json::Value =
                serde_json::from_str(raw.trim()).map_err(|e| err(format!("invalid JSON: {e}")))?;
            if expect_array && !parsed.is_array() {
                return Err(err("expected a JSON array ([ ... ])".to_owned()));
            }
            if !expect_array && !parsed.is_object() {
                return Err(err("expected a JSON object ({ ... })".to_owned()));
            }
            json_value_to_toml(&parsed).map_err(err)?;
            Ok(())
        }
        _ => Err(err("value type does not match setting schema".to_owned())),
    }
}

fn parse_text_for_spec(
    spec: &CodexSettingSpec,
    raw: &str,
) -> Result<CodexSettingValue, CodexSettingsError> {
    let err = |reason: String| CodexSettingsError::InvalidValue {
        key: spec.display_key.to_owned(),
        reason,
    };

    match spec.kind {
        CodexSettingKind::Bool { .. } => match raw.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Ok(CodexSettingValue::Bool(true)),
            "false" | "0" | "no" | "off" => Ok(CodexSettingValue::Bool(false)),
            _ => Err(err("expected true or false".to_owned())),
        },
        CodexSettingKind::Choice {
            options,
            allow_custom,
            ..
        } => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Err(err("value cannot be empty".to_owned()));
            }
            if !allow_custom && !options.contains(&trimmed) {
                return Err(err(format!("expected one of: {}", options.join(", "))));
            }
            Ok(CodexSettingValue::String(trimmed.to_owned()))
        }
        CodexSettingKind::String { allow_empty, .. } => {
            let trimmed = raw.trim();
            if !allow_empty && trimmed.is_empty() {
                return Err(err("value cannot be empty".to_owned()));
            }
            Ok(CodexSettingValue::String(
                if allow_empty && raw.is_empty() {
                    String::new()
                } else {
                    trimmed.to_owned()
                },
            ))
        }
        CodexSettingKind::Integer { min, max, .. } => {
            let parsed: i64 = raw
                .trim()
                .parse()
                .map_err(|_| err("must be a whole integer".to_owned()))?;
            if let Some(lo) = min
                && parsed < lo
            {
                return Err(err(format!("must be at least {lo}")));
            }
            if let Some(hi) = max
                && parsed > hi
            {
                return Err(err(format!("must be at most {hi}")));
            }
            Ok(CodexSettingValue::Integer(parsed))
        }
        CodexSettingKind::Number { min, max, .. } => {
            let trimmed = raw.trim();
            let parsed: f64 = trimmed
                .parse()
                .map_err(|_| err("must be a valid number".to_owned()))?;
            if !parsed.is_finite() {
                return Err(err("must be a finite number".to_owned()));
            }
            if let Some(lo) = min
                && parsed < lo
            {
                return Err(err(format!("must be at least {lo}")));
            }
            if let Some(hi) = max
                && parsed > hi
            {
                return Err(err(format!("must be at most {hi}")));
            }
            Ok(CodexSettingValue::Number(trimmed.to_owned()))
        }
        CodexSettingKind::StringList { .. } => {
            let items: Vec<String> = raw
                .split([',', '\n'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            Ok(CodexSettingValue::StringList(items))
        }
        CodexSettingKind::Json { expect_array, .. } => {
            let trimmed = raw.trim();
            let parsed: serde_json::Value =
                serde_json::from_str(trimmed).map_err(|e| err(format!("invalid JSON: {e}")))?;
            if expect_array && !parsed.is_array() {
                return Err(err("expected a JSON array ([ ... ])".to_owned()));
            }
            if !expect_array && !parsed.is_object() {
                return Err(err("expected a JSON object ({ ... })".to_owned()));
            }
            json_value_to_toml(&parsed).map_err(err)?;
            let normalized = serde_json::to_string(&parsed).unwrap_or_else(|_| trimmed.to_owned());
            Ok(CodexSettingValue::Json(normalized))
        }
    }
}

fn setting_value_to_toml(
    spec: &CodexSettingSpec,
    value: &CodexSettingValue,
) -> Result<String, CodexSettingsError> {
    match value {
        CodexSettingValue::Bool(b) => Ok(if *b { "true" } else { "false" }.to_owned()),
        CodexSettingValue::String(s) => Ok(toml_quote_string(s)),
        CodexSettingValue::Integer(n) => Ok(n.to_string()),
        CodexSettingValue::Number(raw) => {
            let trimmed = raw.trim();
            let parsed: f64 = trimmed
                .parse()
                .map_err(|_| CodexSettingsError::InvalidValue {
                    key: spec.display_key.to_owned(),
                    reason: "invalid number".to_owned(),
                })?;
            if !parsed.is_finite() {
                return Err(CodexSettingsError::InvalidValue {
                    key: spec.display_key.to_owned(),
                    reason: "number must be finite".to_owned(),
                });
            }
            Ok(trimmed.to_owned())
        }
        CodexSettingValue::StringList(items) => {
            let quoted: Vec<String> = items.iter().map(|s| toml_quote_string(s)).collect();
            Ok(format!("[{}]", quoted.join(", ")))
        }
        CodexSettingValue::Json(raw) => {
            let parsed: serde_json::Value = serde_json::from_str(raw.trim()).map_err(|err| {
                CodexSettingsError::InvalidValue {
                    key: spec.display_key.to_owned(),
                    reason: format!("invalid JSON: {err}"),
                }
            })?;
            json_value_to_toml(&parsed).map_err(|reason| CodexSettingsError::InvalidValue {
                key: spec.display_key.to_owned(),
                reason,
            })
        }
    }
}

fn toml_quote_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('"');
    for ch in raw.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c == '\x7f' => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn toml_quote_key(key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    {
        key.to_owned()
    } else {
        toml_quote_string(key)
    }
}

fn json_value_to_toml(value: &serde_json::Value) -> Result<String, String> {
    match value {
        serde_json::Value::Null => Err("TOML does not support null values".to_owned()),
        serde_json::Value::Bool(b) => Ok(if *b { "true" } else { "false" }.to_owned()),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        serde_json::Value::String(s) => Ok(toml_quote_string(s)),
        serde_json::Value::Array(items) => {
            let mut rendered = Vec::with_capacity(items.len());
            for item in items {
                rendered.push(json_value_to_toml(item)?);
            }
            Ok(format!("[{}]", rendered.join(", ")))
        }
        serde_json::Value::Object(map) => {
            let mut entries = Vec::with_capacity(map.len());
            for (k, v) in map {
                entries.push(format!(
                    "{} = {}",
                    toml_quote_key(k),
                    json_value_to_toml(v)?
                ));
            }
            Ok(format!("{{{}}}", entries.join(", ")))
        }
    }
}

fn encode_cli_flag(args: &mut Vec<String>, encoding: CliFlagEncoding, value: &CodexSettingValue) {
    match (encoding, value) {
        (CliFlagEncoding::BoolFlag(flag), CodexSettingValue::Bool(true)) => {
            args.push(flag.to_owned());
        }
        (CliFlagEncoding::BoolFlag(_), CodexSettingValue::Bool(false)) => {}
        (CliFlagEncoding::BoolTogglePair { on, off }, CodexSettingValue::Bool(enabled)) => {
            args.push(if *enabled { on } else { off }.to_owned());
        }
        (CliFlagEncoding::BoolValueFlag(flag), CodexSettingValue::Bool(enabled)) => {
            args.push(flag.to_owned());
            args.push(if *enabled { "true" } else { "false" }.to_owned());
        }
        (CliFlagEncoding::ValueFlag(flag), CodexSettingValue::String(s)) => {
            args.push(flag.to_owned());
            args.push(s.clone());
        }
        (CliFlagEncoding::ValueFlag(flag), CodexSettingValue::Integer(n)) => {
            args.push(flag.to_owned());
            args.push(n.to_string());
        }
        (CliFlagEncoding::ValueFlag(flag), CodexSettingValue::Number(n)) => {
            args.push(flag.to_owned());
            args.push(n.clone());
        }
        (CliFlagEncoding::ValueFlag(flag), CodexSettingValue::Json(raw)) => {
            args.push(flag.to_owned());
            args.push(raw.clone());
        }
        (CliFlagEncoding::OptionalValueFlag(flag), CodexSettingValue::String(s)) => {
            args.push(flag.to_owned());
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                args.push(trimmed.to_owned());
            }
        }
        (CliFlagEncoding::MultiValueFlag(flag), CodexSettingValue::StringList(items)) => {
            if items.is_empty() {
                args.push(flag.to_owned());
                args.push(String::new());
            } else {
                args.push(flag.to_owned());
                args.extend(items.iter().cloned());
            }
        }
        (CliFlagEncoding::RepeatableFlag(flag), CodexSettingValue::StringList(items)) => {
            for item in items {
                args.push(flag.to_owned());
                args.push(item.clone());
            }
        }
        _ => {}
    }
}

fn shell_quote(raw: &str) -> String {
    if raw.is_empty() {
        return "''".to_owned();
    }
    let safe = raw.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '=' | '@' | ',')
    });
    if safe {
        raw.to_owned()
    } else {
        format!("'{}'", raw.replace('\'', "'\\''"))
    }
}

/// Complete catalog of user-configurable Codex CLI launch flags and `config.toml` (`-c`) options.
pub const CODEX_SETTING_SPECS: &[CodexSettingSpec] = &[
    // --- Category 1: Launch & CLI Flags (`codex --help` / `codex exec --help`) -----------
    CodexSettingSpec {
        id: "cli.sandbox",
        label: "Sandbox Mode (CLI Flag)",
        display_key: "--sandbox",
        description: "Sandbox policy passed directly on the CLI when executing model-generated shell commands.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--sandbox")),
        kind: CodexSettingKind::Choice {
            options: &["read-only", "workspace-write", "danger-full-access"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "cli.ask-for-approval",
        label: "Ask for Approval (Interactive)",
        display_key: "--ask-for-approval",
        description: "Configure when interactive Codex sessions ask for human approval before running commands.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--ask-for-approval")),
        kind: CodexSettingKind::Choice {
            options: &["on-request", "never"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "cli.profile",
        label: "Config Profile Layer",
        display_key: "--profile",
        description: "Layer $CODEX_HOME/<name>.config.toml on top of the base user config.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--profile")),
        kind: CodexSettingKind::String {
            placeholder: "e.g. fast or strict",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "cli.approve-for-me",
        label: "Auto-Review Approvals",
        display_key: "--approve-for-me",
        description: "Route approval requests through automatic review using the workspace-write sandbox.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--approve-for-me")),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.dangerously-bypass-approvals-and-sandbox",
        label: "Dangerously Bypass Approvals & Sandbox",
        display_key: "--dangerously-bypass-approvals-and-sandbox",
        description: "Skip all confirmation prompts and run commands without sandboxing. Use only in externally sandboxed environments.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag(
            "--dangerously-bypass-approvals-and-sandbox",
        )),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.dangerously-bypass-hook-trust",
        label: "Dangerously Bypass Hook Trust",
        display_key: "--dangerously-bypass-hook-trust",
        description: "Run enabled hooks without requiring persisted hook trust for this invocation.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag(
            "--dangerously-bypass-hook-trust",
        )),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.worktree",
        label: "Managed Git Worktree",
        display_key: "--worktree",
        description: "Run the session inside a new Codex-managed Git worktree.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--worktree")),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.add-dir",
        label: "Additional Writable Directories",
        display_key: "--add-dir",
        description: "Extra directories that should be writable alongside the primary workspace (repeatable).",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::RepeatableFlag("--add-dir")),
        kind: CodexSettingKind::StringList {
            placeholder: "/path/to/shared-libs, ../docs",
        },
    },
    CodexSettingSpec {
        id: "cli.enable",
        label: "Enable Features (CLI)",
        display_key: "--enable",
        description: "Feature flag names to enable on launch (repeatable; equivalent to -c features.<name>=true).",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::RepeatableFlag("--enable")),
        kind: CodexSettingKind::StringList {
            placeholder: "memories, multi_agent_v2",
        },
    },
    CodexSettingSpec {
        id: "cli.disable",
        label: "Disable Features (CLI)",
        display_key: "--disable",
        description: "Feature flag names to disable on launch (repeatable; equivalent to -c features.<name>=false).",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::RepeatableFlag("--disable")),
        kind: CodexSettingKind::StringList {
            placeholder: "shell_snapshot, daemon_auto_start",
        },
    },
    CodexSettingSpec {
        id: "cli.oss",
        label: "Use Open-Source Provider",
        display_key: "--oss",
        description: "Route inference through a local open-source model provider instead of OpenAI.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--oss")),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.local-provider",
        label: "Local OSS Provider",
        display_key: "--local-provider",
        description: "Local open-source provider to use when --oss is enabled (lmstudio or ollama).",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--local-provider")),
        kind: CodexSettingKind::Choice {
            options: &["lmstudio", "ollama"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "cli.strict-config",
        label: "Strict Config Validation",
        display_key: "--strict-config",
        description: "Fail immediately if config.toml contains unrecognized fields for this Codex version.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--strict-config")),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.search",
        label: "Enable Live Web Search (Interactive)",
        display_key: "--search",
        description: "Enable live native Responses web_search without per-call approval in interactive sessions.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--search")),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.no-alt-screen",
        label: "Disable Alternate Screen (Interactive)",
        display_key: "--no-alt-screen",
        description: "Run the interactive TUI in inline mode, preserving terminal scrollback history.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--no-alt-screen")),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.no-daemon",
        label: "Disable Shared Daemon",
        display_key: "--no-daemon",
        description: "Run without connecting to the shared background app-server daemon even if running.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--no-daemon")),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.remote",
        label: "Remote App-Server Endpoint",
        display_key: "--remote",
        description: "Connect to a remote app-server endpoint (ws://, wss://, or unix://PATH).",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--remote")),
        kind: CodexSettingKind::String {
            placeholder: "ws://127.0.0.1:8787 or unix:///tmp/codex.sock",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "cli.remote-auth-token-env",
        label: "Remote Auth Token Env Var",
        display_key: "--remote-auth-token-env",
        description: "Name of the environment variable holding the bearer token for remote WebSocket connections.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--remote-auth-token-env")),
        kind: CodexSettingKind::String {
            placeholder: "CODEX_REMOTE_TOKEN",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "cli.skip-git-repo-check",
        label: "Skip Git Repo Check (Exec)",
        display_key: "--skip-git-repo-check",
        description: "Allow running `codex exec` outside a Git repository.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--skip-git-repo-check")),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.ephemeral",
        label: "Ephemeral Session (Exec)",
        display_key: "--ephemeral",
        description: "Run `codex exec` without persisting session rollout files to disk.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--ephemeral")),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.ignore-user-config",
        label: "Ignore User Config (Exec)",
        display_key: "--ignore-user-config",
        description: "Skip loading $CODEX_HOME/config.toml during `codex exec` while keeping auth from CODEX_HOME.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--ignore-user-config")),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.ignore-rules",
        label: "Ignore Execpolicy Rules (Exec)",
        display_key: "--ignore-rules",
        description: "Do not load user or project execpolicy .rules files during `codex exec`.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--ignore-rules")),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.trust",
        label: "Trust Project (Exec)",
        display_key: "--trust",
        description: "Save project trust (including MCP commands outside sandbox); does not override explicit untrust.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--trust")),
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "cli.color",
        label: "Output Color Mode (Exec)",
        display_key: "--color",
        description: "Control ANSI color formatting in `codex exec` output (auto, always, never).",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--color")),
        kind: CodexSettingKind::Choice {
            options: &["auto", "always", "never"],
            allow_custom: false,
            default_hint: Some("auto"),
        },
    },
    CodexSettingSpec {
        id: "cli.output-schema",
        label: "Final Response JSON Schema (Exec)",
        display_key: "--output-schema",
        description: "Path to a JSON Schema file constraining the model's final response shape in `codex exec`.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--output-schema")),
        kind: CodexSettingKind::String {
            placeholder: "/path/to/response-schema.json",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "cli.output-last-message",
        label: "Write Last Message to File (Exec)",
        display_key: "--output-last-message",
        description: "File path where `codex exec` writes the final assistant message.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--output-last-message")),
        kind: CodexSettingKind::String {
            placeholder: "/tmp/codex-last-message.md",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "cli.cyber-access-program",
        label: "Cyber Access Program (Exec)",
        display_key: "--cyber-access-program",
        description: "Request an experimental Cyber access program for OpenAI provider turns.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--cyber-access-program")),
        kind: CodexSettingKind::Choice {
            options: &["standard", "daybreak_blue", "daybreak_red"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "cli.thread-source",
        label: "Thread Source Classification (Exec)",
        display_key: "--thread-source",
        description: "Source classification tag attached to newly created or forked threads.",
        category: CodexSettingCategory::CliFlags,
        target: CodexSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--thread-source")),
        kind: CodexSettingKind::String {
            placeholder: "e.g. cli or automation",
            allow_empty: false,
            default_hint: None,
        },
    },
    // --- Category 2: Models & Reasoning --------------------------------------------------
    CodexSettingSpec {
        id: "model_reasoning_effort",
        label: "Reasoning Effort",
        display_key: "model_reasoning_effort",
        description: "Reasoning effort level sent to models that support adjustable reasoning.",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "model_reasoning_effort",
        },
        kind: CodexSettingKind::Choice {
            options: &["none", "minimal", "low", "medium", "high", "xhigh"],
            allow_custom: true,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "plan_mode_reasoning_effort",
        label: "Plan Mode Reasoning Effort",
        display_key: "plan_mode_reasoning_effort",
        description: "Reasoning effort override used specifically while in plan mode.",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "plan_mode_reasoning_effort",
        },
        kind: CodexSettingKind::Choice {
            options: &["none", "minimal", "low", "medium", "high", "xhigh"],
            allow_custom: true,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "model_reasoning_summary",
        label: "Reasoning Summary",
        display_key: "model_reasoning_summary",
        description: "Summary verbosity for model reasoning traces (auto, concise, detailed, none).",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "model_reasoning_summary",
        },
        kind: CodexSettingKind::Choice {
            options: &["auto", "concise", "detailed", "none"],
            allow_custom: false,
            default_hint: Some("auto"),
        },
    },
    CodexSettingSpec {
        id: "model_verbosity",
        label: "Model Response Verbosity",
        display_key: "model_verbosity",
        description: "Responses API text.verbosity control for GPT-5+ models (low, medium, high).",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "model_verbosity",
        },
        kind: CodexSettingKind::Choice {
            options: &["low", "medium", "high"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "service_tier",
        label: "Service Tier",
        display_key: "service_tier",
        description: "Explicit OpenAI service tier for new turns (default, priority, flex, or legacy fast).",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "service_tier",
        },
        kind: CodexSettingKind::Choice {
            options: &["default", "priority", "flex", "fast"],
            allow_custom: true,
            default_hint: Some("default"),
        },
    },
    CodexSettingSpec {
        id: "review_model",
        label: "Review Model Override",
        display_key: "review_model",
        description: "Model override used specifically by `/review` and `codex review`.",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "review_model",
        },
        kind: CodexSettingKind::String {
            placeholder: "e.g. gpt-5.3-codex",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "model_provider",
        label: "Model Provider ID",
        display_key: "model_provider",
        description: "Provider key from built-in providers or [model_providers] (e.g. openai, ollama).",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "model_provider",
        },
        kind: CodexSettingKind::String {
            placeholder: "openai",
            allow_empty: false,
            default_hint: Some("openai"),
        },
    },
    CodexSettingSpec {
        id: "oss_provider",
        label: "Preferred OSS Provider",
        display_key: "oss_provider",
        description: "Default local provider when using open-source models (e.g. lmstudio or ollama).",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "oss_provider",
        },
        kind: CodexSettingKind::Choice {
            options: &["lmstudio", "ollama"],
            allow_custom: true,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "openai_base_url",
        label: "OpenAI Base URL Override",
        display_key: "openai_base_url",
        description: "Custom base URL for the built-in `openai` model provider (proxy or gateway).",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "openai_base_url",
        },
        kind: CodexSettingKind::String {
            placeholder: "https://api.openai.com/v1",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "chatgpt_base_url",
        label: "ChatGPT Base URL Override",
        display_key: "chatgpt_base_url",
        description: "Custom base URL for ChatGPT-authenticated backend requests.",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "chatgpt_base_url",
        },
        kind: CodexSettingKind::String {
            placeholder: "https://chatgpt.com/backend-api",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "hide_agent_reasoning",
        label: "Hide Agent Reasoning",
        display_key: "hide_agent_reasoning",
        description: "Hide AgentReasoning events from UI and output streams.",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "hide_agent_reasoning",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "show_raw_agent_reasoning",
        label: "Show Raw Agent Reasoning",
        display_key: "show_raw_agent_reasoning",
        description: "Display raw AgentReasoningRawContentEvent payloads in UI and output.",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "show_raw_agent_reasoning",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "model_context_window",
        label: "Model Context Window (Tokens)",
        display_key: "model_context_window",
        description: "Override the effective context window size (in tokens) reported for the model.",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "model_context_window",
        },
        kind: CodexSettingKind::Integer {
            min: Some(1024),
            max: None,
            placeholder: "e.g. 272000",
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "model_auto_compact_token_limit",
        label: "Auto-Compact Token Limit",
        display_key: "model_auto_compact_token_limit",
        description: "Token usage threshold triggering automatic conversation history compaction.",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "model_auto_compact_token_limit",
        },
        kind: CodexSettingKind::Integer {
            min: Some(1024),
            max: None,
            placeholder: "e.g. 180000",
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "model_auto_compact_token_limit_scope",
        label: "Auto-Compact Token Limit Scope",
        display_key: "model_auto_compact_token_limit_scope",
        description: "Apply auto-compaction threshold to total context or only tokens after the carried prefix.",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "model_auto_compact_token_limit_scope",
        },
        kind: CodexSettingKind::Choice {
            options: &["total", "body_after_prefix"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "model_post_turn_compact_threshold_percent",
        label: "Post-Turn Compact Threshold (%)",
        display_key: "model_post_turn_compact_threshold_percent",
        description: "Percentage of usable context window triggering compaction after a final response (0 disables).",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "model_post_turn_compact_threshold_percent",
        },
        kind: CodexSettingKind::Integer {
            min: Some(0),
            max: Some(100),
            placeholder: "e.g. 85",
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "responses.process_mode",
        label: "Response Processing Mode",
        display_key: "responses.process_mode",
        description: "Process model response items incrementally while streaming or only after completion.",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "responses.process_mode",
        },
        kind: CodexSettingKind::Choice {
            options: &["streaming", "after_complete_response"],
            allow_custom: false,
            default_hint: Some("streaming"),
        },
    },
    CodexSettingSpec {
        id: "daybreak",
        label: "Daybreak Mode",
        display_key: "daybreak",
        description: "Default Daybreak preference for new threads and non-interactive turns.",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml { path: "daybreak" },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    CodexSettingSpec {
        id: "model_catalog_json",
        label: "Custom Model Catalog JSON Path",
        display_key: "model_catalog_json",
        description: "Absolute path to a local JSON model catalog loaded on Codex startup.",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "model_catalog_json",
        },
        kind: CodexSettingKind::String {
            placeholder: "/path/to/models.json",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "model_providers",
        label: "Custom Model Providers (JSON)",
        display_key: "model_providers",
        description: "Map of custom provider entries extending built-in providers (serialized to TOML inline table).",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "model_providers",
        },
        kind: CodexSettingKind::Json {
            placeholder: r#"{"my_proxy":{"name":"Proxy","base_url":"https://proxy.example/v1","env_key":"PROXY_API_KEY"}}"#,
            expect_array: false,
        },
    },
    CodexSettingSpec {
        id: "responses_api_metadata",
        label: "Responses API Metadata (JSON)",
        display_key: "responses_api_metadata",
        description: "Key-value string metadata attached to every OpenAI Responses API request.",
        category: CodexSettingCategory::ModelsAndReasoning,
        target: CodexSettingTarget::ConfigToml {
            path: "responses_api_metadata",
        },
        kind: CodexSettingKind::Json {
            placeholder: r#"{"team":"platform","env":"ci"}"#,
            expect_array: false,
        },
    },
    // --- Category 3: Approvals & Permissions ---------------------------------------------
    CodexSettingSpec {
        id: "approval_policy",
        label: "Approval Policy",
        display_key: "approval_policy",
        description: "Default approval policy for executing commands (`on-request` or `never`).",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "approval_policy",
        },
        kind: CodexSettingKind::Choice {
            options: &["on-request", "never"],
            allow_custom: false,
            default_hint: Some("on-request"),
        },
    },
    CodexSettingSpec {
        id: "approvals_reviewer",
        label: "Approvals Reviewer",
        display_key: "approvals_reviewer",
        description: "Who reviews escalated approval requests: interactive user or automatic guardian reviewer.",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "approvals_reviewer",
        },
        kind: CodexSettingKind::Choice {
            options: &["user", "auto_review", "guardian_subagent"],
            allow_custom: false,
            default_hint: Some("user"),
        },
    },
    CodexSettingSpec {
        id: "default_permissions",
        label: "Default Permissions Profile",
        display_key: "default_permissions",
        description: "Named permissions profile to apply (built-in `:name` or a key under [permissions]).",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "default_permissions",
        },
        kind: CodexSettingKind::String {
            placeholder: "e.g. :workspace-write or custom-profile",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "permissions",
        label: "Named Permissions Profiles (JSON)",
        display_key: "permissions",
        description: "Custom permissions profile definitions keyed by profile name.",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "permissions",
        },
        kind: CodexSettingKind::Json {
            placeholder: r#"{"strict":{"filesystem":{"read":["."]}}}"#,
            expect_array: false,
        },
    },
    CodexSettingSpec {
        id: "auto_review.policy",
        label: "Auto-Review Guardian Policy",
        display_key: "auto_review.policy",
        description: "Additional natural-language policy instructions injected into the guardian auto-reviewer prompt.",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "auto_review.policy",
        },
        kind: CodexSettingKind::String {
            placeholder: "Deny destructive git pushes and production deploys",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "auto_review.circuit_break_action",
        label: "Auto-Review Circuit Break Action",
        display_key: "auto_review.circuit_break_action",
        description: "Whether guardian circuit-breaker interruptions emit default or strict structured errors.",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "auto_review.circuit_break_action",
        },
        kind: CodexSettingKind::Choice {
            options: &["default", "strict"],
            allow_custom: false,
            default_hint: Some("default"),
        },
    },
    CodexSettingSpec {
        id: "auto_review.timeout_ms",
        label: "Auto-Review Timeout (ms)",
        display_key: "auto_review.timeout_ms",
        description: "Overall Guardian review deadline in milliseconds (0 disables deadline).",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "auto_review.timeout_ms",
        },
        kind: CodexSettingKind::Integer {
            min: Some(0),
            max: None,
            placeholder: "90000",
            default_hint: Some(90_000),
        },
    },
    CodexSettingSpec {
        id: "auto_review.conversation_history_max_output_tokens",
        label: "Auto-Review History Max Tokens",
        display_key: "auto_review.conversation_history_max_output_tokens",
        description: "Maximum estimated tokens per Guardian conversation-history tool response.",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "auto_review.conversation_history_max_output_tokens",
        },
        kind: CodexSettingKind::Integer {
            min: Some(256),
            max: None,
            placeholder: "4000",
            default_hint: Some(4000),
        },
    },
    CodexSettingSpec {
        id: "apps._default.enabled",
        label: "Apps Enabled by Default",
        display_key: "apps._default.enabled",
        description: "Whether Codex Apps integrations are enabled unless overridden per app.",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "apps._default.enabled",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "apps._default.default_tools_approval_mode",
        label: "Default App Tools Approval Mode",
        display_key: "apps._default.default_tools_approval_mode",
        description: "Default approval mode for Codex App tools (auto, prompt, writes, approve).",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "apps._default.default_tools_approval_mode",
        },
        kind: CodexSettingKind::Choice {
            options: &["auto", "prompt", "writes", "approve"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "apps._default.approvals_reviewer",
        label: "Default App Approvals Reviewer",
        display_key: "apps._default.approvals_reviewer",
        description: "Reviewer for app tool approval prompts unless overridden per app.",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "apps._default.approvals_reviewer",
        },
        kind: CodexSettingKind::Choice {
            options: &["user", "auto_review", "guardian_subagent"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "apps._default.destructive_enabled",
        label: "Allow Destructive App Tools",
        display_key: "apps._default.destructive_enabled",
        description: "Allow app tools annotated with destructive_hint = true by default.",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "apps._default.destructive_enabled",
        },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    CodexSettingSpec {
        id: "apps._default.open_world_enabled",
        label: "Allow Open-World App Tools",
        display_key: "apps._default.open_world_enabled",
        description: "Allow app tools annotated with open_world_hint = true by default.",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "apps._default.open_world_enabled",
        },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    CodexSettingSpec {
        id: "include_permissions_instructions",
        label: "Include Permissions Instructions",
        display_key: "include_permissions_instructions",
        description: "Inject the <permissions instructions> developer block into prompts.",
        category: CodexSettingCategory::ApprovalsAndPermissions,
        target: CodexSettingTarget::ConfigToml {
            path: "include_permissions_instructions",
        },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    // --- Category 4: Sandbox & Shell -----------------------------------------------------
    CodexSettingSpec {
        id: "sandbox_mode",
        label: "Sandbox Mode",
        display_key: "sandbox_mode",
        description: "Default sandbox mode in config.toml for executing shell commands.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "sandbox_mode",
        },
        kind: CodexSettingKind::Choice {
            options: &["read-only", "workspace-write", "danger-full-access"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "sandbox_workspace_write.network_access",
        label: "Workspace-Write Network Access",
        display_key: "sandbox_workspace_write.network_access",
        description: "Allow outbound network access inside the workspace-write sandbox.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "sandbox_workspace_write.network_access",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "sandbox_workspace_write.writable_roots",
        label: "Extra Writable Roots",
        display_key: "sandbox_workspace_write.writable_roots",
        description: "Additional absolute directory paths writable under workspace-write sandbox mode.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "sandbox_workspace_write.writable_roots",
        },
        kind: CodexSettingKind::StringList {
            placeholder: "/tmp/build-cache, /Users/dev/.cargo",
        },
    },
    CodexSettingSpec {
        id: "sandbox_workspace_write.exclude_slash_tmp",
        label: "Exclude /tmp From Writable Roots",
        display_key: "sandbox_workspace_write.exclude_slash_tmp",
        description: "Prevent workspace-write sandbox from automatically granting write access to /tmp.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "sandbox_workspace_write.exclude_slash_tmp",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "sandbox_workspace_write.exclude_tmpdir_env_var",
        label: "Exclude $TMPDIR From Writable Roots",
        display_key: "sandbox_workspace_write.exclude_tmpdir_env_var",
        description: "Prevent workspace-write sandbox from granting write access to the TMPDIR directory.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "sandbox_workspace_write.exclude_tmpdir_env_var",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "allow_login_shell",
        label: "Allow Login Shell",
        display_key: "allow_login_shell",
        description: "Allow shell tools to request a login shell (`login = true`).",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "allow_login_shell",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "allow_symlinked_codex_home",
        label: "Allow Symlinked CODEX_HOME (macOS)",
        display_key: "allow_symlinked_codex_home",
        description: "Allow macOS sandbox writable roots at or beneath CODEX_HOME to traverse symlinks.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "allow_symlinked_codex_home",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "background_terminal_max_timeout",
        label: "Background Terminal Max Timeout (ms)",
        display_key: "background_terminal_max_timeout",
        description: "Maximum poll window in milliseconds for background terminal output (`write_stdin`).",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "background_terminal_max_timeout",
        },
        kind: CodexSettingKind::Integer {
            min: Some(1000),
            max: None,
            placeholder: "300000",
            default_hint: Some(300_000),
        },
    },
    CodexSettingSpec {
        id: "shell_environment_policy.inherit",
        label: "Shell Env Inheritance",
        display_key: "shell_environment_policy.inherit",
        description: "Which parent environment variables shell tool subprocesses inherit (core, all, none).",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "shell_environment_policy.inherit",
        },
        kind: CodexSettingKind::Choice {
            options: &["core", "all", "none"],
            allow_custom: false,
            default_hint: Some("core"),
        },
    },
    CodexSettingSpec {
        id: "shell_environment_policy.ignore_default_excludes",
        label: "Ignore Default Env Excludes",
        display_key: "shell_environment_policy.ignore_default_excludes",
        description: "Disable built-in secret/token env variable exclusion patterns for shell tools.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "shell_environment_policy.ignore_default_excludes",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "shell_environment_policy.include_only",
        label: "Shell Env Include-Only Patterns",
        display_key: "shell_environment_policy.include_only",
        description: "Regex patterns restricting which environment variables are forwarded to shell tools.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "shell_environment_policy.include_only",
        },
        kind: CodexSettingKind::StringList {
            placeholder: "^PATH$, ^HOME$, ^CARGO_.*",
        },
    },
    CodexSettingSpec {
        id: "shell_environment_policy.exclude",
        label: "Shell Env Exclude Patterns",
        display_key: "shell_environment_policy.exclude",
        description: "Regex patterns stripped from shell tool subprocess environments.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "shell_environment_policy.exclude",
        },
        kind: CodexSettingKind::StringList {
            placeholder: ".*_SECRET$, AWS_.*",
        },
    },
    CodexSettingSpec {
        id: "shell_environment_policy.set",
        label: "Shell Env Fixed Overrides (JSON)",
        display_key: "shell_environment_policy.set",
        description: "Map of environment variables always injected into shell tool subprocesses.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "shell_environment_policy.set",
        },
        kind: CodexSettingKind::Json {
            placeholder: r#"{"CI":"1","RUST_BACKTRACE":"1"}"#,
            expect_array: false,
        },
    },
    CodexSettingSpec {
        id: "shell_environment_policy.experimental_use_profile",
        label: "Shell Env Use Profile",
        display_key: "shell_environment_policy.experimental_use_profile",
        description: "Use shell profile initialization when building the subprocess environment.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "shell_environment_policy.experimental_use_profile",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "browser_use.allow_history_access",
        label: "Browser Use History Access",
        display_key: "browser_use.allow_history_access",
        description: "Allow browser automation tool to read browser history.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "browser_use.allow_history_access",
        },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    CodexSettingSpec {
        id: "browser_use.default_origin_policy.access",
        label: "Browser Default Origin Access",
        display_key: "browser_use.default_origin_policy.access",
        description: "Default allow/deny access policy for web origins in browser automation.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "browser_use.default_origin_policy.access",
        },
        kind: CodexSettingKind::Choice {
            options: &["allow", "deny"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "browser_use.default_origin_policy.downloads",
        label: "Browser Default Downloads Policy",
        display_key: "browser_use.default_origin_policy.downloads",
        description: "Default allow/deny policy for file downloads in browser automation.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "browser_use.default_origin_policy.downloads",
        },
        kind: CodexSettingKind::Choice {
            options: &["allow", "deny"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "browser_use.default_origin_policy.uploads",
        label: "Browser Default Uploads Policy",
        display_key: "browser_use.default_origin_policy.uploads",
        description: "Default allow/deny policy for file uploads in browser automation.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "browser_use.default_origin_policy.uploads",
        },
        kind: CodexSettingKind::Choice {
            options: &["allow", "deny"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "computer_use.default_app_access",
        label: "Computer Use Default App Access",
        display_key: "computer_use.default_app_access",
        description: "Default allow/deny policy for desktop apps in computer-use automation.",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "computer_use.default_app_access",
        },
        kind: CodexSettingKind::Choice {
            options: &["allow", "deny"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "windows.sandbox",
        label: "Windows Sandbox Mode",
        display_key: "windows.sandbox",
        description: "Sandbox backend selection on Windows hosts (elevated, unelevated, mxc).",
        category: CodexSettingCategory::SandboxAndShell,
        target: CodexSettingTarget::ConfigToml {
            path: "windows.sandbox",
        },
        kind: CodexSettingKind::Choice {
            options: &["elevated", "unelevated", "mxc"],
            allow_custom: false,
            default_hint: None,
        },
    },
    // --- Category 5: Context & Memory ----------------------------------------------------
    CodexSettingSpec {
        id: "developer_instructions",
        label: "Developer Instructions",
        display_key: "developer_instructions",
        description: "Additional instructions inserted as a `developer` role message on every session.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "developer_instructions",
        },
        kind: CodexSettingKind::String {
            placeholder: "Follow repository conventions and run cargo test before finishing.",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "instructions",
        label: "System Instructions Override",
        display_key: "instructions",
        description: "Custom system instructions passed to Codex.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "instructions",
        },
        kind: CodexSettingKind::String {
            placeholder: "Custom system instructions text",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "model_instructions_file",
        label: "Model Instructions File",
        display_key: "model_instructions_file",
        description: "Absolute path to a file overriding the built-in instructions for the selected model.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "model_instructions_file",
        },
        kind: CodexSettingKind::String {
            placeholder: "/path/to/instructions.md",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "compact_prompt",
        label: "Compaction Prompt",
        display_key: "compact_prompt",
        description: "Custom prompt used when compacting conversation history.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "compact_prompt",
        },
        kind: CodexSettingKind::String {
            placeholder: "Preserve file paths, test results, and open TODOs.",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "experimental_compact_prompt_file",
        label: "Compaction Prompt File",
        display_key: "experimental_compact_prompt_file",
        description: "Absolute path to a file containing the custom history compaction prompt.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "experimental_compact_prompt_file",
        },
        kind: CodexSettingKind::String {
            placeholder: "/path/to/compact-prompt.md",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "project_doc_fallback_filenames",
        label: "Project Doc Fallback Filenames",
        display_key: "project_doc_fallback_filenames",
        description: "Ordered fallback filenames checked when AGENTS.md is missing (e.g. CLAUDE.md, TEAM.md).",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "project_doc_fallback_filenames",
        },
        kind: CodexSettingKind::StringList {
            placeholder: "CLAUDE.md, .github/copilot-instructions.md",
        },
    },
    CodexSettingSpec {
        id: "project_doc_max_bytes",
        label: "Project Doc Max Bytes",
        display_key: "project_doc_max_bytes",
        description: "Maximum total bytes loaded from project instruction files across selected environments.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "project_doc_max_bytes",
        },
        kind: CodexSettingKind::Integer {
            min: Some(0),
            max: None,
            placeholder: "32768",
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "project_root_markers",
        label: "Project Root Markers",
        display_key: "project_root_markers",
        description: "Files or directories marking the project root when scanning parent folders for .codex.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "project_root_markers",
        },
        kind: CodexSettingKind::StringList {
            placeholder: ".git, .hg, Cargo.toml",
        },
    },
    CodexSettingSpec {
        id: "tool_output_token_limit",
        label: "Tool Output Token Limit",
        display_key: "tool_output_token_limit",
        description: "Token budget applied when storing tool/function outputs in the context manager.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "tool_output_token_limit",
        },
        kind: CodexSettingKind::Integer {
            min: Some(256),
            max: None,
            placeholder: "10000",
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "include_environment_context",
        label: "Include Environment Context",
        display_key: "include_environment_context",
        description: "Inject the <environment_context> user block (cwd, shell, OS) into sessions.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "include_environment_context",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "include_environment_context_time",
        label: "Include Time in Environment Context",
        display_key: "include_environment_context_time",
        description: "Include the current local date and timezone in <environment_context>.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "include_environment_context_time",
        },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    CodexSettingSpec {
        id: "include_apps_instructions",
        label: "Include Apps Instructions",
        display_key: "include_apps_instructions",
        description: "Inject the <apps_instructions> developer block into prompts.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "include_apps_instructions",
        },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    CodexSettingSpec {
        id: "include_collaboration_mode_instructions",
        label: "Include Collaboration Mode Instructions",
        display_key: "include_collaboration_mode_instructions",
        description: "Inject the <collaboration_mode> developer block into prompts.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "include_collaboration_mode_instructions",
        },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    CodexSettingSpec {
        id: "history.persistence",
        label: "History Persistence",
        display_key: "history.persistence",
        description: "Control whether session prompts are written to ~/.codex/history.jsonl.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "history.persistence",
        },
        kind: CodexSettingKind::Choice {
            options: &["save-all", "none"],
            allow_custom: false,
            default_hint: Some("save-all"),
        },
    },
    CodexSettingSpec {
        id: "history.max_bytes",
        label: "History File Max Bytes",
        display_key: "history.max_bytes",
        description: "Maximum size in bytes of ~/.codex/history.jsonl before oldest entries are pruned.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "history.max_bytes",
        },
        kind: CodexSettingKind::Integer {
            min: Some(1024),
            max: None,
            placeholder: "10485760",
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "memories.use_memories",
        label: "Use Saved Memories",
        display_key: "memories.use_memories",
        description: "Inject memory usage instructions into developer prompts.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "memories.use_memories",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "memories.generate_memories",
        label: "Generate Memories From Threads",
        display_key: "memories.generate_memories",
        description: "Allow newly created threads to generate memories in the state database.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "memories.generate_memories",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "memories.version",
        label: "Memories Pipeline Version",
        display_key: "memories.version",
        description: "Select memory subsystem pipeline version (v1 or v2).",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "memories.version",
        },
        kind: CodexSettingKind::Choice {
            options: &["v1", "v2"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "memories.extract_model",
        label: "Memory Extraction Model",
        display_key: "memories.extract_model",
        description: "Model used for summarizing threads into raw memories.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "memories.extract_model",
        },
        kind: CodexSettingKind::String {
            placeholder: "e.g. gpt-5.3-codex",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "memories.consolidation_model",
        label: "Memory Consolidation Model",
        display_key: "memories.consolidation_model",
        description: "Model used for global memory consolidation passes.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "memories.consolidation_model",
        },
        kind: CodexSettingKind::String {
            placeholder: "e.g. gpt-5.3-codex",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "memories.max_rollout_age_days",
        label: "Memory Max Rollout Age (Days)",
        display_key: "memories.max_rollout_age_days",
        description: "Maximum age in days of threads considered for memory extraction.",
        category: CodexSettingCategory::ContextAndMemory,
        target: CodexSettingTarget::ConfigToml {
            path: "memories.max_rollout_age_days",
        },
        kind: CodexSettingKind::Integer {
            min: Some(1),
            max: None,
            placeholder: "30",
            default_hint: None,
        },
    },
    // --- Category 6: Agents, Tools & Web -------------------------------------------------
    CodexSettingSpec {
        id: "agents.enabled",
        label: "Multi-Agent Tools Enabled",
        display_key: "agents.enabled",
        description: "Enable spawning and orchestrating subagent threads.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "agents.enabled",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "agents.default_subagent_model",
        label: "Default Subagent Model",
        display_key: "agents.default_subagent_model",
        description: "Default model for spawned subagents when the spawn call omits one.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "agents.default_subagent_model",
        },
        kind: CodexSettingKind::String {
            placeholder: "e.g. gpt-6-luna",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "agents.default_subagent_reasoning_effort",
        label: "Default Subagent Reasoning Effort",
        display_key: "agents.default_subagent_reasoning_effort",
        description: "Default reasoning effort for spawned subagents when not explicitly specified.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "agents.default_subagent_reasoning_effort",
        },
        kind: CodexSettingKind::Choice {
            options: &["none", "minimal", "low", "medium", "high", "xhigh"],
            allow_custom: true,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "agents.max_concurrent_threads_per_session",
        label: "Max Concurrent Agent Threads",
        display_key: "agents.max_concurrent_threads_per_session",
        description: "Maximum number of spawned agent threads open concurrently per session.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "agents.max_concurrent_threads_per_session",
        },
        kind: CodexSettingKind::Integer {
            min: Some(1),
            max: None,
            placeholder: "6",
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "agents.max_depth",
        label: "Max Agent Nesting Depth",
        display_key: "agents.max_depth",
        description: "Maximum nesting depth for V1 subagent threads.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "agents.max_depth",
        },
        kind: CodexSettingKind::Integer {
            min: Some(1),
            max: None,
            placeholder: "2",
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "agents.interrupt_message",
        label: "Record Agent Interrupt Message",
        display_key: "agents.interrupt_message",
        description: "Record a model-visible message whenever an agent turn is interrupted.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "agents.interrupt_message",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "goals.max_goal_token_budget",
        label: "Max Goal Token Budget",
        display_key: "goals.max_goal_token_budget",
        description: "Maximum token budget allowed for a goal and default budget for new goals.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "goals.max_goal_token_budget",
        },
        kind: CodexSettingKind::Integer {
            min: Some(1000),
            max: None,
            placeholder: "500000",
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "web_search",
        label: "Web Search Mode",
        display_key: "web_search",
        description: "Controls the built-in web search tool mode (disabled, cached, indexed, or live).",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml { path: "web_search" },
        kind: CodexSettingKind::Choice {
            options: &["disabled", "cached", "indexed", "live"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "tools.client_search_provider",
        label: "Client Web Search Provider",
        display_key: "tools.client_search_provider",
        description: "Client provider preference when the model does not select a search provider.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "tools.client_search_provider",
        },
        kind: CodexSettingKind::Choice {
            options: &["standalone", "search_mcp"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "tools.web_search.context_size",
        label: "Web Search Context Size",
        display_key: "tools.web_search.context_size",
        description: "Amount of search result context returned per web search call (low, medium, high).",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "tools.web_search.context_size",
        },
        kind: CodexSettingKind::Choice {
            options: &["low", "medium", "high"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "tools.web_search.allowed_domains",
        label: "Web Search Allowed Domains",
        display_key: "tools.web_search.allowed_domains",
        description: "Restrict web search tool results to specific domain names.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "tools.web_search.allowed_domains",
        },
        kind: CodexSettingKind::StringList {
            placeholder: "docs.rs, rust-lang.org, github.com",
        },
    },
    CodexSettingSpec {
        id: "tools.update_plan.enabled",
        label: "Enable update_plan Tool",
        display_key: "tools.update_plan.enabled",
        description: "Expose the built-in update_plan tool to Codex sessions.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "tools.update_plan.enabled",
        },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    CodexSettingSpec {
        id: "tools.experimental_request_user_input.enabled",
        label: "Enable request_user_input Tool",
        display_key: "tools.experimental_request_user_input.enabled",
        description: "Expose the experimental request_user_input tool.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "tools.experimental_request_user_input.enabled",
        },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    CodexSettingSpec {
        id: "skills.bundled.enabled",
        label: "Bundled Skills Enabled",
        display_key: "skills.bundled.enabled",
        description: "Enable Codex's built-in bundled skills.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "skills.bundled.enabled",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "skills.include_instructions",
        label: "Include Skills Instructions Block",
        display_key: "skills.include_instructions",
        description: "Automatically inject available skills instructions into session turns.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "skills.include_instructions",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "skills.max_context_tokens",
        label: "Skills Catalog Max Context Tokens",
        display_key: "skills.max_context_tokens",
        description: "Maximum tokens reserved for the available-skills catalog (capped at 10,000).",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "skills.max_context_tokens",
        },
        kind: CodexSettingKind::Integer {
            min: Some(0),
            max: Some(10_000),
            placeholder: "4000",
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "cloud.skills.enabled",
        label: "Cloud Skills Enabled",
        display_key: "cloud.skills.enabled",
        description: "Permit loading host-supplied cloud skills.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "cloud.skills.enabled",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "code_mode.heartbeat_interval_s",
        label: "Code Mode Heartbeat Interval (s)",
        display_key: "code_mode.heartbeat_interval_s",
        description: "Idle seconds between Code Mode gRPC stream heartbeats (0 disables).",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml {
            path: "code_mode.heartbeat_interval_s",
        },
        kind: CodexSettingKind::Integer {
            min: Some(0),
            max: None,
            placeholder: "0",
            default_hint: Some(0),
        },
    },
    CodexSettingSpec {
        id: "profile",
        label: "Named Config Profile",
        display_key: "profile",
        description: "Profile name selected from the [profiles] table in config.toml.",
        category: CodexSettingCategory::AgentsAndTools,
        target: CodexSettingTarget::ConfigToml { path: "profile" },
        kind: CodexSettingKind::String {
            placeholder: "e.g. automation",
            allow_empty: false,
            default_hint: None,
        },
    },
    // --- Category 7: MCP, Plugins & Hooks ------------------------------------------------
    CodexSettingSpec {
        id: "mcp_servers",
        label: "MCP Servers (JSON)",
        display_key: "mcp_servers",
        description: "MCP server definitions keyed by server name (serialized to TOML inline table).",
        category: CodexSettingCategory::McpPluginsAndHooks,
        target: CodexSettingTarget::ConfigToml {
            path: "mcp_servers",
        },
        kind: CodexSettingKind::Json {
            placeholder: r#"{"docs":{"url":"https://docs.example/mcp"}}"#,
            expect_array: false,
        },
    },
    CodexSettingSpec {
        id: "mcp_optional_startup_grace_ms",
        label: "Optional MCP Startup Grace (ms)",
        display_key: "mcp_optional_startup_grace_ms",
        description: "Milliseconds to wait for optional MCP servers during initial tool catalog startup (0 disables).",
        category: CodexSettingCategory::McpPluginsAndHooks,
        target: CodexSettingTarget::ConfigToml {
            path: "mcp_optional_startup_grace_ms",
        },
        kind: CodexSettingKind::Integer {
            min: Some(0),
            max: None,
            placeholder: "1000",
            default_hint: Some(1000),
        },
    },
    CodexSettingSpec {
        id: "mcp_oauth_credentials_store",
        label: "MCP OAuth Credentials Store",
        display_key: "mcp_oauth_credentials_store",
        description: "Backend used to store MCP OAuth credentials (auto, file, or keyring).",
        category: CodexSettingCategory::McpPluginsAndHooks,
        target: CodexSettingTarget::ConfigToml {
            path: "mcp_oauth_credentials_store",
        },
        kind: CodexSettingKind::Choice {
            options: &["auto", "file", "keyring"],
            allow_custom: false,
            default_hint: Some("auto"),
        },
    },
    CodexSettingSpec {
        id: "mcp_oauth_callback_port",
        label: "MCP OAuth Callback Port",
        display_key: "mcp_oauth_callback_port",
        description: "Fixed local HTTP callback port for MCP OAuth login (ephemeral when unset).",
        category: CodexSettingCategory::McpPluginsAndHooks,
        target: CodexSettingTarget::ConfigToml {
            path: "mcp_oauth_callback_port",
        },
        kind: CodexSettingKind::Integer {
            min: Some(1),
            max: Some(65535),
            placeholder: "8484",
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "mcp_oauth_callback_url",
        label: "MCP OAuth Callback URL",
        display_key: "mcp_oauth_callback_url",
        description: "Custom redirect URI sent in MCP OAuth authorization requests.",
        category: CodexSettingCategory::McpPluginsAndHooks,
        target: CodexSettingTarget::ConfigToml {
            path: "mcp_oauth_callback_url",
        },
        kind: CodexSettingKind::String {
            placeholder: "http://127.0.0.1:8484/callback",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "orchestrator.mcp.enabled",
        label: "Orchestrator MCP Enabled",
        display_key: "orchestrator.mcp.enabled",
        description: "Enable orchestrator-owned MCP integration.",
        category: CodexSettingCategory::McpPluginsAndHooks,
        target: CodexSettingTarget::ConfigToml {
            path: "orchestrator.mcp.enabled",
        },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    CodexSettingSpec {
        id: "plugins._default.enabled",
        label: "Plugins Enabled by Default",
        display_key: "plugins._default.enabled",
        description: "Default activation state for installed Codex plugins.",
        category: CodexSettingCategory::McpPluginsAndHooks,
        target: CodexSettingTarget::ConfigToml {
            path: "plugins._default.enabled",
        },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    CodexSettingSpec {
        id: "marketplaces",
        label: "Plugin Marketplaces (JSON)",
        display_key: "marketplaces",
        description: "User-level plugin marketplace entries keyed by marketplace name.",
        category: CodexSettingCategory::McpPluginsAndHooks,
        target: CodexSettingTarget::ConfigToml {
            path: "marketplaces",
        },
        kind: CodexSettingKind::Json {
            placeholder: r#"{"internal":{"source_type":"git","source":"https://git.example/plugins.git"}}"#,
            expect_array: false,
        },
    },
    CodexSettingSpec {
        id: "hooks",
        label: "Lifecycle Hooks (JSON)",
        display_key: "hooks",
        description: "Inline lifecycle hook configuration (SessionStart, PreToolUse, PostToolUse, Stop, etc.).",
        category: CodexSettingCategory::McpPluginsAndHooks,
        target: CodexSettingTarget::ConfigToml { path: "hooks" },
        kind: CodexSettingKind::Json {
            placeholder: r#"{"Stop":[]}"#,
            expect_array: false,
        },
    },
    // --- Category 8: Auth, Storage & Telemetry -------------------------------------------
    CodexSettingSpec {
        id: "cli_auth_credentials_store",
        label: "CLI Auth Credentials Store",
        display_key: "cli_auth_credentials_store",
        description: "Backend for storing Codex CLI authentication credentials (file, keyring, auto, ephemeral).",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "cli_auth_credentials_store",
        },
        kind: CodexSettingKind::Choice {
            options: &["file", "keyring", "auto", "ephemeral"],
            allow_custom: false,
            default_hint: Some("file"),
        },
    },
    CodexSettingSpec {
        id: "forced_login_method",
        label: "Forced Login Method",
        display_key: "forced_login_method",
        description: "Restrict authentication to either ChatGPT login or OpenAI API key.",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "forced_login_method",
        },
        kind: CodexSettingKind::Choice {
            options: &["chatgpt", "api"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "forced_chatgpt_workspace_id",
        label: "Forced ChatGPT Workspace IDs",
        display_key: "forced_chatgpt_workspace_id",
        description: "Restrict ChatGPT login to one or more specific workspace IDs.",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "forced_chatgpt_workspace_id",
        },
        kind: CodexSettingKind::StringList {
            placeholder: "ws-11111111, ws-22222222",
        },
    },
    CodexSettingSpec {
        id: "sqlite_home",
        label: "SQLite State Directory",
        display_key: "sqlite_home",
        description: "Absolute directory where Codex stores its SQLite state DB (overrides CODEX_SQLITE_HOME).",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "sqlite_home",
        },
        kind: CodexSettingKind::String {
            placeholder: "/path/to/codex-sqlite",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "log_dir",
        label: "Log Directory",
        display_key: "log_dir",
        description: "Directory for Codex log files and SQLite logs database.",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml { path: "log_dir" },
        kind: CodexSettingKind::String {
            placeholder: "/tmp/codex-logs",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "thread_unload_delay_secs",
        label: "Thread Unload Delay (s)",
        display_key: "thread_unload_delay_secs",
        description: "Idle seconds before app-server unloads an inactive thread with no subscribers (0 unloads immediately).",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "thread_unload_delay_secs",
        },
        kind: CodexSettingKind::Integer {
            min: Some(0),
            max: None,
            placeholder: "60",
            default_hint: Some(60),
        },
    },
    CodexSettingSpec {
        id: "analytics.enabled",
        label: "OpenAI Analytics Enabled",
        display_key: "analytics.enabled",
        description: "Enable or disable OpenAI product analytics (independent of custom OTLP exporters).",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "analytics.enabled",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "feedback.enabled",
        label: "Feedback Collection Enabled",
        display_key: "feedback.enabled",
        description: "Enable or disable feedback uploads and background Guardian review retention.",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "feedback.enabled",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "otel.environment",
        label: "OTel Environment Tag",
        display_key: "otel.environment",
        description: "Environment label attached to exported OpenTelemetry traces (e.g. dev, staging, prod).",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "otel.environment",
        },
        kind: CodexSettingKind::String {
            placeholder: "dev",
            allow_empty: false,
            default_hint: Some("dev"),
        },
    },
    CodexSettingSpec {
        id: "otel.exporter",
        label: "OTel Log Exporter",
        display_key: "otel.exporter",
        description: "OpenTelemetry log exporter mode (`none` or `statsig`). Use raw -c overrides for custom OTLP tables.",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "otel.exporter",
        },
        kind: CodexSettingKind::Choice {
            options: &["none", "statsig"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "otel.metrics_exporter",
        label: "OTel Metrics Exporter",
        display_key: "otel.metrics_exporter",
        description: "OpenTelemetry metrics exporter (`none` or `statsig`).",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "otel.metrics_exporter",
        },
        kind: CodexSettingKind::Choice {
            options: &["none", "statsig"],
            allow_custom: false,
            default_hint: Some("statsig"),
        },
    },
    CodexSettingSpec {
        id: "otel.trace_exporter",
        label: "OTel Trace Exporter",
        display_key: "otel.trace_exporter",
        description: "OpenTelemetry trace exporter (`none` or `statsig`).",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "otel.trace_exporter",
        },
        kind: CodexSettingKind::Choice {
            options: &["none", "statsig"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "otel.log_user_prompt",
        label: "OTel Log User Prompts",
        display_key: "otel.log_user_prompt",
        description: "Include user prompt text in exported OpenTelemetry traces.",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "otel.log_user_prompt",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "otel.log_agent_responses",
        label: "OTel Log Agent Responses",
        display_key: "otel.log_agent_responses",
        description: "Export final main-agent and subagent responses to OTLP log exporters.",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "otel.log_agent_responses",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "otel.log_sse_deltas",
        label: "OTel Log SSE Deltas",
        display_key: "otel.log_sse_deltas",
        description: "Log successful SSE `.delta` events in OTLP logs.",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "otel.log_sse_deltas",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "otel.tool_result.max_bytes",
        label: "OTel Tool Result Max Bytes",
        display_key: "otel.tool_result.max_bytes",
        description: "Maximum UTF-8 bytes of tool output logged to OpenTelemetry before truncation.",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "otel.tool_result.max_bytes",
        },
        kind: CodexSettingKind::Integer {
            min: Some(0),
            max: None,
            placeholder: "2048",
            default_hint: Some(2048),
        },
    },
    CodexSettingSpec {
        id: "otel.span_attributes",
        label: "OTel Span Attributes (JSON)",
        display_key: "otel.span_attributes",
        description: "Key-value string attributes added to every exported OpenTelemetry trace span.",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "otel.span_attributes",
        },
        kind: CodexSettingKind::Json {
            placeholder: r#"{"service.team":"core","runner":"bruk"}"#,
            expect_array: false,
        },
    },
    CodexSettingSpec {
        id: "rollout_trace.max_bytes",
        label: "Rollout Trace Max Bytes",
        display_key: "rollout_trace.max_bytes",
        description: "Maximum bytes retained for local rollout trace manifests, logs, and shards.",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "rollout_trace.max_bytes",
        },
        kind: CodexSettingKind::Integer {
            min: Some(1024),
            max: None,
            placeholder: "268435456",
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "suppress_unstable_features_warning",
        label: "Suppress Unstable Features Warning",
        display_key: "suppress_unstable_features_warning",
        description: "Silence startup warnings when under-development feature flags are enabled.",
        category: CodexSettingCategory::AuthAndTelemetry,
        target: CodexSettingTarget::ConfigToml {
            path: "suppress_unstable_features_warning",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    // --- Category 9: Feature Flags (`[features]`) ----------------------------------------
    CodexSettingSpec {
        id: "features.fast_mode",
        label: "Fast Mode",
        display_key: "features.fast_mode",
        description: "Enable Codex fast inference mode feature.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.fast_mode",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.ultrafast_mode",
        label: "Ultrafast Mode",
        display_key: "features.ultrafast_mode",
        description: "Enable Codex ultrafast mode feature.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.ultrafast_mode",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.memories",
        label: "Memories Feature",
        display_key: "features.memories",
        description: "Enable cross-session memory extraction and recall.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.memories",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "features.multi_agent_v2",
        label: "Multi-Agent V2",
        display_key: "features.multi_agent_v2",
        description: "Enable the V2 multi-agent orchestration subsystem.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.multi_agent_v2",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "features.hooks",
        label: "Lifecycle Hooks Feature",
        display_key: "features.hooks",
        description: "Enable execution of configured Codex lifecycle hooks.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.hooks",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.plugins",
        label: "Plugins Feature",
        display_key: "features.plugins",
        description: "Enable Codex plugin discovery and loading.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.plugins",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.apps",
        label: "Codex Apps Feature",
        display_key: "features.apps",
        description: "Enable Codex Apps connector integration.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.apps",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.browser_use",
        label: "Browser Use Feature",
        display_key: "features.browser_use",
        description: "Enable built-in browser automation tools.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.browser_use",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.computer_use",
        label: "Computer Use Feature",
        display_key: "features.computer_use",
        description: "Enable built-in desktop computer-use tools.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.computer_use",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.shell_tool",
        label: "Shell Tool",
        display_key: "features.shell_tool",
        description: "Enable the default shell command execution tool.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.shell_tool",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.shell_snapshot",
        label: "Shell Environment Snapshot",
        display_key: "features.shell_snapshot",
        description: "Snapshot shell environment state to accelerate repeated command execution.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.shell_snapshot",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.unified_exec",
        label: "Unified Exec",
        display_key: "features.unified_exec",
        description: "Use the unified exec runner for shell and background command sessions.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.unified_exec",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.unified_exec_tty",
        label: "Unified Exec PTY Support",
        display_key: "features.unified_exec_tty",
        description: "Allow unified exec sessions to allocate interactive PTYs.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.unified_exec_tty",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.guardian_approval",
        label: "Guardian Auto-Approval",
        display_key: "features.guardian_approval",
        description: "Enable the Guardian approval subagent subsystem.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.guardian_approval",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.daemon_auto_start",
        label: "Daemon Auto-Start",
        display_key: "features.daemon_auto_start",
        description: "Automatically start the shared background app-server daemon on launch.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.daemon_auto_start",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.view_image",
        label: "View Image Tool",
        display_key: "features.view_image",
        description: "Enable local image inspection tool for multimodal prompts.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.view_image",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.image_generation",
        label: "Image Generation Tool",
        display_key: "features.image_generation",
        description: "Enable built-in image generation tool.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.image_generation",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.sleep_tool",
        label: "Sleep Tool",
        display_key: "features.sleep_tool",
        description: "Expose the built-in sleep tool for waiting on external jobs.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.sleep_tool",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.goals",
        label: "Goals Feature",
        display_key: "features.goals",
        description: "Enable long-running goal tracking subsystem.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.goals",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.enable_request_compression",
        label: "API Request Compression",
        display_key: "features.enable_request_compression",
        description: "Compress outbound API request payloads.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.enable_request_compression",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.network_proxy",
        label: "Managed Network Proxy (Experimental)",
        display_key: "features.network_proxy",
        description: "Enable experimental Codex-managed network proxy for sandboxed commands.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.network_proxy",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "features.respect_system_proxy",
        label: "Respect System Proxy (Experimental)",
        display_key: "features.respect_system_proxy",
        description: "Honor OS-configured system HTTP/HTTPS proxy settings.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.respect_system_proxy",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "features.prevent_idle_sleep",
        label: "Prevent Idle Sleep (Experimental)",
        display_key: "features.prevent_idle_sleep",
        description: "Hold an OS sleep assertion while Codex turns are actively running.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.prevent_idle_sleep",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "features.secret_auth_storage",
        label: "Secret Auth Storage",
        display_key: "features.secret_auth_storage",
        description: "Store auth credentials using OS secret storage backend.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.secret_auth_storage",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "features.skill_search",
        label: "Skill Search",
        display_key: "features.skill_search",
        description: "Enable dynamic skill search across installed and discoverable skills.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.skill_search",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.tool_suggest",
        label: "Tool Suggestions",
        display_key: "features.tool_suggest",
        description: "Suggest discoverable tools/integrations relevant to the current task.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.tool_suggest",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.unbounded_connection_retries",
        label: "Unbounded Connection Retries",
        display_key: "features.unbounded_connection_retries",
        description: "Keep retrying transient network/stream disconnections during long turns.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.unbounded_connection_retries",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "features.suppress_model_change_announcements",
        label: "Suppress Model Change Announcements",
        display_key: "features.suppress_model_change_announcements",
        description: "Suppress automatic system notices when the active model changes mid-session.",
        category: CodexSettingCategory::Features,
        target: CodexSettingTarget::ConfigToml {
            path: "features.suppress_model_change_announcements",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    // --- Category 10: Interface & TUI ----------------------------------------------------
    CodexSettingSpec {
        id: "file_opener",
        label: "URI File Opener",
        display_key: "file_opener",
        description: "Editor URI scheme used to hyperlink file citations in model output.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "file_opener",
        },
        kind: CodexSettingKind::Choice {
            options: &["vscode", "vscode-insiders", "windsurf", "cursor", "none"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "notify",
        label: "External Notification Command",
        display_key: "notify",
        description: "Command and arguments spawned for end-user turn notifications (comma-separated argv).",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml { path: "notify" },
        kind: CodexSettingKind::StringList {
            placeholder: "/usr/local/bin/notify-send, Codex",
        },
    },
    CodexSettingSpec {
        id: "tui.theme",
        label: "TUI Syntax Theme",
        display_key: "tui.theme",
        description: "Kebab-case syntax highlighting theme name overriding automatic light/dark detection.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml { path: "tui.theme" },
        kind: CodexSettingKind::String {
            placeholder: "e.g. catppuccin-mocha or github-light",
            allow_empty: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "tui.alternate_screen",
        label: "TUI Alternate Screen",
        display_key: "tui.alternate_screen",
        description: "Control whether the interactive TUI uses the terminal alternate screen buffer.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.alternate_screen",
        },
        kind: CodexSettingKind::Choice {
            options: &["auto", "always", "never"],
            allow_custom: false,
            default_hint: Some("auto"),
        },
    },
    CodexSettingSpec {
        id: "tui.animations",
        label: "TUI Animations",
        display_key: "tui.animations",
        description: "Enable welcome screen animations, shimmer effects, and loading spinners.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.animations",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "tui.fullscreen_transcript",
        label: "Fullscreen Transcript",
        display_key: "tui.fullscreen_transcript",
        description: "Let Codex own fullscreen transcript scrolling, selection, and search.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.fullscreen_transcript",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "tui.collapse_turn_details",
        label: "Collapse Turn Details",
        display_key: "tui.collapse_turn_details",
        description: "Summarize tool calls and collapse completed turns in the fullscreen transcript.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.collapse_turn_details",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "tui.raw_output_mode",
        label: "Raw Output Mode",
        display_key: "tui.raw_output_mode",
        description: "Start the TUI in raw scrollback mode for copy-friendly plain transcript output.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.raw_output_mode",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "tui.vim_mode_default",
        label: "Vim Mode Default",
        display_key: "tui.vim_mode_default",
        description: "Start the interactive composer in Vim Normal mode by default.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.vim_mode_default",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "tui.auto_recap",
        label: "Automatic Unfocused Recap",
        display_key: "tui.auto_recap",
        description: "Generate automatic conversation recaps when the terminal is unfocused.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.auto_recap",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "tui.notifications",
        label: "TUI Desktop Notifications",
        display_key: "tui.notifications",
        description: "Enable terminal desktop notifications from the TUI.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.notifications",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "tui.notification_condition",
        label: "Notification Condition",
        display_key: "tui.notification_condition",
        description: "Deliver terminal notifications only when unfocused or always.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.notification_condition",
        },
        kind: CodexSettingKind::Choice {
            options: &["unfocused", "always"],
            allow_custom: false,
            default_hint: Some("unfocused"),
        },
    },
    CodexSettingSpec {
        id: "tui.notification_method",
        label: "Notification Escape Method",
        display_key: "tui.notification_method",
        description: "Terminal notification protocol (auto, osc9, osc777, bel).",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.notification_method",
        },
        kind: CodexSettingKind::Choice {
            options: &["auto", "osc9", "osc777", "bel"],
            allow_custom: false,
            default_hint: Some("auto"),
        },
    },
    CodexSettingSpec {
        id: "tui.copy_on_select",
        label: "Copy on Select",
        display_key: "tui.copy_on_select",
        description: "Copy selected TUI text automatically on mouse release (auto, always, never).",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.copy_on_select",
        },
        kind: CodexSettingKind::Choice {
            options: &["auto", "always", "never"],
            allow_custom: false,
            default_hint: Some("auto"),
        },
    },
    CodexSettingSpec {
        id: "tui.disable_paste_burst",
        label: "Disable Paste Burst Detection",
        display_key: "tui.disable_paste_burst",
        description: "Insert typed characters immediately without burst-paste buffering.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.disable_paste_burst",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "tui.mouse_scroll_speed",
        label: "Mouse Scroll Speed",
        display_key: "tui.mouse_scroll_speed",
        description: "Mouse wheel scroll speed multiplier for transcript scrolling (default 1.0).",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.mouse_scroll_speed",
        },
        kind: CodexSettingKind::Number {
            min: Some(0.1),
            max: Some(20.0),
            placeholder: "1.0",
            default_hint: Some("1.0"),
        },
    },
    CodexSettingSpec {
        id: "tui.show_tooltips",
        label: "Show Startup Tooltips",
        display_key: "tui.show_tooltips",
        description: "Display tips on the TUI welcome screen.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.show_tooltips",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "tui.show_server_version_notice",
        label: "Show Server Version Notice",
        display_key: "tui.show_server_version_notice",
        description: "Display informational notices when app-server version differs from CLI.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.show_server_version_notice",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "tui.session_picker_view",
        label: "Session Picker Density",
        display_key: "tui.session_picker_view",
        description: "Layout density for resume/fork session picker lists.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.session_picker_view",
        },
        kind: CodexSettingKind::Choice {
            options: &["comfortable", "dense"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "tui.resume_cwd",
        label: "Resume Working Directory",
        display_key: "tui.resume_cwd",
        description: "Working directory selected when resuming or forking a session (current or session).",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.resume_cwd",
        },
        kind: CodexSettingKind::Choice {
            options: &["current", "session"],
            allow_custom: false,
            default_hint: None,
        },
    },
    CodexSettingSpec {
        id: "tui.status_line",
        label: "TUI Status Line Items",
        display_key: "tui.status_line",
        description: "Ordered list of status line item identifiers displayed in the TUI footer.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.status_line",
        },
        kind: CodexSettingKind::StringList {
            placeholder: "model, context, git-branch",
        },
    },
    CodexSettingSpec {
        id: "tui.status_line_use_colors",
        label: "Colorize TUI Status Line",
        display_key: "tui.status_line_use_colors",
        description: "Style status line items with colors from the active syntax theme.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.status_line_use_colors",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    CodexSettingSpec {
        id: "tui.rendering.tables",
        label: "Rich Markdown Tables",
        display_key: "tui.rendering.tables",
        description: "Render Markdown pipe tables with box-drawing characters in the TUI.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.rendering.tables",
        },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    CodexSettingSpec {
        id: "tui.rendering.mermaid",
        label: "Rich Mermaid Diagrams",
        display_key: "tui.rendering.mermaid",
        description: "Render Mermaid code blocks as diagrams in the TUI.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "tui.rendering.mermaid",
        },
        kind: CodexSettingKind::Bool { default_hint: None },
    },
    CodexSettingSpec {
        id: "notice.hide_full_access_warning",
        label: "Hide Full Access Warning",
        display_key: "notice.hide_full_access_warning",
        description: "Suppress the startup warning prompt when running in full-access mode.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "notice.hide_full_access_warning",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    CodexSettingSpec {
        id: "notice.hide_rate_limit_model_nudge",
        label: "Hide Rate-Limit Model Nudge",
        display_key: "notice.hide_rate_limit_model_nudge",
        description: "Opt out of automatic rate-limit model switch reminders.",
        category: CodexSettingCategory::InterfaceAndTui,
        target: CodexSettingTarget::ConfigToml {
            path: "notice.hide_rate_limit_model_nudge",
        },
        kind: CodexSettingKind::Bool {
            default_hint: Some(false),
        },
    },
];

/// Curated presets for well-known Codex CLI environment variables.
pub const CODEX_ENV_PRESETS: &[CodexEnvPreset] = &[
    CodexEnvPreset {
        name: "CODEX_HOME",
        description: "Root directory for Codex config.toml, auth credentials, sessions, and logs (default ~/.codex).",
        placeholder: "/path/to/.codex",
        suggested_value: "",
    },
    CodexEnvPreset {
        name: "CODEX_SQLITE_HOME",
        description: "Directory for Codex SQLite state database (default $CODEX_HOME).",
        placeholder: "/path/to/codex-sqlite",
        suggested_value: "",
    },
    CodexEnvPreset {
        name: "OPENAI_API_KEY",
        description: "OpenAI API key used when authenticating via API billing instead of ChatGPT login.",
        placeholder: "sk-...",
        suggested_value: "",
    },
    CodexEnvPreset {
        name: "OPENAI_BASE_URL",
        description: "Override the default OpenAI API endpoint URL for proxies or gateways.",
        placeholder: "https://api.openai.com/v1",
        suggested_value: "",
    },
    CodexEnvPreset {
        name: "CODEX_ACCESS_TOKEN",
        description: "Bearer access token for automated Codex authentication.",
        placeholder: "token",
        suggested_value: "",
    },
    CodexEnvPreset {
        name: "RUST_LOG",
        description: "Tracing log filter for debugging Codex CLI internals (e.g. codex_core=debug).",
        placeholder: "codex_core=info",
        suggested_value: "codex_core=info",
    },
    CodexEnvPreset {
        name: "NO_COLOR",
        description: "Disable ANSI color output across Codex CLI and child tools (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    CodexEnvPreset {
        name: "HTTP_PROXY",
        description: "HTTP proxy URL for outbound Codex requests.",
        placeholder: "http://127.0.0.1:8080",
        suggested_value: "",
    },
    CodexEnvPreset {
        name: "HTTPS_PROXY",
        description: "HTTPS proxy URL for outbound Codex requests.",
        placeholder: "http://127.0.0.1:8080",
        suggested_value: "",
    },
    CodexEnvPreset {
        name: "NO_PROXY",
        description: "Comma-separated hostnames/CIDRs bypassing HTTP(S)_PROXY.",
        placeholder: "localhost,127.0.0.1",
        suggested_value: "localhost,127.0.0.1",
    },
    CodexEnvPreset {
        name: "SSL_CERT_FILE",
        description: "Path to a custom CA certificate bundle for enterprise TLS proxies.",
        placeholder: "/etc/ssl/certs/corp-ca.pem",
        suggested_value: "",
    },
    CodexEnvPreset {
        name: "OTEL_EXPORTER_OTLP_ENDPOINT",
        description: "OTLP collector endpoint URL for OpenTelemetry export.",
        placeholder: "http://localhost:4317",
        suggested_value: "",
    },
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn catalog_has_unique_ids_and_covers_all_categories() {
        let mut seen = HashSet::new();
        for spec in CODEX_SETTING_SPECS {
            assert!(
                seen.insert(spec.id),
                "duplicate CodexSettingSpec id: {}",
                spec.id
            );
            assert!(!spec.label.is_empty());
            assert!(!spec.display_key.is_empty());
            assert!(!spec.description.is_empty());
            assert_ne!(
                spec.id, CODEX_DISABLE_UPDATE_CHECK_KEY,
                "check_for_update_on_startup is always enforced by Bruk and must not be user-overridable"
            );
        }

        for category in CodexSettingCategory::ALL {
            if *category == CodexSettingCategory::EnvironmentAndRaw {
                assert!(!CODEX_ENV_PRESETS.is_empty());
            } else {
                assert!(
                    !codex_setting_specs_for_category(*category).is_empty(),
                    "category {:?} should not be empty",
                    category
                );
            }
        }
    }

    #[test]
    fn builds_toml_config_overrides_and_merges_extra_overrides() {
        let mut settings = CodexSpawnSettings::default();
        settings
            .set_value(
                "model_reasoning_effort",
                CodexSettingValue::String("high".to_owned()),
            )
            .unwrap();
        settings
            .set_value(
                "sandbox_workspace_write.network_access",
                CodexSettingValue::Bool(true),
            )
            .unwrap();
        settings
            .set_from_text(
                "sandbox_workspace_write.writable_roots",
                "/tmp/cache, /opt/tools",
            )
            .unwrap();
        settings
            .set_from_text("otel.span_attributes", r#"{"runner":"bruk","tier":2}"#)
            .unwrap();
        settings
            .set_from_text("auto_review.timeout_ms", "45000")
            .unwrap();
        settings
            .set_extra_config_overrides(
                "# comment line\nexperimental.foo = true\nmodel_reasoning_summary = \"detailed\"",
            )
            .unwrap();

        let pairs = settings.build_user_config_overrides().unwrap();
        let map: BTreeMap<String, String> = pairs.into_iter().collect();
        assert_eq!(
            map.get("model_reasoning_effort").map(String::as_str),
            Some("\"high\"")
        );
        assert_eq!(
            map.get("sandbox_workspace_write.network_access")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            map.get("sandbox_workspace_write.writable_roots")
                .map(String::as_str),
            Some("[\"/tmp/cache\", \"/opt/tools\"]")
        );
        assert_eq!(
            map.get("otel.span_attributes").map(String::as_str),
            Some("{runner = \"bruk\", tier = 2}")
        );
        assert_eq!(
            map.get("auto_review.timeout_ms").map(String::as_str),
            Some("45000")
        );
        assert_eq!(
            map.get("experimental.foo").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            map.get("model_reasoning_summary").map(String::as_str),
            Some("\"detailed\"")
        );
    }

    #[test]
    fn validates_numbers_integers_json_env_vars_extra_args_and_extra_config() {
        let mut settings = CodexSpawnSettings::default();

        assert!(
            settings
                .set_from_text("mcp_oauth_callback_port", "70000")
                .is_err()
        );
        assert!(
            settings
                .set_from_text("mcp_oauth_callback_port", "8484")
                .is_ok()
        );

        assert!(
            settings
                .set_from_text("tui.mouse_scroll_speed", "50.0")
                .is_err()
        );
        assert!(
            settings
                .set_from_text("tui.mouse_scroll_speed", "1.5")
                .is_ok()
        );

        assert!(settings.set_from_text("sandbox_mode", "invalid").is_err());
        assert!(
            settings
                .set_from_text("sandbox_mode", "workspace-write")
                .is_ok()
        );

        // Reasoning effort allows custom model-advertised levels.
        assert!(
            settings
                .set_from_text("model_reasoning_effort", "max")
                .is_ok()
        );

        // JSON object validation + TOML null rejection.
        assert!(settings.set_from_text("hooks", "[1, 2]").is_err());
        assert!(
            settings
                .set_from_text("otel.span_attributes", r#"{"bad":null}"#)
                .is_err()
        );
        assert!(settings.set_from_text("hooks", r#"{"Stop":[]}"#).is_ok());

        // Env var validation.
        assert!(settings.set_env_var("", "1").is_err());
        assert!(settings.set_env_var("123BAD", "1").is_err());
        assert!(settings.set_env_var("BAD-NAME", "1").is_err());
        assert!(settings.set_env_var("CODEX_HOME", "/tmp/codex").is_ok());

        // Extra CLI args shell quoting validation.
        assert!(settings.set_extra_cli_args("--profile \"unclosed").is_err());
        assert!(
            settings
                .set_extra_cli_args("--profile 'team profile' --ephemeral")
                .is_ok()
        );

        // Extra -c config overrides validation.
        assert!(
            settings
                .set_extra_config_overrides("missing_equals")
                .is_err()
        );
        assert!(
            settings
                .set_extra_config_overrides("bad..key = true")
                .is_err()
        );
        assert!(
            settings
                .set_extra_config_overrides("check_for_update_on_startup = true")
                .is_err()
        );
        assert!(
            settings
                .set_extra_config_overrides("custom_flag = true\nnested.key = \"val\"")
                .is_ok()
        );
    }

    #[test]
    fn builds_spawn_plan_with_flags_config_overrides_env_and_exec_mode() {
        let mut settings = CodexSpawnSettings::default();
        settings
            .set_value(
                "cli.sandbox",
                CodexSettingValue::String("workspace-write".to_owned()),
            )
            .unwrap();
        settings
            .set_value("cli.worktree", CodexSettingValue::Bool(true))
            .unwrap();
        settings
            .set_from_text("cli.add-dir", "/shared/a, /shared/b")
            .unwrap();
        settings
            .set_value(
                "model_reasoning_effort",
                CodexSettingValue::String("xhigh".to_owned()),
            )
            .unwrap();
        settings
            .set_value("features.memories", CodexSettingValue::Bool(true))
            .unwrap();
        settings.set_env_var("NO_COLOR", "1").unwrap();
        settings
            .set_extra_cli_args("--extra-opt 'two words'")
            .unwrap();

        let plan = build_codex_spawn_plan(
            "/usr/local/bin/codex",
            Some("gpt-6.2-sol"),
            &settings,
            &CodexSpawnOptions {
                working_dir: Some(PathBuf::from("/workspace/repo")),
                model_override: None,
                exec_mode: true,
                json_events: true,
                prompt: Some("Run tests".to_owned()),
            },
        )
        .unwrap();

        assert_eq!(plan.binary_path, PathBuf::from("/usr/local/bin/codex"));
        assert_eq!(plan.env.get("NO_COLOR").map(String::as_str), Some("1"));
        assert_eq!(plan.args.first().map(String::as_str), Some("exec"));
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["-c", "check_for_update_on_startup=false"])
        );
        assert!(plan.args.contains(&"--json".to_owned()));
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--model", "gpt-6.2-sol"])
        );
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["-c", "model_reasoning_effort=\"xhigh\""])
        );
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["-c", "features.memories=true"])
        );
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--sandbox", "workspace-write"])
        );
        assert!(plan.args.contains(&"--worktree".to_owned()));
        assert!(
            plan.args
                .windows(4)
                .any(|w| w == ["--add-dir", "/shared/a", "--add-dir", "/shared/b"])
        );
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--extra-opt", "two words"])
        );
        assert_eq!(plan.args.last().map(String::as_str), Some("Run tests"));

        let preview = plan.preview_command_line();
        assert!(preview.starts_with(
            "NO_COLOR=1 /usr/local/bin/codex exec -c check_for_update_on_startup=false"
        ));
        assert!(preview.contains("'two words'"));
        assert!(plan.config_overrides_pretty().is_some());
    }

    #[test]
    fn always_forces_disable_update_check_even_with_defaults_or_override_attempts() {
        let mut settings = CodexSpawnSettings::default();
        assert!(matches!(
            settings.set_extra_config_overrides("check_for_update_on_startup = true"),
            Err(CodexSettingsError::ReservedConfigKey(_))
        ));

        let default_plan = build_codex_spawn_plan(
            "/usr/local/bin/codex",
            None,
            &settings,
            &CodexSpawnOptions::default(),
        )
        .unwrap();
        assert!(
            default_plan
                .args
                .windows(2)
                .any(|w| w == ["-c", "check_for_update_on_startup=false"])
        );
        assert!(default_plan.config_overrides_pretty().is_none());

        // Even if hand-edited storage injects check_for_update_on_startup = true, Bruk strips it
        // and still emits check_for_update_on_startup=false.
        settings.extra_config_overrides =
            "check_for_update_on_startup = true\ncustom_flag = 1".to_owned();
        let forced_plan = build_codex_spawn_plan(
            "/usr/local/bin/codex",
            None,
            &settings,
            &CodexSpawnOptions::default(),
        )
        .unwrap();
        assert!(
            forced_plan
                .args
                .windows(2)
                .any(|w| w == ["-c", "check_for_update_on_startup=false"])
        );
        assert!(
            !forced_plan
                .args
                .iter()
                .any(|a| a.contains("check_for_update_on_startup=true"))
        );
        assert_eq!(
            forced_plan.config_overrides,
            vec![("custom_flag".to_owned(), "1".to_owned())]
        );
    }

    #[cfg(unix)]
    #[test]
    fn spawned_command_delivers_flags_config_overrides_and_env_to_child_process() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let fake_codex = temp.path().join("codex");
        std::fs::write(
            &fake_codex,
            "#!/bin/sh\necho \"ENV=$NO_COLOR\"\nfor a in \"$@\"; do echo \"ARG=$a\"; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_codex, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut settings = CodexSpawnSettings::default();
        settings
            .set_value(
                "model_reasoning_effort",
                CodexSettingValue::String("high".to_owned()),
            )
            .unwrap();
        settings
            .set_value("cli.worktree", CodexSettingValue::Bool(true))
            .unwrap();
        settings.set_env_var("NO_COLOR", "1").unwrap();

        let output = build_codex_command(
            &fake_codex,
            Some("gpt-6.2-sol"),
            &settings,
            &CodexSpawnOptions {
                exec_mode: true,
                prompt: Some("hello".to_owned()),
                ..Default::default()
            },
        )
        .unwrap()
        .output()
        .unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("ENV=1\n"));
        assert!(stdout.contains("ARG=exec\n"));
        assert!(stdout.contains("ARG=-c\nARG=check_for_update_on_startup=false\n"));
        assert!(stdout.contains("ARG=--model\nARG=gpt-6.2-sol\n"));
        assert!(stdout.contains("ARG=-c\nARG=model_reasoning_effort=\"high\"\n"));
        assert!(stdout.contains("ARG=--worktree\n"));
        assert!(stdout.contains("ARG=hello\n"));
    }
}
