//! MCP servers for workflow steps and chats.
//!
//! Servers come from three places, added together:
//!
//! 1. **Project** — tracked in the repository at `.agents/mcp_config.json` (the file Antigravity
//!    reads itself), or the root `.mcp.json` when that file doesn't exist. Never both.
//! 2. **Project extras** — servers one user adds for one project, kept in `bruk.db`.
//! 3. **User extras** — servers one user adds for every project, kept in `bruk.db`.
//!
//! Extras only ever add servers: one named like a project server is dropped with a problem, so a
//! workflow always gets the project's definition. A workflow, composite or agent can set
//! `mcp_sources: project` to withhold every extra (and the MCP settings of the provider spawn
//! settings) so it runs with exactly the servers the repository declares.
//!
//! Project servers start commands from the repository, so they only load once the user has
//! approved the file's current contents ([`ProjectMcpConfig::fingerprint`]).
//!
//! Every way a server can fail to reach the agent becomes an [`McpProblem`] the app shows as a
//! notification: an invalid file, a missing `${VAR}`, a command that isn't on the `PATH`, a
//! server the provider can't take, or a server the provider reports as failed once it starts.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    model::ProjectId,
    preferences::ProviderKind,
    secrets::{Exposure, ResolvedSecrets, SecretProblem, ValueOrigin},
};

/// The project's MCP file, relative to the repository root. Antigravity reads it on its own.
pub const AGENTS_MCP_CONFIG_PATH: &str = ".agents/mcp_config.json";
/// The fallback project MCP file, used when [`AGENTS_MCP_CONFIG_PATH`] doesn't exist.
pub const ROOT_MCP_JSON_PATH: &str = ".mcp.json";
/// `RUST_LOG` that makes Codex report MCP servers that fail to start on stderr (its JSON event
/// stream doesn't), while keeping the rest of its logging off.
pub const CODEX_MCP_RUST_LOG: &str = "error,codex_mcp=warn";
/// Codex setting that turns off the ChatGPT account's connectors (`codex_apps`), which are the
/// user's own and load even from an isolated `CODEX_HOME`.
pub const CODEX_DISABLE_APPS_ARG: &str = "features.apps=false";
/// File (inside a step's or chat's runtime directory) that Claude gets with `--mcp-config`.
pub const CLAUDE_MCP_CONFIG_FILE: &str = "step-mcp.json";
/// File (inside a step's or chat's runtime directory) Antigravity writes its log to, which is
/// where it reports MCP servers that fail to start.
pub const ANTIGRAVITY_LOG_FILE: &str = "antigravity.log";

/// Which tracked file a project's servers came from.
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

/// How an MCP server is reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransport {
    /// A local command speaking MCP over stdin/stdout.
    Stdio {
        command: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        env: BTreeMap<String, String>,
    },
    /// A remote server over streamable HTTP (or the older SSE transport when `sse`).
    Http {
        url: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        headers: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        sse: bool,
    },
}

impl McpTransport {
    /// `command args…` or the URL, for lists.
    pub fn summary(&self) -> String {
        match self {
            Self::Stdio { command, args, .. } => {
                let mut out = command.clone();
                for arg in args {
                    out.push(' ');
                    out.push_str(arg);
                }
                out
            }
            Self::Http { url, .. } => url.clone(),
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Stdio { .. } => "stdio",
            Self::Http { sse: true, .. } => "sse",
            Self::Http { .. } => "http",
        }
    }
}

/// One MCP server definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerDef {
    pub name: String,
    pub transport: McpTransport,
    /// Listed but not loaded (`"disabled": true` in the file, or switched off in Bruk).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

impl McpServerDef {
    /// Builds a server from the fields of Bruk's add-server form: a command (with `args_text`
    /// split like a shell would) or an `http(s)://` URL, plus `KEY=value` (env) or
    /// `Header: value` (headers) lines.
    pub fn from_form(
        name: &str,
        command_or_url: &str,
        args_text: &str,
        pairs_text: &str,
    ) -> Result<Self, String> {
        let name = name.trim();
        validate_server_name(name)?;
        let target = command_or_url.trim();
        if target.is_empty() {
            return Err("enter a command or a URL".to_owned());
        }
        let is_url = target.starts_with("http://") || target.starts_with("https://");
        let transport = if is_url {
            if !args_text.trim().is_empty() {
                return Err("arguments only apply to commands, not URLs".to_owned());
            }
            let mut headers = BTreeMap::new();
            for line in pairs_text.lines().map(str::trim).filter(|l| !l.is_empty()) {
                let (key, value) = line
                    .split_once(':')
                    .ok_or_else(|| format!("'{line}' isn't a 'Header: value' line"))?;
                let key = key.trim();
                if key.is_empty() {
                    return Err(format!("'{line}' has no header name"));
                }
                headers.insert(key.to_owned(), value.trim().to_owned());
            }
            McpTransport::Http {
                url: target.to_owned(),
                headers,
                sse: false,
            }
        } else {
            let args = crate::claude_settings::split_shell_words(args_text)
                .map_err(|err| err.to_string())?;
            let mut env = BTreeMap::new();
            for line in pairs_text.lines().map(str::trim).filter(|l| !l.is_empty()) {
                let (key, value) = line
                    .split_once('=')
                    .ok_or_else(|| format!("'{line}' isn't a 'KEY=value' line"))?;
                let key = key.trim();
                if key.is_empty() {
                    return Err(format!("'{line}' has no variable name"));
                }
                env.insert(key.to_owned(), value.to_owned());
            }
            McpTransport::Stdio {
                command: target.to_owned(),
                args,
                env,
            }
        };
        Ok(Self {
            name: name.to_owned(),
            transport,
            disabled: false,
        })
    }

    /// The `${VAR}` names this server needs from the agent's environment, in order.
    pub fn env_references(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut push_all = |text: &str| {
            for var in env_references(text) {
                if !out.contains(&var) {
                    out.push(var);
                }
            }
        };
        match &self.transport {
            McpTransport::Stdio { command, args, env } => {
                push_all(command);
                args.iter().for_each(|a| push_all(a));
                env.values().for_each(|v| push_all(v));
            }
            McpTransport::Http { url, headers, .. } => {
                push_all(url);
                headers.values().for_each(|v| push_all(v));
            }
        }
        out
    }
}

/// Server names become parts of tool names (`mcp__<name>__<tool>`) and Codex setting keys, so
/// they're limited to letters, digits, `-` and `_`.
pub fn validate_server_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("server name can't be empty".to_owned());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "server name '{name}' can only use letters, digits, '-' and '_'"
        ));
    }
    Ok(())
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

/// Whether `${VAR}` in `text` has a `:-default`, so a missing variable isn't a problem.
fn reference_has_default(text: &str, var: &str) -> bool {
    text.contains(&format!("${{{var}:-"))
}

/// Replaces every `${VAR}` / `${VAR:-default}` in `text` using `lookup`. `None` when a variable
/// without a default isn't set.
fn expand_references(text: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return Some(out);
        };
        let inner = &after[..end];
        let (name, default) = match inner.split_once(":-") {
            Some((n, d)) => (n.trim(), Some(d)),
            None => (inner.trim(), None),
        };
        let value = lookup(name).or_else(|| default.map(str::to_owned))?;
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// `Some(VAR)` when `text` is exactly one `${VAR}` reference (no default).
fn sole_reference(text: &str) -> Option<&str> {
    let inner = text.trim().strip_prefix("${")?.strip_suffix('}')?;
    (!inner.contains(['$', '{', '}', ':'])).then_some(inner.trim())
}

// ---------------------------------------------------------------------------------------------
// The project file
// ---------------------------------------------------------------------------------------------

/// A server entry in the project file that Bruk couldn't use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidMcpEntry {
    pub name: String,
    pub reason: String,
}

/// The project's tracked MCP servers, read from the project's checkout (never a run's
/// worktree, so a step can't hand the next one new servers by editing the file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMcpConfig {
    pub file: ProjectMcpFile,
    pub servers: Vec<McpServerDef>,
    pub invalid: Vec<InvalidMcpEntry>,
    /// SHA-256 of the file's bytes; approving the servers records it.
    pub fingerprint: String,
}

