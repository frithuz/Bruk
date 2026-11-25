//! Real local workflow execution engine, git worktree isolation, live transcript streaming,
//! approval gate signaling, and live `git diff` capture.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    claude_settings::ClaudeSpawnSettings,
    codex_settings::CodexSpawnSettings,
    definitions::{
        AgentTaskStepDef, BrukCatalog, CommentOnIssueStepDef, CompositeStepDef, LoopNodeDef,
        OpenPullRequestStepDef, SetIssueStatusStepDef, ShellCommandStepDef, WorkflowDef,
        WorkflowNodeDef,
    },
    github_intake::{
        GitHubAuth, add_github_issue_label, comment_on_github_issue, remove_github_issue_label,
        resolve_gh_binary,
    },
    inbox::ReviewFileStat,
    mcp::{
        ANTIGRAVITY_LOG_FILE, McpFailureTracker, McpOrigin, McpProblem, McpSources,
        McpSourcesPolicy, antigravity_log_failures, claude_init_failures, codex_stderr_failure,
        resolve_mcp_servers,
    },
    model::ProjectId,
    preferences::ProviderKind,
    project_settings::{IssueSourceAuthMode, ProjectSandboxMode},
    secrets::{
        CancellationToken, ResolvedSecrets, RunSecretsCache, SecretId, SecretProblem,
        SecretRedactor, SecretResolver, SecretsCache, collect_workflow_secret_requirements,
    },
    step_context::{
        EphemeralCodexAuthGuard, StepMcpScope, StepSecretsDelivery, TemplateContext,
        apply_antigravity_secrets_delivery, apply_claude_secrets_delivery,
        apply_codex_secrets_delivery, build_isolated_antigravity_spawn_plan,
        build_isolated_claude_spawn_plan, build_isolated_codex_spawn_plan, compile_step_context,
        default_step_runtime_dir, materialize_step_context,
    },
    workflow_editor::{ExitField, ExitOperator, LoopExhaustedAction, LoopRepeatMode},
    workflow_run::{
        AttemptBanner, BudgetGuardState, DiffFileChange, DiffLine, DiffLineKind, RunExecutionState,
        RunStepDetail, RunStepStatus, StepAgentInfo, StepGitInfo, StepOutputChip, StepProgressInfo,
        StepTranscriptEntry, TerminalBadgeTone, TerminalLine, ToolStatusTone, WorkflowRunViewData,
    },
};

/// Errors produced by workflow runner setup or execution.
#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("workflow '{0}' has no executable steps")]
    EmptyWorkflow(String),
    #[error("provider CLI for {0:?} is not configured or could not be found")]
    ProviderBinaryMissing(ProviderKind),
    #[error("step context error: {0}")]
    StepContext(#[from] crate::step_context::StepContextError),
    #[error("process execution failed: {0}")]
    Process(String),
    #[error("composite '{0}' was not found in catalog")]
    CompositeNotFound(String),
    #[error("run stopped by operator")]
    Stopped,
    #[error("{0}")]
    Mcp(String),
    #[error("{0}")]
    Secret(String),
}

/// Complete configuration passed to [`spawn_workflow_run`] or [`execute_workflow_blocking`].
#[derive(Debug, Clone)]
pub struct WorkflowRunRequest {
    pub run_id: String,
    pub run_number: u32,
    pub project_id: ProjectId,
    pub project_name: String,
    pub project_root: PathBuf,
    pub default_branch: String,
    pub workflow: WorkflowDef,
    pub catalog: BrukCatalog,
    pub inputs: BTreeMap<String, String>,
    pub issue_number: Option<u32>,
    pub github_repo: Option<String>,
    pub github_auth: Option<GitHubAuth>,
    /// GitHub sign-in mode (used with `github_pat_secret_id` for off-UI-thread PAT resolution).
    pub github_auth_mode: IssueSourceAuthMode,
    /// Optional `Internal { purpose: GitHubPat }` secret id resolved on the runner thread.
    pub github_pat_secret_id: Option<SecretId>,
    /// Optional off-UI-thread secret resolver for Env & Internal secrets.
    pub secret_resolver: Option<Arc<SecretResolver>>,
    pub default_provider: ProviderKind,
    pub default_sandbox: ProjectSandboxMode,
    pub model_override: Option<String>,
    pub claude_binary: Option<PathBuf>,
    pub claude_default_model: Option<String>,
    pub claude_settings: ClaudeSpawnSettings,
    pub codex_binary: Option<PathBuf>,
    pub codex_default_model: Option<String>,
    pub codex_settings: CodexSpawnSettings,
    pub source_codex_home: Option<PathBuf>,
    pub antigravity_binary: Option<PathBuf>,
    pub antigravity_default_model: Option<String>,
    pub use_isolated_worktrees: bool,
    pub branch_prefix: String,
    pub open_prs_as_draft: bool,
    pub link_pr_closes_issue: bool,
    pub project_env: BTreeMap<String, String>,
    /// MCP servers the run's steps choose from, gathered when the run starts.
    pub mcp: McpSources,
    pub runtime_root: Option<PathBuf>,
    pub budget_limit_cents: u64,
}

/// Decision supplied by the operator to unblock a suspended approval gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Approve { note: Option<String> },
    RequestChanges { note: Option<String> },
    Stop,
}

#[derive(Default)]
struct GateSlot {
    pending_gate_id: Option<String>,
    decision: Option<GateDecision>,
}

struct ControlShared {
    stopped: AtomicBool,
    active_child_pid: AtomicU32,
    cancel: CancellationToken,
    gate: Mutex<GateSlot>,
    gate_cv: Condvar,
    steering_queue: Mutex<Vec<String>>,
}

/// Thread-safe handle used by the UI or tests to stop, steer, or unblock approval gates.
#[derive(Clone)]
pub struct RunControlHandle {
    shared: Arc<ControlShared>,
}

impl Default for RunControlHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl RunControlHandle {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(ControlShared {
                stopped: AtomicBool::new(false),
                active_child_pid: AtomicU32::new(0),
                cancel: CancellationToken::new(),
                gate: Mutex::new(GateSlot::default()),
                gate_cv: Condvar::new(),
                steering_queue: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Cooperative cancellation token wired to this run's stop handle.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.shared.cancel
    }

    /// Requests cancellation, terminates the active child process if running, kills any in-flight
    /// secret manager subprocesses, and unblocks gates.
    pub fn stop(&self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        self.shared.cancel.cancel();
        let pid = self.shared.active_child_pid.swap(0, Ordering::SeqCst);
        if pid > 0 {
            terminate_pid(pid);
        }
        let mut guard = self.shared.gate.lock().expect("gate mutex poisoned");
        guard.decision = Some(GateDecision::Stop);
        self.shared.gate_cv.notify_all();
    }

    pub fn is_stopped(&self) -> bool {
        self.shared.stopped.load(Ordering::SeqCst)
    }

    /// Unblocks a waiting approval gate with the operator's decision.
    pub fn resolve_gate(&self, decision: GateDecision) {
        let mut guard = self.shared.gate.lock().expect("gate mutex poisoned");
        guard.decision = Some(decision);
        self.shared.gate_cv.notify_all();
    }

    /// Queues a steering note to inject into the next agent step / loop retry.
    pub fn push_steering(&self, note: String) {
        let trimmed = note.trim();
        if !trimmed.is_empty() {
            let mut q = self
                .shared
                .steering_queue
                .lock()
                .expect("steering mutex poisoned");
            q.push(trimmed.to_owned());
        }
    }

    fn drain_steering(&self) -> Vec<String> {
        let mut q = self
            .shared
            .steering_queue
            .lock()
            .expect("steering mutex poisoned");
        std::mem::take(&mut *q)
    }

    fn set_active_pid(&self, pid: u32) {
        self.shared.active_child_pid.store(pid, Ordering::SeqCst);
    }

    fn clear_active_pid(&self) {
        self.shared.active_child_pid.store(0, Ordering::SeqCst);
    }

    fn wait_for_gate(&self, gate_id: &str) -> GateDecision {
        let mut guard = self.shared.gate.lock().expect("gate mutex poisoned");
        if self.is_stopped() {
            return GateDecision::Stop;
        }
        guard.pending_gate_id = Some(gate_id.to_owned());
        guard.decision = None;
        while guard.decision.is_none() && !self.is_stopped() {
            guard = self
                .shared
                .gate_cv
                .wait(guard)
                .expect("gate mutex poisoned");
        }
        guard.pending_gate_id = None;
        if self.is_stopped() {
            GateDecision::Stop
        } else {
            guard.decision.take().unwrap_or(GateDecision::Stop)
        }
    }
}

/// Summary of one planned step emitted when a run starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedStepSummary {
    pub step_id: String,
    pub title: String,
    pub actor_badge: String,
}

/// Live events emitted by the local workflow runner for UI streaming and persistence.
#[derive(Debug, Clone)]
pub enum RunnerEvent {
    RunStarted {
        run_id: String,
        run_number: u32,
        branch_name: String,
        worktree_path: PathBuf,
        steps: Vec<PlannedStepSummary>,
    },
    StepStarted {
        step_index: usize,
        step_id: String,
        title: String,
        actor_badge: String,
        attempt: Option<u32>,
        max_attempts: Option<u32>,
        agent_info: Box<StepAgentInfo>,
        git_info: Box<StepGitInfo>,
    },
    TranscriptAppended {
        step_index: usize,
        entry: StepTranscriptEntry,
    },
    /// An MCP server didn't reach a step (also appended to its transcript), for notifications.
    McpProblem {
        step_index: usize,
        step_title: String,
        problem: McpProblem,
    },
    /// A secret or secret manager failed during preflight or step execution.
    SecretProblem {
        step_index: Option<usize>,
        step_title: String,
        problem: SecretProblem,
    },
    /// MCP servers that started for a step, so earlier notifications about them can clear.
    McpServersReady {
        servers: Vec<(String, McpOrigin)>,
    },
    StepFinished {
        step_index: usize,
        status: RunStepStatus,
        duration_ms: u64,
        output_preview: String,
        tokens_used: u64,
        cost_cents: u64,
    },
    DiffUpdated {
        diffs: Vec<DiffFileChange>,
    },
    ApprovalRequested {
        gate_id: String,
        step_index: usize,
        step_title: String,
        prompt: String,
        branch_name: String,
        base_branch: String,
        diff_summary: Vec<ReviewFileStat>,
    },
    RunFinished {
        status: RunExecutionState,
        total_duration_ms: u64,
        total_tokens: u64,
        total_cost_cents: u64,
        summary: String,
        error: Option<String>,
    },
}

/// Final outcome returned when [`execute_workflow_blocking`] completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcomeSummary {
    pub run_id: String,
    pub status: RunExecutionState,
    pub branch_name: String,
    pub worktree_path: PathBuf,
    pub total_duration_ms: u64,
    pub total_tokens: u64,
    pub total_cost_cents: u64,
    pub summary: String,
    pub error: Option<String>,
}

/// Spawns workflow execution on a dedicated background thread and returns the control handle
/// plus an [`mpsc::Receiver<RunnerEvent>`] for live UI updates.
pub fn spawn_workflow_run(
    request: WorkflowRunRequest,
) -> (RunControlHandle, mpsc::Receiver<RunnerEvent>) {
    let control = RunControlHandle::new();
    let control_for_thread = control.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = execute_workflow_blocking(request, control_for_thread, tx);
    });
    (control, rx)
}

