//! Code-first, 3-layer workflow, composite, agent, and skill definitions.
//!
//! Bruk resolves definitions strictly from three deterministic layers with zero
//! user-global layer so workflow behavior never drifts across developer machines:
//!
//! 1. **Built-in** (`builtin/*`) — embedded into `factory-core`.
//! 2. **Modules** (`<module>/*`) — explicitly declared in `<project>/.agents/bruk/bruk.yaml`.
//! 3. **Project** (`project/*`) — `<project>/.agents/bruk/`.
//!
//! Precedence for unqualified references is `Project > Modules (last declared wins) > Built-in`.
//! Qualified references (`<namespace>/<id>`) pin an exact layer.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    mcp::{McpSourcesPolicy, ProjectMcpState},
    preferences::ProviderKind,
    project_settings::ProjectSandboxMode,
    workflow_editor::{
        ExitField, ExitOperator, LoopExhaustedAction, LoopRepeatMode, LoopWaitBetween,
    },
};

/// Relative root directory for project-checked-in Bruk definitions.
pub const BRUK_CONFIG_DIR: &str = ".agents/bruk";
/// Relative directory for project workflows inside [`BRUK_CONFIG_DIR`].
pub const BRUK_WORKFLOWS_SUBDIR: &str = "workflows";
/// Relative directory for project composites inside [`BRUK_CONFIG_DIR`].
pub const BRUK_COMPOSITES_SUBDIR: &str = "composites";
/// Relative directory for project agents inside [`BRUK_CONFIG_DIR`].
pub const BRUK_AGENTS_SUBDIR: &str = "agents";
/// Relative directory for project skills inside [`BRUK_CONFIG_DIR`].
pub const BRUK_SKILLS_SUBDIR: &str = "skills";
/// Manifest filename inside a project or module Bruk config root.
pub const BRUK_MANIFEST_FILE: &str = "bruk.yaml";
/// Namespace assigned to built-in definitions embedded in Bruk.
pub const BUILTIN_NAMESPACE: &str = "builtin";
/// Namespace assigned to project-local definitions under `<project>/.agents/bruk/`.
pub const PROJECT_NAMESPACE: &str = "project";

const DEFAULT_MANIFEST_SCHEMA_VERSION: u32 = 1;

const BUILTIN_AGENT_FILES: &[(&str, &str)] = &[
    ("planner", include_str!("../builtins/agents/planner.md")),
    (
        "implementer",
        include_str!("../builtins/agents/implementer.md"),
    ),
    ("reviewer", include_str!("../builtins/agents/reviewer.md")),
    ("pr-author", include_str!("../builtins/agents/pr-author.md")),
];

const BUILTIN_SKILL_FILES: &[(&str, &str)] = &[
    (
        "minimal-diffs",
        include_str!("../builtins/skills/minimal-diffs.md"),
    ),
    (
        "test-verification",
        include_str!("../builtins/skills/test-verification.md"),
    ),
];

const BUILTIN_COMPOSITE_FILES: &[(&str, &str)] = &[
    (
        "fix-until-tests-pass",
        include_str!("../builtins/composites/fix-until-tests-pass.yaml"),
    ),
    (
        "review-and-open-pr",
        include_str!("../builtins/composites/review-and-open-pr.yaml"),
    ),
];

const BUILTIN_WORKFLOW_FILES: &[(&str, &str)] = &[
    (
        "fix-issue",
        include_str!("../builtins/workflows/fix-issue.yaml"),
    ),
    (
        "review-pr",
        include_str!("../builtins/workflows/review-pr.yaml"),
    ),
];

/// Errors produced when loading, parsing, resolving, or saving Bruk definitions.
#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("invalid identifier '{0}': expected lowercase letters, digits, '-' or '_'")]
    InvalidIdentifier(String),
    #[error("invalid namespace '{0}': reserved or malformed module name")]
    InvalidModuleName(String),
    #[error("failed to read '{path}': {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write '{path}': {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse YAML in '{path}': {message}")]
    YamlParse { path: String, message: String },
    #[error("failed to serialize YAML: {0}")]
    YamlSerialize(String),
    #[error("invalid Markdown frontmatter in '{path}': {message}")]
    Frontmatter { path: String, message: String },
    #[error("module '{name}' could not be loaded: {message}")]
    ModuleLoad { name: String, message: String },
    #[error("agent '{0}' was not found in catalog")]
    AgentNotFound(String),
    #[error("cyclic agent `extends` chain detected: {0}")]
    AgentExtendsCycle(String),
    #[error("agent '{agent}' extends missing base agent '{base}'")]
    AgentExtendsNotFound { agent: String, base: String },
}

/// Which layer supplied a definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(tag = "layer", rename_all = "snake_case")]
pub enum DefinitionOrigin {
    #[default]
    BuiltIn,
    Module {
        name: String,
        source: String,
        file_path: PathBuf,
    },
    Project {
        file_path: PathBuf,
    },
}

impl DefinitionOrigin {
    /// Canonical namespace for this layer (`"builtin"`, module name, or `"project"`).
    pub fn namespace(&self) -> &str {
        match self {
            Self::BuiltIn => BUILTIN_NAMESPACE,
            Self::Module { name, .. } => name.as_str(),
            Self::Project { .. } => PROJECT_NAMESPACE,
        }
    }

    /// Compact label displayed in UI badges.
    pub fn badge_label(&self) -> String {
        match self {
            Self::BuiltIn => "Built-in".to_owned(),
            Self::Module { name, .. } => format!("Module · {name}"),
            Self::Project { .. } => "Project".to_owned(),
        }
    }

    /// Whether editing this definition must copy-on-write into `<project>/.agents/bruk/`.
    pub fn is_read_only(&self) -> bool {
        !matches!(self, Self::Project { .. })
    }

    /// Formats `<namespace>/<id>`.
    pub fn canonical_id(&self, id: &str) -> String {
        format!("{}/{id}", self.namespace())
    }

    /// Relative display path for project settings & inspector footers.
    pub fn display_path(&self, kind_subdir: &str, file_name: &str) -> String {
        match self {
            Self::BuiltIn => format!("builtin/{kind_subdir}/{file_name}"),
            Self::Module { name, .. } => format!("{name}/{kind_subdir}/{file_name}"),
            Self::Project { .. } => format!("{BRUK_CONFIG_DIR}/{kind_subdir}/{file_name}"),
        }
    }
}

/// Parsed reference to a definition (either unqualified `"reviewer"` or qualified `"acme/reviewer"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DefinitionRef {
    pub namespace: Option<String>,
    pub id: String,
}

impl DefinitionRef {
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Some((ns, id)) = trimmed.split_once('/') {
            let ns = ns.trim();
            let id = id.trim();
            if ns.is_empty() || id.is_empty() || id.contains('/') {
                return None;
            }
            Some(Self {
                namespace: Some(ns.to_owned()),
                id: id.to_owned(),
            })
        } else {
            Some(Self {
                namespace: None,
                id: trimmed.to_owned(),
            })
        }
    }
}

fn default_schema_version() -> u32 {
    DEFAULT_MANIFEST_SCHEMA_VERSION
}

fn default_true() -> bool {
    true
}

fn default_input_type() -> String {
    "string".to_owned()
}

fn default_max_attempts() -> u32 {
    3
}

/// Project or module manifest loaded from `<project>/.agents/bruk/bruk.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrukManifest {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub modules: Vec<ModuleDecl>,
}

impl Default for BrukManifest {
    fn default() -> Self {
        Self {
            schema_version: DEFAULT_MANIFEST_SCHEMA_VERSION,
            modules: Vec::new(),
        }
    }
}

/// One shared module dependency declared in `bruk.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleDecl {
    pub name: String,
    pub source: ModuleSource,
}

/// Where a shared module's definitions are loaded from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModuleSource {
    /// Local directory path (relative paths resolve against the project root).
    Path { path: String },
    /// Git repository (cloned/fetched into Bruk's module cache).
    Git {
        git: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        r#ref: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
}

impl ModuleSource {
    pub fn display_source(&self) -> String {
        match self {
            Self::Path { path } => path.clone(),
            Self::Git { git, r#ref, path } => {
                let mut out = git.clone();
                if let Some(git_ref) = r#ref.as_deref().filter(|s| !s.trim().is_empty()) {
                    out.push('@');
                    out.push_str(git_ref.trim());
                }
                if let Some(sub) = path.as_deref().filter(|s| !s.trim().is_empty()) {
                    out.push('#');
                    out.push_str(sub.trim());
                }
                out
            }
        }
    }
}

/// Policy controlling whether git modules may hit the network when loading a catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModuleFetchPolicy {
    /// Never invoke `git clone`/`git fetch`; only load already-cached git modules and local paths.
    #[default]
    CacheOnly,
    /// Clone missing git modules into the cache directory if not yet present.
    FetchIfMissing,
    /// Fetch and update cached git modules before loading.
    Refresh,
}

/// Options for [`BrukCatalog::load_for_project_with_options`].
#[derive(Debug, Clone, Default)]
pub struct CatalogLoadOptions {
    pub module_fetch_policy: ModuleFetchPolicy,
    /// Override cache directory for git modules (defaults to `<Bruk folder>/cache/modules`).
    pub module_cache_dir: Option<PathBuf>,
}

/// Tool allow/deny list declared in an agent definition.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AgentToolsPolicy {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
}