/// What a project has for MCP servers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProjectMcpState {
    /// Neither `.agents/mcp_config.json` nor `.mcp.json` exists.
    #[default]
    Absent,
    Loaded(ProjectMcpConfig),
    /// The file exists but can't be read or isn't valid JSON with an `mcpServers` object.
    Invalid {
        file: ProjectMcpFile,
        message: String,
    },
}

impl ProjectMcpState {
    pub fn config(&self) -> Option<&ProjectMcpConfig> {
        match self {
            Self::Loaded(config) => Some(config),
            _ => None,
        }
    }

    pub fn file(&self) -> Option<ProjectMcpFile> {
        match self {
            Self::Absent => None,
            Self::Loaded(config) => Some(config.file),
            Self::Invalid { file, .. } => Some(*file),
        }
    }
}

/// Reads the project's MCP servers from `<project_root>/.agents/mcp_config.json`, or from
/// `<project_root>/.mcp.json` when the first doesn't exist.
pub fn load_project_mcp(project_root: &Path) -> ProjectMcpState {
    for &file in ProjectMcpFile::LOOKUP_ORDER {
        let path = project_root.join(file.relative_path());
        if !path.is_file() {
            continue;
        }
        return match fs::read(&path) {
            Ok(bytes) => match parse_mcp_json(&String::from_utf8_lossy(&bytes)) {
                Ok((servers, invalid)) => {
                    if !invalid.is_empty() {
                        log::warn!(
                            "mcp.project_config.partial path={} servers={} invalid={}",
                            path.display(),
                            servers.len(),
                            invalid.len()
                        );
                    } else {
                        log::debug!(
                            "mcp.project_config.loaded path={} servers={}",
                            path.display(),
                            servers.len()
                        );
                    }
                    ProjectMcpState::Loaded(ProjectMcpConfig {
                        file,
                        servers,
                        invalid,
                        fingerprint: fingerprint(&bytes),
                    })
                }
                Err(message) => {
                    log::warn!(
                        "mcp.project_config.invalid path={} err={message}",
                        path.display()
                    );
                    ProjectMcpState::Invalid { file, message }
                }
            },
            Err(err) => {
                log::warn!(
                    "mcp.project_config.read_failed path={} err={err}",
                    path.display()
                );
                ProjectMcpState::Invalid {
                    file,
                    message: err.to_string(),
                }
            }
        };
    }
    ProjectMcpState::Absent
}

/// SHA-256 hex digest used to tell whether an approved project file has changed.
pub fn fingerprint(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Parses an `{"mcpServers": {…}}` document (the format shared by `.mcp.json`, Claude Code,
/// Antigravity's `mcp_config.json`, VS Code and Cursor). Remote servers may give their address
/// as `url`, `serverUrl` (Antigravity) or `httpUrl`. Entries Bruk can't use are returned
/// separately instead of failing the whole file.
pub fn parse_mcp_json(raw: &str) -> Result<(Vec<McpServerDef>, Vec<InvalidMcpEntry>), String> {
    let doc: Value = serde_json::from_str(raw).map_err(|err| format!("isn't valid JSON: {err}"))?;
    let servers = match doc.get("mcpServers") {
        Some(Value::Object(map)) => map,
        Some(_) => return Err("'mcpServers' must be an object".to_owned()),
        None if doc.is_object() => return Ok((Vec::new(), Vec::new())),
        None => return Err("must be a JSON object with 'mcpServers'".to_owned()),
    };
    let mut parsed = Vec::new();
    let mut invalid = Vec::new();
    for (name, entry) in servers {
        match parse_server_entry(name, entry) {
            Ok(server) => parsed.push(server),
            Err(reason) => invalid.push(InvalidMcpEntry {
                name: name.clone(),
                reason,
            }),
        }
    }
    Ok((parsed, invalid))
}

fn parse_server_entry(name: &str, entry: &Value) -> Result<McpServerDef, String> {
    validate_server_name(name)?;
    let obj = entry
        .as_object()
        .ok_or_else(|| "must be an object".to_owned())?;
    let disabled = obj
        .get("disabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let kind = obj.get("type").and_then(Value::as_str).unwrap_or("");
    let url = ["url", "serverUrl", "httpUrl"]
        .iter()
        .find_map(|key| obj.get(*key).and_then(Value::as_str));
    let command = obj.get("command").and_then(Value::as_str);
    let transport = match (command, url) {
        (Some(_), Some(_)) => return Err("has both 'command' and a URL".to_owned()),
        (Some(command), None) => {
            if command.trim().is_empty() {
                return Err("'command' is empty".to_owned());
            }
            let args = match obj.get("args") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| "'args' must be a list of strings".to_owned())
                    })
                    .collect::<Result<_, _>>()?,
                Some(_) => return Err("'args' must be a list of strings".to_owned()),
            };
            McpTransport::Stdio {
                command: command.to_owned(),
                args,
                env: string_map(obj.get("env"), "env")?,
            }
        }
        (None, Some(url)) => McpTransport::Http {
            url: url.to_owned(),
            headers: string_map(obj.get("headers"), "headers")?,
            sse: kind == "sse",
        },
        (None, None) => return Err("needs a 'command' or a 'url'".to_owned()),
    };
    Ok(McpServerDef {
        name: name.to_owned(),
        transport,
        disabled,
    })
}

fn string_map(value: Option<&Value>, field: &str) -> Result<BTreeMap<String, String>, String> {
    match value {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(k, v)| match v {
                Value::String(s) => Ok((k.clone(), s.clone())),
                Value::Number(n) => Ok((k.clone(), n.to_string())),
                Value::Bool(b) => Ok((k.clone(), b.to_string())),
                _ => Err(format!("'{field}.{k}' must be a string")),
            })
            .collect(),
        Some(_) => Err(format!("'{field}' must be an object")),
    }
}

// ---------------------------------------------------------------------------------------------
// Sources, policy and resolution
// ---------------------------------------------------------------------------------------------

/// Which servers a workflow, composite or agent accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpSourcesPolicy {
    /// Project servers plus the user's project and user extras.
    #[default]
    All,
    /// Only the servers the repository declares; extras (and the MCP settings in the provider
    /// spawn settings) are withheld.
    Project,
}

impl McpSourcesPolicy {
    pub fn is_default(&self) -> bool {
        *self == Self::All
    }

    /// The stricter of two policies: an inner scope can tighten, never loosen.
    pub fn stricter(self, other: Self) -> Self {
        if self == Self::Project || other == Self::Project {
            Self::Project
        } else {
            Self::All
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Project and your servers",
            Self::Project => "Project servers only",
        }
    }
}

/// Where a resolved server came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

/// Everything a run or chat turn has to choose servers from, gathered when it starts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpSources {
    /// The project's tracked servers, as read from its checkout.
    pub project: ProjectMcpState,
    /// Whether the user allows project servers at all (Preferences › Extensions).
    pub allow_project_servers: bool,
    /// The fingerprint of the project file the user approved, if any.
    pub approved_fingerprint: Option<String>,
    pub project_extras: Vec<McpServerDef>,
    pub user_extras: Vec<McpServerDef>,
}

impl McpSources {
    /// Gathers the sources for one run or chat turn: the project's file as its catalog read it,
    /// the user's extras and approval for the project, and the user's extras for all projects.
    pub fn gather(
        project: &ProjectMcpState,
        settings: Option<&crate::project_settings::ProjectSettings>,
        extensions: &crate::preferences::ExtensionsSettings,
    ) -> Self {
        Self {
            project: project.clone(),
            allow_project_servers: extensions.allow_project_mcp_servers,
            approved_fingerprint: settings.and_then(|s| s.approved_mcp_fingerprint.clone()),
            project_extras: settings.map(|s| s.mcp_servers.clone()).unwrap_or_default(),
            user_extras: extensions.mcp_servers.clone(),
        }
    }

    /// Whether the project's servers may load: allowed in Preferences and the file's current
    /// contents approved.
    pub fn project_trusted(&self) -> bool {
        match (&self.project, self.approved_fingerprint.as_deref()) {
            (ProjectMcpState::Loaded(config), Some(approved)) => {
                self.allow_project_servers && config.fingerprint == approved
            }
            _ => false,
        }
    }

