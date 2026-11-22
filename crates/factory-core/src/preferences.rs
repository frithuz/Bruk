//! Application preferences, provider account configuration, and validation logic.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::claude_cli::{
    ClaudeCliDetails, ClaudeCliProbe, ClaudeModelOption, ClaudeUsageSummary, ClaudeUsageWindow,
};

/// Top-level toolbar tabs inside the macOS Preferences window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferencesTab {
    General,
    Appearance,
    #[default]
    Providers,
    Machines,
    Extensions,
    Advanced,
}

impl PreferencesTab {
    pub const ALL: &'static [PreferencesTab] = &[
        Self::General,
        Self::Appearance,
        Self::Providers,
        Self::Machines,
        Self::Extensions,
        Self::Advanced,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Providers => "Providers",
            Self::Machines => "Machines",
            Self::Extensions => "Extensions",
            Self::Advanced => "Advanced",
        }
    }
}

/// Color scheme preference applied to the desktop application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceMode {
    #[default]
    System,
    Light,
    Dark,
}

impl AppearanceMode {
    pub const ALL: &'static [AppearanceMode] = &[Self::System, Self::Light, Self::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// Which main view opens by default when the application launches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultLandingView {
    #[default]
    Dashboard,
    Inbox,
    Machines,
    Workspace,
}

impl DefaultLandingView {
    pub const ALL: &'static [DefaultLandingView] = &[
        Self::Dashboard,
        Self::Inbox,
        Self::Machines,
        Self::Workspace,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Inbox => "Inbox",
            Self::Machines => "Machines",
            Self::Workspace => "Workspace",
        }
    }
}

/// Supported AI provider families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Claude,
    Codex,
    OpenRouter,
}

impl ProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::OpenRouter => "OpenRouter",
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            Self::Claude => "Cl",
            Self::Codex => "Cx",
            Self::OpenRouter => "OR",
        }
    }

    pub fn group_hint(self) -> &'static str {
        match self {
            Self::Claude | Self::Codex => "one account",
            Self::OpenRouter => "one or more keys",
        }
    }
}

/// Outcome of the most recent connectivity or credential check for a provider.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "state", content = "message", rename_all = "snake_case")]
pub enum ConnectionTestState {
    #[default]
    Untested,
    Works,
    Failed(String),
}

fn default_true() -> bool {
    true
}

/// Single-account CLI provider configuration (Claude or Codex).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliAccount {
    pub id: String,
    pub provider: ProviderKind,
    pub display_name: String,
    pub email: String,
    pub plan: String,
    pub connected: bool,
    pub cli_binary: String,
    /// Optional user-specified path to the CLI binary when it is not on the default `PATH`.
    #[serde(default)]
    pub custom_cli_path: Option<String>,
    /// Whether the CLI executable was found on the system (`PATH` or `custom_cli_path`).
    #[serde(default = "default_true")]
    pub binary_found: bool,
    /// Whether this provider uses the user's existing system CLI login instead of an isolated config.
    #[serde(default)]
    pub uses_system_login: bool,
    pub config_dir: String,
    pub default_model: String,
    /// Model families unchecked in the Model Family Filter (e.g. `["Fable"]`).
    #[serde(default)]
    pub hidden_model_families: Vec<String>,
    /// Live model choices queried from the CLI (never persisted so upgrades stay fresh).
    #[serde(skip)]
    pub available_models: Vec<ClaudeModelOption>,
    /// Live plan usage/rate-limit windows queried from the CLI (never persisted).
    #[serde(skip)]
    pub usage: Option<ClaudeUsageSummary>,
    /// Optional non-fatal note if querying models/usage from the CLI returned a warning.
    #[serde(skip)]
    pub usage_warning: Option<String>,
    pub test_state: ConnectionTestState,
    pub used_by_projects: Vec<String>,
}

impl CliAccount {
    /// Compact subtitle displayed beneath the provider name in the account list.
    pub fn list_subtitle(&self) -> String {
        if !self.binary_found {
            return "CLI not found".to_owned();
        }
        if !self.connected {
            return "Not signed in".to_owned();
        }
        match (self.email.trim(), self.plan.trim()) {
            ("", "") => "Signed in".to_owned(),
            (email, "") => email.to_owned(),
            ("", plan) => plan.to_owned(),
            (email, plan) => format!("{email} · {plan}"),
        }
    }

    /// Subtitle displayed beneath the title in the detail header.
    pub fn header_subtitle(&self) -> String {
        if self.uses_system_login {
            format!("{} · System CLI login", self.provider.label())
        } else {
            format!("{} · CLI account", self.provider.label())
        }
    }

    /// Returns whether the given model family is enabled (visible) in the model selection view.
    pub fn is_model_family_visible(&self, family: &str) -> bool {
        !self
            .hidden_model_families
            .iter()
            .any(|hidden| hidden.eq_ignore_ascii_case(family))
    }

    /// Returns the distinct model families available for filtering, preserving CLI discovery order.
    pub fn available_model_families(&self) -> Vec<String> {
        let mut families = Vec::new();
        let mut push_unique = |family: String| {
            if !family.is_empty()
                && !families
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(&family))
            {
                families.push(family);
            }
        };

        if !self.available_models.is_empty() {
            for model in &self.available_models {
                push_unique(model.family());
            }
        } else if self.provider == ProviderKind::Claude {
            for fallback in ["Opus", "Sonnet", "Haiku"] {
                push_unique(fallback.to_owned());
            }
        }

        // Ensure previously hidden families remain toggleable even before live models finish loading.
        for hidden in &self.hidden_model_families {
            push_unique(hidden.clone());
        }