impl AgentToolsPolicy {
    pub fn is_empty(&self) -> bool {
        self.allow.is_empty() && self.deny.is_empty()
    }
}

/// YAML frontmatter of `agents/<id>.md`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
struct AgentFrontmatter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extends: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider: Option<ProviderKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sandbox: Option<ProjectSandboxMode>,
    #[serde(default, skip_serializing_if = "AgentToolsPolicy::is_empty")]
    tools: AgentToolsPolicy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    subagents: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    include_repo_instructions: Option<bool>,
    /// MCP servers this agent needs; when set, it gets only these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mcp: Vec<String>,
    /// `project` keeps the user's own MCP servers away from this agent.
    #[serde(default, skip_serializing_if = "McpSourcesPolicy::is_default")]
    mcp_sources: McpSourcesPolicy,
}

/// Raw agent definition parsed from `agents/<id>.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDef {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Optional base agent reference for strictly additive inheritance.
    pub extends: Option<String>,
    pub provider: Option<ProviderKind>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub sandbox: Option<ProjectSandboxMode>,
    pub tools: AgentToolsPolicy,
    pub skills: Vec<String>,
    pub subagents: Vec<String>,
    pub include_repo_instructions: Option<bool>,
    /// MCP servers this agent needs (`mcp:`); empty means every server the policy allows.
    #[serde(default)]
    pub mcp: Vec<String>,
    /// `mcp_sources: project` withholds the user's own MCP servers.
    #[serde(default)]
    pub mcp_sources: McpSourcesPolicy,
    pub instructions: String,
    #[serde(skip)]
    pub origin: DefinitionOrigin,
}

impl AgentDef {
    pub fn canonical_id(&self) -> String {
        self.origin.canonical_id(&self.id)
    }

    /// Parses an `agents/<id>.md` Markdown + YAML frontmatter file.
    pub fn parse_markdown(
        fallback_id: &str,
        raw: &str,
        origin: DefinitionOrigin,
        source_path: &str,
    ) -> Result<Self, CatalogError> {
        let (yaml, body) = split_frontmatter(raw, source_path)?;
        let fm: AgentFrontmatter = if yaml.trim().is_empty() {
            AgentFrontmatter::default()
        } else {
            serde_saphyr::from_str(&yaml).map_err(|err| CatalogError::YamlParse {
                path: source_path.to_owned(),
                message: err.to_string(),
            })?
        };
        let id = fm
            .id
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| fallback_id.to_owned());
        validate_identifier(&id)?;
        let name = fm
            .name
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| title_case_slug(&id));

        Ok(Self {
            id,
            name,
            description: fm.description.trim().to_owned(),
            extends: fm
                .extends
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
            provider: fm.provider,
            model: fm
                .model
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
            reasoning_effort: fm
                .reasoning_effort
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
            sandbox: fm.sandbox,
            tools: fm.tools,
            skills: dedup_strings(fm.skills),
            subagents: dedup_strings(fm.subagents),
            include_repo_instructions: fm.include_repo_instructions,
            mcp: dedup_strings(fm.mcp),
            mcp_sources: fm.mcp_sources,
            instructions: body.trim().to_owned(),
            origin,
        })
    }

    /// Serializes this agent definition back to Markdown with YAML frontmatter.
    pub fn to_markdown(&self) -> Result<String, CatalogError> {
        let fm = AgentFrontmatter {
            id: Some(self.id.clone()),
            name: Some(self.name.clone()),
            description: self.description.clone(),
            extends: self.extends.clone(),
            provider: self.provider,
            model: self.model.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
            sandbox: self.sandbox,
            tools: self.tools.clone(),
            skills: self.skills.clone(),
            subagents: self.subagents.clone(),
            include_repo_instructions: self.include_repo_instructions,
            mcp: self.mcp.clone(),
            mcp_sources: self.mcp_sources,
        };
        let yaml = serde_saphyr::to_string(&fm)
            .map_err(|err| CatalogError::YamlSerialize(err.to_string()))?;
        let body = self.instructions.trim();
        if body.is_empty() {
            Ok(format!("---\n{yaml}---\n"))
        } else {
            Ok(format!("---\n{yaml}---\n\n{body}\n"))
        }
    }
}

/// Fully resolved agent produced after applying any additive `extends` inheritance chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAgent {
    pub id: String,
    pub canonical_id: String,
    pub name: String,
    pub description: String,
    pub origin: DefinitionOrigin,
    /// Ordered canonical IDs from root base agent to this leaf agent.
    pub lineage: Vec<String>,
    pub provider: Option<ProviderKind>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub sandbox: Option<ProjectSandboxMode>,
    pub tools: AgentToolsPolicy,
    pub skills: Vec<String>,
    pub subagents: Vec<String>,
    /// Whether repo-checked-in `CLAUDE.md` / `AGENTS.md` files remain active (default `true`).
    pub include_repo_instructions: bool,
    /// MCP servers the agent (and its bases) need; empty means every allowed server.
    pub mcp: Vec<String>,
    /// The strictest `mcp_sources` along the `extends` chain.
    pub mcp_sources: McpSourcesPolicy,
    pub instructions: String,
    /// Non-fatal notices (e.g., child tried to change an already-set base scalar under `extends`).
    pub notices: Vec<String>,
}

/// YAML frontmatter of `skills/<id>.md` or `skills/<id>/SKILL.md`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
struct SkillFrontmatter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    description: String,
}

/// Reusable focused instruction snippet injected only when referenced by an agent or step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub instructions: String,
    #[serde(skip)]
    pub origin: DefinitionOrigin,
}

impl SkillDef {
    pub fn canonical_id(&self) -> String {
        self.origin.canonical_id(&self.id)
    }

    pub fn parse_markdown(
        fallback_id: &str,
        raw: &str,
        origin: DefinitionOrigin,
        source_path: &str,
    ) -> Result<Self, CatalogError> {
        let (yaml, body) = split_frontmatter(raw, source_path)?;
        let fm: SkillFrontmatter = if yaml.trim().is_empty() {
            SkillFrontmatter::default()
        } else {
            serde_saphyr::from_str(&yaml).map_err(|err| CatalogError::YamlParse {
                path: source_path.to_owned(),
                message: err.to_string(),
            })?
        };
        let id = fm
            .id
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| fallback_id.to_owned());
        validate_identifier(&id)?;
        let name = fm
            .name
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| title_case_slug(&id));

        Ok(Self {
            id,
            name,
            description: fm.description.trim().to_owned(),
            instructions: body.trim().to_owned(),
            origin,
        })
    }
}

/// Workflow trigger configuration persisted in `workflows/<id>.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowTriggersDef {
    #[serde(default = "default_true")]
    pub work_queue: bool,
    #[serde(default = "default_true")]
    pub ad_hoc: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issue_labels: Vec<String>,
}

impl Default for WorkflowTriggersDef {
    fn default() -> Self {
        Self {
            work_queue: true,
            ad_hoc: true,
            issue_labels: Vec::new(),
        }
    }
}

/// Typed input parameter declaration on a workflow or composite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputParamDef {
    #[serde(default = "default_input_type")]
    pub r#type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// Exit condition evaluated after each loop iteration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopExitConditionDef {
    pub step: String,
    pub field: ExitField,
    pub operator: ExitOperator,
    pub expected: String,
}

/// Agent invocation step (Claude CLI or Codex CLI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTaskStepDef {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<String>,
    /// MCP servers this step needs, on top of its agent's; when set, it gets only these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp: Vec<String>,
}

/// Deterministic shell command step run inside the workflow checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellCommandStepDef {
    pub id: String,
    pub name: String,
    pub command: String,
}

/// Human review / approval gate that suspends the run and posts to Inbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalGateStepDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub prompt: String,
}

/// Invocation of a reusable composite subgraph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositeStepDef {
    pub id: String,
    pub name: String,
    pub uses: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub with: BTreeMap<String, String>,
}

/// Repeat container wrapping one or more child steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopNodeDef {
    pub id: String,
    pub name: String,
    #[serde(default = "default_repeat_mode")]
    pub repeat: LoopRepeatMode,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_wait_between")]
    pub wait_between: LoopWaitBetween,
    #[serde(default = "default_true")]
    pub pass_previous_outputs: bool,
    #[serde(default)]
    pub discard_file_changes: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exit_conditions: Vec<LoopExitConditionDef>,
    #[serde(default = "default_exhausted_action")]
    pub on_exhausted: LoopExhaustedAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Box<WorkflowNodeDef>>,
    #[serde(default)]
    pub steps: Vec<WorkflowNodeDef>,
}

fn default_repeat_mode() -> LoopRepeatMode {
    LoopRepeatMode::UntilCheckPasses
}

fn default_wait_between() -> LoopWaitBetween {
    LoopWaitBetween::None
}

fn default_exhausted_action() -> LoopExhaustedAction {
    LoopExhaustedAction::FailRun
}

/// Posts a comment back to the linked GitHub issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentOnIssueStepDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub body: String,
}

/// Updates labels / status on the linked issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetIssueStatusStepDef {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_labels: Vec<String>,
}

/// Opens a pull request via `gh pr create`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenPullRequestStepDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,
}

/// Conditional guard step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionStepDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub expression: String,
}