/// Executes a workflow synchronously on the calling thread, streaming [`RunnerEvent`]s to `tx`.
pub fn execute_workflow_blocking(
    request: WorkflowRunRequest,
    control: RunControlHandle,
    tx: mpsc::Sender<RunnerEvent>,
) -> RunOutcomeSummary {
    let started_at = Instant::now();
    let runtime_root = request.runtime_root.clone().unwrap_or_else(|| {
        default_step_runtime_dir(&request.run_id, 0)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir)
    });
    let _ = fs::create_dir_all(&runtime_root);

    let (working_dir, branch_name, is_isolated_worktree) =
        prepare_checkout(&request, &runtime_root);

    let planned = collect_planned_steps(&request.workflow.nodes, &request.catalog);
    let _ = tx.send(RunnerEvent::RunStarted {
        run_id: request.run_id.clone(),
        run_number: request.run_number,
        branch_name: branch_name.clone(),
        worktree_path: working_dir.clone(),
        steps: planned,
    });

    let mut template_ctx = TemplateContext {
        inputs: request.inputs.clone(),
        step_outputs: BTreeMap::new(),
        loop_attempt: None,
        loop_previous: BTreeMap::new(),
    };
    for (k, param) in &request.workflow.inputs {
        if !template_ctx.inputs.contains_key(k)
            && let Some(default_val) = param.default.as_ref()
        {
            template_ctx.inputs.insert(k.clone(), default_val.clone());
        }
    }

    let mut state = ExecutionCursor {
        request: &request,
        control: &control,
        tx: &tx,
        runtime_root: &runtime_root,
        working_dir: &working_dir,
        branch_name: &branch_name,
        is_isolated_worktree,
        next_step_index: 0,
        total_tokens: 0,
        total_cost_cents: 0,
        mcp_policy: request.workflow.mcp_sources,
        secrets_cache: RunSecretsCache::new(),
        redactor: SecretRedactor::empty(),
        internal_secrets_for_redaction: Vec::new(),
    };

    // Start-of-run AST secret preflight (§9): resolve all required workflow secrets in one batch
    // before step 1 begins.
    let preflight_result = preflight_workflow_secrets(&mut state);
    let result = match preflight_result {
        Ok(()) => execute_node_list(&mut state, &request.workflow.nodes, &mut template_ctx),
        Err(err) => Err(err),
    };
    let steps_executed = state.next_step_index;
    let total_tokens = state.total_tokens;
    let total_cost_cents = state.total_cost_cents;

    let total_duration_ms = started_at.elapsed().as_millis() as u64;
    let (status, summary, error) = match result {
        Ok(()) => (
            RunExecutionState::Succeeded,
            format!(
                "Completed '{}' ({} step{})",
                request.workflow.name,
                steps_executed,
                if steps_executed == 1 { "" } else { "s" }
            ),
            None,
        ),
        Err(RunnerError::Stopped) => (
            RunExecutionState::Stopped,
            format!("Stopped '{}' by operator", request.workflow.name),
            None,
        ),
        Err(err) => {
            let msg = state.redactor.redact(&err.to_string());
            (
                RunExecutionState::Failed,
                format!("Failed '{}': {msg}", request.workflow.name),
                Some(msg),
            )
        }
    };

    let _ = tx.send(RunnerEvent::RunFinished {
        status,
        total_duration_ms,
        total_tokens,
        total_cost_cents,
        summary: summary.clone(),
        error: error.clone(),
    });

    RunOutcomeSummary {
        run_id: request.run_id,
        status,
        branch_name,
        worktree_path: working_dir,
        total_duration_ms,
        total_tokens,
        total_cost_cents,
        summary,
        error,
    }
}

struct ExecutionCursor<'a> {
    request: &'a WorkflowRunRequest,
    control: &'a RunControlHandle,
    tx: &'a mpsc::Sender<RunnerEvent>,
    runtime_root: &'a Path,
    working_dir: &'a Path,
    branch_name: &'a str,
    is_isolated_worktree: bool,
    next_step_index: usize,
    total_tokens: u64,
    total_cost_cents: u64,
    /// The strictest `mcp_sources` of the workflow and the composites the cursor is inside.
    mcp_policy: McpSourcesPolicy,
    /// Per-run secret cache; cleared and zeroized on ApprovalGate suspension.
    secrets_cache: RunSecretsCache,
    /// Structured redactor built from all resolved secrets used in this run.
    redactor: SecretRedactor,
    /// Internal secrets resolved during this run (for redaction across outputs/diffs/errors).
    internal_secrets_for_redaction: Vec<SecretString>,
}

impl ExecutionCursor<'_> {
    fn refresh_redactor(&mut self, resolved: Option<&ResolvedSecrets>) {
        if let Some(res) = resolved {
            self.redactor =
                res.build_redactor_with_internals(self.internal_secrets_for_redaction.iter());
        } else if !self.internal_secrets_for_redaction.is_empty() {
            let pairs: Vec<(&str, &SecretString)> = self
                .internal_secrets_for_redaction
                .iter()
                .map(|s| ("internal", s))
                .collect();
            self.redactor = SecretRedactor::from_secrets(pairs);
        }
    }

    fn resolve_github_auth_for_step(&mut self) -> Result<Option<GitHubAuth>, RunnerError> {
        if let Some(auth) = self.request.github_auth.as_ref() {
            if let GitHubAuth::PersonalToken(tok) = auth
                && !tok.trim().is_empty()
            {
                self.internal_secrets_for_redaction
                    .push(SecretString::from(tok.clone()));
                self.refresh_redactor(None);
            }
            return Ok(Some(auth.clone()));
        }
        match self.request.github_auth_mode {
            IssueSourceAuthMode::GitHubCli => Ok(Some(GitHubAuth::GitHubCli)),
            IssueSourceAuthMode::PersonalToken => {
                let Some(resolver) = self.request.secret_resolver.as_ref() else {
                    return Ok(None);
                };
                let Some(pat_id) = self.request.github_pat_secret_id.as_ref() else {
                    return Err(RunnerError::Secret(
                        "GitHub Personal Access Token is not selected in Project Settings › Issues & intake".to_owned(),
                    ));
                };
                match resolver.resolve_internal(pat_id, self.control.cancellation_token()) {
                    Ok(secret) => {
                        let tok = secret.expose_secret().to_owned();
                        self.internal_secrets_for_redaction.push(secret);
                        self.refresh_redactor(None);
                        Ok(Some(GitHubAuth::PersonalToken(tok)))
                    }
                    Err(failure) => {
                        let problem = failure.to_problem(Some(self.request.project_id));
                        let msg = problem.title();
                        let _ = self.tx.send(RunnerEvent::SecretProblem {
                            step_index: None,
                            step_title: self.request.workflow.name.clone(),
                            problem,
                        });
                        Err(RunnerError::Secret(msg))
                    }
                }
            }
        }
    }
}

/// Start-of-run batched AST preflight (§9).
fn preflight_workflow_secrets(cursor: &mut ExecutionCursor<'_>) -> Result<(), RunnerError> {
    let Some(resolver) = cursor.request.secret_resolver.as_ref() else {
        return Ok(());
    };
    let req = collect_workflow_secret_requirements(
        &cursor.request.workflow,
        &cursor.request.catalog,
        &cursor.request.mcp,
        cursor.request.github_auth_mode,
    );
    let resolved = resolver.resolve_for_execution(
        cursor.request.project_id,
        &req.mcp_referenced_vars,
        req.needs_agent_and_steps,
        &mut cursor.secrets_cache,
        cursor.control.cancellation_token(),
    );

    if !resolved.failures.is_empty() {
        let mut msgs = Vec::new();
        for failure in resolved.failures.values() {
            let problem = failure.to_problem(Some(cursor.request.project_id));
            msgs.push(problem.title());
            let _ = cursor.tx.send(RunnerEvent::SecretProblem {
                step_index: None,
                step_title: cursor.request.workflow.name.clone(),
                problem,
            });
        }
        return Err(RunnerError::Secret(format!(
            "workflow '{}' cannot start: {}",
            cursor.request.workflow.name,
            msgs.join("; ")
        )));
    }

    // Preflight GitHub PAT if any GitHub step exists under PersonalToken auth mode.
    if req.needs_github_pat
        && cursor.request.github_auth.is_none()
        && let Some(pat_id) = cursor.request.github_pat_secret_id.as_ref()
    {
        match resolver.resolve_internal(pat_id, cursor.control.cancellation_token()) {
            Ok(secret) => {
                cursor.internal_secrets_for_redaction.push(secret);
            }
            Err(failure) => {
                let problem = failure.to_problem(Some(cursor.request.project_id));
                let msg = problem.title();
                let _ = cursor.tx.send(RunnerEvent::SecretProblem {
                    step_index: None,
                    step_title: cursor.request.workflow.name.clone(),
                    problem,
                });
                return Err(RunnerError::Secret(format!(
                    "workflow '{}' cannot start: {msg}",
                    cursor.request.workflow.name
                )));
            }
        }
    }

    cursor.refresh_redactor(Some(&resolved));
    Ok(())
}

