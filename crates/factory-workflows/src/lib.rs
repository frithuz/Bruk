//! Layered Bruk workflow/agent/skill definitions (YAML + embedded builtins),
//! visual workflow editor, workflows overview, dry-run engine, run view models,
//! and isolated step context compilation.

pub mod definitions;
pub mod engine;
pub mod step_context;
pub mod workflow_editor;
pub mod workflow_run;
pub mod workflows_overview;

pub use definitions::{
    AgentDef, AgentTaskStepDef, AgentToolsPolicy, ApprovalGateStepDef, BRUK_AGENTS_SUBDIR,
    BRUK_COMPOSITES_SUBDIR, BRUK_CONFIG_DIR, BRUK_MANIFEST_FILE, BRUK_SKILLS_SUBDIR,
    BRUK_WORKFLOWS_SUBDIR, BUILTIN_NAMESPACE, BrukCatalog, BrukManifest, CatalogDiagnostic,
    CatalogError, CatalogLoadOptions, CommentOnIssueStepDef, CompositeDef, CompositeStepDef,
    ConditionStepDef, DefinitionOrigin, DefinitionRef, DiagnosticSeverity, InputParamDef,
    LoadedModuleInfo, LoopExitConditionDef, LoopNodeDef, ModuleDecl, ModuleFetchPolicy,
    ModuleSource, OpenPullRequestStepDef, PROJECT_NAMESPACE, ResolvedAgent, SetIssueStatusStepDef,
    ShellCommandStepDef, SkillDef, WorkflowDef, WorkflowNodeDef, WorkflowTriggersDef,
    default_module_cache_dir, sync_project_workflows_from_catalog, validate_identifier,
};
pub use engine::{DryRunExecutor, FactoryError, FactoryState, StepExecutor, StepOutcome};
pub use step_context::{
    ANTIGRAVITY_READ_ONLY_DENY_RULES, ANTIGRAVITY_WORKSPACE_WRITE_ALLOW_RULES,
    AntigravitySpawnPlan, MaterializedStepContext, StepContextBundle, StepContextError,
    SubagentContextSpec, TemplateContext, build_isolated_antigravity_spawn_plan,
    build_isolated_claude_spawn_plan, build_isolated_codex_spawn_plan, compile_step_context,
    default_step_runtime_dir, materialize_step_context, provider_effort_levels,
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