/// Executable node in a workflow or composite YAML document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowNodeDef {
    AgentTask(AgentTaskStepDef),
    ShellCommand(ShellCommandStepDef),
    ApprovalGate(ApprovalGateStepDef),
    Composite(CompositeStepDef),
    Loop(LoopNodeDef),
    CommentOnIssue(CommentOnIssueStepDef),
    SetIssueStatus(SetIssueStatusStepDef),
    OpenPullRequest(OpenPullRequestStepDef),
    Condition(ConditionStepDef),
}

impl WorkflowNodeDef {
    pub fn id(&self) -> &str {
        match self {
            Self::AgentTask(s) => &s.id,
            Self::ShellCommand(s) => &s.id,
            Self::ApprovalGate(s) => &s.id,
            Self::Composite(s) => &s.id,
            Self::Loop(l) => &l.id,
            Self::CommentOnIssue(s) => &s.id,
            Self::SetIssueStatus(s) => &s.id,
            Self::OpenPullRequest(s) => &s.id,
            Self::Condition(s) => &s.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::AgentTask(s) => &s.name,
            Self::ShellCommand(s) => &s.name,
            Self::ApprovalGate(s) => &s.name,
            Self::Composite(s) => &s.name,
            Self::Loop(l) => &l.name,
            Self::CommentOnIssue(s) => &s.name,
            Self::SetIssueStatus(s) => &s.name,
            Self::OpenPullRequest(s) => &s.name,
            Self::Condition(s) => &s.name,
        }
    }
}

/// Reusable multi-step subgraph loaded from `composites/<id>.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositeDef {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, InputParamDef>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, String>,
    /// `project` runs this composite's steps with only the project's MCP servers, even inside
    /// a workflow that allows the user's own.
    #[serde(default, skip_serializing_if = "McpSourcesPolicy::is_default")]
    pub mcp_sources: McpSourcesPolicy,
    #[serde(default)]
    pub nodes: Vec<WorkflowNodeDef>,
    #[serde(skip)]
    pub origin: DefinitionOrigin,
}

impl CompositeDef {
    pub fn canonical_id(&self) -> String {
        self.origin.canonical_id(&self.id)
    }

    pub fn parse_yaml(
        fallback_id: &str,
        raw: &str,
        origin: DefinitionOrigin,
        source_path: &str,
    ) -> Result<Self, CatalogError> {
        let mut parsed: Self =
            serde_saphyr::from_str(raw).map_err(|err| CatalogError::YamlParse {
                path: source_path.to_owned(),
                message: err.to_string(),
            })?;
        if parsed.id.trim().is_empty() {
            parsed.id = fallback_id.to_owned();
        } else {
            parsed.id = parsed.id.trim().to_owned();
        }
        validate_identifier(&parsed.id)?;
        if parsed.name.trim().is_empty() {
            parsed.name = title_case_slug(&parsed.id);
        }
        parsed.origin = origin;
        Ok(parsed)
    }

    pub fn to_yaml(&self) -> Result<String, CatalogError> {
        serde_saphyr::to_string(self).map_err(|err| CatalogError::YamlSerialize(err.to_string()))
    }
}

/// Top-level workflow loaded from `workflows/<id>.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDef {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub triggers: WorkflowTriggersDef,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, InputParamDef>,
    /// `project` withholds the user's own MCP servers from every step, so the workflow runs
    /// with exactly the servers the repository declares.
    #[serde(default, skip_serializing_if = "McpSourcesPolicy::is_default")]
    pub mcp_sources: McpSourcesPolicy,
    #[serde(default)]
    pub nodes: Vec<WorkflowNodeDef>,
    #[serde(skip)]
    pub origin: DefinitionOrigin,
}

impl WorkflowDef {
    pub fn canonical_id(&self) -> String {
        self.origin.canonical_id(&self.id)
    }

    pub fn parse_yaml(
        fallback_id: &str,
        raw: &str,
        origin: DefinitionOrigin,
        source_path: &str,
    ) -> Result<Self, CatalogError> {
        let mut parsed: Self =
            serde_saphyr::from_str(raw).map_err(|err| CatalogError::YamlParse {
                path: source_path.to_owned(),
                message: err.to_string(),
            })?;
        if parsed.id.trim().is_empty() {
            parsed.id = fallback_id.to_owned();
        } else {
            parsed.id = parsed.id.trim().to_owned();
        }
        validate_identifier(&parsed.id)?;
        if parsed.name.trim().is_empty() {
            parsed.name = title_case_slug(&parsed.id);
        }
        parsed.origin = origin;
        Ok(parsed)
    }

    pub fn to_yaml(&self) -> Result<String, CatalogError> {
        serde_saphyr::to_string(self).map_err(|err| CatalogError::YamlSerialize(err.to_string()))
    }

    /// Counts executable leaf steps (expanding loop steps, counting composites as 1 top-level step).
    pub fn step_count(&self) -> usize {
        self.nodes
            .iter()
            .map(|node| match node {
                WorkflowNodeDef::Loop(lp) => lp.steps.len().max(1),
                _ => 1,
            })
            .sum()
    }
}

/// Severity of a catalog validation diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Warning,
    Error,
}

/// Non-fatal or blocking diagnostic discovered while loading or validating a catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogDiagnostic {
    pub severity: DiagnosticSeverity,
    pub subject: String,
    pub message: String,
}

/// Status summary for a module declared in `bruk.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedModuleInfo {
    pub name: String,
    pub source: String,
    pub resolved_dir: Option<PathBuf>,
    pub loaded: bool,
    pub error: Option<String>,
}

/// Unified 3-layer catalog (`Built-in -> Modules -> Project`) for one project.
#[derive(Debug, Clone, Default)]
pub struct BrukCatalog {
    pub project_root: Option<PathBuf>,
    pub manifest: BrukManifest,
    pub loaded_modules: Vec<LoadedModuleInfo>,
    pub load_diagnostics: Vec<CatalogDiagnostic>,
    /// The project's tracked MCP servers (`.agents/mcp_config.json`, else `.mcp.json`), read
    /// from the project's checkout when the catalog loads.
    pub project_mcp: ProjectMcpState,

    /// All definitions indexed by canonical `<namespace>/<id>`.
    agents_by_canonical: BTreeMap<String, AgentDef>,
    skills_by_canonical: BTreeMap<String, SkillDef>,
    composites_by_canonical: BTreeMap<String, CompositeDef>,
    workflows_by_canonical: BTreeMap<String, WorkflowDef>,

    /// Effective winning canonical ID for each unqualified `<id>`.
    effective_agents: BTreeMap<String, String>,
    effective_skills: BTreeMap<String, String>,
    effective_composites: BTreeMap<String, String>,
    effective_workflows: BTreeMap<String, String>,
}

impl BrukCatalog {
    /// Loads only the embedded built-in layer (`builtin/*`).
    pub fn builtin_only() -> Self {
        let mut catalog = Self::default();
        catalog.load_builtins();
        catalog
    }

    /// Loads the full 3-layer catalog (`Built-in -> Modules -> Project`) for `project_root`
    /// using cached modules (no network calls).
    pub fn load_for_project(project_root: impl AsRef<Path>) -> Self {
        Self::load_for_project_with_options(project_root, &CatalogLoadOptions::default())
    }

    /// Loads the full 3-layer catalog for `project_root` with explicit module fetch options.
    pub fn load_for_project_with_options(
        project_root: impl AsRef<Path>,
        options: &CatalogLoadOptions,
    ) -> Self {
        let project_root = project_root.as_ref().to_path_buf();
        let mut catalog = Self {
            project_root: Some(project_root.clone()),
            ..Self::default()
        };

        // Layer 1: Built-in defaults compiled into Bruk.
        catalog.load_builtins();

        let project_config_dir = project_root.join(BRUK_CONFIG_DIR);
        let manifest_path = project_config_dir.join(BRUK_MANIFEST_FILE);
        if manifest_path.is_file() {
            match fs::read_to_string(&manifest_path) {
                Ok(raw) => match serde_saphyr::from_str::<BrukManifest>(&raw) {
                    Ok(manifest) => {
                        catalog.manifest = manifest;
                    }
                    Err(err) => {
                        catalog.load_diagnostics.push(CatalogDiagnostic {
                            severity: DiagnosticSeverity::Error,
                            subject: manifest_path.display().to_string(),
                            message: format!("Invalid bruk.yaml manifest: {err}"),
                        });
                    }
                },
                Err(err) => {
                    catalog.load_diagnostics.push(CatalogDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        subject: manifest_path.display().to_string(),
                        message: format!("Could not read bruk.yaml: {err}"),
                    });
                }
            }
        }

        // Layer 2: Modules explicitly declared in `<project>/.agents/bruk/bruk.yaml`.
        let modules = catalog.manifest.modules.clone();
        for module in modules {
            if let Err(err) = validate_module_name(&module.name) {
                catalog.load_diagnostics.push(CatalogDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    subject: format!("module:{}", module.name),
                    message: err.to_string(),
                });
                catalog.loaded_modules.push(LoadedModuleInfo {
                    name: module.name.clone(),
                    source: module.source.display_source(),
                    resolved_dir: None,
                    loaded: false,
                    error: Some(err.to_string()),
                });
                continue;
            }

