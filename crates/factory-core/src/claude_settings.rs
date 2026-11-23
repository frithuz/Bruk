//! Complete catalog, persistence model, validation, and command builder for custom
//! Claude Code spawn settings.
//!
//! Every user-configurable setting discovered from `claude --help` (CLI launch flags),
//! the official Claude Code `settings.json` schema (passed via `--settings <json>`), and
//! Claude Code environment variables is modeled here as an optional override over the
//! user's existing Claude CLI defaults. Unset options are omitted so Bruk never clobbers
//! `~/.claude/settings.json` unless explicitly overridden in Preferences.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Environment variable always forced to `"1"` on every spawned `claude` process so a run
/// cannot silently auto-update the CLI binary out from under subsequent runs.
pub const CLAUDE_DISABLE_AUTOUPDATER_ENV: &str = "DISABLE_AUTOUPDATER";

/// Value always assigned to [`CLAUDE_DISABLE_AUTOUPDATER_ENV`] when spawning `claude`.
pub const CLAUDE_DISABLE_AUTOUPDATER_VALUE: &str = "1";

/// Applies mandatory Bruk runtime environment guards to any `claude` [`Command`].
///
/// Always sets `DISABLE_AUTOUPDATER=1` so background auto-updates cannot change the installed
/// Claude CLI version mid-flight or between runs.
pub fn apply_mandatory_claude_spawn_env(cmd: &mut Command) {
    cmd.env(
        CLAUDE_DISABLE_AUTOUPDATER_ENV,
        CLAUDE_DISABLE_AUTOUPDATER_VALUE,
    );
}

/// Creates a new [`Command`] for spawning the `claude` binary with mandatory Bruk spawn guards
/// (such as `DISABLE_AUTOUPDATER=1`) pre-applied.
pub fn new_claude_command(binary_path: impl AsRef<Path>) -> Command {
    let mut cmd = Command::new(binary_path.as_ref());
    apply_mandatory_claude_spawn_env(&mut cmd);
    cmd
}

/// Logical grouping for Claude spawn settings displayed in the Preferences UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeSettingCategory {
    CliFlags,
    ModelsAndReasoning,
    Permissions,
    Sandbox,
    ContextAndGit,
    AgentsAndWorkflows,
    McpPluginsAndHooks,
    AuthAndCloud,
    InterfaceAndSession,
    EnvironmentAndRaw,
}

impl ClaudeSettingCategory {
    pub const ALL: &'static [Self] = &[
        Self::CliFlags,
        Self::ModelsAndReasoning,
        Self::Permissions,
        Self::Sandbox,
        Self::ContextAndGit,
        Self::AgentsAndWorkflows,
        Self::McpPluginsAndHooks,
        Self::AuthAndCloud,
        Self::InterfaceAndSession,
        Self::EnvironmentAndRaw,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::CliFlags => "Launch & CLI",
            Self::ModelsAndReasoning => "Models & Reasoning",
            Self::Permissions => "Permissions",
            Self::Sandbox => "Sandbox & Security",
            Self::ContextAndGit => "Context & Git",
            Self::AgentsAndWorkflows => "Agents & Skills",
            Self::McpPluginsAndHooks => "MCP, Plugins & Hooks",
            Self::AuthAndCloud => "Auth & Cloud",
            Self::InterfaceAndSession => "Interface & Session",
            Self::EnvironmentAndRaw => "Environment & Raw",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::CliFlags => "cli-flags",
            Self::ModelsAndReasoning => "models",
            Self::Permissions => "permissions",
            Self::Sandbox => "sandbox",
            Self::ContextAndGit => "context-git",
            Self::AgentsAndWorkflows => "agents-skills",
            Self::McpPluginsAndHooks => "mcp-plugins-hooks",
            Self::AuthAndCloud => "auth-cloud",
            Self::InterfaceAndSession => "interface-session",
            Self::EnvironmentAndRaw => "env-raw",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::CliFlags => {
                "Command-line flags passed directly to the claude binary on every spawn."
            }
            Self::ModelsAndReasoning => {
                "Reasoning effort, extended thinking, fast mode, advisor, and fallback model chain."
            }
            Self::Permissions => {
                "Default permission mode, allow/ask/deny rules, and auto mode classifier tuning."
            }
            Self::Sandbox => {
                "Sandboxed bash execution, network domain filters, filesystem rules, and credential isolation."
            }
            Self::ContextAndGit => {
                "Auto-compaction, auto-memory, CLAUDE.md exclusions, git attribution, and worktree behavior."
            }
            Self::AgentsAndWorkflows => {
                "Main-thread agent, agent teams, dynamic workflows, skill budgets, and artifacts."
            }
            Self::McpPluginsAndHooks => {
                "Project MCP server approvals, plugin enablement, marketplaces, and lifecycle hooks."
            }
            Self::AuthAndCloud => {
                "Credential helpers, login restrictions, update channel, and enterprise wrappers."
            }
            Self::InterfaceAndSession => {
                "Output style, language, shell, editor mode, theme, notifications, status line, and voice."
            }
            Self::EnvironmentAndRaw => {
                "Process environment variables, extra raw CLI flags, and custom --settings JSON."
            }
        }
    }
}

/// How a CLI launch flag setting is serialized into `argv`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliFlagEncoding {
    /// Emits `flag` when `Bool(true)`; omitted when `Bool(false)` or unset.
    BoolFlag(&'static str),
    /// Emits `on` when `Bool(true)`, `off` when `Bool(false)`.
    BoolTogglePair { on: &'static str, off: &'static str },
    /// Emits `flag` followed by `"true"` or `"false"`.
    BoolValueFlag(&'static str),
    /// Emits `flag` followed by the scalar string/number value.
    ValueFlag(&'static str),
    /// Emits just `flag` when string is empty, or `flag <value>` when non-empty.
    OptionalValueFlag(&'static str),
    /// Emits `flag` followed by all list items as separate arguments.
    MultiValueFlag(&'static str),
    /// Emits `flag <item>` once per list item.
    RepeatableFlag(&'static str),
}

/// Where a setting is applied when spawning Claude.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeSettingTarget {
    /// Passed as a CLI flag directly on the `claude` command line.
    CliFlag(CliFlagEncoding),
    /// Inserted into the JSON object passed via `--settings <json>`.
    /// Dot-separated paths (e.g. `"sandbox.network.allowLocalBinding"`) create nested objects.
    SettingsJson { path: &'static str },
}

/// Typed schema and UI hint for one Claude setting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClaudeSettingKind {
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
        /// When `true` and the list has exactly 1 entry, serialize as a JSON string instead of a 1-element array.
        single_as_string: bool,
    },
    Json {
        placeholder: &'static str,
        expect_array: bool,
    },
}

/// Static specification of one user-configurable Claude spawn setting.
#[derive(Debug, Clone, Copy)]
pub struct ClaudeSettingSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub display_key: &'static str,
    pub description: &'static str,
    pub category: ClaudeSettingCategory,
    pub target: ClaudeSettingTarget,
    pub kind: ClaudeSettingKind,
}

impl ClaudeSettingSpec {
    /// Human-readable hint describing the CLI default value when unset.
    pub fn default_hint_label(&self) -> Option<String> {
        match self.kind {
            ClaudeSettingKind::Bool {
                default_hint: Some(true),
            } => Some("CLI default: On".to_owned()),
            ClaudeSettingKind::Bool {
                default_hint: Some(false),
            } => Some("CLI default: Off".to_owned()),
            ClaudeSettingKind::Choice {
                default_hint: Some(val),
                ..
            } => Some(format!("CLI default: {val}")),
            ClaudeSettingKind::String {
                default_hint: Some(val),
                ..
            } => Some(format!("CLI default: {val}")),
            ClaudeSettingKind::Integer {
                default_hint: Some(val),
                ..
            } => Some(format!("CLI default: {val}")),
            ClaudeSettingKind::Number {
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

/// Quick-pick preset for a well-known Claude Code environment variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaudeEnvPreset {
    pub name: &'static str,
    pub description: &'static str,
    pub placeholder: &'static str,
    pub suggested_value: &'static str,
}

impl ClaudeEnvPreset {
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

/// Typed value stored for an overridden Claude setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ClaudeSettingValue {
    Bool(bool),
    String(String),
    Integer(i64),
    /// Validated finite decimal number stored as text to preserve `Eq` and exact formatting.
    Number(String),
    StringList(Vec<String>),
    /// Validated JSON object or array payload.
    Json(String),
}

impl ClaudeSettingValue {
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

/// Validation and serialization errors for [`ClaudeSpawnSettings`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClaudeSettingsError {
    #[error("unknown Claude setting '{0}'")]
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
        "'{0}' is always enforced by Bruk (set to 1) so runs cannot auto-update Claude; updates must be user-directed"
    )]
    ReservedEnvVar(String),
    #[error("invalid extra CLI arguments: {0}")]
    InvalidExtraCliArgs(String),
    #[error("extra settings JSON must be a valid JSON object: {0}")]
    InvalidExtraSettingsJson(String),
}

/// Persisted custom settings applied whenever Bruk spawns the `claude` CLI.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ClaudeSpawnSettings {
    /// Overrides for CLI launch flags (keyed by [`ClaudeSettingSpec::id`]).
    #[serde(default)]
    pub cli_flags: BTreeMap<String, ClaudeSettingValue>,
    /// Overrides for Claude Code user settings passed via `--settings <json>`.
    #[serde(default)]
    pub user_settings: BTreeMap<String, ClaudeSettingValue>,
    /// Environment variables applied to the spawned `claude` process.
    #[serde(default)]
    pub env_vars: BTreeMap<String, String>,
    /// Extra raw CLI arguments appended verbatim (shell-word split) when spawning Claude.
    #[serde(default)]
    pub extra_cli_args: String,
    /// Extra raw JSON object deep-merged into the `--settings` JSON payload.
    #[serde(default)]
    pub extra_settings_json: String,
}

impl ClaudeSpawnSettings {
    /// Looks up the current override value for a catalog setting by `spec_id`.
    pub fn get(&self, spec_id: &str) -> Option<&ClaudeSettingValue> {
        self.cli_flags
            .get(spec_id)
            .or_else(|| self.user_settings.get(spec_id))
    }

    /// Returns `true` when no custom flags, user settings, env vars, or raw overrides are configured.
    pub fn is_empty(&self) -> bool {
        self.override_count() == 0
    }

    /// Total number of active custom overrides across flags, settings, env vars, and raw fields.
    pub fn override_count(&self) -> usize {
        self.cli_flags.len()
            + self.user_settings.len()
            + self.env_vars.len()
            + usize::from(!self.extra_cli_args.trim().is_empty())
            + usize::from(!self.extra_settings_json.trim().is_empty())
    }

    /// Number of active custom overrides belonging to `category`.
    pub fn override_count_in_category(&self, category: ClaudeSettingCategory) -> usize {
        if category == ClaudeSettingCategory::EnvironmentAndRaw {
            return self.env_vars.len()
                + usize::from(!self.extra_cli_args.trim().is_empty())
                + usize::from(!self.extra_settings_json.trim().is_empty());
        }
        CLAUDE_SETTING_SPECS
            .iter()
            .filter(|spec| spec.category == category && self.get(spec.id).is_some())
            .count()
    }

    /// Clears all custom spawn overrides back to CLI defaults.
    pub fn reset_all(&mut self) {
        self.cli_flags.clear();
        self.user_settings.clear();
        self.env_vars.clear();
        self.extra_cli_args.clear();
        self.extra_settings_json.clear();
    }

    /// Removes the override for `spec_id` so it inherits the CLI default.
    pub fn clear(&mut self, spec_id: &str) -> bool {
        self.cli_flags.remove(spec_id).is_some() || self.user_settings.remove(spec_id).is_some()
    }

    /// Validates and stores a typed override value for `spec_id`.
    pub fn set_value(
        &mut self,
        spec_id: &str,
        value: ClaudeSettingValue,
    ) -> Result<(), ClaudeSettingsError> {
        let spec = find_claude_setting_spec(spec_id)
            .ok_or_else(|| ClaudeSettingsError::UnknownSetting(spec_id.to_owned()))?;
        validate_value_for_spec(spec, &value)?;
        match spec.target {
            ClaudeSettingTarget::CliFlag(_) => {
                self.cli_flags.insert(spec.id.to_owned(), value);
            }
            ClaudeSettingTarget::SettingsJson { .. } => {
                self.user_settings.insert(spec.id.to_owned(), value);
            }
        }
        Ok(())
    }

    /// Parses, validates, and stores an override value from user text input.
    pub fn set_from_text(&mut self, spec_id: &str, raw: &str) -> Result<(), ClaudeSettingsError> {
        let spec = find_claude_setting_spec(spec_id)
            .ok_or_else(|| ClaudeSettingsError::UnknownSetting(spec_id.to_owned()))?;
        let parsed = parse_text_for_spec(spec, raw)?;
        self.set_value(spec_id, parsed)
    }

    /// Validates and sets a process environment variable to inject on spawn.
    pub fn set_env_var(&mut self, key: &str, value: &str) -> Result<(), ClaudeSettingsError> {
        let key = key.trim();
        validate_env_var_name(key)?;
        if key == CLAUDE_DISABLE_AUTOUPDATER_ENV {
            return Err(ClaudeSettingsError::ReservedEnvVar(key.to_owned()));
        }
        self.env_vars.insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    /// Removes a configured environment variable override.
    pub fn remove_env_var(&mut self, key: &str) -> bool {
        self.env_vars.remove(key.trim()).is_some()
    }

    /// Validates and stores extra raw CLI arguments (shell-word split at spawn time).
    pub fn set_extra_cli_args(&mut self, raw: &str) -> Result<(), ClaudeSettingsError> {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            split_shell_words(trimmed)?;
        }
        self.extra_cli_args = trimmed.to_owned();
        Ok(())
    }

    /// Validates and stores an extra raw JSON object merged into `--settings`.
    pub fn set_extra_settings_json(&mut self, raw: &str) -> Result<(), ClaudeSettingsError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            self.extra_settings_json.clear();
            return Ok(());
        }
        let parsed: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|err| ClaudeSettingsError::InvalidExtraSettingsJson(err.to_string()))?;
        if !parsed.is_object() {
            return Err(ClaudeSettingsError::InvalidExtraSettingsJson(
                "top-level value must be a JSON object ({ ... })".to_owned(),
            ));
        }
        let formatted =
            serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| trimmed.to_owned());
        self.extra_settings_json = formatted;
        Ok(())
    }

    /// Builds the combined `--settings` JSON object from all configured user settings and
    /// `extra_settings_json`. Returns `Ok(None)` when no JSON settings overrides are active.
    pub fn build_settings_json(&self) -> Result<Option<serde_json::Value>, ClaudeSettingsError> {
        let mut root = serde_json::Map::new();

        for (spec_id, value) in &self.user_settings {
            let Some(spec) = find_claude_setting_spec(spec_id) else {
                continue;
            };
            let ClaudeSettingTarget::SettingsJson { path } = spec.target else {
                continue;
            };
            let json_val = setting_value_to_json(spec, value)?;
            insert_dot_path(&mut root, path, json_val);
        }

        let extra_raw = self.extra_settings_json.trim();
        if !extra_raw.is_empty() {
            let extra_val: serde_json::Value = serde_json::from_str(extra_raw)
                .map_err(|err| ClaudeSettingsError::InvalidExtraSettingsJson(err.to_string()))?;
            let Some(extra_obj) = extra_val.as_object() else {
                return Err(ClaudeSettingsError::InvalidExtraSettingsJson(
                    "top-level value must be a JSON object ({ ... })".to_owned(),
                ));
            };
            deep_merge_objects(&mut root, extra_obj);
        }

        // Prevent a nested `env.DISABLE_AUTOUPDATER` override in raw `--settings` JSON from
        // re-enabling background auto-updates (Claude Code's `settings.env` overrides shell env).
        if let Some(serde_json::Value::Object(env_obj)) = root.get_mut("env")
            && env_obj.contains_key(CLAUDE_DISABLE_AUTOUPDATER_ENV)
        {
            env_obj.insert(
                CLAUDE_DISABLE_AUTOUPDATER_ENV.to_owned(),
                serde_json::Value::String(CLAUDE_DISABLE_AUTOUPDATER_VALUE.to_owned()),
            );
        }

        if root.is_empty() {
            Ok(None)
        } else {
            Ok(Some(serde_json::Value::Object(root)))
        }
    }

    /// Builds the CLI flag arguments from `cli_flags` and `extra_cli_args` (excluding `--settings`,
    /// which [`build_claude_spawn_plan`] appends after serializing [`Self::build_settings_json`]).
    pub fn build_cli_flag_args(&self) -> Result<Vec<String>, ClaudeSettingsError> {
        let mut args = Vec::new();

        // Emit flags in deterministic catalog order.
        for spec in CLAUDE_SETTING_SPECS {
            let ClaudeSettingTarget::CliFlag(encoding) = spec.target else {
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

/// Per-invocation options merged with persistent [`ClaudeSpawnSettings`] when spawning `claude`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClaudeSpawnOptions {
    /// Optional working directory for the spawned `claude` process.
    pub working_dir: Option<PathBuf>,
    /// Optional per-run model override (takes precedence over `default_model`).
    pub model_override: Option<String>,
    /// Run non-interactively with `--print`.
    pub print_mode: bool,
    /// Use bidirectional `stream-json` input/output formatting (implies `--print`).
    pub stream_json: bool,
    /// Optional display session name (`--name`).
    pub session_name: Option<String>,
    /// Optional positional prompt argument passed last.
    pub prompt: Option<String>,
}

/// Fully resolved invocation plan ready to preview or convert into a [`std::process::Command`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeSpawnPlan {
    pub binary_path: PathBuf,
    pub working_dir: Option<PathBuf>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub settings_json: Option<serde_json::Value>,
}

impl ClaudeSpawnPlan {
    /// Builds a [`std::process::Command`] configured with the binary, working dir, args, and env.
    pub fn to_command(&self) -> Command {
        let mut cmd = new_claude_command(&self.binary_path);
        if let Some(dir) = &self.working_dir {
            cmd.current_dir(dir);
        }
        cmd.args(&self.args);
        cmd.envs(&self.env);
        apply_mandatory_claude_spawn_env(&mut cmd);
        cmd
    }

    /// Renders a shell-safe command preview string for display in Preferences.
    pub fn preview_command_line(&self) -> String {
        let mut parts = Vec::new();
        for (k, v) in &self.env {
            parts.push(format!("{k}={}", shell_quote(v)));
        }
        parts.push(shell_quote(&self.binary_path.display().to_string()));
        for arg in &self.args {
            parts.push(shell_quote(arg));
        }
        parts.join(" ")
    }

    /// Pretty-printed JSON payload passed to `--settings`, if any user settings are configured.
    pub fn settings_json_pretty(&self) -> Option<String> {
        self.settings_json
            .as_ref()
            .and_then(|val| serde_json::to_string_pretty(val).ok())
    }
}

/// Builds a complete [`ClaudeSpawnPlan`] combining the resolved binary, default model,
/// user-configured [`ClaudeSpawnSettings`], and per-invocation [`ClaudeSpawnOptions`].
pub fn build_claude_spawn_plan(
    binary_path: impl AsRef<Path>,
    default_model: Option<&str>,
    settings: &ClaudeSpawnSettings,
    options: &ClaudeSpawnOptions,
) -> Result<ClaudeSpawnPlan, ClaudeSettingsError> {
    let mut args = Vec::new();

    if options.print_mode || options.stream_json {
        args.push("--print".to_owned());
    }
    if options.stream_json {
        args.push("--input-format".to_owned());
        args.push("stream-json".to_owned());
        args.push("--output-format".to_owned());
        args.push("stream-json".to_owned());
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

    if let Some(name) = options
        .session_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        args.push("--name".to_owned());
        args.push(name.to_owned());
    }

    args.extend(settings.build_cli_flag_args()?);

    let settings_json = settings.build_settings_json()?;
    if let Some(ref json_val) = settings_json {
        let serialized = serde_json::to_string(json_val)
            .map_err(|err| ClaudeSettingsError::InvalidExtraSettingsJson(err.to_string()))?;
        args.push("--settings".to_owned());
        args.push(serialized);
    }

    if let Some(prompt) = options
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        args.push(prompt.to_owned());
    }

    let mut env = settings.env_vars.clone();
    env.insert(
        CLAUDE_DISABLE_AUTOUPDATER_ENV.to_owned(),
        CLAUDE_DISABLE_AUTOUPDATER_VALUE.to_owned(),
    );

    Ok(ClaudeSpawnPlan {
        binary_path: binary_path.as_ref().to_path_buf(),
        working_dir: options.working_dir.clone(),
        args,
        env,
        settings_json,
    })
}

