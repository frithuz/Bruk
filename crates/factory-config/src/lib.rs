//! Application configuration, Bruk home folder management, MCP resolution,
//! project settings, preferences, and inbox attention models.

pub mod bruk_home;
pub mod inbox;
pub mod mcp;
pub mod preferences;
pub mod project_settings;

pub use bruk_home::{
    BRUK_HOME_ENV, BrukHome, CloneError, CloneSource, ClonedRepository, DEFAULT_HOME_DIR_NAME,
    HOME_DATABASE_FILE, HomeEnvironment, HomeError, HomeResolution, HomeSetup, InitRepositoryError,
    InitRepositoryOptions, InitializedRepository, clone_repository, display_with_tilde,
    init_local_repository, parse_clone_source, project_dir_name,
    resolve_new_repository_destination, resolve_system_home, validate_branch_name,
};
pub use inbox::{
    AgentBadge, AlwaysAllowScope, AnsweredInboxItem, AttentionRequest, AttentionRequestBody,
    AttentionRequestKind, InboxFilter, InboxState, LoopAttemptSummary, LoopExhaustedDetail,
    McpProblemDetail, PermissionDetail, PermissionRiskTag, QuestionDetail, QuestionOption,
    ReviewDetail, ReviewFileStat, SecretProblemDetail, WaitingRulesConfig,
};
pub use preferences::{
    AdvancedSettings, AppPreferences, AppearanceMode, AppearanceSettings, CliAccount,
    ConnectionTestState, DEFAULT_MONO_FONT_FAMILY, DEFAULT_UI_FONT_FAMILY, DefaultLandingView,
    ExtensionEntry, ExtensionsSettings, FontPreference, GeneralSettings, MachinesSettings,
    OpenRouterKey, PreferencesError, PreferencesTab, ProviderKind, SSH_AGENT_PRESETS,
    SSH_AUTH_SOCK_ENV, SshAgentPreset, detected_ssh_auth_sock, format_dollars_cents,
    format_whole_or_cents,
};
pub use project_settings::{
    IssueSourceAuthMode, IssueSourceKind, ProjectEnvVar, ProjectIntakeRule, ProjectSandboxMode,
    ProjectSettings, ProjectSettingsError, ProjectSettingsTab, ProjectWorkflowSetting,
};
