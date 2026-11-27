//! Lightweight shared domain types, identifiers, and pure helpers used across
//! all Software Factory crates.
//!
//! Keeping these types in a small, dependency-light leaf crate breaks inter-module
//! cycles and lets downstream domain and UI crates compile and cache independently.

pub mod appearance;
pub mod model;
pub mod provider;
pub mod secrets;
pub mod util;
pub mod workflow;

pub use appearance::{
    AppearanceMode, AppearanceSettings, DEFAULT_MONO_FONT_FAMILY, DEFAULT_UI_FONT_FAMILY,
    FontPreference, ProjectColor,
};
pub use model::{
    LogEntry, LogLevel, Project, ProjectId, RunId, RunStatus, StepKind, StepRun, StepRunStatus,
    Workflow, WorkflowId, WorkflowRun, WorkflowStep,
};
pub use provider::{
    ClaudeModelOption, ClaudeQuotaReset, ClaudeUsageSummary, ClaudeUsageWindow, CliFlagEncoding,
    ProviderKind, SSH_AUTH_SOCK_ENV, model_family_from_id,
};
pub use secrets::{
    AGENTS_MCP_CONFIG_PATH, EnvScope, Exposure, InternalPurpose, ManagerId, McpFix, McpOrigin,
    McpProblem, ONE_PASSWORD_MANAGER_ID, ProjectMcpFile, ROOT_MCP_JSON_PATH, SecretFailure,
    SecretFix, SecretId, SecretKind, SecretProblem, SecretSource, SecretState, Sharing,
    env_references,
};
pub use util::{
    BRUK_APP_VERSION, BRUK_DATA_SCHEMA, clamp_percent, current_unix_seconds,
    default_severity_for_used, expand_tilde, format_dollars_cents, format_whole_or_cents,
    is_executable_file, metadata_is_executable, micros_to_cents, split_shell_words_raw,
};
pub use workflow::{
    DiffFileChange, DiffLine, DiffLineKind, ExitField, ExitOperator, IssueSourceAuthMode,
    IssueSourceKind, LoopExhaustedAction, LoopRepeatMode, LoopWaitBetween, ProjectSandboxMode,
    parse_unified_diff,
};
