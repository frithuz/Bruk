//! Secret and MCP problem/identifier value types shared across crates.

use std::{collections::BTreeSet, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::ProjectId;

/// Immutable UUID v4 identifying a secret record. Used in Keychain account keys.
pub type SecretId = String;

/// Immutable identifier for a configured external secret manager (e.g. `"1password"` or UUID).
pub type ManagerId = String;

/// Built-in manager identifier for 1Password (`op`).
pub const ONE_PASSWORD_MANAGER_ID: &str = "1password";

/// The project's MCP file, relative to the repository root. Antigravity reads it on its own.
pub const AGENTS_MCP_CONFIG_PATH: &str = ".agents/mcp_config.json";
/// The fallback project MCP file, used when [`AGENTS_MCP_CONFIG_PATH`] doesn't exist.
pub const ROOT_MCP_JSON_PATH: &str = ".mcp.json";

/// Type-safe split between `${NAME}` Env secrets and sealed Internal Bruk credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SecretKind {
    /// Answers `${NAME}` for MCP servers, provider spawn settings, and (when `AgentAndSteps`) shells.
    Env { scope: EnvScope, exposure: Exposure },
    /// Sealed Bruk credential. Reached only by [`SecretId`]; never participates in `${NAME}` lookup
    /// and never placed in an agent's or workflow shell step's environment.
    Internal { purpose: InternalPurpose },
}

/// Scope of an `Env` secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", content = "data", rename_all = "snake_case")]
pub enum EnvScope {
    /// Shared across projects according to `shared_with` (defaults to `AllProjects` on creation).
    App { shared_with: Sharing },
    /// Project-specific secret (overrides any App secret of the same name).
    Project(ProjectId),
}

/// Which projects an App-scoped Env secret is shared with.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "mode", content = "projects", rename_all = "snake_case")]
pub enum Sharing {
    #[default]
    AllProjects,
    Projects(BTreeSet<ProjectId>),
    NoProjects,
}

impl Sharing {
    pub fn includes_project(&self, project_id: ProjectId) -> bool {
        match self {
            Self::AllProjects => true,
            Self::Projects(set) => set.contains(&project_id),
            Self::NoProjects => false,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::AllProjects => "All projects".to_owned(),
            Self::Projects(set) if set.is_empty() => "No projects".to_owned(),
            Self::Projects(set) if set.len() == 1 => "1 project".to_owned(),
            Self::Projects(set) => format!("{} projects", set.len()),
            Self::NoProjects => "No projects".to_owned(),
        }
    }
}

/// Least-privilege exposure level for an `Env` secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exposure {
    /// Injected only when an MCP server the step loads references the name (default for new Env secrets).
    #[default]
    McpOnly,
    /// Also injected into the agent's shell and workflow shell steps.
    AgentAndSteps,
}

impl Exposure {
    pub fn label(self) -> &'static str {
        match self {
            Self::McpOnly => "MCP servers only",
            Self::AgentAndSteps => "Agent and steps",
        }
    }
}

/// Purpose of a sealed `Internal` credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "purpose", rename_all = "snake_case")]
pub enum InternalPurpose {
    /// Reusable named App-level GitHub PAT, selectable per project.
    GitHubPat,
    /// OpenRouter API key referenced by `OpenRouterKey.secret_id`.
    OpenRouterKey,
    /// Auth token for 1Password Service Account or a Command Manager (MUST be Keychain-backed).
    ManagerAuth { manager: ManagerId },
    /// Secret backing a Claude or Codex spawn-settings env var.
    ProviderSpawnEnv,
}

/// Where a secret's value lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum SecretSource {
    Keychain,
    Manager {
        manager: ManagerId,
        reference: String,
    },
}

impl SecretSource {
    pub fn badge_label(&self) -> String {
        match self {
            Self::Keychain => "Keychain".to_owned(),
            Self::Manager { manager, .. } => {
                if manager == ONE_PASSWORD_MANAGER_ID {
                    "1Password".to_owned()
                } else {
                    manager.clone()
                }
            }
        }
    }
}

/// Availability state of a secret's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretState {
    Ready,
    NeedsValue,
}

/// A failure to resolve an existing secret record or its backing store/manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum SecretFailure {
    NeedsValue {
        secret_id: SecretId,
        name: String,
    },
    ReferenceNotFound {
        secret_id: SecretId,
        name: String,
        manager: ManagerId,
        detail: String,
    },
    ManagerLocked {
        manager: ManagerId,
        detail: String,
    },
    ManagerAuthFailed {
        manager: ManagerId,
        detail: String,
    },
    ManagerNotInstalled {
        manager: ManagerId,
        binary: String,
    },
    StoreUnavailable {
        detail: String,
    },
    TimedOut {
        manager: ManagerId,
        timeout_secs: u64,
    },
    Cancelled,
}