            match resolve_module_directory(&project_root, &module, options) {
                Ok(module_dir) => {
                    catalog.load_directory_layer(&module_dir, |file_path| {
                        DefinitionOrigin::Module {
                            name: module.name.clone(),
                            source: module.source.display_source(),
                            file_path,
                        }
                    });
                    catalog.loaded_modules.push(LoadedModuleInfo {
                        name: module.name,
                        source: module.source.display_source(),
                        resolved_dir: Some(module_dir),
                        loaded: true,
                        error: None,
                    });
                }
                Err(err) => {
                    catalog.load_diagnostics.push(CatalogDiagnostic {
                        severity: DiagnosticSeverity::Warning,
                        subject: format!("module:{}", module.name),
                        message: err.to_string(),
                    });
                    catalog.loaded_modules.push(LoadedModuleInfo {
                        name: module.name,
                        source: module.source.display_source(),
                        resolved_dir: None,
                        loaded: false,
                        error: Some(err.to_string()),
                    });
                }
            }
        }

        // Layer 3: Project-specific definitions in `<project>/.agents/bruk/`.
        if project_config_dir.is_dir() {
            catalog.load_directory_layer(&project_config_dir, |file_path| {
                DefinitionOrigin::Project { file_path }
            });
        }

        catalog.project_mcp = crate::mcp::load_project_mcp(&project_root);
        if let ProjectMcpState::Invalid { file, message } = &catalog.project_mcp {
            catalog.load_diagnostics.push(CatalogDiagnostic {
                severity: DiagnosticSeverity::Error,
                subject: file.relative_path().to_owned(),
                message: format!("{message}; none of the project's MCP servers load"),
            });
        }

        catalog
    }

    fn load_builtins(&mut self) {
        for &(id, raw) in BUILTIN_AGENT_FILES {
            let path = format!("builtin/agents/{id}.md");
            let agent = AgentDef::parse_markdown(id, raw, DefinitionOrigin::BuiltIn, &path)
                .expect("embedded builtin agent must be valid");
            self.insert_agent(agent);
        }
        for &(id, raw) in BUILTIN_SKILL_FILES {
            let path = format!("builtin/skills/{id}.md");
            let skill = SkillDef::parse_markdown(id, raw, DefinitionOrigin::BuiltIn, &path)
                .expect("embedded builtin skill must be valid");
            self.insert_skill(skill);
        }
        for &(id, raw) in BUILTIN_COMPOSITE_FILES {
            let path = format!("builtin/composites/{id}.yaml");
            let composite = CompositeDef::parse_yaml(id, raw, DefinitionOrigin::BuiltIn, &path)
                .expect("embedded builtin composite must be valid");
            self.insert_composite(composite);
        }
        for &(id, raw) in BUILTIN_WORKFLOW_FILES {
            let path = format!("builtin/workflows/{id}.yaml");
            let workflow = WorkflowDef::parse_yaml(id, raw, DefinitionOrigin::BuiltIn, &path)
                .expect("embedded builtin workflow must be valid");
            self.insert_workflow(workflow);
        }
    }

    fn load_directory_layer(
        &mut self,
        root: &Path,
        make_origin: impl Fn(PathBuf) -> DefinitionOrigin,
    ) {
        // Agents: `<root>/agents/*.md`
        let agents_dir = root.join(BRUK_AGENTS_SUBDIR);
        for file_path in sorted_files_with_extensions(&agents_dir, &["md"]) {
            let fallback_id = file_stem_or_empty(&file_path);
            let display = file_path.display().to_string();
            match fs::read_to_string(&file_path) {
                Ok(raw) => match AgentDef::parse_markdown(
                    &fallback_id,
                    &raw,
                    make_origin(file_path.clone()),
                    &display,
                ) {
                    Ok(agent) => self.insert_agent(agent),
                    Err(err) => self.load_diagnostics.push(CatalogDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        subject: display,
                        message: err.to_string(),
                    }),
                },
                Err(err) => self.load_diagnostics.push(CatalogDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    subject: display,
                    message: format!("Could not read agent file: {err}"),
                }),
            }
        }

        // Skills: `<root>/skills/*.md` and `<root>/skills/<id>/SKILL.md`
        let skills_dir = root.join(BRUK_SKILLS_SUBDIR);
        for (fallback_id, file_path) in sorted_skill_files(&skills_dir) {
            let display = file_path.display().to_string();
            match fs::read_to_string(&file_path) {
                Ok(raw) => match SkillDef::parse_markdown(
                    &fallback_id,
                    &raw,
                    make_origin(file_path.clone()),
                    &display,
                ) {
                    Ok(skill) => self.insert_skill(skill),
                    Err(err) => self.load_diagnostics.push(CatalogDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        subject: display,
                        message: err.to_string(),
                    }),
                },
                Err(err) => self.load_diagnostics.push(CatalogDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    subject: display,
                    message: format!("Could not read skill file: {err}"),
                }),
            }
        }

        // Composites: `<root>/composites/*.yaml` or `*.yml`
        let composites_dir = root.join(BRUK_COMPOSITES_SUBDIR);
        for file_path in sorted_files_with_extensions(&composites_dir, &["yaml", "yml"]) {
            let fallback_id = file_stem_or_empty(&file_path);
            let display = file_path.display().to_string();
            match fs::read_to_string(&file_path) {
                Ok(raw) => match CompositeDef::parse_yaml(
                    &fallback_id,
                    &raw,
                    make_origin(file_path.clone()),
                    &display,
                ) {
                    Ok(composite) => self.insert_composite(composite),
                    Err(err) => self.load_diagnostics.push(CatalogDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        subject: display,
                        message: err.to_string(),
                    }),
                },
                Err(err) => self.load_diagnostics.push(CatalogDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    subject: display,
                    message: format!("Could not read composite file: {err}"),
                }),
            }
        }

        // Workflows: `<root>/workflows/*.yaml` or `*.yml`
        let workflows_dir = root.join(BRUK_WORKFLOWS_SUBDIR);
        for file_path in sorted_files_with_extensions(&workflows_dir, &["yaml", "yml"]) {
            let fallback_id = file_stem_or_empty(&file_path);
            let display = file_path.display().to_string();
            match fs::read_to_string(&file_path) {
                Ok(raw) => match WorkflowDef::parse_yaml(
                    &fallback_id,
                    &raw,
                    make_origin(file_path.clone()),
                    &display,
                ) {
                    Ok(workflow) => self.insert_workflow(workflow),
                    Err(err) => self.load_diagnostics.push(CatalogDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        subject: display,
                        message: err.to_string(),
                    }),
                },
                Err(err) => self.load_diagnostics.push(CatalogDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    subject: display,
                    message: format!("Could not read workflow file: {err}"),
                }),
            }
        }
    }

    fn insert_agent(&mut self, agent: AgentDef) {
        let canonical = agent.canonical_id();
        self.effective_agents
            .insert(agent.id.clone(), canonical.clone());
        self.agents_by_canonical.insert(canonical, agent);
    }

    fn insert_skill(&mut self, skill: SkillDef) {
        let canonical = skill.canonical_id();
        self.effective_skills
            .insert(skill.id.clone(), canonical.clone());
        self.skills_by_canonical.insert(canonical, skill);
    }

    fn insert_composite(&mut self, composite: CompositeDef) {
        let canonical = composite.canonical_id();
        self.effective_composites
            .insert(composite.id.clone(), canonical.clone());
        self.composites_by_canonical.insert(canonical, composite);
    }

    fn insert_workflow(&mut self, workflow: WorkflowDef) {
        let canonical = workflow.canonical_id();
        self.effective_workflows
            .insert(workflow.id.clone(), canonical.clone());
        self.workflows_by_canonical.insert(canonical, workflow);
    }

    /// Returns all effective (winning-layer) agents sorted by ID.
    pub fn effective_agents(&self) -> Vec<&AgentDef> {
        self.effective_agents
            .values()
            .filter_map(|canonical| self.agents_by_canonical.get(canonical))
            .collect()
    }

    /// Returns all effective (winning-layer) skills sorted by ID.
    pub fn effective_skills(&self) -> Vec<&SkillDef> {
        self.effective_skills
            .values()
            .filter_map(|canonical| self.skills_by_canonical.get(canonical))
            .collect()
    }

    /// Returns all effective (winning-layer) composites sorted by ID.
    pub fn effective_composites(&self) -> Vec<&CompositeDef> {
        self.effective_composites
            .values()
            .filter_map(|canonical| self.composites_by_canonical.get(canonical))
            .collect()
    }

    /// Returns all effective (winning-layer) workflows sorted by ID.
    pub fn effective_workflows(&self) -> Vec<&WorkflowDef> {
        self.effective_workflows
            .values()
            .filter_map(|canonical| self.workflows_by_canonical.get(canonical))
            .collect()
    }

    /// Looks up a raw (un-merged) agent by qualified (`<ns>/<id>`) or unqualified (`<id>`) reference.
    pub fn raw_agent(&self, reference: &str) -> Option<&AgentDef> {
        let parsed = DefinitionRef::parse(reference)?;
        match parsed.namespace {
            Some(ns) => self.agents_by_canonical.get(&format!("{ns}/{}", parsed.id)),
            None => self
                .effective_agents
                .get(&parsed.id)
                .and_then(|canonical| self.agents_by_canonical.get(canonical)),
        }
    }

    /// Looks up a skill by qualified or unqualified reference.
    pub fn resolve_skill(&self, reference: &str) -> Option<&SkillDef> {
        let parsed = DefinitionRef::parse(reference)?;
        match parsed.namespace {
            Some(ns) => self.skills_by_canonical.get(&format!("{ns}/{}", parsed.id)),
            None => self
                .effective_skills
                .get(&parsed.id)
                .and_then(|canonical| self.skills_by_canonical.get(canonical)),
        }
    }

    /// Looks up a composite by qualified or unqualified reference.
    pub fn resolve_composite(&self, reference: &str) -> Option<&CompositeDef> {
        let parsed = DefinitionRef::parse(reference)?;
        match parsed.namespace {
            Some(ns) => self
                .composites_by_canonical
                .get(&format!("{ns}/{}", parsed.id)),
            None => self
                .effective_composites
                .get(&parsed.id)
                .and_then(|canonical| self.composites_by_canonical.get(canonical)),
        }
    }

    /// Looks up a workflow by qualified or unqualified reference.
    pub fn resolve_workflow(&self, reference: &str) -> Option<&WorkflowDef> {
        let parsed = DefinitionRef::parse(reference)?;
        match parsed.namespace {
            Some(ns) => self
                .workflows_by_canonical
                .get(&format!("{ns}/{}", parsed.id)),
            None => self
                .effective_workflows
                .get(&parsed.id)
                .and_then(|canonical| self.workflows_by_canonical.get(canonical)),
        }
    }

    /// Looks up a workflow by ID or display name (case-insensitive fallback for intake rules).
    pub fn resolve_workflow_by_id_or_name(&self, query: &str) -> Option<&WorkflowDef> {
        if let Some(wf) = self.resolve_workflow(query) {
            return Some(wf);
        }
        let needle = query.trim().to_ascii_lowercase();
        self.effective_workflows()
            .into_iter()
            .find(|wf| wf.name.to_ascii_lowercase() == needle)
    }

    /// Resolves an agent and applies its strictly additive `extends` inheritance chain.
    ///
    /// Rules for `extends`:
    /// - Instructions are concatenated (`base + "\n\n" + child`).
    /// - `skills`, `subagents`, `tools.allow`, and `tools.deny` are deduplicated unions
    ///   (base order preserved, child additions appended).
    /// - Scalar fields (`provider`, `model`, `reasoning_effort`, `sandbox`,
    ///   `include_repo_instructions`) are filled only when unset on the base; attempts to
    ///   mutate an already-set base scalar preserve the base value and record a notice.
    pub fn resolve_agent(&self, reference: &str) -> Result<ResolvedAgent, CatalogError> {
        let leaf = self
            .raw_agent(reference)
            .ok_or_else(|| CatalogError::AgentNotFound(reference.to_owned()))?;
        let mut chain = Vec::new();
        let mut visited = BTreeSet::new();
        self.collect_agent_chain(leaf, &mut visited, &mut chain)?;
        // `chain` is leaf -> ... -> root; reverse so we fold root -> ... -> leaf.
        chain.reverse();

        let mut lineage = Vec::with_capacity(chain.len());
        let mut provider = None;
        let mut model = None;
        let mut reasoning_effort = None;
        let mut sandbox = None;
        let mut include_repo_instructions = None;
        let mut allow_tools = Vec::new();
        let mut deny_tools = Vec::new();
        let mut skills = Vec::new();
        let mut subagents = Vec::new();
        let mut mcp = Vec::new();
        let mut mcp_sources = McpSourcesPolicy::All;
        let mut instruction_blocks = Vec::new();
        let mut notices = Vec::new();

        for agent in &chain {
            let canonical = agent.canonical_id();
            lineage.push(canonical.clone());

            merge_additive_scalar(
                &mut provider,
                agent.provider,
                "provider",
                &canonical,
                &mut notices,
            );
            merge_additive_scalar_string(
                &mut model,
                agent.model.as_deref(),
                "model",
                &canonical,
                &mut notices,
            );
            merge_additive_scalar_string(
                &mut reasoning_effort,
                agent.reasoning_effort.as_deref(),
                "reasoning_effort",
                &canonical,
                &mut notices,
            );
            merge_additive_scalar(
                &mut sandbox,
                agent.sandbox,
                "sandbox",
                &canonical,
                &mut notices,
            );
            merge_additive_scalar(
                &mut include_repo_instructions,
                agent.include_repo_instructions,
                "include_repo_instructions",
                &canonical,
                &mut notices,
            );

            append_unique(&mut allow_tools, &agent.tools.allow);
            append_unique(&mut deny_tools, &agent.tools.deny);
            append_unique(&mut skills, &agent.skills);
            append_unique(&mut subagents, &agent.subagents);
            append_unique(&mut mcp, &agent.mcp);
            mcp_sources = mcp_sources.stricter(agent.mcp_sources);

            let trimmed_body = agent.instructions.trim();
            if !trimmed_body.is_empty() {
                instruction_blocks.push(trimmed_body);
            }
        }

        let description = chain
            .iter()
            .rev()
            .find(|a| !a.description.trim().is_empty())
            .map(|a| a.description.clone())
            .unwrap_or_default();

        Ok(ResolvedAgent {
            id: leaf.id.clone(),
            canonical_id: leaf.canonical_id(),
            name: leaf.name.clone(),
            description,
            origin: leaf.origin.clone(),
            lineage,
            provider,
            model,
            reasoning_effort,
            sandbox,
            tools: AgentToolsPolicy {
                allow: allow_tools,
                deny: deny_tools,
            },
            skills,
            subagents,
            include_repo_instructions: include_repo_instructions.unwrap_or(true),
            mcp,
            mcp_sources,
            instructions: instruction_blocks.join("\n\n"),
            notices,
        })
    }

    fn collect_agent_chain<'a>(
        &'a self,
        current: &'a AgentDef,
        visited: &mut BTreeSet<String>,
        chain: &mut Vec<&'a AgentDef>,
    ) -> Result<(), CatalogError> {
        let canonical = current.canonical_id();
        if !visited.insert(canonical.clone()) {
            let mut cycle_nodes: Vec<String> = chain.iter().map(|a| a.canonical_id()).collect();
            cycle_nodes.push(canonical);
            return Err(CatalogError::AgentExtendsCycle(cycle_nodes.join(" -> ")));
        }
        chain.push(current);

        if let Some(base_ref) = current.extends.as_deref() {
            let base = self.resolve_agent_base(current, base_ref).ok_or_else(|| {
                CatalogError::AgentExtendsNotFound {
                    agent: canonical.clone(),
                    base: base_ref.to_owned(),
                }
            })?;
            self.collect_agent_chain(base, visited, chain)?;
        }
        Ok(())
    }

    /// Resolves `extends` targets cleanly even when a project agent shares an ID with a lower
    /// layer (e.g. `project/implementer` writing `extends: builtin/implementer` or `extends: implementer`).
    fn resolve_agent_base<'a>(
        &'a self,
        child: &'a AgentDef,
        base_ref: &str,
    ) -> Option<&'a AgentDef> {
        let parsed = DefinitionRef::parse(base_ref)?;
        if let Some(ns) = parsed.namespace {
            return self.agents_by_canonical.get(&format!("{ns}/{}", parsed.id));
        }

        // Unqualified `extends`: if it points at a different ID, resolve normally.
        if parsed.id != child.id {
            return self.raw_agent(&parsed.id);
        }

        // Unqualified self-ID `extends` (e.g. `project/implementer` extending `implementer`):
        // walk strictly lower layers (earlier modules, then builtin) so it doesn't self-cycle.
        let lower_namespaces = self.lower_namespaces_for(&child.origin);
        for ns in lower_namespaces {
            let key = format!("{ns}/{}", parsed.id);
            if let Some(found) = self.agents_by_canonical.get(&key) {
                return Some(found);
            }
        }
        None
    }

    fn lower_namespaces_for(&self, origin: &DefinitionOrigin) -> Vec<String> {
        let mut namespaces = Vec::new();
        match origin {
            DefinitionOrigin::Project { .. } => {
                for module in self.manifest.modules.iter().rev() {
                    namespaces.push(module.name.clone());
                }
                namespaces.push(BUILTIN_NAMESPACE.to_owned());
            }
            DefinitionOrigin::Module { name, .. } => {
                let mut seen_self = false;
                for module in self.manifest.modules.iter().rev() {
                    if seen_self {
                        namespaces.push(module.name.clone());
                    } else if module.name == *name {
                        seen_self = true;
                    }
                }
                namespaces.push(BUILTIN_NAMESPACE.to_owned());
            }
            DefinitionOrigin::BuiltIn => {}
        }
        namespaces
    }

    /// Validates all effective agents, composites, and workflows in this catalog.
    pub fn validate(&self) -> Vec<CatalogDiagnostic> {
        let mut diagnostics = self.load_diagnostics.clone();

        for agent in self.effective_agents() {
            match self.resolve_agent(&agent.canonical_id()) {
                Ok(resolved) => {
                    for notice in resolved.notices {
                        diagnostics.push(CatalogDiagnostic {
                            severity: DiagnosticSeverity::Warning,
                            subject: agent.canonical_id(),
                            message: notice,
                        });
                    }
                    for skill_ref in &resolved.skills {
                        if self.resolve_skill(skill_ref).is_none() {
                            diagnostics.push(CatalogDiagnostic {
                                severity: DiagnosticSeverity::Error,
                                subject: agent.canonical_id(),
                                message: format!("References unknown skill '{skill_ref}'"),
                            });
                        }
                    }
                    for subagent_ref in &resolved.subagents {
                        if self.raw_agent(subagent_ref).is_none() {
                            diagnostics.push(CatalogDiagnostic {
                                severity: DiagnosticSeverity::Error,
                                subject: agent.canonical_id(),
                                message: format!("References unknown subagent '{subagent_ref}'"),
                            });
                        }
                    }
                }
                Err(err) => diagnostics.push(CatalogDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    subject: agent.canonical_id(),
                    message: err.to_string(),
                }),
            }
        }

        for composite in self.effective_composites() {
            let subject = composite.canonical_id();
            if composite.nodes.is_empty() {
                diagnostics.push(CatalogDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    subject: subject.clone(),
                    message: "Composite must contain at least one step.".to_owned(),
                });
            }
            let mut visited_composites = BTreeSet::from([composite.canonical_id()]);
            self.validate_nodes(
                &subject,
                &composite.nodes,
                &mut visited_composites,
                &mut diagnostics,
            );
        }

        for workflow in self.effective_workflows() {
            let subject = workflow.canonical_id();
            if workflow.nodes.is_empty() {
                diagnostics.push(CatalogDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    subject: subject.clone(),
                    message: "Workflow must contain at least one step.".to_owned(),
                });
            }
            let mut visited_composites = BTreeSet::new();
            self.validate_nodes(
                &subject,
                &workflow.nodes,
                &mut visited_composites,
                &mut diagnostics,
            );
        }

        diagnostics
    }

    fn validate_nodes(
        &self,
        subject: &str,
        nodes: &[WorkflowNodeDef],
        visited_composites: &mut BTreeSet<String>,
        diagnostics: &mut Vec<CatalogDiagnostic>,
    ) {
        for node in nodes {
            match node {
                WorkflowNodeDef::AgentTask(step) => {
                    if let Some(agent_ref) = step.agent.as_deref()
                        && let Err(err) = self.resolve_agent(agent_ref)
                    {
                        diagnostics.push(CatalogDiagnostic {
                            severity: DiagnosticSeverity::Error,
                            subject: format!("{subject}::{}", step.id),
                            message: err.to_string(),
                        });
                    }
                    for skill_ref in &step.skills {
                        if self.resolve_skill(skill_ref).is_none() {
                            diagnostics.push(CatalogDiagnostic {
                                severity: DiagnosticSeverity::Error,
                                subject: format!("{subject}::{}", step.id),
                                message: format!("References unknown skill '{skill_ref}'"),
                            });
                        }
                    }
                }
                WorkflowNodeDef::ShellCommand(step) => {
                    if step.command.trim().is_empty() {
                        diagnostics.push(CatalogDiagnostic {
                            severity: DiagnosticSeverity::Error,
                            subject: format!("{subject}::{}", step.id),
                            message: "Shell command step has an empty command.".to_owned(),
                        });
                    }
                }
                WorkflowNodeDef::Composite(step) => match self.resolve_composite(&step.uses) {
                    Some(comp) => {
                        let canonical = comp.canonical_id();
                        if !visited_composites.insert(canonical.clone()) {
                            diagnostics.push(CatalogDiagnostic {
                                severity: DiagnosticSeverity::Error,
                                subject: format!("{subject}::{}", step.id),
                                message: format!("Cyclic composite reference '{canonical}'"),
                            });
                        } else {
                            for (input_name, param) in &comp.inputs {
                                if param.required
                                    && param.default.is_none()
                                    && !step.with.contains_key(input_name)
                                {
                                    diagnostics.push(CatalogDiagnostic {
                                        severity: DiagnosticSeverity::Warning,
                                        subject: format!("{subject}::{}", step.id),
                                        message: format!(
                                            "Composite '{}' expects required input '{input_name}'",
                                            comp.id
                                        ),
                                    });
                                }
                            }
                            self.validate_nodes(
                                subject,
                                &comp.nodes,
                                visited_composites,
                                diagnostics,
                            );
                            visited_composites.remove(&canonical);
                        }
                    }
                    None => diagnostics.push(CatalogDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        subject: format!("{subject}::{}", step.id),
                        message: format!("References unknown composite '{}'", step.uses),
                    }),
                },
                WorkflowNodeDef::Loop(lp) => {
                    if lp.max_attempts == 0 {
                        diagnostics.push(CatalogDiagnostic {
                            severity: DiagnosticSeverity::Error,
                            subject: format!("{subject}::{}", lp.id),
                            message: "Loop max_attempts must be at least 1.".to_owned(),
                        });
                    }
                    if lp.steps.is_empty() {
                        diagnostics.push(CatalogDiagnostic {
                            severity: DiagnosticSeverity::Error,
                            subject: format!("{subject}::{}", lp.id),
                            message: "Loop must contain at least one inner step.".to_owned(),
                        });
                    }
                    self.validate_nodes(subject, &lp.steps, visited_composites, diagnostics);
                    if let Some(fallback) = lp.fallback.as_deref() {
                        self.validate_nodes(
                            subject,
                            std::slice::from_ref(fallback),
                            visited_composites,
                            diagnostics,
                        );
                    }
                }
                WorkflowNodeDef::ApprovalGate(_)
                | WorkflowNodeDef::CommentOnIssue(_)
                | WorkflowNodeDef::SetIssueStatus(_)
                | WorkflowNodeDef::OpenPullRequest(_)
                | WorkflowNodeDef::Condition(_) => {}
            }
        }
    }

    /// Writes a workflow YAML file into `<project_root>/.agents/bruk/workflows/<id>.yaml`.
    pub fn save_project_workflow(
        project_root: impl AsRef<Path>,
        workflow: &WorkflowDef,
    ) -> Result<PathBuf, CatalogError> {
        validate_identifier(&workflow.id)?;
        let dir = project_root
            .as_ref()
            .join(BRUK_CONFIG_DIR)
            .join(BRUK_WORKFLOWS_SUBDIR);
        fs::create_dir_all(&dir).map_err(|source| CatalogError::WriteFile {
            path: dir.clone(),
            source,
        })?;
        let file_path = dir.join(format!("{}.yaml", workflow.id));
        let yaml = workflow.to_yaml()?;
        fs::write(&file_path, yaml).map_err(|source| CatalogError::WriteFile {
            path: file_path.clone(),
            source,
        })?;
        Ok(file_path)
    }

    /// Removes a project-local workflow file `<project_root>/.agents/bruk/workflows/<id>.yaml`.
    pub fn delete_project_workflow(
        project_root: impl AsRef<Path>,
        workflow_id: &str,
    ) -> Result<bool, CatalogError> {
        validate_identifier(workflow_id)?;
        let dir = project_root
            .as_ref()
            .join(BRUK_CONFIG_DIR)
            .join(BRUK_WORKFLOWS_SUBDIR);
        let mut removed = false;
        for ext in ["yaml", "yml"] {
            let file_path = dir.join(format!("{workflow_id}.{ext}"));
            if file_path.is_file() {
                fs::remove_file(&file_path).map_err(|source| CatalogError::WriteFile {
                    path: file_path,
                    source,
                })?;
                removed = true;
            }
        }
        Ok(removed)
    }

    /// Writes an agent Markdown file into `<project_root>/.agents/bruk/agents/<id>.md`.
    pub fn save_project_agent(
        project_root: impl AsRef<Path>,
        agent: &AgentDef,
    ) -> Result<PathBuf, CatalogError> {
        validate_identifier(&agent.id)?;
        let dir = project_root
            .as_ref()
            .join(BRUK_CONFIG_DIR)
            .join(BRUK_AGENTS_SUBDIR);
        fs::create_dir_all(&dir).map_err(|source| CatalogError::WriteFile {
            path: dir.clone(),
            source,
        })?;
        let file_path = dir.join(format!("{}.md", agent.id));
        let markdown = agent.to_markdown()?;
        fs::write(&file_path, markdown).map_err(|source| CatalogError::WriteFile {
            path: file_path.clone(),
            source,
        })?;
        Ok(file_path)
    }
}