        families
    }

    /// Returns only the CLI-reported models whose family is currently enabled by the filter.
    pub fn visible_available_models(&self) -> Vec<ClaudeModelOption> {
        self.available_models
            .iter()
            .filter(|model| self.is_model_family_visible(&model.family()))
            .cloned()
            .collect()
    }
}

/// OpenRouter API key configuration stored as metadata (secret stays in macOS Keychain).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRouterKey {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub key_last4: String,
    pub monthly_cap_cents: Option<u64>,
    pub monthly_used_cents: u64,
    pub cap_note: String,
    pub default_model: String,
    pub is_default: bool,
    pub used_by_projects: Vec<String>,
    pub test_state: ConnectionTestState,
}

impl OpenRouterKey {
    /// Masked API key display (never exposes raw credentials).
    pub fn masked_key(&self) -> String {
        format!("{}••••••••••••{}", self.key_prefix, self.key_last4)
    }

    /// Compact subtitle displayed in the left provider account list.
    pub fn list_subtitle(&self) -> String {
        let cap_text = match self.monthly_cap_cents {
            Some(cents) => format!("{} a month", format_whole_or_cents(cents)),
            None => "No monthly cap".to_owned(),
        };
        format!("Key ••{} · {cap_text}", self.key_last4)
    }

    /// Summary string displayed in the "Monthly spending cap" row.
    pub fn cap_summary(&self) -> String {
        let used = format_dollars_cents(self.monthly_used_cents);
        match self.monthly_cap_cents {
            Some(cap) => format!("{} · {used} used", format_dollars_cents(cap)),
            None => format!("No cap · {used} used"),
        }
    }

    /// Formatted project usage string for the "Used by" card.
    pub fn used_by_projects_display(&self) -> String {
        if self.used_by_projects.is_empty() {
            "None".to_owned()
        } else {
            self.used_by_projects.join(" · ")
        }
    }
}

/// General startup and workflow behavior preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub default_landing_view: DefaultLandingView,
    pub default_projects_dir: String,
    pub confirm_before_cancel: bool,
    pub notify_on_human_gate: bool,
    pub notify_on_run_failure: bool,
    pub auto_check_updates: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            default_landing_view: DefaultLandingView::Dashboard,
            default_projects_dir: "~/src".to_owned(),
            confirm_before_cancel: true,
            notify_on_human_gate: true,
            notify_on_run_failure: true,
            auto_check_updates: true,
        }
    }
}

/// Visual appearance preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearanceSettings {
    pub mode: AppearanceMode,
    pub compact_sidebar: bool,
    pub monospace_font_size: u8,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            mode: AppearanceMode::System,
            compact_sidebar: false,
            monospace_font_size: 12,
        }
    }
}

/// Compute machine execution preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachinesSettings {
    pub max_concurrent_runs: u32,
    pub heartbeat_interval_secs: u32,
    pub auto_reconnect_runners: bool,
    pub prefer_local_machine: bool,
}

impl Default for MachinesSettings {
    fn default() -> Self {
        Self {
            max_concurrent_runs: 3,
            heartbeat_interval_secs: 15,
            auto_reconnect_runners: true,
            prefer_local_machine: true,
        }
    }
}

/// One toggleable integration or extension entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub enabled: bool,
}

/// Extensions and tool integration preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionsSettings {
    pub allow_project_mcp_servers: bool,
    pub entries: Vec<ExtensionEntry>,
}

impl Default for ExtensionsSettings {
    fn default() -> Self {
        Self::empty()
    }
}

impl ExtensionsSettings {
    /// Clean default with no mock extensions installed.
    pub fn empty() -> Self {
        Self {
            allow_project_mcp_servers: true,
            entries: Vec::new(),
        }
    }

    /// Sample extensions populated in Demo Mode.
    pub fn sample() -> Self {
        Self {
            allow_project_mcp_servers: true,
            entries: vec![
                ExtensionEntry {
                    id: "github".to_owned(),
                    name: "GitHub Issues & Pull Requests".to_owned(),
                    description: "Sync issues, open draft PRs, and monitor CI checks.".to_owned(),
                    version: "1.2.0".to_owned(),
                    enabled: true,
                },
                ExtensionEntry {
                    id: "itsaplan".to_owned(),
                    name: "itsaplan Issue Tracker".to_owned(),
                    description: "Pull project tasks and update issue status on run completion."
                        .to_owned(),
                    version: "0.9.4".to_owned(),
                    enabled: true,
                },
                ExtensionEntry {
                    id: "git-worktrees".to_owned(),
                    name: "Isolated Git Worktrees".to_owned(),
                    description: "Run parallel agent workflows in clean per-run worktrees."
                        .to_owned(),
                    version: "1.0.0".to_owned(),
                    enabled: true,
                },
                ExtensionEntry {
                    id: "mcp-bridge".to_owned(),
                    name: "Local MCP Tool Bridge".to_owned(),
                    description: "Expose local developer tools to agent steps over stdio."
                        .to_owned(),
                    version: "0.4.1".to_owned(),
                    enabled: false,
                },
            ],
        }
    }
}

/// Advanced storage, logging, and diagnostic settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvancedSettings {
    pub isolated_cli_root: String,
    pub verbose_step_logs: bool,
    pub keep_worktrees_on_failure: bool,
    pub log_retention_days: u32,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            isolated_cli_root: "~/Library/Application Support/software-factory/providers"
                .to_owned(),
            verbose_step_logs: false,
            keep_worktrees_on_failure: true,
            log_retention_days: 30,
        }
    }
}

