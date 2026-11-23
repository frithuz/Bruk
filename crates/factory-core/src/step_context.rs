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
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    claude_settings::{
        CLAUDE_DISABLE_AUTOUPDATER_ENV, CLAUDE_DISABLE_AUTOUPDATER_VALUE, ClaudeSettingsError,
        ClaudeSpawnOptions, ClaudeSpawnPlan, ClaudeSpawnSettings, build_claude_spawn_plan,
    },
    codex_settings::{
        CODEX_DISABLE_UPDATE_CHECK_ARG, CodexSettingsError, CodexSpawnOptions, CodexSpawnPlan,
        CodexSpawnSettings, build_codex_spawn_plan,
    },
    definitions::{AgentTaskStepDef, AgentToolsPolicy, BrukCatalog, CatalogError, ResolvedAgent},
    preferences::ProviderKind,
    project_settings::ProjectSandboxMode,
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
}

/// Out-of-repo materialized runtime files for one step execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedStepContext {
    pub bundle: StepContextBundle,
    pub runtime_dir: PathBuf,
    pub instructions_file: Option<PathBuf>,
    pub claude_agents_file: Option<PathBuf>,
    pub codex_home_dir: Option<PathBuf>,
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
    })
}

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

    let codex_home_dir = if bundle.provider == ProviderKind::Codex {
        let home = runtime_dir.join("codex-home");
        fs::create_dir_all(&home).map_err(|source| StepContextError::CreateDir {
            path: home.clone(),
            source,
        })?;
        if let Some(src_home) = source_codex_home {
            // Copy ONLY auth credential files; never copy user AGENTS.md, skills/, or config.toml.
            for auth_file in ["auth.json", "credentials.json"] {
                let src = src_home.join(auth_file);
                if src.is_file() {
                    let dst = home.join(auth_file);
                    let _ = fs::copy(&src, &dst);
                }
            }
        }
        Some(home)
    } else {
        None
    };

    Ok(MaterializedStepContext {
        bundle,
        runtime_dir,
        instructions_file,
        claude_agents_file,
        codex_home_dir,
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
    push_bool_flag_if_missing(&mut plan.args, "--strict-mcp-config");
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

    Ok(plan)
}

fn push_codex_config(plan: &mut CodexSpawnPlan, key: &str, toml_value: &str) {
    plan.args.push("-c".to_owned());
    plan.args.push(format!("{key}={toml_value}"));
    plan.config_overrides
        .push((key.to_owned(), toml_value.to_owned()));
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
        };

        let bundle = compile_step_context(
            &catalog,
            &step,
            &TemplateContext::default(),
            ProviderKind::Codex,
            ProjectSandboxMode::WorkspaceWrite,
            None,
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
}
