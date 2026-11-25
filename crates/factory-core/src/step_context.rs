//! Just-in-time per-step context bundler and user-context-isolated CLI spawn planners.
//!
//! Ensures every agent step receives **only** the context required for the action at hand:
//! - Only the step's resolved agent (with additive `extends` applied)
//! - Only the skills explicitly referenced by that agent or step
//! - Only the subagents explicitly declared by that agent
//! - Repo-checked-in instructions (`CLAUDE.md`, `AGENTS.md`, repo `.claude/`) stay allowed
//! - User-level CLI context (`~/.claude/*`, personal MCP/hooks/skills, `~/.codex/*`) is blocked
//! - All transient step files are materialized outside the git checkout so `git status` stays clean

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    antigravity_cli::{ANTIGRAVITY_DISABLE_AUTO_UPDATE_ENV, ANTIGRAVITY_DISABLE_AUTO_UPDATE_VALUE},
    claude_settings::{
        CLAUDE_DISABLE_AUTOUPDATER_ENV, CLAUDE_DISABLE_AUTOUPDATER_VALUE, ClaudeSettingsError,
        ClaudeSpawnOptions, ClaudeSpawnPlan, ClaudeSpawnSettings, build_claude_spawn_plan,
    },
    codex_settings::{
        CODEX_DISABLE_UPDATE_CHECK_ARG, CodexSettingValue, CodexSettingsError, CodexSpawnOptions,
        CodexSpawnPlan, CodexSpawnSettings, build_codex_spawn_plan,
    },
    definitions::{AgentTaskStepDef, AgentToolsPolicy, BrukCatalog, CatalogError, ResolvedAgent},
    mcp::{
        ANTIGRAVITY_LOG_FILE, AntigravityMcpDelivery, CODEX_DISABLE_APPS_ARG, CODEX_MCP_RUST_LOG,
        CodexMcpDelivery, McpSources, McpSourcesPolicy, ResolvedMcpSet, antigravity_mcp_delivery,
        claude_mcp_config, claude_mcp_config_path, codex_mcp_delivery,
        codex_mcp_delivery_with_secrets, preflight_mcp_servers, preflight_mcp_servers_with_secrets,
        resolve_mcp_servers,
    },
    model::ProjectId,
    preferences::ProviderKind,
    project_settings::ProjectSandboxMode,
    secrets::{ResolvedSecrets, SecretProblem, is_ambient_credential_var_name},
};

/// Errors returned while compiling or materializing a step context bundle.
#[derive(Debug, Error)]
pub enum StepContextError {
    #[error("catalog resolution failed: {0}")]
    Catalog(#[from] CatalogError),
    #[error("referenced skill '{0}' was not found in catalog")]
    SkillNotFound(String),
    #[error("referenced subagent '{0}' was not found in catalog")]
    SubagentNotFound(String),
    #[error("failed to create runtime directory '{path}': {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write runtime file '{path}': {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("claude spawn plan error: {0}")]
    Claude(#[from] ClaudeSettingsError),
    #[error("codex spawn plan error: {0}")]
    Codex(#[from] CodexSettingsError),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Template variable values available when rendering a workflow step prompt or command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateContext {
    pub inputs: BTreeMap<String, String>,
    /// Keyed as `<step_id>.<field>` (e.g. `read_plan.output`, `run_tests.stderr`).
    pub step_outputs: BTreeMap<String, String>,
    /// Current loop attempt (1-indexed) if inside a loop.
    pub loop_attempt: Option<u32>,
    /// Previous loop attempt outputs keyed as `<step_id>.<field>`.
    pub loop_previous: BTreeMap<String, String>,
}

impl TemplateContext {
    /// Substitutes `{{ key }}` expressions (supporting `inputs.*`, `steps.*`, `loop.attempt`,
    /// and `loop.previous.*`). Unknown keys expand to an empty string so prompts stay clean.
    pub fn render(&self, template: &str) -> String {
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after_open = &rest[start + 2..];
            let Some(end) = after_open.find("}}") else {
                out.push_str(&rest[start..]);
                return out;
            };
            let expr = after_open[..end].trim();
            if let Some(value) = self.lookup(expr) {
                out.push_str(&value);
            }
            rest = &after_open[end + 2..];
        }
        out.push_str(rest);
        out
    }

    fn lookup(&self, expr: &str) -> Option<String> {
        if let Some(key) = expr.strip_prefix("inputs.") {
            return self.inputs.get(key.trim()).cloned();
        }
        if let Some(key) = expr.strip_prefix("steps.") {
            return self.step_outputs.get(key.trim()).cloned();
        }
        if expr == "loop.attempt" {
            return self.loop_attempt.map(|n| n.to_string());
        }
        if let Some(key) = expr.strip_prefix("loop.previous.") {
            return self.loop_previous.get(key.trim()).cloned();
        }
        self.inputs.get(expr).cloned()
    }
}

/// One resolved subagent exposed strictly to the step that declared it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentContextSpec {
    pub id: String,
    pub canonical_id: String,
    pub description: String,
    pub prompt: String,
    pub model: Option<String>,
    pub tools: AgentToolsPolicy,
}

/// Minimal, self-contained context compiled for a single [`AgentTaskStepDef`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepContextBundle {
    pub step_id: String,
    pub step_name: String,
    pub agent: Option<ResolvedAgent>,
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub sandbox: ProjectSandboxMode,
    pub tools: AgentToolsPolicy,
    pub include_repo_instructions: bool,
    pub included_skill_ids: Vec<String>,
    pub subagents: Vec<SubagentContextSpec>,
    /// Combined system/developer instructions (agent body + ONLY declared skills).
    pub system_instructions: String,
    /// Rendered task prompt for this single step.
    pub rendered_prompt: String,
    /// The MCP servers this step gets, and every problem keeping one from it.
    pub mcp: ResolvedMcpSet,
    /// How [`Self::mcp`] reaches Codex (when the provider is Codex).
    pub codex_mcp: CodexMcpDelivery,
    /// What Bruk does about MCP for Antigravity (when the provider is Antigravity).
    pub antigravity_mcp: AntigravityMcpDelivery,
    /// Dedicated blocking secret problems encountered during MCP preflight.
    pub secret_problems: Vec<SecretProblem>,
}

/// Secrets delivery policy applied on top of a provider spawn plan (§7 Delivery Matrix).
#[derive(Debug, Clone, Default)]
pub struct StepSecretsDelivery {
    /// Plain + `AgentAndSteps` + (for Claude/Codex) referenced `McpOnly` env values.
    pub env: BTreeMap<String, String>,
    /// Resolved `Internal { purpose: ProviderSpawnEnv }` values for Claude / Codex spawn settings.
    pub provider_spawn_env: BTreeMap<String, String>,
    /// Variable names explicitly stripped via `env_remove` (excluded, unshared, unreferenced `McpOnly`,
    /// `Internal` names, `GH_TOKEN`, `OP_SERVICE_ACCOUNT_TOKEN`, `OP_CONNECT_*`).
    pub env_remove: BTreeSet<String>,
    /// Names of `McpOnly` secrets injected for MCP servers (denied in Claude sandbox / Codex shell).
    pub mcp_only_names: BTreeSet<String>,
    /// Names of `AgentAndSteps` secrets allowed in agent shell (drives Codex `ignore_default_excludes`).
    pub agent_and_steps_names: BTreeSet<String>,
}

impl StepSecretsDelivery {
    pub fn from_resolved(
        resolved: &ResolvedSecrets,
        provider_spawn_env: BTreeMap<String, String>,
        include_mcp_only_in_env: bool,
    ) -> Self {
        let env = if include_mcp_only_in_env {
            resolved.full_agent_env()
        } else {
            resolved.agent_and_steps_env()
        };
        let mcp_only_names = resolved.mcp_only_secret_names();
        let mut env_remove = resolved.strip_from_child_env.clone();
        if !include_mcp_only_in_env {
            env_remove.extend(mcp_only_names.iter().cloned());
        }
        Self {
            env,
            provider_spawn_env,
            env_remove,
            mcp_only_names,
            agent_and_steps_names: resolved.agent_and_steps_secret_names(),
        }
    }
}

/// What a step chooses MCP servers from.
#[derive(Debug, Clone, Copy)]
pub struct StepMcpScope<'a> {
    pub sources: &'a McpSources,
    /// The strictest `mcp_sources` of the workflow and composites around the step.
    pub policy: McpSourcesPolicy,
    /// Plain project environment variables (including `PATH`).
    pub env: &'a BTreeMap<String, String>,
    /// Optional resolved secrets from [`crate::secrets::SecretResolver`].
    pub resolved_secrets: Option<&'a ResolvedSecrets>,
    /// Project id for attaching to [`SecretProblem`]s.
    pub project_id: Option<ProjectId>,
}

impl StepMcpScope<'static> {
    /// No MCP servers at all (tests and callers without a project).
    pub fn none() -> Self {
        static SOURCES: std::sync::OnceLock<McpSources> = std::sync::OnceLock::new();
        static ENV: BTreeMap<String, String> = BTreeMap::new();
        Self {
            sources: SOURCES.get_or_init(McpSources::default),
            policy: McpSourcesPolicy::All,
            env: &ENV,
            resolved_secrets: None,
            project_id: None,
        }
    }
}