/// Validation and lookup errors returned when mutating [`AppPreferences`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PreferencesError {
    #[error("name cannot be empty")]
    EmptyName,
    #[error("a key named '{0}' already exists")]
    DuplicateKeyName(String),
    #[error("API key must be at least 8 characters")]
    InvalidApiKey,
    #[error("model identifier cannot be empty")]
    EmptyModel,
    #[error("spending cap must be a valid non-negative dollar amount")]
    InvalidSpendingCap,
    #[error("provider account '{0}' was not found")]
    ProviderNotFound(String),
}

/// Current persisted preferences schema version.
///
/// - Version 2 separates real user preferences from Demo Mode fixtures; older
///   pre-Demo-Mode rows (`schema_version < 2`) were pre-seeded with mock provider
///   accounts and extensions and are automatically cleaned on load.
/// - Version 3 makes the Codex CLI provider opt-in (`Option<CliAccount>`); v2
///   rows that only carried the default unconnected Codex placeholder are cleared
///   automatically while preserving Claude, OpenRouter, and all other settings.
pub const PREFERENCES_SCHEMA_VERSION: u32 = 3;

/// Complete persisted application preferences aggregate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppPreferences {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub demo_mode: bool,
    pub general: GeneralSettings,
    pub appearance: AppearanceSettings,
    pub claude: CliAccount,
    #[serde(default)]
    pub codex: Option<CliAccount>,
    pub openrouter_keys: Vec<OpenRouterKey>,
    pub machines: MachinesSettings,
    pub extensions: ExtensionsSettings,
    pub advanced: AdvancedSettings,
    #[serde(default = "default_next_key_seq")]
    next_key_seq: u64,
}

fn default_next_key_seq() -> u64 {
    3
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self::empty()
    }
}