/// Convenience helper returning a ready-to-spawn [`std::process::Command`] for `claude`.
pub fn build_claude_command(
    binary_path: impl AsRef<Path>,
    default_model: Option<&str>,
    settings: &ClaudeSpawnSettings,
    options: &ClaudeSpawnOptions,
) -> Result<Command, ClaudeSettingsError> {
    let plan = build_claude_spawn_plan(binary_path, default_model, settings, options)?;
    Ok(plan.to_command())
}

/// Finds a setting specification by its canonical [`ClaudeSettingSpec::id`].
pub fn find_claude_setting_spec(id: &str) -> Option<&'static ClaudeSettingSpec> {
    CLAUDE_SETTING_SPECS.iter().find(|spec| spec.id == id)
}

/// Returns all setting specifications belonging to `category`.
pub fn claude_setting_specs_for_category(
    category: ClaudeSettingCategory,
) -> Vec<&'static ClaudeSettingSpec> {
    CLAUDE_SETTING_SPECS
        .iter()
        .filter(|spec| spec.category == category)
        .collect()
}

fn validate_env_var_name(name: &str) -> Result<(), ClaudeSettingsError> {
    if name.is_empty() {
        return Err(ClaudeSettingsError::EmptyEnvName);
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(ClaudeSettingsError::EmptyEnvName);
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(ClaudeSettingsError::InvalidEnvName(name.to_owned()));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(ClaudeSettingsError::InvalidEnvName(name.to_owned()));
    }
    Ok(())
}