/// Out-of-repo materialized runtime files for one step execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedStepContext {
    pub bundle: StepContextBundle,
    pub runtime_dir: PathBuf,
    pub instructions_file: Option<PathBuf>,
    pub claude_agents_file: Option<PathBuf>,
    pub codex_home_dir: Option<PathBuf>,
    /// Ephemeral `0600` copies of Codex auth files created only when symlinking failed.
    pub ephemeral_codex_auth_files: Vec<PathBuf>,
    /// The `--mcp-config` file for Claude, when the step has servers.
    pub claude_mcp_file: Option<PathBuf>,
}

impl MaterializedStepContext {
    /// Unconditionally removes any ephemeral fallback copies of Codex auth credentials.
    pub fn cleanup_ephemeral_codex_auth(&self) {
        for path in &self.ephemeral_codex_auth_files {
            let _ = fs::remove_file(path);
        }
    }
}

/// RAII guard ensuring ephemeral Codex auth copies are deleted when a workflow step ends.
pub struct EphemeralCodexAuthGuard {
    paths: Vec<PathBuf>,
}

impl EphemeralCodexAuthGuard {
    pub fn for_context(ctx: &MaterializedStepContext) -> Self {
        Self {
            paths: ctx.ephemeral_codex_auth_files.clone(),
        }
    }
}

impl Drop for EphemeralCodexAuthGuard {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

/// Compiles the minimal [`StepContextBundle`] for one [`AgentTaskStepDef`].
///
/// Only the step's resolved agent, declared skills (agent + step), and declared subagents
/// are included—never unrelated agents or skills from the catalog.
pub fn compile_step_context(
    catalog: &BrukCatalog,
    step: &AgentTaskStepDef,
    template_ctx: &TemplateContext,
    default_provider: ProviderKind,
    default_sandbox: ProjectSandboxMode,
    project_model_override: Option<&str>,
    mcp_scope: StepMcpScope<'_>,
) -> Result<StepContextBundle, StepContextError> {
    let resolved_agent = match step
        .agent
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(agent_ref) => Some(catalog.resolve_agent(agent_ref)?),
        None => None,
    };

    let provider = step
        .provider
        .or_else(|| resolved_agent.as_ref().and_then(|a| a.provider))
        .unwrap_or(default_provider);

    let model = step
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            resolved_agent
                .as_ref()
                .and_then(|a| a.model.clone())
                .filter(|s| !s.trim().is_empty())
        })
        .or_else(|| {
            project_model_override
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        });

    let reasoning_effort = resolved_agent
        .as_ref()
        .and_then(|a| a.reasoning_effort.clone());

    let sandbox = resolved_agent
        .as_ref()
        .and_then(|a| a.sandbox)
        .unwrap_or(default_sandbox);

    let tools = resolved_agent
        .as_ref()
        .map(|a| a.tools.clone())
        .unwrap_or_default();

    let include_repo_instructions = resolved_agent
        .as_ref()
        .map(|a| a.include_repo_instructions)
        .unwrap_or(true);

    // Collect ONLY skills explicitly referenced by this agent + this step (deduplicated, ordered).
    let mut skill_refs = Vec::new();
    if let Some(agent) = resolved_agent.as_ref() {
        for s in &agent.skills {
            if !skill_refs.contains(s) {
                skill_refs.push(s.clone());
            }
        }
    }
    for s in &step.skills {
        let trimmed = s.trim();
        if !trimmed.is_empty() && !skill_refs.iter().any(|existing| existing == trimmed) {
            skill_refs.push(trimmed.to_owned());
        }
    }

    let mut sections = Vec::new();
    if let Some(agent) = resolved_agent.as_ref()
        && !agent.instructions.trim().is_empty()
    {
        sections.push(agent.instructions.trim().to_owned());
    }

    let mut included_skill_ids = Vec::new();
    for skill_ref in &skill_refs {
        let skill = catalog
            .resolve_skill(skill_ref)
            .ok_or_else(|| StepContextError::SkillNotFound(skill_ref.clone()))?;
        included_skill_ids.push(skill.canonical_id());
        if !skill.instructions.trim().is_empty() {
            sections.push(format!(
                "## Skill: {}\n\n{}",
                skill.name,
                skill.instructions.trim()
            ));
        }
    }

    // Collect ONLY subagents explicitly declared by this agent.
    let mut subagents = Vec::new();
    if let Some(agent) = resolved_agent.as_ref() {
        for sub_ref in &agent.subagents {
            let resolved_sub = catalog
                .resolve_agent(sub_ref)
                .map_err(|_| StepContextError::SubagentNotFound(sub_ref.clone()))?;
            let mut sub_sections = Vec::new();
            if !resolved_sub.instructions.trim().is_empty() {
                sub_sections.push(resolved_sub.instructions.trim().to_owned());
            }
            for sub_skill_ref in &resolved_sub.skills {
                if let Some(sk) = catalog.resolve_skill(sub_skill_ref)
                    && !sk.instructions.trim().is_empty()
                {
                    sub_sections.push(format!(
                        "## Skill: {}\n\n{}",
                        sk.name,
                        sk.instructions.trim()
                    ));
                }
            }
            subagents.push(SubagentContextSpec {
                id: resolved_sub.id.clone(),
                canonical_id: resolved_sub.canonical_id,
                description: if resolved_sub.description.is_empty() {
                    resolved_sub.name.clone()
                } else {
                    resolved_sub.description
                },
                prompt: sub_sections.join("\n\n"),
                model: resolved_sub.model,
                tools: resolved_sub.tools,
            });
        }
    }

    // Render step prompt with bound step inputs merged on top of workflow inputs.
    let mut step_ctx = template_ctx.clone();
    for (k, v) in &step.inputs {
        let rendered_val = template_ctx.render(v);
        step_ctx.inputs.insert(k.clone(), rendered_val);
    }
    let rendered_prompt = step_ctx.render(&step.prompt);

    // MCP servers: the agent can only tighten the policy; the agent's and step's `mcp:` lists
    // narrow it to the servers they name.
    let policy = mcp_scope.policy.stricter(
        resolved_agent
            .as_ref()
            .map(|a| a.mcp_sources)
            .unwrap_or_default(),
    );
    let mut allowlist: Vec<String> = resolved_agent
        .as_ref()
        .map(|a| a.mcp.clone())
        .unwrap_or_default();
    for name in step.mcp.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        if !allowlist.iter().any(|existing| existing == name) {
            allowlist.push(name.to_owned());
        }
    }
    let mut mcp = resolve_mcp_servers(mcp_scope.sources, policy, &allowlist);
    let lookup = |key: &str| {
        mcp_scope
            .env
            .get(key)
            .cloned()
            .or_else(|| std::env::var(key).ok())
    };
    let mut codex_mcp = CodexMcpDelivery::default();
    let mut antigravity_mcp = AntigravityMcpDelivery::default();
    let mut secret_problems = Vec::new();
    let plain_path = mcp_scope.env.get("PATH").map(String::as_str);

    match provider {
        ProviderKind::Claude => {
            if let Some(resolved) = mcp_scope.resolved_secrets {
                secret_problems = preflight_mcp_servers_with_secrets(
                    &mut mcp,
                    resolved,
                    plain_path,
                    mcp_scope.project_id,
                );
            } else {
                preflight_mcp_servers(&mut mcp, &lookup);
            }
        }
        ProviderKind::Codex => {
            if let Some(resolved) = mcp_scope.resolved_secrets {
                secret_problems = preflight_mcp_servers_with_secrets(
                    &mut mcp,
                    resolved,
                    plain_path,
                    mcp_scope.project_id,
                );
                let agent_env = resolved.agent_and_steps_env();
                codex_mcp = codex_mcp_delivery_with_secrets(&mut mcp, resolved, &agent_env);
            } else {
                preflight_mcp_servers(&mut mcp, &lookup);
                codex_mcp = codex_mcp_delivery(&mut mcp, &lookup);
            }
        }
        ProviderKind::Antigravity => {
            antigravity_mcp = antigravity_mcp_delivery(&mut mcp, mcp_scope.sources, &allowlist);
            if let Some(resolved) = mcp_scope.resolved_secrets {
                secret_problems = preflight_mcp_servers_with_secrets(
                    &mut mcp,
                    resolved,
                    plain_path,
                    mcp_scope.project_id,
                );
            } else {
                preflight_mcp_servers(&mut mcp, &lookup);
            }
        }
        ProviderKind::OpenRouter => {}
    }

    Ok(StepContextBundle {
        step_id: step.id.clone(),
        step_name: step.name.clone(),
        agent: resolved_agent,
        provider,
        model,
        reasoning_effort,
        sandbox,
        tools,
        include_repo_instructions,
        included_skill_ids,
        subagents,
        system_instructions: sections.join("\n\n"),
        rendered_prompt,
        mcp,
        codex_mcp,
        antigravity_mcp,
        secret_problems,
    })
}

/// Folder (inside a step's or chat's runtime directory) of the isolated `CODEX_HOME` that Codex
/// runs with.
pub const ISOLATED_CODEX_HOME_DIR: &str = "codex-home";