/// Resolves provider spawn-settings `secret_env_vars` (`Internal { purpose: ProviderSpawnEnv }`).
fn resolve_provider_spawn_env(
    cursor: &mut ExecutionCursor<'_>,
    provider: ProviderKind,
    step_index: usize,
    step_name: &str,
) -> Result<BTreeMap<String, String>, RunnerError> {
    let Some(resolver) = cursor.request.secret_resolver.as_ref() else {
        return Ok(BTreeMap::new());
    };
    let bindings = match provider {
        ProviderKind::Claude => &cursor.request.claude_settings.secret_env_vars,
        ProviderKind::Codex => &cursor.request.codex_settings.secret_env_vars,
        _ => return Ok(BTreeMap::new()),
    };
    let mut out = BTreeMap::new();
    for (env_name, secret_id) in bindings {
        match resolver.resolve_internal(secret_id, cursor.control.cancellation_token()) {
            Ok(secret) => {
                out.insert(env_name.clone(), secret.expose_secret().to_owned());
                cursor.internal_secrets_for_redaction.push(secret);
            }
            Err(failure) => {
                let problem = failure.to_problem(Some(cursor.request.project_id));
                let msg = problem.title();
                report_secret_problem(cursor, step_index, step_name, problem);
                return Err(RunnerError::Secret(format!(
                    "step '{step_name}' failed to resolve provider setting '{env_name}': {msg}"
                )));
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Default)]
struct StepOutputCapture {
    exit_code: i32,
    stdout: String,
    stderr: String,
    output: String,
}

fn execute_node_list(
    cursor: &mut ExecutionCursor<'_>,
    nodes: &[WorkflowNodeDef],
    ctx: &mut TemplateContext,
) -> Result<(), RunnerError> {
    for node in nodes {
        if cursor.control.is_stopped() {
            return Err(RunnerError::Stopped);
        }
        execute_node(cursor, node, ctx, None, None)?;
    }
    Ok(())
}

fn execute_node(
    cursor: &mut ExecutionCursor<'_>,
    node: &WorkflowNodeDef,
    ctx: &mut TemplateContext,
    attempt: Option<u32>,
    max_attempts: Option<u32>,
) -> Result<StepOutputCapture, RunnerError> {
    if cursor.control.is_stopped() {
        return Err(RunnerError::Stopped);
    }

    match node {
        WorkflowNodeDef::AgentTask(step) => {
            execute_agent_step(cursor, step, ctx, attempt, max_attempts)
        }
        WorkflowNodeDef::ShellCommand(step) => {
            execute_shell_step(cursor, step, ctx, attempt, max_attempts)
        }
        WorkflowNodeDef::ApprovalGate(step) => {
            execute_approval_gate(cursor, &step.id, &step.name, &step.prompt, ctx)
        }
        WorkflowNodeDef::Composite(step) => execute_composite(cursor, step, ctx),
        WorkflowNodeDef::Loop(lp) => execute_loop(cursor, lp, ctx),
        WorkflowNodeDef::CommentOnIssue(step) => execute_comment_on_issue(cursor, step, ctx),
        WorkflowNodeDef::SetIssueStatus(step) => execute_set_issue_status(cursor, step),
        WorkflowNodeDef::OpenPullRequest(step) => execute_open_pr(cursor, step, ctx),
        WorkflowNodeDef::Condition(step) => {
            let rendered = ctx.render(&step.expression);
            record_step_outputs(ctx, &step.id, 0, &rendered, "", &rendered);
            Ok(StepOutputCapture {
                output: rendered,
                ..Default::default()
            })
        }
    }
}

fn execute_agent_step(
    cursor: &mut ExecutionCursor<'_>,
    step: &AgentTaskStepDef,
    ctx: &mut TemplateContext,
    attempt: Option<u32>,
    max_attempts: Option<u32>,
) -> Result<StepOutputCapture, RunnerError> {
    let step_index = cursor.next_step_index;
    cursor.next_step_index += 1;
    let started = Instant::now();

    let mut effective_step = step.clone();
    let steering = cursor.control.drain_steering();
    if !steering.is_empty() {
        effective_step.prompt.push_str("\n\nOperator steering:\n");
        for note in &steering {
            effective_step.prompt.push_str("- ");
            effective_step.prompt.push_str(note);
            effective_step.prompt.push('\n');
        }
    }
    if let Some(att) = attempt
        && att > 1
        && !ctx.loop_previous.is_empty()
    {
        effective_step
            .prompt
            .push_str("\n\nPrevious loop attempt outputs:\n");
        for (k, v) in &ctx.loop_previous {
            let snippet = truncate_for_prompt(v, 2000);
            effective_step
                .prompt
                .push_str(&format!("- {k}: {snippet}\n"));
        }
    }

    // Collect MCP `${VAR}` names referenced by this step so `SecretResolver` injects ONLY
    // this step's referenced `McpOnly` secrets plus active `AgentAndSteps` secrets.
    let resolved_secrets_opt = if let Some(resolver) = cursor.request.secret_resolver.as_ref() {
        let resolved_agent = effective_step
            .agent
            .as_deref()
            .and_then(|a| cursor.request.catalog.resolve_agent(a).ok());
        let step_policy = cursor.mcp_policy.stricter(
            resolved_agent
                .as_ref()
                .map(|a| a.mcp_sources)
                .unwrap_or_default(),
        );
        let mut allowlist: Vec<String> = resolved_agent
            .as_ref()
            .map(|a| a.mcp.clone())
            .unwrap_or_default();
        for name in effective_step
            .mcp
            .iter()
            .map(|n| n.trim())
            .filter(|n| !n.is_empty())
        {
            if !allowlist.iter().any(|existing| existing == name) {
                allowlist.push(name.to_owned());
            }
        }
        let preview_mcp = resolve_mcp_servers(&cursor.request.mcp, step_policy, &allowlist);
        let mut referenced_vars = BTreeSet::new();
        for srv in &preview_mcp.servers {
            for var in srv.def.env_references() {
                referenced_vars.insert(var);
            }
        }
        let res = resolver.resolve_for_execution(
            cursor.request.project_id,
            &referenced_vars,
            true,
            &mut cursor.secrets_cache,
            cursor.control.cancellation_token(),
        );
        cursor.refresh_redactor(Some(&res));
        Some(res)
    } else {
        None
    };

    let bundle = compile_step_context(
        &cursor.request.catalog,
        &effective_step,
        ctx,
        cursor.request.default_provider,
        cursor.request.default_sandbox,
        cursor.request.model_override.as_deref(),
        StepMcpScope {
            sources: &cursor.request.mcp,
            policy: cursor.mcp_policy,
            env: &cursor.request.project_env,
            resolved_secrets: resolved_secrets_opt.as_ref(),
            project_id: Some(cursor.request.project_id),
        },
    )?;
    let step_runtime_dir =
        cursor
            .runtime_root
            .join(format!("step-{:03}-{}", step_index + 1, step.id));
    let materialized = materialize_step_context(
        bundle,
        &step_runtime_dir,
        cursor.request.source_codex_home.as_deref(),
    )?;
    // Unconditionally clean up any fallback ephemeral 0600 Codex auth copies when this step exits.
    let _codex_auth_guard = EphemeralCodexAuthGuard::for_context(&materialized);

    let provider = materialized.bundle.provider;
    let actor_badge = match materialized.bundle.agent.as_ref() {
        Some(agent) => format!("{} · {}", provider.label(), agent.id),
        None => provider.label().to_owned(),
    };
    let agent_info = StepAgentInfo {
        agent_name: materialized
            .bundle
            .agent
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| provider.label().to_owned()),
        agent_scope: materialized
            .bundle
            .agent
            .as_ref()
            .map(|a| a.origin.badge_label()),
        provider: provider.label().to_owned(),
        model: materialized
            .bundle
            .model
            .clone()
            .unwrap_or_else(|| "default".to_owned()),
        sandbox: materialized.bundle.sandbox.label().to_owned(),
        config: {
            let base = if materialized.bundle.included_skill_ids.is_empty() {
                "Isolated (repo rules allowed)".to_owned()
            } else {
                format!(
                    "Skills: {}",
                    materialized.bundle.included_skill_ids.join(", ")
                )
            };
            format!("{base} · {}", materialized.bundle.mcp.summary())
        },
        approvals: "Isolated".to_owned(),
    };
    let git_info = current_git_info(cursor);

    let _ = cursor.tx.send(RunnerEvent::StepStarted {
        step_index,
        step_id: step.id.clone(),
        title: step.name.clone(),
        actor_badge,
        attempt,
        max_attempts,
        agent_info: Box::new(agent_info),
        git_info: Box::new(git_info),
    });

    for note in steering {
        let _ = cursor.tx.send(RunnerEvent::TranscriptAppended {
            step_index,
            entry: StepTranscriptEntry::UserSteering { message: note },
        });
    }

    // Report any dedicated blocking secret problems (e.g. broken AgentAndSteps or MCP secret).
    let mut secret_blocking: Vec<String> = Vec::new();
    for problem in &materialized.bundle.secret_problems {
        if problem.blocking {
            secret_blocking.push(problem.title());
        }
        report_secret_problem(cursor, step_index, &step.name, problem.clone());
    }
    if let Some(resolved) = resolved_secrets_opt.as_ref() {
        for failure in resolved.failures.values() {
            let problem = failure.to_problem(Some(cursor.request.project_id));
            if !materialized
                .bundle
                .secret_problems
                .iter()
                .any(|p| p.dedupe_key() == problem.dedupe_key())
            {
                secret_blocking.push(problem.title());
                report_secret_problem(cursor, step_index, &step.name, problem);
            }
        }
    }
    if !secret_blocking.is_empty() {
        let summary = secret_blocking.join("; ");
        let _ = cursor.tx.send(RunnerEvent::StepFinished {
            step_index,
            status: RunStepStatus::Failed,
            duration_ms: started.elapsed().as_millis() as u64,
            output_preview: truncate_for_prompt(&summary, 240),
            tokens_used: 0,
            cost_cents: 0,
        });
        return Err(RunnerError::Secret(format!(
            "step '{}' cannot run due to secret failure: {summary}",
            step.name
        )));
    }

    for problem in &materialized.bundle.mcp.problems {
        report_mcp_problem(cursor, step_index, &step.name, problem.clone());
    }
    if let Some(summary) = materialized.bundle.mcp.blocking_summary() {
        let _ = cursor.tx.send(RunnerEvent::StepFinished {
            step_index,
            status: RunStepStatus::Failed,
            duration_ms: started.elapsed().as_millis() as u64,
            output_preview: truncate_for_prompt(&summary, 240),
            tokens_used: 0,
            cost_cents: 0,
        });
        return Err(RunnerError::Mcp(format!(
            "step '{}' can't run without its MCP servers: {summary}",
            step.name
        )));
    }

    let provider_spawn_env = resolve_provider_spawn_env(cursor, provider, step_index, &step.name)?;
    cursor.refresh_redactor(resolved_secrets_opt.as_ref());

    let mut cmd = match provider {
        ProviderKind::Claude => {
            let bin = cursor
                .request
                .claude_binary
                .as_ref()
                .ok_or(RunnerError::ProviderBinaryMissing(ProviderKind::Claude))?;
            let mut plan = build_isolated_claude_spawn_plan(
                bin,
                cursor.request.claude_default_model.as_deref(),
                &cursor.request.claude_settings,
                cursor.working_dir,
                &materialized,
                Some(&format!("{}-{}", cursor.request.run_id, step.id)),
                &cursor.request.project_env,
            )?;
            if let Some(resolved) = resolved_secrets_opt.as_ref() {
                let delivery =
                    StepSecretsDelivery::from_resolved(resolved, provider_spawn_env, true);
                apply_claude_secrets_delivery(&mut plan, &delivery);
            }
            plan.to_command()
        }
        ProviderKind::Codex => {
            let bin = cursor
                .request
                .codex_binary
                .as_ref()
                .ok_or(RunnerError::ProviderBinaryMissing(ProviderKind::Codex))?;
            let mut plan = build_isolated_codex_spawn_plan(
                bin,
                cursor.request.codex_default_model.as_deref(),
                &cursor.request.codex_settings,
                cursor.working_dir,
                &materialized,
                &cursor.request.project_env,
            )?;
            if let Some(resolved) = resolved_secrets_opt.as_ref() {
                let delivery =
                    StepSecretsDelivery::from_resolved(resolved, provider_spawn_env, true);
                apply_codex_secrets_delivery(
                    &mut plan,
                    &delivery,
                    &cursor.request.codex_settings,
                    &materialized.bundle.codex_mcp,
                );
            }
            plan.to_command()
        }
        ProviderKind::Antigravity => {
            let bin = cursor.request.antigravity_binary.as_ref().ok_or(
                RunnerError::ProviderBinaryMissing(ProviderKind::Antigravity),
            )?;
            let mut plan = build_isolated_antigravity_spawn_plan(
                bin,
                cursor.request.antigravity_default_model.as_deref(),
                cursor.working_dir,
                &materialized,
                &cursor.request.project_env,
            );
            if let Some(resolved) = resolved_secrets_opt.as_ref() {
                // Antigravity NEVER receives McpOnly secrets (§7).
                let delivery = StepSecretsDelivery::from_resolved(resolved, BTreeMap::new(), false);
                apply_antigravity_secrets_delivery(&mut plan, &delivery);
            }
            // `--print=<prompt>` keeps a prompt that starts with `-` from reading as a flag.
            let prompt = plan.first_message(&materialized.bundle.rendered_prompt);
            plan.args.push(format!("--print={prompt}"));
            plan.to_command()
        }
        ProviderKind::OpenRouter => {
            return Err(RunnerError::ProviderBinaryMissing(ProviderKind::OpenRouter));
        }
    };

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|err| RunnerError::Process(format!("failed to spawn {:?}: {err}", provider)))?;
    cursor.control.set_active_pid(child.id());

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let mut mcp_failures = Vec::new();
        if let Some(pipe) = stderr_pipe {
            let reader = BufReader::new(pipe);
            for line in reader.lines().map_while(Result::ok) {
                if let Some(failure) = codex_stderr_failure(&line) {
                    mcp_failures.push(failure);
                    continue;
                }
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(&line);
            }
        }
        (buf, mcp_failures)
    });
    let mut mcp_tracker = McpFailureTracker::default();
    let mut mcp_blocking: Vec<String> = Vec::new();

    let mut assistant_chunks = Vec::new();
    let mut raw_stdout = String::new();
    let mut tokens_used = 0u64;
    let mut cost_cents = 0u64;
    let badge = provider.label().to_owned();
    let attempt_banner = attempt.filter(|&a| a > 1).map(|a| AttemptBanner {
        summary: format!("Attempt {a}"),
        details: "Re-running with previous attempt outputs".to_owned(),
    });
    let mut emitted_banner = false;

    if let Some(pipe) = stdout_pipe {
        let reader = BufReader::new(pipe);
        for line in reader.lines().map_while(Result::ok) {
            if !raw_stdout.is_empty() {
                raw_stdout.push('\n');
            }
            raw_stdout.push_str(&line);

            if provider == ProviderKind::Claude
                && line.contains("\"mcp_servers\"")
                && let Ok(event) = serde_json::from_str::<serde_json::Value>(&line)
            {
                for failure in claude_init_failures(&event) {
                    if let Some(problem) = mcp_tracker.record(&materialized.bundle.mcp, &failure) {
                        if problem.blocking {
                            mcp_blocking.push(problem.title());
                        }
                        report_mcp_problem(cursor, step_index, &step.name, problem);
                    }
                }
                if !mcp_blocking.is_empty() {
                    // A server the step needs didn't start: stop before the model runs.
                    let _ = child.kill();
                    break;
                }
            }

            let parsed = match provider {
                ProviderKind::Claude => parse_claude_stream_json_line(&line),
                ProviderKind::Codex => parse_codex_json_line(&line),
                ProviderKind::Antigravity => parse_antigravity_json_line(&line),
                ProviderKind::OpenRouter => None,
            };

            match parsed {
                Some(ParsedStreamEvent::AgentText(text)) => {
                    let text = cursor.redactor.redact(&text);
                    assistant_chunks.push(text.clone());
                    let banner = if !emitted_banner {
                        emitted_banner = true;
                        attempt_banner.clone()
                    } else {
                        None
                    };
                    let _ = cursor.tx.send(RunnerEvent::TranscriptAppended {
                        step_index,
                        entry: StepTranscriptEntry::AgentTurn {
                            badge: badge.clone(),
                            attempt_banner: banner,
                            body: text,
                        },
                    });
                }
                Some(ParsedStreamEvent::ToolCall {
                    verb,
                    target,
                    status_text,
                    tone,
                }) => {
                    let _ = cursor.tx.send(RunnerEvent::TranscriptAppended {
                        step_index,
                        entry: StepTranscriptEntry::ToolCall {
                            verb: cursor.redactor.redact(&verb),
                            target: cursor.redactor.redact(&target),
                            status_text: cursor.redactor.redact(&status_text),
                            status_tone: tone,
                            output_lines: Vec::new(),
                            expanded: false,
                        },
                    });
                }
                Some(ParsedStreamEvent::Usage {
                    tokens,
                    cost_cents: c,
                    result_text,
                }) => {
                    tokens_used = tokens_used.saturating_add(tokens);
                    cost_cents = cost_cents.saturating_add(c);
                    if let Some(txt) = result_text
                        && assistant_chunks.is_empty()
                        && !txt.trim().is_empty()
                    {
                        let txt = cursor.redactor.redact(&txt);
                        assistant_chunks.push(txt.clone());
                        let _ = cursor.tx.send(RunnerEvent::TranscriptAppended {
                            step_index,
                            entry: StepTranscriptEntry::AgentTurn {
                                badge: badge.clone(),
                                attempt_banner: None,
                                body: txt,
                            },
                        });
                    }
                }
                None => {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with('{') {
                        let redacted = cursor.redactor.redact(trimmed);
                        assistant_chunks.push(redacted.clone());
                        let _ = cursor.tx.send(RunnerEvent::TranscriptAppended {
                            step_index,
                            entry: StepTranscriptEntry::AgentTurn {
                                badge: badge.clone(),
                                attempt_banner: None,
                                body: redacted,
                            },
                        });
                    }
                }
            }
        }
    }

    let status = wait_child_with_cancel(&mut child, cursor.control)?;
    cursor.control.clear_active_pid();
    let (raw_stderr_text, codex_failures) = stderr_handle.join().unwrap_or_default();
    let stderr_text = cursor.redactor.redact(&raw_stderr_text);
    let duration_ms = started.elapsed().as_millis() as u64;

    let mut late_failures = codex_failures;
    if provider == ProviderKind::Antigravity {
        let log_path = materialized.runtime_dir.join(ANTIGRAVITY_LOG_FILE);
        if let Ok(log) = std::fs::read_to_string(&log_path) {
            late_failures.extend(antigravity_log_failures(&log));
        }
        // Delete antigravity.log immediately after parsing MCP failures (§7).
        let _ = std::fs::remove_file(&log_path);
    }
    for failure in &late_failures {
        if let Some(problem) = mcp_tracker.record(&materialized.bundle.mcp, failure) {
            if problem.blocking {
                mcp_blocking.push(problem.title());
            }
            report_mcp_problem(cursor, step_index, &step.name, problem);
        }
    }
    if mcp_blocking.is_empty() && !cursor.control.is_stopped() {
        let servers = mcp_tracker.healthy(&materialized.bundle.mcp);
        if !servers.is_empty() {
            let _ = cursor.tx.send(RunnerEvent::McpServersReady { servers });
        }
    }
    if !mcp_blocking.is_empty() && !cursor.control.is_stopped() {
        let summary = cursor.redactor.redact(&mcp_blocking.join("; "));
        let _ = cursor.tx.send(RunnerEvent::StepFinished {
            step_index,
            status: RunStepStatus::Failed,
            duration_ms,
            output_preview: truncate_for_prompt(&summary, 240),
            tokens_used,
            cost_cents,
        });
        cursor.total_tokens = cursor.total_tokens.saturating_add(tokens_used);
        cursor.total_cost_cents = cursor.total_cost_cents.saturating_add(cost_cents);
        return Err(RunnerError::Mcp(format!(
            "step '{}' lost MCP servers it needs: {summary}",
            step.name
        )));
    }

    cursor.total_tokens = cursor.total_tokens.saturating_add(tokens_used);
    cursor.total_cost_cents = cursor.total_cost_cents.saturating_add(cost_cents);

    // Scrub raw_stdout before recording into TemplateContext.step_outputs (§7).
    let redacted_raw_stdout = cursor.redactor.redact(&raw_stdout);
    let output = if !assistant_chunks.is_empty() {
        assistant_chunks.join("\n")
    } else {
        redacted_raw_stdout.clone()
    };
    let exit_code = status.code().unwrap_or(-1);
    record_step_outputs(
        ctx,
        &step.id,
        exit_code,
        &redacted_raw_stdout,
        &stderr_text,
        &output,
    );
    emit_live_diffs(cursor);

    let step_status = if cursor.control.is_stopped() {
        RunStepStatus::Stopped
    } else if exit_code == 0 {
        RunStepStatus::Done
    } else {
        RunStepStatus::Failed
    };

    let _ = cursor.tx.send(RunnerEvent::StepFinished {
        step_index,
        status: step_status,
        duration_ms,
        output_preview: truncate_for_prompt(&output, 240),
        tokens_used,
        cost_cents,
    });

    if cursor.control.is_stopped() {
        return Err(RunnerError::Stopped);
    }
    if exit_code != 0 && attempt.is_none() {
        let err_detail = if stderr_text.trim().is_empty() {
            format!("step '{}' exited with code {exit_code}", step.name)
        } else {
            format!(
                "step '{}' failed (exit {exit_code}): {}",
                step.name,
                truncate_for_prompt(&stderr_text, 300)
            )
        };
        return Err(RunnerError::Process(err_detail));
    }

    Ok(StepOutputCapture {
        exit_code,
        stdout: redacted_raw_stdout,
        stderr: stderr_text,
        output,
    })
}