    /// Whether the project has servers waiting for the user's approval.
    pub fn needs_approval(&self) -> bool {
        self.allow_project_servers
            && self
                .project
                .config()
                .is_some_and(|c| c.servers.iter().any(|s| !s.disabled))
            && !self.project_trusted()
    }
}

/// What the notification's button does to fix a problem.
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

/// One server that will be handed to the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMcpServer {
    pub def: McpServerDef,
    pub origin: McpOrigin,
    /// The agent or step names it, so it failing stops the step.
    pub required: bool,
}

/// The servers one step or chat turn gets, and why any others don't reach it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedMcpSet {
    pub policy: McpSourcesPolicy,
    pub servers: Vec<ResolvedMcpServer>,
    /// Extras held back by a project-only policy or an agent/step allowlist.
    pub withheld: Vec<(String, McpOrigin)>,
    pub problems: Vec<McpProblem>,
    /// Whether the project's servers may load (see [`McpSources::project_trusted`]).
    pub project_trusted: bool,
    /// The project's tracked file, if it has one.
    pub project_file: Option<ProjectMcpFile>,
}

impl ResolvedMcpSet {
    pub fn has_blocking_problem(&self) -> bool {
        self.problems.iter().any(|p| p.blocking)
    }

    pub fn blocking_summary(&self) -> Option<String> {
        let blocking: Vec<String> = self
            .problems
            .iter()
            .filter(|p| p.blocking)
            .map(McpProblem::title)
            .collect();
        (!blocking.is_empty()).then(|| blocking.join("; "))
    }

    pub fn server(&self, name: &str) -> Option<&ResolvedMcpServer> {
        self.servers.iter().find(|s| s.def.name == name)
    }

    /// Short description for a step's inspector (`MCP: linear, playwright · project only`).
    pub fn summary(&self) -> String {
        let mut out = if self.servers.is_empty() {
            "MCP: none".to_owned()
        } else {
            format!(
                "MCP: {}",
                self.servers
                    .iter()
                    .map(|s| s.def.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        if self.policy == McpSourcesPolicy::Project {
            out.push_str(" · project only");
        }
        if !self.withheld.is_empty() {
            out.push_str(&format!(" · {} withheld", self.withheld.len()));
        }
        out
    }

    fn is_required(&self, name: &str, origin: Option<McpOrigin>) -> bool {
        self.server(name).is_some_and(|s| s.required)
            || (self.policy == McpSourcesPolicy::Project
                && origin.is_some_and(McpOrigin::is_project))
    }

    fn push_problem(
        &mut self,
        server: Option<&str>,
        origin: Option<McpOrigin>,
        reason: String,
        fix: McpFix,
    ) {
        let blocking = match server {
            Some(name) => self.is_required(name, origin),
            None => false,
        };
        self.problems.push(McpProblem {
            server: server.map(str::to_owned),
            origin,
            reason,
            fix,
            blocking,
        });
    }

    fn drop_server(&mut self, name: &str) {
        self.servers.retain(|s| s.def.name != name);
    }
}

/// Chooses the servers one step or chat turn gets.
///
/// `allowlist` is the union of the agent's and the step's `mcp:` names; empty means every
/// server the policy allows.
pub fn resolve_mcp_servers(
    sources: &McpSources,
    policy: McpSourcesPolicy,
    allowlist: &[String],
) -> ResolvedMcpSet {
    let project_trusted = sources.project_trusted();
    let mut set = ResolvedMcpSet {
        policy,
        project_trusted,
        project_file: sources.project.file(),
        ..Default::default()
    };
    let requires_project = policy == McpSourcesPolicy::Project || !allowlist.is_empty();

    // Project servers.
    let mut project_names = BTreeSet::new();
    match &sources.project {
        ProjectMcpState::Absent => {}
        ProjectMcpState::Invalid { file, message } => {
            set.problems.push(McpProblem {
                server: None,
                origin: Some(McpOrigin::Project(*file)),
                reason: format!(
                    "The project's {} {message}, so none of its MCP servers load.",
                    file.relative_path()
                ),
                fix: McpFix::EditProjectFile(*file),
                blocking: requires_project,
            });
        }
        ProjectMcpState::Loaded(config) => {
            let origin = McpOrigin::Project(config.file);
            for server in &config.servers {
                project_names.insert(server.name.clone());
            }
            for entry in &config.invalid {
                project_names.insert(entry.name.clone());
                set.problems.push(McpProblem {
                    server: Some(entry.name.clone()),
                    origin: Some(origin),
                    reason: format!("can't be used: {}", entry.reason),
                    fix: McpFix::EditProjectFile(config.file),
                    blocking: policy == McpSourcesPolicy::Project
                        || allowlist.contains(&entry.name),
                });
            }
            let enabled: Vec<&McpServerDef> =
                config.servers.iter().filter(|s| !s.disabled).collect();
            if !enabled.is_empty() {
                if !sources.allow_project_servers {
                    set.problems.push(McpProblem {
                        server: None,
                        origin: Some(origin),
                        reason: format!(
                            "Project MCP servers are turned off in Preferences › Extensions, so the {} servers in {} don't load.",
                            enabled.len(),
                            config.file.relative_path()
                        ),
                        fix: McpFix::AllowProjectServers,
                        blocking: requires_project,
                    });
                } else if !project_trusted {
                    let reason = if sources.approved_fingerprint.is_some() {
                        format!(
                            "{} changed since you approved it. Review it before its servers load again.",
                            config.file.relative_path()
                        )
                    } else {
                        format!(
                            "{} declares {} MCP servers. Review and approve them before they load.",
                            config.file.relative_path(),
                            enabled.len()
                        )
                    };
                    set.problems.push(McpProblem {
                        server: None,
                        origin: Some(origin),
                        reason,
                        fix: McpFix::ApproveProjectServers,
                        blocking: requires_project,
                    });
                } else {
                    for server in enabled {
                        set.servers.push(ResolvedMcpServer {
                            def: server.clone(),
                            origin,
                            required: false,
                        });
                    }
                }
            }
        }
    }

    // Extras: project extras first, so they win over user extras of the same name.
    let mut taken: BTreeSet<String> = project_names.clone();
    for (origin, extras) in [
        (McpOrigin::ProjectExtra, &sources.project_extras),
        (McpOrigin::UserExtra, &sources.user_extras),
    ] {
        for server in extras.iter().filter(|s| !s.disabled) {
            if project_names.contains(&server.name) {
                set.problems.push(McpProblem {
                    server: Some(server.name.clone()),
                    origin: Some(origin),
                    reason: "has the same name as a project server, so the project's definition is used. Rename yours to use both.".to_owned(),
                    fix: McpFix::EditExtra {
                        origin,
                        name: server.name.clone(),
                    },
                    blocking: false,
                });
                continue;
            }
            if !taken.insert(server.name.clone()) {
                set.problems.push(McpProblem {
                    server: Some(server.name.clone()),
                    origin: Some(origin),
                    reason: "has the same name as one of your servers for this project, which is used instead.".to_owned(),
                    fix: McpFix::EditExtra {
                        origin,
                        name: server.name.clone(),
                    },
                    blocking: false,
                });
                continue;
            }
            if policy == McpSourcesPolicy::Project {
                set.withheld.push((server.name.clone(), origin));
            } else {
                set.servers.push(ResolvedMcpServer {
                    def: server.clone(),
                    origin,
                    required: false,
                });
            }
        }
    }

    // Allowlist from the agent and step: narrows, and every name on it is required.
    if !allowlist.is_empty() {
        let wanted: BTreeSet<&str> = allowlist.iter().map(String::as_str).collect();
        let mut kept = Vec::new();
        for mut server in std::mem::take(&mut set.servers) {
            if wanted.contains(server.def.name.as_str()) {
                server.required = true;
                kept.push(server);
            } else if !server.origin.is_project() {
                set.withheld.push((server.def.name.clone(), server.origin));
            }
        }
        set.servers = kept;
        for name in allowlist {
            if set.server(name).is_some() {
                continue;
            }
            let already_reported = set
                .problems
                .iter()
                .any(|p| p.server.as_deref() == Some(name.as_str()));
            if already_reported || (project_names.contains(name) && !project_trusted) {
                // The file/approval problem above already says why it's missing.
                for problem in set.problems.iter_mut() {
                    if problem.server.as_deref() == Some(name.as_str()) || problem.server.is_none()
                    {
                        problem.blocking = true;
                    }
                }
                continue;
            }
            let reason = if set.withheld.iter().any(|(n, _)| n == name) {
                "is needed by this step, but this workflow only allows project servers.".to_owned()
            } else {
                "is needed by this step, but no project or user server has that name.".to_owned()
            };
            set.problems.push(McpProblem {
                server: Some(name.clone()),
                origin: None,
                reason,
                fix: match set.project_file {
                    Some(file) => McpFix::EditProjectFile(file),
                    None => McpFix::None,
                },
                blocking: true,
            });
        }
    }

    set
}

/// Preflight check backed by [`ResolvedSecrets`]:
/// - Keeps `PATH` resolution wired strictly to plain `plain_path_env` + host `PATH`.
/// - Existing secret records that failed to read (`resolved.failures`) ALWAYS block (never masked
///   by `${VAR:-default}`), returning dedicated [`SecretProblem`]s.
/// - Excluded / unshared secrets (`resolved.strip_from_child_env`) never fall through to host env.
pub fn preflight_mcp_servers_with_secrets(
    set: &mut ResolvedMcpSet,
    resolved: &ResolvedSecrets,
    plain_path_env: Option<&str>,
    project_id: Option<ProjectId>,
) -> Vec<SecretProblem> {
    use secrecy::ExposeSecret;

    type FailedMcpRecord = (String, McpOrigin, Vec<(String, McpFix, bool)>);
    let mut secret_problems: Vec<SecretProblem> = Vec::new();
    let mut failed_mcp: Vec<FailedMcpRecord> = Vec::new();

    for server in &set.servers {
        let mut reasons: Vec<(String, McpFix, bool)> = Vec::new();
        let texts = server_texts(&server.def);
        for var in server.def.env_references() {
            // 1. Fail-stop: an existing secret record that failed to read ALWAYS blocks,
            //    even if `${VAR:-default}` is present or the server is optional.
            if let Some((_, failure)) = resolved
                .failures
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&var))
            {
                let problem = failure.to_problem(project_id);
                reasons.push((problem.reason.clone(), McpFix::None, true));
                if !secret_problems
                    .iter()
                    .any(|p| p.dedupe_key() == problem.dedupe_key())
                {
                    secret_problems.push(problem);
                }
                continue;
            }

            let has_default = texts.iter().any(|t| reference_has_default(t, &var));

            // 2. Exclusion barrier: excluded or unshared App secrets NEVER fall through to host env.
            let is_stripped = resolved
                .strip_from_child_env
                .iter()
                .any(|k| k.eq_ignore_ascii_case(&var));
            let resolved_val = resolved
                .values
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&var))
                .map(|(_, rv)| rv.value.expose_secret());
            let present = if let Some(val) = resolved_val {
                !val.is_empty()
            } else if is_stripped {
                false
            } else {
                std::env::var(&var).is_ok_and(|v| !v.is_empty())
            };

            if !has_default && !present {
                let blocking = set.is_required(&server.def.name, Some(server.origin));
                reasons.push((
                    format!(
                        "needs {var}, which isn't set. Add it in Project Settings › Environment & Secrets."
                    ),
                    McpFix::SetEnvVar(var.clone()),
                    blocking,
                ));
            }
        }
        if let McpTransport::Stdio { command, .. } = &server.def.transport
            && env_references(command).is_empty()
            && !command_exists(command, plain_path_env)
        {
            let blocking = set.is_required(&server.def.name, Some(server.origin));
            reasons.push((
                format!(
                    "runs '{command}', which isn't installed or isn't on the PATH agents get. Install it, or set PATH in Project Settings › Environment."
                ),
                McpFix::InstallCommand(command.clone()),
                blocking,
            ));
        }
        if !reasons.is_empty() {
            failed_mcp.push((server.def.name.clone(), server.origin, reasons));
        }
    }

    for (name, origin, reasons) in failed_mcp {
        for (reason, fix, blocking) in reasons {
            let fix = match (&fix, origin) {
                (McpFix::InstallCommand(_), McpOrigin::ProjectExtra | McpOrigin::UserExtra) => {
                    McpFix::EditExtra {
                        origin,
                        name: name.clone(),
                    }
                }
                _ => fix,
            };
            set.problems.push(McpProblem {
                server: Some(name.clone()),
                origin: Some(origin),
                reason,
                fix,
                blocking,
            });
        }
        set.drop_server(&name);
    }

    secret_problems
}