impl SecretFailure {
    pub fn short_message(&self) -> String {
        match self {
            Self::NeedsValue { name, .. } => format!("{name} has no value"),
            Self::ReferenceNotFound { name, detail, .. } => {
                if detail.is_empty() {
                    format!("{name} was not found in the secret manager")
                } else {
                    format!("{name}: {detail}")
                }
            }
            Self::ManagerLocked { manager, detail } => {
                let label = manager_display_name(manager);
                if detail.is_empty() {
                    format!("{label} is locked")
                } else {
                    format!("{label} is locked: {detail}")
                }
            }
            Self::ManagerAuthFailed { manager, detail } => {
                let label = manager_display_name(manager);
                if detail.is_empty() {
                    format!("{label} authentication failed")
                } else {
                    format!("{label} authentication failed: {detail}")
                }
            }
            Self::ManagerNotInstalled { binary, .. } => format!("{binary} wasn't found"),
            Self::StoreUnavailable { detail } => {
                if detail.is_empty() {
                    "Keychain unavailable".to_owned()
                } else {
                    format!("Keychain unavailable: {detail}")
                }
            }
            Self::TimedOut {
                manager,
                timeout_secs,
            } => {
                let label = manager_display_name(manager);
                format!("{label} timed out after {timeout_secs}s")
            }
            Self::Cancelled => "Secret resolution was cancelled".to_owned(),
        }
    }

    pub fn fix(&self) -> SecretFix {
        match self {
            Self::NeedsValue { secret_id, .. } | Self::ReferenceNotFound { secret_id, .. } => {
                SecretFix::EditSecret(secret_id.clone())
            }
            Self::ManagerLocked { manager, .. } | Self::TimedOut { manager, .. } => {
                SecretFix::UnlockManager(manager.clone())
            }
            Self::ManagerNotInstalled { manager, .. } | Self::ManagerAuthFailed { manager, .. } => {
                SecretFix::OpenManagerSettings(manager.clone())
            }
            Self::StoreUnavailable { .. } => SecretFix::OpenSecretsSettings,
            Self::Cancelled => SecretFix::None,
        }
    }

    /// Converts this failure into a blocking [`SecretProblem`].
    pub fn to_problem(&self, project_id: Option<ProjectId>) -> SecretProblem {
        let secret_name = match self {
            Self::NeedsValue { name, .. } | Self::ReferenceNotFound { name, .. } => {
                Some(name.clone())
            }
            _ => None,
        };
        let secret_id = match self {
            Self::NeedsValue { secret_id, .. } | Self::ReferenceNotFound { secret_id, .. } => {
                Some(secret_id.clone())
            }
            _ => None,
        };
        let manager_id = match self {
            Self::ReferenceNotFound { manager, .. }
            | Self::ManagerLocked { manager, .. }
            | Self::ManagerAuthFailed { manager, .. }
            | Self::ManagerNotInstalled { manager, .. }
            | Self::TimedOut { manager, .. } => Some(manager.clone()),
            _ => None,
        };
        SecretProblem {
            secret_name,
            secret_id,
            manager_id,
            project_id,
            reason: self.short_message(),
            fix: self.fix(),
            blocking: true,
        }
    }
}

fn manager_display_name(manager: &str) -> &str {
    if manager == ONE_PASSWORD_MANAGER_ID {
        "1Password"
    } else {
        manager
    }
}

/// Dedicated secret problem surfaced in banners, Inbox *Setup*, and notifications.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretProblem {
    #[serde(default)]
    pub secret_name: Option<String>,
    #[serde(default)]
    pub secret_id: Option<SecretId>,
    #[serde(default)]
    pub manager_id: Option<ManagerId>,
    #[serde(default)]
    pub project_id: Option<ProjectId>,
    pub reason: String,
    pub fix: SecretFix,
    /// True whenever an existing secret record failed to resolve or a required secret is missing.
    pub blocking: bool,
}

impl SecretProblem {
    pub fn title(&self) -> String {
        match self.secret_name.as_deref() {
            Some(name) => {
                if self.reason.starts_with(name) {
                    self.reason.clone()
                } else {
                    format!("{name}: {}", self.reason)
                }
            }
            None => self.reason.clone(),
        }
    }

    pub fn dedupe_key(&self) -> String {
        format!(
            "secret:{}:{}:{}:{}",
            self.secret_id.as_deref().unwrap_or(""),
            self.secret_name.as_deref().unwrap_or(""),
            self.manager_id.as_deref().unwrap_or(""),
            self.reason,
        )
    }
}

/// Direct action offered to resolve a [`SecretProblem`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", content = "data", rename_all = "snake_case")]
pub enum SecretFix {
    #[serde(alias = "set_env_var")]
    AddSecret {
        name: String,
        #[serde(default)]
        project_id: Option<ProjectId>,
    },
    EditSecret(SecretId),
    UnlockManager(ManagerId),
    OpenManagerSettings(ManagerId),
    OpenSecretsSettings,
    ResumeQueuedWork {
        project_id: ProjectId,
    },
    DeleteLegacyDatabases {
        paths: Vec<PathBuf>,
    },
    None,
}