/// Shows a secret problem in the step's transcript and passes it on for notifications.
fn report_secret_problem(
    cursor: &ExecutionCursor<'_>,
    step_index: usize,
    step_title: &str,
    problem: SecretProblem,
) {
    let _ = cursor.tx.send(RunnerEvent::TranscriptAppended {
        step_index,
        entry: StepTranscriptEntry::SecretProblem {
            problem: problem.clone(),
        },
    });
    let _ = cursor.tx.send(RunnerEvent::SecretProblem {
        step_index: Some(step_index),
        step_title: step_title.to_owned(),
        problem,
    });
}

/// Shows an MCP problem in the step's transcript and passes it on for notifications.
fn report_mcp_problem(
    cursor: &ExecutionCursor<'_>,
    step_index: usize,
    step_title: &str,
    problem: McpProblem,
) {
    let _ = cursor.tx.send(RunnerEvent::TranscriptAppended {
        step_index,
        entry: StepTranscriptEntry::McpProblem {
            problem: problem.clone(),
        },
    });
    let _ = cursor.tx.send(RunnerEvent::McpProblem {
        step_index,
        step_title: step_title.to_owned(),
        problem,
    });
}

fn execute_shell_step(
    cursor: &mut ExecutionCursor<'_>,
    step: &ShellCommandStepDef,
    ctx: &mut TemplateContext,
    attempt: Option<u32>,
    max_attempts: Option<u32>,
) -> Result<StepOutputCapture, RunnerError> {
    let step_index = cursor.next_step_index;
    cursor.next_step_index += 1;
    let started = Instant::now();
    let rendered_cmd = ctx.render(&step.command);

    let _ = cursor.tx.send(RunnerEvent::StepStarted {
        step_index,
        step_id: step.id.clone(),
        title: step.name.clone(),
        actor_badge: format!("Shell · {}", truncate_for_prompt(&rendered_cmd, 42)),
        attempt,
        max_attempts,
        agent_info: Box::new(StepAgentInfo {
            agent_name: "Shell".to_owned(),
            agent_scope: None,
            provider: "Local shell".to_owned(),
            model: "sh".to_owned(),
            sandbox: cursor.request.default_sandbox.label().to_owned(),
            config: "Isolated env".to_owned(),
            approvals: "Auto".to_owned(),
        }),
        git_info: Box::new(current_git_info(cursor)),
    });

    // Shell steps receive plain project vars + `AgentAndSteps` secrets ONLY, and explicitly strip
    // all `McpOnly`, excluded, unshared, `Internal`, `GH_TOKEN`, and `OP_*` vars (§7).
    let (shell_env, shell_env_remove) =
        if let Some(resolver) = cursor.request.secret_resolver.as_ref() {
            let resolved = resolver.resolve_for_execution(
                cursor.request.project_id,
                &BTreeSet::new(),
                true,
                &mut cursor.secrets_cache,
                cursor.control.cancellation_token(),
            );
            if !resolved.failures.is_empty() {
                let mut msgs = Vec::new();
                for failure in resolved.failures.values() {
                    let problem = failure.to_problem(Some(cursor.request.project_id));
                    msgs.push(problem.title());
                    report_secret_problem(cursor, step_index, &step.name, problem);
                }
                let summary = msgs.join("; ");
                let _ = cursor.tx.send(RunnerEvent::StepFinished {
                    step_index,
                    status: RunStepStatus::Failed,
                    duration_ms: started.elapsed().as_millis() as u64,
                    output_preview: truncate_for_prompt(&summary, 240),
                    tokens_used: 0,
                    cost_cents: 0,
                });
                return Err(RunnerError::Secret(format!(
                    "shell step '{}' cannot run due to secret failure: {summary}",
                    step.name
                )));
            }
            cursor.refresh_redactor(Some(&resolved));
            let mut strip = resolved.strip_from_child_env.clone();
            strip.extend(resolved.mcp_only_secret_names());
            (resolved.agent_and_steps_env(), strip)
        } else {
            (cursor.request.project_env.clone(), BTreeSet::new())
        };

    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(&rendered_cmd)
        .current_dir(cursor.working_dir);
    for key in &shell_env_remove {
        cmd.env_remove(key);
    }
    cmd.envs(&shell_env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|err| RunnerError::Process(format!("failed to run shell command: {err}")))?;
    cursor.control.set_active_pid(child.id());

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stderr_handle = std::thread::spawn(move || {
        let mut lines = Vec::new();
        if let Some(pipe) = stderr_pipe {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                lines.push(line);
            }
        }
        lines
    });

    let mut stdout_lines = Vec::new();
    if let Some(pipe) = stdout_pipe {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            stdout_lines.push(line);
        }
    }

    let status = wait_child_with_cancel(&mut child, cursor.control)?;
    cursor.control.clear_active_pid();
    let stderr_lines = stderr_handle.join().unwrap_or_default();
    let duration_ms = started.elapsed().as_millis() as u64;

    let exit_code = status.code().unwrap_or(-1);
    let redacted_stdout_lines: Vec<String> = stdout_lines
        .iter()
        .map(|l| cursor.redactor.redact(l))
        .collect();
    let redacted_stderr_lines: Vec<String> = stderr_lines
        .iter()
        .map(|l| cursor.redactor.redact(l))
        .collect();
    let stdout_str = redacted_stdout_lines.join("\n");
    let stderr_str = redacted_stderr_lines.join("\n");

    let mut terminal_lines: Vec<TerminalLine> = redacted_stdout_lines
        .iter()
        .take(80)
        .map(|line| TerminalLine {
            badge: None,
            badge_tone: TerminalBadgeTone::Info,
            text: line.clone(),
            muted_suffix: None,
        })
        .collect();
    for err_line in redacted_stderr_lines.iter().take(40) {
        terminal_lines.push(TerminalLine {
            badge: Some("ERR".to_owned()),
            badge_tone: TerminalBadgeTone::Fail,
            text: err_line.clone(),
            muted_suffix: None,
        });
    }

    let _ = cursor.tx.send(RunnerEvent::TranscriptAppended {
        step_index,
        entry: StepTranscriptEntry::ToolCall {
            verb: "Run".to_owned(),
            target: cursor.redactor.redact(&rendered_cmd),
            status_text: if exit_code == 0 {
                "exit 0".to_owned()
            } else {
                format!("exit {exit_code}")
            },
            status_tone: if exit_code == 0 {
                ToolStatusTone::Success
            } else {
                ToolStatusTone::Error
            },
            output_lines: terminal_lines,
            expanded: exit_code != 0,
        },
    });

    let combined = if stderr_str.is_empty() {
        stdout_str.clone()
    } else if stdout_str.is_empty() {
        stderr_str.clone()
    } else {
        format!("{stdout_str}\n{stderr_str}")
    };
    record_step_outputs(
        ctx,
        &step.id,
        exit_code,
        &stdout_str,
        &stderr_str,
        &combined,
    );
    emit_live_diffs(cursor);

    let step_status = if cursor.control.is_stopped() {
        RunStepStatus::Stopped
    } else if exit_code == 0 {
        RunStepStatus::Done
    } else {
        RunStepStatus::Failed
    };

    let _ = cursor.tx.send(RunnerEvent::StepFinished {
        step_index,
        status: step_status,
        duration_ms,
        output_preview: truncate_for_prompt(&combined, 240),
        tokens_used: 0,
        cost_cents: 0,
    });

    if cursor.control.is_stopped() {
        return Err(RunnerError::Stopped);
    }
    // Inside a loop (`attempt.is_some()`), non-zero exit codes are evaluated by loop exit conditions!
    if exit_code != 0 && attempt.is_none() {
        return Err(RunnerError::Process(format!(
            "shell step '{}' exited with status {exit_code}",
            step.name
        )));
    }

    Ok(StepOutputCapture {
        exit_code,
        stdout: stdout_str,
        stderr: stderr_str,
        output: combined,
    })
}