/// Checks what Bruk can before starting the agent: `${VAR}`s that aren't set and commands
/// that aren't on the `PATH`. Servers that fail are dropped from the set (the agent would only
/// fail to start them) and recorded as problems.
///
/// `lookup` resolves variables the way the agent's process will see them (the project's
/// environment, then Bruk's own).
pub fn preflight_mcp_servers(set: &mut ResolvedMcpSet, lookup: &dyn Fn(&str) -> Option<String>) {
    let mut failed = Vec::new();
    for server in &set.servers {
        let mut reasons: Vec<(String, McpFix)> = Vec::new();
        let texts = server_texts(&server.def);
        for var in server.def.env_references() {
            let has_default = texts.iter().any(|t| reference_has_default(t, &var));
            if !has_default && lookup(&var).is_none_or(|v| v.is_empty()) {
                reasons.push((
                    format!(
                        "needs {var}, which isn't set. Add it in Project Settings › Environment."
                    ),
                    McpFix::SetEnvVar(var.clone()),
                ));
            }
        }
        if let McpTransport::Stdio { command, .. } = &server.def.transport
            && env_references(command).is_empty()
            && !command_exists(command, lookup("PATH").as_deref())
        {
            reasons.push((
                format!(
                    "runs '{command}', which isn't installed or isn't on the PATH agents get. Install it, or set PATH in Project Settings › Environment."
                ),
                McpFix::InstallCommand(command.clone()),
            ));
        }
        if !reasons.is_empty() {
            failed.push((server.def.name.clone(), server.origin, reasons));
        }
    }
    for (name, origin, reasons) in failed {
        for (reason, fix) in reasons {
            let fix = match (&fix, origin) {
                (McpFix::InstallCommand(_), McpOrigin::ProjectExtra | McpOrigin::UserExtra) => {
                    McpFix::EditExtra {
                        origin,
                        name: name.clone(),
                    }
                }
                _ => fix,
            };
            set.push_problem(Some(&name), Some(origin), reason, fix);
        }
        set.drop_server(&name);
    }
}

fn server_texts(def: &McpServerDef) -> Vec<&str> {
    match &def.transport {
        McpTransport::Stdio { command, args, env } => std::iter::once(command.as_str())
            .chain(args.iter().map(String::as_str))
            .chain(env.values().map(String::as_str))
            .collect(),
        McpTransport::Http { url, headers, .. } => std::iter::once(url.as_str())
            .chain(headers.values().map(String::as_str))
            .collect(),
    }
}

fn command_exists(command: &str, path_env: Option<&str>) -> bool {
    let command = command.trim();
    if command.contains('/') {
        return crate::expand_tilde(command).is_file();
    }
    let path = path_env
        .map(str::to_owned)
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    path.split(':')
        .filter(|dir| !dir.is_empty())
        .any(|dir| is_executable(&Path::new(dir).join(command)))
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

// ---------------------------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------------------------

/// The `--mcp-config` document for Claude. `${VAR}`s stay as they are: Claude expands them
/// from its environment, so no secret is written to disk.
pub fn claude_mcp_config(set: &ResolvedMcpSet) -> Value {
    let mut servers = Map::new();
    for server in &set.servers {
        let mut entry = Map::new();
        match &server.def.transport {
            McpTransport::Stdio { command, args, env } => {
                entry.insert("type".into(), "stdio".into());
                entry.insert("command".into(), command.clone().into());
                entry.insert(
                    "args".into(),
                    Value::Array(args.iter().cloned().map(Value::String).collect()),
                );
                if !env.is_empty() {
                    entry.insert("env".into(), string_object(env));
                }
            }
            McpTransport::Http { url, headers, sse } => {
                entry.insert("type".into(), if *sse { "sse" } else { "http" }.into());
                entry.insert("url".into(), url.clone().into());
                if !headers.is_empty() {
                    entry.insert("headers".into(), string_object(headers));
                }
            }
        }
        servers.insert(server.def.name.clone(), Value::Object(entry));
    }
    serde_json::json!({ "mcpServers": servers })
}

fn string_object(map: &BTreeMap<String, String>) -> Value {
    Value::Object(
        map.iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect(),
    )
}

/// How the servers reach Codex: `-c mcp_servers.<name>={…}` overrides, plus variables Bruk
/// sets in Codex's environment so it can forward them to servers (`env_vars`) without the
/// values appearing in arguments or files.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodexMcpDelivery {
    /// `key=value` strings, each passed after `-c`.
    pub config_overrides: Vec<String>,
    /// Variables to set in Codex's own environment.
    pub env: BTreeMap<String, String>,
    /// Keys in [`Self::env`] whose expanded value came from an `McpOnly` secret (not plain vars),
    /// so `shell_environment_policy.exclude` strips only those from Codex's agent shell.
    pub mcp_only_mapped_keys: BTreeSet<String>,
}

