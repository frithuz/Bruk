//! Core domain model and workflow execution engine for Software Factory.
//!
//! This crate is intentionally independent of any UI or platform framework so
//! workflow orchestration, persistence, and agent integrations can be tested
//! and reused across the macOS app, CLI tools, or background daemons.

pub mod bruk_home;
pub mod chat;
pub mod claude_cli;
pub mod claude_settings;
pub mod codex_cli;
pub mod codex_settings;
pub mod dashboard;
pub mod definitions;
pub mod engine;
pub mod github_intake;
pub mod inbox;
pub mod machines;
pub mod model;
pub mod preferences;
pub mod project_settings;
pub mod runner;
pub mod step_context;
pub mod storage;
pub mod work_queue;
pub mod workflow_editor;
pub mod workflow_run;
pub mod workflows_overview;

pub use bruk_home::{
    BRUK_HOME_ENV, BrukHome, CloneError, CloneSource, ClonedRepository, DEFAULT_HOME_DIR_NAME,
    HOME_DATABASE_FILE, HomeEnvironment, HomeError, HomeResolution, HomeSetup, clone_repository,
    display_with_tilde, parse_clone_source, project_dir_name, resolve_system_home,
};
pub use chat::{
    ChatContextCategory, ChatContextGroup, ChatContextItem, ChatContextKind, ChatContextUsage,
    ChatEntry, ChatEvent, ChatNoticeTone, ChatProviderTotals, ChatSession, ChatSessionStatus,
    ChatSessionSummary, ChatStreamItem, ChatToolStatus, ChatTurnControl, ChatTurnError,
    ChatTurnRequest, ChatUsageDelta, ChatWorkspace, ChatWorktree, ChatWorktreeSetup,
    UNTITLED_CHAT_TITLE, UsageCount, build_chat_turn_command, chat_title_from_message,
    chat_worktree_branch, execute_chat_turn_blocking, micros_to_cents, parse_claude_chat_line,
    parse_claude_context_usage, parse_codex_chat_line, parse_codex_rollout_context,
    query_chat_context, remove_chat_worktree, spawn_chat_turn,
};
pub use claude_cli::{
    CLAUDE_BINARY_NAME, ClaudeBinaryError, ClaudeCliDetails, ClaudeCliProbe, ClaudeModelOption,
    ClaudeQuotaReset, ClaudeUsageSummary, ClaudeUsageWindow, expand_tilde, fetch_claude_details,
    format_reset_countdown, format_reset_countdown_now, format_reset_schedule,
    format_reset_schedule_now, model_family_from_id, parse_claude_auth_status,
    parse_claude_details_stream, probe_claude_cli, probe_claude_cli_from_env,
    resolve_claude_binary, resolve_claude_binary_from_env, run_claude_auth_login,
};
pub use claude_settings::{
    CLAUDE_DISABLE_AUTOUPDATER_ENV, CLAUDE_DISABLE_AUTOUPDATER_VALUE, CLAUDE_ENV_PRESETS,
    CLAUDE_SETTING_SPECS, ClaudeEnvPreset, ClaudeSettingCategory, ClaudeSettingKind,
    ClaudeSettingSpec, ClaudeSettingTarget, ClaudeSettingValue, ClaudeSettingsError,
    ClaudeSpawnOptions, ClaudeSpawnPlan, ClaudeSpawnSettings, CliFlagEncoding,
    apply_mandatory_claude_spawn_env, build_claude_command, build_claude_spawn_plan,
    claude_setting_specs_for_category, find_claude_setting_spec, new_claude_command,
    split_shell_words,
};
pub use codex_cli::{
    CODEX_BINARY_NAME, CodexBinaryError, CodexCliDetails, CodexCliProbe, fetch_codex_details,
    parse_codex_account_stream, parse_codex_details_stream, probe_codex_cli,
    probe_codex_cli_from_env, resolve_codex_binary, resolve_codex_binary_from_env, run_codex_login,
};
pub use codex_settings::{
    CODEX_DISABLE_UPDATE_CHECK_ARG, CODEX_DISABLE_UPDATE_CHECK_KEY,
    CODEX_DISABLE_UPDATE_CHECK_VALUE, CODEX_ENV_PRESETS, CODEX_SETTING_SPECS, CodexEnvPreset,
    CodexSettingCategory, CodexSettingKind, CodexSettingSpec, CodexSettingTarget,
    CodexSettingValue, CodexSettingsError, CodexSpawnOptions, CodexSpawnPlan, CodexSpawnSettings,
    apply_mandatory_codex_spawn_args, build_codex_command, build_codex_spawn_plan,
    codex_setting_specs_for_category, find_codex_setting_spec, new_codex_subcommand,
};
pub use dashboard::{
    DashboardDataset, DashboardGranularity, DashboardInsight, DashboardQuery, DashboardSnapshot,
    HighTokenRun, InboxItem, InboxRequestKind, MachineNode, MachineStatus, ProjectActivityRow,
    ProjectColor, ProjectTokens, ProviderUsage, TokenBucket, WaitMetrics, format_axis_tokens,
    format_currency, format_tenths_hours, format_tokens, format_wait_minutes,
};
pub use definitions::{
    AgentDef, AgentTaskStepDef, AgentToolsPolicy, ApprovalGateStepDef, BRUK_AGENTS_SUBDIR,
    BRUK_COMPOSITES_SUBDIR, BRUK_CONFIG_DIR, BRUK_MANIFEST_FILE, BRUK_SKILLS_SUBDIR,
    BRUK_WORKFLOWS_SUBDIR, BUILTIN_NAMESPACE, BrukCatalog, BrukManifest, CatalogDiagnostic,
    CatalogError, CatalogLoadOptions, CommentOnIssueStepDef, CompositeDef, CompositeStepDef,
    ConditionStepDef, DefinitionOrigin, DefinitionRef, DiagnosticSeverity, InputParamDef,
    LoadedModuleInfo, LoopExitConditionDef, LoopNodeDef, ModuleDecl, ModuleFetchPolicy,
    ModuleSource, OpenPullRequestStepDef, PROJECT_NAMESPACE, ResolvedAgent, SetIssueStatusStepDef,
    ShellCommandStepDef, SkillDef, WorkflowDef, WorkflowNodeDef, WorkflowTriggersDef,
    default_module_cache_dir, validate_identifier,
};
pub use engine::{DryRunExecutor, FactoryError, FactoryState, StepExecutor, StepOutcome};
pub use github_intake::{
    AGENT_WORKING_LABEL, DetectedGitRemote, GH_BINARY_NAME, GitHubAuth, GitHubFilterClause,
    GitHubFilterQualifier, GitHubIntakeError, GitHubIssue, GitHubSyncSnapshot,
    add_github_issue_label, build_github_search_query, comment_on_github_issue,
    current_unix_seconds, detect_git_remote_info, fetch_github_open_issues,
    fetch_github_open_issues_with_filter, format_synced_relative, normalize_assignee_handle,
    normalize_github_repo_slug, parse_github_issues_json, parse_github_repo_slug,
    remove_github_issue_label, resolve_gh_binary,
};
pub use inbox::{
    AgentBadge, AlwaysAllowScope, AnsweredInboxItem, AttentionRequest, AttentionRequestBody,
    AttentionRequestKind, InboxFilter, InboxState, LoopAttemptSummary, LoopExhaustedDetail,
    PermissionDetail, PermissionRiskTag, QuestionDetail, QuestionOption, ReviewDetail,
    ReviewFileStat, WaitingRulesConfig,
};
pub use machines::{
    BRUK_APP_VERSION, BRUK_DATA_SCHEMA, CoreUpdatePolicy, MachineAgentKind, MachineCatchUpSummary,
    MachineConnectionStatus, MachinePlatform, MachineRecord, MachineSession, MachinesError,
    MachinesFleet, NewMachineDraft, SchemaCompatibility, SessionNowTone,
};
pub use model::{
    LogEntry, LogLevel, Project, ProjectId, RunId, RunStatus, StepKind, StepRun, StepRunStatus,
    Workflow, WorkflowId, WorkflowRun, WorkflowStep,
};
pub use preferences::{
    AdvancedSettings, AppPreferences, AppearanceMode, AppearanceSettings, CliAccount,
    ConnectionTestState, DEFAULT_MONO_FONT_FAMILY, DEFAULT_UI_FONT_FAMILY, DefaultLandingView,
    ExtensionEntry, ExtensionsSettings, FontPreference, GeneralSettings, MachinesSettings,
    OpenRouterKey, PreferencesError, PreferencesTab, ProviderKind, format_dollars_cents,
    format_whole_or_cents,
};
pub use project_settings::{
    IssueSourceAuthMode, IssueSourceKind, ProjectEnvVar, ProjectIntakeRule, ProjectSandboxMode,
    ProjectSettings, ProjectSettingsError, ProjectSettingsTab, ProjectWorkflowSetting,
};
pub use runner::{
    GateDecision, ParsedStreamEvent, PlannedStepSummary, RunControlHandle, RunOutcomeSummary,
    RunnerError, RunnerEvent, WorkflowRunRequest, apply_runner_event_to_view_data,
    collect_git_diffs, execute_workflow_blocking, parse_claude_stream_json_line,
    parse_codex_json_line, parse_unified_diff, spawn_workflow_run,
};
pub use step_context::{
    MaterializedStepContext, StepContextBundle, StepContextError, SubagentContextSpec,
    TemplateContext, build_isolated_claude_spawn_plan, build_isolated_codex_spawn_plan,
    compile_step_context, default_step_runtime_dir, materialize_step_context,
};
pub use storage::{PersistedChatUsage, PersistedWorkflowRun, ProjectDatabase, StorageError};
pub use work_queue::{
    GitHubIntakeDiff, IntakeRule, ProjectWorkQueue, ProjectWorkflowSummary, UnmatchedIssue,
    WorkQueueError, WorkQueueItem, WorkQueueStage, WorkQueueViewMode,
};
pub use workflow_editor::{
    EditorFallbackStep, EditorLoopBlock, EditorNode, EditorRevision, EditorSelection, EditorStep,
    EditorTriggerConfig, ExitField, ExitOperator, LoopExhaustedAction, LoopExitCondition,
    LoopRepeatMode, LoopWaitBetween, PaletteCategory, StepTypeKind, ValidationReport,
    WorkflowEditorDocument,
};
pub use workflow_run::{
    AttemptBanner, BudgetGuardState, CoverageFileRow, DeployWatchEntry, DiffFileChange, DiffLine,
    DiffLineKind, RunDetailTab, RunExecutionState, RunStepDetail, RunStepStatus, StepAgentInfo,
    StepGitInfo, StepIoBinding, StepOutputChip, StepProgressInfo, StepTranscriptEntry,
    TerminalBadgeTone, TerminalLine, ToolStatusTone, WorkflowRunError, WorkflowRunViewData,
};
pub use workflows_overview::{
    ProjectWorkflowsOverview, RecentRunStatus, RunOutcomeDot, WorkflowNowTone,
    WorkflowOverviewEntry, WorkflowRecentRun, WorkflowStepGroup, WorkflowStepPreview,
    WorkflowTemplateKind, WorkflowTriggerKind, WorkflowTriggerLine, WorkflowsFilter,
    WorkflowsOverviewError, WorkflowsSort,
};