fn execute_approval_gate(
    cursor: &mut ExecutionCursor<'_>,
    step_id: &str,
    step_name: &str,
    prompt_template: &str,
    ctx: &mut TemplateContext,
) -> Result<StepOutputCapture, RunnerError> {
    let step_index = cursor.next_step_index;
    cursor.next_step_index += 1;
    let started = Instant::now();
    let rendered_prompt = ctx.render(prompt_template);

    let _ = cursor.tx.send(RunnerEvent::StepStarted {
        step_index,
        step_id: step_id.to_owned(),
        title: step_name.to_owned(),
        actor_badge: "You".to_owned(),
        attempt: None,
        max_attempts: None,
        agent_info: Box::new(StepAgentInfo {
            agent_name: "Human approval".to_owned(),
            agent_scope: None,
            provider: "Operator".to_owned(),
            model: "Manual gate".to_owned(),
            sandbox: "—".to_owned(),
            config: "Inbox".to_owned(),
            approvals: "Required".to_owned(),
        }),
        git_info: Box::new(current_git_info(cursor)),
    });

    let mut diffs = collect_git_diffs(cursor.working_dir);
    cursor.redactor.redact_diffs(&mut diffs);
    let diff_summary: Vec<ReviewFileStat> = diffs
        .iter()
        .map(|d| ReviewFileStat {
            path: d.path.clone(),
            additions: d.additions as u32,
            deletions: d.deletions as u32,
        })
        .collect();
    let _ = cursor.tx.send(RunnerEvent::DiffUpdated { diffs });

    let gate_id = format!("gate-{}-{step_index}", cursor.request.run_id);
    let _ = cursor.tx.send(RunnerEvent::ApprovalRequested {
        gate_id: gate_id.clone(),
        step_index,
        step_title: step_name.to_owned(),
        prompt: cursor.redactor.redact(&rendered_prompt),
        branch_name: cursor.branch_name.to_owned(),
        base_branch: cursor.request.default_branch.clone(),
        diff_summary,
    });

    // Clear and zeroize RunSecrets immediately before suspending at an ApprovalGate (§4)
    // so decrypted credentials never sit in RAM while waiting for human review.
    cursor.secrets_cache.clear_and_zeroize();

    let decision = cursor.control.wait_for_gate(&gate_id);
    let duration_ms = started.elapsed().as_millis() as u64;

    match decision {
        GateDecision::Approve { note } => {
            let msg = match note.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(n) => format!("Approved: {n}"),
                None => "Approved by operator.".to_owned(),
            };
            let _ = cursor.tx.send(RunnerEvent::TranscriptAppended {
                step_index,
                entry: StepTranscriptEntry::UserSteering {
                    message: msg.clone(),
                },
            });
            record_step_outputs(ctx, step_id, 0, &msg, "", &msg);
            let _ = cursor.tx.send(RunnerEvent::StepFinished {
                step_index,
                status: RunStepStatus::Done,
                duration_ms,
                output_preview: msg.clone(),
                tokens_used: 0,
                cost_cents: 0,
            });
            Ok(StepOutputCapture {
                exit_code: 0,
                output: msg,
                ..Default::default()
            })
        }
        GateDecision::RequestChanges { note } => {
            let reason = note
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("Changes requested at review gate");
            let _ = cursor.tx.send(RunnerEvent::StepFinished {
                step_index,
                status: RunStepStatus::Failed,
                duration_ms,
                output_preview: reason.to_owned(),
                tokens_used: 0,
                cost_cents: 0,
            });
            Err(RunnerError::Process(reason.to_owned()))
        }
        GateDecision::Stop => {
            let _ = cursor.tx.send(RunnerEvent::StepFinished {
                step_index,
                status: RunStepStatus::Stopped,
                duration_ms,
                output_preview: "Stopped at approval gate".to_owned(),
                tokens_used: 0,
                cost_cents: 0,
            });
            Err(RunnerError::Stopped)
        }
    }
}

fn execute_composite(
    cursor: &mut ExecutionCursor<'_>,
    step: &CompositeStepDef,
    parent_ctx: &mut TemplateContext,
) -> Result<StepOutputCapture, RunnerError> {
    let comp = cursor
        .request
        .catalog
        .resolve_composite(&step.uses)
        .cloned()
        .ok_or_else(|| RunnerError::CompositeNotFound(step.uses.clone()))?;

    let mut child_ctx = TemplateContext {
        inputs: BTreeMap::new(),
        step_outputs: parent_ctx.step_outputs.clone(),
        loop_attempt: None,
        loop_previous: BTreeMap::new(),
    };
    for (k, param) in &comp.inputs {
        if let Some(default_val) = param.default.as_ref() {
            child_ctx.inputs.insert(k.clone(), default_val.clone());
        }
    }
    for (k, expr) in &step.with {
        let rendered = parent_ctx.render(expr);
        child_ctx.inputs.insert(k.clone(), rendered);
    }

    let outer_policy = cursor.mcp_policy;
    cursor.mcp_policy = outer_policy.stricter(comp.mcp_sources);
    let result = execute_node_list(cursor, &comp.nodes, &mut child_ctx);
    cursor.mcp_policy = outer_policy;
    result?;

    let mut primary_output = String::new();
    for (out_name, expr) in &comp.outputs {
        let rendered = child_ctx.render(expr);
        parent_ctx
            .step_outputs
            .insert(format!("{}.{out_name}", step.id), rendered.clone());
        if primary_output.is_empty() {
            primary_output = rendered;
        }
    }
    parent_ctx
        .step_outputs
        .insert(format!("{}.output", step.id), primary_output.clone());

    Ok(StepOutputCapture {
        output: primary_output,
        ..Default::default()
    })
}

fn execute_loop(
    cursor: &mut ExecutionCursor<'_>,
    lp: &LoopNodeDef,
    ctx: &mut TemplateContext,
) -> Result<StepOutputCapture, RunnerError> {
    let max_attempts = lp.max_attempts.max(1);
    let mut last_capture = StepOutputCapture::default();

    for attempt in 1..=max_attempts {
        if cursor.control.is_stopped() {
            return Err(RunnerError::Stopped);
        }

        if attempt > 1 && lp.discard_file_changes && cursor.working_dir.join(".git").exists() {
            let _ = Command::new("git")
                .args(["checkout", "--", "."])
                .current_dir(cursor.working_dir)
                .output();
        }

        ctx.loop_attempt = Some(attempt);
        let mut iter_outputs = BTreeMap::new();
        for child in &lp.steps {
            let cap = execute_node(cursor, child, ctx, Some(attempt), Some(max_attempts))?;
            iter_outputs.insert(
                format!("{}.exit_code", child.id()),
                cap.exit_code.to_string(),
            );
            iter_outputs.insert(format!("{}.stdout", child.id()), cap.stdout.clone());
            iter_outputs.insert(format!("{}.stderr", child.id()), cap.stderr.clone());
            iter_outputs.insert(format!("{}.output", child.id()), cap.output.clone());
            iter_outputs.insert(
                format!("{}.status", child.id()),
                if cap.exit_code == 0 {
                    "ok".to_owned()
                } else {
                    "failed".to_owned()
                },
            );
            last_capture = cap;
        }

        let satisfied = match lp.repeat {
            LoopRepeatMode::FixedCount => attempt == max_attempts,
            LoopRepeatMode::UntilCheckPasses | LoopRepeatMode::ForEachItem => {
                if lp.exit_conditions.is_empty() {
                    last_capture.exit_code == 0
                } else {
                    lp.exit_conditions
                        .iter()
                        .all(|cond| evaluate_exit_condition(cond, &iter_outputs))
                }
            }
        };

        if satisfied {
            ctx.loop_attempt = None;
            ctx.loop_previous.clear();
            return Ok(last_capture);
        }

        if lp.pass_previous_outputs {
            ctx.loop_previous = iter_outputs;
        }
    }

    ctx.loop_attempt = None;
    match lp.on_exhausted {
        LoopExhaustedAction::GoToFallbackStep => {
            if let Some(fallback) = lp.fallback.as_deref() {
                let _ = execute_node(cursor, fallback, ctx, None, None);
            }
            Err(RunnerError::Process(format!(
                "loop '{}' exhausted {max_attempts} attempts",
                lp.name
            )))
        }
        LoopExhaustedAction::FailRun => Err(RunnerError::Process(format!(
            "loop '{}' exhausted {max_attempts} attempts",
            lp.name
        ))),
        LoopExhaustedAction::AskApproval => execute_approval_gate(
            cursor,
            &format!("{}_exhausted", lp.id),
            &format!("{} exhausted", lp.name),
            &format!(
                "Loop '{}' reached {max_attempts} attempts without passing. Approve to continue anyway or stop.",
                lp.name
            ),
            ctx,
        ),
        LoopExhaustedAction::ContinueNext => Ok(last_capture),
    }
}

fn execute_comment_on_issue(
    cursor: &mut ExecutionCursor<'_>,
    step: &CommentOnIssueStepDef,
    ctx: &mut TemplateContext,
) -> Result<StepOutputCapture, RunnerError> {
    let step_index = cursor.next_step_index;
    cursor.next_step_index += 1;
    let started = Instant::now();
    let body = cursor.redactor.redact(&ctx.render(&step.body));

    let _ = cursor.tx.send(RunnerEvent::StepStarted {
        step_index,
        step_id: step.id.clone(),
        title: step.name.clone(),
        actor_badge: "Issue source".to_owned(),
        attempt: None,
        max_attempts: None,
        agent_info: Box::new(StepAgentInfo {
            agent_name: "GitHub Issues".to_owned(),
            agent_scope: None,
            provider: "gh CLI".to_owned(),
            model: "—".to_owned(),
            sandbox: "—".to_owned(),
            config: "Issue comment".to_owned(),
            approvals: "Auto".to_owned(),
        }),
        git_info: Box::new(current_git_info(cursor)),
    });

    let resolved_auth = cursor.resolve_github_auth_for_step()?;
    let outcome = if let (Some(repo), Some(issue_no), Some(auth)) = (
        cursor.request.github_repo.as_deref(),
        cursor.request.issue_number,
        resolved_auth.as_ref(),
    ) {
        comment_on_github_issue(repo, issue_no, &body, auth, None)
            .map(|()| format!("Posted comment on {repo}#{issue_no}"))
            .map_err(|err| RunnerError::Process(cursor.redactor.redact(&err.to_string())))
    } else {
        Ok(format!("Comment prepared (no linked GitHub issue): {body}"))
    };

    let duration_ms = started.elapsed().as_millis() as u64;
    match outcome {
        Ok(msg) => {
            record_step_outputs(ctx, &step.id, 0, &msg, "", &msg);
            let _ = cursor.tx.send(RunnerEvent::StepFinished {
                step_index,
                status: RunStepStatus::Done,
                duration_ms,
                output_preview: truncate_for_prompt(&msg, 240),
                tokens_used: 0,
                cost_cents: 0,
            });
            Ok(StepOutputCapture {
                output: msg,
                ..Default::default()
            })
        }
        Err(err) => {
            let _ = cursor.tx.send(RunnerEvent::StepFinished {
                step_index,
                status: RunStepStatus::Failed,
                duration_ms,
                output_preview: err.to_string(),
                tokens_used: 0,
                cost_cents: 0,
            });
            Err(err)
        }
    }
}

fn execute_set_issue_status(
    cursor: &mut ExecutionCursor<'_>,
    step: &SetIssueStatusStepDef,
) -> Result<StepOutputCapture, RunnerError> {
    let step_index = cursor.next_step_index;
    cursor.next_step_index += 1;
    let started = Instant::now();

    let _ = cursor.tx.send(RunnerEvent::StepStarted {
        step_index,
        step_id: step.id.clone(),
        title: step.name.clone(),
        actor_badge: "Issue source".to_owned(),
        attempt: None,
        max_attempts: None,
        agent_info: Box::new(StepAgentInfo {
            agent_name: "GitHub Issues".to_owned(),
            agent_scope: None,
            provider: "gh CLI".to_owned(),
            model: "—".to_owned(),
            sandbox: "—".to_owned(),
            config: "Labels".to_owned(),
            approvals: "Auto".to_owned(),
        }),
        git_info: Box::new(current_git_info(cursor)),
    });

    let resolved_auth = cursor.resolve_github_auth_for_step()?;
    if let (Some(repo), Some(issue_no), Some(auth)) = (
        cursor.request.github_repo.as_deref(),
        cursor.request.issue_number,
        resolved_auth.as_ref(),
    ) {
        for label in &step.add_labels {
            let _ = add_github_issue_label(repo, issue_no, label, auth, None);
        }
        for label in &step.remove_labels {
            let _ = remove_github_issue_label(repo, issue_no, label, auth, None);
        }
    }

    let duration_ms = started.elapsed().as_millis() as u64;
    let _ = cursor.tx.send(RunnerEvent::StepFinished {
        step_index,
        status: RunStepStatus::Done,
        duration_ms,
        output_preview: "Updated issue labels".to_owned(),
        tokens_used: 0,
        cost_cents: 0,
    });
    Ok(StepOutputCapture::default())
}