/// Translates the set for Codex with secret-exposure tracking and collision detection against
/// existing plain / `AgentAndSteps` environment variables.
pub fn codex_mcp_delivery_with_secrets(
    set: &mut ResolvedMcpSet,
    resolved: &ResolvedSecrets,
    existing_agent_env: &BTreeMap<String, String>,
) -> CodexMcpDelivery {
    use secrecy::ExposeSecret;
    let lookup = |key: &str| -> Option<String> {
        if let Some((_, rv)) = resolved
            .values
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
        {
            return Some(rv.value.expose_secret().to_owned());
        }
        if resolved
            .strip_from_child_env
            .iter()
            .any(|k| k.eq_ignore_ascii_case(key))
            || resolved
                .failures
                .keys()
                .any(|k| k.eq_ignore_ascii_case(key))
        {
            return None;
        }
        std::env::var(key).ok()
    };
    let is_mcp_only_var = |var: &str| -> bool {
        resolved
            .values
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(var))
            .is_some_and(|(_, rv)| {
                rv.exposure == Exposure::McpOnly && matches!(rv.origin, ValueOrigin::Secret { .. })
            })
    };

    let mut delivery = CodexMcpDelivery::default();
    let mut unsupported: Vec<(String, McpOrigin, String)> = Vec::new();
    for server in &set.servers {
        match codex_server_table_checked(
            &server.def,
            &lookup,
            &is_mcp_only_var,
            existing_agent_env,
            &mut delivery.env,
            &mut delivery.mcp_only_mapped_keys,
        ) {
            Ok(table) => delivery
                .config_overrides
                .push(format!("mcp_servers.{}={table}", server.def.name)),
            Err(reason) => unsupported.push((server.def.name.clone(), server.origin, reason)),
        }
    }
    for (name, origin, reason) in unsupported {
        let fix = match origin {
            McpOrigin::Project(file) => McpFix::EditProjectFile(file),
            _ => McpFix::EditExtra {
                origin,
                name: name.clone(),
            },
        };
        set.push_problem(
            Some(&name),
            Some(origin),
            format!("can't be used with Codex: {reason}"),
            fix,
        );
        set.drop_server(&name);
    }
    delivery
}