impl AppPreferences {
    /// Clean initial preferences with no fake accounts, keys, or demo data.
    pub fn empty() -> Self {
        Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            demo_mode: false,
            general: GeneralSettings::default(),
            appearance: AppearanceSettings::default(),
            claude: CliAccount {
                id: "claude".to_owned(),
                provider: ProviderKind::Claude,
                display_name: "Claude".to_owned(),
                email: String::new(),
                plan: String::new(),
                connected: false,
                cli_binary: "claude".to_owned(),
                custom_cli_path: None,
                binary_found: false,
                uses_system_login: true,
                config_dir: "~/.claude".to_owned(),
                default_model: "claude-sonnet-4-6".to_owned(),
                hidden_model_families: Vec::new(),
                available_models: Vec::new(),
                usage: None,
                usage_warning: None,
                test_state: ConnectionTestState::Untested,
                used_by_projects: Vec::new(),
            },
            codex: None,
            openrouter_keys: Vec::new(),
            machines: MachinesSettings::default(),
            extensions: ExtensionsSettings::empty(),
            advanced: AdvancedSettings::default(),
            next_key_seq: 1,
        }
    }

    /// Sample preferences pre-populated to match the reference macOS Preferences design in Demo Mode.
    pub fn sample() -> Self {
        Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            demo_mode: false,
            general: GeneralSettings::default(),
            appearance: AppearanceSettings::default(),
            claude: CliAccount {
                id: "claude".to_owned(),
                provider: ProviderKind::Claude,
                display_name: "Claude".to_owned(),
                email: "jonathan@garbee.me".to_owned(),
                plan: "Max".to_owned(),
                connected: true,
                cli_binary: "claude".to_owned(),
                custom_cli_path: None,
                binary_found: true,
                uses_system_login: true,
                config_dir: "~/.claude".to_owned(),
                default_model: "claude-sonnet-4-6".to_owned(),
                hidden_model_families: Vec::new(),
                available_models: vec![
                    ClaudeModelOption {
                        value: "opus".to_owned(),
                        resolved_model: "claude-opus-5-5".to_owned(),
                        display_name: "Opus 5.5".to_owned(),
                        description: "For complex work and everyday tasks".to_owned(),
                    },
                    ClaudeModelOption {
                        value: "fable".to_owned(),
                        resolved_model: "claude-fable-5-2".to_owned(),
                        display_name: "Fable 5.2".to_owned(),
                        description: "For your toughest challenges".to_owned(),
                    },
                    ClaudeModelOption {
                        value: "sonnet".to_owned(),
                        resolved_model: "claude-sonnet-5-5".to_owned(),
                        display_name: "Sonnet 5.5".to_owned(),
                        description: "Most efficient for simpler tasks".to_owned(),
                    },
                    ClaudeModelOption {
                        value: "haiku".to_owned(),
                        resolved_model: "claude-haiku-5-5".to_owned(),
                        display_name: "Haiku 5.5".to_owned(),
                        description: "Fastest for quick answers".to_owned(),
                    },
                ],
                usage: Some(ClaudeUsageSummary {
                    rate_limits_available: true,
                    windows: vec![
                        ClaudeUsageWindow {
                            label: "Current session (5h)".to_owned(),
                            used_percent: 18,
                            remaining_percent: 82,
                            resets_at: Some("2026-11-23T00:00:00+00:00".to_owned()),
                            severity: "normal".to_owned(),
                        },
                        ClaudeUsageWindow {
                            label: "Weekly (all models)".to_owned(),
                            used_percent: 9,
                            remaining_percent: 91,
                            resets_at: Some("2026-11-29T20:00:00+00:00".to_owned()),
                            severity: "normal".to_owned(),
                        },
                    ],
                    extra_usage_note: None,
                }),
                usage_warning: None,
                test_state: ConnectionTestState::Works,
                used_by_projects: vec![
                    "api-gateway".to_owned(),
                    "marketing-site".to_owned(),
                    "mobile-app".to_owned(),
                ],
            },
            codex: Some(CliAccount {
                id: "codex".to_owned(),
                provider: ProviderKind::Codex,
                display_name: "Codex".to_owned(),
                email: "jonathan@garbee.me".to_owned(),
                plan: "Pro".to_owned(),
                connected: true,
                cli_binary: "codex".to_owned(),
                custom_cli_path: None,
                binary_found: true,
                uses_system_login: false,
                config_dir: "~/Library/Application Support/software-factory/providers/codex"
                    .to_owned(),
                default_model: "gpt-5.3-codex".to_owned(),
                hidden_model_families: Vec::new(),
                available_models: Vec::new(),
                usage: None,
                usage_warning: None,
                test_state: ConnectionTestState::Works,
                used_by_projects: vec!["api-gateway".to_owned(), "mobile-app".to_owned()],
            }),
            openrouter_keys: vec![
                OpenRouterKey {
                    id: "openrouter-personal".to_owned(),
                    name: "Personal".to_owned(),
                    key_prefix: "sk-or-".to_owned(),
                    key_last4: "3f9a".to_owned(),
                    monthly_cap_cents: Some(2_500),
                    monthly_used_cents: 890,
                    cap_note: "Configured on your personal OpenRouter account. Bruk stops starting OpenRouter steps on this key when the cap is reached.".to_owned(),
                    default_model: "openrouter/auto".to_owned(),
                    is_default: true,
                    used_by_projects: vec!["mobile-app".to_owned()],
                    test_state: ConnectionTestState::Works,
                },
                OpenRouterKey {
                    id: "openrouter-acme".to_owned(),
                    name: "Acme team".to_owned(),
                    key_prefix: "sk-or-".to_owned(),
                    key_last4: "81c0".to_owned(),
                    monthly_cap_cents: Some(20_000),
                    monthly_used_cents: 4_120,
                    cap_note: "Set by the Acme team admin on OpenRouter. Bruk stops starting OpenRouter steps on this key when the cap is reached.".to_owned(),
                    default_model: "openrouter/auto".to_owned(),
                    is_default: false,
                    used_by_projects: vec!["api-gateway".to_owned(), "marketing-site".to_owned()],
                    test_state: ConnectionTestState::Works,
                },
            ],
            machines: MachinesSettings::default(),
            extensions: ExtensionsSettings::sample(),
            advanced: AdvancedSettings::default(),
            next_key_seq: 3,
        }
    }

    /// Migrates older persisted preferences schemas to [`PREFERENCES_SCHEMA_VERSION`].
    ///
    /// - `< 2`: clears legacy pre-Demo-Mode sample providers and extensions while
    ///   preserving general/appearance settings.
    /// - `2`: clears the previously always-seeded unconnected Codex CLI placeholder
    ///   now that Codex is an opt-in registered provider.
    ///
    /// Returns `true` if a migration was applied.
    pub fn migrate_legacy_sample_defaults(&mut self) -> bool {
        if self.schema_version >= PREFERENCES_SCHEMA_VERSION {
            return false;
        }
        if self.schema_version < 2 {
            let clean = Self::empty();
            self.claude = clean.claude;
            self.codex = clean.codex;
            self.openrouter_keys = clean.openrouter_keys;
            self.extensions = clean.extensions;
            self.next_key_seq = clean.next_key_seq;
        } else if self.schema_version < 3
            && self
                .codex
                .as_ref()
                .is_some_and(|codex| !codex.connected && codex.used_by_projects.is_empty())
        {
            self.codex = None;
        }
        self.schema_version = PREFERENCES_SCHEMA_VERSION;
        true
    }

    /// Looks up an OpenRouter key by id.
    pub fn openrouter_key(&self, id: &str) -> Option<&OpenRouterKey> {
        self.openrouter_keys.iter().find(|key| key.id == id)
    }

    /// Looks up a registered CLI account by id (`"claude"` or `"codex"`).
    pub fn cli_account(&self, id: &str) -> Option<&CliAccount> {
        if self.claude.id == id {
            Some(&self.claude)
        } else if let Some(codex) = &self.codex
            && codex.id == id
        {
            Some(codex)
        } else {
            None
        }
    }

    /// Registers the Codex CLI provider account with the requested binary and default model.
    pub fn add_codex_account(
        &mut self,
        cli_binary: &str,
        default_model: &str,
    ) -> Result<String, PreferencesError> {
        let binary = cli_binary.trim();
        let cli_binary = if binary.is_empty() {
            "codex".to_owned()
        } else {
            binary.to_owned()
        };
        let model = default_model.trim();
        let default_model = if model.is_empty() {
            "gpt-5.3-codex".to_owned()
        } else {
            model.to_owned()
        };

        self.codex = Some(CliAccount {
            id: "codex".to_owned(),
            provider: ProviderKind::Codex,
            display_name: "Codex".to_owned(),
            email: String::new(),
            plan: String::new(),
            connected: false,
            cli_binary,
            custom_cli_path: None,
            binary_found: true,
            uses_system_login: false,
            config_dir: "~/Library/Application Support/software-factory/providers/codex".to_owned(),
            default_model,
            hidden_model_families: Vec::new(),
            available_models: Vec::new(),
            usage: None,
            usage_warning: None,
            test_state: ConnectionTestState::Untested,
            used_by_projects: Vec::new(),
        });
        Ok("codex".to_owned())
    }

    /// Unregisters the Codex CLI provider account. Returns the next provider id to select.
    pub fn remove_codex_account(&mut self) -> Result<String, PreferencesError> {
        if self.codex.take().is_none() {
            return Err(PreferencesError::ProviderNotFound("codex".to_owned()));
        }
        Ok(self.claude.id.clone())
    }

    /// Adds a new OpenRouter key after validating its label, key format, spending cap, and model.
    pub fn add_openrouter_key(
        &mut self,
        name: &str,
        raw_api_key: &str,
        monthly_cap_input: &str,
        default_model: &str,
    ) -> Result<String, PreferencesError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(PreferencesError::EmptyName);
        }
        if self
            .openrouter_keys
            .iter()
            .any(|k| k.name.eq_ignore_ascii_case(name))
        {
            return Err(PreferencesError::DuplicateKeyName(name.to_owned()));
        }

        let (key_prefix, key_last4) = extract_key_preview(raw_api_key)?;
        let monthly_cap_cents = parse_dollar_amount(monthly_cap_input)?;
        let model = default_model.trim();
        let default_model = if model.is_empty() {
            "openrouter/auto".to_owned()
        } else {
            model.to_owned()
        };

        let id = format!("openrouter-{}", self.next_key_seq);
        self.next_key_seq += 1;
        let is_default = self.openrouter_keys.is_empty();

        self.openrouter_keys.push(OpenRouterKey {
            id: id.clone(),
            name: name.to_owned(),
            key_prefix,
            key_last4,
            monthly_cap_cents,
            monthly_used_cents: 0,
            cap_note: "Stored in the macOS Keychain. Bruk stops starting OpenRouter steps on this key when the cap is reached.".to_owned(),
            default_model,
            is_default,
            used_by_projects: Vec::new(),
            test_state: ConnectionTestState::Works,
        });

        Ok(id)
    }

    /// Replaces the API key secret metadata for an existing OpenRouter key.
    pub fn replace_openrouter_key_secret(
        &mut self,
        id: &str,
        raw_api_key: &str,
    ) -> Result<(), PreferencesError> {
        let (prefix, last4) = extract_key_preview(raw_api_key)?;
        let key = self
            .openrouter_keys
            .iter_mut()
            .find(|k| k.id == id)
            .ok_or_else(|| PreferencesError::ProviderNotFound(id.to_owned()))?;
        key.key_prefix = prefix;
        key.key_last4 = last4;
        key.test_state = ConnectionTestState::Works;
        Ok(())
    }

    /// Updates the monthly spending cap for an OpenRouter key from user text input.
    pub fn update_openrouter_cap(
        &mut self,
        id: &str,
        monthly_cap_input: &str,
    ) -> Result<(), PreferencesError> {
        let cap_cents = parse_dollar_amount(monthly_cap_input)?;
        let key = self
            .openrouter_keys
            .iter_mut()
            .find(|k| k.id == id)
            .ok_or_else(|| PreferencesError::ProviderNotFound(id.to_owned()))?;
        key.monthly_cap_cents = cap_cents;
        Ok(())
    }

    /// Updates the default model for an OpenRouter key or CLI account.
    pub fn update_provider_model(&mut self, id: &str, model: &str) -> Result<(), PreferencesError> {
        let model = model.trim();
        if model.is_empty() {
            return Err(PreferencesError::EmptyModel);
        }
        if self.claude.id == id {
            self.claude.default_model = model.to_owned();
            return Ok(());
        }
        if let Some(codex) = &mut self.codex
            && codex.id == id
        {
            codex.default_model = model.to_owned();
            return Ok(());
        }
        let key = self
            .openrouter_keys
            .iter_mut()
            .find(|k| k.id == id)
            .ok_or_else(|| PreferencesError::ProviderNotFound(id.to_owned()))?;
        key.default_model = model.to_owned();
        Ok(())
    }

    /// Marks the specified OpenRouter key as the default key.
    pub fn make_default_openrouter_key(&mut self, id: &str) -> Result<(), PreferencesError> {
        if !self.openrouter_keys.iter().any(|k| k.id == id) {
            return Err(PreferencesError::ProviderNotFound(id.to_owned()));
        }
        for key in &mut self.openrouter_keys {
            key.is_default = key.id == id;
        }
        Ok(())
    }

    /// Renames an OpenRouter key.
    pub fn rename_openrouter_key(
        &mut self,
        id: &str,
        new_name: &str,
    ) -> Result<(), PreferencesError> {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err(PreferencesError::EmptyName);
        }
        if self
            .openrouter_keys
            .iter()
            .any(|k| k.id != id && k.name.eq_ignore_ascii_case(new_name))
        {
            return Err(PreferencesError::DuplicateKeyName(new_name.to_owned()));
        }
        let key = self
            .openrouter_keys
            .iter_mut()
            .find(|k| k.id == id)
            .ok_or_else(|| PreferencesError::ProviderNotFound(id.to_owned()))?;
        key.name = new_name.to_owned();
        Ok(())
    }

    /// Removes an OpenRouter key, promoting the first remaining key to default if needed.
    /// Returns the next provider id to select.
    pub fn remove_openrouter_key(&mut self, id: &str) -> Result<String, PreferencesError> {
        let index = self
            .openrouter_keys
            .iter()
            .position(|k| k.id == id)
            .ok_or_else(|| PreferencesError::ProviderNotFound(id.to_owned()))?;
        let removed = self.openrouter_keys.remove(index);
        if removed.is_default
            && let Some(first) = self.openrouter_keys.first_mut()
        {
            first.is_default = true;
        }
        let next_selected = self
            .openrouter_keys
            .get(index.min(self.openrouter_keys.len().saturating_sub(1)))
            .map(|k| k.id.clone())
            .unwrap_or_else(|| self.claude.id.clone());
        Ok(next_selected)
    }

    /// Runs a lightweight local check for the specified provider account.
    pub fn test_provider(&mut self, id: &str) -> Result<ConnectionTestState, PreferencesError> {
        if self.claude.id == id {
            self.claude.test_state = if self.claude.connected {
                ConnectionTestState::Works
            } else {
                ConnectionTestState::Failed("Sign in to the Claude CLI first".to_owned())
            };
            return Ok(self.claude.test_state.clone());
        }
        if let Some(codex) = &mut self.codex
            && codex.id == id
        {
            codex.test_state = if codex.connected {
                ConnectionTestState::Works
            } else {
                ConnectionTestState::Failed("Sign in to the Codex CLI first".to_owned())
            };
            return Ok(codex.test_state.clone());
        }
        let key = self
            .openrouter_keys
            .iter_mut()
            .find(|k| k.id == id)
            .ok_or_else(|| PreferencesError::ProviderNotFound(id.to_owned()))?;
        key.test_state = if key.key_last4.len() == 4 {
            ConnectionTestState::Works
        } else {
            ConnectionTestState::Failed("Key format looks invalid".to_owned())
        };
        Ok(key.test_state.clone())
    }

    /// Connects or disconnects a registered CLI provider account (`"claude"` or `"codex"`).
    pub fn set_cli_connected(&mut self, id: &str, connected: bool) -> Result<(), PreferencesError> {
        let account = if self.claude.id == id {
            &mut self.claude
        } else if let Some(codex) = &mut self.codex
            && codex.id == id
        {
            codex
        } else {
            return Err(PreferencesError::ProviderNotFound(id.to_owned()));
        };
        account.connected = connected;
        account.test_state = if connected {
            ConnectionTestState::Works
        } else {
            ConnectionTestState::Untested
        };
        Ok(())
    }

    /// Sets or clears the optional custom binary path override for the Claude CLI.
    ///
    /// Empty or whitespace-only input clears the override and reverts to default `PATH` lookup.
    pub fn set_claude_custom_path(&mut self, custom_path: Option<&str>) {
        let normalized = custom_path
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        self.claude.custom_cli_path = normalized;
    }

    /// Applies the outcome of probing the local `claude` CLI onto `self.claude`.
    pub fn apply_claude_probe(&mut self, probe: &ClaudeCliProbe, explicit_test: bool) {
        self.claude.uses_system_login = true;
        match probe {
            ClaudeCliProbe::Authenticated {
                binary_path,
                email,
                plan,
                config_dir,
                ..
            } => {
                self.claude.binary_found = true;
                self.claude.connected = true;
                self.claude.cli_binary = binary_path.display().to_string();
                self.claude.email = email.clone();
                self.claude.plan = plan.clone();
                self.claude.config_dir = config_dir.clone();
                self.claude.test_state = ConnectionTestState::Works;
            }
            ClaudeCliProbe::NotAuthenticated {
                binary_path,
                config_dir,
            } => {
                self.claude.binary_found = true;
                self.claude.connected = false;
                self.claude.cli_binary = binary_path.display().to_string();
                self.claude.email.clear();
                self.claude.plan.clear();
                self.claude.available_models.clear();
                self.claude.usage = None;
                self.claude.usage_warning = None;
                self.claude.config_dir = config_dir.clone();
                self.claude.test_state = if explicit_test {
                    ConnectionTestState::Failed("Sign in to the Claude CLI first".to_owned())
                } else {
                    ConnectionTestState::Untested
                };
            }
            ClaudeCliProbe::BinaryNotFound { error } => {
                self.claude.binary_found = false;
                self.claude.connected = false;
                self.claude.cli_binary = self
                    .claude
                    .custom_cli_path
                    .clone()
                    .unwrap_or_else(|| "claude".to_owned());
                self.claude.email.clear();
                self.claude.plan.clear();
                self.claude.available_models.clear();
                self.claude.usage = None;
                self.claude.usage_warning = None;
                self.claude.config_dir = "~/.claude".to_owned();
                self.claude.test_state = ConnectionTestState::Failed(error.short_status());
            }
            ClaudeCliProbe::ProbeFailed {
                binary_path,
                message,
            } => {
                self.claude.binary_found = true;
                self.claude.connected = false;
                self.claude.cli_binary = binary_path.display().to_string();
                self.claude.email.clear();
                self.claude.plan.clear();
                self.claude.available_models.clear();
                self.claude.usage = None;
                self.claude.usage_warning = None;
                self.claude.test_state = ConnectionTestState::Failed(message.clone());
            }
        }
    }

    /// Applies live model options and usage limits fetched from the Claude CLI.
    pub fn apply_claude_details(&mut self, details: ClaudeCliDetails) {
        if !details.models.is_empty() {
            self.claude.available_models = details.models;
        }
        if details.usage.is_some() {
            self.claude.usage = details.usage;
        }
        self.claude.usage_warning = details.warning;
    }

    /// Toggles visibility of a Claude model family (e.g. `"Fable"`) in the model selection view.
    ///
    /// Returns `true` if the family is now visible (checked), or `false` if hidden (unchecked).
    pub fn toggle_claude_model_family(&mut self, family: &str) -> bool {
        let trimmed = family.trim();
        if trimmed.is_empty() {
            return true;
        }
        if let Some(idx) = self
            .claude
            .hidden_model_families
            .iter()
            .position(|existing| existing.eq_ignore_ascii_case(trimmed))
        {
            self.claude.hidden_model_families.remove(idx);
            true
        } else {
            self.claude.hidden_model_families.push(trimmed.to_owned());
            false
        }
    }

    /// Toggles an extension entry by id.
    pub fn toggle_extension(&mut self, id: &str) {
        if let Some(entry) = self.extensions.entries.iter_mut().find(|e| e.id == id) {
            entry.enabled = !entry.enabled;
        }
    }
}