impl SecretFix {
    pub fn button_label(&self) -> Option<&'static str> {
        match self {
            Self::AddSecret { .. } => Some("Add Secret…"),
            Self::EditSecret(_) => Some("Edit Secret…"),
            Self::UnlockManager(_) => Some("Unlock & Retry"),
            Self::OpenManagerSettings(_) => Some("Open Manager Settings"),
            Self::OpenSecretsSettings => Some("Open Secrets Settings"),
            Self::ResumeQueuedWork { .. } => Some("Resume Queued Work"),
            Self::DeleteLegacyDatabases { .. } => Some("Delete Legacy Files…"),
            Self::None => None,
        }
    }
}

/// Which tracked file a project's MCP servers came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectMcpFile {
    /// `.agents/mcp_config.json`
    AgentsMcpConfig,
    /// `.mcp.json`, used only when `.agents/mcp_config.json` doesn't exist.
    RootMcpJson,
}

impl ProjectMcpFile {
    /// Lookup order: the first file that exists is the project's.
    pub const LOOKUP_ORDER: &'static [Self] = &[Self::AgentsMcpConfig, Self::RootMcpJson];

    pub fn relative_path(self) -> &'static str {
        match self {
            Self::AgentsMcpConfig => AGENTS_MCP_CONFIG_PATH,
            Self::RootMcpJson => ROOT_MCP_JSON_PATH,
        }
    }
}

/// Where an MCP server came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "file", rename_all = "snake_case")]
pub enum McpOrigin {
    Project(ProjectMcpFile),
    ProjectExtra,
    UserExtra,
}

impl McpOrigin {
    pub fn label(self) -> &'static str {
        match self {
            Self::Project(file) => file.relative_path(),
            Self::ProjectExtra => "your servers for this project",
            Self::UserExtra => "your servers for all projects",
        }
    }

    pub fn is_project(self) -> bool {
        matches!(self, Self::Project(_))
    }
}

/// What the notification's button does to fix an MCP problem.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "action", content = "target", rename_all = "snake_case")]
pub enum McpFix {
    /// Open Project Settings › Environment to add this variable.
    SetEnvVar(String),
    /// Open Project Settings › MCP Servers to review and approve the project file.
    ApproveProjectServers,
    /// Open Preferences › Extensions to turn project servers back on.
    AllowProjectServers,
    /// Open the editor for one of the user's own servers.
    EditExtra {
        origin: McpOrigin,
        name: String,
    },
    /// The project file needs fixing (in the repository).
    EditProjectFile(ProjectMcpFile),
    /// Install the command or put it on the agents' `PATH` (Project Settings › Environment).
    InstallCommand(String),
    /// Use another provider for this step or chat.
    ChangeProvider,
    None,
}

impl McpFix {
    pub fn button_label(&self) -> Option<&'static str> {
        match self {
            Self::SetEnvVar(_) => Some("Set Variable…"),
            Self::ApproveProjectServers => Some("Review Servers…"),
            Self::AllowProjectServers => Some("Open Extensions…"),
            Self::EditExtra { .. } => Some("Edit Server…"),
            Self::EditProjectFile(_) | Self::InstallCommand(_) => Some("Open MCP Servers…"),
            Self::ChangeProvider | Self::None => None,
        }
    }
}

/// Something that keeps an MCP server from reaching the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpProblem {
    /// The server, when the problem is about one.
    pub server: Option<String>,
    pub origin: Option<McpOrigin>,
    /// What went wrong, as a sentence fragment that reads after the server name
    /// (`needs LINEAR_TOKEN, which isn't set`).
    pub reason: String,
    pub fix: McpFix,
    /// Whether the step or chat turn can't run (or has to stop) because of it: a project
    /// server in a project-only workflow, or a server an agent or step names.
    pub blocking: bool,
}

impl McpProblem {
    /// One line for notifications: `MCP server 'linear' (.agents/mcp_config.json) needs …`.
    pub fn title(&self) -> String {
        match (&self.server, self.origin) {
            (Some(server), Some(origin)) => {
                format!("MCP server '{server}' ({}) {}", origin.label(), self.reason)
            }
            (Some(server), None) => format!("MCP server '{server}' {}", self.reason),
            (None, _) => self.reason.clone(),
        }
    }

    /// Identifies repeats of the same problem so they collapse into one notification.
    pub fn dedupe_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.server.as_deref().unwrap_or("*"),
            self.origin.map(|o| o.label()).unwrap_or("*"),
            self.reason
        )
    }
}

/// The `${VAR}` (or `${VAR:-default}`) names referenced in `text`.
pub fn env_references(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else { break };
        let inner = &after[..end];
        let name = inner.split(":-").next().unwrap_or("").trim();
        if !name.is_empty() && !out.iter().any(|n| n == name) {
            out.push(name.to_owned());
        }
        rest = &after[end + 1..];
    }
    out
}