/// Validates a definition ID (`a-z`, `0-9`, `-`, `_`, non-empty).
pub fn validate_identifier(id: &str) -> Result<(), CatalogError> {
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        return Err(CatalogError::InvalidIdentifier(id.to_owned()));
    }
    Ok(())
}

fn validate_module_name(name: &str) -> Result<(), CatalogError> {
    if name == BUILTIN_NAMESPACE || name == PROJECT_NAMESPACE || validate_identifier(name).is_err()
    {
        return Err(CatalogError::InvalidModuleName(name.to_owned()));
    }
    Ok(())
}

fn resolve_module_directory(
    project_root: &Path,
    module: &ModuleDecl,
    options: &CatalogLoadOptions,
) -> Result<PathBuf, CatalogError> {
    match &module.source {
        ModuleSource::Path { path } => {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                return Err(CatalogError::ModuleLoad {
                    name: module.name.clone(),
                    message: "module path cannot be empty".to_owned(),
                });
            }
            let raw = PathBuf::from(trimmed);
            let resolved = if raw.is_absolute() {
                raw
            } else {
                project_root.join(raw)
            };
            if !resolved.is_dir() {
                return Err(CatalogError::ModuleLoad {
                    name: module.name.clone(),
                    message: format!("directory '{}' does not exist", resolved.display()),
                });
            }
            Ok(resolved)
        }
        ModuleSource::Git { git, r#ref, path } => {
            let cache_root = match &options.module_cache_dir {
                Some(dir) => dir.clone(),
                None => default_module_cache_dir()?,
            };
            let slug = format!(
                "{}-{:016x}",
                sanitize_cache_segment(&module.name),
                fnv1a_64(git.as_bytes())
            );
            let repo_dir = cache_root.join(slug);

            if !repo_dir.join(".git").exists() {
                if options.module_fetch_policy == ModuleFetchPolicy::CacheOnly {
                    return Err(CatalogError::ModuleLoad {
                        name: module.name.clone(),
                        message: format!(
                            "git module '{}' is not cached locally yet (expected {})",
                            git,
                            repo_dir.display()
                        ),
                    });
                }
                fs::create_dir_all(&cache_root).map_err(|err| CatalogError::ModuleLoad {
                    name: module.name.clone(),
                    message: format!("failed to create module cache dir: {err}"),
                })?;
                run_git(
                    &cache_root,
                    &["clone", "--quiet", git, &repo_dir.to_string_lossy()],
                )
                .map_err(|msg| CatalogError::ModuleLoad {
                    name: module.name.clone(),
                    message: msg,
                })?;
                if let Some(git_ref) = r#ref.as_deref().filter(|s| !s.trim().is_empty()) {
                    run_git(&repo_dir, &["checkout", "--quiet", git_ref.trim()]).map_err(
                        |msg| CatalogError::ModuleLoad {
                            name: module.name.clone(),
                            message: msg,
                        },
                    )?;
                }
            } else if options.module_fetch_policy == ModuleFetchPolicy::Refresh {
                run_git(&repo_dir, &["fetch", "--all", "--tags", "--quiet"]).map_err(|msg| {
                    CatalogError::ModuleLoad {
                        name: module.name.clone(),
                        message: msg,
                    }
                })?;
                if let Some(git_ref) = r#ref.as_deref().filter(|s| !s.trim().is_empty()) {
                    run_git(&repo_dir, &["checkout", "--quiet", git_ref.trim()]).map_err(
                        |msg| CatalogError::ModuleLoad {
                            name: module.name.clone(),
                            message: msg,
                        },
                    )?;
                }
            }

            let target_dir = match path.as_deref().filter(|s| !s.trim().is_empty()) {
                Some(sub) => repo_dir.join(sub.trim()),
                None => repo_dir,
            };
            if !target_dir.is_dir() {
                return Err(CatalogError::ModuleLoad {
                    name: module.name.clone(),
                    message: format!(
                        "module directory '{}' was not found inside git repository",
                        target_dir.display()
                    ),
                });
            }
            Ok(target_dir)
        }
    }
}