/// Materializes a [`StepContextBundle`] into an out-of-repo `runtime_dir`.
///
/// For Codex runs, creates an isolated `codex-home/` directory copying **only** auth credential
/// files (`auth.json`) from `source_codex_home` (if present), deliberately excluding user-level
/// `AGENTS.md`, `skills/`, `prompts/`, and `config.toml`.
pub fn materialize_step_context(
    bundle: StepContextBundle,
    runtime_dir: impl AsRef<Path>,
    source_codex_home: Option<&Path>,
) -> Result<MaterializedStepContext, StepContextError> {
    let runtime_dir = runtime_dir.as_ref().to_path_buf();
    fs::create_dir_all(&runtime_dir).map_err(|source| StepContextError::CreateDir {
        path: runtime_dir.clone(),
        source,
    })?;

    let instructions_file = if !bundle.system_instructions.trim().is_empty() {
        let path = runtime_dir.join("step-instructions.md");
        fs::write(&path, &bundle.system_instructions).map_err(|source| {
            StepContextError::WriteFile {
                path: path.clone(),
                source,
            }
        })?;
        Some(path)
    } else {
        None
    };

    let claude_agents_file = if !bundle.subagents.is_empty() {
        let mut agents_obj = serde_json::Map::new();
        for sub in &bundle.subagents {
            let mut entry = serde_json::Map::new();
            entry.insert(
                "description".to_owned(),
                serde_json::Value::String(sub.description.clone()),
            );
            entry.insert(
                "prompt".to_owned(),
                serde_json::Value::String(sub.prompt.clone()),
            );
            if !sub.tools.allow.is_empty() {
                entry.insert(
                    "tools".to_owned(),
                    serde_json::Value::Array(
                        sub.tools
                            .allow
                            .iter()
                            .cloned()
                            .map(serde_json::Value::String)
                            .collect(),
                    ),
                );
            }
            if let Some(model) = sub.model.as_deref() {
                entry.insert(
                    "model".to_owned(),
                    serde_json::Value::String(model.to_owned()),
                );
            }
            agents_obj.insert(sub.id.clone(), serde_json::Value::Object(entry));
        }
        let json = serde_json::to_string_pretty(&serde_json::Value::Object(agents_obj))?;
        let path = runtime_dir.join("step-agents.json");
        fs::write(&path, json).map_err(|source| StepContextError::WriteFile {
            path: path.clone(),
            source,
        })?;
        Some(path)
    } else {
        None
    };

    let mut ephemeral_codex_auth_files = Vec::new();
    let codex_home_dir = if bundle.provider == ProviderKind::Codex {
        let home = runtime_dir.join(ISOLATED_CODEX_HOME_DIR);
        fs::create_dir_all(&home).map_err(|source| StepContextError::CreateDir {
            path: home.clone(),
            source,
        })?;
        if let Some(src_home) = source_codex_home {
            // Link ONLY auth credential files via symlink (never leave permanent plaintext copies;
            // fall back to a 0600 ephemeral copy tracked for unconditional drop cleanup).
            for auth_file in ["auth.json", "credentials.json"] {
                let src = src_home.join(auth_file);
                if src.is_file() {
                    let dst = home.join(auth_file);
                    let _ = fs::remove_file(&dst);
                    if !try_symlink_file(&src, &dst) && fs::copy(&src, &dst).is_ok() {
                        set_owner_read_write_only(&dst);
                        ephemeral_codex_auth_files.push(dst);
                    }
                }
            }
        }
        Some(home)
    } else {
        None
    };

    // Claude gets the step's servers as an out-of-repo `--mcp-config` file. `${VAR}`s stay
    // unexpanded (Claude expands them from its environment), so no secret is written.
    let claude_mcp_file =
        if bundle.provider == ProviderKind::Claude && !bundle.mcp.servers.is_empty() {
            let path = claude_mcp_config_path(&runtime_dir);
            let json = serde_json::to_string_pretty(&claude_mcp_config(&bundle.mcp))?;
            fs::write(&path, json).map_err(|source| StepContextError::WriteFile {
                path: path.clone(),
                source,
            })?;
            Some(path)
        } else {
            None
        };

    Ok(MaterializedStepContext {
        bundle,
        runtime_dir,
        instructions_file,
        claude_agents_file,
        codex_home_dir,
        ephemeral_codex_auth_files,
        claude_mcp_file,
    })
}

/// Default out-of-repo runtime directory for a workflow run step: `<Bruk folder>/runs/<run-id>/`,
/// or the system temp directory when no Bruk folder has been set up.
pub fn default_step_runtime_dir(run_id: &str, step_index: usize) -> PathBuf {
    let base = crate::bruk_home::resolve_system_home()
        .map(|home| home.run_dir(run_id))
        .unwrap_or_else(|| std::env::temp_dir().join("bruk-runs").join(run_id));
    base.join(format!("step-{step_index:03}"))
}