fn validate_value_for_spec(
    spec: &ClaudeSettingSpec,
    value: &ClaudeSettingValue,
) -> Result<(), ClaudeSettingsError> {
    let err = |reason: String| ClaudeSettingsError::InvalidValue {
        key: spec.display_key.to_owned(),
        reason,
    };

    match (spec.kind, value) {
        (ClaudeSettingKind::Bool { .. }, ClaudeSettingValue::Bool(_)) => Ok(()),
        (
            ClaudeSettingKind::Choice {
                options,
                allow_custom,
                ..
            },
            ClaudeSettingValue::String(s),
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
        (ClaudeSettingKind::String { allow_empty, .. }, ClaudeSettingValue::String(s)) => {
            if !allow_empty && s.trim().is_empty() {
                return Err(err("value cannot be empty".to_owned()));
            }
            Ok(())
        }
        (ClaudeSettingKind::Integer { min, max, .. }, ClaudeSettingValue::Integer(n)) => {
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
        (ClaudeSettingKind::Number { min, max, .. }, ClaudeSettingValue::Number(raw)) => {
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
        (ClaudeSettingKind::StringList { .. }, ClaudeSettingValue::StringList(_)) => Ok(()),
        (ClaudeSettingKind::Json { expect_array, .. }, ClaudeSettingValue::Json(raw)) => {
            let parsed: serde_json::Value =
                serde_json::from_str(raw.trim()).map_err(|e| err(format!("invalid JSON: {e}")))?;
            if expect_array && !parsed.is_array() {
                return Err(err("expected a JSON array ([ ... ])".to_owned()));
            }
            if !expect_array && !parsed.is_object() {
                return Err(err("expected a JSON object ({ ... })".to_owned()));
            }
            Ok(())
        }
        _ => Err(err("value type does not match setting schema".to_owned())),
    }
}

fn parse_text_for_spec(
    spec: &ClaudeSettingSpec,
    raw: &str,
) -> Result<ClaudeSettingValue, ClaudeSettingsError> {
    let err = |reason: String| ClaudeSettingsError::InvalidValue {
        key: spec.display_key.to_owned(),
        reason,
    };

    match spec.kind {
        ClaudeSettingKind::Bool { .. } => match raw.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Ok(ClaudeSettingValue::Bool(true)),
            "false" | "0" | "no" | "off" => Ok(ClaudeSettingValue::Bool(false)),
            _ => Err(err("expected true or false".to_owned())),
        },
        ClaudeSettingKind::Choice {
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
            Ok(ClaudeSettingValue::String(trimmed.to_owned()))
        }
        ClaudeSettingKind::String { allow_empty, .. } => {
            let trimmed = raw.trim();
            if !allow_empty && trimmed.is_empty() {
                return Err(err("value cannot be empty".to_owned()));
            }
            Ok(ClaudeSettingValue::String(
                if allow_empty && raw.is_empty() {
                    String::new()
                } else {
                    trimmed.to_owned()
                },
            ))
        }
        ClaudeSettingKind::Integer { min, max, .. } => {
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
            Ok(ClaudeSettingValue::Integer(parsed))
        }
        ClaudeSettingKind::Number { min, max, .. } => {
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
            Ok(ClaudeSettingValue::Number(trimmed.to_owned()))
        }
        ClaudeSettingKind::StringList { .. } => {
            let items: Vec<String> = raw
                .split([',', '\n'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            Ok(ClaudeSettingValue::StringList(items))
        }
        ClaudeSettingKind::Json { expect_array, .. } => {
            let trimmed = raw.trim();
            let parsed: serde_json::Value =
                serde_json::from_str(trimmed).map_err(|e| err(format!("invalid JSON: {e}")))?;
            if expect_array && !parsed.is_array() {
                return Err(err("expected a JSON array ([ ... ])".to_owned()));
            }
            if !expect_array && !parsed.is_object() {
                return Err(err("expected a JSON object ({ ... })".to_owned()));
            }
            let normalized = serde_json::to_string(&parsed).unwrap_or_else(|_| trimmed.to_owned());
            Ok(ClaudeSettingValue::Json(normalized))
        }
    }
}

fn setting_value_to_json(
    spec: &ClaudeSettingSpec,
    value: &ClaudeSettingValue,
) -> Result<serde_json::Value, ClaudeSettingsError> {
    match value {
        ClaudeSettingValue::Bool(b) => Ok(serde_json::Value::Bool(*b)),
        ClaudeSettingValue::String(s) => Ok(serde_json::Value::String(s.clone())),
        ClaudeSettingValue::Integer(n) => Ok(serde_json::Value::Number((*n).into())),
        ClaudeSettingValue::Number(raw) => {
            let parsed: f64 =
                raw.trim()
                    .parse()
                    .map_err(|_| ClaudeSettingsError::InvalidValue {
                        key: spec.display_key.to_owned(),
                        reason: "invalid number".to_owned(),
                    })?;
            let number = serde_json::Number::from_f64(parsed).ok_or_else(|| {
                ClaudeSettingsError::InvalidValue {
                    key: spec.display_key.to_owned(),
                    reason: "number must be finite".to_owned(),
                }
            })?;
            Ok(serde_json::Value::Number(number))
        }
        ClaudeSettingValue::StringList(items) => {
            if let ClaudeSettingKind::StringList {
                single_as_string: true,
                ..
            } = spec.kind
                && items.len() == 1
            {
                return Ok(serde_json::Value::String(items[0].clone()));
            }
            Ok(serde_json::Value::Array(
                items
                    .iter()
                    .map(|item| serde_json::Value::String(item.clone()))
                    .collect(),
            ))
        }
        ClaudeSettingValue::Json(raw) => {
            serde_json::from_str(raw.trim()).map_err(|err| ClaudeSettingsError::InvalidValue {
                key: spec.display_key.to_owned(),
                reason: format!("invalid JSON: {err}"),
            })
        }
    }
}

fn encode_cli_flag(args: &mut Vec<String>, encoding: CliFlagEncoding, value: &ClaudeSettingValue) {
    match (encoding, value) {
        (CliFlagEncoding::BoolFlag(flag), ClaudeSettingValue::Bool(true)) => {
            args.push(flag.to_owned());
        }
        (CliFlagEncoding::BoolFlag(_), ClaudeSettingValue::Bool(false)) => {}
        (CliFlagEncoding::BoolTogglePair { on, off }, ClaudeSettingValue::Bool(enabled)) => {
            args.push(if *enabled { on } else { off }.to_owned());
        }
        (CliFlagEncoding::BoolValueFlag(flag), ClaudeSettingValue::Bool(enabled)) => {
            args.push(flag.to_owned());
            args.push(if *enabled { "true" } else { "false" }.to_owned());
        }
        (CliFlagEncoding::ValueFlag(flag), ClaudeSettingValue::String(s)) => {
            args.push(flag.to_owned());
            args.push(s.clone());
        }
        (CliFlagEncoding::ValueFlag(flag), ClaudeSettingValue::Integer(n)) => {
            args.push(flag.to_owned());
            args.push(n.to_string());
        }
        (CliFlagEncoding::ValueFlag(flag), ClaudeSettingValue::Number(n)) => {
            args.push(flag.to_owned());
            args.push(n.clone());
        }
        (CliFlagEncoding::ValueFlag(flag), ClaudeSettingValue::Json(raw)) => {
            args.push(flag.to_owned());
            args.push(raw.clone());
        }
        (CliFlagEncoding::OptionalValueFlag(flag), ClaudeSettingValue::String(s)) => {
            args.push(flag.to_owned());
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                args.push(trimmed.to_owned());
            }
        }
        (CliFlagEncoding::MultiValueFlag(flag), ClaudeSettingValue::StringList(items)) => {
            if items.is_empty() {
                args.push(flag.to_owned());
                args.push(String::new());
            } else {
                args.push(flag.to_owned());
                args.extend(items.iter().cloned());
            }
        }
        (CliFlagEncoding::RepeatableFlag(flag), ClaudeSettingValue::StringList(items)) => {
            for item in items {
                args.push(flag.to_owned());
                args.push(item.clone());
            }
        }
        _ => {}
    }
}

fn insert_dot_path(
    root: &mut serde_json::Map<String, serde_json::Value>,
    path: &str,
    value: serde_json::Value,
) {
    let segments: Vec<&str> = path.split('.').collect();
    let mut current = root;
    for segment in &segments[..segments.len().saturating_sub(1)] {
        let entry = current
            .entry((*segment).to_owned())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !entry.is_object() {
            *entry = serde_json::Value::Object(serde_json::Map::new());
        }
        current = entry
            .as_object_mut()
            .expect("inserted JSON value is an object");
    }
    if let Some(leaf) = segments.last() {
        current.insert((*leaf).to_owned(), value);
    }
}

fn deep_merge_objects(
    base: &mut serde_json::Map<String, serde_json::Value>,
    overlay: &serde_json::Map<String, serde_json::Value>,
) {
    for (k, v) in overlay {
        match (base.get_mut(k), v) {
            (Some(serde_json::Value::Object(base_child)), serde_json::Value::Object(ov_child)) => {
                deep_merge_objects(base_child, ov_child);
            }
            _ => {
                base.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Splits a shell-style argument string respecting single quotes, double quotes, and escapes.
pub fn split_shell_words(input: &str) -> Result<Vec<String>, ClaudeSettingsError> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            Some('"') => {
                if ch == '"' {
                    quote = None;
                } else if ch == '\\' {
                    if let Some(&next) = chars.peek() {
                        if matches!(next, '"' | '\\' | '$' | '`') {
                            current.push(chars.next().unwrap());
                        } else {
                            current.push('\\');
                        }
                    } else {
                        return Err(ClaudeSettingsError::InvalidExtraCliArgs(
                            "trailing backslash inside double quotes".to_owned(),
                        ));
                    }
                } else {
                    current.push(ch);
                }
            }
            None => match ch {
                c if c.is_whitespace() => {
                    if in_word {
                        words.push(std::mem::take(&mut current));
                        in_word = false;
                    }
                }
                '\'' | '"' => {
                    quote = Some(ch);
                    in_word = true;
                }
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        current.push(escaped);
                        in_word = true;
                    } else {
                        return Err(ClaudeSettingsError::InvalidExtraCliArgs(
                            "trailing backslash at end of arguments".to_owned(),
                        ));
                    }
                }
                other => {
                    current.push(other);
                    in_word = true;
                }
            },
            Some(_) => unreachable!(),
        }
    }

    if let Some(open) = quote {
        return Err(ClaudeSettingsError::InvalidExtraCliArgs(format!(
            "unclosed quote ({open})"
        )));
    }
    if in_word {
        words.push(current);
    }

    Ok(words)
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

/// Complete catalog of user-configurable Claude CLI launch flags and `settings.json` options.
pub const CLAUDE_SETTING_SPECS: &[ClaudeSettingSpec] = &[
    // --- Category 1: Launch & CLI Flags (`claude --help`) --------------------------------
    ClaudeSettingSpec {
        id: "cli.permission-mode",
        label: "Permission Mode",
        display_key: "--permission-mode",
        description: "Permission mode passed when launching a Claude session.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--permission-mode")),
        kind: ClaudeSettingKind::Choice {
            options: &[
                "manual",
                "acceptEdits",
                "auto",
                "plan",
                "dontAsk",
                "bypassPermissions",
            ],
            allow_custom: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "cli.permission-prompts",
        label: "Permission Prompts Target",
        display_key: "--permission-prompts",
        description: "Who answers permission prompts in --print mode: host or none (auto-deny).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--permission-prompts")),
        kind: ClaudeSettingKind::Choice {
            options: &["host", "none"],
            allow_custom: false,
            default_hint: Some("host"),
        },
    },
    ClaudeSettingSpec {
        id: "cli.effort",
        label: "Session Effort Level",
        display_key: "--effort",
        description: "Reasoning effort level for spawned sessions (low, medium, high, xhigh, max).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--effort")),
        kind: ClaudeSettingKind::Choice {
            options: &["low", "medium", "high", "xhigh", "max"],
            allow_custom: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "cli.fallback-model",
        label: "Fallback Models",
        display_key: "--fallback-model",
        description: "Comma-separated models to fall back to when the primary model is busy or unavailable.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--fallback-model")),
        kind: ClaudeSettingKind::String {
            placeholder: "sonnet,haiku",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "cli.agent",
        label: "Session Agent",
        display_key: "--agent",
        description: "Named built-in or custom agent for the session (overrides the 'agent' setting).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--agent")),
        kind: ClaudeSettingKind::String {
            placeholder: "e.g. reviewer",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "cli.agents",
        label: "Custom Agents Definition",
        display_key: "--agents",
        description: "JSON object defining custom agents, or a file path in --print mode.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--agents")),
        kind: ClaudeSettingKind::String {
            placeholder: r#"{"reviewer":{"description":"Reviews code","prompt":"..."}}"#,
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "cli.system-prompt",
        label: "System Prompt Override",
        display_key: "--system-prompt",
        description: "Replace the default system prompt for the session.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--system-prompt")),
        kind: ClaudeSettingKind::String {
            placeholder: "Custom system prompt text",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "cli.append-system-prompt",
        label: "Append System Prompt",
        display_key: "--append-system-prompt",
        description: "Append extra instructions to the end of Claude's default system prompt.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--append-system-prompt")),
        kind: ClaudeSettingKind::String {
            placeholder: "Additional instructions appended to system prompt",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "cli.system-prompt-snapshot",
        label: "System Prompt Snapshot",
        display_key: "--system-prompt-snapshot",
        description: "Record the system prompt once per conversation and reuse verbatim, or re-render each turn.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag(
            "--system-prompt-snapshot",
        )),
        kind: ClaudeSettingKind::Choice {
            options: &["on", "off"],
            allow_custom: false,
            default_hint: Some("on"),
        },
    },
    ClaudeSettingSpec {
        id: "cli.exclude-dynamic-system-prompt-sections",
        label: "Exclude Dynamic Prompt Sections",
        display_key: "--exclude-dynamic-system-prompt-sections",
        description: "Move per-machine sections (cwd, env, git status) out of system prompt for better prompt-cache reuse.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag(
            "--exclude-dynamic-system-prompt-sections",
        )),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.tools",
        label: "Built-in Tools Set",
        display_key: "--tools",
        description: "Built-in tools to expose: 'default', specific names (Bash, Edit, Read), or empty for none.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::MultiValueFlag("--tools")),
        kind: ClaudeSettingKind::StringList {
            placeholder: "Bash, Edit, Read",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "cli.allowed-tools",
        label: "Allowed Tools",
        display_key: "--allowed-tools",
        description: "Tool names or rules pre-approved on spawn (e.g. Bash(git *), Edit).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::MultiValueFlag("--allowed-tools")),
        kind: ClaudeSettingKind::StringList {
            placeholder: "Bash(git *), Edit, Read",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "cli.disallowed-tools",
        label: "Disallowed Tools",
        display_key: "--disallowed-tools",
        description: "Tool names or rules explicitly denied on spawn (e.g. Bash(rm *), WebFetch).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::MultiValueFlag("--disallowed-tools")),
        kind: ClaudeSettingKind::StringList {
            placeholder: "WebFetch, Bash(rm *)",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "cli.add-dir",
        label: "Additional Directories",
        display_key: "--add-dir",
        description: "Extra directories Claude is allowed to access beyond the working directory.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::MultiValueFlag("--add-dir")),
        kind: ClaudeSettingKind::StringList {
            placeholder: "/path/to/shared-libs, ../docs",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "cli.autocompact",
        label: "Auto-Compact Window",
        display_key: "--autocompact",
        description: "Auto-compact window size: 'auto' or a token count of at least 100k (e.g. 150000).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--autocompact")),
        kind: ClaudeSettingKind::String {
            placeholder: "auto or 150000",
            allow_empty: false,
            default_hint: Some("auto"),
        },
    },
    ClaudeSettingSpec {
        id: "cli.max-budget-usd",
        label: "Max Budget (USD)",
        display_key: "--max-budget-usd",
        description: "Maximum dollar spend allowed per --print invocation.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--max-budget-usd")),
        kind: ClaudeSettingKind::Number {
            min: Some(0.0),
            max: None,
            placeholder: "e.g. 5.00",
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "cli.dangerously-skip-permissions",
        label: "Dangerously Skip Permissions",
        display_key: "--dangerously-skip-permissions",
        description: "Bypass all permission checks immediately. Recommended only for isolated sandboxes.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag(
            "--dangerously-skip-permissions",
        )),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.allow-dangerously-skip-permissions",
        label: "Allow Dangerously Skip Permissions",
        display_key: "--allow-dangerously-skip-permissions",
        description: "Enable bypassing permission checks as an option without turning it on by default.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag(
            "--allow-dangerously-skip-permissions",
        )),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.setting-sources",
        label: "Setting Sources",
        display_key: "--setting-sources",
        description: "Comma-separated list of setting sources to load (user, project, local).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--setting-sources")),
        kind: ClaudeSettingKind::String {
            placeholder: "user,project,local",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "cli.settings-file",
        label: "Additional Settings File",
        display_key: "--settings (file)",
        description: "Path to an extra settings JSON file loaded before Bruk's inline setting overrides.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--settings")),
        kind: ClaudeSettingKind::String {
            placeholder: "~/.claude/bruk-profile.json",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "cli.mcp-config",
        label: "MCP Config Files / JSON",
        display_key: "--mcp-config",
        description: "Load MCP servers from one or more JSON files or inline JSON strings.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::MultiValueFlag("--mcp-config")),
        kind: ClaudeSettingKind::StringList {
            placeholder: "/path/to/mcp.json",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "cli.strict-mcp-config",
        label: "Strict MCP Config",
        display_key: "--strict-mcp-config",
        description: "Only load MCP servers from --mcp-config, ignoring all other MCP configurations.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--strict-mcp-config")),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.plugin-dir",
        label: "Plugin Directories / Zips",
        display_key: "--plugin-dir",
        description: "Session-only plugin directories or .zip files (emitted as repeatable --plugin-dir flags).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::RepeatableFlag("--plugin-dir")),
        kind: ClaudeSettingKind::StringList {
            placeholder: "/path/to/plugin, /path/to/plugin.zip",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "cli.plugin-url",
        label: "Plugin URLs",
        display_key: "--plugin-url",
        description: "Session-only plugin .zip URLs (emitted as repeatable --plugin-url flags).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::RepeatableFlag("--plugin-url")),
        kind: ClaudeSettingKind::StringList {
            placeholder: "https://example.com/plugin.zip",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "cli.betas",
        label: "API Beta Headers",
        display_key: "--betas",
        description: "Anthropic API beta header identifiers to include in requests (API key users only).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::MultiValueFlag("--betas")),
        kind: ClaudeSettingKind::StringList {
            placeholder: "interleaved-thinking-2025-05-14",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "cli.disable-slash-commands",
        label: "Disable Slash Commands & Skills",
        display_key: "--disable-slash-commands",
        description: "Disable all skills and slash commands for spawned sessions.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--disable-slash-commands")),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.bare",
        label: "Bare Minimal Mode",
        display_key: "--bare",
        description: "Skip hooks, LSP, plugin sync, attribution, auto-memory, keychain reads, and CLAUDE.md discovery.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--bare")),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.safe-mode",
        label: "Safe Mode",
        display_key: "--safe-mode",
        description: "Disable all user/project customizations (CLAUDE.md, skills, plugins, hooks, MCP, themes).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--safe-mode")),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.restricted",
        label: "Restricted Mode",
        display_key: "--restricted",
        description: "Remove command/code execution tools & WebFetch, ignore user/project settings, and refuse bypassPermissions.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--restricted")),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.no-session-persistence",
        label: "No Session Persistence",
        display_key: "--no-session-persistence",
        description: "Do not save --print sessions to disk (cannot be resumed later).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--no-session-persistence")),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.verbose",
        label: "Verbose CLI Output",
        display_key: "--verbose",
        description: "Pass --verbose on spawn to override the verbose setting and include detailed output.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--verbose")),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.debug",
        label: "Debug Mode & Filter",
        display_key: "--debug",
        description: "Enable debug logging with an optional category filter (e.g. hooks,mcp or !hooks); leave empty for all.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::OptionalValueFlag("--debug")),
        kind: ClaudeSettingKind::String {
            placeholder: "Leave blank for all, or e.g. hooks,mcp",
            allow_empty: true,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "cli.debug-file",
        label: "Debug Log File Path",
        display_key: "--debug-file",
        description: "Write Claude debug logs to a specific file path (implicitly enables debug mode).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::ValueFlag("--debug-file")),
        kind: ClaudeSettingKind::String {
            placeholder: "/tmp/claude-debug.log",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "cli.worktree",
        label: "Git Worktree Isolation",
        display_key: "--worktree",
        description: "Create a git worktree for the session (leave blank for auto-named, or supply a worktree name).",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::OptionalValueFlag("--worktree")),
        kind: ClaudeSettingKind::String {
            placeholder: "Leave blank for auto name, or enter branch/name",
            allow_empty: true,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "cli.brief",
        label: "Brief Messaging Tool",
        display_key: "--brief",
        description: "Turn on the SendUserMessage tool so Claude can send concise updates.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--brief")),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.chrome",
        label: "Claude in Chrome",
        display_key: "--chrome / --no-chrome",
        description: "Explicitly enable (--chrome) or disable (--no-chrome) Chrome browser integration.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolTogglePair {
            on: "--chrome",
            off: "--no-chrome",
        }),
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "cli.ide",
        label: "Auto-Connect IDE Flag",
        display_key: "--ide",
        description: "Automatically connect to a running IDE on startup if exactly one valid IDE is available.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--ide")),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.ax-screen-reader",
        label: "Screen Reader Output Flag",
        display_key: "--ax-screen-reader",
        description: "Render screen-reader friendly flat text without decorative borders or animations.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--ax-screen-reader")),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.forward-subagent-text",
        label: "Forward Subagent Text",
        display_key: "--forward-subagent-text",
        description: "Forward subagent text and thinking blocks in stream-json output with parent_tool_use_id.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--forward-subagent-text")),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.include-hook-events",
        label: "Include Hook Events",
        display_key: "--include-hook-events",
        description: "Include all hook lifecycle events in stream-json output.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--include-hook-events")),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.include-partial-messages",
        label: "Include Partial Messages",
        display_key: "--include-partial-messages",
        description: "Emit partial streaming message chunks as they arrive in stream-json output.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag(
            "--include-partial-messages",
        )),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "cli.prompt-suggestions",
        label: "Prompt Suggestions",
        display_key: "--prompt-suggestions",
        description: "Emit a predicted next user prompt after each turn in print/SDK mode.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolValueFlag(
            "--prompt-suggestions",
        )),
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "cli.replay-user-messages",
        label: "Replay User Messages",
        display_key: "--replay-user-messages",
        description: "Re-emit user messages from stdin back onto stdout for acknowledgment in stream-json mode.",
        category: ClaudeSettingCategory::CliFlags,
        target: ClaudeSettingTarget::CliFlag(CliFlagEncoding::BoolFlag("--replay-user-messages")),
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    // --- Category 2: Models & Reasoning (`--settings`) -----------------------------------
    ClaudeSettingSpec {
        id: "effortLevel",
        label: "Default Effort Level",
        display_key: "effortLevel",
        description: "Persist adaptive reasoning effort across sessions (low, medium, high, xhigh).",
        category: ClaudeSettingCategory::ModelsAndReasoning,
        target: ClaudeSettingTarget::SettingsJson {
            path: "effortLevel",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["low", "medium", "high", "xhigh"],
            allow_custom: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "alwaysThinkingEnabled",
        label: "Always Thinking Enabled",
        display_key: "alwaysThinkingEnabled",
        description: "Enable extended thinking by default for all sessions.",
        category: ClaudeSettingCategory::ModelsAndReasoning,
        target: ClaudeSettingTarget::SettingsJson {
            path: "alwaysThinkingEnabled",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "fastMode",
        label: "Fast Mode",
        display_key: "fastMode",
        description: "Enable fast mode for faster output at higher per-token cost without downgrading models.",
        category: ClaudeSettingCategory::ModelsAndReasoning,
        target: ClaudeSettingTarget::SettingsJson { path: "fastMode" },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "fastModePerSessionOptIn",
        label: "Fast Mode Per-Session Opt-In",
        display_key: "fastModePerSessionOptIn",
        description: "Require opting into /fast per session instead of persisting fast mode across sessions.",
        category: ClaudeSettingCategory::ModelsAndReasoning,
        target: ClaudeSettingTarget::SettingsJson {
            path: "fastModePerSessionOptIn",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "advisorModel",
        label: "Advisor Model",
        display_key: "advisorModel",
        description: "Model alias (opus, sonnet, fable) or full model ID for the server-side advisor tool.",
        category: ClaudeSettingCategory::ModelsAndReasoning,
        target: ClaudeSettingTarget::SettingsJson {
            path: "advisorModel",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "opus",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "fallbackModel",
        label: "Fallback Model Chain",
        display_key: "fallbackModel",
        description: "Ordered fallback models tried when the primary model is overloaded or unavailable.",
        category: ClaudeSettingCategory::ModelsAndReasoning,
        target: ClaudeSettingTarget::SettingsJson {
            path: "fallbackModel",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "sonnet, haiku",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "availableModels",
        label: "Available Models Allowlist",
        display_key: "availableModels",
        description: "Restrict which models can be selected inside spawned Claude sessions.",
        category: ClaudeSettingCategory::ModelsAndReasoning,
        target: ClaudeSettingTarget::SettingsJson {
            path: "availableModels",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "opus, sonnet, haiku",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "teammateDefaultModel",
        label: "Teammate Default Model",
        display_key: "teammateDefaultModel",
        description: "Default model alias or ID for agent team teammates when not specified in the spawn prompt.",
        category: ClaudeSettingCategory::ModelsAndReasoning,
        target: ClaudeSettingTarget::SettingsJson {
            path: "teammateDefaultModel",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "sonnet",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "modelOverrides",
        label: "Model ID Overrides",
        display_key: "modelOverrides",
        description: "JSON map of Anthropic model IDs to provider-specific IDs (Bedrock ARNs, Vertex names, Foundry).",
        category: ClaudeSettingCategory::ModelsAndReasoning,
        target: ClaudeSettingTarget::SettingsJson {
            path: "modelOverrides",
        },
        kind: ClaudeSettingKind::Json {
            placeholder: r#"{"claude-sonnet-4-6":"us.anthropic.claude-sonnet-4-6-v1:0"}"#,
            expect_array: false,
        },
    },
    ClaudeSettingSpec {
        id: "showThinkingSummaries",
        label: "Show Thinking Summaries",
        display_key: "showThinkingSummaries",
        description: "Generate and display thinking summaries in transcript views.",
        category: ClaudeSettingCategory::ModelsAndReasoning,
        target: ClaudeSettingTarget::SettingsJson {
            path: "showThinkingSummaries",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    // --- Category 3: Permissions & Auto Mode (`--settings`) ------------------------------
    ClaudeSettingSpec {
        id: "permissions.defaultMode",
        label: "Default Permission Mode",
        display_key: "permissions.defaultMode",
        description: "Default tool permission mode applied in settings (default, manual, acceptEdits, auto, plan, dontAsk, bypassPermissions, delegate).",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "permissions.defaultMode",
        },
        kind: ClaudeSettingKind::Choice {
            options: &[
                "default",
                "manual",
                "acceptEdits",
                "auto",
                "plan",
                "dontAsk",
                "bypassPermissions",
                "delegate",
            ],
            allow_custom: false,
            default_hint: Some("default"),
        },
    },
    ClaudeSettingSpec {
        id: "permissions.allow",
        label: "Allow Permission Rules",
        display_key: "permissions.allow",
        description: "Permission rules automatically approved without prompting (e.g. Bash(cargo test*), Read(*)).",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "permissions.allow",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "Bash(cargo test*), Bash(git status), Read(*)",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "permissions.ask",
        label: "Ask Permission Rules",
        display_key: "permissions.ask",
        description: "Permission rules that must always prompt for confirmation before executing.",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "permissions.ask",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "Bash(git push*), WebFetch",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "permissions.deny",
        label: "Deny Permission Rules",
        display_key: "permissions.deny",
        description: "Permission rules unconditionally blocked from running (e.g. Read(./.env), Bash(rm -rf *)).",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "permissions.deny",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "Read(./.env), Bash(rm -rf *)",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "permissions.additionalDirectories",
        label: "Additional Permission Directories",
        display_key: "permissions.additionalDirectories",
        description: "Extra filesystem paths included in the session's permission scope.",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "permissions.additionalDirectories",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "~/shared-docs, /opt/workspace",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "permissions.disableBypassPermissionsMode",
        label: "Disable Bypass Permissions Mode",
        display_key: "permissions.disableBypassPermissionsMode",
        description: "Prevent bypassing permission prompts inside spawned sessions.",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "permissions.disableBypassPermissionsMode",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["disable"],
            allow_custom: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "permissions.disableAutoMode",
        label: "Disable Auto Mode (Permissions)",
        display_key: "permissions.disableAutoMode",
        description: "Prevent auto mode from being activated via Shift+Tab or --permission-mode auto.",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "permissions.disableAutoMode",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["disable"],
            allow_custom: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "disableAutoMode",
        label: "Disable Auto Mode (Top-Level)",
        display_key: "disableAutoMode",
        description: "Top-level setting preventing auto mode activation across sessions.",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "disableAutoMode",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["disable"],
            allow_custom: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "useAutoModeDuringPlan",
        label: "Use Auto Mode During Plan",
        display_key: "useAutoModeDuringPlan",
        description: "Apply the auto mode classifier during plan mode to auto-approve safe read-only tool calls.",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "useAutoModeDuringPlan",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "permissionExplainerEnabled",
        label: "Permission Explainer",
        display_key: "permissionExplainerEnabled",
        description: "Show a model-generated explanation of Bash/PowerShell commands on Ctrl+E.",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "permissionExplainerEnabled",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "skipDangerousModePermissionPrompt",
        label: "Skip Dangerous Mode Prompt",
        display_key: "skipDangerousModePermissionPrompt",
        description: "Mark the bypass permissions confirmation dialog as already accepted.",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "skipDangerousModePermissionPrompt",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "autoMode.allow",
        label: "Auto Mode Allow Rules",
        display_key: "autoMode.allow",
        description: "Custom allow rules for the auto mode classifier prompt.",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "autoMode.allow",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "Allow running unit tests and linters",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "autoMode.soft_deny",
        label: "Auto Mode Soft-Deny Rules",
        display_key: "autoMode.soft_deny",
        description: "Soft-deny rules prompting confirmation in the auto mode classifier.",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "autoMode.soft_deny",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "Prompt before modifying deployment configs",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "autoMode.hard_deny",
        label: "Auto Mode Hard-Deny Rules",
        display_key: "autoMode.hard_deny",
        description: "Hard-deny rules blocked unconditionally by the auto mode classifier.",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "autoMode.hard_deny",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "Block force pushes to main",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "autoMode.environment",
        label: "Auto Mode Environment Context",
        display_key: "autoMode.environment",
        description: "Environment description lines given to the auto mode classifier.",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "autoMode.environment",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "Local macOS development workstation",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "autoMode.classifyAllShell",
        label: "Classify All Shell Commands",
        display_key: "autoMode.classifyAllShell",
        description: "Route every Bash/PowerShell command through the auto mode classifier.",
        category: ClaudeSettingCategory::Permissions,
        target: ClaudeSettingTarget::SettingsJson {
            path: "autoMode.classifyAllShell",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    // --- Category 4: Sandbox & Security (`--settings`) -----------------------------------
    ClaudeSettingSpec {
        id: "sandbox.enabled",
        label: "Enable Bash Sandbox",
        display_key: "sandbox.enabled",
        description: "Run bash commands inside Claude Code's OS-level execution sandbox.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.enabled",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "sandbox.autoAllowBashIfSandboxed",
        label: "Auto-Allow Sandboxed Bash",
        display_key: "sandbox.autoAllowBashIfSandboxed",
        description: "Automatically allow bash commands without prompting when they execute inside the sandbox.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.autoAllowBashIfSandboxed",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.allowUnsandboxedCommands",
        label: "Allow Unsandboxed Fallback",
        display_key: "sandbox.allowUnsandboxedCommands",
        description: "Permit commands to opt out of the sandbox via dangerouslyDisableSandbox.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.allowUnsandboxedCommands",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "sandbox.excludedCommands",
        label: "Sandbox Excluded Commands",
        display_key: "sandbox.excludedCommands",
        description: "Commands that should always run outside the sandbox (e.g. git, docker).",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.excludedCommands",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "git, docker",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.failIfUnavailable",
        label: "Fail If Sandbox Unavailable",
        display_key: "sandbox.failIfUnavailable",
        description: "Treat missing sandbox dependencies as a hard startup error instead of degrading.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.failIfUnavailable",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.enabledPlatforms",
        label: "Sandbox Enabled Platforms",
        display_key: "sandbox.enabledPlatforms",
        description: "Limit sandbox configuration to listed platforms (e.g. macos, linux).",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.enabledPlatforms",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "macos, linux",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.allowAppleEvents",
        label: "Allow Apple Events (macOS)",
        display_key: "sandbox.allowAppleEvents",
        description: "Allow sandboxed commands to send Apple Events on macOS.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.allowAppleEvents",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "sandbox.enableWeakerNetworkIsolation",
        label: "Weaker Network Isolation (macOS)",
        display_key: "sandbox.enableWeakerNetworkIsolation",
        description: "Allow macOS com.apple.trustd.agent TLS trust lookups inside the sandbox (helpful for Go tools).",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.enableWeakerNetworkIsolation",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.enableWeakerNestedSandbox",
        label: "Weaker Nested Sandbox (Docker)",
        display_key: "sandbox.enableWeakerNestedSandbox",
        description: "Enable weaker sandbox mode for unprivileged Docker environments where --proc mounting fails.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.enableWeakerNestedSandbox",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "sandbox.network.allowedDomains",
        label: "Sandbox Allowed Domains",
        display_key: "sandbox.network.allowedDomains",
        description: "Domain allowlist for sandboxed network requests (supports wildcards like *.github.com).",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.network.allowedDomains",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "github.com, *.npmjs.org, crates.io",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.network.deniedDomains",
        label: "Sandbox Denied Domains",
        display_key: "sandbox.network.deniedDomains",
        description: "Domain blocklist for sandboxed network requests (overrides broader allowlist wildcards).",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.network.deniedDomains",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "telemetry.example.com",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.network.strictAllowlist",
        label: "Strict Network Allowlist",
        display_key: "sandbox.network.strictAllowlist",
        description: "Deny non-allowlisted hosts for sandboxed commands automatically without prompting.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.network.strictAllowlist",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "sandbox.network.allowLocalBinding",
        label: "Allow Local Port Binding",
        display_key: "sandbox.network.allowLocalBinding",
        description: "Allow sandboxed commands to bind localhost network ports.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.network.allowLocalBinding",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.network.allowAllUnixSockets",
        label: "Allow All Unix Sockets",
        display_key: "sandbox.network.allowAllUnixSockets",
        description: "Allow all Unix domain socket connections inside the sandbox (overrides allowUnixSockets).",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.network.allowAllUnixSockets",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "sandbox.network.allowUnixSockets",
        label: "Allowed Unix Sockets",
        display_key: "sandbox.network.allowUnixSockets",
        description: "Specific Unix socket paths accessible inside the sandbox (e.g. SSH agent, Docker).",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.network.allowUnixSockets",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "/var/run/docker.sock",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.network.allowMachLookup",
        label: "Allowed Mach Services (macOS)",
        display_key: "sandbox.network.allowMachLookup",
        description: "Additional macOS XPC/Mach service names the sandbox may look up (supports trailing *).",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.network.allowMachLookup",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "com.apple.SecurityServer",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.network.httpProxyPort",
        label: "Sandbox HTTP Proxy Port",
        display_key: "sandbox.network.httpProxyPort",
        description: "Custom HTTP proxy port (1–65535) for sandbox network filtering.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.network.httpProxyPort",
        },
        kind: ClaudeSettingKind::Integer {
            min: Some(1),
            max: Some(65535),
            placeholder: "8080",
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.network.socksProxyPort",
        label: "Sandbox SOCKS Proxy Port",
        display_key: "sandbox.network.socksProxyPort",
        description: "Custom SOCKS proxy port (1–65535) for sandbox network filtering.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.network.socksProxyPort",
        },
        kind: ClaudeSettingKind::Integer {
            min: Some(1),
            max: Some(65535),
            placeholder: "1080",
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.filesystem.disabled",
        label: "Disable Filesystem Isolation",
        display_key: "sandbox.filesystem.disabled",
        description: "Skip filesystem isolation while keeping network isolation active.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.filesystem.disabled",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.filesystem.allowRead",
        label: "Sandbox Allow Read Paths",
        display_key: "sandbox.filesystem.allowRead",
        description: "Paths re-allowed for reading inside denyRead regions.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.filesystem.allowRead",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "~/.gitconfig, /opt/sdk",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.filesystem.denyRead",
        label: "Sandbox Deny Read Paths",
        display_key: "sandbox.filesystem.denyRead",
        description: "Paths explicitly blocked from reading by sandboxed subprocesses.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.filesystem.denyRead",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "~/.ssh, ~/.aws",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.filesystem.allowWrite",
        label: "Sandbox Allow Write Paths",
        display_key: "sandbox.filesystem.allowWrite",
        description: "Paths where sandboxed subprocesses are permitted to write.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.filesystem.allowWrite",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "/tmp, ~/.cache/cargo",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.filesystem.denyWrite",
        label: "Sandbox Deny Write Paths",
        display_key: "sandbox.filesystem.denyWrite",
        description: "Paths explicitly blocked from writing (takes precedence over allowWrite).",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.filesystem.denyWrite",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: ".git/hooks, ~/.zshrc",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.credentials.files",
        label: "Hidden Credential Files",
        display_key: "sandbox.credentials.files",
        description: "Credential file paths hidden from sandboxed commands.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.credentials.files",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "~/.netrc, ~/.npmrc",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.credentials.envVars",
        label: "Scrubbed Credential Env Vars",
        display_key: "sandbox.credentials.envVars",
        description: "Secret environment variable names unset for sandboxed commands.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.credentials.envVars",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "GITHUB_TOKEN, AWS_SECRET_ACCESS_KEY",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.credentials.allowPlaintextInject",
        label: "Allow Plaintext Credential Inject",
        display_key: "sandbox.credentials.allowPlaintextInject",
        description: "Allow credential mask substitution on plain HTTP requests as well as HTTPS.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.credentials.allowPlaintextInject",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.ripgrep.command",
        label: "Custom Ripgrep Command",
        display_key: "sandbox.ripgrep.command",
        description: "Override the ripgrep binary path used by Claude Code's search tools.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.ripgrep.command",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "/opt/homebrew/bin/rg",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.ripgrep.args",
        label: "Custom Ripgrep Arguments",
        display_key: "sandbox.ripgrep.args",
        description: "Additional arguments passed to the ripgrep binary.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.ripgrep.args",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "--hidden, --follow",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "sandbox.ignoreViolations",
        label: "Sandbox Ignore Violations",
        display_key: "sandbox.ignoreViolations",
        description: "JSON map of command patterns (or '*') to filesystem paths whose sandbox violations are ignored.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "sandbox.ignoreViolations",
        },
        kind: ClaudeSettingKind::Json {
            placeholder: r#"{"*":["/tmp"]}"#,
            expect_array: false,
        },
    },
    ClaudeSettingSpec {
        id: "skipWebFetchPreflight",
        label: "Skip WebFetch Preflight",
        display_key: "skipWebFetchPreflight",
        description: "Skip the WebFetch blocklist preflight check in restricted enterprise networks.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "skipWebFetchPreflight",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "disableSkillShellExecution",
        label: "Disable Skill Shell Execution",
        display_key: "disableSkillShellExecution",
        description: "Disable inline shell execution inside skills and custom slash commands.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "disableSkillShellExecution",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "disableDeepLinkRegistration",
        label: "Disable Deep-Link Registration",
        display_key: "disableDeepLinkRegistration",
        description: "Prevent Claude Code from registering the claude-cli:// URL scheme on startup.",
        category: ClaudeSettingCategory::Sandbox,
        target: ClaudeSettingTarget::SettingsJson {
            path: "disableDeepLinkRegistration",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["disable"],
            allow_custom: false,
            default_hint: None,
        },
    },
    // --- Category 5: Context, Memory & Git (`--settings`) --------------------------------
    ClaudeSettingSpec {
        id: "autoCompactEnabled",
        label: "Auto-Compact Enabled",
        display_key: "autoCompactEnabled",
        description: "Automatically compact conversations when context approaches the model limit.",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "autoCompactEnabled",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "autoMemoryEnabled",
        label: "Auto-Memory Enabled",
        display_key: "autoMemoryEnabled",
        description: "Automatically save useful project context into ~/.claude/projects/<project>/memory/.",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "autoMemoryEnabled",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "autoMemoryDirectory",
        label: "Auto-Memory Directory",
        display_key: "autoMemoryDirectory",
        description: "Custom directory path for auto-memory storage (supports ~/ expansion).",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "autoMemoryDirectory",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "~/.claude/memory",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "claudeMdExcludes",
        label: "CLAUDE.md Exclude Globs",
        display_key: "claudeMdExcludes",
        description: "Glob patterns for CLAUDE.md files to skip loading (helpful in monorepos).",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "claudeMdExcludes",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "packages/legacy/**/CLAUDE.md",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "plansDirectory",
        label: "Plans Directory",
        display_key: "plansDirectory",
        description: "Directory where Claude stores generated implementation plan files.",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "plansDirectory",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "~/.claude/plans",
            allow_empty: false,
            default_hint: Some("~/.claude/plans"),
        },
    },
    ClaudeSettingSpec {
        id: "cleanupPeriodDays",
        label: "Session Cleanup Period (Days)",
        display_key: "cleanupPeriodDays",
        description: "Days to retain transcripts, orphaned worktrees, tasks, shell snapshots, and backups (min 1).",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "cleanupPeriodDays",
        },
        kind: ClaudeSettingKind::Integer {
            min: Some(1),
            max: None,
            placeholder: "30",
            default_hint: Some(30),
        },
    },
    ClaudeSettingSpec {
        id: "fileCheckpointingEnabled",
        label: "File Checkpointing",
        display_key: "fileCheckpointingEnabled",
        description: "Snapshot edited files so /rewind can restore previous file states.",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "fileCheckpointingEnabled",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "respectGitignore",
        label: "Respect .gitignore",
        display_key: "respectGitignore",
        description: "Exclude .gitignore-matched files from @ file suggestions and file discovery.",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "respectGitignore",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "includeGitInstructions",
        label: "Include Git Instructions",
        display_key: "includeGitInstructions",
        description: "Include built-in git commit and PR workflow instructions in Claude's system prompt.",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "includeGitInstructions",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "attribution.commit",
        label: "Git Commit Attribution",
        display_key: "attribution.commit",
        description: "Custom commit attribution trailer text. Save an empty string to disable commit attribution.",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "attribution.commit",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "Leave blank to disable, or enter Co-Authored-By trailer",
            allow_empty: true,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "attribution.pr",
        label: "Pull Request Attribution",
        display_key: "attribution.pr",
        description: "Custom pull request attribution footer. Save an empty string to disable PR attribution.",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "attribution.pr",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "Leave blank to disable, or enter PR footer text",
            allow_empty: true,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "attribution.sessionUrl",
        label: "Session URL Attribution",
        display_key: "attribution.sessionUrl",
        description: "Append a claude.ai session link trailer on commits and pull request descriptions.",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "attribution.sessionUrl",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "includeCoAuthoredBy",
        label: "Include Co-Authored-By (Legacy)",
        display_key: "includeCoAuthoredBy",
        description: "Legacy boolean toggle for co-authored-by trailers (prefer attribution.commit / attribution.pr).",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "includeCoAuthoredBy",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "worktree.baseRef",
        label: "Worktree Base Ref",
        display_key: "worktree.baseRef",
        description: "Branch worktrees from origin/<default> ('fresh') or local HEAD ('head').",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "worktree.baseRef",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["fresh", "head"],
            allow_custom: false,
            default_hint: Some("fresh"),
        },
    },
    ClaudeSettingSpec {
        id: "worktree.bgIsolation",
        label: "Background Worktree Isolation",
        display_key: "worktree.bgIsolation",
        description: "Isolation mode for background sessions: 'worktree' or 'none'.",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "worktree.bgIsolation",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["worktree", "none"],
            allow_custom: false,
            default_hint: Some("worktree"),
        },
    },
    ClaudeSettingSpec {
        id: "worktree.sparsePaths",
        label: "Worktree Sparse Paths",
        display_key: "worktree.sparsePaths",
        description: "Directories checked out in each worktree via git sparse-checkout (cone mode).",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "worktree.sparsePaths",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "crates/app, docs",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "worktree.symlinkDirectories",
        label: "Worktree Symlink Directories",
        display_key: "worktree.symlinkDirectories",
        description: "Directories symlinked from the main checkout into each worktree (e.g. node_modules).",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "worktree.symlinkDirectories",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "node_modules, .venv",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "prUrlTemplate",
        label: "PR URL Template",
        display_key: "prUrlTemplate",
        description: "URL template for PR badges using {host}, {owner}, {repo}, {number} placeholders.",
        category: ClaudeSettingCategory::ContextAndGit,
        target: ClaudeSettingTarget::SettingsJson {
            path: "prUrlTemplate",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "https://{host}/{owner}/{repo}/pull/{number}",
            allow_empty: false,
            default_hint: None,
        },
    },
    // --- Category 6: Agents, Skills & Workflows (`--settings`) ---------------------------
    ClaudeSettingSpec {
        id: "agent",
        label: "Default Main-Thread Agent",
        display_key: "agent",
        description: "Name of a built-in or custom agent used for the main thread by default.",
        category: ClaudeSettingCategory::AgentsAndWorkflows,
        target: ClaudeSettingTarget::SettingsJson { path: "agent" },
        kind: ClaudeSettingKind::String {
            placeholder: "e.g. implementer",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "teammateMode",
        label: "Agent Team Display Mode",
        display_key: "teammateMode",
        description: "How agent team teammates display: auto, in-process, tmux, or iterm2.",
        category: ClaudeSettingCategory::AgentsAndWorkflows,
        target: ClaudeSettingTarget::SettingsJson {
            path: "teammateMode",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["auto", "in-process", "tmux", "iterm2"],
            allow_custom: false,
            default_hint: Some("in-process"),
        },
    },
    ClaudeSettingSpec {
        id: "workflowKeywordTriggerEnabled",
        label: "Workflow Keyword Trigger",
        display_key: "workflowKeywordTriggerEnabled",
        description: "Allow the 'ultracode' keyword in prompts to trigger dynamic workflows.",
        category: ClaudeSettingCategory::AgentsAndWorkflows,
        target: ClaudeSettingTarget::SettingsJson {
            path: "workflowKeywordTriggerEnabled",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "workflowSizeGuideline",
        label: "Workflow Size Guideline",
        display_key: "workflowSizeGuideline",
        description: "Target agent count guideline for dynamic workflows (unrestricted, small, medium, large).",
        category: ClaudeSettingCategory::AgentsAndWorkflows,
        target: ClaudeSettingTarget::SettingsJson {
            path: "workflowSizeGuideline",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["unrestricted", "small", "medium", "large"],
            allow_custom: false,
            default_hint: Some("unrestricted"),
        },
    },
    ClaudeSettingSpec {
        id: "disableWorkflows",
        label: "Disable Dynamic Workflows",
        display_key: "disableWorkflows",
        description: "Disable dynamic workflows and bundled workflow commands.",
        category: ClaudeSettingCategory::AgentsAndWorkflows,
        target: ClaudeSettingTarget::SettingsJson {
            path: "disableWorkflows",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "disableBundledSkills",
        label: "Disable Bundled Skills",
        display_key: "disableBundledSkills",
        description: "Remove skills and workflows that ship bundled with Claude Code.",
        category: ClaudeSettingCategory::AgentsAndWorkflows,
        target: ClaudeSettingTarget::SettingsJson {
            path: "disableBundledSkills",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "skillListingBudgetFraction",
        label: "Skill Listing Context Fraction",
        display_key: "skillListingBudgetFraction",
        description: "Fraction of model context window reserved for the skill listing (0.0–1.0; default 0.01).",
        category: ClaudeSettingCategory::AgentsAndWorkflows,
        target: ClaudeSettingTarget::SettingsJson {
            path: "skillListingBudgetFraction",
        },
        kind: ClaudeSettingKind::Number {
            min: Some(0.0),
            max: Some(1.0),
            placeholder: "0.01",
            default_hint: Some("0.01"),
        },
    },
    ClaudeSettingSpec {
        id: "skillListingMaxDescChars",
        label: "Skill Listing Max Description Chars",
        display_key: "skillListingMaxDescChars",
        description: "Per-skill character cap for description + when_to_use text sent to Claude.",
        category: ClaudeSettingCategory::AgentsAndWorkflows,
        target: ClaudeSettingTarget::SettingsJson {
            path: "skillListingMaxDescChars",
        },
        kind: ClaudeSettingKind::Integer {
            min: Some(1),
            max: None,
            placeholder: "1536",
            default_hint: Some(1536),
        },
    },
    ClaudeSettingSpec {
        id: "skillOverrides",
        label: "Skill Visibility Overrides",
        display_key: "skillOverrides",
        description: "JSON map of skill names to visibility overrides (e.g. on, off, hidden).",
        category: ClaudeSettingCategory::AgentsAndWorkflows,
        target: ClaudeSettingTarget::SettingsJson {
            path: "skillOverrides",
        },
        kind: ClaudeSettingKind::Json {
            placeholder: r#"{"my-skill":"off"}"#,
            expect_array: false,
        },
    },
    ClaudeSettingSpec {
        id: "enableArtifact",
        label: "Enable Artifact Tool",
        display_key: "enableArtifact",
        description: "Explicitly enable or disable the Artifact tool for publishing session output.",
        category: ClaudeSettingCategory::AgentsAndWorkflows,
        target: ClaudeSettingTarget::SettingsJson {
            path: "enableArtifact",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "disableArtifact",
        label: "Disable Artifact Tool",
        display_key: "disableArtifact",
        description: "Disable the Artifact tool (equivalent to CLAUDE_CODE_DISABLE_ARTIFACT=1).",
        category: ClaudeSettingCategory::AgentsAndWorkflows,
        target: ClaudeSettingTarget::SettingsJson {
            path: "disableArtifact",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    // --- Category 7: MCP, Plugins & Hooks (`--settings`) ---------------------------------
    ClaudeSettingSpec {
        id: "enableAllProjectMcpServers",
        label: "Auto-Approve All Project MCP Servers",
        display_key: "enableAllProjectMcpServers",
        description: "Automatically approve all MCP servers declared in a project's .mcp.json.",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "enableAllProjectMcpServers",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "enabledMcpjsonServers",
        label: "Approved .mcp.json Servers",
        display_key: "enabledMcpjsonServers",
        description: "Specific server names from .mcp.json to approve automatically.",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "enabledMcpjsonServers",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "postgres, github",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "disabledMcpjsonServers",
        label: "Rejected .mcp.json Servers",
        display_key: "disabledMcpjsonServers",
        description: "Specific server names from .mcp.json to block.",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "disabledMcpjsonServers",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "legacy-db",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "allowedMcpServers",
        label: "Allowed MCP Servers",
        display_key: "allowedMcpServers",
        description: "JSON array of allowed MCP server matchers (serverName, serverCommand, or serverUrl).",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "allowedMcpServers",
        },
        kind: ClaudeSettingKind::Json {
            placeholder: r#"[{"serverName":"github"}]"#,
            expect_array: true,
        },
    },
    ClaudeSettingSpec {
        id: "deniedMcpServers",
        label: "Denied MCP Servers",
        display_key: "deniedMcpServers",
        description: "JSON array of explicitly blocked MCP server matchers.",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "deniedMcpServers",
        },
        kind: ClaudeSettingKind::Json {
            placeholder: r#"[{"serverName":"untrusted"}]"#,
            expect_array: true,
        },
    },
    ClaudeSettingSpec {
        id: "disableClaudeAiConnectors",
        label: "Disable claude.ai Connectors",
        display_key: "disableClaudeAiConnectors",
        description: "Stop claude.ai MCP connectors from being auto-fetched or connected.",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "disableClaudeAiConnectors",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "enabledPlugins",
        label: "Enabled Plugins Map",
        display_key: "enabledPlugins",
        description: "JSON map of 'plugin@marketplace' identifiers to true/false enablement flags.",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "enabledPlugins",
        },
        kind: ClaudeSettingKind::Json {
            placeholder: r#"{"formatter@anthropic-tools":true}"#,
            expect_array: false,
        },
    },
    ClaudeSettingSpec {
        id: "pluginConfigs",
        label: "Plugin Configurations",
        display_key: "pluginConfigs",
        description: "JSON object of per-plugin user configs keyed by 'plugin@marketplace'.",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "pluginConfigs",
        },
        kind: ClaudeSettingKind::Json {
            placeholder: r#"{"plugin@marketplace":{}}"#,
            expect_array: false,
        },
    },
    ClaudeSettingSpec {
        id: "extraKnownMarketplaces",
        label: "Extra Plugin Marketplaces",
        display_key: "extraKnownMarketplaces",
        description: "JSON object defining extra plugin marketplaces available to the session.",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "extraKnownMarketplaces",
        },
        kind: ClaudeSettingKind::Json {
            placeholder: r#"{"internal":{"source":{"source":"github","repo":"org/plugins"}}}"#,
            expect_array: false,
        },
    },
    ClaudeSettingSpec {
        id: "skippedPlugins",
        label: "Skipped Plugins",
        display_key: "skippedPlugins",
        description: "Plugin IDs (plugin@marketplace) skipped from auto-install prompts.",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "skippedPlugins",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "example@marketplace",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "skippedMarketplaces",
        label: "Skipped Marketplaces",
        display_key: "skippedMarketplaces",
        description: "Marketplace names skipped from install prompts.",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "skippedMarketplaces",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "community-marketplace",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "disableAllHooks",
        label: "Disable All Hooks",
        display_key: "disableAllHooks",
        description: "Disable all lifecycle hooks and statusLine command execution.",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "disableAllHooks",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "allowedHttpHookUrls",
        label: "Allowed HTTP Hook URLs",
        display_key: "allowedHttpHookUrls",
        description: "URL patterns HTTP hooks are allowed to call (supports * wildcards).",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "allowedHttpHookUrls",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "https://hooks.internal.example/*",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "httpHookAllowedEnvVars",
        label: "HTTP Hook Allowed Env Vars",
        display_key: "httpHookAllowedEnvVars",
        description: "Environment variable names HTTP hooks may interpolate into request headers.",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson {
            path: "httpHookAllowedEnvVars",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "HOOK_AUTH_TOKEN",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "hooks",
        label: "Lifecycle Hooks JSON",
        display_key: "hooks",
        description: "JSON object configuring lifecycle hooks (PreToolUse, PostToolUse, Stop, SessionStart, etc.).",
        category: ClaudeSettingCategory::McpPluginsAndHooks,
        target: ClaudeSettingTarget::SettingsJson { path: "hooks" },
        kind: ClaudeSettingKind::Json {
            placeholder: r#"{"PreToolUse":[]}"#,
            expect_array: false,
        },
    },
    // --- Category 8: Auth, Cloud & Updates (`--settings`) --------------------------------
    ClaudeSettingSpec {
        id: "apiKeyHelper",
        label: "API Key Helper Script",
        display_key: "apiKeyHelper",
        description: "Path to a script or command that outputs an authentication value on stdout.",
        category: ClaudeSettingCategory::AuthAndCloud,
        target: ClaudeSettingTarget::SettingsJson {
            path: "apiKeyHelper",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "/usr/local/bin/get-anthropic-key.sh",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "awsAuthRefresh",
        label: "AWS Auth Refresh Command",
        display_key: "awsAuthRefresh",
        description: "Command run when AWS Bedrock credentials are expired or unavailable.",
        category: ClaudeSettingCategory::AuthAndCloud,
        target: ClaudeSettingTarget::SettingsJson {
            path: "awsAuthRefresh",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "aws sso login --profile dev",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "awsCredentialExport",
        label: "AWS Credential Export Command",
        display_key: "awsCredentialExport",
        description: "Command outputting AWS temporary credentials as JSON.",
        category: ClaudeSettingCategory::AuthAndCloud,
        target: ClaudeSettingTarget::SettingsJson {
            path: "awsCredentialExport",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "/usr/local/bin/export-aws-creds.sh",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "gcpAuthRefresh",
        label: "GCP Auth Refresh Command",
        display_key: "gcpAuthRefresh",
        description: "Command run when Google Cloud Vertex AI credentials are expired.",
        category: ClaudeSettingCategory::AuthAndCloud,
        target: ClaudeSettingTarget::SettingsJson {
            path: "gcpAuthRefresh",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "gcloud auth application-default login",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "otelHeadersHelper",
        label: "OTEL Headers Helper",
        display_key: "otelHeadersHelper",
        description: "Executable or command outputting OpenTelemetry headers as a JSON object.",
        category: ClaudeSettingCategory::AuthAndCloud,
        target: ClaudeSettingTarget::SettingsJson {
            path: "otelHeadersHelper",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "/usr/local/bin/otel-headers.sh",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "forceLoginMethod",
        label: "Force Login Method",
        display_key: "forceLoginMethod",
        description: "Restrict login to claudeai, console, or gateway accounts.",
        category: ClaudeSettingCategory::AuthAndCloud,
        target: ClaudeSettingTarget::SettingsJson {
            path: "forceLoginMethod",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["claudeai", "console", "gateway"],
            allow_custom: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "forceLoginGatewayUrl",
        label: "Force Login Gateway URL",
        display_key: "forceLoginGatewayUrl",
        description: "Pre-fill and lock the Cloud gateway URL on login.",
        category: ClaudeSettingCategory::AuthAndCloud,
        target: ClaudeSettingTarget::SettingsJson {
            path: "forceLoginGatewayUrl",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "https://gateway.example.com",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "forceLoginOrgUUID",
        label: "Force Login Org UUID(s)",
        display_key: "forceLoginOrgUUID",
        description: "Organization UUID (or comma-separated list of UUIDs) allowed for OAuth login.",
        category: ClaudeSettingCategory::AuthAndCloud,
        target: ClaudeSettingTarget::SettingsJson {
            path: "forceLoginOrgUUID",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
            single_as_string: true,
        },
    },
    ClaudeSettingSpec {
        id: "autoUpdatesChannel",
        label: "Auto-Updates Channel",
        display_key: "autoUpdatesChannel",
        description: "Claude Code release channel ('stable' or 'latest') for manual updates; Bruk always sets DISABLE_AUTOUPDATER=1 on spawn.",
        category: ClaudeSettingCategory::AuthAndCloud,
        target: ClaudeSettingTarget::SettingsJson {
            path: "autoUpdatesChannel",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["latest", "stable"],
            allow_custom: false,
            default_hint: Some("latest"),
        },
    },
    ClaudeSettingSpec {
        id: "minimumVersion",
        label: "Minimum CLI Version",
        display_key: "minimumVersion",
        description: "Minimum Claude Code version to stay on when switching channels.",
        category: ClaudeSettingCategory::AuthAndCloud,
        target: ClaudeSettingTarget::SettingsJson {
            path: "minimumVersion",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "2.1.300",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "processWrapper",
        label: "Background Process Wrapper",
        display_key: "processWrapper",
        description: "Launcher command placed in front of background processes Claude Code starts.",
        category: ClaudeSettingCategory::AuthAndCloud,
        target: ClaudeSettingTarget::SettingsJson {
            path: "processWrapper",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "/usr/local/bin/corp-wrapper",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "feedbackSurveyRate",
        label: "Feedback Survey Rate",
        display_key: "feedbackSurveyRate",
        description: "Probability (0.0–1.0) that session quality surveys appear (0 disables surveys).",
        category: ClaudeSettingCategory::AuthAndCloud,
        target: ClaudeSettingTarget::SettingsJson {
            path: "feedbackSurveyRate",
        },
        kind: ClaudeSettingKind::Number {
            min: Some(0.0),
            max: Some(1.0),
            placeholder: "0",
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "companyAnnouncements",
        label: "Startup Announcements",
        display_key: "companyAnnouncements",
        description: "Startup banner messages displayed when sessions launch.",
        category: ClaudeSettingCategory::AuthAndCloud,
        target: ClaudeSettingTarget::SettingsJson {
            path: "companyAnnouncements",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "Remember to run cargo test before opening a PR",
            single_as_string: false,
        },
    },
    // --- Category 9: Interface & Session (`--settings`) ----------------------------------
    ClaudeSettingSpec {
        id: "outputStyle",
        label: "Output Style",
        display_key: "outputStyle",
        description: "Assistant response style: default, Proactive, Explanatory, Learning, or a custom style name.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "outputStyle",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["default", "Proactive", "Explanatory", "Learning"],
            allow_custom: true,
            default_hint: Some("default"),
        },
    },
    ClaudeSettingSpec {
        id: "language",
        label: "Preferred Response Language",
        display_key: "language",
        description: "Language Claude responds in by default (e.g. english, japanese, spanish, french).",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson { path: "language" },
        kind: ClaudeSettingKind::String {
            placeholder: "english",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "defaultShell",
        label: "Default Input-Box Shell",
        display_key: "defaultShell",
        description: "Default shell used for input-box ! commands (bash or powershell).",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "defaultShell",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["bash", "powershell"],
            allow_custom: false,
            default_hint: Some("bash"),
        },
    },
    ClaudeSettingSpec {
        id: "editorMode",
        label: "Editor Mode",
        display_key: "editorMode",
        description: "Input prompt keybindings: normal or vim.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson { path: "editorMode" },
        kind: ClaudeSettingKind::Choice {
            options: &["normal", "vim"],
            allow_custom: false,
            default_hint: Some("normal"),
        },
    },
    ClaudeSettingSpec {
        id: "theme",
        label: "Color Theme",
        display_key: "theme",
        description: "Claude CLI color theme or custom:<slug> theme reference.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson { path: "theme" },
        kind: ClaudeSettingKind::Choice {
            options: &[
                "auto",
                "dark",
                "light",
                "dark-daltonized",
                "light-daltonized",
                "dark-ansi",
                "light-ansi",
            ],
            allow_custom: true,
            default_hint: Some("dark"),
        },
    },
    ClaudeSettingSpec {
        id: "tui",
        label: "TUI Rendering Mode",
        display_key: "tui",
        description: "Terminal renderer: 'fullscreen' (alt-screen) or 'default' (classic scrollback).",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson { path: "tui" },
        kind: ClaudeSettingKind::Choice {
            options: &["default", "fullscreen"],
            allow_custom: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "viewMode",
        label: "Transcript View Mode",
        display_key: "viewMode",
        description: "Transcript detail level: default, verbose, or focus.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson { path: "viewMode" },
        kind: ClaudeSettingKind::Choice {
            options: &["default", "verbose", "focus"],
            allow_custom: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "verbose",
        label: "Verbose Tool Output",
        display_key: "verbose",
        description: "Show full tool output instead of truncated summaries.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson { path: "verbose" },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "diffTool",
        label: "Diff Viewer Target",
        display_key: "diffTool",
        description: "Where to display file diffs when an IDE is connected: auto or terminal.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson { path: "diffTool" },
        kind: ClaudeSettingKind::Choice {
            options: &["auto", "terminal"],
            allow_custom: false,
            default_hint: Some("auto"),
        },
    },
    ClaudeSettingSpec {
        id: "autoConnectIde",
        label: "Auto-Connect IDE",
        display_key: "autoConnectIde",
        description: "Automatically connect to a running IDE when starting from an external terminal.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "autoConnectIde",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "autoInstallIdeExtension",
        label: "Auto-Install IDE Extension",
        display_key: "autoInstallIdeExtension",
        description: "Automatically install the Claude Code IDE extension when running inside VS Code.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "autoInstallIdeExtension",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "autoScrollEnabled",
        label: "Auto-Scroll Output",
        display_key: "autoScrollEnabled",
        description: "Follow new output to the bottom of the conversation in fullscreen rendering.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "autoScrollEnabled",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "awaySummaryEnabled",
        label: "Away Session Recap",
        display_key: "awaySummaryEnabled",
        description: "Show a one-line session recap after returning to the terminal.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "awaySummaryEnabled",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "showTurnDuration",
        label: "Show Turn Duration",
        display_key: "showTurnDuration",
        description: "Display turn duration messages after responses.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "showTurnDuration",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "showClearContextOnPlanAccept",
        label: "Clear Context Option on Plan Accept",
        display_key: "showClearContextOnPlanAccept",
        description: "Offer a 'clear context' choice in the plan approval dialog.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "showClearContextOnPlanAccept",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "respondToBashCommands",
        label: "Respond to ! Bash Commands",
        display_key: "respondToBashCommands",
        description: "Whether Claude replies after an input-box ! shell command finishes.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "respondToBashCommands",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "emojiCompletionEnabled",
        label: "Emoji Shortcode Completion",
        display_key: "emojiCompletionEnabled",
        description: "Suggest and expand :shortcode: emoji in the prompt input.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "emojiCompletionEnabled",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "externalEditorContext",
        label: "External Editor Context",
        display_key: "externalEditorContext",
        description: "Prepend Claude's previous response as commented context when pressing Ctrl+G.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "externalEditorContext",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "prefersReducedMotion",
        label: "Prefer Reduced Motion",
        display_key: "prefersReducedMotion",
        description: "Reduce or disable UI spinner, shimmer, and flash animations.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "prefersReducedMotion",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "axScreenReader",
        label: "Screen-Reader Friendly Output",
        display_key: "axScreenReader",
        description: "Render flat text output without decorative borders or animations.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "axScreenReader",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "syntaxHighlightingDisabled",
        label: "Disable Syntax Highlighting",
        display_key: "syntaxHighlightingDisabled",
        description: "Disable syntax highlighting in diffs, code blocks, and file previews.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "syntaxHighlightingDisabled",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "terminalProgressBarEnabled",
        label: "Terminal Progress Bar",
        display_key: "terminalProgressBarEnabled",
        description: "Show native terminal progress bar in supported terminals (iTerm2, Windows Terminal).",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "terminalProgressBarEnabled",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "wheelScrollAccelerationEnabled",
        label: "Wheel Scroll Acceleration",
        display_key: "wheelScrollAccelerationEnabled",
        description: "Accelerate mouse-wheel scroll speed during fast scrolls in fullscreen mode.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "wheelScrollAccelerationEnabled",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "preferredNotifChannel",
        label: "Notification Channel",
        display_key: "preferredNotifChannel",
        description: "Delivery method for task-complete and permission-prompt notifications.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "preferredNotifChannel",
        },
        kind: ClaudeSettingKind::Choice {
            options: &[
                "auto",
                "terminal_bell",
                "iterm2",
                "iterm2_with_bell",
                "kitty",
                "ghostty",
                "notifications_disabled",
            ],
            allow_custom: false,
            default_hint: Some("auto"),
        },
    },
    ClaudeSettingSpec {
        id: "askUserQuestionTimeout",
        label: "AskUserQuestion Idle Timeout",
        display_key: "askUserQuestionTimeout",
        description: "Idle timeout before an unanswered question dialog auto-continues.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "askUserQuestionTimeout",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["60s", "5m", "10m", "never"],
            allow_custom: false,
            default_hint: Some("never"),
        },
    },
    ClaudeSettingSpec {
        id: "remoteControlAtStartup",
        label: "Remote Control at Startup",
        display_key: "remoteControlAtStartup",
        description: "Connect Remote Control automatically when each interactive session starts.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "remoteControlAtStartup",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "disableRemoteControl",
        label: "Disable Remote Control",
        display_key: "disableRemoteControl",
        description: "Block Remote Control commands, auto-start, and in-session toggles.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "disableRemoteControl",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "agentPushNotifEnabled",
        label: "Proactive Push Notifications",
        display_key: "agentPushNotifEnabled",
        description: "Allow proactive phone push notifications via Remote Control when long tasks finish.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "agentPushNotifEnabled",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "inputNeededNotifEnabled",
        label: "Input-Needed Push Notifications",
        display_key: "inputNeededNotifEnabled",
        description: "Send a phone push notification via Remote Control when waiting on a prompt or question.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "inputNeededNotifEnabled",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "spinnerTipsEnabled",
        label: "Spinner Tips Enabled",
        display_key: "spinnerTipsEnabled",
        description: "Show tips in the working spinner.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "spinnerTipsEnabled",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(true),
        },
    },
    ClaudeSettingSpec {
        id: "spinnerTipsOverride.excludeDefault",
        label: "Exclude Default Spinner Tips",
        display_key: "spinnerTipsOverride.excludeDefault",
        description: "Show only custom spinner tips instead of merging with built-in tips.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "spinnerTipsOverride.excludeDefault",
        },
        kind: ClaudeSettingKind::Bool {
            default_hint: Some(false),
        },
    },
    ClaudeSettingSpec {
        id: "spinnerTipsOverride.tips",
        label: "Custom Spinner Tips",
        display_key: "spinnerTipsOverride.tips",
        description: "Custom tip strings displayed while Claude is working.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "spinnerTipsOverride.tips",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "Keep commits focused, Run tests early",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "spinnerVerbs.mode",
        label: "Spinner Verbs Mode",
        display_key: "spinnerVerbs.mode",
        description: "Combine custom spinner verbs with defaults ('append') or replace them ('replace').",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "spinnerVerbs.mode",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["append", "replace"],
            allow_custom: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "spinnerVerbs.verbs",
        label: "Custom Spinner Verbs",
        display_key: "spinnerVerbs.verbs",
        description: "Custom verbs shown in spinner progress text.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "spinnerVerbs.verbs",
        },
        kind: ClaudeSettingKind::StringList {
            placeholder: "Compiling, Analyzing, Forging",
            single_as_string: false,
        },
    },
    ClaudeSettingSpec {
        id: "statusLine.type",
        label: "Status Line Handler Type",
        display_key: "statusLine.type",
        description: "Status line handler type (must be 'command' when using statusLine.command).",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "statusLine.type",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["command"],
            allow_custom: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "statusLine.command",
        label: "Status Line Command",
        display_key: "statusLine.command",
        description: "Shell command or script rendering custom session status line output.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "statusLine.command",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "~/.claude/statusline.sh",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "statusLine.padding",
        label: "Status Line Padding",
        display_key: "statusLine.padding",
        description: "Extra horizontal spacing characters added to the status line.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "statusLine.padding",
        },
        kind: ClaudeSettingKind::Integer {
            min: Some(0),
            max: Some(64),
            placeholder: "0",
            default_hint: Some(0),
        },
    },
    ClaudeSettingSpec {
        id: "statusLine.refreshInterval",
        label: "Status Line Refresh Interval (s)",
        display_key: "statusLine.refreshInterval",
        description: "Re-run the status line command every N seconds in addition to event updates.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "statusLine.refreshInterval",
        },
        kind: ClaudeSettingKind::Integer {
            min: Some(1),
            max: None,
            placeholder: "5",
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "statusLine.hideVimModeIndicator",
        label: "Hide Built-In Vim Mode Indicator",
        display_key: "statusLine.hideVimModeIndicator",
        description: "Suppress the built-in vim mode indicator when your status line script draws its own.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "statusLine.hideVimModeIndicator",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "subagentStatusLine.type",
        label: "Subagent Status Line Type",
        display_key: "subagentStatusLine.type",
        description: "Handler type for subagent session status lines ('command').",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "subagentStatusLine.type",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["command"],
            allow_custom: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "subagentStatusLine.command",
        label: "Subagent Status Line Command",
        display_key: "subagentStatusLine.command",
        description: "Shell command rendering the status line for subagent sessions.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "subagentStatusLine.command",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "~/.claude/subagent-statusline.sh",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "fileSuggestion.type",
        label: "File Suggestion Handler Type",
        display_key: "fileSuggestion.type",
        description: "Handler type for custom @ file autocomplete ('command').",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "fileSuggestion.type",
        },
        kind: ClaudeSettingKind::Choice {
            options: &["command"],
            allow_custom: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "fileSuggestion.command",
        label: "File Suggestion Command",
        display_key: "fileSuggestion.command",
        description: "Shell command executed to generate custom @ file autocomplete suggestions.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "fileSuggestion.command",
        },
        kind: ClaudeSettingKind::String {
            placeholder: "fd --type f",
            allow_empty: false,
            default_hint: None,
        },
    },
    ClaudeSettingSpec {
        id: "footerLinksRegexes",
        label: "Footer Links Regexes",
        display_key: "footerLinksRegexes",
        description: "JSON array of regex rules rendering clickable badges in the footer.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "footerLinksRegexes",
        },
        kind: ClaudeSettingKind::Json {
            placeholder: r#"[{"pattern":"JIRA-\\d+","url":"https://jira.example.com/browse/$0"}]"#,
            expect_array: true,
        },
    },
    ClaudeSettingSpec {
        id: "voice.enabled",
        label: "Voice Dictation Enabled",
        display_key: "voice.enabled",
        description: "Enable push-to-talk voice dictation input.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "voice.enabled",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "voice.mode",
        label: "Voice Recording Mode",
        display_key: "voice.mode",
        description: "Voice dictation activation style: 'hold' (push-to-talk) or 'tap'.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson { path: "voice.mode" },
        kind: ClaudeSettingKind::Choice {
            options: &["hold", "tap"],
            allow_custom: false,
            default_hint: Some("hold"),
        },
    },
    ClaudeSettingSpec {
        id: "voice.autoSubmit",
        label: "Voice Auto-Submit",
        display_key: "voice.autoSubmit",
        description: "Auto-submit the prompt on key release in hold mode when transcript has 3+ words.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "voice.autoSubmit",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "voiceEnabled",
        label: "Voice Enabled (Legacy)",
        display_key: "voiceEnabled",
        description: "Legacy top-level alias for voice.enabled.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "voiceEnabled",
        },
        kind: ClaudeSettingKind::Bool { default_hint: None },
    },
    ClaudeSettingSpec {
        id: "vimInsertModeRemaps",
        label: "Vim Insert Mode Remaps",
        display_key: "vimInsertModeRemaps",
        description: "JSON map of two-key INSERT-mode sequences remapped to Escape in vim mode.",
        category: ClaudeSettingCategory::InterfaceAndSession,
        target: ClaudeSettingTarget::SettingsJson {
            path: "vimInsertModeRemaps",
        },
        kind: ClaudeSettingKind::Json {
            placeholder: r#"{"jk":"Escape"}"#,
            expect_array: false,
        },
    },
];

/// Curated quick-add presets for widely used Claude Code environment variables.
pub const CLAUDE_ENV_PRESETS: &[ClaudeEnvPreset] = &[
    ClaudeEnvPreset {
        name: "ANTHROPIC_BASE_URL",
        description: "Custom API endpoint URL for a proxy or gateway.",
        placeholder: "https://api.anthropic.com",
        suggested_value: "",
    },
    ClaudeEnvPreset {
        name: "ANTHROPIC_CUSTOM_HEADERS",
        description: "Custom HTTP headers for API requests ('Name: Value').",
        placeholder: "X-Custom-Header: value",
        suggested_value: "",
    },
    ClaudeEnvPreset {
        name: "ANTHROPIC_MODEL",
        description: "Override the active model via environment variable.",
        placeholder: "claude-sonnet-4-6",
        suggested_value: "",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CONFIG_DIR",
        description: "Override the Claude CLI configuration directory.",
        placeholder: "~/.claude",
        suggested_value: "",
    },
    ClaudeEnvPreset {
        name: "MAX_THINKING_TOKENS",
        description: "Token budget limit for extended thinking.",
        placeholder: "16000",
        suggested_value: "16000",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
        description: "Maximum output tokens per model response.",
        placeholder: "32000",
        suggested_value: "32000",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        description: "Cap the effective model context window in tokens.",
        placeholder: "200000",
        suggested_value: "200000",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_MAX_TURNS",
        description: "Maximum agentic turns allowed in a session.",
        placeholder: "25",
        suggested_value: "25",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_EFFORT_LEVEL",
        description: "Reasoning effort level override (low, medium, high, xhigh, max).",
        placeholder: "high",
        suggested_value: "high",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_SUBAGENT_MODEL",
        description: "Default model used by spawned subagents.",
        placeholder: "sonnet",
        suggested_value: "sonnet",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
        description: "Context capacity percentage (1–100) triggering auto-compaction.",
        placeholder: "80",
        suggested_value: "80",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
        description: "Context capacity for auto-compaction calculations in tokens.",
        placeholder: "150000",
        suggested_value: "150000",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_DISABLE_1M_CONTEXT",
        description: "Disable 1M context window models (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_DISABLE_ADAPTIVE_THINKING",
        description: "Disable adaptive reasoning (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_DISABLE_AUTO_MEMORY",
        description: "Disable automatic memory saves (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_DISABLE_FAST_MODE",
        description: "Disable fast mode (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
        description: "Disable telemetry, error reporting, and auto-updates at once (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS",
        description: "Enable experimental multi-agent teams (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_SIMPLE",
        description: "Run in minimal bare mode (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_SHELL",
        description: "Explicit shell path used for Bash tool invocations.",
        placeholder: "/bin/zsh",
        suggested_value: "/bin/zsh",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_TMPDIR",
        description: "Custom temporary directory for Claude subprocesses.",
        placeholder: "/tmp/claude",
        suggested_value: "",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR",
        description: "Reset working directory to project root after each Bash command (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "API_TIMEOUT_MS",
        description: "Timeout for Anthropic API requests in milliseconds (default 600000).",
        placeholder: "600000",
        suggested_value: "600000",
    },
    ClaudeEnvPreset {
        name: "BASH_DEFAULT_TIMEOUT_MS",
        description: "Default Bash tool command timeout in milliseconds (default 120000).",
        placeholder: "120000",
        suggested_value: "120000",
    },
    ClaudeEnvPreset {
        name: "BASH_MAX_TIMEOUT_MS",
        description: "Maximum Bash tool command timeout in milliseconds (default 600000).",
        placeholder: "600000",
        suggested_value: "600000",
    },
    ClaudeEnvPreset {
        name: "BASH_MAX_OUTPUT_LENGTH",
        description: "Maximum Bash output characters retained before truncation.",
        placeholder: "30000",
        suggested_value: "30000",
    },
    ClaudeEnvPreset {
        name: "MCP_TIMEOUT",
        description: "MCP server startup timeout in milliseconds.",
        placeholder: "30000",
        suggested_value: "30000",
    },
    ClaudeEnvPreset {
        name: "MCP_TOOL_TIMEOUT",
        description: "MCP tool execution timeout in milliseconds.",
        placeholder: "60000",
        suggested_value: "60000",
    },
    ClaudeEnvPreset {
        name: "MAX_MCP_OUTPUT_TOKENS",
        description: "Maximum tokens accepted from an MCP tool response.",
        placeholder: "25000",
        suggested_value: "25000",
    },
    ClaudeEnvPreset {
        name: "DISABLE_TELEMETRY",
        description: "Opt out of Claude Code telemetry (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "DISABLE_ERROR_REPORTING",
        description: "Opt out of crash and error reporting (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "DISABLE_PROMPT_CACHING",
        description: "Disable prompt caching for all models (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "DO_NOT_TRACK",
        description: "Standard opt-out flag disabling tracking/telemetry (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_USE_BEDROCK",
        description: "Route Claude Code requests through Amazon Bedrock (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_USE_VERTEX",
        description: "Route Claude Code requests through Google Cloud Vertex AI (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_USE_FOUNDRY",
        description: "Route Claude Code requests through Microsoft Foundry (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
        name: "AWS_REGION",
        description: "AWS region for Bedrock requests (e.g. us-east-1).",
        placeholder: "us-east-1",
        suggested_value: "us-east-1",
    },
    ClaudeEnvPreset {
        name: "CLOUD_ML_REGION",
        description: "Google Cloud region for Vertex AI requests (e.g. global or us-east5).",
        placeholder: "global",
        suggested_value: "global",
    },
    ClaudeEnvPreset {
        name: "CLAUDE_CODE_ENABLE_TELEMETRY",
        description: "Enable OpenTelemetry metrics/logs/traces export (set to 1).",
        placeholder: "1",
        suggested_value: "1",
    },
    ClaudeEnvPreset {
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
        for spec in CLAUDE_SETTING_SPECS {
            assert!(
                seen.insert(spec.id),
                "duplicate ClaudeSettingSpec id: {}",
                spec.id
            );
            assert!(!spec.label.is_empty());
            assert!(!spec.display_key.is_empty());
            assert!(!spec.description.is_empty());
        }

        for category in ClaudeSettingCategory::ALL {
            if *category == ClaudeSettingCategory::EnvironmentAndRaw {
                assert!(!CLAUDE_ENV_PRESETS.is_empty());
            } else {
                assert!(
                    !claude_setting_specs_for_category(*category).is_empty(),
                    "category {:?} should not be empty",
                    category
                );
            }
        }
    }

    #[test]
    fn builds_nested_settings_json_and_merges_extra_raw_json() {
        let mut settings = ClaudeSpawnSettings::default();
        settings
            .set_value("effortLevel", ClaudeSettingValue::String("high".to_owned()))
            .unwrap();
        settings
            .set_value("alwaysThinkingEnabled", ClaudeSettingValue::Bool(true))
            .unwrap();
        settings
            .set_value(
                "permissions.defaultMode",
                ClaudeSettingValue::String("acceptEdits".to_owned()),
            )
            .unwrap();
        settings
            .set_from_text("permissions.allow", "Bash(cargo test*), Read(*)")
            .unwrap();
        settings
            .set_value("sandbox.enabled", ClaudeSettingValue::Bool(true))
            .unwrap();
        settings
            .set_from_text("sandbox.network.httpProxyPort", "8080")
            .unwrap();
        settings.set_from_text("attribution.commit", "").unwrap();
        settings
            .set_from_text("forceLoginOrgUUID", "11111111-2222-3333-4444-555555555555")
            .unwrap();
        settings
            .set_extra_settings_json(
                r#"{"sandbox":{"network":{"allowLocalBinding":true}},"customExperimentalFlag":42}"#,
            )
            .unwrap();

        let json = settings.build_settings_json().unwrap().unwrap();
        assert_eq!(json["effortLevel"], "high");
        assert_eq!(json["alwaysThinkingEnabled"], true);
        assert_eq!(json["permissions"]["defaultMode"], "acceptEdits");
        assert_eq!(
            json["permissions"]["allow"],
            serde_json::json!(["Bash(cargo test*)", "Read(*)"])
        );
        assert_eq!(json["sandbox"]["enabled"], true);
        assert_eq!(json["sandbox"]["network"]["httpProxyPort"], 8080);
        assert_eq!(json["sandbox"]["network"]["allowLocalBinding"], true);
        assert_eq!(json["attribution"]["commit"], "");
        assert_eq!(
            json["forceLoginOrgUUID"],
            "11111111-2222-3333-4444-555555555555"
        );
        assert_eq!(json["customExperimentalFlag"], 42);
    }

    #[test]
    fn validates_numbers_integers_json_env_vars_and_extra_args() {
        let mut settings = ClaudeSpawnSettings::default();

        assert!(settings.set_from_text("cleanupPeriodDays", "0").is_err());
        assert!(settings.set_from_text("cleanupPeriodDays", "14").is_ok());

        assert!(
            settings
                .set_from_text("sandbox.network.httpProxyPort", "70000")
                .is_err()
        );
        assert!(
            settings
                .set_from_text("sandbox.network.httpProxyPort", "8080")
                .is_ok()
        );

        assert!(settings.set_from_text("feedbackSurveyRate", "1.5").is_err());
        assert!(settings.set_from_text("feedbackSurveyRate", "0.05").is_ok());

        assert!(settings.set_from_text("effortLevel", "invalid").is_err());
        assert!(settings.set_from_text("effortLevel", "xhigh").is_ok());

        // Theme allows custom:<slug>.
        assert!(settings.set_from_text("theme", "custom:oceanic").is_ok());

        // JSON object vs JSON array validation.
        assert!(settings.set_from_text("hooks", "[1, 2]").is_err());
        assert!(settings.set_from_text("hooks", r#"{"Stop":[]}"#).is_ok());
        assert!(
            settings
                .set_from_text("allowedMcpServers", r#"{"bad":true}"#)
                .is_err()
        );
        assert!(
            settings
                .set_from_text("allowedMcpServers", r#"[{"serverName":"github"}]"#)
                .is_ok()
        );

        // Env var validation.
        assert!(settings.set_env_var("", "1").is_err());
        assert!(settings.set_env_var("123BAD", "1").is_err());
        assert!(settings.set_env_var("BAD-NAME", "1").is_err());
        assert!(settings.set_env_var("DISABLE_TELEMETRY", "1").is_ok());

        // Extra CLI args shell quoting validation.
        assert!(
            settings
                .set_extra_cli_args("--append-system-prompt \"unclosed")
                .is_err()
        );
        assert!(
            settings
                .set_extra_cli_args("--custom-flag 'hello world' --retries 3")
                .is_ok()
        );

        // Extra JSON must be an object.
        assert!(settings.set_extra_settings_json("[1,2,3]").is_err());
        assert!(
            settings
                .set_extra_settings_json(r#"{"customKey":"ok"}"#)
                .is_ok()
        );
    }

    #[test]
    fn builds_spawn_plan_with_all_cli_flag_encodings_and_env() {
        let mut settings = ClaudeSpawnSettings::default();
        settings
            .set_value(
                "cli.permission-mode",
                ClaudeSettingValue::String("acceptEdits".to_owned()),
            )
            .unwrap();
        settings
            .set_value("cli.effort", ClaudeSettingValue::String("xhigh".to_owned()))
            .unwrap();
        settings
            .set_from_text("cli.add-dir", "/shared/a, /shared/b")
            .unwrap();
        settings
            .set_from_text("cli.plugin-dir", "/plugins/one, /plugins/two.zip")
            .unwrap();
        settings
            .set_value("cli.chrome", ClaudeSettingValue::Bool(false))
            .unwrap();
        settings
            .set_value("cli.bare", ClaudeSettingValue::Bool(true))
            .unwrap();
        settings
            .set_value("cli.prompt-suggestions", ClaudeSettingValue::Bool(true))
            .unwrap();
        settings.set_from_text("cli.debug", "hooks,mcp").unwrap();
        settings.set_from_text("cli.worktree", "").unwrap();
        settings
            .set_value("alwaysThinkingEnabled", ClaudeSettingValue::Bool(true))
            .unwrap();
        settings.set_env_var("DISABLE_TELEMETRY", "1").unwrap();
        settings
            .set_extra_cli_args("--extra-opt 'two words'")
            .unwrap();

        let plan = build_claude_spawn_plan(
            "/usr/local/bin/claude",
            Some("claude-sonnet-4-6"),
            &settings,
            &ClaudeSpawnOptions {
                working_dir: Some(PathBuf::from("/workspace/repo")),
                model_override: None,
                print_mode: true,
                stream_json: false,
                session_name: Some("fix-42".to_owned()),
                prompt: Some("Run tests".to_owned()),
            },
        )
        .unwrap();

        assert_eq!(plan.binary_path, PathBuf::from("/usr/local/bin/claude"));
        assert_eq!(
            plan.env.get("DISABLE_TELEMETRY").map(String::as_str),
            Some("1")
        );
        assert!(plan.args.contains(&"--print".to_owned()));
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--model", "claude-sonnet-4-6"])
        );
        assert!(plan.args.windows(2).any(|w| w == ["--name", "fix-42"]));
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--permission-mode", "acceptEdits"])
        );
        assert!(plan.args.windows(2).any(|w| w == ["--effort", "xhigh"]));
        assert!(
            plan.args
                .windows(3)
                .any(|w| w == ["--add-dir", "/shared/a", "/shared/b"])
        );
        assert!(plan.args.windows(4).any(|w| w
            == [
                "--plugin-dir",
                "/plugins/one",
                "--plugin-dir",
                "/plugins/two.zip"
            ]));
        assert!(plan.args.contains(&"--no-chrome".to_owned()));
        assert!(plan.args.contains(&"--bare".to_owned()));
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--prompt-suggestions", "true"])
        );
        assert!(plan.args.windows(2).any(|w| w == ["--debug", "hooks,mcp"]));
        assert!(plan.args.contains(&"--worktree".to_owned()));
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--extra-opt", "two words"])
        );
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--settings", r#"{"alwaysThinkingEnabled":true}"#])
        );
        assert_eq!(plan.args.last().map(String::as_str), Some("Run tests"));

        let preview = plan.preview_command_line();
        assert!(
            preview.starts_with("DISABLE_AUTOUPDATER=1 DISABLE_TELEMETRY=1 /usr/local/bin/claude")
        );
        assert!(preview.contains("'two words'"));
    }

    #[test]
    fn always_forces_disable_autoupdater_env_even_with_defaults_or_override_attempts() {
        let mut settings = ClaudeSpawnSettings::default();
        assert!(matches!(
            settings.set_env_var("DISABLE_AUTOUPDATER", "0"),
            Err(ClaudeSettingsError::ReservedEnvVar(_))
        ));

        // Even with zero user overrides, DISABLE_AUTOUPDATER=1 is always set.
        let default_plan = build_claude_spawn_plan(
            "/usr/local/bin/claude",
            None,
            &settings,
            &ClaudeSpawnOptions::default(),
        )
        .unwrap();
        assert_eq!(
            default_plan
                .env
                .get("DISABLE_AUTOUPDATER")
                .map(String::as_str),
            Some("1")
        );
        assert!(
            default_plan
                .preview_command_line()
                .starts_with("DISABLE_AUTOUPDATER=1 ")
        );

        // Even if legacy/hand-edited data tries to unset it in env_vars or extra_settings_json.env,
        // Bruk still enforces DISABLE_AUTOUPDATER=1 in both process env and --settings JSON.
        settings
            .env_vars
            .insert("DISABLE_AUTOUPDATER".to_owned(), "0".to_owned());
        settings
            .set_extra_settings_json(r#"{"env":{"DISABLE_AUTOUPDATER":"0","CUSTOM":"ok"}}"#)
            .unwrap();
        let forced_plan = build_claude_spawn_plan(
            "/usr/local/bin/claude",
            None,
            &settings,
            &ClaudeSpawnOptions::default(),
        )
        .unwrap();
        assert_eq!(
            forced_plan
                .env
                .get("DISABLE_AUTOUPDATER")
                .map(String::as_str),
            Some("1")
        );
        let json = forced_plan.settings_json.unwrap();
        assert_eq!(json["env"]["DISABLE_AUTOUPDATER"], "1");
        assert_eq!(json["env"]["CUSTOM"], "ok");
    }

    #[cfg(unix)]
    #[test]
    fn spawned_command_delivers_flags_settings_json_and_env_to_child_process() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let fake_claude = temp.path().join("claude");
        std::fs::write(
            &fake_claude,
            "#!/bin/sh\necho \"AUTOUPDATE=$DISABLE_AUTOUPDATER\"\necho \"ENV=$DISABLE_TELEMETRY\"\nfor a in \"$@\"; do echo \"ARG=$a\"; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_claude, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut settings = ClaudeSpawnSettings::default();
        settings
            .set_value("cli.effort", ClaudeSettingValue::String("high".to_owned()))
            .unwrap();
        settings
            .set_value("sandbox.enabled", ClaudeSettingValue::Bool(true))
            .unwrap();
        settings.set_env_var("DISABLE_TELEMETRY", "1").unwrap();

        let output = build_claude_command(
            &fake_claude,
            Some("sonnet"),
            &settings,
            &ClaudeSpawnOptions {
                print_mode: true,
                prompt: Some("hello".to_owned()),
                ..Default::default()
            },
        )
        .unwrap()
        .output()
        .unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("AUTOUPDATE=1\n"));
        assert!(stdout.contains("ENV=1\n"));
        assert!(stdout.contains("ARG=--print\n"));
        assert!(stdout.contains("ARG=--model\nARG=sonnet\n"));
        assert!(stdout.contains("ARG=--effort\nARG=high\n"));
        assert!(stdout.contains("ARG=--settings\nARG={\"sandbox\":{\"enabled\":true}}\n"));
        assert!(stdout.contains("ARG=hello\n"));
    }
}