/// Default cache directory for fetched git modules: `<Bruk folder>/cache/modules`
/// (never holds user-authored definitions).
pub fn default_module_cache_dir() -> Result<PathBuf, CatalogError> {
    crate::bruk_home::resolve_system_home()
        .map(|home| home.module_cache_dir())
        .ok_or_else(|| CatalogError::ModuleLoad {
            name: "cache".to_owned(),
            message: "the Bruk folder has not been set up yet".to_owned(),
        })
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|err| format!("failed to run git {:?}: {err}", args))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(format!("git {:?} failed: {stderr}", args))
    }
}

fn split_frontmatter(raw: &str, source_path: &str) -> Result<(String, String), CatalogError> {
    let normalized = raw.replace("\r\n", "\n");
    let trimmed_start = normalized.trim_start_matches('\u{feff}');
    let Some(after_open) = trimmed_start.strip_prefix("---\n") else {
        return Ok((String::new(), trimmed_start.to_owned()));
    };

    if let Some(body) = after_open.strip_prefix("---\n") {
        return Ok((String::new(), body.to_owned()));
    }
    if after_open == "---" {
        return Ok((String::new(), String::new()));
    }

    if let Some(close_idx) = after_open.find("\n---\n") {
        let yaml = after_open[..close_idx].to_owned();
        let body = after_open[close_idx + "\n---\n".len()..].to_owned();
        return Ok((yaml, body));
    }
    if let Some(yaml) = after_open.strip_suffix("\n---") {
        return Ok((yaml.to_owned(), String::new()));
    }

    Err(CatalogError::Frontmatter {
        path: source_path.to_owned(),
        message: "unterminated YAML frontmatter block (missing closing '---')".to_owned(),
    })
}