/// Builds a user-context-isolated [`ClaudeSpawnPlan`] for one materialized workflow step.
///
/// Enforces:
/// - `--setting-sources project` (strips user `~/.claude/*` context while preserving repo files & auth)
/// - `--strict-mcp-config` (blocks personal `~/.claude.json` MCP servers)
/// - `--disable-slash-commands` (blocks ambient skill discovery; only Bruk's step skills enter context)
/// - `--append-system-prompt-file` with ONLY this step's resolved agent + skills
/// - `--agents` with ONLY this step's declared subagents (omitted when none)
/// - `DISABLE_AUTOUPDATER=1`
pub fn build_isolated_claude_spawn_plan(
    binary_path: impl AsRef<Path>,
    default_model: Option<&str>,
    base_settings: &ClaudeSpawnSettings,
    working_dir: impl AsRef<Path>,
    materialized: &MaterializedStepContext,
    session_name: Option<&str>,
    project_env: &BTreeMap<String, String>,
) -> Result<ClaudeSpawnPlan, StepContextError> {
    let options = ClaudeSpawnOptions {
        working_dir: Some(working_dir.as_ref().to_path_buf()),
        model_override: materialized.bundle.model.clone(),
        print_mode: true,
        stream_json: false,
        session_name: session_name.map(str::to_owned),
        prompt: None,
    };

    let mut plan = build_claude_spawn_plan(binary_path, default_model, base_settings, &options)?;

    // Stream structured JSON events on stdout while keeping positional prompt support.
    push_flag_if_missing(&mut plan.args, "--output-format", "stream-json");
    push_bool_flag_if_missing(&mut plan.args, "--verbose");

    // Strip user-level context while allowing repo-checked-in project config (`CLAUDE.md`, `.claude/`).
    set_or_push_flag(&mut plan.args, "--setting-sources", "project");
    if materialized.bundle.mcp.policy == McpSourcesPolicy::Project {
        // The user's own `--mcp-config` from Claude spawn settings is an extra too.
        remove_variadic_flag(&mut plan.args, "--mcp-config");
    }
    push_bool_flag_if_missing(&mut plan.args, "--strict-mcp-config");
    if let Some(mcp_file) = materialized.claude_mcp_file.as_ref() {
        // `--mcp-config` takes several values, so it goes right before another flag where it
        // can't swallow the prompt.
        let at = plan
            .args
            .iter()
            .position(|a| a == "--strict-mcp-config")
            .unwrap_or(plan.args.len());
        plan.args.insert(at, "--mcp-config".to_owned());
        plan.args
            .insert(at + 1, mcp_file.to_string_lossy().into_owned());
    }
    push_bool_flag_if_missing(&mut plan.args, "--disable-slash-commands");

    // Permission mode derived from resolved agent/project sandbox if not overridden.
    if !has_flag(&plan.args, "--permission-mode") {
        let mode = match materialized.bundle.sandbox {
            ProjectSandboxMode::ReadOnly => "plan",
            ProjectSandboxMode::WorkspaceWrite => "acceptEdits",
            ProjectSandboxMode::FullAccess => "bypassPermissions",
        };
        plan.args.push("--permission-mode".to_owned());
        plan.args.push(mode.to_owned());
    }

    // Step-scoped tool policy from resolved agent.
    if !materialized.bundle.tools.allow.is_empty() && !has_flag(&plan.args, "--allowedTools") {
        plan.args.push("--allowedTools".to_owned());
        plan.args.push(materialized.bundle.tools.allow.join(","));
    }
    if !materialized.bundle.tools.deny.is_empty() && !has_flag(&plan.args, "--disallowedTools") {
        plan.args.push("--disallowedTools".to_owned());
        plan.args.push(materialized.bundle.tools.deny.join(","));
    }

    // Inject ONLY this step's resolved agent + skills via out-of-repo file.
    if let Some(instructions_file) = materialized.instructions_file.as_ref() {
        plan.args.push("--append-system-prompt-file".to_owned());
        plan.args
            .push(instructions_file.to_string_lossy().into_owned());
    }

    // Inject ONLY this step's explicitly declared subagents.
    if let Some(agents_file) = materialized.claude_agents_file.as_ref() {
        plan.args.push("--agents".to_owned());
        plan.args.push(agents_file.to_string_lossy().into_owned());
    }

    // Positional rendered prompt goes last.
    if !materialized.bundle.rendered_prompt.trim().is_empty() {
        plan.args.push(materialized.bundle.rendered_prompt.clone());
    }

    // Merge project env vars without letting them override mandatory Bruk guards.
    for (k, v) in project_env {
        if k != CLAUDE_DISABLE_AUTOUPDATER_ENV {
            plan.env.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    plan.env.insert(
        CLAUDE_DISABLE_AUTOUPDATER_ENV.to_owned(),
        CLAUDE_DISABLE_AUTOUPDATER_VALUE.to_owned(),
    );

    Ok(plan)
}

/// Builds a user-context-isolated [`CodexSpawnPlan`] for one materialized workflow step.
///
/// Enforces:
/// - Ephemeral `CODEX_HOME` containing only auth credentials (blocks `~/.codex/AGENTS.md`,
///   `~/.codex/skills/`, and personal `~/.codex/config.toml`)
/// - `-c check_for_update_on_startup=false`
/// - `-c skills.bundled.enabled=false` and `-c skills.include_instructions=false`
/// - `-c developer_instructions=...` with ONLY this step's resolved agent + skills
///   (while keeping repo-checked-in `AGENTS.md` active unless `include_repo_instructions == false`)
pub fn build_isolated_codex_spawn_plan(
    binary_path: impl AsRef<Path>,
    default_model: Option<&str>,
    base_settings: &CodexSpawnSettings,
    working_dir: impl AsRef<Path>,
    materialized: &MaterializedStepContext,
    project_env: &BTreeMap<String, String>,
) -> Result<CodexSpawnPlan, StepContextError> {
    let options = CodexSpawnOptions {
        working_dir: Some(working_dir.as_ref().to_path_buf()),
        model_override: materialized.bundle.model.clone(),
        exec_mode: true,
        json_events: true,
        prompt: None,
    };

    let mut plan = build_codex_spawn_plan(binary_path, default_model, base_settings, &options)?;

    // Ensure mandatory update check guard is always present.
    if !plan
        .args
        .iter()
        .any(|a| a == CODEX_DISABLE_UPDATE_CHECK_ARG)
    {
        plan.args.push("-c".to_owned());
        plan.args.push(CODEX_DISABLE_UPDATE_CHECK_ARG.to_owned());
    }

    // Block ambient/bundled skills so only Bruk's step-selected skills enter context.
    push_codex_config(&mut plan, "skills.bundled.enabled", "false");
    push_codex_config(&mut plan, "skills.include_instructions", "false");

    // MCP: the ChatGPT account's connectors are the user's own and load even from an isolated
    // CODEX_HOME, so they're always off. Under a project-only policy the user's own
    // `mcp_servers` from Codex spawn settings go too. Bruk's servers come after the spawn
    // settings so a whole-table `mcp_servers=` override there can't replace them.
    if !plan.args.iter().any(|a| a == CODEX_DISABLE_APPS_ARG) {
        plan.args.push("-c".to_owned());
        plan.args.push(CODEX_DISABLE_APPS_ARG.to_owned());
    }
    if materialized.bundle.mcp.policy == McpSourcesPolicy::Project {
        remove_codex_config_prefix(&mut plan, "mcp_servers");
    }
    for override_arg in &materialized.bundle.codex_mcp.config_overrides {
        plan.args.push("-c".to_owned());
        plan.args.push(override_arg.clone());
    }

    // Apply resolved sandbox mode.
    let sandbox_val = match materialized.bundle.sandbox {
        ProjectSandboxMode::ReadOnly => "\"read-only\"",
        ProjectSandboxMode::WorkspaceWrite => "\"workspace-write\"",
        ProjectSandboxMode::FullAccess => "\"danger-full-access\"",
    };
    push_codex_config(&mut plan, "sandbox_mode", sandbox_val);

    if let Some(effort) = materialized
        .bundle
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        push_codex_config(&mut plan, "model_reasoning_effort", &toml_quote(effort));
    }

    if !materialized.bundle.include_repo_instructions {
        push_codex_config(&mut plan, "project_doc_max_bytes", "0");
    }

    // Inject ONLY this step's resolved agent + skills via `developer_instructions` so repo AGENTS.md stays active.
    if !materialized.bundle.system_instructions.trim().is_empty() {
        push_codex_config(
            &mut plan,
            "developer_instructions",
            &toml_quote(&materialized.bundle.system_instructions),
        );
    }

    // Positional rendered prompt goes last.
    if !materialized.bundle.rendered_prompt.trim().is_empty() {
        plan.args.push(materialized.bundle.rendered_prompt.clone());
    }

    // Merge project env vars and force isolated CODEX_HOME.
    for (k, v) in project_env {
        if k != "CODEX_HOME" {
            plan.env.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    if let Some(codex_home) = materialized.codex_home_dir.as_ref() {
        plan.env.insert(
            "CODEX_HOME".to_owned(),
            codex_home.to_string_lossy().into_owned(),
        );
    }
    // Variables Codex forwards to servers (`env_vars`), and the logging that makes it report
    // servers that fail to start on stderr.
    for (k, v) in &materialized.bundle.codex_mcp.env {
        plan.env.insert(k.clone(), v.clone());
    }
    plan.env
        .insert("RUST_LOG".to_owned(), CODEX_MCP_RUST_LOG.to_owned());

    Ok(plan)
}

/// Antigravity permission rules a Read-only step or chat runs with. Rules name permissions,
/// not tools: `write_file` covers every file edit, `command` every shell command, `mcp` every
/// MCP tool. Reading and searching stay allowed.
pub const ANTIGRAVITY_READ_ONLY_DENY_RULES: &[&str] = &["write_file(*)", "command(*)", "mcp(*)"];

/// Antigravity permission rules that keep every MCP tool from being used.
pub const ANTIGRAVITY_DENY_MCP_RULES: &[&str] = &["mcp(*)"];

/// Antigravity permission rules a Workspace write step or chat runs with (inside `--sandbox`).
pub const ANTIGRAVITY_WORKSPACE_WRITE_ALLOW_RULES: &[&str] = &["write_file(*)", "command(*)"];

/// A resolved `agy` invocation: binary, working directory, arguments and environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravitySpawnPlan {
    pub binary_path: PathBuf,
    pub working_dir: Option<PathBuf>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// Environment variable names explicitly removed from the spawned `agy` process.
    pub env_remove: BTreeSet<String>,
    /// The step's agent and skills, which Antigravity can't take as a system prompt in print
    /// mode; Bruk sends them ahead of the first message instead.
    pub instructions: Option<String>,
}

impl AntigravitySpawnPlan {
    /// Builds a [`std::process::Command`] configured with the binary, working dir, args, and env.
    pub fn to_command(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(&self.binary_path);
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

    /// The first message of a conversation with the step's instructions ahead of `prompt`.
    pub fn first_message(&self, prompt: &str) -> String {
        match self.instructions.as_deref() {
            Some(instructions) => {
                format!("<instructions>\n{instructions}\n</instructions>\n\n{prompt}")
            }
            None => prompt.to_owned(),
        }
    }
}

/// Builds a user-context-isolated [`AntigravitySpawnPlan`] for one materialized workflow step
/// or chat turn (`agy` in print mode, streaming JSON events).
///
/// Enforces:
/// - `AGY_CLI_DISABLE_AUTO_UPDATE=true`
/// - `--disable-slash-commands` so skill and command expansion stays off; only Bruk's step
///   skills enter the conversation
/// - the access the step allows, always in the terminal sandbox (`--sandbox`) short of Full
///   access: Read-only denies file edits, commands and MCP tools ([`ANTIGRAVITY_READ_ONLY_DENY_RULES`]),
///   Workspace write allows them ([`ANTIGRAVITY_WORKSPACE_WRITE_ALLOW_RULES`]), Full access approves every tool
///   (`--dangerously-skip-permissions`)
///
/// Unlike Claude and Codex, `agy` can't be pointed at a separate config folder in print mode
/// (its login lives in the Keychain next to `~/.gemini`), so the user's own Antigravity rules,
/// skills, plugins and MCP servers in `~/.gemini/config` still load.
/// - the step's agent and skills as [`AntigravitySpawnPlan::instructions`]
///
/// The prompt isn't added: callers pass it as `--print=<prompt>` or over stdin.
pub fn build_isolated_antigravity_spawn_plan(
    binary_path: impl AsRef<Path>,
    default_model: Option<&str>,
    working_dir: impl AsRef<Path>,
    materialized: &MaterializedStepContext,
    project_env: &BTreeMap<String, String>,
) -> AntigravitySpawnPlan {
    let bundle = &materialized.bundle;
    let mut args = vec!["--output-format".to_owned(), "stream-json".to_owned()];
    let model = bundle
        .model
        .as_deref()
        .or(default_model)
        .map(str::trim)
        .filter(|m| !m.is_empty());
    if let Some(model) = model {
        args.push("--model".to_owned());
        args.push(model.to_owned());
    }
    if let Some(effort) = bundle
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|e| ["low", "medium", "high", "xhigh", "max"].contains(e))
    {
        args.push("--effort".to_owned());
        args.push(effort.to_owned());
    }
    args.push("--disable-slash-commands".to_owned());
    // Antigravity reports MCP servers that fail to start only in its log.
    args.push(format!(
        "--log-file={}",
        materialized
            .runtime_dir
            .join(ANTIGRAVITY_LOG_FILE)
            .display()
    ));
    // Antigravity starts the servers in `.agents/mcp_config.json` on its own; until the user
    // approves that file, none of their tools may be used.
    let deny_mcp: &[&str] = if bundle.antigravity_mcp.deny_all_mcp {
        ANTIGRAVITY_DENY_MCP_RULES
    } else {
        &[]
    };
    // Print mode can't ask anyone, so it refuses any tool call that would need approval.
    match bundle.sandbox {
        ProjectSandboxMode::ReadOnly => {
            // `--mode plan` isn't read-only (it writes a plan document), so deny the
            // permissions that change files or run commands.
            let settings = serde_json::json!({
                "permissions": { "deny": ANTIGRAVITY_READ_ONLY_DENY_RULES },
            });
            args.push("--settings".to_owned());
            args.push(settings.to_string());
            args.push("--sandbox".to_owned());
        }
        ProjectSandboxMode::WorkspaceWrite => {
            // `--mode auto` isn't offered to every account, so allow edits and commands
            // outright and keep the terminal sandbox around them.
            let mut permissions = serde_json::json!({
                "allow": ANTIGRAVITY_WORKSPACE_WRITE_ALLOW_RULES,
            });
            if !deny_mcp.is_empty() {
                permissions["deny"] = serde_json::json!(deny_mcp);
            }
            let settings = serde_json::json!({ "permissions": permissions });
            args.push("--settings".to_owned());
            args.push(settings.to_string());
            args.push("--sandbox".to_owned());
        }
        ProjectSandboxMode::FullAccess => {
            if !deny_mcp.is_empty() {
                let settings = serde_json::json!({ "permissions": { "deny": deny_mcp } });
                args.push("--settings".to_owned());
                args.push(settings.to_string());
            }
            args.push("--dangerously-skip-permissions".to_owned());
        }
    }

    let mut env = BTreeMap::new();
    for (k, v) in project_env {
        if k != ANTIGRAVITY_DISABLE_AUTO_UPDATE_ENV {
            env.insert(k.clone(), v.clone());
        }
    }
    env.insert(
        ANTIGRAVITY_DISABLE_AUTO_UPDATE_ENV.to_owned(),
        ANTIGRAVITY_DISABLE_AUTO_UPDATE_VALUE.to_owned(),
    );

    AntigravitySpawnPlan {
        binary_path: binary_path.as_ref().to_path_buf(),
        working_dir: Some(working_dir.as_ref().to_path_buf()),
        args,
        env,
        env_remove: BTreeSet::new(),
        instructions: Some(bundle.system_instructions.trim())
            .filter(|i| !i.is_empty())
            .map(str::to_owned),
    }
}

/// Applies least-privilege secrets delivery and isolation onto a [`ClaudeSpawnPlan`] (§7):
/// - Strips excluded, unshared, unreferenced `McpOnly`, `Internal`, `GH_TOKEN`, and `OP_*` vars.
/// - Injects resolved `ProviderSpawnEnv` secrets (marking them masked for previews).
/// - Deep-merges `McpOnly` names into `--settings` JSON `sandbox.credentials.envVars` as
///   `{ "name": "<VAR>", "mode": "deny" }` objects.
pub fn apply_claude_secrets_delivery(plan: &mut ClaudeSpawnPlan, delivery: &StepSecretsDelivery) {
    plan.env_remove.extend(delivery.env_remove.iter().cloned());
    for (k, v) in &delivery.provider_spawn_env {
        if k != CLAUDE_DISABLE_AUTOUPDATER_ENV {
            plan.env.insert(k.clone(), v.clone());
            plan.masked_env_keys.insert(k.clone());
            plan.env_remove.remove(k);
        }
    }
    for (k, v) in &delivery.env {
        if k != CLAUDE_DISABLE_AUTOUPDATER_ENV {
            plan.env.entry(k.clone()).or_insert_with(|| v.clone());
            plan.env_remove.remove(k);
        }
    }

    if !delivery.mcp_only_names.is_empty() {
        let mut root = match plan.settings_json.take() {
            Some(serde_json::Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        };
        let sandbox = root
            .entry("sandbox".to_owned())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !sandbox.is_object() {
            *sandbox = serde_json::Value::Object(serde_json::Map::new());
        }
        let sandbox_obj = sandbox.as_object_mut().expect("sandbox object");
        let creds = sandbox_obj
            .entry("credentials".to_owned())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !creds.is_object() {
            *creds = serde_json::Value::Object(serde_json::Map::new());
        }
        let creds_obj = creds.as_object_mut().expect("credentials object");
        let env_vars_val = creds_obj
            .entry("envVars".to_owned())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        if !env_vars_val.is_array() {
            *env_vars_val = serde_json::Value::Array(Vec::new());
        }
        let arr = env_vars_val.as_array_mut().expect("envVars array");

        // Normalize any legacy bare-string entries to `{ "name": ..., "mode": "deny" }` objects.
        for item in arr.iter_mut() {
            if let Some(name) = item.as_str().map(str::to_owned) {
                *item = serde_json::json!({ "name": name, "mode": "deny" });
            }
        }
        for name in &delivery.mcp_only_names {
            let already = arr.iter().any(|item| {
                item.get("name")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|existing| existing == name)
            });
            if !already {
                arr.push(serde_json::json!({ "name": name, "mode": "deny" }));
            }
        }

        let merged_val = serde_json::Value::Object(root);
        if let Ok(serialized) = serde_json::to_string(&merged_val) {
            set_or_push_flag(&mut plan.args, "--settings", &serialized);
        }
        plan.settings_json = Some(merged_val);
    }
}

/// Applies least-privilege secrets delivery and isolation onto a [`CodexSpawnPlan`] (§7):
/// - Strips excluded, unshared, unreferenced `McpOnly`, `Internal`, `GH_TOKEN`, and `OP_*` vars.
/// - Injects resolved `ProviderSpawnEnv` secrets.
/// - When `AgentAndSteps` secrets exist, sets `-c shell_environment_policy.ignore_default_excludes=true`
///   and `env_remove`s every ambient host env var containing `KEY`/`SECRET`/`TOKEN` unless explicitly allowed.
/// - Unions `shell_environment_policy.exclude` with user excludes + `McpOnly` names + Codex stdio
///   mapped keys sourced from `McpOnly` secrets (not mapped plain vars).
pub fn apply_codex_secrets_delivery(
    plan: &mut CodexSpawnPlan,
    delivery: &StepSecretsDelivery,
    base_settings: &CodexSpawnSettings,
    codex_mcp: &CodexMcpDelivery,
) {
    plan.env_remove.extend(delivery.env_remove.iter().cloned());
    for (k, v) in &delivery.provider_spawn_env {
        if k != "CODEX_HOME" {
            plan.env.insert(k.clone(), v.clone());
            plan.masked_env_keys.insert(k.clone());
            plan.env_remove.remove(k);
        }
    }
    for (k, v) in &delivery.env {
        if k != "CODEX_HOME" {
            plan.env.entry(k.clone()).or_insert_with(|| v.clone());
            plan.env_remove.remove(k);
        }
    }

    if !delivery.agent_and_steps_names.is_empty() {
        remove_codex_config_prefix(plan, "shell_environment_policy.ignore_default_excludes");
        push_codex_config(
            plan,
            "shell_environment_policy.ignore_default_excludes",
            "true",
        );

        // Strip ambient host env vars containing KEY/SECRET/TOKEN unless explicitly allowed.
        for (host_key, _) in std::env::vars() {
            if !is_ambient_credential_var_name(&host_key) {
                continue;
            }
            let allowed = delivery
                .agent_and_steps_names
                .iter()
                .any(|k| k.eq_ignore_ascii_case(&host_key))
                || delivery
                    .mcp_only_names
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(&host_key))
                || codex_mcp
                    .env
                    .keys()
                    .any(|k| k.eq_ignore_ascii_case(&host_key))
                || matches!(host_key.as_str(), "OPENAI_API_KEY" | "CODEX_ACCESS_TOKEN")
                || base_settings.env_vars.contains_key(&host_key)
                || base_settings.secret_env_vars.contains_key(&host_key);
            if !allowed {
                plan.env_remove.insert(host_key);
            }
        }
    }

    // Union `shell_environment_policy.exclude` with user excludes + McpOnly names + McpOnly-sourced mapped keys.
    let mut excludes: BTreeSet<String> = BTreeSet::new();
    if let Some(CodexSettingValue::StringList(user_excludes)) =
        base_settings.get("shell_environment_policy.exclude")
    {
        for item in user_excludes {
            let trimmed = item.trim();
            if !trimmed.is_empty() {
                excludes.insert(trimmed.to_owned());
            }
        }
    }
    excludes.extend(delivery.mcp_only_names.iter().cloned());
    excludes.extend(codex_mcp.mcp_only_mapped_keys.iter().cloned());

    if !excludes.is_empty() {
        remove_codex_config_prefix(plan, "shell_environment_policy.exclude");
        let array_toml = format!(
            "[{}]",
            excludes
                .iter()
                .map(|k| toml_quote(k))
                .collect::<Vec<_>>()
                .join(",")
        );
        push_codex_config(plan, "shell_environment_policy.exclude", &array_toml);
    }
}

/// Applies least-privilege secrets delivery and isolation onto an [`AntigravitySpawnPlan`] (§7):
/// - NEVER injects `McpOnly` secrets into `agy`.
/// - Explicitly strips all `McpOnly`, excluded, unshared, `Internal`, `GH_TOKEN`, and `OP_*` vars.
pub fn apply_antigravity_secrets_delivery(
    plan: &mut AntigravitySpawnPlan,
    delivery: &StepSecretsDelivery,
) {
    plan.env_remove.extend(delivery.env_remove.iter().cloned());
    plan.env_remove
        .extend(delivery.mcp_only_names.iter().cloned());
    for k in &delivery.mcp_only_names {
        plan.env.remove(k);
    }
    for (k, v) in &delivery.env {
        if k != ANTIGRAVITY_DISABLE_AUTO_UPDATE_ENV && !delivery.mcp_only_names.contains(k) {
            plan.env.insert(k.clone(), v.clone());
            plan.env_remove.remove(k);
        }
    }
}

fn try_symlink_file(src: &Path, dst: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dst).is_ok()
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(src, dst).is_ok()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (src, dst);
        false
    }
}