/// Extracts a safe prefix + last-4 preview from a raw API key without storing the secret.
pub fn extract_key_preview(raw_key: &str) -> Result<(String, String), PreferencesError> {
    let trimmed = raw_key.trim();
    if trimmed.len() < 8 {
        return Err(PreferencesError::InvalidApiKey);
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let last4: String = chars[chars.len() - 4..].iter().collect();
    let prefix = if trimmed.starts_with("sk-or-v1-") || trimmed.starts_with("sk-or-") {
        "sk-or-".to_owned()
    } else if trimmed.starts_with("sk-") {
        "sk-".to_owned()
    } else {
        "key-".to_owned()
    };
    Ok((prefix, last4))
}

/// Parses a user-entered dollar cap string (`""` / `"none"` -> `None`, `"200"` / `"$200.50"` -> cents).
pub fn parse_dollar_amount(input: &str) -> Result<Option<u64>, PreferencesError> {
    let trimmed = input.trim().trim_start_matches('$').trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    if trimmed.starts_with('-') {
        return Err(PreferencesError::InvalidSpendingCap);
    }
    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() > 2 {
        return Err(PreferencesError::InvalidSpendingCap);
    }
    let dollars_str = parts[0].replace(',', "");
    let dollars: u64 = if dollars_str.is_empty() {
        0
    } else {
        dollars_str
            .parse()
            .map_err(|_| PreferencesError::InvalidSpendingCap)?
    };
    let cents: u64 = if parts.len() == 2 {
        let frac = parts[1];
        if frac.is_empty() || frac.len() > 2 || !frac.chars().all(|c| c.is_ascii_digit()) {
            return Err(PreferencesError::InvalidSpendingCap);
        }
        let parsed: u64 = frac
            .parse()
            .map_err(|_| PreferencesError::InvalidSpendingCap)?;
        if frac.len() == 1 { parsed * 10 } else { parsed }
    } else {
        0
    };
    dollars
        .checked_mul(100)
        .and_then(|d| d.checked_add(cents))
        .map(Some)
        .ok_or(PreferencesError::InvalidSpendingCap)
}

/// Formats cents as `$200.00`.
pub fn format_dollars_cents(cents: u64) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}