fn sorted_files_with_extensions(dir: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| extensions.iter().any(|e| ext.eq_ignore_ascii_case(e)))
        })
        .collect();
    files.sort();
    files
}

fn sorted_skill_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            items.push((file_stem_or_empty(&path), path));
        } else if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.is_file() {
                let folder_id = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_owned();
                items.push((folder_id, skill_md));
            }
        }
    }
    items.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    items
}

fn file_stem_or_empty(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned()
}

fn title_case_slug(slug: &str) -> String {
    slug.split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn dedup_strings(items: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        let trimmed = item.trim();
        if !trimmed.is_empty() && !out.iter().any(|existing: &String| existing == trimmed) {
            out.push(trimmed.to_owned());
        }
    }
    out
}

fn append_unique(dest: &mut Vec<String>, additions: &[String]) {
    for item in additions {
        let trimmed = item.trim();
        if !trimmed.is_empty() && !dest.iter().any(|existing| existing == trimmed) {
            dest.push(trimmed.to_owned());
        }
    }
}

fn merge_additive_scalar<T: Copy + PartialEq + std::fmt::Debug>(
    slot: &mut Option<T>,
    incoming: Option<T>,
    field: &str,
    canonical_id: &str,
    notices: &mut Vec<String>,
) {
    if let Some(value) = incoming {
        match slot {
            None => *slot = Some(value),
            Some(existing) if *existing == value => {}
            Some(_) => notices.push(format!(
                "Agent '{canonical_id}' sets '{field}' under `extends`, which is additive-only; base value kept."
            )),
        }
    }
}

fn merge_additive_scalar_string(
    slot: &mut Option<String>,
    incoming: Option<&str>,
    field: &str,
    canonical_id: &str,
    notices: &mut Vec<String>,
) {
    if let Some(value) = incoming.map(str::trim).filter(|s| !s.is_empty()) {
        match slot {
            None => *slot = Some(value.to_owned()),
            Some(existing) if existing == value => {}
            Some(_) => notices.push(format!(
                "Agent '{canonical_id}' sets '{field}' under `extends`, which is additive-only; base value kept."
            )),
        }
    }
}