fn set_owner_read_write_only(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn push_codex_config(plan: &mut CodexSpawnPlan, key: &str, toml_value: &str) {
    plan.args.push("-c".to_owned());
    plan.args.push(format!("{key}={toml_value}"));
    plan.config_overrides
        .push((key.to_owned(), toml_value.to_owned()));
}

/// Removes `-c <key>…` overrides (from spawn settings) whose key starts with `prefix`.
fn remove_codex_config_prefix(plan: &mut CodexSpawnPlan, prefix: &str) {
    let matches = |value: &str| {
        let key = value.split('=').next().unwrap_or("").trim();
        key == prefix || key.starts_with(&format!("{prefix}."))
    };
    let mut i = 0;
    while i < plan.args.len() {
        if plan.args[i] == "-c" && plan.args.get(i + 1).is_some_and(|v| matches(v)) {
            plan.args.drain(i..i + 2);
        } else {
            i += 1;
        }
    }
    plan.config_overrides.retain(|(key, _)| !matches(key));
}

/// Removes `flag` and the values after it (up to the next `-`-prefixed argument).
fn remove_variadic_flag(args: &mut Vec<String>, flag: &str) {
    let mut i = 0;
    while i < args.len() {
        if args[i] == flag {
            let mut end = i + 1;
            while end < args.len() && !args[end].starts_with('-') {
                end += 1;
            }
            args.drain(i..end);
        } else if args[i].starts_with(&format!("{flag}=")) {
            args.remove(i);
        } else {
            i += 1;
        }
    }
}

fn toml_quote(raw: &str) -> String {
    serde_json::to_string(raw).unwrap_or_else(|_| format!("\"{raw}\""))
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn push_bool_flag_if_missing(args: &mut Vec<String>, flag: &str) {
    if !has_flag(args, flag) {
        args.push(flag.to_owned());
    }
}

fn push_flag_if_missing(args: &mut Vec<String>, flag: &str, value: &str) {
    if !has_flag(args, flag) {
        args.push(flag.to_owned());
        args.push(value.to_owned());
    }
}

fn set_or_push_flag(args: &mut Vec<String>, flag: &str, value: &str) {
    if let Some(pos) = args.iter().position(|a| a == flag) {
        if pos + 1 < args.len() {
            args[pos + 1] = value.to_owned();
        } else {
            args.push(value.to_owned());
        }
    } else {
        args.push(flag.to_owned());
        args.push(value.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::definitions::BRUK_CONFIG_DIR;

    #[test]
    fn compiles_minimal_step_context_excluding_unrelated_agents_and_skills() {
        let catalog = BrukCatalog::builtin_only();
        let step = AgentTaskStepDef {
            id: "plan".to_owned(),
            name: "Plan".to_owned(),
            agent: Some("planner".to_owned()),
            provider: Some(ProviderKind::Claude),
            model: None,
            skills: Vec::new(),
            prompt: "Plan fix for {{ inputs.prompt }}".to_owned(),
            inputs: BTreeMap::new(),
            outputs: Vec::new(),
            mcp: Vec::new(),
        };
        let mut ctx = TemplateContext::default();
        ctx.inputs
            .insert("prompt".to_owned(), "Issue #42 timeout".to_owned());

        let bundle = compile_step_context(
            &catalog,
            &step,
            &ctx,
            ProviderKind::Claude,
            ProjectSandboxMode::WorkspaceWrite,
            None,
            StepMcpScope::none(),
        )
        .unwrap();

        assert_eq!(bundle.rendered_prompt, "Plan fix for Issue #42 timeout");
        assert_eq!(bundle.sandbox, ProjectSandboxMode::ReadOnly);
        assert_eq!(bundle.included_skill_ids, vec!["builtin/minimal-diffs"]);
        assert!(bundle.subagents.is_empty());
        assert!(
            bundle
                .system_instructions
                .contains("software architect and planner")
        );
        assert!(
            bundle
                .system_instructions
                .contains("## Skill: Minimal Diffs")
        );
        // Unrelated agent & skill context MUST NOT leak into this step!
        assert!(!bundle.system_instructions.contains("Test Verification"));
        assert!(!bundle.system_instructions.contains("thorough code review"));
        assert!(!bundle.system_instructions.contains("isolated git worktree"));
    }

    #[test]
    fn claude_isolated_plan_strips_user_context_and_keeps_worktree_clean() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let runtime_dir = temp.path().join("runtime/step-001");
        fs::create_dir_all(repo.join(BRUK_CONFIG_DIR).join("agents")).unwrap();
        fs::write(
            repo.join(BRUK_CONFIG_DIR).join("agents/lead.md"),
            "---\nid: lead\nextends: planner\nsubagents:\n  - reviewer\n---\nCoordinate carefully.\n",
        )
        .unwrap();

        let catalog = BrukCatalog::load_for_project(&repo);
        let step = AgentTaskStepDef {
            id: "lead_step".to_owned(),
            name: "Lead".to_owned(),
            agent: Some("lead".to_owned()),
            provider: Some(ProviderKind::Claude),
            model: None,
            skills: vec!["test-verification".to_owned()],
            prompt: "Analyze {{ steps.prev.output }}".to_owned(),
            inputs: BTreeMap::new(),
            outputs: Vec::new(),
            mcp: Vec::new(),
        };
        let mut ctx = TemplateContext::default();
        ctx.step_outputs
            .insert("prev.output".to_owned(), "diff summary".to_owned());

        let bundle = compile_step_context(
            &catalog,
            &step,
            &ctx,
            ProviderKind::Claude,
            ProjectSandboxMode::WorkspaceWrite,
            Some("claude-sonnet-4-6"),
            StepMcpScope::none(),
        )
        .unwrap();
        let mat = materialize_step_context(bundle, &runtime_dir, None).unwrap();

        // Files live strictly in runtime_dir, NOT inside repo.
        let instr_path = mat.instructions_file.as_ref().unwrap();
        let agents_path = mat.claude_agents_file.as_ref().unwrap();
        assert!(instr_path.starts_with(&runtime_dir));
        assert!(agents_path.starts_with(&runtime_dir));
        assert!(!instr_path.starts_with(&repo));

        let agents_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(agents_path).unwrap()).unwrap();
        assert!(agents_json.get("reviewer").is_some());
        assert!(agents_json.get("implementer").is_none());

        let plan = build_isolated_claude_spawn_plan(
            "/usr/local/bin/claude",
            Some("sonnet"),
            &ClaudeSpawnSettings::default(),
            &repo,
            &mat,
            Some("run-1-lead"),
            &BTreeMap::new(),
        )
        .unwrap();

        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--setting-sources", "project"])
        );
        assert!(plan.args.contains(&"--strict-mcp-config".to_owned()));
        assert!(plan.args.contains(&"--disable-slash-commands".to_owned()));
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--output-format", "stream-json"])
        );
        assert!(
            plan.args
                .windows(2)
                .any(|w| w[0] == "--append-system-prompt-file")
        );
        assert!(plan.args.windows(2).any(|w| w[0] == "--agents"));
        assert_eq!(
            plan.args.last().map(String::as_str),
            Some("Analyze diff summary")
        );
        assert_eq!(
            plan.env
                .get(CLAUDE_DISABLE_AUTOUPDATER_ENV)
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn codex_isolated_plan_creates_auth_only_codex_home_and_preserves_repo_agents_md() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let fake_user_codex = temp.path().join("user-codex-home");
        let runtime_dir = temp.path().join("runtime/step-002");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(fake_user_codex.join("skills")).unwrap();
        fs::write(fake_user_codex.join("auth.json"), r#"{"token":"ok"}"#).unwrap();
        fs::write(fake_user_codex.join("AGENTS.md"), "PERSONAL USER RULE").unwrap();
        fs::write(fake_user_codex.join("config.toml"), "model = \"personal\"").unwrap();
        fs::write(fake_user_codex.join("skills/personal.md"), "PERSONAL SKILL").unwrap();

        let catalog = BrukCatalog::builtin_only();
        let step = AgentTaskStepDef {
            id: "impl".to_owned(),
            name: "Implement".to_owned(),
            agent: Some("implementer".to_owned()),
            provider: Some(ProviderKind::Codex),
            model: Some("gpt-5.3-codex".to_owned()),
            skills: Vec::new(),
            prompt: "Apply patch".to_owned(),
            inputs: BTreeMap::new(),
            outputs: Vec::new(),
            mcp: Vec::new(),
        };

        let bundle = compile_step_context(
            &catalog,
            &step,
            &TemplateContext::default(),
            ProviderKind::Codex,
            ProjectSandboxMode::WorkspaceWrite,
            None,
            StepMcpScope::none(),
        )
        .unwrap();
        let mat = materialize_step_context(bundle, &runtime_dir, Some(&fake_user_codex)).unwrap();

        let isolated_home = mat.codex_home_dir.as_ref().unwrap();
        assert!(isolated_home.join("auth.json").is_file());
        assert!(!isolated_home.join("AGENTS.md").exists());
        assert!(!isolated_home.join("config.toml").exists());
        assert!(!isolated_home.join("skills").exists());

        let plan = build_isolated_codex_spawn_plan(
            "/usr/local/bin/codex",
            None,
            &CodexSpawnSettings::default(),
            &repo,
            &mat,
            &BTreeMap::new(),
        )
        .unwrap();

        assert_eq!(
            plan.env.get("CODEX_HOME").map(PathBuf::from).as_deref(),
            Some(isolated_home.as_path())
        );
        assert!(
            plan.args
                .contains(&CODEX_DISABLE_UPDATE_CHECK_ARG.to_owned())
        );
        assert!(
            plan.args
                .contains(&"skills.bundled.enabled=false".to_owned())
        );
        assert!(
            plan.args
                .contains(&"skills.include_instructions=false".to_owned())
        );
        assert!(
            plan.args
                .contains(&"sandbox_mode=\"workspace-write\"".to_owned())
        );
        assert!(
            plan.args
                .iter()
                .any(|a| a.starts_with("developer_instructions="))
        );
        // Repo AGENTS.md remains enabled (project_doc_max_bytes is NOT zeroed).
        assert!(!plan.args.contains(&"project_doc_max_bytes=0".to_owned()));
        assert_eq!(plan.args.last().map(String::as_str), Some("Apply patch"));
    }

    fn mcp_sources_for(repo: &Path) -> McpSources {
        let project = crate::mcp::load_project_mcp(repo);
        let fingerprint = project.config().map(|c| c.fingerprint.clone());
        McpSources {
            project,
            allow_project_servers: true,
            approved_fingerprint: fingerprint,
            project_extras: Vec::new(),
            user_extras: vec![crate::mcp::McpServerDef::from_form("mine", "sh", "", "").unwrap()],
        }
    }

    fn mcp_repo(temp: &TempDir) -> PathBuf {
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join(".agents")).unwrap();
        fs::write(
            repo.join(".agents/mcp_config.json"),
            r#"{"mcpServers":{
                "proj":{"command":"sh","args":["-c","true"],"env":{"TOKEN":"${PROJ_TOKEN}"}},
                "web":{"serverUrl":"https://example.com/mcp","headers":{"Authorization":"Bearer ${WEB_TOKEN}"}}
            }}"#,
        )
        .unwrap();
        repo
    }

    fn agent_step(provider: ProviderKind) -> AgentTaskStepDef {
        AgentTaskStepDef {
            id: "s".to_owned(),
            name: "S".to_owned(),
            agent: None,
            provider: Some(provider),
            model: None,
            skills: Vec::new(),
            prompt: "Go".to_owned(),
            inputs: BTreeMap::new(),
            outputs: Vec::new(),
            mcp: Vec::new(),
        }
    }

    fn mcp_env() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("PROJ_TOKEN".to_owned(), "p-secret".to_owned()),
            ("WEB_TOKEN".to_owned(), "w-secret".to_owned()),
            ("PATH".to_owned(), "/bin:/usr/bin".to_owned()),
        ])
    }

    #[test]
    fn claude_gets_project_and_user_servers_without_secrets_on_disk() {
        let temp = TempDir::new().unwrap();
        let repo = mcp_repo(&temp);
        let sources = mcp_sources_for(&repo);
        let env = mcp_env();
        let catalog = BrukCatalog::load_for_project(&repo);
        let bundle = compile_step_context(
            &catalog,
            &agent_step(ProviderKind::Claude),
            &TemplateContext::default(),
            ProviderKind::Claude,
            ProjectSandboxMode::WorkspaceWrite,
            None,
            StepMcpScope {
                sources: &sources,
                policy: McpSourcesPolicy::All,
                env: &env,
                resolved_secrets: None,
                project_id: None,
            },
        )
        .unwrap();
        assert!(bundle.mcp.problems.is_empty(), "{:?}", bundle.mcp.problems);
        let runtime_dir = temp.path().join("rt");
        let mat = materialize_step_context(bundle, &runtime_dir, None).unwrap();
        let file = mat.claude_mcp_file.clone().unwrap();
        let raw = fs::read_to_string(&file).unwrap();
        assert!(raw.contains("${PROJ_TOKEN}") && !raw.contains("p-secret"));
        let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let names: Vec<&String> = json["mcpServers"].as_object().unwrap().keys().collect();
        assert_eq!(names, ["mine", "proj", "web"]);

        let mut user_settings = ClaudeSpawnSettings::default();
        user_settings
            .set_from_text("cli.mcp-config", "/home/me/mcp.json")
            .unwrap();
        let plan = build_isolated_claude_spawn_plan(
            "/usr/local/bin/claude",
            None,
            &user_settings,
            &repo,
            &mat,
            None,
            &env,
        )
        .unwrap();
        let strict = plan
            .args
            .iter()
            .position(|a| a == "--strict-mcp-config")
            .unwrap();
        assert_eq!(plan.args[strict - 2], "--mcp-config");
        assert_eq!(plan.args[strict - 1], file.to_string_lossy());
        assert!(
            plan.args.contains(&"/home/me/mcp.json".to_owned()),
            "user extras stay under `all`"
        );
        assert_eq!(plan.args.last().map(String::as_str), Some("Go"));
    }

    #[test]
    fn project_only_policy_strips_user_mcp_settings_for_claude_and_codex() {
        let temp = TempDir::new().unwrap();
        let repo = mcp_repo(&temp);
        let sources = mcp_sources_for(&repo);
        let env = mcp_env();
        let catalog = BrukCatalog::load_for_project(&repo);
        let scope = StepMcpScope {
            sources: &sources,
            policy: McpSourcesPolicy::Project,
            env: &env,
            resolved_secrets: None,
            project_id: None,
        };

        let bundle = compile_step_context(
            &catalog,
            &agent_step(ProviderKind::Claude),
            &TemplateContext::default(),
            ProviderKind::Claude,
            ProjectSandboxMode::WorkspaceWrite,
            None,
            scope,
        )
        .unwrap();
        assert_eq!(bundle.mcp.withheld.len(), 1);
        let mat = materialize_step_context(bundle, temp.path().join("rt-claude"), None).unwrap();
        let mut user_settings = ClaudeSpawnSettings::default();
        user_settings
            .set_from_text("cli.mcp-config", "/home/me/mcp.json")
            .unwrap();
        let plan = build_isolated_claude_spawn_plan(
            "/usr/local/bin/claude",
            None,
            &user_settings,
            &repo,
            &mat,
            None,
            &env,
        )
        .unwrap();
        assert!(!plan.args.contains(&"/home/me/mcp.json".to_owned()));
        assert_eq!(plan.args.iter().filter(|a| *a == "--mcp-config").count(), 1);

        let bundle = compile_step_context(
            &catalog,
            &agent_step(ProviderKind::Codex),
            &TemplateContext::default(),
            ProviderKind::Codex,
            ProjectSandboxMode::WorkspaceWrite,
            None,
            scope,
        )
        .unwrap();
        let mat = materialize_step_context(bundle, temp.path().join("rt-codex"), None).unwrap();
        let mut codex_settings = CodexSpawnSettings::default();
        codex_settings
            .set_from_text("mcp_servers", r#"{"mine":{"command":"sh"}}"#)
            .unwrap();
        let plan = build_isolated_codex_spawn_plan(
            "/usr/local/bin/codex",
            None,
            &codex_settings,
            &repo,
            &mat,
            &env,
        )
        .unwrap();
        let overrides: Vec<&String> = plan
            .args
            .windows(2)
            .filter(|w| w[0] == "-c")
            .map(|w| &w[1])
            .collect();
        assert!(!overrides.iter().any(|o| o.starts_with("mcp_servers=")));
        assert!(overrides.iter().any(|o| o.starts_with("mcp_servers.proj=")));
        assert!(
            overrides
                .iter()
                .any(|o| o.contains("bearer_token_env_var=\"WEB_TOKEN\""))
        );
        assert!(
            overrides
                .iter()
                .any(|o| *o == crate::mcp::CODEX_DISABLE_APPS_ARG)
        );
        assert!(!overrides.iter().any(|o| o.contains("p-secret")));
        assert_eq!(plan.env.get("TOKEN").map(String::as_str), Some("p-secret"));
        assert_eq!(
            plan.env.get("RUST_LOG").map(String::as_str),
            Some(crate::mcp::CODEX_MCP_RUST_LOG)
        );
    }

    #[test]
    fn codex_puts_bruk_servers_after_a_whole_table_override_from_spawn_settings() {
        let temp = TempDir::new().unwrap();
        let repo = mcp_repo(&temp);
        let sources = mcp_sources_for(&repo);
        let env = mcp_env();
        let catalog = BrukCatalog::load_for_project(&repo);
        let bundle = compile_step_context(
            &catalog,
            &agent_step(ProviderKind::Codex),
            &TemplateContext::default(),
            ProviderKind::Codex,
            ProjectSandboxMode::WorkspaceWrite,
            None,
            StepMcpScope {
                sources: &sources,
                policy: McpSourcesPolicy::All,
                env: &env,
                resolved_secrets: None,
                project_id: None,
            },
        )
        .unwrap();
        let mat = materialize_step_context(bundle, temp.path().join("rt"), None).unwrap();
        let mut codex_settings = CodexSpawnSettings::default();
        codex_settings
            .set_from_text("mcp_servers", r#"{"theirs":{"command":"sh"}}"#)
            .unwrap();
        let plan = build_isolated_codex_spawn_plan(
            "/usr/local/bin/codex",
            None,
            &codex_settings,
            &repo,
            &mat,
            &env,
        )
        .unwrap();
        let table = plan
            .args
            .iter()
            .position(|a| a.starts_with("mcp_servers="))
            .unwrap();
        let ours = plan
            .args
            .iter()
            .position(|a| a.starts_with("mcp_servers.proj="))
            .unwrap();
        assert!(
            table < ours,
            "a whole-table override after ours would replace them"
        );
    }

    #[test]
    fn antigravity_denies_mcp_tools_until_the_project_file_is_approved() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo-no-vars");
        fs::create_dir_all(repo.join(".agents")).unwrap();
        fs::write(
            repo.join(".agents/mcp_config.json"),
            r#"{"mcpServers":{"local":{"command":"sh","args":["-c","true"]}}}"#,
        )
        .unwrap();
        let mut sources = mcp_sources_for(&repo);
        sources.approved_fingerprint = None;
        let env = mcp_env();
        let catalog = BrukCatalog::load_for_project(&repo);
        for (sandbox, approved) in [
            (ProjectSandboxMode::WorkspaceWrite, false),
            (ProjectSandboxMode::FullAccess, false),
            (ProjectSandboxMode::WorkspaceWrite, true),
        ] {
            let mut sources = sources.clone();
            if approved {
                sources.approved_fingerprint =
                    sources.project.config().map(|c| c.fingerprint.clone());
            }
            let bundle = compile_step_context(
                &catalog,
                &agent_step(ProviderKind::Antigravity),
                &TemplateContext::default(),
                ProviderKind::Antigravity,
                sandbox,
                None,
                StepMcpScope {
                    sources: &sources,
                    policy: McpSourcesPolicy::All,
                    env: &env,
                    resolved_secrets: None,
                    project_id: None,
                },
            )
            .unwrap();
            assert!(!bundle.mcp.has_blocking_problem());
            let mat = materialize_step_context(bundle, temp.path().join("rt"), None).unwrap();
            let plan = build_isolated_antigravity_spawn_plan("/bin/agy", None, &repo, &mat, &env);
            let settings = plan.args.iter().find(|a| a.contains("permissions"));
            let denies_mcp = settings.is_some_and(|s| s.contains("mcp(*)"));
            assert_eq!(denies_mcp, !approved, "{sandbox:?} approved={approved}");
            assert!(plan.args.iter().any(|a| a.starts_with("--log-file=")));
        }
    }

    #[test]
    fn antigravity_blocks_agents_mcp_config_with_env_var_references() {
        let temp = TempDir::new().unwrap();
        let repo = mcp_repo(&temp);
        let sources = mcp_sources_for(&repo);
        let env = mcp_env();
        let catalog = BrukCatalog::load_for_project(&repo);
        let bundle = compile_step_context(
            &catalog,
            &agent_step(ProviderKind::Antigravity),
            &TemplateContext::default(),
            ProviderKind::Antigravity,
            ProjectSandboxMode::WorkspaceWrite,
            None,
            StepMcpScope {
                sources: &sources,
                policy: McpSourcesPolicy::All,
                env: &env,
                resolved_secrets: None,
                project_id: None,
            },
        )
        .unwrap();
        assert!(bundle.mcp.has_blocking_problem());
        assert!(
            bundle
                .mcp
                .problems
                .iter()
                .any(|p| p.blocking && p.fix == crate::mcp::McpFix::ChangeProvider),
            "{:?}",
            bundle.mcp.problems
        );
    }

    #[test]
    fn agent_frontmatter_narrows_servers_and_tightens_the_policy() {
        let temp = TempDir::new().unwrap();
        let repo = mcp_repo(&temp);
        fs::create_dir_all(repo.join(BRUK_CONFIG_DIR).join("agents")).unwrap();
        fs::write(
            repo.join(BRUK_CONFIG_DIR).join("agents/web.md"),
            "---\nid: web\nmcp:\n  - web\nmcp_sources: project\n---\nBrowse.\n",
        )
        .unwrap();
        let catalog = BrukCatalog::load_for_project(&repo);
        let sources = mcp_sources_for(&repo);
        let env = mcp_env();
        let mut step = agent_step(ProviderKind::Claude);
        step.agent = Some("web".to_owned());
        let bundle = compile_step_context(
            &catalog,
            &step,
            &TemplateContext::default(),
            ProviderKind::Claude,
            ProjectSandboxMode::WorkspaceWrite,
            None,
            StepMcpScope {
                sources: &sources,
                policy: McpSourcesPolicy::All,
                env: &env,
                resolved_secrets: None,
                project_id: None,
            },
        )
        .unwrap();
        assert_eq!(bundle.mcp.policy, McpSourcesPolicy::Project);
        let names: Vec<&str> = bundle
            .mcp
            .servers
            .iter()
            .map(|s| s.def.name.as_str())
            .collect();
        assert_eq!(names, ["web"]);
        assert!(bundle.mcp.servers[0].required);
    }

    #[test]
    fn claude_secrets_delivery_emits_deny_objects_and_strips_env_remove() {
        let settings = ClaudeSpawnSettings {
            extra_settings_json:
                r#"{"sandbox":{"credentials":{"envVars":[{"name":"PRE_EXISTING","mode":"deny"}]}}}"#
                    .to_owned(),
            ..Default::default()
        };
        let mut plan =
            crate::build_claude_spawn_plan("/usr/bin/claude", None, &settings, &Default::default())
                .unwrap();

        let delivery = StepSecretsDelivery {
            env: [("AGENT_ONLY".to_owned(), "agent_secret_val_1234".to_owned())]
                .into_iter()
                .collect(),
            env_remove: ["EXCLUDED_VAR".to_owned(), "MCP_ONLY".to_owned()]
                .into_iter()
                .collect(),
            mcp_only_names: ["MCP_ONLY".to_owned(), "INTERNAL_SECRET".to_owned()]
                .into_iter()
                .collect(),
            agent_and_steps_names: ["AGENT_ONLY".to_owned()].into_iter().collect(),
            provider_spawn_env: [(
                "ANTHROPIC_API_KEY".to_owned(),
                "sk-ant-spawn-9999".to_owned(),
            )]
            .into_iter()
            .collect(),
        };

        apply_claude_secrets_delivery(&mut plan, &delivery);

        assert_eq!(
            plan.env.get("AGENT_ONLY").map(String::as_str),
            Some("agent_secret_val_1234")
        );
        assert_eq!(
            plan.env.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some("sk-ant-spawn-9999")
        );
        assert!(plan.env_remove.contains("EXCLUDED_VAR"));
        assert!(plan.env_remove.contains("MCP_ONLY"));
        // Preview must mask provider_spawn_env secrets
        let preview = plan.preview_command_line();
        assert!(!preview.contains("sk-ant-spawn-9999"));
        assert!(preview.contains("[secret:ANTHROPIC_API_KEY]"));

        // Verify --settings JSON merged sandbox.credentials.envVars as {name, mode: "deny"} objects.
        let parsed = plan.settings_json.as_ref().unwrap();
        let arr = parsed["sandbox"]["credentials"]["envVars"]
            .as_array()
            .unwrap();
        let names: BTreeSet<&str> = arr
            .iter()
            .map(|v| {
                assert_eq!(v["mode"], "deny");
                v["name"].as_str().unwrap()
            })
            .collect();
        assert!(names.contains("PRE_EXISTING"));
        assert!(names.contains("MCP_ONLY"));
        assert!(names.contains("INTERNAL_SECRET"));
    }

    #[test]
    fn codex_secrets_delivery_sets_ignore_default_excludes_and_merges_exclusions() {
        let settings = CodexSpawnSettings::default();
        let mut plan =
            crate::build_codex_spawn_plan("/usr/bin/codex", None, &settings, &Default::default())
                .unwrap();

        let delivery = StepSecretsDelivery {
            env: [(
                "CUSTOM_API_KEY".to_owned(),
                "sk_custom_1234567890".to_owned(),
            )]
            .into_iter()
            .collect(),
            env_remove: ["AMBIENT_TOKEN".to_owned(), "MCP_SECRET".to_owned()]
                .into_iter()
                .collect(),
            mcp_only_names: ["MCP_SECRET".to_owned()].into_iter().collect(),
            agent_and_steps_names: ["CUSTOM_API_KEY".to_owned()].into_iter().collect(),
            provider_spawn_env: [(
                "OPENAI_API_KEY".to_owned(),
                "sk-openai-spawn-9999".to_owned(),
            )]
            .into_iter()
            .collect(),
        };
        let codex_mcp = CodexMcpDelivery::default();

        apply_codex_secrets_delivery(&mut plan, &delivery, &settings, &codex_mcp);

        assert_eq!(
            plan.env.get("CUSTOM_API_KEY").map(String::as_str),
            Some("sk_custom_1234567890")
        );
        assert!(plan.env_remove.contains("AMBIENT_TOKEN"));
        assert!(plan.env_remove.contains("MCP_SECRET"));
        assert!(plan.config_overrides.iter().any(|(k, v)| {
            k == "shell_environment_policy.ignore_default_excludes" && v == "true"
        }));
        assert!(
            plan.config_overrides
                .iter()
                .any(|(k, v)| k == "shell_environment_policy.exclude" && v.contains("MCP_SECRET"))
        );
        let preview = plan.preview_command_line();
        assert!(!preview.contains("sk-openai-spawn-9999"));
        assert!(preview.contains("[secret:OPENAI_API_KEY]"));
    }
}