fn execute_open_pr(
    cursor: &mut ExecutionCursor<'_>,
    step: &OpenPullRequestStepDef,
    ctx: &mut TemplateContext,
) -> Result<StepOutputCapture, RunnerError> {
    let step_index = cursor.next_step_index;
    cursor.next_step_index += 1;
    let started = Instant::now();

    let title = cursor.redactor.redact(&ctx.render(&step.title));
    let mut body = cursor.redactor.redact(&ctx.render(&step.body));
    if cursor.request.link_pr_closes_issue
        && let Some(issue_no) = cursor.request.issue_number
        && !body.contains(&format!("#{issue_no}"))
    {
        body.push_str(&format!("\n\nCloses #{issue_no}"));
    }

    let _ = cursor.tx.send(RunnerEvent::StepStarted {
        step_index,
        step_id: step.id.clone(),
        title: step.name.clone(),
        actor_badge: "GitHub PR".to_owned(),
        attempt: None,
        max_attempts: None,
        agent_info: Box::new(StepAgentInfo {
            agent_name: "Pull Request".to_owned(),
            agent_scope: None,
            provider: "gh CLI".to_owned(),
            model: "—".to_owned(),
            sandbox: "—".to_owned(),
            config: "gh pr create".to_owned(),
            approvals: "Auto".to_owned(),
        }),
        git_info: Box::new(current_git_info(cursor)),
    });

    let resolved_auth = cursor.resolve_github_auth_for_step()?;
    let pr_result = match resolve_gh_binary(None) {
        Ok(gh_bin) => {
            let mut cmd = Command::new(gh_bin);
            cmd.current_dir(cursor.working_dir)
                .args(["pr", "create", "--title", &title, "--body", &body]);
            if step.draft.unwrap_or(cursor.request.open_prs_as_draft) {
                cmd.arg("--draft");
            }
            if let Some(GitHubAuth::PersonalToken(tok)) = resolved_auth.as_ref() {
                cmd.env("GH_TOKEN", tok);
            }
            cmd.output()
                .map_err(|err| {
                    RunnerError::Process(
                        cursor
                            .redactor
                            .redact(&format!("failed to run gh pr create: {err}")),
                    )
                })
                .and_then(|out| {
                    if out.status.success() {
                        Ok(cursor
                            .redactor
                            .redact(String::from_utf8_lossy(&out.stdout).trim()))
                    } else {
                        Err(RunnerError::Process(
                            cursor
                                .redactor
                                .redact(String::from_utf8_lossy(&out.stderr).trim()),
                        ))
                    }
                })
        }
        Err(err) => Err(RunnerError::Process(
            cursor.redactor.redact(&err.to_string()),
        )),
    };

    let duration_ms = started.elapsed().as_millis() as u64;
    match pr_result {
        Ok(url) => {
            record_step_outputs(ctx, &step.id, 0, &url, "", &url);
            let _ = cursor.tx.send(RunnerEvent::StepFinished {
                step_index,
                status: RunStepStatus::Done,
                duration_ms,
                output_preview: url.clone(),
                tokens_used: 0,
                cost_cents: 0,
            });
            Ok(StepOutputCapture {
                stdout: url.clone(),
                output: url,
                ..Default::default()
            })
        }
        Err(err) => {
            let _ = cursor.tx.send(RunnerEvent::StepFinished {
                step_index,
                status: RunStepStatus::Failed,
                duration_ms,
                output_preview: err.to_string(),
                tokens_used: 0,
                cost_cents: 0,
            });
            Err(err)
        }
    }
}

fn evaluate_exit_condition(
    cond: &crate::definitions::LoopExitConditionDef,
    iter_outputs: &BTreeMap<String, String>,
) -> bool {
    let field_key = match cond.field {
        ExitField::ExitCode => "exit_code",
        ExitField::Stdout => "stdout",
        ExitField::Stderr => "stderr",
        ExitField::Status => "status",
    };
    let actual = iter_outputs
        .get(&format!("{}.{field_key}", cond.step))
        .map(String::as_str)
        .unwrap_or("");
    let expected = cond.expected.trim();
    match cond.operator {
        ExitOperator::Equals => actual.trim() == expected,
        ExitOperator::NotEquals => actual.trim() != expected,
        ExitOperator::Contains => actual.contains(expected),
        ExitOperator::LessThan => actual
            .trim()
            .parse::<f64>()
            .ok()
            .zip(expected.parse::<f64>().ok())
            .is_some_and(|(a, b)| a < b),
    }
}

fn prepare_checkout(request: &WorkflowRunRequest, runtime_root: &Path) -> (PathBuf, String, bool) {
    let prefix = if request.branch_prefix.trim().is_empty() {
        "agent/"
    } else {
        request.branch_prefix.trim()
    };
    let branch_name = match request.issue_number {
        Some(issue_no) => format!("{prefix}issue-{issue_no}-run-{}", request.run_number),
        None => format!("{prefix}{}-run-{}", request.workflow.id, request.run_number),
    };

    if request.use_isolated_worktrees && is_git_repo_with_head(&request.project_root) {
        let worktree_dir = runtime_root.join("worktree");
        let output = Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                &branch_name,
                &worktree_dir.to_string_lossy(),
                "HEAD",
            ])
            .current_dir(&request.project_root)
            .output();
        if let Ok(out) = output
            && out.status.success()
        {
            return (worktree_dir, branch_name, true);
        }
    }

    (
        request.project_root.clone(),
        request.default_branch.clone(),
        false,
    )
}

fn is_git_repo_with_head(dir: &Path) -> bool {
    dir.is_dir()
        && Command::new("git")
            .args(["rev-parse", "--verify", "HEAD"])
            .current_dir(dir)
            .output()
            .is_ok_and(|out| out.status.success())
}

fn current_git_info(cursor: &ExecutionCursor<'_>) -> StepGitInfo {
    StepGitInfo {
        mode_badge: if cursor.is_isolated_worktree {
            "Worktree".to_owned()
        } else {
            "Checkout".to_owned()
        },
        machine: "This Mac".to_owned(),
        branch: cursor.branch_name.to_owned(),
        checkout: cursor
            .working_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo")
            .to_owned(),
        working_dir: cursor.working_dir.display().to_string(),
    }
}

fn emit_live_diffs(cursor: &ExecutionCursor<'_>) {
    let mut diffs = collect_git_diffs(cursor.working_dir);
    cursor.redactor.redact_diffs(&mut diffs);
    if !diffs.is_empty() {
        let _ = cursor.tx.send(RunnerEvent::DiffUpdated { diffs });
    }
}

/// Parses live `git diff` in `working_dir` into [`DiffFileChange`] cards for the Diff tab.
pub fn collect_git_diffs(working_dir: &Path) -> Vec<DiffFileChange> {
    let Ok(output) = Command::new("git")
        .args([
            "-c",
            "diff.noprefix=false",
            "-c",
            "diff.mnemonicPrefix=false",
            "diff",
            "--no-ext-diff",
            "--no-color",
            "--unified=3",
        ])
        .current_dir(working_dir)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    parse_unified_diff(&raw)
}

/// Parses a unified git diff string into structured [`DiffFileChange`] entries.
pub fn parse_unified_diff(raw: &str) -> Vec<DiffFileChange> {
    let mut files = Vec::new();
    let mut current: Option<DiffFileChange> = None;

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(prev) = current.take() {
                files.push(prev);
            }
            let path = rest
                .split_whitespace()
                .nth(1)
                .map(|p| p.strip_prefix("b/").unwrap_or(p))
                .unwrap_or(rest)
                .to_owned();
            current = Some(DiffFileChange {
                path,
                additions: 0,
                deletions: 0,
                hunk_header: String::new(),
                lines: Vec::new(),
            });
            continue;
        }
        let Some(file) = current.as_mut() else {
            continue;
        };
        if line.starts_with("@@ ") {
            if file.hunk_header.is_empty() {
                file.hunk_header = line.to_owned();
            }
            continue;
        }
        if line.starts_with("+++ ")
            || line.starts_with("--- ")
            || line.starts_with("index ")
            || line.starts_with("new file ")
            || line.starts_with("deleted file ")
        {
            continue;
        }
        if let Some(added) = line.strip_prefix('+') {
            file.additions += 1;
            if file.lines.len() < 120 {
                file.lines.push(DiffLine {
                    kind: DiffLineKind::Added,
                    text: added.to_owned(),
                });
            }
        } else if let Some(removed) = line.strip_prefix('-') {
            file.deletions += 1;
            if file.lines.len() < 120 {
                file.lines.push(DiffLine {
                    kind: DiffLineKind::Removed,
                    text: removed.to_owned(),
                });
            }
        } else if let Some(ctx_line) = line.strip_prefix(' ')
            && file.lines.len() < 120
        {
            file.lines.push(DiffLine {
                kind: DiffLineKind::Context,
                text: ctx_line.to_owned(),
            });
        }
    }

    if let Some(last) = current {
        files.push(last);
    }
    files
}

fn collect_planned_steps(
    nodes: &[WorkflowNodeDef],
    catalog: &BrukCatalog,
) -> Vec<PlannedStepSummary> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            WorkflowNodeDef::Composite(comp_step) => {
                if let Some(comp) = catalog.resolve_composite(&comp_step.uses) {
                    out.extend(collect_planned_steps(&comp.nodes, catalog));
                } else {
                    out.push(PlannedStepSummary {
                        step_id: comp_step.id.clone(),
                        title: comp_step.name.clone(),
                        actor_badge: format!("Composite · {}", comp_step.uses),
                    });
                }
            }
            WorkflowNodeDef::Loop(lp) => {
                out.extend(collect_planned_steps(&lp.steps, catalog));
            }
            WorkflowNodeDef::AgentTask(s) => out.push(PlannedStepSummary {
                step_id: s.id.clone(),
                title: s.name.clone(),
                actor_badge: s.agent.clone().unwrap_or_else(|| "Agent".to_owned()),
            }),
            WorkflowNodeDef::ShellCommand(s) => out.push(PlannedStepSummary {
                step_id: s.id.clone(),
                title: s.name.clone(),
                actor_badge: "Shell".to_owned(),
            }),
            WorkflowNodeDef::ApprovalGate(s) => out.push(PlannedStepSummary {
                step_id: s.id.clone(),
                title: s.name.clone(),
                actor_badge: "You".to_owned(),
            }),
            WorkflowNodeDef::CommentOnIssue(s) => out.push(PlannedStepSummary {
                step_id: s.id.clone(),
                title: s.name.clone(),
                actor_badge: "Issue source".to_owned(),
            }),
            WorkflowNodeDef::SetIssueStatus(s) => out.push(PlannedStepSummary {
                step_id: s.id.clone(),
                title: s.name.clone(),
                actor_badge: "Issue source".to_owned(),
            }),
            WorkflowNodeDef::OpenPullRequest(s) => out.push(PlannedStepSummary {
                step_id: s.id.clone(),
                title: s.name.clone(),
                actor_badge: "GitHub PR".to_owned(),
            }),
            WorkflowNodeDef::Condition(s) => out.push(PlannedStepSummary {
                step_id: s.id.clone(),
                title: s.name.clone(),
                actor_badge: "Condition".to_owned(),
            }),
        }
    }
    out
}

/// Structured event parsed from a Claude `stream-json` or Codex `--json` stdout line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedStreamEvent {
    AgentText(String),
    ToolCall {
        verb: String,
        target: String,
        status_text: String,
        tone: ToolStatusTone,
    },
    Usage {
        tokens: u64,
        cost_cents: u64,
        result_text: Option<String>,
    },
}