/// Translates the set for Codex. Servers Codex can't take (a `${VAR}` in a command, argument or
/// URL; SSE; a header that mixes text and a variable other than `Bearer ${VAR}`) are dropped
/// with a problem.
pub fn codex_mcp_delivery(
    set: &mut ResolvedMcpSet,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> CodexMcpDelivery {
    let mut delivery = CodexMcpDelivery::default();
    let empty_agent_env = BTreeMap::new();
    let no_mcp_only = |_: &str| false;
    let mut unsupported: Vec<(String, McpOrigin, String)> = Vec::new();
    for server in &set.servers {
        match codex_server_table_checked(
            &server.def,
            lookup,
            &no_mcp_only,
            &empty_agent_env,
            &mut delivery.env,
            &mut delivery.mcp_only_mapped_keys,
        ) {
            Ok(table) => delivery
                .config_overrides
                .push(format!("mcp_servers.{}={table}", server.def.name)),
            Err(reason) => unsupported.push((server.def.name.clone(), server.origin, reason)),
        }
    }
    for (name, origin, reason) in unsupported {
        let fix = match origin {
            McpOrigin::Project(file) => McpFix::EditProjectFile(file),
            _ => McpFix::EditExtra {
                origin,
                name: name.clone(),
            },
        };
        set.push_problem(
            Some(&name),
            Some(origin),
            format!("can't be used with Codex: {reason}"),
            fix,
        );
        set.drop_server(&name);
    }
    delivery
}

fn codex_server_table_checked(
    def: &McpServerDef,
    lookup: &dyn Fn(&str) -> Option<String>,
    is_mcp_only_var: &dyn Fn(&str) -> bool,
    existing_agent_env: &BTreeMap<String, String>,
    process_env: &mut BTreeMap<String, String>,
    mcp_only_mapped_keys: &mut BTreeSet<String>,
) -> Result<String, String> {
    let mut fields: Vec<String> = Vec::new();
    match &def.transport {
        McpTransport::Stdio { command, args, env } => {
            if !env_references(command).is_empty() {
                return Err("a ${VAR} in 'command' (Codex doesn't expand variables there)".into());
            }
            if args.iter().any(|a| !env_references(a).is_empty()) {
                return Err(
                    "a ${VAR} in 'args' (Codex doesn't expand variables there; pass it in 'env')"
                        .into(),
                );
            }
            fields.push(format!("command={}", toml_string(command)));
            fields.push(format!(
                "args=[{}]",
                args.iter()
                    .map(|a| toml_string(a))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
            let mut literal = Vec::new();
            let mut forwarded = Vec::new();
            let mut staged_env: Vec<(String, String, bool)> = Vec::new();
            for (key, value) in env {
                let refs = env_references(value);
                if refs.is_empty() {
                    literal.push(format!("{}={}", toml_key(key), toml_string(value)));
                    continue;
                }
                let expanded = expand_references(value, lookup)
                    .ok_or_else(|| format!("'{key}' refers to a variable that isn't set"))?;
                // Reject collision with an existing plain / AgentAndSteps env var instead of clobbering it.
                if let Some(existing) = existing_agent_env.get(key)
                    && *existing != expanded
                {
                    return Err(format!(
                        "env key '{key}' collides with a project/agent environment variable of the same name; Codex shares one environment"
                    ));
                }
                if let Some(existing) = process_env.get(key)
                    && *existing != expanded
                {
                    return Err(format!(
                        "another server sets {key} to a different value; Codex shares one environment between servers"
                    ));
                }
                let uses_mcp_only = refs.iter().any(|r| is_mcp_only_var(r));
                staged_env.push((key.clone(), expanded, uses_mcp_only));
                forwarded.push(toml_string(key));
            }
            for (key, expanded, uses_mcp_only) in staged_env {
                process_env.insert(key.clone(), expanded);
                if uses_mcp_only {
                    mcp_only_mapped_keys.insert(key);
                }
            }
            if !literal.is_empty() {
                fields.push(format!("env={{{}}}", literal.join(",")));
            }
            if !forwarded.is_empty() {
                fields.push(format!("env_vars=[{}]", forwarded.join(",")));
            }
        }
        McpTransport::Http { url, headers, sse } => {
            if *sse {
                return Err("SSE servers (Codex only speaks streamable HTTP)".into());
            }
            if !env_references(url).is_empty() {
                return Err("a ${VAR} in the URL (Codex doesn't expand variables there)".into());
            }
            fields.push(format!("url={}", toml_string(url)));
            let mut literal = Vec::new();
            let mut from_env = Vec::new();
            for (key, value) in headers {
                if env_references(value).is_empty() {
                    literal.push(format!("{}={}", toml_key(key), toml_string(value)));
                } else if key.eq_ignore_ascii_case("authorization")
                    && let Some(var) = value
                        .trim()
                        .strip_prefix("Bearer ")
                        .and_then(|rest| sole_reference(rest))
                {
                    fields.push(format!("bearer_token_env_var={}", toml_string(var)));
                } else if let Some(var) = sole_reference(value) {
                    from_env.push(format!("{}={}", toml_key(key), toml_string(var)));
                } else {
                    return Err(format!(
                        "header '{key}' mixes text and a variable (Codex takes 'Bearer ${{VAR}}' or a header that is exactly '${{VAR}}')"
                    ));
                }
            }
            if !literal.is_empty() {
                fields.push(format!("http_headers={{{}}}", literal.join(",")));
            }
            if !from_env.is_empty() {
                fields.push(format!("env_http_headers={{{}}}", from_env.join(",")));
            }
        }
    }
    Ok(format!("{{{}}}", fields.join(",")))
}

fn toml_string(raw: &str) -> String {
    // JSON string escaping is valid TOML basic-string escaping.
    serde_json::to_string(raw).unwrap_or_else(|_| format!("\"{raw}\""))
}

fn toml_key(raw: &str) -> String {
    if !raw.is_empty()
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        raw.to_owned()
    } else {
        toml_string(raw)
    }
}

/// What Bruk can and can't do for Antigravity, which reads `.agents/mcp_config.json` from its
/// working directory on its own (and the user's `~/.gemini/config` servers with it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AntigravityMcpDelivery {
    /// Deny every MCP tool (`mcp(*)`): the project file isn't approved, so its servers must not
    /// be usable even though Antigravity starts them.
    pub deny_all_mcp: bool,
}

/// Records what doesn't reach Antigravity as problems and says whether MCP tools must be
/// denied. A project-only workflow can't run on Antigravity: it always loads the user's own
/// servers too.
pub fn antigravity_mcp_delivery(
    set: &mut ResolvedMcpSet,
    sources: &McpSources,
    allowlist: &[String],
) -> AntigravityMcpDelivery {
    let mut delivery = AntigravityMcpDelivery::default();
    if set.policy == McpSourcesPolicy::Project {
        set.problems.push(McpProblem {
            server: None,
            origin: None,
            reason: "This workflow only allows project MCP servers, but Antigravity always loads your own servers from ~/.gemini/config too. Run it with Claude or Codex.".to_owned(),
            fix: McpFix::ChangeProvider,
            blocking: true,
        });
    }
    let file = sources.project.file();
    if file == Some(ProjectMcpFile::AgentsMcpConfig) && !set.project_trusted {
        delivery.deny_all_mcp = true;
    }
    // Antigravity (`agy` v1.5.16) launches enabled `.agents/mcp_config.json` servers on its own
    // (even under `permissions.deny: ["mcp(*)"]`) and performs ZERO `${VAR}` expansion anywhere.
    // Block before spawning `agy` whenever any enabled server in `.agents/mcp_config.json` uses `${VAR}`.
    if let Some(config) = sources.project.config()
        && config.file == ProjectMcpFile::AgentsMcpConfig
    {
        for server in config.servers.iter().filter(|s| !s.disabled) {
            let refs = server.env_references();
            if !refs.is_empty() {
                set.problems.push(McpProblem {
                    server: Some(server.name.clone()),
                    origin: Some(McpOrigin::Project(ProjectMcpFile::AgentsMcpConfig)),
                    reason: format!(
                        "uses {} in .agents/mcp_config.json, which Antigravity does not expand and would send literally. Switch this step or chat to Claude or Codex.",
                        refs.iter()
                            .map(|r| format!("${{{r}}}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    fix: McpFix::ChangeProvider,
                    blocking: true,
                });
                set.drop_server(&server.name);
            }
        }
    }
    let mut not_delivered = Vec::new();
    for server in &set.servers {
        match server.origin {
            McpOrigin::Project(ProjectMcpFile::AgentsMcpConfig) => {}
            McpOrigin::Project(ProjectMcpFile::RootMcpJson) => not_delivered.push((
                server.def.name.clone(),
                server.origin,
                "isn't available to Antigravity, which only reads .agents/mcp_config.json. Move the project's servers there to use them with every provider.".to_owned(),
                McpFix::EditProjectFile(ProjectMcpFile::RootMcpJson),
            )),
            origin => not_delivered.push((
                server.def.name.clone(),
                origin,
                "isn't available to Antigravity, which can't be given extra servers. Add it to Antigravity with `agy mcp add` to use it there.".to_owned(),
                McpFix::None,
            )),
        }
    }
    for (name, origin, reason, fix) in not_delivered {
        set.push_problem(Some(&name), Some(origin), reason, fix);
        set.drop_server(&name);
    }
    if !allowlist.is_empty() && set.servers.len() > allowlist.len() {
        set.problems.push(McpProblem {
            server: None,
            origin: None,
            reason: "Antigravity loads every server in .agents/mcp_config.json; the agent's or step's `mcp:` list can't narrow them.".to_owned(),
            fix: McpFix::None,
            blocking: false,
        });
    }
    delivery
}

// ---------------------------------------------------------------------------------------------
// Failures reported by the providers once they start
// ---------------------------------------------------------------------------------------------

/// A server the provider reported as failed to start or connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRuntimeFailure {
    pub server: String,
    pub reason: String,
}

/// Reads Claude's `system`/`init` event (`"mcp_servers": [{"name", "status"}]`). Claude reports a
/// status but not why.
pub fn claude_init_failures(event: &Value) -> Vec<McpRuntimeFailure> {
    if event.get("type").and_then(Value::as_str) != Some("system")
        || event.get("subtype").and_then(Value::as_str) != Some("init")
    {
        return Vec::new();
    }
    let Some(servers) = event.get("mcp_servers").and_then(Value::as_array) else {
        return Vec::new();
    };
    servers
        .iter()
        .filter_map(|s| {
            let name = s.get("name").and_then(Value::as_str)?;
            let reason = match s.get("status").and_then(Value::as_str)? {
                "connected" | "pending" => return None,
                "needs-auth" => "needs you to sign in to it".to_owned(),
                "failed" => "failed to start or connect".to_owned(),
                other => format!("isn't available ({other})"),
            };
            Some(McpRuntimeFailure {
                server: name.to_owned(),
                reason,
            })
        })
        .collect()
}

/// Reads one line of Codex's stderr (with [`CODEX_MCP_RUST_LOG`]):
/// `… WARN … MCP server startup failed server_name="x" error=No such file or directory (os error 2)`.
pub fn codex_stderr_failure(line: &str) -> Option<McpRuntimeFailure> {
    let rest = &line[line.find("MCP server startup failed")?..];
    let name_start = rest.find("server_name=\"")? + "server_name=\"".len();
    let name_len = rest[name_start..].find('"')?;
    let server = &rest[name_start..name_start + name_len];
    let reason = rest
        .find("error=")
        .map(|i| rest[i + "error=".len()..].trim())
        .filter(|r| !r.is_empty())
        .unwrap_or("failed to start");
    Some(McpRuntimeFailure {
        server: server.to_owned(),
        reason: format!("failed to start: {reason}"),
    })
}

/// Reads Antigravity's log (`… connector.go:131] MCP server x: exec: "y": executable file not
/// found in $PATH`). It only logs servers whose command fails to start.
pub fn antigravity_log_failures(log: &str) -> Vec<McpRuntimeFailure> {
    let mut out: Vec<McpRuntimeFailure> = Vec::new();
    for line in log.lines() {
        let Some(idx) = line.find("] MCP server ") else {
            continue;
        };
        let rest = &line[idx + "] MCP server ".len()..];
        let Some((name, reason)) = rest.split_once(": ") else {
            continue;
        };
        if validate_server_name(name).is_err() || out.iter().any(|f| f.server == name) {
            continue;
        }
        out.push(McpRuntimeFailure {
            server: name.to_owned(),
            reason: format!("failed to start: {}", reason.trim()),
        });
    }
    out
}

/// Turns a provider's report into a problem for the set the step was given. `None` for servers
/// Bruk didn't hand over (the provider's own), so they don't raise notifications.
pub fn runtime_problem(set: &ResolvedMcpSet, failure: &McpRuntimeFailure) -> Option<McpProblem> {
    let server = set.server(&failure.server)?;
    let fix = match server.origin {
        McpOrigin::Project(file) => McpFix::EditProjectFile(file),
        origin => McpFix::EditExtra {
            origin,
            name: server.def.name.clone(),
        },
    };
    Some(McpProblem {
        server: Some(server.def.name.clone()),
        origin: Some(server.origin),
        reason: failure.reason.clone(),
        fix,
        blocking: set.is_required(&server.def.name, Some(server.origin)),
    })
}

/// Collects runtime failures once per server (Codex logs each one several times).
#[derive(Debug, Default)]
pub struct McpFailureTracker {
    seen: BTreeSet<String>,
}

impl McpFailureTracker {
    /// The problem for `failure` the first time a server fails; `None` for repeats and for
    /// servers Bruk didn't hand over.
    pub fn record(
        &mut self,
        set: &ResolvedMcpSet,
        failure: &McpRuntimeFailure,
    ) -> Option<McpProblem> {
        let problem = runtime_problem(set, failure)?;
        self.seen.insert(failure.server.clone()).then_some(problem)
    }

    /// The servers of `set` that no failure was recorded for: they reached the agent.
    pub fn healthy(&self, set: &ResolvedMcpSet) -> Vec<(String, McpOrigin)> {
        set.servers
            .iter()
            .filter(|s| !self.seen.contains(&s.def.name))
            .map(|s| (s.def.name.clone(), s.origin))
            .collect()
    }
}

/// `Path` of the Claude MCP config for a runtime directory.
pub fn claude_mcp_config_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(CLAUDE_MCP_CONFIG_FILE)
}

/// Every provider Bruk hands servers to itself.
pub fn provider_takes_extras(provider: ProviderKind) -> bool {
    matches!(provider, ProviderKind::Claude | ProviderKind::Codex)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio(name: &str, command: &str) -> McpServerDef {
        McpServerDef {
            name: name.to_owned(),
            transport: McpTransport::Stdio {
                command: command.to_owned(),
                args: Vec::new(),
                env: BTreeMap::new(),
            },
            disabled: false,
        }
    }

    fn loaded(file: ProjectMcpFile, servers: Vec<McpServerDef>) -> ProjectMcpState {
        ProjectMcpState::Loaded(ProjectMcpConfig {
            file,
            servers,
            invalid: Vec::new(),
            fingerprint: "abc".to_owned(),
        })
    }

    fn trusted(project: ProjectMcpState) -> McpSources {
        McpSources {
            project,
            allow_project_servers: true,
            approved_fingerprint: Some("abc".to_owned()),
            ..Default::default()
        }
    }

    fn names(set: &ResolvedMcpSet) -> Vec<&str> {
        set.servers.iter().map(|s| s.def.name.as_str()).collect()
    }

    #[test]
    fn agents_mcp_config_wins_over_root_mcp_json_and_they_never_merge() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".mcp.json"),
            r#"{"mcpServers":{"root":{"command":"a"}}}"#,
        )
        .unwrap();
        let state = load_project_mcp(dir.path());
        let config = state.config().unwrap();
        assert_eq!(config.file, ProjectMcpFile::RootMcpJson);
        assert_eq!(config.servers[0].name, "root");

        fs::create_dir_all(dir.path().join(".agents")).unwrap();
        fs::write(
            dir.path().join(".agents/mcp_config.json"),
            r#"{"mcpServers":{"agents":{"serverUrl":"https://x/mcp","headers":{"A":"b"}}}}"#,
        )
        .unwrap();
        let state = load_project_mcp(dir.path());
        let config = state.config().unwrap();
        assert_eq!(config.file, ProjectMcpFile::AgentsMcpConfig);
        assert_eq!(
            config.servers.iter().map(|s| &s.name).collect::<Vec<_>>(),
            ["agents"]
        );
        assert!(matches!(
            config.servers[0].transport,
            McpTransport::Http { ref url, .. } if url == "https://x/mcp"
        ));
        assert_eq!(config.fingerprint.len(), 64);
    }

    #[test]
    fn missing_project_file_is_absent_and_bad_json_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_project_mcp(dir.path()), ProjectMcpState::Absent);
        fs::write(dir.path().join(".mcp.json"), "{nope").unwrap();
        assert!(matches!(
            load_project_mcp(dir.path()),
            ProjectMcpState::Invalid {
                file: ProjectMcpFile::RootMcpJson,
                ..
            }
        ));
    }

    #[test]
    fn parses_antigravity_and_claude_shapes_and_reports_bad_entries() {
        let (servers, invalid) = parse_mcp_json(
            r#"{"mcpServers":{
                "st":{"command":"python3","args":["-I","x.py"],"env":{"TOK":"abc"},"disabled":false},
                "web":{"disabled":true,"headers":{"Authorization":"Bearer x"},"serverUrl":"https://e/mcp"},
                "old":{"type":"sse","url":"https://e/sse"},
                "bad":{"args":["x"]},
                "bad name":{"command":"x"}
            }}"#,
        )
        .unwrap();
        assert_eq!(servers.len(), 3);
        let web = servers.iter().find(|s| s.name == "web").unwrap();
        assert!(web.disabled);
        let old = servers.iter().find(|s| s.name == "old").unwrap();
        assert_eq!(old.transport.kind_label(), "sse");
        assert_eq!(invalid.len(), 2);
    }

    #[test]
    fn untrusted_or_changed_project_file_needs_approval() {
        let mut sources = trusted(loaded(
            ProjectMcpFile::AgentsMcpConfig,
            vec![stdio("a", "x")],
        ));
        sources.approved_fingerprint = None;
        let set = resolve_mcp_servers(&sources, McpSourcesPolicy::All, &[]);
        assert!(set.servers.is_empty());
        assert_eq!(set.problems[0].fix, McpFix::ApproveProjectServers);
        assert!(!set.problems[0].blocking);

        sources.approved_fingerprint = Some("old".to_owned());
        let set = resolve_mcp_servers(&sources, McpSourcesPolicy::Project, &[]);
        assert!(set.problems[0].reason.contains("changed"));
        assert!(
            set.problems[0].blocking,
            "project-only workflows need the project's servers"
        );
    }

    #[test]
    fn extras_only_add_and_never_replace_project_servers() {
        let mut sources = trusted(loaded(
            ProjectMcpFile::AgentsMcpConfig,
            vec![stdio("shared", "proj")],
        ));
        sources.project_extras = vec![stdio("shared", "mine"), stdio("pe", "x")];
        sources.user_extras = vec![stdio("pe", "y"), stdio("ue", "z")];
        let set = resolve_mcp_servers(&sources, McpSourcesPolicy::All, &[]);
        assert_eq!(names(&set), ["shared", "pe", "ue"]);
        let shared = set.server("shared").unwrap();
        assert!(shared.origin.is_project());
        assert_eq!(set.server("pe").unwrap().origin, McpOrigin::ProjectExtra);
        assert_eq!(set.problems.len(), 2);
        assert!(set.problems.iter().all(|p| !p.blocking));
    }

    #[test]
    fn project_only_policy_withholds_every_extra() {
        let mut sources = trusted(loaded(ProjectMcpFile::RootMcpJson, vec![stdio("p", "x")]));
        sources.project_extras = vec![stdio("pe", "x")];
        sources.user_extras = vec![stdio("ue", "x")];
        let set = resolve_mcp_servers(&sources, McpSourcesPolicy::Project, &[]);
        assert_eq!(names(&set), ["p"]);
        assert_eq!(set.withheld.len(), 2);
        assert!(set.summary().contains("project only"));
    }

    #[test]
    fn policies_only_get_stricter() {
        use McpSourcesPolicy::*;
        assert_eq!(All.stricter(Project), Project);
        assert_eq!(Project.stricter(All), Project);
        assert_eq!(All.stricter(All), All);
    }

    #[test]
    fn allowlist_narrows_and_makes_named_servers_required() {
        let mut sources = trusted(loaded(
            ProjectMcpFile::AgentsMcpConfig,
            vec![stdio("a", "x"), stdio("b", "x")],
        ));
        sources.user_extras = vec![stdio("u", "x")];
        let set = resolve_mcp_servers(
            &sources,
            McpSourcesPolicy::All,
            &["b".into(), "missing".into()],
        );
        assert_eq!(names(&set), ["b"]);
        assert!(set.server("b").unwrap().required);
        let missing = set
            .problems
            .iter()
            .find(|p| p.server.as_deref() == Some("missing"))
            .unwrap();
        assert!(missing.blocking);
    }

    #[test]
    fn preflight_reports_missing_variables_and_commands_and_drops_those_servers() {
        let mut env_server = stdio("tok", "sh");
        if let McpTransport::Stdio { env, .. } = &mut env_server.transport {
            env.insert("TOKEN".into(), "${LINEAR_TOKEN}".into());
            env.insert("OPT".into(), "${OPTIONAL:-x}".into());
        }
        let sources = trusted(loaded(
            ProjectMcpFile::AgentsMcpConfig,
            vec![
                env_server,
                stdio("nocmd", "definitely-not-a-real-binary-xyz"),
                stdio("ok", "sh"),
            ],
        ));
        let mut set = resolve_mcp_servers(&sources, McpSourcesPolicy::Project, &[]);
        let lookup = |k: &str| (k == "PATH").then(|| "/bin:/usr/bin".to_owned());
        preflight_mcp_servers(&mut set, &lookup);
        assert_eq!(names(&set), ["ok"]);
        assert_eq!(set.problems.len(), 2);
        assert!(set.problems.iter().all(|p| p.blocking));
        assert!(
            set.problems
                .iter()
                .any(|p| p.fix == McpFix::SetEnvVar("LINEAR_TOKEN".into()))
        );
        assert!(
            set.problems[1]
                .title()
                .contains("'nocmd' (.agents/mcp_config.json)")
        );
    }

    #[test]
    fn claude_config_keeps_variable_references() {
        let mut def = stdio("a", "npx");
        if let McpTransport::Stdio { env, .. } = &mut def.transport {
            env.insert("T".into(), "${T}".into());
        }
        let sources = trusted(loaded(ProjectMcpFile::AgentsMcpConfig, vec![def]));
        let set = resolve_mcp_servers(&sources, McpSourcesPolicy::All, &[]);
        let json = claude_mcp_config(&set);
        assert_eq!(json["mcpServers"]["a"]["env"]["T"], "${T}");
        assert_eq!(json["mcpServers"]["a"]["type"], "stdio");
    }

    #[test]
    fn codex_delivery_forwards_secrets_through_its_environment() {
        let (servers, _) = parse_mcp_json(
            r#"{"mcpServers":{
                "s":{"command":"npx","args":["-y","pkg"],"env":{"PLAIN":"1","TOKEN":"pre-${SECRET}"}},
                "h":{"url":"https://e/mcp","headers":{"Authorization":"Bearer ${GH}","X-Key":"${KEY}","X-Lit":"v"}},
                "argvar":{"command":"x","args":["--t=${SECRET}"]},
                "sse":{"type":"sse","url":"https://e/sse"}
            }}"#,
        )
        .unwrap();
        let sources = trusted(loaded(ProjectMcpFile::AgentsMcpConfig, servers));
        let mut set = resolve_mcp_servers(&sources, McpSourcesPolicy::All, &[]);
        let lookup = |k: &str| (k == "SECRET").then(|| "shh".to_owned());
        let delivery = codex_mcp_delivery(&mut set, &lookup);
        assert_eq!(names(&set), ["h", "s"]);
        assert_eq!(
            delivery.env.get("TOKEN").map(String::as_str),
            Some("pre-shh")
        );
        let s = delivery
            .config_overrides
            .iter()
            .find(|o| o.starts_with("mcp_servers.s="))
            .unwrap();
        assert!(s.contains(r#"env={PLAIN="1"}"#) && s.contains(r#"env_vars=["TOKEN"]"#));
        assert!(!s.contains("shh"), "secrets never go in arguments");
        let h = delivery
            .config_overrides
            .iter()
            .find(|o| o.starts_with("mcp_servers.h="))
            .unwrap();
        assert!(h.contains(r#"bearer_token_env_var="GH""#));
        assert!(h.contains(r#"env_http_headers={X-Key="KEY"}"#));
        assert!(h.contains(r#"http_headers={X-Lit="v"}"#));
        assert_eq!(set.problems.len(), 2);
    }

    #[test]
    fn antigravity_refuses_project_only_and_denies_unapproved_servers() {
        let mut sources = trusted(loaded(
            ProjectMcpFile::AgentsMcpConfig,
            vec![stdio("p", "x")],
        ));
        sources.approved_fingerprint = None;
        sources.user_extras = vec![stdio("u", "x")];
        let mut set = resolve_mcp_servers(&sources, McpSourcesPolicy::All, &[]);
        let delivery = antigravity_mcp_delivery(&mut set, &sources, &[]);
        assert!(delivery.deny_all_mcp);
        assert!(set.servers.is_empty(), "extras can't reach Antigravity");
        assert!(!set.has_blocking_problem());

        let sources = trusted(loaded(
            ProjectMcpFile::AgentsMcpConfig,
            vec![stdio("p", "x")],
        ));
        let mut set = resolve_mcp_servers(&sources, McpSourcesPolicy::Project, &[]);
        let delivery = antigravity_mcp_delivery(&mut set, &sources, &[]);
        assert!(!delivery.deny_all_mcp);
        assert!(set.has_blocking_problem());
    }

    #[test]
    fn reads_failures_from_each_provider() {
        let init: Value = serde_json::from_str(
            r#"{"type":"system","subtype":"init","mcp_servers":[{"name":"ok","status":"connected"},{"name":"b","status":"failed"},{"name":"r","status":"needs-auth"}]}"#,
        )
        .unwrap();
        let failures = claude_init_failures(&init);
        assert_eq!(
            failures
                .iter()
                .map(|f| f.server.as_str())
                .collect::<Vec<_>>(),
            ["b", "r"]
        );

        let line = r#"2026-11-24T20:04:36.129318Z  WARN mcp.request: codex_mcp::rmcp_client: MCP server startup failed server_name="broken" error=No such file or directory (os error 2)"#;
        let failure = codex_stderr_failure(line).unwrap();
        assert_eq!(failure.server, "broken");
        assert!(failure.reason.contains("No such file"));
        assert!(codex_stderr_failure("Reading additional input from stdin...").is_none());

        let log = "W1124 15:05:23.830780    1075 connector.go:131] MCP server projbroken: exec: \"nope\": executable file not found in $PATH\nW1124 15:05:21.862157 154 rpcs_plugin_auth.go:110] Not reloading plugin MCP servers: x";
        let failures = antigravity_log_failures(log);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].server, "projbroken");
    }

    #[test]
    fn runtime_failures_are_reported_once_and_only_for_bruk_servers() {
        let sources = trusted(loaded(
            ProjectMcpFile::AgentsMcpConfig,
            vec![stdio("p", "x")],
        ));
        let set = resolve_mcp_servers(&sources, McpSourcesPolicy::Project, &[]);
        let mut tracker = McpFailureTracker::default();
        let failure = McpRuntimeFailure {
            server: "p".into(),
            reason: "failed".into(),
        };
        let problem = tracker.record(&set, &failure).unwrap();
        assert!(problem.blocking);
        assert!(tracker.record(&set, &failure).is_none());
        let theirs = McpRuntimeFailure {
            server: "codex_apps".into(),
            reason: "x".into(),
        };
        assert!(tracker.record(&set, &theirs).is_none());
    }

    #[test]
    fn form_builds_stdio_and_http_servers() {
        let s =
            McpServerDef::from_form("pw", "npx", "-y '@playwright/mcp'", "A=1\nB=${B}").unwrap();
        assert!(
            matches!(&s.transport, McpTransport::Stdio { args, env, .. } if args == &["-y", "@playwright/mcp"] && env.len() == 2)
        );
        assert_eq!(s.env_references(), ["B"]);
        let h = McpServerDef::from_form("gh", "https://api/mcp", "", "Authorization: Bearer ${GH}")
            .unwrap();
        assert!(
            matches!(&h.transport, McpTransport::Http { headers, .. } if headers["Authorization"] == "Bearer ${GH}")
        );
        assert!(McpServerDef::from_form("bad name", "x", "", "").is_err());
        assert!(McpServerDef::from_form("u", "https://x", "-a", "").is_err());
    }
}