/// Formats cents as `$200` when whole dollars, or `$25.50` otherwise.
pub fn format_whole_or_cents(cents: u64) -> String {
    if cents.is_multiple_of(100) {
        format!("${}", cents / 100)
    } else {
        format_dollars_cents(cents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_preferences_match_reference_providers() {
        let prefs = AppPreferences::sample();
        assert_eq!(prefs.claude.list_subtitle(), "jonathan@garbee.me · Max");
        assert_eq!(
            prefs.codex.as_ref().unwrap().list_subtitle(),
            "jonathan@garbee.me · Pro"
        );
        assert_eq!(prefs.openrouter_keys.len(), 2);

        let personal = prefs.openrouter_key("openrouter-personal").unwrap();
        assert!(personal.is_default);
        assert_eq!(personal.list_subtitle(), "Key ••3f9a · $25 a month");

        let acme = prefs.openrouter_key("openrouter-acme").unwrap();
        assert!(!acme.is_default);
        assert_eq!(acme.masked_key(), "sk-or-••••••••••••81c0");
        assert_eq!(acme.list_subtitle(), "Key ••81c0 · $200 a month");
        assert_eq!(acme.cap_summary(), "$200.00 · $41.20 used");
        assert_eq!(acme.default_model, "openrouter/auto");
        assert_eq!(
            acme.used_by_projects_display(),
            "api-gateway · marketing-site"
        );
    }

    #[test]
    fn openrouter_key_lifecycle_and_validation() {
        let mut prefs = AppPreferences::sample();

        // Add new key
        let id = prefs
            .add_openrouter_key(
                "Staging CI",
                "sk-or-v1-abcdef12345699ab",
                "$75.50",
                "anthropic/claude-sonnet-4",
            )
            .unwrap();
        let added = prefs.openrouter_key(&id).unwrap();
        assert_eq!(added.masked_key(), "sk-or-••••••••••••99ab");
        assert_eq!(added.monthly_cap_cents, Some(7_550));
        assert_eq!(added.default_model, "anthropic/claude-sonnet-4");
        assert!(!added.is_default);

        // Duplicate name rejected
        assert!(matches!(
            prefs.add_openrouter_key("staging ci", "sk-or-12345678", "", ""),
            Err(PreferencesError::DuplicateKeyName(_))
        ));

        // Make default switches default flag cleanly
        prefs.make_default_openrouter_key(&id).unwrap();
        assert!(prefs.openrouter_key(&id).unwrap().is_default);
        assert!(
            !prefs
                .openrouter_key("openrouter-personal")
                .unwrap()
                .is_default
        );

        // Rename, replace secret, update cap & model
        prefs.rename_openrouter_key(&id, "Production CI").unwrap();
        prefs
            .replace_openrouter_key_secret(&id, "sk-or-v1-zzzzzzzz42ef")
            .unwrap();
        prefs.update_openrouter_cap(&id, "500").unwrap();
        prefs
            .update_provider_model(&id, "google/gemini-2.5-pro")
            .unwrap();
        let updated = prefs.openrouter_key(&id).unwrap();
        assert_eq!(updated.name, "Production CI");
        assert_eq!(updated.masked_key(), "sk-or-••••••••••••42ef");
        assert_eq!(updated.monthly_cap_cents, Some(50_000));
        assert_eq!(updated.default_model, "google/gemini-2.5-pro");

        // Removing the default key promotes the first remaining key
        let next = prefs.remove_openrouter_key(&id).unwrap();
        assert_eq!(prefs.openrouter_keys.len(), 2);
        assert!(
            prefs
                .openrouter_key("openrouter-personal")
                .unwrap()
                .is_default
        );
        assert!(!next.is_empty());
    }

    #[test]
    fn dollar_amount_parser_handles_valid_and_invalid_inputs() {
        assert_eq!(parse_dollar_amount("").unwrap(), None);
        assert_eq!(parse_dollar_amount("none").unwrap(), None);
        assert_eq!(parse_dollar_amount("200").unwrap(), Some(20_000));
        assert_eq!(parse_dollar_amount("$200.00").unwrap(), Some(20_000));
        assert_eq!(parse_dollar_amount("$1,250.5").unwrap(), Some(125_050));
        assert!(parse_dollar_amount("-10").is_err());
        assert!(parse_dollar_amount("abc").is_err());
        assert!(parse_dollar_amount("12.345").is_err());
    }

    #[test]
    fn claude_probe_updates_account_status_and_custom_path() {
        use std::path::PathBuf;

        use crate::claude_cli::ClaudeBinaryError;

        let mut prefs = AppPreferences::empty();
        assert!(!prefs.claude.binary_found);
        assert_eq!(prefs.claude.list_subtitle(), "CLI not found");

        prefs.apply_claude_probe(
            &ClaudeCliProbe::Authenticated {
                binary_path: PathBuf::from("/Users/dev/.local/bin/claude"),
                email: "dev@example.com".to_owned(),
                plan: "Max 5x".to_owned(),
                auth_method: "claude.ai".to_owned(),
                config_dir: "/Users/dev/.claude".to_owned(),
            },
            false,
        );
        assert!(prefs.claude.connected);
        assert!(prefs.claude.binary_found);
        assert!(prefs.claude.uses_system_login);
        assert_eq!(prefs.claude.cli_binary, "/Users/dev/.local/bin/claude");
        assert_eq!(prefs.claude.config_dir, "/Users/dev/.claude");
        assert_eq!(prefs.claude.list_subtitle(), "dev@example.com · Max 5x");
        assert_eq!(prefs.claude.test_state, ConnectionTestState::Works);

        prefs.apply_claude_probe(
            &ClaudeCliProbe::NotAuthenticated {
                binary_path: PathBuf::from("/Users/dev/.local/bin/claude"),
                config_dir: "/Users/dev/.claude".to_owned(),
            },
            false,
        );
        assert!(!prefs.claude.connected);
        assert!(prefs.claude.binary_found);
        assert_eq!(prefs.claude.list_subtitle(), "Not signed in");
        assert_eq!(prefs.claude.test_state, ConnectionTestState::Untested);

        prefs.set_claude_custom_path(Some("  /opt/claude/bin/claude  "));
        assert_eq!(
            prefs.claude.custom_cli_path.as_deref(),
            Some("/opt/claude/bin/claude")
        );
        prefs.apply_claude_probe(
            &ClaudeCliProbe::BinaryNotFound {
                error: ClaudeBinaryError::CustomPathNotFound(PathBuf::from(
                    "/opt/claude/bin/claude",
                )),
            },
            false,
        );
        assert!(!prefs.claude.binary_found);
        assert!(!prefs.claude.connected);
        assert_eq!(prefs.claude.list_subtitle(), "CLI not found");
        assert_eq!(
            prefs.claude.test_state,
            ConnectionTestState::Failed("Custom path not found".to_owned())
        );

        prefs.set_claude_custom_path(Some("   "));
        assert_eq!(prefs.claude.custom_cli_path, None);
    }

    #[test]
    fn deserializes_legacy_cli_account_without_new_claude_fields() {
        let legacy_json = r#"{
            "id": "claude",
            "provider": "claude",
            "display_name": "Claude",
            "email": "",
            "plan": "",
            "connected": false,
            "cli_binary": "claude",
            "config_dir": "~/.claude",
            "default_model": "claude-sonnet-4-6",
            "test_state": {"state": "untested"},
            "used_by_projects": []
        }"#;
        let account: CliAccount = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(account.custom_cli_path, None);
        assert!(account.binary_found);
        assert!(!account.uses_system_login);
        assert!(account.hidden_model_families.is_empty());
    }

    #[test]
    fn model_family_filter_hides_and_restores_matching_models() {
        let mut prefs = AppPreferences::sample();
        assert_eq!(
            prefs.claude.available_model_families(),
            vec!["Opus", "Fable", "Sonnet", "Haiku"]
        );
        assert_eq!(prefs.claude.visible_available_models().len(), 4);

        // Uncheck Fable.
        assert!(!prefs.toggle_claude_model_family("Fable"));
        assert!(!prefs.claude.is_model_family_visible("fable"));
        let visible = prefs.claude.visible_available_models();
        assert_eq!(visible.len(), 3);
        assert!(visible.iter().all(|m| m.family() != "Fable"));

        // Re-check Fable (case-insensitive).
        assert!(prefs.toggle_claude_model_family("fable"));
        assert!(prefs.claude.is_model_family_visible("Fable"));
        assert_eq!(prefs.claude.visible_available_models().len(), 4);
    }

    #[test]
    fn codex_account_is_unregistered_by_default_and_supports_add_and_remove() {
        let mut prefs = AppPreferences::empty();
        assert!(prefs.codex.is_none());
        assert!(prefs.cli_account("codex").is_none());
        assert_eq!(
            prefs.test_provider("codex"),
            Err(PreferencesError::ProviderNotFound("codex".to_owned()))
        );

        let id = prefs.add_codex_account("", "").unwrap();
        assert_eq!(id, "codex");
        let codex = prefs.cli_account("codex").unwrap();
        assert_eq!(codex.cli_binary, "codex");
        assert_eq!(codex.default_model, "gpt-5.3-codex");
        assert!(!codex.connected);

        let next = prefs.remove_codex_account().unwrap();
        assert_eq!(next, "claude");
        assert!(prefs.codex.is_none());
        assert!(prefs.cli_account("codex").is_none());
    }
}