/// Parses one line of `claude --print --output-format stream-json --verbose` output.
pub fn parse_claude_stream_json_line(line: &str) -> Option<ParsedStreamEvent> {
    let val: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let event_type = val.get("type")?.as_str()?;

    match event_type {
        "assistant" => {
            let content = val
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())?;
            for block in content {
                let btype = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if btype == "text"
                    && let Some(text) = block.get("text").and_then(|t| t.as_str())
                    && !text.trim().is_empty()
                {
                    return Some(ParsedStreamEvent::AgentText(text.to_owned()));
                }
                if btype == "tool_use" {
                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("Tool");
                    let target = block
                        .get("input")
                        .and_then(|i| {
                            i.get("file_path")
                                .or_else(|| i.get("command"))
                                .or_else(|| i.get("pattern"))
                                .or_else(|| i.get("path"))
                        })
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    return Some(ParsedStreamEvent::ToolCall {
                        verb: name.to_owned(),
                        target,
                        status_text: "called".to_owned(),
                        tone: ToolStatusTone::Neutral,
                    });
                }
            }
            None
        }
        "result" => {
            let usage = val.get("usage");
            let input = usage
                .and_then(|u| u.get("input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let output = usage
                .and_then(|u| u.get("output_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_read = usage
                .and_then(|u| u.get("cache_read_input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_write = usage
                .and_then(|u| u.get("cache_creation_input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cost_usd = val
                .get("total_cost_usd")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let result_text = val
                .get("result")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            Some(ParsedStreamEvent::Usage {
                tokens: input + output + cache_read + cache_write,
                cost_cents: (cost_usd * 100.0).round() as u64,
                result_text,
            })
        }
        _ => None,
    }
}

/// Parses one line of `codex exec --json` JSONL output.
pub fn parse_codex_json_line(line: &str) -> Option<ParsedStreamEvent> {
    let val: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let event_type = val.get("type")?.as_str()?;

    match event_type {
        "item.completed" => {
            let item = val.get("item")?;
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if item_type == "agent_message"
                && let Some(text) = item.get("text").and_then(|t| t.as_str())
                && !text.trim().is_empty()
            {
                return Some(ParsedStreamEvent::AgentText(text.to_owned()));
            }
            if item_type == "command_execution" {
                let cmd = item
                    .get("command")
                    .and_then(|c| c.as_str())
                    .unwrap_or("command")
                    .to_owned();
                let exit = item.get("exit_code").and_then(|c| c.as_i64()).unwrap_or(0);
                return Some(ParsedStreamEvent::ToolCall {
                    verb: "Shell".to_owned(),
                    target: cmd,
                    status_text: format!("exit {exit}"),
                    tone: if exit == 0 {
                        ToolStatusTone::Success
                    } else {
                        ToolStatusTone::Error
                    },
                });
            }
            None
        }
        "turn.completed" => {
            let usage = val.get("usage")?;
            let input = usage
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let output = usage
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            Some(ParsedStreamEvent::Usage {
                tokens: input + output,
                cost_cents: 0,
                result_text: None,
            })
        }
        _ => None,
    }
}

/// Parses one line of `agy --output-format stream-json` output.
pub fn parse_antigravity_json_line(line: &str) -> Option<ParsedStreamEvent> {
    let val: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    match val.get("event")?.as_str()? {
        "step_update" => {
            let step = val.get("step_update")?;
            if step.get("step_type").and_then(|t| t.as_str()) != Some("tool") {
                return None;
            }
            let state = step.get("state").and_then(|s| s.as_str()).unwrap_or("");
            if state == "ACTIVE" {
                return None;
            }
            let info = step.get("tool_info");
            let name = step
                .get("tool_name")
                .and_then(|n| n.as_str())
                .or_else(|| info.and_then(|i| i.get("name")).and_then(|n| n.as_str()))
                .unwrap_or("tool");
            let (verb, target) =
                crate::chat::antigravity_tool_label(name, info.and_then(|i| i.get("parameters")));
            let failed = state != "DONE"
                || info
                    .and_then(|i| i.get("error"))
                    .is_some_and(|e| !e.is_null() && e.as_str() != Some(""));
            Some(ParsedStreamEvent::ToolCall {
                verb,
                target,
                status_text: if failed { "failed" } else { "done" }.to_owned(),
                tone: if failed {
                    ToolStatusTone::Error
                } else {
                    ToolStatusTone::Success
                },
            })
        }
        "result" => {
            let result = val.get("result")?;
            let ok = result
                .get("status")
                .and_then(|s| s.as_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("success"));
            let text_key = if ok { "response" } else { "error" };
            Some(ParsedStreamEvent::Usage {
                tokens: crate::chat::antigravity_usage_tokens(result.get("usage")),
                cost_cents: 0,
                result_text: result
                    .get(text_key)
                    .and_then(|v| v.as_str())
                    .filter(|t| !t.trim().is_empty())
                    .map(str::to_owned),
            })
        }
        _ => None,
    }
}

/// Applies a [`RunnerEvent`] directly onto a [`WorkflowRunViewData`] model for live UI rendering.
pub fn apply_runner_event_to_view_data(
    view: &mut WorkflowRunViewData,
    event: &RunnerEvent,
    budget_limit_cents: u64,
) {
    match event {
        RunnerEvent::RunStarted {
            run_number,
            branch_name,
            worktree_path,
            steps,
            ..
        } => {
            view.run_number = *run_number;
            view.execution_state = RunExecutionState::Running;
            view.branch_name = branch_name.clone();
            view.worktree_name = worktree_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("worktree")
                .to_owned();
            view.subtitle = format!("Running on {branch_name}");
            view.budget_guard = BudgetGuardState {
                spent_cents: 0,
                limit_cents: budget_limit_cents,
                pause_when_reached: true,
            };
            view.steps = steps
                .iter()
                .enumerate()
                .map(|(idx, s)| RunStepDetail {
                    number: idx + 1,
                    title: s.title.clone(),
                    status: RunStepStatus::Queued,
                    duration: None,
                    loop_badge: None,
                    actor_badge: s.actor_badge.clone(),
                    agent: StepAgentInfo {
                        agent_name: s.actor_badge.clone(),
                        agent_scope: None,
                        provider: "—".to_owned(),
                        model: "—".to_owned(),
                        sandbox: "—".to_owned(),
                        config: "—".to_owned(),
                        approvals: "—".to_owned(),
                    },
                    git: StepGitInfo {
                        mode_badge: "Worktree".to_owned(),
                        machine: "This Mac".to_owned(),
                        branch: branch_name.clone(),
                        checkout: view.worktree_name.clone(),
                        working_dir: worktree_path.display().to_string(),
                    },
                    progress: StepProgressInfo {
                        loop_label: None,
                        exit_when: None,
                        elapsed: "0:00".to_owned(),
                        tokens: "0 tok".to_owned(),
                    },
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                    transcript: Vec::new(),
                    next_callout: None,
                })
                .collect();
        }
        RunnerEvent::StepStarted {
            step_index,
            title,
            actor_badge,
            attempt,
            max_attempts,
            agent_info,
            git_info,
            ..
        } => {
            while view.steps.len() <= *step_index {
                let num = view.steps.len() + 1;
                view.steps.push(RunStepDetail {
                    number: num,
                    title: title.clone(),
                    status: RunStepStatus::Queued,
                    duration: None,
                    loop_badge: None,
                    actor_badge: actor_badge.clone(),
                    agent: (**agent_info).clone(),
                    git: (**git_info).clone(),
                    progress: StepProgressInfo {
                        loop_label: None,
                        exit_when: None,
                        elapsed: "0:00".to_owned(),
                        tokens: "0 tok".to_owned(),
                    },
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                    transcript: Vec::new(),
                    next_callout: None,
                });
            }
            let step = &mut view.steps[*step_index];
            step.title = title.clone();
            step.actor_badge = actor_badge.clone();
            step.status = RunStepStatus::Running;
            step.agent = (**agent_info).clone();
            step.git = (**git_info).clone();
            if let (Some(a), Some(max)) = (attempt, max_attempts) {
                step.loop_badge = Some(format!("attempt {a}/{max}"));
                step.progress.loop_label = Some(format!("Attempt {a} of {max}"));
                view.attempt_summary = Some(format!("Attempt {a} of {max}"));
            }
            view.active_step_index = *step_index;
            view.active_provider_name = agent_info.provider.clone();
            view.execution_state = RunExecutionState::Running;
        }
        RunnerEvent::TranscriptAppended { step_index, entry } => {
            if let Some(step) = view.steps.get_mut(*step_index) {
                step.transcript.push(entry.clone());
            }
        }
        RunnerEvent::StepFinished {
            step_index,
            status,
            duration_ms,
            output_preview,
            tokens_used,
            cost_cents,
        } => {
            if let Some(step) = view.steps.get_mut(*step_index) {
                step.status = *status;
                step.duration = Some(format_duration_ms(*duration_ms));
                step.progress.elapsed = format_duration_ms(*duration_ms);
                if *tokens_used > 0 {
                    step.progress.tokens = format!("{tokens_used} tok");
                }
                if !output_preview.is_empty() && step.outputs.is_empty() {
                    step.outputs.push(StepOutputChip {
                        name: "output".to_owned(),
                        pending: false,
                    });
                }
            }
            view.budget_guard.spent_cents =
                view.budget_guard.spent_cents.saturating_add(*cost_cents);
        }
        // The transcript entry sent alongside it carries the problem into the view.
        RunnerEvent::McpProblem { .. }
        | RunnerEvent::SecretProblem { .. }
        | RunnerEvent::McpServersReady { .. } => {}
        RunnerEvent::DiffUpdated { diffs } => {
            view.diffs = diffs.clone();
        }
        RunnerEvent::ApprovalRequested {
            step_index, prompt, ..
        } => {
            view.execution_state = RunExecutionState::NeedsApproval;
            if let Some(step) = view.steps.get_mut(*step_index) {
                step.status = RunStepStatus::NeedsApproval;
                step.next_callout = Some(("Review required".to_owned(), prompt.clone()));
            }
        }
        RunnerEvent::RunFinished {
            status,
            total_duration_ms,
            total_cost_cents,
            summary,
            ..
        } => {
            view.execution_state = *status;
            view.total_elapsed = format_duration_ms(*total_duration_ms);
            view.budget_guard.spent_cents = *total_cost_cents;
            view.subtitle = summary.clone();
        }
    }
}

fn record_step_outputs(
    ctx: &mut TemplateContext,
    step_id: &str,
    exit_code: i32,
    stdout: &str,
    stderr: &str,
    output: &str,
) {
    ctx.step_outputs
        .insert(format!("{step_id}.exit_code"), exit_code.to_string());
    ctx.step_outputs
        .insert(format!("{step_id}.stdout"), stdout.to_owned());
    ctx.step_outputs
        .insert(format!("{step_id}.stderr"), stderr.to_owned());
    ctx.step_outputs
        .insert(format!("{step_id}.output"), output.to_owned());
}

fn wait_child_with_cancel(
    child: &mut Child,
    control: &RunControlHandle,
) -> Result<std::process::ExitStatus, RunnerError> {
    loop {
        if control.is_stopped() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RunnerError::Stopped);
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(err) => {
                return Err(RunnerError::Process(format!(
                    "failed waiting on child process: {err}"
                )));
            }
        }
    }
}

pub(crate) fn terminate_pid(pid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

fn truncate_for_prompt(raw: &str, max_chars: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_owned()
    } else {
        let mut s: String = trimmed.chars().take(max_chars).collect();
        s.push('…');
        s
    }
}

fn format_duration_ms(ms: u64) -> String {
    let secs = ms / 1000;
    let mins = secs / 60;
    let rem = secs % 60;
    format!("{mins}:{rem:02}")
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    use super::*;
    use crate::definitions::{ApprovalGateStepDef, LoopExitConditionDef, WorkflowTriggersDef};

    #[test]
    fn parses_antigravity_stream_json_events() {
        let tool = r#"{"event":"step_update","step_update":{"step_index":5,"state":"DONE","step_type":"tool","tool_name":"run_command","tool_info":{"parameters":{"CommandLine":"npm test"},"output":"ok"}}}"#;
        assert_eq!(
            parse_antigravity_json_line(tool),
            Some(ParsedStreamEvent::ToolCall {
                verb: "Shell".to_owned(),
                target: "npm test".to_owned(),
                status_text: "done".to_owned(),
                tone: ToolStatusTone::Success,
            })
        );
        let result = r#"{"event":"result","result":{"status":"SUCCESS","response":"All green.","usage":{"input_tokens":100,"output_tokens":5,"cache_read_tokens":50}}}"#;
        assert_eq!(
            parse_antigravity_json_line(result),
            Some(ParsedStreamEvent::Usage {
                tokens: 155,
                cost_cents: 0,
                result_text: Some("All green.".to_owned()),
            })
        );
        assert_eq!(
            parse_antigravity_json_line(
                r#"{"event":"step_update","step_update":{"state":"ACTIVE","step_type":"agent_response","text_delta":"All"}}"#
            ),
            None
        );
    }

    #[test]
    fn parses_claude_stream_json_and_codex_jsonl_events() {
        let claude_text = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Analyzing bug root cause."}]}}"#;
        assert_eq!(
            parse_claude_stream_json_line(claude_text),
            Some(ParsedStreamEvent::AgentText(
                "Analyzing bug root cause.".to_owned()
            ))
        );

        let claude_tool = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/lib.rs"}}]}}"#;
        assert_eq!(
            parse_claude_stream_json_line(claude_tool),
            Some(ParsedStreamEvent::ToolCall {
                verb: "Read".to_owned(),
                target: "src/lib.rs".to_owned(),
                status_text: "called".to_owned(),
                tone: ToolStatusTone::Neutral,
            })
        );

        let claude_res = r#"{"type":"result","total_cost_usd":0.42,"usage":{"input_tokens":1200,"output_tokens":300,"cache_read_input_tokens":500},"result":"Done"}"#;
        assert_eq!(
            parse_claude_stream_json_line(claude_res),
            Some(ParsedStreamEvent::Usage {
                tokens: 2000,
                cost_cents: 42,
                result_text: Some("Done".to_owned()),
            })
        );

        let codex_msg =
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"Patch applied."}}"#;
        assert_eq!(
            parse_codex_json_line(codex_msg),
            Some(ParsedStreamEvent::AgentText("Patch applied.".to_owned()))
        );
    }

    #[test]
    fn executes_real_workflow_with_worktree_loop_retry_fake_claude_diff_and_approval_gate() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("demo-repo");
        fs::create_dir_all(&repo).unwrap();

        // Initialize a real git repo with one initial commit so worktree isolation activates.
        fs::write(repo.join("status.txt"), "broken\n").unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["add", "."],
            vec!["commit", "-m", "initial"],
        ] {
            let status = Command::new("git")
                .args(&args)
                .current_dir(&repo)
                .status()
                .unwrap();
            assert!(status.success());
        }

        // Create a deterministic fake `claude` CLI that verifies isolation flags, edits `status.txt`
        // to "fixed" on attempt 2 (when previous failure output is present), and emits stream-json.
        let fake_claude = temp.path().join("fake-claude");
        let script = r#"#!/bin/sh
# Verify Bruk enforced user-context isolation flags
echo "$@" | grep -q -- "--setting-sources project" || { echo "missing setting-sources" >&2; exit 2; }
echo "$@" | grep -q -- "--strict-mcp-config" || { echo "missing strict-mcp" >&2; exit 2; }
echo "$@" | grep -q -- "--disable-slash-commands" || { echo "missing disable-slash" >&2; exit 2; }

if echo "$@" | grep -q "still broken"; then
  echo "fixed" > status.txt
  echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Fixed status.txt on retry."}]}}'
else
  echo "attempt1" > status.txt
  echo '{"type":"assistant","message":{"content":[{"type":"text","text":"First attempt edit."}]}}'
fi
echo '{"type":"result","total_cost_usd":0.05,"usage":{"input_tokens":400,"output_tokens":100}}'
"#;
        fs::write(&fake_claude, script).unwrap();
        fs::set_permissions(&fake_claude, fs::Permissions::from_mode(0o755)).unwrap();

        let workflow = WorkflowDef {
            id: "fix-status".to_owned(),
            name: "Fix status".to_owned(),
            description: "Retry fix until check passes, then approve".to_owned(),
            draft: false,
            triggers: WorkflowTriggersDef::default(),
            inputs: BTreeMap::new(),
            mcp_sources: Default::default(),
            nodes: vec![
                WorkflowNodeDef::Loop(LoopNodeDef {
                    id: "fix_loop".to_owned(),
                    name: "Fix until check passes".to_owned(),
                    repeat: LoopRepeatMode::UntilCheckPasses,
                    max_attempts: 3,
                    wait_between: crate::workflow_editor::LoopWaitBetween::None,
                    pass_previous_outputs: true,
                    discard_file_changes: false,
                    exit_conditions: vec![LoopExitConditionDef {
                        step: "check".to_owned(),
                        field: ExitField::ExitCode,
                        operator: ExitOperator::Equals,
                        expected: "0".to_owned(),
                    }],
                    on_exhausted: LoopExhaustedAction::FailRun,
                    fallback: None,
                    steps: vec![
                        WorkflowNodeDef::AgentTask(AgentTaskStepDef {
                            id: "fix".to_owned(),
                            name: "Apply fix".to_owned(),
                            agent: Some("implementer".to_owned()),
                            provider: Some(ProviderKind::Claude),
                            model: None,
                            skills: Vec::new(),
                            prompt: "Fix status file".to_owned(),
                            inputs: BTreeMap::new(),
                            outputs: Vec::new(),
                            mcp: Vec::new(),
                        }),
                        WorkflowNodeDef::ShellCommand(ShellCommandStepDef {
                            id: "check".to_owned(),
                            name: "Verify status".to_owned(),
                            command:
                                "grep -q fixed status.txt || { echo 'still broken' >&2; exit 1; }"
                                    .to_owned(),
                        }),
                    ],
                }),
                WorkflowNodeDef::ApprovalGate(ApprovalGateStepDef {
                    id: "review".to_owned(),
                    name: "Review gate".to_owned(),
                    prompt: "Approve the verified diff".to_owned(),
                }),
            ],
            origin: crate::definitions::DefinitionOrigin::BuiltIn,
        };

        let runtime_root = temp.path().join("runtime-run-1");
        let req = WorkflowRunRequest {
            run_id: "run-test-1".to_owned(),
            run_number: 1,
            project_id: 1,
            project_name: "demo-repo".to_owned(),
            project_root: repo.clone(),
            default_branch: "main".to_owned(),
            workflow,
            catalog: BrukCatalog::builtin_only(),
            inputs: BTreeMap::new(),
            issue_number: None,
            github_repo: None,
            github_auth: None,
            github_auth_mode: IssueSourceAuthMode::GitHubCli,
            github_pat_secret_id: None,
            secret_resolver: None,
            default_provider: ProviderKind::Claude,
            default_sandbox: ProjectSandboxMode::WorkspaceWrite,
            model_override: None,
            claude_binary: Some(fake_claude),
            claude_default_model: Some("sonnet".to_owned()),
            claude_settings: ClaudeSpawnSettings::default(),
            codex_binary: None,
            codex_default_model: None,
            codex_settings: CodexSpawnSettings::default(),
            source_codex_home: None,
            antigravity_binary: None,
            antigravity_default_model: None,
            use_isolated_worktrees: true,
            branch_prefix: "agent/".to_owned(),
            open_prs_as_draft: false,
            link_pr_closes_issue: false,
            project_env: BTreeMap::new(),
            mcp: Default::default(),
            runtime_root: Some(runtime_root.clone()),
            budget_limit_cents: 500,
        };

        let (control, rx) = spawn_workflow_run(req);
        let mut view = WorkflowRunViewData::empty("demo-repo", "Fix status");
        let mut saw_approval = false;
        let mut final_state = None;

        for event in rx {
            apply_runner_event_to_view_data(&mut view, &event, 500);
            match event {
                RunnerEvent::ApprovalRequested { .. } => {
                    saw_approval = true;
                    control.resolve_gate(GateDecision::Approve {
                        note: Some("LGTM".to_owned()),
                    });
                }
                RunnerEvent::RunFinished { status, .. } => {
                    final_state = Some(status);
                    break;
                }
                _ => {}
            }
        }

        assert!(saw_approval);
        assert_eq!(final_state, Some(RunExecutionState::Succeeded));
        assert_eq!(view.execution_state, RunExecutionState::Succeeded);
        assert_eq!(view.budget_guard.spent_cents, 10); // 2 attempts * 5 cents
        assert!(view.diffs.iter().any(|d| d.path == "status.txt"));
        // Main repo working tree stays untouched because work ran in an isolated worktree!
        assert_eq!(
            fs::read_to_string(repo.join("status.txt")).unwrap(),
            "broken\n"
        );
        assert_eq!(
            fs::read_to_string(runtime_root.join("worktree/status.txt")).unwrap(),
            "fixed\n"
        );
    }

    /// Runs a one-step Claude workflow against a fake `claude` that reports `statuses` for the
    /// MCP servers it was given, returning every event and whether the fake CLI ran.
    fn run_mcp_workflow(
        policy: McpSourcesPolicy,
        approved: bool,
        statuses: &str,
    ) -> (Vec<RunnerEvent>, bool) {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join(".agents")).unwrap();
        fs::write(
            repo.join(".agents/mcp_config.json"),
            r#"{"mcpServers":{"proj":{"command":"sh"},"other":{"command":"sh"}}}"#,
        )
        .unwrap();
        let marker = temp.path().join("ran");
        let fake_claude = temp.path().join("fake-claude");
        fs::write(
            &fake_claude,
            format!(
                r#"#!/bin/sh
echo "$@" | grep -q -- "--mcp-config" || {{ echo "missing mcp-config" >&2; exit 2; }}
touch "{marker}"
echo '{{"type":"system","subtype":"init","mcp_servers":[{statuses}]}}'
echo '{{"type":"assistant","message":{{"content":[{{"type":"text","text":"done"}}]}}}}'
echo '{{"type":"result","total_cost_usd":0.01,"usage":{{"input_tokens":1,"output_tokens":1}}}}'
"#,
                marker = marker.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&fake_claude, fs::Permissions::from_mode(0o755)).unwrap();

        let project = crate::mcp::load_project_mcp(&repo);
        let fingerprint = project.config().map(|c| c.fingerprint.clone());
        let workflow = WorkflowDef {
            id: "mcp".to_owned(),
            name: "MCP".to_owned(),
            description: String::new(),
            draft: false,
            triggers: WorkflowTriggersDef::default(),
            inputs: BTreeMap::new(),
            mcp_sources: policy,
            nodes: vec![WorkflowNodeDef::AgentTask(AgentTaskStepDef {
                id: "work".to_owned(),
                name: "Work".to_owned(),
                agent: None,
                provider: Some(ProviderKind::Claude),
                model: None,
                skills: Vec::new(),
                prompt: "Go".to_owned(),
                inputs: BTreeMap::new(),
                outputs: Vec::new(),
                mcp: Vec::new(),
            })],
            origin: crate::definitions::DefinitionOrigin::BuiltIn,
        };
        let req = WorkflowRunRequest {
            run_id: "run-mcp".to_owned(),
            run_number: 1,
            project_id: 1,
            project_name: "repo".to_owned(),
            project_root: repo.clone(),
            default_branch: "main".to_owned(),
            workflow,
            catalog: BrukCatalog::builtin_only(),
            inputs: BTreeMap::new(),
            issue_number: None,
            github_repo: None,
            github_auth: None,
            github_auth_mode: IssueSourceAuthMode::GitHubCli,
            github_pat_secret_id: None,
            secret_resolver: None,
            default_provider: ProviderKind::Claude,
            default_sandbox: ProjectSandboxMode::WorkspaceWrite,
            model_override: None,
            claude_binary: Some(fake_claude),
            claude_default_model: None,
            claude_settings: ClaudeSpawnSettings::default(),
            codex_binary: None,
            codex_default_model: None,
            codex_settings: CodexSpawnSettings::default(),
            source_codex_home: None,
            antigravity_binary: None,
            antigravity_default_model: None,
            use_isolated_worktrees: false,
            branch_prefix: "agent/".to_owned(),
            open_prs_as_draft: false,
            link_pr_closes_issue: false,
            project_env: BTreeMap::new(),
            mcp: McpSources {
                project,
                allow_project_servers: true,
                approved_fingerprint: if approved { fingerprint } else { None },
                project_extras: Vec::new(),
                user_extras: Vec::new(),
            },
            runtime_root: Some(temp.path().join("rt")),
            budget_limit_cents: 500,
        };
        let (_control, rx) = spawn_workflow_run(req);
        let mut events = Vec::new();
        for event in rx {
            let done = matches!(event, RunnerEvent::RunFinished { .. });
            events.push(event);
            if done {
                break;
            }
        }
        (events, marker.exists())
    }

    fn run_status(events: &[RunnerEvent]) -> Option<RunExecutionState> {
        events.iter().find_map(|e| match e {
            RunnerEvent::RunFinished { status, .. } => Some(*status),
            _ => None,
        })
    }

    fn mcp_problems(events: &[RunnerEvent]) -> Vec<&McpProblem> {
        events
            .iter()
            .filter_map(|e| match e {
                RunnerEvent::McpProblem { problem, .. } => Some(problem),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn project_only_workflow_stops_when_a_project_server_fails_to_start() {
        let (events, ran) = run_mcp_workflow(
            McpSourcesPolicy::Project,
            true,
            r#"{"name":"proj","status":"failed"},{"name":"other","status":"connected"}"#,
        );
        assert!(ran);
        assert_eq!(run_status(&events), Some(RunExecutionState::Failed));
        let problems = mcp_problems(&events);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].server.as_deref(), Some("proj"));
        assert!(problems[0].blocking);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, RunnerEvent::McpServersReady { .. }))
        );
    }

    #[test]
    fn project_only_workflow_doesnt_start_the_agent_with_an_unapproved_project_file() {
        let (events, ran) = run_mcp_workflow(McpSourcesPolicy::Project, false, "");
        assert!(
            !ran,
            "the agent must not start without the servers it needs"
        );
        assert_eq!(run_status(&events), Some(RunExecutionState::Failed));
        let problems = mcp_problems(&events);
        assert_eq!(problems[0].fix, crate::mcp::McpFix::ApproveProjectServers);
        assert!(problems[0].blocking);
    }

    #[test]
    fn permissive_workflow_continues_and_reports_failed_and_ready_servers() {
        let (events, ran) = run_mcp_workflow(
            McpSourcesPolicy::All,
            true,
            r#"{"name":"proj","status":"failed"},{"name":"other","status":"connected"}"#,
        );
        assert!(ran);
        assert_eq!(run_status(&events), Some(RunExecutionState::Succeeded));
        let problems = mcp_problems(&events);
        assert_eq!(problems.len(), 1);
        assert!(!problems[0].blocking);
        let ready = events
            .iter()
            .find_map(|e| match e {
                RunnerEvent::McpServersReady { servers } => Some(servers.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            ready.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["other"]
        );
        assert!(events.iter().any(|e| matches!(
            e,
            RunnerEvent::TranscriptAppended {
                entry: StepTranscriptEntry::McpProblem { .. },
                ..
            }
        )));
    }
}