fn sanitize_cache_segment(raw: &str) -> String {
    raw.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn builtin_catalog_loads_and_validates_cleanly() {
        let catalog = BrukCatalog::builtin_only();
        assert_eq!(catalog.effective_agents().len(), 4);
        assert_eq!(catalog.effective_skills().len(), 2);
        assert_eq!(catalog.effective_composites().len(), 2);
        assert_eq!(catalog.effective_workflows().len(), 2);

        let diagnostics = catalog.validate();
        assert!(
            diagnostics.is_empty(),
            "built-in catalog should validate cleanly, got: {diagnostics:?}"
        );

        let planner = catalog.resolve_agent("planner").unwrap();
        assert_eq!(planner.canonical_id, "builtin/planner");
        assert_eq!(planner.sandbox, Some(ProjectSandboxMode::ReadOnly));
        assert!(planner.include_repo_instructions);
        assert_eq!(planner.skills, vec!["minimal-diffs".to_owned()]);
    }

    #[test]
    fn three_layer_precedence_project_beats_module_beats_builtin() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("my-repo");
        let module_dir = temp.path().join("acme-module");

        // Module defines a custom reviewer and a new skill.
        fs::create_dir_all(module_dir.join("agents")).unwrap();
        fs::create_dir_all(module_dir.join("skills")).unwrap();
        fs::write(
            module_dir.join("skills/acme-security.md"),
            "---\nid: acme-security\nname: Acme Security\n---\nCheck OWASP top 10.\n",
        )
        .unwrap();
        fs::write(
            module_dir.join("agents/reviewer.md"),
            "---\nid: reviewer\nname: Acme Reviewer\nskills:\n  - acme-security\n---\nAcme module review rules.\n",
        )
        .unwrap();

        // Project declares the module and adds its own workflow + additive agent extension.
        let project_bruk = project.join(BRUK_CONFIG_DIR);
        fs::create_dir_all(project_bruk.join("agents")).unwrap();
        fs::create_dir_all(project_bruk.join("workflows")).unwrap();
        fs::write(
            project_bruk.join(BRUK_MANIFEST_FILE),
            format!(
                "schema_version: 1\nmodules:\n  - name: acme\n    source:\n      path: {}\n",
                module_dir.display()
            ),
        )
        .unwrap();
        fs::write(
            project_bruk.join("agents/reviewer.md"),
            "---\nid: reviewer\nname: Project Reviewer\nextends: acme/reviewer\nprovider: claude\nskills:\n  - minimal-diffs\n---\nAlso verify SQLite migrations.\n",
        )
        .unwrap();
        fs::write(
            project_bruk.join("workflows/custom-check.yaml"),
            "id: custom-check\nname: Custom Check\nnodes:\n  - kind: agent_task\n    id: rev\n    name: Review\n    agent: reviewer\n    prompt: Check diff\n",
        )
        .unwrap();

        let catalog = BrukCatalog::load_for_project(&project);
        assert!(catalog.validate().is_empty(), "{:?}", catalog.validate());

        // Effective unqualified `reviewer` is the project one extending `acme/reviewer`.
        let resolved = catalog.resolve_agent("reviewer").unwrap();
        assert_eq!(resolved.canonical_id, "project/reviewer");
        assert_eq!(
            resolved.lineage,
            vec!["acme/reviewer".to_owned(), "project/reviewer".to_owned()]
        );
        assert_eq!(resolved.provider, Some(ProviderKind::Claude));
        assert_eq!(
            resolved.skills,
            vec!["acme-security".to_owned(), "minimal-diffs".to_owned()]
        );
        assert!(resolved.instructions.contains("Acme module review rules."));
        assert!(
            resolved
                .instructions
                .contains("Also verify SQLite migrations.")
        );

        // Qualified refs still reach lower layers directly.
        let builtin_reviewer = catalog.resolve_agent("builtin/reviewer").unwrap();
        assert_eq!(builtin_reviewer.canonical_id, "builtin/reviewer");
        assert_eq!(
            builtin_reviewer.lineage,
            vec!["builtin/reviewer".to_owned()]
        );

        // Project workflow is discovered alongside builtins.
        let wf = catalog.resolve_workflow("custom-check").unwrap();
        assert!(matches!(wf.origin, DefinitionOrigin::Project { .. }));
        assert!(catalog.resolve_workflow("fix-issue").is_some());
    }

    #[test]
    fn extends_is_strictly_additive_and_flags_scalar_conflicts_and_cycles() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("repo");
        let agents_dir = project.join(BRUK_CONFIG_DIR).join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        // Try to change builtin/planner's already-set `sandbox: read_only` under `extends`.
        fs::write(
            agents_dir.join("planner.md"),
            "---\nid: planner\nextends: planner\nsandbox: full_access\nmodel: claude-opus-4-6\n---\nExtra project planning guidelines.\n",
        )
        .unwrap();

        // Also create a cyclic pair of agents.
        fs::write(
            agents_dir.join("loop-a.md"),
            "---\nid: loop-a\nextends: loop-b\n---\nA\n",
        )
        .unwrap();
        fs::write(
            agents_dir.join("loop-b.md"),
            "---\nid: loop-b\nextends: loop-a\n---\nB\n",
        )
        .unwrap();

        let catalog = BrukCatalog::load_for_project(&project);

        // Unqualified self-ID `extends: planner` resolves against lower `builtin/planner`.
        let planner = catalog.resolve_agent("planner").unwrap();
        assert_eq!(
            planner.lineage,
            vec!["builtin/planner".to_owned(), "project/planner".to_owned()]
        );
        // Base sandbox preserved, unset model filled in, notice recorded.
        assert_eq!(planner.sandbox, Some(ProjectSandboxMode::ReadOnly));
        assert_eq!(planner.model.as_deref(), Some("claude-opus-4-6"));
        assert_eq!(planner.notices.len(), 1);
        assert!(
            planner
                .instructions
                .contains("Extra project planning guidelines.")
        );

        // Cycle is detected.
        assert!(matches!(
            catalog.resolve_agent("loop-a"),
            Err(CatalogError::AgentExtendsCycle(_))
        ));
    }

    #[test]
    fn supports_folder_based_skill_md_and_workflow_save_round_trip() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("repo");
        let skill_dir = project
            .join(BRUK_CONFIG_DIR)
            .join("skills")
            .join("rust-audit");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Rust Audit\ndescription: Clippy and unsafe checks\n---\nRun cargo clippy with -D warnings.\n",
        )
        .unwrap();

        let wf = WorkflowDef {
            id: "audit-repo".to_owned(),
            name: "Audit Repo".to_owned(),
            description: "Run static analysis and review".to_owned(),
            draft: false,
            triggers: WorkflowTriggersDef {
                work_queue: false,
                ad_hoc: true,
                issue_labels: Vec::new(),
            },
            inputs: BTreeMap::new(),
            mcp_sources: Default::default(),
            nodes: vec![
                WorkflowNodeDef::ShellCommand(ShellCommandStepDef {
                    id: "clippy".to_owned(),
                    name: "Clippy".to_owned(),
                    command: "cargo clippy --workspace --all-targets -- -D warnings".to_owned(),
                }),
                WorkflowNodeDef::AgentTask(AgentTaskStepDef {
                    id: "review".to_owned(),
                    name: "Review".to_owned(),
                    agent: Some("reviewer".to_owned()),
                    provider: Some(ProviderKind::Claude),
                    model: None,
                    skills: vec!["rust-audit".to_owned()],
                    prompt: "Summarize findings.".to_owned(),
                    inputs: BTreeMap::new(),
                    outputs: Vec::new(),
                    mcp: Vec::new(),
                }),
            ],
            origin: DefinitionOrigin::BuiltIn,
        };

        let saved_path = BrukCatalog::save_project_workflow(&project, &wf).unwrap();
        assert_eq!(
            saved_path,
            project
                .join(BRUK_CONFIG_DIR)
                .join("workflows")
                .join("audit-repo.yaml")
        );

        let catalog = BrukCatalog::load_for_project(&project);
        assert!(catalog.validate().is_empty(), "{:?}", catalog.validate());
        let skill = catalog.resolve_skill("rust-audit").unwrap();
        assert_eq!(skill.name, "Rust Audit");
        let loaded = catalog.resolve_workflow("audit-repo").unwrap();
        assert_eq!(loaded.name, "Audit Repo");
        assert_eq!(loaded.nodes.len(), 2);
        assert!(matches!(loaded.origin, DefinitionOrigin::Project { .. }));

        assert!(BrukCatalog::delete_project_workflow(&project, "audit-repo").unwrap());
        let reloaded = BrukCatalog::load_for_project(&project);
        assert!(reloaded.resolve_workflow("audit-repo").is_none());
    }

    #[test]
    fn loads_git_module_from_local_git_repository() {
        let temp = TempDir::new().unwrap();
        let remote_repo = temp.path().join("company-module-repo");
        fs::create_dir_all(remote_repo.join("agents")).unwrap();
        fs::write(
            remote_repo.join("agents/security.md"),
            "---\nid: security\nname: Security Auditor\n---\nAudit auth boundaries.\n",
        )
        .unwrap();

        run_git(&remote_repo, &["init", "-b", "main"]).unwrap();
        run_git(&remote_repo, &["config", "user.email", "test@example.com"]).unwrap();
        run_git(&remote_repo, &["config", "user.name", "Test"]).unwrap();
        run_git(&remote_repo, &["add", "."]).unwrap();
        run_git(&remote_repo, &["commit", "-m", "init module"]).unwrap();

        let project = temp.path().join("app-repo");
        let bruk_dir = project.join(BRUK_CONFIG_DIR);
        fs::create_dir_all(&bruk_dir).unwrap();
        fs::write(
            bruk_dir.join(BRUK_MANIFEST_FILE),
            format!(
                "schema_version: 1\nmodules:\n  - name: acme-sec\n    source:\n      git: {}\n      ref: main\n",
                remote_repo.display()
            ),
        )
        .unwrap();

        let cache_dir = temp.path().join("module-cache");
        let catalog = BrukCatalog::load_for_project_with_options(
            &project,
            &CatalogLoadOptions {
                module_fetch_policy: ModuleFetchPolicy::FetchIfMissing,
                module_cache_dir: Some(cache_dir),
            },
        );
        assert!(catalog.validate().is_empty(), "{:?}", catalog.validate());
        let sec = catalog.resolve_agent("acme-sec/security").unwrap();
        assert_eq!(sec.name, "Security Auditor");
        assert!(sec.instructions.contains("Audit auth boundaries."));
    }

    #[test]
    fn mcp_settings_survive_yaml_and_the_workflow_editor() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        fs::create_dir_all(repo.join(".agents/bruk/workflows")).unwrap();
        fs::write(
            repo.join(".agents/bruk/workflows/strict.yaml"),
            "id: strict\nname: Strict\nmcp_sources: project\nnodes:\n  - kind: agent_task\n    id: browse\n    name: Browse\n    prompt: Go\n    mcp:\n      - playwright\n",
        )
        .unwrap();
        fs::write(
            repo.join(".mcp.json"),
            r#"{"mcpServers":{"playwright":{"command":"npx"}}}"#,
        )
        .unwrap();
        let catalog = BrukCatalog::load_for_project(repo);
        assert_eq!(
            catalog.project_mcp.file(),
            Some(crate::mcp::ProjectMcpFile::RootMcpJson)
        );
        let wf = catalog.resolve_workflow("strict").cloned().unwrap();
        assert_eq!(wf.mcp_sources, McpSourcesPolicy::Project);
        let WorkflowNodeDef::AgentTask(step) = &wf.nodes[0] else {
            panic!("expected an agent step, got {:?}", wf.nodes[0]);
        };
        assert_eq!(step.mcp, ["playwright"]);

        let doc = crate::workflow_editor::WorkflowEditorDocument::from_workflow_def(
            "repo", &wf, &catalog,
        );
        let saved = doc.to_workflow_def();
        assert_eq!(saved.mcp_sources, McpSourcesPolicy::Project);
        let WorkflowNodeDef::AgentTask(step) = &saved.nodes[0] else {
            panic!("expected an agent step");
        };
        assert_eq!(step.mcp, ["playwright"]);
        assert!(saved.to_yaml().unwrap().contains("mcp_sources: project"));
    }
}
