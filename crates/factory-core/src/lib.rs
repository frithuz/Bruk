//! Core domain model and workflow execution engine for Software Factory.
//!
//! This crate is intentionally independent of any UI or platform framework so
//! workflow orchestration, persistence, and agent integrations can be tested
//! and reused across the macOS app, CLI tools, or background daemons.

pub mod dashboard;
pub mod engine;
pub mod model;
pub mod preferences;
pub mod project_settings;
pub mod storage;
pub mod work_queue;
pub mod workflow_run;

pub use dashboard::{
    DashboardDataset, DashboardGranularity, DashboardInsight, DashboardQuery, DashboardSnapshot,
    HighTokenRun, InboxItem, InboxRequestKind, MachineNode, MachineStatus, ProjectActivityRow,
    ProjectColor, ProjectTokens, ProviderUsage, TokenBucket, WaitMetrics, format_axis_tokens,
    format_currency, format_tenths_hours, format_tokens, format_wait_minutes,
};
pub use engine::{DryRunExecutor, FactoryError, FactoryState, StepExecutor, StepOutcome};
pub use model::{
    LogEntry, LogLevel, Project, ProjectId, RunId, RunStatus, StepKind, StepRun, StepRunStatus,
    Workflow, WorkflowId, WorkflowRun, WorkflowStep,
};
pub use preferences::{
    AdvancedSettings, AppPreferences, AppearanceMode, AppearanceSettings, CliAccount,
    ConnectionTestState, DefaultLandingView, ExtensionEntry, ExtensionsSettings, GeneralSettings,
    MachinesSettings, OpenRouterKey, PreferencesError, PreferencesTab, ProviderKind,
    format_dollars_cents, format_whole_or_cents,
};
pub use project_settings::{
    IssueSourceAuthMode, IssueSourceKind, ProjectEnvVar, ProjectIntakeRule, ProjectSandboxMode,
    ProjectSettings, ProjectSettingsError, ProjectSettingsTab, ProjectWorkflowSetting,
};
pub use storage::{ProjectDatabase, StorageError, default_db_path};
pub use work_queue::{
    IntakeRule, ProjectWorkQueue, ProjectWorkflowSummary, UnmatchedIssue, WorkQueueError,
    WorkQueueItem, WorkQueueStage, WorkQueueViewMode,
};
pub use workflow_run::{
    AttemptBanner, BudgetGuardState, CoverageFileRow, DeployWatchEntry, DiffFileChange, DiffLine,
    DiffLineKind, RunDetailTab, RunExecutionState, RunStepDetail, RunStepStatus, StepAgentInfo,
    StepGitInfo, StepIoBinding, StepOutputChip, StepProgressInfo, StepTranscriptEntry,
    TerminalBadgeTone, TerminalLine, ToolStatusTone, WorkflowRunError, WorkflowRunViewData,
};
