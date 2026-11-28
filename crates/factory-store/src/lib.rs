//! Reactive per-surface application state backed by `factory-core` and SQLite.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    future::Future,
    path::Path,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
};

use factory_chat::{
    CHAT_ATTACHMENT_MAX_BYTES, ChatContextUsage, ChatEntry, ChatEvent, ChatGitStatus,
    ChatPullRequestRequest, ChatSession, ChatSessionStatus, ChatSessionSummary, ChatTokenBreakdown,
    ChatTurnControl, ChatTurnRequest, ChatUsageDelta, ChatWorkspace, ChatWorktree,
    ChatWorktreeSetup, GitHubPullRequestLookup, MergedBranchCleanup, RemoteBranchCleanup,
    apply_chat_worktree_to_checkout, attachment_mime_type, chat_branch_merge_evidence,
    chat_worktree_branch, delete_merged_branch, fetch_base_branch, format_bytes,
    inspect_chat_git_status, is_model_image_type, open_chat_pull_request, open_dir_in_terminal,
    open_url_in_browser, plan_chat_attachments, query_chat_context, remove_chat_worktree,
    spawn_chat_turn, worktree_has_uncommitted_changes,
};
use factory_config::{
    AgentBadge, AlwaysAllowScope, AnsweredInboxItem, AppPreferences, AttentionRequest,
    AttentionRequestBody, AttentionRequestKind, BrukHome, CliAccount, DefaultLandingView,
    FontPreference, HomeEnvironment, HomeError, HomeResolution, InboxFilter, InboxState,
    InitRepositoryOptions, PreferencesTab, ProjectSettings, ProjectSettingsTab, ReviewDetail,
    SecretProblemDetail, clone_repository, display_with_tilde, init_local_repository,
    parse_clone_source, project_dir_name, resolve_new_repository_destination, validate_branch_name,
};
use factory_github::{
    AGENT_WORKING_LABEL, GitHubAuth, GitHubFilterClause, IntakeRule, ProjectWorkQueue,
    WorkQueueStage, WorkQueueViewMode, add_github_issue_label, detect_git_remote_info,
    fetch_github_open_issues_with_filter, remove_github_issue_label, resolve_gh_binary,
};
use factory_machines::{
    CoreUpdatePolicy, MachineAgentKind, MachinePlatform, MachineRecord, MachinesFleet,
    NewMachineDraft,
};
use factory_providers::{
    AntigravityCliProbe, ClaudeCliProbe, ClaudeSettingValue, ClaudeSpawnOptions, ClaudeSpawnPlan,
    ClaudeSpawnSettings, CodexCliProbe, CodexSettingValue, CodexSpawnOptions, CodexSpawnPlan,
    CodexSpawnSettings, fetch_antigravity_details, fetch_antigravity_usage, fetch_claude_details,
    fetch_codex_details, find_claude_setting_spec, find_codex_setting_spec, probe_antigravity_cli,
    probe_claude_cli, probe_codex_cli, resolve_antigravity_binary, resolve_claude_binary,
    resolve_codex_binary, run_antigravity_login, run_claude_auth_login, run_codex_login,
};
use factory_runner::{
    GateDecision, RunControlHandle, RunnerEvent, WorkflowRunRequest,
    apply_runner_event_to_view_data, spawn_workflow_run,
};
use factory_secrets::{
    CancellationToken, ChatSecretsCache, CommandManagerConfig, EnvScope, Exposure, InternalPurpose,
    ManagerId, ManagerStatus, MemoryStore, ONE_PASSWORD_MANAGER_ID, OnePasswordAuthMode,
    OnePasswordManager, SecretFix, SecretId, SecretKind, SecretManager, SecretProblem,
    SecretRecord, SecretRedactor, SecretResolver, SecretState, SecretStore,
    SecretsMetadataSnapshot, Sharing, StoreStatus, contains_env_reference, default_secret_store,
    sanitize_env_segment, validate_env_name, validate_secret_value,
};
use factory_storage::{
    DashboardDataset, DashboardGranularity, DashboardQuery, DashboardSnapshot,
    PersistedWorkflowRun, ProjectDatabase, StorageError, delete_legacy_database_files,
    find_legacy_database_files, migrate_plaintext_secrets,
};
use factory_types::{
    AppearanceMode, AppearanceSettings, ExitField, ExitOperator, IssueSourceAuthMode,
    IssueSourceKind, LoopExhaustedAction, LoopRepeatMode, LoopWaitBetween, Project, ProjectId,
    ProjectSandboxMode, ProviderKind, RunId, RunStatus, Workflow, WorkflowId, WorkflowRun,
    current_unix_seconds, expand_tilde,
};
use factory_workflows::{
    AgentDef, AgentTaskStepDef, BrukCatalog, CatalogLoadOptions, CompositeDef, DefinitionOrigin,
    DeployWatchEntry, DryRunExecutor, EditorSelection, FactoryState, ProjectWorkflowsOverview,
    RecentRunStatus, RunDetailTab, RunExecutionState, RunOutcomeDot, SkillDef, StepTypeKind,
    ValidationReport, WorkflowDef, WorkflowEditorDocument, WorkflowNodeDef, WorkflowRecentRun,
    WorkflowRunViewData, WorkflowTemplateKind, WorkflowTriggersDef, WorkflowsFilter, WorkflowsSort,
    provider_effort_levels, sync_project_workflows_from_catalog,
};
use secrecy::{ExposeSecret, SecretString};

#[derive(Default)]
struct RunnerRegistry {
    active_by_key: BTreeMap<(String, String), RunControlHandle>,
    gates_by_id: BTreeMap<String, (RunControlHandle, (String, String))>,
    run_seq: u64,
}

fn runner_registry() -> &'static Mutex<RunnerRegistry> {
    static REGISTRY: std::sync::OnceLock<Mutex<RunnerRegistry>> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(RunnerRegistry::default()))
}
use nsh_core::{
    element::desktop::main_window,
    geometry::{Bounds, Points},
    platform::Appearance,
    prelude::*,
};

/// Primary screen shown in the main content area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveView {
    Dashboard,
    Inbox,
    Machines,
    WorkQueue,
    Workflows,
    WorkflowEditor,
    WorkflowRun,
    ProjectSettings,
    Workspace,
    RegisterProject,
    Chat,
}

/// Controls how `claude` is searched on `PATH` when no custom binary path is configured.
#[derive(Debug, Clone, Default)]
pub enum ClaudePathLookup {
    /// Search the current process's `PATH` environment variable (production default).
    #[default]
    SystemPath,
    /// Search only this explicit `PATH` value (used by isolated tests).
    ExplicitPath(OsString),
}

impl ClaudePathLookup {
    fn resolve_path_env(&self) -> Option<OsString> {
        match self {
            Self::SystemPath => std::env::var_os("PATH"),
            Self::ExplicitPath(path) => Some(path.clone()),
        }
    }
}

struct ThreadOneshot<T> {
    state: Arc<Mutex<(Option<T>, Option<Waker>)>>,
}

impl<T> Future for ThreadOneshot<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut guard = self.state.lock().expect("oneshot lock poisoned");
        if let Some(value) = guard.0.take() {
            Poll::Ready(value)
        } else {
            guard.1 = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

fn spawn_blocking_task<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> ThreadOneshot<T> {
    let state = Arc::new(Mutex::new((None, None::<Waker>)));
    let shared = state.clone();
    std::thread::spawn(move || {
        let result = work();
        let waker = {
            let mut guard = shared.lock().expect("oneshot lock poisoned");
            guard.0 = Some(result);
            guard.1.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    });
    ThreadOneshot { state }
}

/// First-launch choice of where the Bruk folder lives.
#[derive(Clone)]
struct HomeSetupDraft {
    environment: HomeEnvironment,
    root_input: String,
    legacy_database: Option<std::path::PathBuf>,
    import_legacy: bool,
    error: Option<String>,
}

#[derive(Clone)]
struct AppState {
    factory: FactoryState,
    db: Option<Arc<Mutex<ProjectDatabase>>>,
    db_path_display: String,
    home: Option<BrukHome>,
    home_setup: Option<HomeSetupDraft>,
    clone_source_input: String,
    clone_name_input: String,
    clone_in_progress: bool,
    create_name_input: String,
    create_location_input: String,
    create_branch_input: String,
    create_readme: bool,
    create_in_progress: bool,
    demo_mode: bool,
    using_sample_projects: bool,
    saved_real_preferences: Option<AppPreferences>,
    claude_path_lookup: ClaudePathLookup,
    claude_sign_in_in_progress: bool,
    claude_details_loading: bool,
    claude_probe_seq: Arc<AtomicU64>,
    codex_sign_in_in_progress: bool,
    codex_details_loading: bool,
    codex_probe_seq: Arc<AtomicU64>,
    antigravity_sign_in_in_progress: bool,
    antigravity_details_loading: bool,
    antigravity_probe_seq: Arc<AtomicU64>,
    active_view: ActiveView,
    selected_project: Option<ProjectId>,
    expanded_projects: BTreeSet<ProjectId>,
    selected_workflow: Option<WorkflowId>,
    selected_run: Option<RunId>,
    work_queues: BTreeMap<String, ProjectWorkQueue>,
    work_queue_view_mode: WorkQueueViewMode,
    work_queue_selected_issue: Option<u32>,
    work_queue_inspector_visible: bool,
    work_queue_ad_hoc_open: bool,
    work_queue_unmatched_expanded: bool,
    work_queue_syncing: bool,
    catalogs: BTreeMap<String, BrukCatalog>,
    workflows_overviews: BTreeMap<String, ProjectWorkflowsOverview>,
    workflows_filter: WorkflowsFilter,
    workflows_sort: WorkflowsSort,
    workflows_search: String,
    workflows_selected_id: Option<String>,
    workflows_inspector_visible: bool,
    workflows_new_drawer_open: bool,
    workflows_import_drawer_open: bool,
    workflows_editing_selected: bool,
    workflows_ad_hoc_open: bool,
    workflows_more_menu_open: bool,
    workflow_editors: BTreeMap<(String, String), WorkflowEditorDocument>,
    active_workflow_editor_key: (String, String),
    workflow_editor_return_view: ActiveView,
    workflow_editor_palette_search: String,
    workflow_editor_inspector_visible: bool,
    workflow_editor_history_open: bool,
    workflow_editor_ad_hoc_open: bool,
    workflow_editor_validation: Option<ValidationReport>,
    workflow_runs_detail: BTreeMap<(String, String), WorkflowRunViewData>,
    active_workflow_run_key: (String, String),
    workflow_run_selected_step: usize,
    workflow_run_tab: RunDetailTab,
    workflow_run_inspector_visible: bool,
    workflow_run_command_palette_open: bool,
    deploy_watch_entries: Vec<DeployWatchEntry>,
    dashboard_dataset: DashboardDataset,
    dashboard_granularity: DashboardGranularity,
    dashboard_period_offset: i32,
    dashboard_project_filter: Option<String>,
    tokens_show_table: bool,
    inbox: InboxState,
    inbox_filter: InboxFilter,
    inbox_selected_id: Option<String>,
    inbox_answered_expanded: bool,
    inbox_waiting_rules_open: bool,
    inbox_editing_command: bool,
    inbox_edited_command: String,
    inbox_always_allow_open: bool,
    inbox_always_allow_pattern: String,
    inbox_note_draft: String,
    inbox_question_choice: Option<String>,
    inbox_feedback: Option<String>,
    menubar_popover_open: bool,
    menubar_anchor_bounds: Option<Bounds<Points>>,
    machines_fleet: MachinesFleet,
    selected_machine_id: Option<u64>,
    machines_changes_expanded: bool,
    machines_update_cores_open: bool,
    machines_add_drawer_open: bool,
    machines_settings_open: bool,
    preferences: AppPreferences,
    preferences_open: bool,
    preferences_focused: bool,
    preferences_tab: PreferencesTab,
    selected_provider_id: String,
    selected_prefs_machine_id: Option<u64>,
    preferences_feedback: Option<String>,
    preferences_error: Option<String>,
    project_settings: BTreeMap<String, ProjectSettings>,
    project_settings_tab: ProjectSettingsTab,
    project_settings_feedback: Option<String>,
    project_settings_error: Option<String>,
    chat_sessions: BTreeMap<String, ChatSession>,
    /// The open chat of each project; none means its composer starts a new chat.
    chat_selected: BTreeMap<ProjectId, String>,
    /// What the next new chat uses; `None` follows the agent, then the project's settings.
    chat_new_provider: Option<ProviderKind>,
    chat_new_agent: Option<String>,
    /// Model for the next new chat; `None` keeps the agent's, project's or provider's default.
    chat_new_model: Option<String>,
    /// Reasoning effort for the next new chat; `None` keeps the agent's or provider's.
    chat_new_effort: Option<String>,
    chat_new_sandbox: Option<ProjectSandboxMode>,
    chat_new_workspace: Option<ChatWorkspace>,
    /// Files waiting in the composer to go with the next message.
    chat_pending_attachments: Vec<PendingChatAttachment>,
    /// Whether chats show their context-window panel.
    chat_context_open: bool,
    /// Chats whose context window is being read.
    chat_context_loading: BTreeSet<String>,
    /// Why reading a chat's context window failed.
    chat_context_errors: BTreeMap<String, String>,
    /// Whether chats show their Changes / Git panel.
    chat_changes_open: bool,
    /// Latest git status & diffs inspected for each chat.
    chat_git_status: BTreeMap<String, ChatGitStatus>,
    /// Chats whose git status/diffs are currently being read.
    chat_git_loading: BTreeSet<String>,
    /// Chats currently running a git action (commit, PR, apply).
    chat_git_busy: BTreeSet<String>,
    /// Latest git inspection or action error for a chat.
    chat_git_errors: BTreeMap<String, String>,
    /// Latest git action feedback banner for a chat.
    chat_git_feedback: BTreeMap<String, String>,
    /// Pull request URL opened from this chat during the current session.
    chat_pr_urls: BTreeMap<String, String>,
    /// Projects whose merged chat worktrees are being looked for and pruned.
    worktree_prune_projects: BTreeSet<ProjectId>,
    /// Chats whose worktree is being checked for pruning; they take no messages meanwhile.
    chats_pruning: BTreeSet<String>,
    secret_store: Arc<dyn SecretStore>,
    secret_resolver: Arc<SecretResolver>,
    secrets: Vec<SecretRecord>,
    chat_secrets_caches: BTreeMap<String, Arc<Mutex<ChatSecretsCache>>>,
    secrets_store_status: StoreStatus,
    db_path_change_pending: Option<(std::path::PathBuf, std::path::PathBuf)>,
    legacy_db_files_found: Vec<std::path::PathBuf>,
    legacy_database_paths: Vec<std::path::PathBuf>,
    secrets_feedback: Option<String>,
    secrets_error: Option<String>,
    secrets_test_in_progress: bool,
    secrets_test_result: Option<Result<String, String>>,
    secrets_prefill_name: Option<String>,
    secrets_prefill_scope: Option<EnvScope>,
    sidebar_visible: bool,
    status_message: Option<String>,
    registration_error: Option<String>,
    updater_state: factory_updater::UpdaterState,
    updater_banner_dismissed: bool,
}

impl Default for AppState {
    fn default() -> Self {
        let _span = factory_logging::boot_log::span("boot.store.default_state");
        let home_env = {
            let _s = factory_logging::boot_log::span("boot.store.home_env_system");
            HomeEnvironment::system()
        };
        let secret_store = {
            let _s = factory_logging::boot_log::span("boot.store.secret_store_init");
            default_secret_store()
        };
        Self::from_environment(home_env, ClaudePathLookup::SystemPath, secret_store)
    }
}

fn claude_probe_outcome(probe: &ClaudeCliProbe) -> &'static str {
    match probe {
        ClaudeCliProbe::Authenticated { .. } => "authenticated",
        ClaudeCliProbe::NotAuthenticated { .. } => "not_authenticated",
        ClaudeCliProbe::BinaryNotFound { .. } => "binary_not_found",
        ClaudeCliProbe::ProbeFailed { .. } => "probe_failed",
    }
}

fn codex_probe_outcome(probe: &CodexCliProbe) -> &'static str {
    match probe {
        CodexCliProbe::Authenticated { .. } => "authenticated",
        CodexCliProbe::NotAuthenticated { .. } => "not_authenticated",
        CodexCliProbe::BinaryNotFound { .. } => "binary_not_found",
        CodexCliProbe::ProbeFailed { .. } => "probe_failed",
    }
}

fn antigravity_probe_outcome(probe: &AntigravityCliProbe) -> &'static str {
    match probe {
        AntigravityCliProbe::Authenticated { .. } => "authenticated",
        AntigravityCliProbe::NotAuthenticated { .. } => "not_authenticated",
        AntigravityCliProbe::BinaryNotFound { .. } => "binary_not_found",
        AntigravityCliProbe::ProbeFailed { .. } => "probe_failed",
    }
}

/// Loads a project's definitions catalog, caching git modules in the Bruk folder.
fn load_catalog(home: Option<&BrukHome>, project_root: &Path) -> BrukCatalog {
    let options = CatalogLoadOptions {
        module_cache_dir: home.map(BrukHome::module_cache_dir),
        ..CatalogLoadOptions::default()
    };
    BrukCatalog::load_for_project_with_options(project_root, &options)
}

impl AppState {
    /// Opens the Bruk folder this machine is configured for, or waits for first-launch setup.
    fn from_environment(
        environment: Result<HomeEnvironment, HomeError>,
        claude_path_lookup: ClaudePathLookup,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        match environment {
            Ok(environment) => {
                let legacy_paths = environment.legacy_database_paths.clone();
                let resolve_span = factory_logging::boot_log::span("boot.store.home_resolve");
                let resolution = environment.resolve();
                resolve_span.finish(match &resolution {
                    HomeResolution::Ready(_) => "resolution=ready",
                    HomeResolution::NeedsSetup { .. } => "resolution=needs_setup",
                });
                match resolution {
                    HomeResolution::Ready(home) => {
                        Self::from_home(home, claude_path_lookup, secret_store, legacy_paths)
                    }
                    HomeResolution::NeedsSetup { suggested_root } => Self::awaiting_home_setup(
                        environment,
                        &suggested_root,
                        claude_path_lookup,
                        secret_store,
                    ),
                }
            }
            Err(err) => {
                factory_logging::boot_log::error("boot.store.home_env_error", &format!("{err}"));
                Self::from_database_result_with_path(
                    Err(StorageError::Home(err)),
                    None,
                    claude_path_lookup,
                    secret_store,
                    Vec::new(),
                )
            }
        }
    }

    fn from_home(
        home: BrukHome,
        claude_path_lookup: ClaudePathLookup,
        secret_store: Arc<dyn SecretStore>,
        legacy_database_paths: Vec<std::path::PathBuf>,
    ) -> Self {
        let db_span = factory_logging::boot_log::span("boot.store.db_open");
        let result = ProjectDatabase::open_home(&home);
        if result.is_ok() {
            db_span.finish("ok=true");
        } else {
            db_span.finish_error("ok=false");
        }
        Self::from_database_result_with_path(
            result,
            Some(home),
            claude_path_lookup,
            secret_store,
            legacy_database_paths,
        )
    }

    fn awaiting_home_setup(
        environment: HomeEnvironment,
        suggested_root: &Path,
        claude_path_lookup: ClaudePathLookup,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        let legacy_paths = environment.legacy_database_paths.clone();
        let mut state = Self::build_for_mode(
            None,
            "Not set up yet".to_owned(),
            Vec::new(),
            AppPreferences::empty(),
            false,
            None,
            None,
            claude_path_lookup,
            None,
            secret_store,
            legacy_paths,
        );
        let legacy_database = environment.legacy_database();
        state.home_setup = Some(HomeSetupDraft {
            root_input: display_with_tilde(suggested_root),
            import_legacy: legacy_database.is_some(),
            legacy_database,
            environment,
            error: None,
        });
        state
    }

    fn from_database_result_with_path(
        result: Result<ProjectDatabase, StorageError>,
        home: Option<BrukHome>,
        claude_path_lookup: ClaudePathLookup,
        secret_store: Arc<dyn SecretStore>,
        legacy_database_paths: Vec<std::path::PathBuf>,
    ) -> Self {
        let (db, db_path_display, persisted_projects, persisted_prefs, status_message, reg_error) =
            match result {
                Ok(db) => {
                    let db_path_display = db
                        .path()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| ":memory:".to_owned());
                    let (projects, mut status_message) = {
                        let span = factory_logging::boot_log::span("boot.store.db_list_projects");
                        match db.list_projects() {
                            Ok(projects) => {
                                span.finish(format!("count={}", projects.len()));
                                (projects, None)
                            }
                            Err(err) => {
                                span.finish_error("ok=false");
                                (Vec::new(), Some(format!("Failed to load projects: {err}")))
                            }
                        }
                    };
                    let preferences = {
                        let span =
                            factory_logging::boot_log::span("boot.store.db_load_preferences");
                        match db.load_preferences() {
                            Ok(Some(prefs)) => {
                                span.finish(format!("found=true demo_mode={}", prefs.demo_mode));
                                prefs
                            }
                            Ok(None) => {
                                span.finish("found=false demo_mode=false");
                                AppPreferences::empty()
                            }
                            Err(err) => {
                                span.finish_error("ok=false");
                                if status_message.is_none() {
                                    status_message =
                                        Some(format!("Failed to load preferences: {err}"));
                                }
                                AppPreferences::empty()
                            }
                        }
                    };
                    (
                        Some(Arc::new(Mutex::new(db))),
                        db_path_display,
                        projects,
                        preferences,
                        status_message,
                        None,
                    )
                }
                Err(err) => {
                    log::error!(target: "bruk::storage", "db.open_failed err={err}");
                    (
                        None,
                        "Unavailable".to_owned(),
                        Vec::new(),
                        AppPreferences::empty(),
                        Some(format!("Could not open SQLite database: {err}")),
                        Some(format!("Database unavailable: {err}")),
                    )
                }
            };

        let demo_mode = persisted_prefs.demo_mode;
        Self::build_for_mode(
            db,
            db_path_display,
            persisted_projects,
            persisted_prefs,
            demo_mode,
            status_message,
            reg_error,
            claude_path_lookup,
            home,
            secret_store,
            legacy_database_paths,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_for_mode(
        db: Option<Arc<Mutex<ProjectDatabase>>>,
        db_path_display: String,
        persisted_projects: Vec<Project>,
        mut persisted_prefs: AppPreferences,
        demo_mode: bool,
        status_message: Option<String>,
        registration_error: Option<String>,
        claude_path_lookup: ClaudePathLookup,
        home: Option<BrukHome>,
        secret_store: Arc<dyn SecretStore>,
        legacy_database_paths: Vec<std::path::PathBuf>,
    ) -> Self {
        let build_span = factory_logging::boot_log::span("boot.store.build_for_mode");
        let secrets_store_status = {
            let span = factory_logging::boot_log::span("boot.store.secret_store_status");
            let status = secret_store.status();
            let label = match &status {
                StoreStatus::Available => "available",
                StoreStatus::Locked => "locked",
                StoreStatus::Unavailable(_) => "unavailable",
            };
            span.finish(format!("status={label}"));
            status
        };
        let mut db_path_change_pending = None;
        let mut startup_secret_problems: Vec<(String, SecretProblem)> = Vec::new();
        let mut secrets_error = None;
        let mut legacy_db_files_found = Vec::new();

        if !demo_mode
            && let Some(db_arc) = db.as_ref()
            && let Ok(guard) = db_arc.lock()
        {
            let checks_span = factory_logging::boot_log::span("boot.store.secrets_startup_checks");
            let path_check_result = {
                let _s = factory_logging::boot_log::span("boot.store.db_path_change_check");
                guard.check_database_path_change()
            };
            match path_check_result {
                Ok(Some((old_path, new_path))) => {
                    db_path_change_pending = Some((old_path.clone(), new_path.clone()));
                    startup_secret_problems.push((
                        "App".to_owned(),
                        SecretProblem {
                            project_id: None,
                            secret_id: None,
                            secret_name: None,
                            manager_id: Some("keychain".to_owned()),
                            reason: format!(
                                "Database moved or cloned ({} → {}). Open Secrets Settings to confirm whether this database was moved or cloned before Keychain cleanup runs.",
                                display_with_tilde(&old_path),
                                display_with_tilde(&new_path)
                            ),
                            fix: SecretFix::OpenSecretsSettings,
                            blocking: false,
                        },
                    ));
                }
                Ok(None) => {
                    let migration_result = {
                        let _s =
                            factory_logging::boot_log::span("boot.store.migrate_plaintext_secrets");
                        migrate_plaintext_secrets(&guard, secret_store.as_ref(), false, false)
                    };
                    match migration_result {
                        Ok(report) => {
                            if report.has_user_notice() {
                                startup_secret_problems.push((
                                    "App".to_owned(),
                                    SecretProblem {
                                        project_id: None,
                                        secret_id: None,
                                        secret_name: None,
                                        manager_id: Some("keychain".to_owned()),
                                        reason: report.rotation_notice_body(),
                                        fix: SecretFix::OpenSecretsSettings,
                                        blocking: false,
                                    },
                                ));
                            }
                            let reloaded = {
                                let _s = factory_logging::boot_log::span(
                                    "boot.store.reload_preferences",
                                );
                                guard.load_preferences()
                            };
                            if let Ok(Some(reloaded)) = reloaded {
                                persisted_prefs = reloaded;
                            }
                        }
                        Err(err) => {
                            let msg = format!("Plaintext secrets migration paused: {err}");
                            secrets_error = Some(msg.clone());
                            startup_secret_problems.push((
                                "App".to_owned(),
                                SecretProblem {
                                    project_id: None,
                                    secret_id: None,
                                    secret_name: None,
                                    manager_id: Some("keychain".to_owned()),
                                    reason: msg,
                                    fix: SecretFix::OpenSecretsSettings,
                                    blocking: true,
                                },
                            ));
                        }
                    }
                    // Secret tombstone sweeping is deferred to `sweep_secret_tombstones_async`
                    // so Keychain I/O does not block GUI boot.
                }
                Err(err) => {
                    secrets_error = Some(format!("Database path check failed: {err}"));
                }
            }

            let legacy_found = {
                let span = factory_logging::boot_log::span("boot.store.find_legacy_databases");
                let found = find_legacy_database_files(&legacy_database_paths);
                span.finish(format!("count={}", found.len()));
                found
            };
            if !legacy_found.is_empty() {
                let paths_list = legacy_found
                    .iter()
                    .map(|p| display_with_tilde(p))
                    .collect::<Vec<_>>()
                    .join(", ");
                startup_secret_problems.push((
                    "App".to_owned(),
                    SecretProblem {
                        project_id: None,
                        secret_id: None,
                        secret_name: None,
                        manager_id: None,
                        reason: format!(
                            "Old Bruk database files remain on disk ({paths_list}) and may still contain unmigrated plaintext credentials."
                        ),
                        fix: SecretFix::DeleteLegacyDatabases {
                            paths: legacy_found.clone(),
                        },
                        blocking: false,
                    },
                ));
                legacy_db_files_found = legacy_found;
            }
            checks_span.finish(format!(
                "path_changed={} legacy_files={}",
                db_path_change_pending.is_some(),
                legacy_db_files_found.len()
            ));
        }
        let (preferences, saved_real_preferences) = if demo_mode {
            let mut real_copy = persisted_prefs.clone();
            real_copy.demo_mode = false;
            let mut sample = AppPreferences::sample();
            sample.demo_mode = true;
            sample.general = persisted_prefs.general;
            sample.appearance = persisted_prefs.appearance;
            sample.claude_spawn_settings = persisted_prefs.claude_spawn_settings;
            sample.codex_spawn_settings = persisted_prefs.codex_spawn_settings;
            sample.machines = persisted_prefs.machines;
            sample.advanced = persisted_prefs.advanced;
            (sample, Some(real_copy))
        } else {
            // Provider CLI probes (`claude`, `codex`, `agy`) run in parallel on background
            // threads via `probe_providers_async` after mount so subprocesses do not block
            // the first GUI frame.
            let mut clean = persisted_prefs;
            clean.demo_mode = false;
            (clean, None)
        };

        let selected_provider_id = default_selected_provider_id(&preferences);
        let active_view = landing_to_active_view(preferences.general.default_landing_view);

        let using_sample_projects = demo_mode && persisted_projects.is_empty();
        let projects = if using_sample_projects {
            DashboardDataset::sample_projects()
        } else {
            persisted_projects
        };

        let factory = FactoryState::with_projects(projects);
        let selected_project = factory.projects().first().map(|project| project.id);
        let selected_workflow = factory.workflows().first().map(|workflow| workflow.id);
        let mut expanded_projects = BTreeSet::new();
        if let Some(pid) = selected_project {
            expanded_projects.insert(pid);
        }

        let mut work_queues = BTreeMap::new();
        let mut catalogs = BTreeMap::new();
        let mut workflows_overviews = BTreeMap::new();
        let mut project_settings = BTreeMap::new();
        {
            let projects_span = factory_logging::boot_log::span("boot.store.load_projects_data");
            for project in factory.projects() {
                let path_str = project.root_path.display().to_string();
                if demo_mode {
                    work_queues.insert(
                        project.name.clone(),
                        ProjectWorkQueue::for_project(&project.name),
                    );
                    workflows_overviews.insert(
                        project.name.clone(),
                        ProjectWorkflowsOverview::for_project(&project.name),
                    );
                    project_settings.insert(
                        project.name.clone(),
                        ProjectSettings::for_project(
                            &project.name,
                            &path_str,
                            &project.default_branch,
                        ),
                    );
                } else {
                    let project_span = factory_logging::boot_log::span("boot.store.project_load");
                    let t_settings = std::time::Instant::now();
                    let loaded_settings = db
                        .as_ref()
                        .and_then(|arc| arc.lock().ok())
                        .and_then(|guard| guard.load_project_settings(project.id).ok().flatten());
                    let mut settings = loaded_settings.unwrap_or_else(|| {
                        ProjectSettings::empty(&project.name, &path_str, &project.default_branch)
                    });
                    let settings_ms = t_settings.elapsed().as_secs_f64() * 1000.0;

                    let t_git = std::time::Instant::now();
                    let mut probed_git = false;
                    if (settings.git_remote.trim().is_empty()
                        || settings.github_repo.trim().is_empty())
                        && {
                            probed_git = true;
                            true
                        }
                        && let Some(detected) = detect_git_remote_info(&project.root_path)
                    {
                        let before_repo = settings.github_repo.clone();
                        let before_remote = settings.git_remote.clone();
                        settings.apply_detected_git_remote(&detected);
                        if (settings.github_repo != before_repo
                            || settings.git_remote != before_remote)
                            && let Some(db_arc) = db.as_ref()
                            && let Ok(guard) = db_arc.lock()
                        {
                            let _ = guard.save_project_settings(project.id, &settings);
                        }
                    }
                    let git_remote_ms = t_git.elapsed().as_secs_f64() * 1000.0;

                    let t_catalog = std::time::Instant::now();
                    let catalog = load_catalog(home.as_ref(), &project.root_path);
                    sync_project_workflows_from_catalog(&mut settings, &catalog);
                    let catalog_ms = t_catalog.elapsed().as_secs_f64() * 1000.0;

                    let t_queue = std::time::Instant::now();
                    let loaded_queue = db
                        .as_ref()
                        .and_then(|arc| arc.lock().ok())
                        .and_then(|guard| guard.load_project_work_queue(project.id).ok().flatten());
                    let mut queue =
                        loaded_queue.unwrap_or_else(|| ProjectWorkQueue::empty(&project.name));
                    if !settings.github_repo.trim().is_empty()
                        || !settings.intake_filters.is_empty()
                        || !settings.intake_rules.is_empty()
                    {
                        apply_project_settings_to_queue(&settings, &mut queue);
                    }
                    let queue_ms = t_queue.elapsed().as_secs_f64() * 1000.0;

                    work_queues.insert(project.name.clone(), queue);
                    workflows_overviews.insert(
                        project.name.clone(),
                        ProjectWorkflowsOverview::from_catalog(&project.name, &catalog),
                    );
                    catalogs.insert(project.name.clone(), catalog);
                    project_settings.insert(project.name.clone(), settings);
                    project_span.finish(format!(
                        "project_id={} settings_ms={settings_ms:.1} git_remote_ms={git_remote_ms:.1} git_probed={probed_git} catalog_ms={catalog_ms:.1} queue_ms={queue_ms:.1}",
                        project.id
                    ));
                }
            }
            projects_span.finish(format!(
                "projects={} demo_mode={demo_mode}",
                factory.projects().len()
            ));
        }

        let work_queue_selected_issue = selected_project
            .and_then(|pid| factory.project(pid))
            .and_then(|project| work_queues.get(&project.name))
            .and_then(ProjectWorkQueue::default_selected_issue);
        let workflows_selected_id = selected_project
            .and_then(|pid| factory.project(pid))
            .and_then(|project| workflows_overviews.get(&project.name))
            .and_then(ProjectWorkflowsOverview::default_selected_id);

        let (
            dashboard_dataset,
            inbox,
            machines_fleet,
            workflow_runs_detail,
            active_workflow_run_key,
            workflow_run_selected_step,
            workflow_editors,
            active_workflow_editor_key,
            deploy_watch_entries,
        ) = if demo_mode {
            let mut runs_detail = BTreeMap::new();
            let default_run = WorkflowRunViewData::sample_release_prep();
            let default_step = default_run.active_step_index;
            runs_detail.insert(
                ("api-gateway".to_owned(), "Release prep".to_owned()),
                default_run,
            );
            (
                DashboardDataset::sample(),
                InboxState::sample(),
                MachinesFleet::sample(),
                runs_detail,
                ("api-gateway".to_owned(), "Release prep".to_owned()),
                default_step,
                BTreeMap::from([(
                    ("api-gateway".to_owned(), "fix-issue".to_owned()),
                    WorkflowEditorDocument::sample_fix_issue("api-gateway"),
                )]),
                ("api-gateway".to_owned(), "fix-issue".to_owned()),
                WorkflowRunViewData::sample_deploy_watch(),
            )
        } else {
            let persisted_runs = {
                let span = factory_logging::boot_log::span("boot.store.load_workflow_runs");
                let runs = db
                    .as_ref()
                    .and_then(|arc| arc.lock().ok())
                    .and_then(|guard| guard.list_all_workflow_runs().ok())
                    .unwrap_or_default();
                span.finish(format!("count={}", runs.len()));
                runs
            };
            let chat_usage = {
                let span = factory_logging::boot_log::span("boot.store.load_chat_usage");
                let usage = db
                    .as_ref()
                    .and_then(|arc| arc.lock().ok())
                    .and_then(|guard| guard.list_chat_usage().ok())
                    .unwrap_or_default();
                span.finish(format!("count={}", usage.len()));
                usage
            };
            let dashboard = {
                let _span = factory_logging::boot_log::span("boot.store.build_dashboard_dataset");
                DashboardDataset::from_usage(&persisted_runs, &chat_usage)
            };
            let mut runs_detail = BTreeMap::new();
            let mut active_key = (String::new(), String::new());
            for run in &persisted_runs {
                let key = (run.project_name.clone(), run.workflow_name.clone());
                if active_key.0.is_empty() {
                    active_key = key.clone();
                }
                runs_detail.entry(key).or_insert_with(|| run.view.clone());
                if let Some(overview) = workflows_overviews.get_mut(&run.project_name)
                    && let Some(entry) = overview.workflow_mut(&run.workflow_id)
                {
                    let dot = match run.status {
                        RunExecutionState::Succeeded => RunOutcomeDot::Succeeded,
                        RunExecutionState::Failed | RunExecutionState::Stopped => {
                            RunOutcomeDot::FailedOrStopped
                        }
                        RunExecutionState::Running
                        | RunExecutionState::Paused
                        | RunExecutionState::NeedsApproval => RunOutcomeDot::RunningOrWaiting,
                    };
                    if entry.last_ten_runs.len() >= 10 {
                        entry.last_ten_runs.remove(0);
                    }
                    entry.last_ten_runs.push(dot);
                    entry.last_run_caption = format!("Run #{}", run.run_number);
                    entry.last_run_order = entry.last_run_order.min(run.run_number);
                    if entry.recent_runs.len() < 5 {
                        entry.recent_runs.push(WorkflowRecentRun {
                            run_number: run.run_number,
                            summary: run.summary.clone(),
                            status: match run.status {
                                RunExecutionState::Succeeded => RecentRunStatus::Done,
                                RunExecutionState::Failed | RunExecutionState::Stopped => {
                                    RecentRunStatus::Failed
                                }
                                RunExecutionState::NeedsApproval => RecentRunStatus::NeedsYou,
                                RunExecutionState::Running | RunExecutionState::Paused => {
                                    RecentRunStatus::Running
                                }
                            },
                        });
                    }
                }
            }
            (
                dashboard,
                InboxState::empty(),
                MachinesFleet::empty(),
                runs_detail,
                active_key,
                0,
                BTreeMap::new(),
                (String::new(), String::new()),
                Vec::new(),
            )
        };

        let chat_sessions = if using_sample_projects {
            BTreeMap::new()
        } else {
            let span = factory_logging::boot_log::span("boot.store.load_chat_sessions");
            let sessions = load_chat_sessions(db.as_ref());
            span.finish(format!("count={}", sessions.len()));
            sessions
        };

        let (secrets, secrets_snapshot) = if demo_mode {
            (Vec::new(), SecretsMetadataSnapshot::default())
        } else if let Some(db_arc) = db.as_ref()
            && let Ok(guard) = db_arc.lock()
        {
            let span = factory_logging::boot_log::span("boot.store.load_secrets_metadata");
            let records = guard.list_secrets().unwrap_or_default();
            let snapshot = guard
                .build_secrets_metadata_snapshot(&preferences)
                .unwrap_or_default();
            span.finish(format!("count={}", records.len()));
            (records, snapshot)
        } else {
            (Vec::new(), SecretsMetadataSnapshot::default())
        };
        let secret_resolver = Arc::new(SecretResolver::new(
            Arc::clone(&secret_store),
            secrets_snapshot,
        ));

        let inbox_selected_id = inbox.default_selected_id();
        let (inbox_edited_command, inbox_always_allow_pattern, inbox_question_choice) =
            draft_defaults_for_selection(&inbox, inbox_selected_id.as_deref());
        let selected_machine_id = machines_fleet.default_selected_id();

        let mut app = Self {
            factory,
            db,
            db_path_display,
            home,
            home_setup: None,
            clone_source_input: String::new(),
            clone_name_input: String::new(),
            clone_in_progress: false,
            create_name_input: String::new(),
            create_location_input: String::new(),
            create_branch_input: "main".to_owned(),
            create_readme: true,
            create_in_progress: false,
            demo_mode,
            using_sample_projects,
            saved_real_preferences,
            claude_path_lookup,
            claude_sign_in_in_progress: false,
            claude_details_loading: false,
            claude_probe_seq: Arc::new(AtomicU64::new(0)),
            codex_sign_in_in_progress: false,
            codex_details_loading: false,
            codex_probe_seq: Arc::new(AtomicU64::new(0)),
            antigravity_sign_in_in_progress: false,
            antigravity_details_loading: false,
            antigravity_probe_seq: Arc::new(AtomicU64::new(0)),
            active_view,
            selected_project,
            expanded_projects,
            selected_workflow,
            selected_run: None,
            work_queues,
            work_queue_view_mode: WorkQueueViewMode::Board,
            work_queue_selected_issue,
            work_queue_inspector_visible: true,
            work_queue_ad_hoc_open: false,
            work_queue_unmatched_expanded: false,
            work_queue_syncing: false,
            catalogs,
            workflows_overviews,
            workflows_filter: WorkflowsFilter::All,
            workflows_sort: WorkflowsSort::LastRun,
            workflows_search: String::new(),
            workflows_selected_id,
            workflows_inspector_visible: true,
            workflows_new_drawer_open: false,
            workflows_import_drawer_open: false,
            workflows_editing_selected: false,
            workflows_ad_hoc_open: false,
            workflows_more_menu_open: false,
            workflow_editors,
            active_workflow_editor_key,
            workflow_editor_return_view: ActiveView::Workflows,
            workflow_editor_palette_search: String::new(),
            workflow_editor_inspector_visible: true,
            workflow_editor_history_open: false,
            workflow_editor_ad_hoc_open: false,
            workflow_editor_validation: None,
            workflow_runs_detail,
            active_workflow_run_key,
            workflow_run_selected_step,
            workflow_run_tab: RunDetailTab::Output,
            workflow_run_inspector_visible: true,
            workflow_run_command_palette_open: false,
            deploy_watch_entries,
            dashboard_dataset,
            dashboard_granularity: DashboardGranularity::Week,
            dashboard_period_offset: 0,
            dashboard_project_filter: None,
            tokens_show_table: false,
            inbox,
            inbox_filter: InboxFilter::All,
            inbox_selected_id,
            inbox_answered_expanded: false,
            inbox_waiting_rules_open: false,
            inbox_editing_command: false,
            inbox_edited_command,
            inbox_always_allow_open: false,
            inbox_always_allow_pattern,
            inbox_note_draft: String::new(),
            inbox_question_choice,
            inbox_feedback: None,
            menubar_popover_open: false,
            menubar_anchor_bounds: None,
            machines_fleet,
            selected_machine_id,
            machines_changes_expanded: false,
            machines_update_cores_open: false,
            machines_add_drawer_open: false,
            machines_settings_open: false,
            preferences,
            preferences_open: false,
            preferences_focused: false,
            preferences_tab: PreferencesTab::Providers,
            selected_provider_id,
            selected_prefs_machine_id: selected_machine_id,
            preferences_feedback: None,
            preferences_error: None,
            project_settings,
            project_settings_tab: ProjectSettingsTab::IssuesAndIntake,
            project_settings_feedback: None,
            project_settings_error: None,
            chat_sessions,
            chat_selected: BTreeMap::new(),
            chat_new_provider: None,
            chat_new_agent: None,
            chat_new_model: None,
            chat_new_effort: None,
            chat_new_sandbox: None,
            chat_new_workspace: None,
            chat_pending_attachments: Vec::new(),
            chat_context_open: false,
            chat_context_loading: BTreeSet::new(),
            chat_context_errors: BTreeMap::new(),
            chat_changes_open: false,
            chat_git_status: BTreeMap::new(),
            chat_git_loading: BTreeSet::new(),
            chat_git_busy: BTreeSet::new(),
            chat_git_errors: BTreeMap::new(),
            chat_git_feedback: BTreeMap::new(),
            chat_pr_urls: BTreeMap::new(),
            worktree_prune_projects: BTreeSet::new(),
            chats_pruning: BTreeSet::new(),
            secret_store,
            secret_resolver,
            secrets,
            chat_secrets_caches: BTreeMap::new(),
            secrets_store_status,
            db_path_change_pending,
            legacy_db_files_found,
            legacy_database_paths,
            secrets_feedback: None,
            secrets_error,
            secrets_test_in_progress: false,
            secrets_test_result: None,
            secrets_prefill_name: None,
            secrets_prefill_scope: None,
            sidebar_visible: true,
            status_message,
            registration_error,
            updater_state: factory_updater::UpdaterState::Idle,
            updater_banner_dismissed: false,
        };
        for (project_name, problem) in startup_secret_problems {
            let _ = app.file_secret_problem(
                McpProblemContext {
                    project_name: &project_name,
                    context_label: "Secrets".to_owned(),
                    where_label: "Startup check".to_owned(),
                    run_number: 0,
                    provider: ProviderKind::Claude,
                },
                &problem,
            );
        }
        build_span.finish(format!(
            "demo_mode={demo_mode} projects={}",
            app.factory.projects().len()
        ));
        app
    }

    /// Where new projects are cloned: the custom directory from preferences, or the Bruk
    /// folder's `projects/`.
    fn projects_dir(&self) -> Option<std::path::PathBuf> {
        let general = &self.preferences.general;
        match self.home.as_ref() {
            Some(home) => Some(general.projects_dir(home)),
            None if !general.projects_dir_follows_home() => {
                Some(expand_tilde(&general.default_projects_dir))
            }
            None => None,
        }
    }

    fn selected_project_name(&self) -> String {
        self.selected_project
            .and_then(|id| self.factory.project(id))
            .or_else(|| self.factory.projects().first())
            .map(|p| p.name.clone())
            .unwrap_or_else(|| {
                if self.demo_mode {
                    "api-gateway".to_owned()
                } else {
                    String::new()
                }
            })
    }

    fn ensure_queue_mut(&mut self, project_name: &str) -> &mut ProjectWorkQueue {
        let demo = self.demo_mode;
        self.work_queues
            .entry(project_name.to_owned())
            .or_insert_with(|| {
                if demo {
                    ProjectWorkQueue::for_project(project_name)
                } else {
                    ProjectWorkQueue::empty(project_name)
                }
            })
    }

    fn queue_for_name(&self, project_name: &str) -> ProjectWorkQueue {
        self.work_queues
            .get(project_name)
            .cloned()
            .unwrap_or_else(|| {
                if self.demo_mode {
                    ProjectWorkQueue::for_project(project_name)
                } else {
                    ProjectWorkQueue::empty(project_name)
                }
            })
    }

    fn ensure_workflows_overview_mut(
        &mut self,
        project_name: &str,
    ) -> &mut ProjectWorkflowsOverview {
        let demo = self.demo_mode;
        self.workflows_overviews
            .entry(project_name.to_owned())
            .or_insert_with(|| {
                if demo {
                    ProjectWorkflowsOverview::for_project(project_name)
                } else {
                    ProjectWorkflowsOverview::empty(project_name)
                }
            })
    }

    fn workflows_overview_for_name(&self, project_name: &str) -> ProjectWorkflowsOverview {
        self.workflows_overviews
            .get(project_name)
            .cloned()
            .unwrap_or_else(|| {
                if self.demo_mode {
                    ProjectWorkflowsOverview::for_project(project_name)
                } else {
                    ProjectWorkflowsOverview::empty(project_name)
                }
            })
    }

    fn sync_selected_workflow_overview_for_active_project(&mut self) {
        let name = self.selected_project_name();
        let selected_id = self.workflows_selected_id.clone();
        let overview = self.ensure_workflows_overview_mut(&name);
        let valid = selected_id
            .as_deref()
            .is_some_and(|id| overview.workflow(id).is_some());
        if !valid {
            let default_id = overview.default_selected_id();
            self.workflows_selected_id = default_id;
        }
    }

    fn selected_project_meta(&self) -> (String, String, String) {
        self.selected_project
            .and_then(|id| self.factory.project(id))
            .or_else(|| self.factory.projects().first())
            .map(|p| {
                (
                    p.name.clone(),
                    p.root_path.display().to_string(),
                    p.default_branch.clone(),
                )
            })
            .unwrap_or_else(|| {
                if self.demo_mode {
                    (
                        "api-gateway".to_owned(),
                        "/Users/jonathan/src/api-gateway".to_owned(),
                        "main".to_owned(),
                    )
                } else {
                    (String::new(), String::new(), "main".to_owned())
                }
            })
    }

    fn ensure_project_settings_mut(&mut self) -> &mut ProjectSettings {
        let (name, path, branch) = self.selected_project_meta();
        let demo = self.demo_mode;
        self.project_settings
            .entry(name.clone())
            .or_insert_with(|| {
                if demo {
                    ProjectSettings::for_project(&name, &path, &branch)
                } else {
                    ProjectSettings::empty(&name, &path, &branch)
                }
            })
    }

    fn active_project_settings_clone(&self) -> ProjectSettings {
        let (name, path, branch) = self.selected_project_meta();
        self.project_settings
            .get(&name)
            .cloned()
            .unwrap_or_else(|| {
                if self.demo_mode {
                    ProjectSettings::for_project(&name, &path, &branch)
                } else {
                    ProjectSettings::empty(&name, &path, &branch)
                }
            })
    }

    fn sync_queue_from_project_settings(&mut self) {
        let settings = self.active_project_settings_clone();
        let queue = self.ensure_queue_mut(&settings.project_name);
        apply_project_settings_to_queue(&settings, queue);
    }

    fn project_root_by_name(&self, name: &str) -> Option<std::path::PathBuf> {
        self.factory
            .projects()
            .iter()
            .find(|project| project.name == name)
            .map(|project| project.root_path.clone())
    }

    fn active_catalog_clone(&self) -> BrukCatalog {
        let name = self.selected_project_name();
        self.catalogs
            .get(&name)
            .cloned()
            .unwrap_or_else(BrukCatalog::builtin_only)
    }

    fn reload_catalog_for_project(&mut self, project_name: &str) {
        if self.demo_mode {
            return;
        }
        let catalog = self
            .project_root_by_name(project_name)
            .map(|root| load_catalog(self.home.as_ref(), &root))
            .unwrap_or_else(BrukCatalog::builtin_only);
        let overview = ProjectWorkflowsOverview::from_catalog(project_name, &catalog);
        if let Some(settings) = self.project_settings.get_mut(project_name) {
            sync_project_workflows_from_catalog(settings, &catalog);
        }
        self.workflows_overviews
            .insert(project_name.to_owned(), overview);
        self.catalogs.insert(project_name.to_owned(), catalog);
        self.sync_selected_workflow_overview_for_active_project();
    }

    fn project_id_by_name(&self, name: &str) -> Option<ProjectId> {
        if self.demo_mode || self.using_sample_projects {
            return None;
        }
        self.factory
            .projects()
            .iter()
            .find(|project| project.name == name)
            .map(|project| project.id)
    }

    fn persist_project_settings_by_name(&mut self, project_name: &str) {
        let Some(project_id) = self.project_id_by_name(project_name) else {
            return;
        };
        let Some(settings) = self.project_settings.get(project_name).cloned() else {
            return;
        };
        if let Some(db_arc) = self.db.as_ref()
            && let Ok(db) = db_arc.lock()
            && let Err(err) = db.save_project_settings(project_id, &settings)
        {
            log::error!(
                target: "bruk::storage",
                "db.save_project_settings_failed project_id={project_id} project={project_name:?} err={err}"
            );
            self.project_settings_error = Some(format!("Could not save project settings: {err}"));
        }
    }

    fn persist_active_project_settings(&mut self) {
        let name = self.selected_project_name();
        self.persist_project_settings_by_name(&name);
    }

    fn persist_work_queue_by_name(&mut self, project_name: &str) {
        let Some(project_id) = self.project_id_by_name(project_name) else {
            return;
        };
        let Some(queue) = self.work_queues.get(project_name).cloned() else {
            return;
        };
        if let Some(db_arc) = self.db.as_ref()
            && let Ok(db) = db_arc.lock()
            && let Err(err) = db.save_project_work_queue(project_id, &queue)
        {
            log::error!(
                target: "bruk::storage",
                "db.save_work_queue_failed project_id={project_id} project={project_name:?} err={err}"
            );
        }
    }

    fn persist_active_work_queue(&mut self) {
        let name = self.selected_project_name();
        self.persist_work_queue_by_name(&name);
    }

    fn flush_secret_resolver_updates(&mut self) {
        let last_used = self.secret_resolver.drain_last_used_updates();
        let needs_value = self.secret_resolver.drain_needs_value_downgrades();
        if last_used.is_empty() && needs_value.is_empty() {
            return;
        }
        if !self.demo_mode
            && let Some(db_arc) = self.db.as_ref()
            && let Ok(db) = db_arc.lock()
        {
            let _ = db.flush_secret_last_used(&last_used);
            let _ = db.mark_secrets_needs_value(&needs_value);
            if let Ok(records) = db.list_secrets() {
                self.secrets = records;
            }
            if let Ok(snapshot) = db.build_secrets_metadata_snapshot(&self.preferences) {
                self.secret_resolver.update_snapshot(snapshot);
            }
        }
    }

    fn refresh_secrets_state(&mut self) {
        self.secrets_store_status = self.secret_store.status();
        if self.demo_mode {
            return;
        }
        let last_used = self.secret_resolver.drain_last_used_updates();
        let needs_value = self.secret_resolver.drain_needs_value_downgrades();
        if let Some(db_arc) = self.db.as_ref()
            && let Ok(db) = db_arc.lock()
        {
            let _ = db.flush_secret_last_used(&last_used);
            let _ = db.mark_secrets_needs_value(&needs_value);
            if let Ok(records) = db.list_secrets() {
                self.secrets = records;
            }
            if let Ok(snapshot) = db.build_secrets_metadata_snapshot(&self.preferences) {
                self.secret_resolver.update_snapshot(snapshot);
            }
        }
    }

    fn ensure_chat_secrets_cache(&mut self, chat_id: &str) -> Arc<Mutex<ChatSecretsCache>> {
        self.chat_secrets_caches
            .entry(chat_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(ChatSecretsCache::new())))
            .clone()
    }

    fn ensure_workflow_run_mut(
        &mut self,
        project_name: &str,
        workflow_name: &str,
    ) -> &mut WorkflowRunViewData {
        let key = (project_name.to_owned(), workflow_name.to_owned());
        let demo = self.demo_mode;
        self.workflow_runs_detail.entry(key).or_insert_with(|| {
            if demo {
                WorkflowRunViewData::for_workflow(project_name, workflow_name)
            } else {
                WorkflowRunViewData::empty(project_name, workflow_name)
            }
        })
    }

    fn active_workflow_run_clone(&self) -> WorkflowRunViewData {
        let (ref project, ref workflow) = self.active_workflow_run_key;
        self.workflow_runs_detail
            .get(&self.active_workflow_run_key)
            .cloned()
            .unwrap_or_else(|| {
                if self.demo_mode {
                    WorkflowRunViewData::for_workflow(project, workflow)
                } else {
                    WorkflowRunViewData::empty(project, workflow)
                }
            })
    }

    fn ensure_workflow_editor_mut(&mut self) -> &mut WorkflowEditorDocument {
        let key = self.active_workflow_editor_key.clone();
        let demo = self.demo_mode;
        self.workflow_editors.entry(key.clone()).or_insert_with(|| {
            if demo {
                WorkflowEditorDocument::for_workflow_name(&key.0, &key.1)
            } else {
                WorkflowEditorDocument::empty(&key.0, &key.1)
            }
        })
    }

    fn active_workflow_editor_clone(&self) -> WorkflowEditorDocument {
        let (ref project, ref wf_id) = self.active_workflow_editor_key;
        self.workflow_editors
            .get(&self.active_workflow_editor_key)
            .cloned()
            .unwrap_or_else(|| {
                if self.demo_mode {
                    WorkflowEditorDocument::for_workflow_name(project, wf_id)
                } else {
                    WorkflowEditorDocument::empty(project, wf_id)
                }
            })
    }

    fn sync_selected_issue_for_active_project(&mut self) {
        let name = self.selected_project_name();
        let selected_issue = self.work_queue_selected_issue;
        let queue = self.ensure_queue_mut(&name);
        let valid = selected_issue.is_some_and(|num| queue.item(num).is_some());
        if !valid {
            let default_issue = queue.default_selected_issue();
            self.work_queue_selected_issue = default_issue;
        }
    }

    fn sync_inbox_selection_and_drafts(&mut self) {
        let valid_pending = self
            .inbox_selected_id
            .as_deref()
            .and_then(|id| self.inbox.pending_by_id(id))
            .is_some_and(|req| self.inbox_filter.matches(req.kind));
        if !valid_pending {
            let next = self
                .inbox
                .pending_oldest_first(self.inbox_filter)
                .last()
                .map(|r| r.id.clone());
            self.inbox_selected_id = next;
        }
        let (cmd, pattern, choice) =
            draft_defaults_for_selection(&self.inbox, self.inbox_selected_id.as_deref());
        self.inbox_editing_command = false;
        self.inbox_edited_command = cmd;
        self.inbox_always_allow_open = false;
        self.inbox_always_allow_pattern = pattern;
        self.inbox_note_draft.clear();
        self.inbox_question_choice = choice;
    }

    fn persist_preferences(&mut self) {
        let to_save = if self.demo_mode {
            let real = self
                .saved_real_preferences
                .get_or_insert_with(AppPreferences::empty);
            real.general = self.preferences.general.clone();
            real.appearance = self.preferences.appearance.clone();
            real.claude_spawn_settings = self.preferences.claude_spawn_settings.clone();
            real.codex_spawn_settings = self.preferences.codex_spawn_settings.clone();
            real.machines = self.preferences.machines.clone();
            real.advanced = self.preferences.advanced.clone();
            real.secrets = self.preferences.secrets.clone();
            real.demo_mode = false;
            let mut persisted = real.clone();
            persisted.demo_mode = true;
            persisted
        } else {
            self.preferences.clone()
        };

        if let Some(db_arc) = self.db.as_ref()
            && let Ok(db) = db_arc.lock()
            && let Err(err) = db.save_preferences(&to_save)
        {
            log::error!(target: "bruk::storage", "db.save_preferences_failed err={err}");
            self.preferences_error = Some(format!("Could not save preferences: {err}"));
        }
        self.refresh_secrets_state();
    }

    fn refresh_claude_status(&mut self, explicit_test: bool) -> ClaudeCliProbe {
        self.claude_probe_seq.fetch_add(1, Ordering::AcqRel);
        let path_env = self.claude_path_lookup.resolve_path_env();
        let probe = probe_claude_cli(
            self.preferences.claude.custom_cli_path.as_deref(),
            path_env.as_deref(),
        );
        self.preferences.apply_claude_probe(&probe, explicit_test);
        probe
    }

    fn refresh_codex_status(&mut self, explicit_test: bool) -> Option<CodexCliProbe> {
        self.codex_probe_seq.fetch_add(1, Ordering::AcqRel);
        let custom_path = self.preferences.codex.as_ref()?.custom_cli_path.clone();
        let path_env = self.claude_path_lookup.resolve_path_env();
        let probe = probe_codex_cli(custom_path.as_deref(), path_env.as_deref());
        self.preferences.apply_codex_probe(&probe, explicit_test);
        Some(probe)
    }

    fn refresh_antigravity_status(&mut self, explicit_test: bool) -> Option<AntigravityCliProbe> {
        self.antigravity_probe_seq.fetch_add(1, Ordering::AcqRel);
        let custom_path = self
            .preferences
            .antigravity
            .as_ref()?
            .custom_cli_path
            .clone();
        let path_env = self.claude_path_lookup.resolve_path_env();
        let probe = probe_antigravity_cli(custom_path.as_deref(), path_env.as_deref());
        self.preferences
            .apply_antigravity_probe(&probe, explicit_test);
        Some(probe)
    }
}

fn draft_defaults_for_selection(
    inbox: &InboxState,
    selected_id: Option<&str>,
) -> (String, String, Option<String>) {
    let Some(req) = selected_id.and_then(|id| inbox.pending_by_id(id)) else {
        return (String::new(), String::new(), None);
    };
    match &req.body {
        AttentionRequestBody::Permission(detail) => (
            detail.command.clone(),
            detail.always_allow_pattern.clone(),
            None,
        ),
        AttentionRequestBody::Question(detail) => {
            let recommended = detail
                .options
                .iter()
                .find(|opt| opt.recommended)
                .or_else(|| detail.options.first())
                .map(|opt| opt.label.clone());
            (String::new(), String::new(), recommended)
        }
        _ => (String::new(), String::new(), None),
    }
}

fn default_selected_provider_id(prefs: &AppPreferences) -> String {
    if prefs.openrouter_key("openrouter-acme").is_some() {
        "openrouter-acme".to_owned()
    } else if let Some(first) = prefs.openrouter_keys.first() {
        first.id.clone()
    } else {
        prefs.claude.id.clone()
    }
}

fn landing_to_active_view(landing: DefaultLandingView) -> ActiveView {
    match landing {
        DefaultLandingView::Dashboard => ActiveView::Dashboard,
        DefaultLandingView::Inbox => ActiveView::Inbox,
        DefaultLandingView::Machines if factory_flags::machines() => ActiveView::Machines,
        // A saved Machines landing view from a build that had the flag on.
        DefaultLandingView::Machines => ActiveView::Dashboard,
        DefaultLandingView::Workspace => ActiveView::Workspace,
    }
}

fn apply_appearance_settings(appearance: &AppearanceSettings) {
    let color_scheme = match appearance.mode {
        AppearanceMode::System => Appearance::System,
        AppearanceMode::Light => Appearance::Light,
        AppearanceMode::Dark => Appearance::Dark,
    };
    nsh_core::element::set_color_scheme(color_scheme);
    factory_fonts::apply_font_tokens(appearance);
}

fn state() -> Signal<AppState> {
    global::<AppState>()
}

/// Initializes the per-surface store from the Bruk folder configured on this machine.
pub fn init_default() {
    let init_span = factory_logging::boot_log::span("boot.store.init_default");
    let appearance = state().with_untracked(|s| s.preferences.appearance.clone());
    {
        let span = factory_logging::boot_log::span("boot.store.apply_appearance");
        apply_appearance_settings(&appearance);
        span.finish(format!("mode={:?}", appearance.mode));
    }
    sweep_secret_tombstones_async();
    probe_providers_async();
    maybe_check_for_updates_on_launch();
    init_span.finish("");
}

/// Initializes the per-surface store against an explicit SQLite DB path.
pub fn init_with_database_path(path: &Path) {
    init_with_database_path_and_claude_path(path, ClaudePathLookup::SystemPath);
}

/// Initializes the per-surface store against an explicit SQLite DB path and Claude `PATH` lookup.
/// Uses an isolated in-memory secret store so tests and custom DB paths never touch the OS keychain.
pub fn init_with_database_path_and_claude_path(path: &Path, claude_path_lookup: ClaudePathLookup) {
    let loaded = AppState::from_database_result_with_path(
        ProjectDatabase::open(path),
        Some(BrukHome::with_database_path(path)),
        claude_path_lookup,
        Arc::new(MemoryStore::new()),
        Vec::new(),
    );
    install_state(loaded);
}

/// Initializes the per-surface store by resolving the Bruk folder from an explicit environment
/// (first-launch setup runs when no folder is configured there).
pub fn init_with_home_environment(
    environment: HomeEnvironment,
    claude_path_lookup: ClaudePathLookup,
) {
    install_state(AppState::from_environment(
        Ok(environment),
        claude_path_lookup,
        Arc::new(MemoryStore::new()),
    ));
}

fn install_state(loaded: AppState) {
    let appearance = loaded.preferences.appearance.clone();
    state().set(loaded);
    apply_appearance_settings(&appearance);
    sweep_secret_tombstones_async();
    probe_providers_async();
}

/// Retries outstanding secret deletion tombstones on a background thread so Keychain I/O
/// never blocks application boot.
fn sweep_secret_tombstones_async() {
    let job = state().with_untracked(|s| {
        if s.demo_mode || s.db_path_change_pending.is_some() {
            return None;
        }
        s.db.as_ref()
            .map(|db| (Arc::clone(db), Arc::clone(&s.secret_store)))
    });
    let Some((db, secret_store)) = job else {
        return;
    };
    let boot_span = factory_logging::boot_log::span("boot.async.sweep_secret_tombstones");
    Owner::detached().run(|| {
        spawn(async move {
            let swept = spawn_blocking_task(move || {
                db.lock()
                    .ok()
                    .and_then(|guard| {
                        guard
                            .sweep_secret_tombstones(secret_store.as_ref(), false)
                            .ok()
                    })
                    .unwrap_or(0)
            })
            .await;
            boot_span.finish(format!("swept={swept}"));
        });
    });
}

/// Probes all configured provider CLIs in parallel on background threads and chains into each
/// authenticated provider's live usage/model refresh without blocking GUI startup.
fn probe_providers_async() {
    let from_boot = factory_logging::boot_log::is_booting();
    probe_claude_async(from_boot);
    probe_codex_async(from_boot);
    probe_antigravity_async(from_boot);
}

fn probe_claude_async(from_boot: bool) {
    let job = state().with_untracked(|s| {
        if s.demo_mode {
            return None;
        }
        let expected_seq = s.claude_probe_seq.fetch_add(1, Ordering::AcqRel) + 1;
        Some((
            expected_seq,
            s.preferences.claude.custom_cli_path.clone(),
            s.claude_path_lookup.resolve_path_env(),
        ))
    });
    let Some((expected_seq, custom_path, path_env)) = job else {
        return;
    };
    let boot_span = factory_logging::boot_log::async_span("boot.async.probe_claude_cli", from_boot);
    Owner::detached().run(|| {
        spawn(async move {
            let probe = spawn_blocking_task(move || {
                probe_claude_cli(custom_path.as_deref(), path_env.as_deref())
            })
            .await;
            let outcome = claude_probe_outcome(&probe);
            let mut should_refresh_details = false;
            state().update(|s| {
                if s.demo_mode
                    || s.claude_probe_seq.load(Ordering::Acquire) != expected_seq
                    || s.claude_sign_in_in_progress
                {
                    return;
                }
                s.preferences.apply_claude_probe(&probe, false);
                should_refresh_details = matches!(probe, ClaudeCliProbe::Authenticated { .. });
            });
            boot_span.finish(format!("outcome={outcome}"));
            if should_refresh_details {
                refresh_claude_details_async_inner(from_boot);
            }
        });
    });
}

fn probe_codex_async(from_boot: bool) {
    let job = state().with_untracked(|s| {
        if s.demo_mode {
            return None;
        }
        let codex = s.preferences.codex.as_ref()?;
        let expected_seq = s.codex_probe_seq.fetch_add(1, Ordering::AcqRel) + 1;
        Some((
            expected_seq,
            codex.custom_cli_path.clone(),
            s.claude_path_lookup.resolve_path_env(),
        ))
    });
    let Some((expected_seq, custom_path, path_env)) = job else {
        return;
    };
    let boot_span = factory_logging::boot_log::async_span("boot.async.probe_codex_cli", from_boot);
    Owner::detached().run(|| {
        spawn(async move {
            let probe = spawn_blocking_task(move || {
                probe_codex_cli(custom_path.as_deref(), path_env.as_deref())
            })
            .await;
            let outcome = codex_probe_outcome(&probe);
            let mut should_refresh_details = false;
            state().update(|s| {
                if s.demo_mode
                    || s.codex_probe_seq.load(Ordering::Acquire) != expected_seq
                    || s.codex_sign_in_in_progress
                    || s.preferences.codex.is_none()
                {
                    return;
                }
                s.preferences.apply_codex_probe(&probe, false);
                should_refresh_details = matches!(probe, CodexCliProbe::Authenticated { .. });
            });
            boot_span.finish(format!("outcome={outcome}"));
            if should_refresh_details {
                refresh_codex_details_async_inner(from_boot);
            }
        });
    });
}

fn probe_antigravity_async(from_boot: bool) {
    let job = state().with_untracked(|s| {
        if s.demo_mode {
            return None;
        }
        let antigravity = s.preferences.antigravity.as_ref()?;
        let expected_seq = s.antigravity_probe_seq.fetch_add(1, Ordering::AcqRel) + 1;
        Some((
            expected_seq,
            antigravity.custom_cli_path.clone(),
            s.claude_path_lookup.resolve_path_env(),
        ))
    });
    let Some((expected_seq, custom_path, path_env)) = job else {
        return;
    };
    let boot_span =
        factory_logging::boot_log::async_span("boot.async.probe_antigravity_cli", from_boot);
    Owner::detached().run(|| {
        spawn(async move {
            let probe = spawn_blocking_task(move || {
                probe_antigravity_cli(custom_path.as_deref(), path_env.as_deref())
            })
            .await;
            let outcome = antigravity_probe_outcome(&probe);
            let mut should_refresh_usage = false;
            state().update(|s| {
                if s.demo_mode
                    || s.antigravity_probe_seq.load(Ordering::Acquire) != expected_seq
                    || s.antigravity_sign_in_in_progress
                    || s.preferences.antigravity.is_none()
                {
                    return;
                }
                s.preferences.apply_antigravity_probe(&probe, false);
                should_refresh_usage = matches!(probe, AntigravityCliProbe::Authenticated { .. });
            });
            boot_span.finish(format!("outcome={outcome}"));
            if should_refresh_usage {
                // `probe_antigravity_cli` already ran `agy models`; only fetch usage quota here.
                refresh_antigravity_usage_async_inner(from_boot);
            }
        });
    });
}

// --- Bruk folder ------------------------------------------------------------------------

pub fn needs_home_setup() -> bool {
    state().with(|s| s.home_setup.is_some())
}

pub fn home_setup_root_input() -> String {
    state().with(|s| {
        s.home_setup
            .as_ref()
            .map(|d| d.root_input.clone())
            .unwrap_or_default()
    })
}

pub fn set_home_setup_root_input(value: &str) {
    state().update(|s| {
        if let Some(draft) = s.home_setup.as_mut() {
            draft.root_input = value.to_owned();
            draft.error = None;
        }
    });
}

/// Whether the folder typed in setup already holds a Bruk database (it will be reused as-is).
pub fn home_setup_target_has_database() -> bool {
    state().with(|s| {
        s.home_setup.as_ref().is_some_and(|d| {
            let trimmed = d.root_input.trim();
            !trimmed.is_empty() && BrukHome::at(expand_tilde(trimmed)).is_initialized()
        })
    })
}

pub fn home_setup_legacy_database() -> Option<String> {
    state().with(|s| {
        s.home_setup
            .as_ref()
            .and_then(|d| d.legacy_database.as_deref())
            .map(display_with_tilde)
    })
}

pub fn home_setup_import_legacy() -> bool {
    state().with(|s| s.home_setup.as_ref().is_some_and(|d| d.import_legacy))
}

pub fn toggle_home_setup_import_legacy() {
    state().update(|s| {
        if let Some(draft) = s.home_setup.as_mut() {
            draft.import_legacy = !draft.import_legacy;
        }
    });
}

pub fn home_setup_error() -> Option<String> {
    state().with(|s| s.home_setup.as_ref().and_then(|d| d.error.clone()))
}

/// Creates (or adopts) the chosen Bruk folder, then loads the app from it.
pub fn complete_home_setup() {
    let Some((draft, claude_path_lookup, secret_store)) = state().with_untracked(|s| {
        s.home_setup.clone().map(|draft| {
            (
                draft,
                s.claude_path_lookup.clone(),
                Arc::clone(&s.secret_store),
            )
        })
    }) else {
        return;
    };
    let legacy_paths = draft.environment.legacy_database_paths.clone();
    match draft.environment.set_up_with_importer(
        &draft.root_input,
        draft.import_legacy,
        ProjectDatabase::copy_database,
    ) {
        Ok(setup) => {
            let root_display = display_with_tilde(setup.home.root());
            log::info!(
                target: "bruk::app",
                "home_setup.completed root={root_display} adopted_existing={} imported_legacy={}",
                setup.adopted_existing,
                setup.imported_from.is_some()
            );
            let mut loaded =
                AppState::from_home(setup.home, claude_path_lookup, secret_store, legacy_paths);
            if loaded.status_message.is_none() {
                loaded.status_message = Some(
                    match (&setup.imported_from, setup.adopted_existing) {
                        (Some(legacy), _) => format!(
                            "Bruk folder ready at {root_display} · imported projects and preferences from {}",
                            display_with_tilde(legacy)
                        ),
                        (None, true) => format!("Using existing Bruk folder at {root_display}"),
                        (None, false) => format!("Bruk folder ready at {root_display}"),
                    },
                );
            }
            install_state(loaded);
        }
        Err(err) => {
            log::warn!(target: "bruk::app", "home_setup.failed err={err}");
            state().update(|s| {
                if let Some(draft) = s.home_setup.as_mut() {
                    draft.error = Some(err.to_string());
                }
            });
        }
    }
}

/// Paths inside the active Bruk folder: (folder, database, cloned projects, run workspaces).
pub fn home_paths_display() -> Option<(String, String, String, String)> {
    state().with(|s| {
        s.home.as_ref().map(|home| {
            (
                display_with_tilde(home.root()),
                display_with_tilde(home.database_path()),
                display_with_tilde(&home.projects_dir()),
                display_with_tilde(&home.runs_dir()),
            )
        })
    })
}

pub fn home_root() -> Option<std::path::PathBuf> {
    state().with_untracked(|s| s.home.as_ref().map(|home| home.root().to_path_buf()))
}

pub fn home_root_display() -> String {
    state().with(|s| {
        s.home
            .as_ref()
            .map(|home| display_with_tilde(home.root()))
            .unwrap_or_else(|| "Unavailable".to_owned())
    })
}

/// Whether a project's clone lives in the Bruk folder (as opposed to an external clone).
pub fn project_is_managed(id: ProjectId) -> bool {
    state().with(|s| {
        let Some(home) = s.home.as_ref() else {
            return false;
        };
        s.factory
            .project(id)
            .is_some_and(|project| home.is_managed_project(&project.root_path))
    })
}

// --- Cloning projects into the Bruk folder ----------------------------------------------

pub fn clear_registration_error() {
    state().update(|s| s.registration_error = None);
}

pub fn clone_source_input() -> String {
    state().with(|s| s.clone_source_input.clone())
}

fn suggested_clone_name(source: &str) -> String {
    parse_clone_source(source)
        .map(|parsed| parsed.suggested_name)
        .unwrap_or_default()
}

/// Updates the repository field, keeping the project name in step with it until the user
/// types a name of their own.
pub fn set_clone_source_input(value: &str) {
    state().update(|s| {
        let name_follows_source = s.clone_name_input.trim().is_empty()
            || s.clone_name_input == suggested_clone_name(&s.clone_source_input);
        s.clone_source_input = value.to_owned();
        if name_follows_source {
            s.clone_name_input = suggested_clone_name(value);
        }
    });
}

pub fn clone_name_input() -> String {
    state().with(|s| s.clone_name_input.clone())
}

pub fn set_clone_name_input(value: &str) {
    state().update(|s| s.clone_name_input = value.to_owned());
}

pub fn clone_in_progress() -> bool {
    state().with(|s| s.clone_in_progress)
}

pub fn clone_destination_caption() -> String {
    state().with(|s| {
        let Some(projects_dir) = s.home.as_ref().and(s.projects_dir()) else {
            return "The Bruk folder is not available.".to_owned();
        };
        let name = s.clone_name_input.trim();
        if name.is_empty() {
            format!("Clones into {}/", display_with_tilde(&projects_dir))
        } else {
            format!(
                "Clones into {}",
                display_with_tilde(&projects_dir.join(project_dir_name(name)))
            )
        }
    })
}

/// Clones the repository typed in the Clone form into the default projects directory
/// (`<Bruk folder>/projects` unless changed in Preferences) on a background thread, then
/// registers it with the branch the clone checked out.
pub fn clone_and_register_project() {
    let mut job = None;
    state().update(|s| {
        if s.clone_in_progress {
            return;
        }
        let Some(home) = s.home.clone() else {
            s.registration_error = Some("The Bruk folder is not available.".to_owned());
            return;
        };
        let source = match parse_clone_source(&s.clone_source_input) {
            Ok(source) => source,
            Err(err) => {
                s.registration_error = Some(err.to_string());
                return;
            }
        };
        let name = match s.clone_name_input.trim() {
            "" => source.suggested_name.clone(),
            typed => typed.to_owned(),
        };
        let destination = s
            .preferences
            .general
            .projects_dir(&home)
            .join(project_dir_name(&name));
        s.clone_in_progress = true;
        s.registration_error = None;
        s.status_message = Some(format!(
            "Cloning {} into {}…",
            source.url,
            display_with_tilde(&destination)
        ));
        job = Some((name, source.url, destination));
    });

    let Some((name, url, destination)) = job else {
        return;
    };
    Owner::detached().run(|| {
        spawn(async move {
            let result = spawn_blocking_task(move || clone_repository(&url, &destination)).await;
            state().update(|s| s.clone_in_progress = false);
            match result {
                Ok(cloned) => {
                    let path = cloned.path.display().to_string();
                    if register_project_with_branch(&name, &path, &cloned.default_branch) {
                        state().update(|s| {
                            s.clone_source_input.clear();
                            s.clone_name_input.clear();
                        });
                    }
                }
                Err(err) => state().update(|s| {
                    let message = err.to_string();
                    s.registration_error = Some(message.clone());
                    s.status_message = Some(message);
                }),
            }
        });
    });
}

// --- Creating new local repositories ----------------------------------------------------

pub fn create_name_input() -> String {
    state().with(|s| s.create_name_input.clone())
}

pub fn set_create_name_input(value: &str) {
    state().update(|s| s.create_name_input = value.to_owned());
}

pub fn create_location_input() -> String {
    state().with(|s| s.create_location_input.clone())
}

pub fn set_create_location_input(value: &str) {
    state().update(|s| s.create_location_input = value.to_owned());
}

pub fn create_branch_input() -> String {
    state().with(|s| s.create_branch_input.clone())
}

pub fn set_create_branch_input(value: &str) {
    state().update(|s| s.create_branch_input = value.to_owned());
}

pub fn create_readme() -> bool {
    state().with(|s| s.create_readme)
}

pub fn toggle_create_readme() {
    state().update(|s| s.create_readme = !s.create_readme);
}

pub fn create_in_progress() -> bool {
    state().with(|s| s.create_in_progress)
}

/// Directory to open the folder picker at when choosing where to create a new local repository.
pub fn create_browse_start_dir() -> Option<std::path::PathBuf> {
    state().with_untracked(|s| {
        let location = s.create_location_input.trim();
        if !location.is_empty() {
            let expanded = expand_tilde(location);
            if expanded.is_dir() {
                return Some(expanded);
            }
        }
        s.projects_dir().filter(|dir| dir.is_dir())
    })
}

pub fn create_destination_caption() -> String {
    state().with(|s| {
        let location = s.create_location_input.trim();
        let parent = if location.is_empty() {
            let Some(projects_dir) = s.home.as_ref().and(s.projects_dir()) else {
                return "The Bruk folder is not available.".to_owned();
            };
            projects_dir
        } else {
            let expanded = expand_tilde(location);
            if !expanded.is_absolute() {
                return "Enter an absolute folder path, or leave blank for the default folder."
                    .to_owned();
            }
            expanded
        };
        let name = s.create_name_input.trim();
        if name.is_empty() {
            format!("Creates in {}/", display_with_tilde(&parent))
        } else {
            format!(
                "Creates {}",
                display_with_tilde(&parent.join(project_dir_name(name)))
            )
        }
    })
}

/// Creates and initializes a new local git repository on a background thread, then registers
/// it as a project with the initialized default branch.
pub fn create_local_repository_project() {
    let mut job = None;
    state().update(|s| {
        if s.create_in_progress {
            return;
        }
        let name = s.create_name_input.trim().to_owned();
        if name.is_empty() {
            s.registration_error = Some("enter a project name".to_owned());
            return;
        }
        let default_projects_dir = match s.home.as_ref().and(s.projects_dir()) {
            Some(dir) => dir,
            None if !s.create_location_input.trim().is_empty() => std::path::PathBuf::new(),
            None => {
                s.registration_error = Some("The Bruk folder is not available.".to_owned());
                return;
            }
        };
        let destination = match resolve_new_repository_destination(
            &s.create_location_input,
            &default_projects_dir,
            &name,
        ) {
            Ok(destination) => destination,
            Err(err) => {
                s.registration_error = Some(err.to_string());
                return;
            }
        };
        let default_branch = match validate_branch_name(&s.create_branch_input) {
            Ok(branch) => branch,
            Err(err) => {
                s.registration_error = Some(err.to_string());
                return;
            }
        };
        if !s.using_sample_projects
            && s.factory
                .projects()
                .iter()
                .any(|project| project.root_path == destination)
        {
            s.registration_error = Some(format!(
                "A project is already registered at '{}'.",
                destination.display()
            ));
            return;
        }
        let options = InitRepositoryOptions {
            default_branch,
            create_readme: s.create_readme,
            project_name: name.clone(),
            initial_commit: true,
        };
        s.create_in_progress = true;
        s.registration_error = None;
        s.status_message = Some(format!(
            "Creating repository at {}…",
            display_with_tilde(&destination)
        ));
        job = Some((name, destination, options));
    });

    let Some((name, destination, options)) = job else {
        return;
    };
    Owner::detached().run(|| {
        spawn(async move {
            let result =
                spawn_blocking_task(move || init_local_repository(&destination, &options)).await;
            state().update(|s| s.create_in_progress = false);
            match result {
                Ok(initialized) => {
                    let path = initialized.path.display().to_string();
                    if register_project_with_branch(&name, &path, &initialized.default_branch) {
                        state().update(|s| {
                            s.create_name_input.clear();
                            s.create_location_input.clear();
                            s.create_branch_input = "main".to_owned();
                            s.create_readme = true;
                        });
                    }
                }
                Err(err) => state().update(|s| {
                    let message = err.to_string();
                    s.registration_error = Some(message.clone());
                    s.status_message = Some(message);
                }),
            }
        });
    });
}

pub fn demo_mode() -> bool {
    state().with(|s| s.demo_mode)
}

pub fn set_demo_mode(enabled: bool) {
    let mut appearance_to_apply = None;
    state().update(|s| {
        if s.demo_mode == enabled {
            return;
        }
        let db = s.db.clone();
        let db_path_display = s.db_path_display.clone();
        let home = s.home.clone();
        let prev_active_view = s.active_view;
        let prev_sidebar_visible = s.sidebar_visible;
        let prev_prefs_open = s.preferences_open;
        let prev_prefs_tab = s.preferences_tab;
        let claude_path_lookup = s.claude_path_lookup.clone();
        let secret_store = Arc::clone(&s.secret_store);
        let legacy_database_paths = s.legacy_database_paths.clone();

        let persisted_projects = db
            .as_ref()
            .and_then(|arc| arc.lock().ok())
            .and_then(|guard| guard.list_projects().ok())
            .unwrap_or_default();

        let base_prefs = if enabled {
            let mut real = s.preferences.clone();
            real.demo_mode = false;
            real
        } else {
            let mut real = s
                .saved_real_preferences
                .clone()
                .unwrap_or_else(AppPreferences::empty);
            real.general = s.preferences.general.clone();
            real.appearance = s.preferences.appearance.clone();
            real.secrets = s.preferences.secrets.clone();
            real.demo_mode = false;
            real
        };

        let status = if enabled {
            Some("Demo Mode enabled — showing sample data".to_owned())
        } else {
            Some("Demo Mode disabled".to_owned())
        };
        log::info!(target: "bruk::app", "demo_mode.changed enabled={enabled}");

        let mut next = AppState::build_for_mode(
            db.clone(),
            db_path_display,
            persisted_projects,
            base_prefs.clone(),
            enabled,
            status,
            None,
            claude_path_lookup,
            home,
            secret_store,
            legacy_database_paths,
        );
        next.sidebar_visible = prev_sidebar_visible;
        next.preferences_open = prev_prefs_open;
        next.preferences_tab = prev_prefs_tab;

        let has_projects = !next.factory.projects().is_empty();
        next.active_view = match prev_active_view {
            ActiveView::Dashboard
            | ActiveView::Inbox
            | ActiveView::Machines
            | ActiveView::RegisterProject => prev_active_view,
            ActiveView::WorkQueue
            | ActiveView::Workflows
            | ActiveView::ProjectSettings
            | ActiveView::Workspace
                if has_projects =>
            {
                prev_active_view
            }
            _ => ActiveView::Dashboard,
        };

        if let Some(db_arc) = db.as_ref()
            && let Ok(guard) = db_arc.lock()
        {
            let mut to_save = base_prefs;
            to_save.demo_mode = enabled;
            let _ = guard.save_preferences(&to_save);
        }

        appearance_to_apply = Some(next.preferences.appearance.clone());
        *s = next;
    });
    if let Some(appearance) = appearance_to_apply {
        apply_appearance_settings(&appearance);
    }
    if !enabled {
        sweep_secret_tombstones_async();
        probe_providers_async();
    }
}

pub fn toggle_demo_mode() {
    let current = state().with_untracked(|s| s.demo_mode);
    set_demo_mode(!current);
}

pub fn sidebar_visible() -> bool {
    state().with(|s| s.sidebar_visible)
}

pub fn toggle_sidebar() {
    state().update(|s| {
        s.sidebar_visible = !s.sidebar_visible;
    });
}

pub fn active_view() -> ActiveView {
    state().with(|s| s.active_view)
}

pub fn open_dashboard() {
    state().update(|s| {
        s.active_view = ActiveView::Dashboard;
        s.status_message = None;
    });
}

pub fn open_inbox() {
    state().update(|s| {
        s.menubar_popover_open = false;
        s.active_view = ActiveView::Inbox;
        s.inbox_waiting_rules_open = false;
        s.inbox_feedback = None;
        s.status_message = None;
        s.sync_inbox_selection_and_drafts();
    });
    main_window().focus();
}

pub fn open_inbox_for_request(id: &str) {
    let target = id.to_owned();
    state().update(|s| {
        s.menubar_popover_open = false;
        let mut selected_answered = false;
        if let Some(req) = s.inbox.pending_by_id(&target) {
            if !s.inbox_filter.matches(req.kind) {
                s.inbox_filter = InboxFilter::All;
            }
            s.inbox_selected_id = Some(target);
        } else if s.inbox.answered_by_id(&target).is_some() {
            s.inbox_answered_expanded = true;
            s.inbox_selected_id = Some(target);
            selected_answered = true;
        }
        s.active_view = ActiveView::Inbox;
        s.inbox_waiting_rules_open = false;
        s.inbox_feedback = None;
        s.status_message = None;
        if !selected_answered {
            s.sync_inbox_selection_and_drafts();
        }
    });
    main_window().focus();
}

pub fn menubar_popover_open() -> bool {
    state().with(|s| s.menubar_popover_open)
}

pub fn menubar_anchor_bounds() -> Option<Bounds<Points>> {
    state().with(|s| s.menubar_anchor_bounds)
}

pub fn open_menubar_popover(bounds: Option<Bounds<Points>>) {
    state().update(|s| {
        s.menubar_anchor_bounds = bounds;
        s.menubar_popover_open = true;
    });
}

pub fn toggle_menubar_popover(bounds: Option<Bounds<Points>>) {
    state().update(|s| {
        if s.menubar_popover_open {
            s.menubar_popover_open = false;
        } else {
            s.menubar_anchor_bounds = bounds;
            s.menubar_popover_open = true;
        }
    });
}

pub fn close_menubar_popover() {
    state().update(|s| {
        s.menubar_popover_open = false;
    });
}

pub fn open_machines() {
    if !factory_flags::machines() {
        return;
    }
    state().update(|s| {
        s.active_view = ActiveView::Machines;
        s.status_message = None;
    });
}

pub fn open_register_project() {
    state().update(|s| {
        s.active_view = ActiveView::RegisterProject;
        s.registration_error = None;
    });
}

pub fn open_workspace() {
    state().update(|s| {
        if s.selected_project.is_none() {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
        }
        s.active_view = ActiveView::Workspace;
    });
}

pub fn db_path_display() -> String {
    state().with(|s| s.db_path_display.clone())
}

pub fn registration_error() -> Option<String> {
    state().with(|s| s.registration_error.clone())
}

pub fn project_ids() -> Vec<ProjectId> {
    state().with(|s| s.factory.projects().iter().map(|p| p.id).collect())
}

pub fn recent_projects() -> Vec<(ProjectId, String)> {
    state().with(|s| {
        s.factory
            .projects()
            .iter()
            .map(|p| (p.id, p.name.clone()))
            .collect()
    })
}

pub fn tracked_project_ids() -> Vec<ProjectId> {
    state().with(|s| {
        if s.using_sample_projects {
            Vec::new()
        } else {
            s.factory.projects().iter().map(|p| p.id).collect()
        }
    })
}

pub fn project_by_id(id: ProjectId) -> Option<Project> {
    state().with(|s| s.factory.project(id).cloned())
}

pub fn selected_project_id() -> Option<ProjectId> {
    state().with(|s| s.selected_project)
}

pub fn selected_project() -> Option<Project> {
    state().with(|s| {
        s.selected_project
            .and_then(|id| s.factory.project(id).cloned())
    })
}

pub fn select_project(id: ProjectId) {
    state().update(|s| {
        s.selected_project = Some(id);
        s.expanded_projects.insert(id);
        s.active_view = ActiveView::Workspace;
        s.selected_run = s.factory.runs_for_project(id).first().map(|run| run.id);
        s.sync_selected_issue_for_active_project();
        s.status_message = None;
    });
}

pub fn is_project_expanded(id: ProjectId) -> bool {
    state().with(|s| s.expanded_projects.contains(&id))
}

pub fn toggle_project_expanded(id: ProjectId) {
    state().update(|s| {
        if !s.expanded_projects.remove(&id) {
            s.expanded_projects.insert(id);
        }
    });
}

pub fn open_work_queue() {
    state().update(|s| {
        if s.selected_project.is_none() {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
        }
        if let Some(pid) = s.selected_project {
            s.expanded_projects.insert(pid);
        }
        s.active_view = ActiveView::WorkQueue;
        s.sync_selected_issue_for_active_project();
        s.status_message = None;
    });
}

pub fn open_work_queue_for_project(id: ProjectId) {
    state().update(|s| {
        let project_changed = s.selected_project != Some(id);
        s.selected_project = Some(id);
        s.expanded_projects.insert(id);
        s.active_view = ActiveView::WorkQueue;
        if project_changed {
            s.work_queue_selected_issue = None;
            s.work_queue_ad_hoc_open = false;
            s.work_queue_unmatched_expanded = false;
        }
        s.sync_selected_issue_for_active_project();
        s.status_message = None;
    });
}

pub fn open_project_workflow(project_id: ProjectId, workflow_name: &str) {
    let target = workflow_name.to_owned();
    state().update(|s| {
        s.selected_project = Some(project_id);
        s.expanded_projects.insert(project_id);
        let project_name = s
            .factory
            .project(project_id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let step_index = s
            .ensure_workflow_run_mut(&project_name, &target)
            .active_step_index;
        s.active_workflow_run_key = (project_name, target);
        s.workflow_run_selected_step = step_index;
        s.workflow_run_tab = RunDetailTab::Output;
        s.workflow_run_command_palette_open = false;
        s.active_view = ActiveView::WorkflowRun;
        s.status_message = None;
    });
}

pub fn open_run_history_for_project(project_id: ProjectId) {
    state().update(|s| {
        s.selected_project = Some(project_id);
        s.expanded_projects.insert(project_id);
        s.active_view = ActiveView::Workspace;
        s.selected_run = s
            .factory
            .runs_for_project(project_id)
            .first()
            .map(|run| run.id);
        s.status_message = None;
    });
}

// --- Workflow Run Detail state ----------------------------------------------------------

pub fn active_workflow_run() -> WorkflowRunViewData {
    state().with(AppState::active_workflow_run_clone)
}

pub fn active_workflow_run_key() -> (String, String) {
    state().with(|s| s.active_workflow_run_key.clone())
}

pub fn workflow_run_selected_step() -> usize {
    state().with(|s| s.workflow_run_selected_step)
}

pub fn select_workflow_run_step(index: usize) {
    state().update(|s| {
        let len = s.active_workflow_run_clone().steps.len();
        if len > 0 {
            s.workflow_run_selected_step = index.min(len - 1);
        }
    });
}

pub fn workflow_run_tab() -> RunDetailTab {
    state().with(|s| s.workflow_run_tab)
}

pub fn set_workflow_run_tab(tab: RunDetailTab) {
    state().update(|s| {
        s.workflow_run_tab = tab;
    });
}

pub fn workflow_run_inspector_visible() -> bool {
    state().with(|s| s.workflow_run_inspector_visible)
}

pub fn toggle_workflow_run_inspector() {
    state().update(|s| {
        s.workflow_run_inspector_visible = !s.workflow_run_inspector_visible;
    });
}

pub fn workflow_run_command_palette_open() -> bool {
    state().with(|s| s.workflow_run_command_palette_open)
}

pub fn toggle_workflow_run_command_palette() {
    state().update(|s| {
        s.workflow_run_command_palette_open = !s.workflow_run_command_palette_open;
    });
}

pub fn set_workflow_run_command_palette_open(open: bool) {
    state().update(|s| {
        s.workflow_run_command_palette_open = open;
    });
}

pub fn toggle_pause_workflow_run() {
    state().update(|s| {
        let (project, workflow) = s.active_workflow_run_key.clone();
        let run = s.ensure_workflow_run_mut(&project, &workflow);
        if run.toggle_pause() {
            let state_label = if run.execution_state == RunExecutionState::Paused {
                "Paused"
            } else {
                "Resumed"
            };
            let run_no = run.run_number;
            log::info!(
                target: "bruk::workflow",
                "workflow.run.toggle_pause project={project:?} workflow={workflow:?} run_number={run_no} state={state_label}"
            );
            s.status_message = Some(format!("{state_label} run #{run_no} ({workflow})"));
        }
    });
}

pub fn stop_workflow_run() {
    let mut stop_key = None;
    state().update(|s| {
        let (project, workflow) = s.active_workflow_run_key.clone();
        stop_key = Some((project.clone(), workflow.clone()));
        let run = s.ensure_workflow_run_mut(&project, &workflow);
        if run.stop() {
            let run_no = run.run_number;
            log::info!(
                target: "bruk::workflow",
                "workflow.run.stop_requested project={project:?} workflow={workflow:?} run_number={run_no}"
            );
            s.status_message = Some(format!("Stopped run #{run_no} ({workflow})"));
        }
    });
    if let Some(key) = stop_key
        && let Ok(reg) = runner_registry().lock()
        && let Some(handle) = reg.active_by_key.get(&key)
    {
        handle.stop();
    }
}

pub fn steer_workflow_run_step(instruction: &str) -> bool {
    let mut sent = false;
    let mut steer_key = None;
    state().update(|s| {
        let step_index = s.workflow_run_selected_step;
        let (project, workflow) = s.active_workflow_run_key.clone();
        let run = s.ensure_workflow_run_mut(&project, &workflow);
        match run.steer_step(step_index, instruction) {
            Ok(()) => {
                steer_key = Some((project, workflow));
                s.workflow_run_tab = RunDetailTab::Output;
                s.status_message = Some(format!(
                    "Sent steering instruction to step {}",
                    step_index + 1
                ));
                sent = true;
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
    if sent
        && let Some(key) = steer_key
        && let Ok(reg) = runner_registry().lock()
        && let Some(handle) = reg.active_by_key.get(&key)
    {
        handle.push_steering(instruction.to_owned());
    }
    sent
}

pub fn toggle_workflow_run_budget_pause() {
    state().update(|s| {
        let (project, workflow) = s.active_workflow_run_key.clone();
        let run = s.ensure_workflow_run_mut(&project, &workflow);
        run.toggle_budget_pause();
    });
}

pub fn approve_workflow_run_review_gate() {
    let mut approve_key = None;
    state().update(|s| {
        let (project, workflow) = s.active_workflow_run_key.clone();
        approve_key = Some((project.clone(), workflow.clone()));
        let run = s.ensure_workflow_run_mut(&project, &workflow);
        if run.approve_review_gate() {
            let run_no = run.run_number;
            log::info!(
                target: "bruk::workflow",
                "workflow.run.approve_gate project={project:?} workflow={workflow:?} run_number={run_no}"
            );
            s.workflow_run_selected_step = run.active_step_index;
            s.status_message = Some(format!("Approved review gate for run #{run_no}"));
        }
    });
    if let Some(key) = approve_key {
        let handle = runner_registry()
            .lock()
            .ok()
            .and_then(|reg| reg.active_by_key.get(&key).cloned());
        if let Some(h) = handle {
            h.resolve_gate(GateDecision::Approve { note: None });
        }
    }
}

pub fn edit_active_workflow() {
    state().update(|s| {
        let (project, workflow_name) = s.active_workflow_run_key.clone();
        let demo = s.demo_mode;
        let overview_entry = s
            .workflows_overview_for_name(&project)
            .workflows
            .into_iter()
            .find(|w| w.name.eq_ignore_ascii_case(&workflow_name));
        let key = if let Some(entry) = overview_entry {
            let wf_id = entry.id.clone();
            let key = (project.clone(), wf_id);
            s.workflow_editors.entry(key.clone()).or_insert_with(|| {
                WorkflowEditorDocument::from_overview_entry_with_demo(&project, &entry, demo)
            });
            key
        } else {
            let doc = if demo {
                WorkflowEditorDocument::for_workflow_name(&project, &workflow_name)
            } else {
                WorkflowEditorDocument::empty(&project, &workflow_name)
            };
            let key = (project.clone(), doc.workflow_id.clone());
            s.workflow_editors.entry(key.clone()).or_insert(doc);
            key
        };
        s.active_workflow_editor_key = key;
        s.workflow_editor_return_view = s.active_view;
        s.workflow_editor_inspector_visible = true;
        s.workflow_editor_history_open = false;
        s.workflow_editor_ad_hoc_open = false;
        s.workflow_editor_validation = None;
        s.active_view = ActiveView::WorkflowEditor;
        s.status_message = None;
    });
}

pub fn deploy_watch_entries() -> Vec<DeployWatchEntry> {
    state().with(|s| s.deploy_watch_entries.clone())
}

/// Persists a new project in SQLite and adds it to the reactive factory state.
pub fn register_project(name: &str, path: &str) -> bool {
    register_project_with_branch(name, path, "main")
}

fn register_project_with_branch(name: &str, path: &str, default_branch: &str) -> bool {
    let mut created = false;
    state().update(|s| {
        let Some(db_arc) = s.db.as_ref() else {
            s.registration_error = Some("SQLite database is not available.".to_owned());
            return;
        };

        let result = match db_arc.lock() {
            Ok(db) => db.register_project(name, path, default_branch, ""),
            Err(_) => {
                s.registration_error = Some("SQLite database lock poisoned.".to_owned());
                return;
            }
        };

        match result {
            Ok(project) => {
                let id = project.id;
                let saved_name = project.name.clone();
                let saved_path = project.root_path.display().to_string();
                let saved_branch = project.default_branch.clone();
                let demo = s.demo_mode;
                if s.using_sample_projects {
                    s.factory = FactoryState::with_projects(vec![project]);
                    s.using_sample_projects = false;
                    s.expanded_projects.clear();
                } else {
                    s.factory.insert_project(project);
                }
                let (initial_settings, initial_catalog) = if demo {
                    (
                        ProjectSettings::for_project(&saved_name, &saved_path, &saved_branch),
                        None,
                    )
                } else {
                    let mut clean = ProjectSettings::empty(&saved_name, &saved_path, &saved_branch);
                    if let Some(detected) = detect_git_remote_info(Path::new(&saved_path)) {
                        clean.apply_detected_git_remote(&detected);
                    }
                    let catalog = load_catalog(s.home.as_ref(), Path::new(&saved_path));
                    sync_project_workflows_from_catalog(&mut clean, &catalog);
                    (clean, Some(catalog))
                };
                if !demo
                    && (!initial_settings.github_repo.trim().is_empty()
                        || !initial_settings.git_remote.trim().is_empty())
                    && let Some(db_arc) = s.db.as_ref()
                    && let Ok(db) = db_arc.lock()
                {
                    let _ = db.save_project_settings(id, &initial_settings);
                }
                let initial_queue = if demo {
                    ProjectWorkQueue::for_project(&saved_name)
                } else {
                    let mut q = ProjectWorkQueue::empty(&saved_name);
                    if !initial_settings.github_repo.trim().is_empty() {
                        apply_project_settings_to_queue(&initial_settings, &mut q);
                    }
                    q
                };
                if !demo
                    && !initial_queue.repo_slug.trim().is_empty()
                    && let Some(db_arc) = s.db.as_ref()
                    && let Ok(db) = db_arc.lock()
                {
                    let _ = db.save_project_work_queue(id, &initial_queue);
                }
                s.work_queues
                    .entry(saved_name.clone())
                    .or_insert(initial_queue);
                if let Some(catalog) = initial_catalog {
                    s.workflows_overviews.insert(
                        saved_name.clone(),
                        ProjectWorkflowsOverview::from_catalog(&saved_name, &catalog),
                    );
                    s.catalogs.insert(saved_name.clone(), catalog);
                } else {
                    s.workflows_overviews
                        .entry(saved_name.clone())
                        .or_insert_with(|| ProjectWorkflowsOverview::for_project(&saved_name));
                }
                s.project_settings
                    .entry(saved_name.clone())
                    .or_insert(initial_settings);
                s.selected_project = Some(id);
                s.expanded_projects.insert(id);
                s.selected_run = None;
                s.work_queue_selected_issue = None;
                s.workflows_selected_id = None;
                s.sync_selected_issue_for_active_project();
                s.sync_selected_workflow_overview_for_active_project();
                s.registration_error = None;
                s.status_message = Some(format!("Registered project '{saved_name}'"));
                created = true;
            }
            Err(err) => {
                let message = err.to_string();
                log::warn!(
                    target: "bruk::projects",
                    "project.register_failed name={name:?} path={path} err={message}"
                );
                s.registration_error = Some(message.clone());
                s.status_message = Some(message);
            }
        }
    });
    created
}

/// Opens an existing project matching `path`, or registers it in SQLite and opens its workspace.
pub fn open_or_register_project_path(path: &Path) -> bool {
    let existing = state().with_untracked(|s| {
        s.factory
            .projects()
            .iter()
            .find(|project| project.root_path == path)
            .map(|project| project.id)
    });
    if let Some(id) = existing {
        select_project(id);
        return true;
    }

    let name = path
        .file_name()
        .and_then(|segment| segment.to_str())
        .filter(|segment| !segment.trim().is_empty())
        .unwrap_or("project");
    let path_str = path.display().to_string();
    if register_project(name, &path_str) {
        open_workspace();
        true
    } else {
        open_register_project();
        false
    }
}

/// Deletes a tracked project from SQLite and removes it from the reactive state.
pub fn remove_project(id: ProjectId) {
    state().update(|s| {
        let project_name = s.factory.project(id).map(|p| p.name.clone());
        let managed_clone = s
            .home
            .as_ref()
            .zip(s.factory.project(id))
            .filter(|(home, project)| home.is_managed_project(&project.root_path))
            .map(|(_, project)| display_with_tilde(&project.root_path));
        if !s.using_sample_projects
            && let Some(db_arc) = s.db.as_ref()
            && let Ok(db) = db_arc.lock()
        {
            if let Err(err) = db.delete_project(id) {
                log::error!(
                    target: "bruk::projects",
                    "project.remove_failed project_id={id} err={err}"
                );
                let message = format!("Failed to remove project: {err}");
                s.registration_error = Some(message.clone());
                s.status_message = Some(message);
                return;
            }
            if s.db_path_change_pending.is_none() {
                let _ = db.sweep_secret_tombstones(s.secret_store.as_ref(), false);
            }
        }
        s.refresh_secrets_state();

        s.factory.remove_project(id);
        let removed_chats: Vec<String> = s
            .chat_sessions
            .values()
            .filter(|chat| chat.project_id == id)
            .map(|chat| chat.id.clone())
            .collect();
        if let Ok(reg) = chat_registry().lock() {
            for chat_id in &removed_chats {
                if let Some(control) = reg.active.get(chat_id) {
                    control.stop();
                }
            }
        }
        s.chat_sessions.retain(|_, chat| chat.project_id != id);
        s.chat_selected.remove(&id);
        if s.selected_project == Some(id) {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
            s.selected_run = s
                .selected_project
                .and_then(|pid| s.factory.runs_for_project(pid).first().map(|r| r.id));
        }
        s.registration_error = None;
        if let Some(name) = project_name {
            s.status_message = Some(match managed_clone {
                Some(clone_path) => {
                    format!("Removed project '{name}'. Its clone is still at {clone_path}.")
                }
                None => format!("Removed project '{name}'"),
            });
        }
    });
}

// --- Chats ------------------------------------------------------------------------------

/// Running chat turns by chat id, so they can be stopped.
#[derive(Default)]
struct ChatRegistry {
    active: BTreeMap<String, ChatTurnControl>,
    seq: u64,
}

fn chat_registry() -> &'static Mutex<ChatRegistry> {
    static REGISTRY: std::sync::OnceLock<Mutex<ChatRegistry>> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(ChatRegistry::default()))
}

fn chat_turn_is_active(chat_id: &str) -> bool {
    chat_registry()
        .lock()
        .is_ok_and(|reg| reg.active.contains_key(chat_id))
}

/// Loads persisted chats, settling turns the app quit in the middle of.
fn load_chat_sessions(db: Option<&Arc<Mutex<ProjectDatabase>>>) -> BTreeMap<String, ChatSession> {
    let Some(guard) = db.and_then(|arc| arc.lock().ok()) else {
        return BTreeMap::new();
    };
    let now = current_unix_seconds();
    let mut sessions = BTreeMap::new();
    for mut session in guard.list_chat_sessions().unwrap_or_default() {
        if !chat_turn_is_active(&session.id) && session.mark_interrupted(now) {
            let _ = guard.save_chat_session(&session);
        }
        sessions.insert(session.id.clone(), session);
    }
    sessions
}

/// Provider binaries and the Codex home that agent turns (workflow steps, chats) spawn with.
struct ProviderLaunch {
    claude_binary: Option<std::path::PathBuf>,
    codex_binary: Option<std::path::PathBuf>,
    source_codex_home: Option<std::path::PathBuf>,
    antigravity_binary: Option<std::path::PathBuf>,
}

/// Compact git change badge shown in the chat header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatGitSummary {
    pub uncommitted_files: usize,
    pub additions: usize,
    pub deletions: usize,
    pub commits_ahead: usize,
}

/// What a chat's header and composer show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveChatMeta {
    pub id: String,
    pub title: String,
    pub agent_label: String,
    pub provider: ProviderKind,
    /// The model and effort picked when the chat started, when not the defaults.
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub sandbox: ProjectSandboxMode,
    pub status: ChatSessionStatus,
    pub message_count: usize,
    pub total_tokens: u64,
    pub total_cost_cents: u64,
    pub workspace: ChatWorkspace,
    pub worktree: Option<ChatWorktree>,
    /// Tokens in the context window and its size, after the latest turn.
    pub context: Option<(u64, u64)>,
    /// Whether this chat's project is a git repository (enables the Changes drawer).
    pub is_git_repo: bool,
    /// Summary of uncommitted changes and commits ahead for the header Changes button.
    pub git_summary: Option<ChatGitSummary>,
    /// When the chat was archived; archived chats are read-only.
    pub archived_at: Option<i64>,
}

/// What the open chat's Changes drawer shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveChatChanges {
    pub chat_id: String,
    pub title: String,
    pub working_dir: std::path::PathBuf,
    pub is_worktree: bool,
    pub default_branch: String,
    pub open_prs_as_draft: bool,
    pub running: bool,
    pub loading: bool,
    pub busy: bool,
    pub status: Option<ChatGitStatus>,
    pub error: Option<String>,
    pub feedback: Option<String>,
    pub pr_url: Option<String>,
}

/// What the open chat's context panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveChatContext {
    pub chat_id: String,
    pub provider: ProviderKind,
    pub usage: Option<ChatContextUsage>,
    pub loading: bool,
    pub error: Option<String>,
    /// The provider has a session to measure (a turn has run).
    pub has_session: bool,
    pub running: bool,
    /// The chat's tokens by kind, the running reply's so far included.
    pub tokens: ChatTokenBreakdown,
    /// Every token the chat's finished replies used, as totaled for the dashboard.
    pub recorded_tokens: u64,
}

/// One row of the open chat's transcript, keyed by chat and position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatEntryRow {
    pub chat_id: String,
    pub index: usize,
    pub provider: ProviderKind,
    pub entry: ChatEntry,
}

impl AppState {
    /// The extra environment every agent process gets: the project's variables, plus the
    /// `SSH_AUTH_SOCK` from Preferences when one is set (a project's own value wins).
    /// The MCP servers a run or chat turn chooses from. The project's file is read from its
    /// checkout now (never from a run's worktree), so edits show up without reloading.
    fn mcp_sources(
        &self,
        project_root: &std::path::Path,
        settings: Option<&ProjectSettings>,
    ) -> factory_config::mcp::McpSources {
        factory_config::mcp::McpSources::gather(
            &factory_config::mcp::load_project_mcp(project_root),
            settings,
            &self.preferences.extensions,
        )
    }

    fn agent_env(&self, settings: Option<&ProjectSettings>) -> BTreeMap<String, String> {
        let mut env: BTreeMap<String, String> = settings
            .map(|settings| {
                settings
                    .env_vars
                    .iter()
                    .filter(|var| !var.is_secret && !var.key.trim().is_empty())
                    .map(|var| (var.key.clone(), var.value.clone()))
                    .collect()
            })
            .unwrap_or_default();
        self.preferences.advanced.apply_agent_env(&mut env);
        env
    }

    fn provider_launch(&self) -> ProviderLaunch {
        let path_env = self.claude_path_lookup.resolve_path_env();
        let claude_binary = resolve_claude_binary(
            self.preferences.claude.custom_cli_path.as_deref(),
            path_env.as_deref(),
        )
        .ok();
        let codex_binary = self.preferences.codex.as_ref().and_then(|cx| {
            resolve_codex_binary(cx.custom_cli_path.as_deref(), path_env.as_deref()).ok()
        });
        let source_codex_home = Some(
            self.preferences
                .codex
                .as_ref()
                .map(|cx| expand_tilde(&cx.config_dir))
                .unwrap_or_else(|| expand_tilde("~/.codex")),
        );
        let antigravity_binary = self.preferences.antigravity.as_ref().and_then(|ag| {
            resolve_antigravity_binary(ag.custom_cli_path.as_deref(), path_env.as_deref()).ok()
        });
        ProviderLaunch {
            claude_binary,
            codex_binary,
            source_codex_home,
            antigravity_binary,
        }
    }

    fn selected_project_settings(&self) -> Option<&ProjectSettings> {
        let project = self
            .selected_project
            .and_then(|pid| self.factory.project(pid))?;
        self.project_settings.get(&project.name)
    }

    fn chat_provider_allowed(&self, provider: ProviderKind) -> bool {
        match provider {
            ProviderKind::Claude => true,
            ProviderKind::Codex => self.preferences.codex.is_some(),
            ProviderKind::Antigravity => self.preferences.antigravity.is_some(),
            ProviderKind::OpenRouter => false,
        }
    }

    fn resolved_chat_new_provider(&self) -> ProviderKind {
        self.chat_new_provider
            .filter(|p| self.chat_provider_allowed(*p))
            .or_else(|| {
                self.selected_project_settings()
                    .map(|settings| settings.default_provider)
                    .filter(|p| self.chat_provider_allowed(*p))
            })
            .unwrap_or(ProviderKind::Claude)
    }

    fn resolved_chat_new_sandbox(&self) -> ProjectSandboxMode {
        self.chat_new_sandbox
            .or_else(|| {
                self.selected_project_settings()
                    .map(|settings| settings.sandbox_mode)
            })
            .unwrap_or_default()
    }

    /// The picked effort, while the new chat's provider takes it.
    fn resolved_chat_new_effort(&self) -> Option<String> {
        self.chat_new_effort.clone().filter(|e| {
            provider_effort_levels(self.resolved_chat_new_provider()).contains(&e.as_str())
        })
    }

    /// The CLI account behind a provider, when it is set up.
    fn cli_account(&self, provider: ProviderKind) -> Option<&CliAccount> {
        match provider {
            ProviderKind::Claude => Some(&self.preferences.claude),
            ProviderKind::Codex => self.preferences.codex.as_ref(),
            ProviderKind::Antigravity => self.preferences.antigravity.as_ref(),
            ProviderKind::OpenRouter => None,
        }
    }

    /// Sets the provider for the next new chat; a model or effort picked for another provider
    /// is dropped.
    fn choose_chat_new_provider(&mut self, provider: ProviderKind) {
        if self.resolved_chat_new_provider() != provider {
            self.chat_new_model = None;
            self.chat_new_effort = None;
        }
        self.chat_new_provider = Some(provider);
    }

    fn reset_chat_composer(&mut self) {
        self.chat_new_provider = None;
        self.chat_new_agent = None;
        self.chat_new_model = None;
        self.chat_new_effort = None;
        self.chat_new_sandbox = None;
        self.chat_new_workspace = None;
    }

    /// Whether the selected project can give a chat its own worktree (it is a git checkout).
    fn chat_worktree_available(&self) -> bool {
        self.selected_project
            .and_then(|pid| self.factory.project(pid))
            .is_some_and(|project| project.root_path.join(".git").exists())
    }

    fn resolved_chat_new_workspace(&self) -> ChatWorkspace {
        match self.chat_new_workspace {
            Some(ChatWorkspace::Worktree) if self.chat_worktree_available() => {
                ChatWorkspace::Worktree
            }
            _ => ChatWorkspace::Checkout,
        }
    }

    /// Records what one turn of a chat used, for the Dashboard.
    fn record_chat_usage(&self, chat_id: &str, usage: ChatUsageDelta, now: i64) {
        if self.using_sample_projects || usage.is_zero() {
            return;
        }
        if let Some(session) = self.chat_sessions.get(chat_id)
            && let Some(db_arc) = self.db.as_ref()
            && let Ok(db) = db_arc.lock()
        {
            let _ = db.record_chat_usage(session, usage, now);
        }
    }

    /// Rebuilds the Dashboard from every recorded workflow run and chat turn.
    fn refresh_dashboard_dataset(&mut self) {
        if self.using_sample_projects {
            return;
        }
        let dataset = {
            let Some(db) = self.db.as_ref().and_then(|arc| arc.lock().ok()) else {
                return;
            };
            DashboardDataset::from_usage(
                &db.list_all_workflow_runs().unwrap_or_default(),
                &db.list_chat_usage().unwrap_or_default(),
            )
        };
        self.dashboard_dataset = dataset;
    }

    fn active_chat_id(&self) -> Option<&String> {
        let pid = self.selected_project?;
        self.chat_selected
            .get(&pid)
            .filter(|id| self.chat_sessions.contains_key(*id))
    }

    fn persist_chat(&self, chat_id: &str) {
        if self.using_sample_projects {
            return;
        }
        if let Some(session) = self.chat_sessions.get(chat_id)
            && let Some(db_arc) = self.db.as_ref()
            && let Ok(db) = db_arc.lock()
            && let Err(err) = db.save_chat_session(session)
        {
            log::error!(
                target: "bruk::storage",
                "db.save_chat_session_failed chat_id={chat_id} err={err}"
            );
        }
    }

    /// Records the user's message in the open chat (starting one with the composer's choices
    /// when none is open) and resolves the turn to run.
    fn begin_chat_turn(
        &mut self,
        text: &str,
        attachment_sources: &[std::path::PathBuf],
        now: i64,
    ) -> Result<(String, ChatTurnRequest), String> {
        let pid = self
            .selected_project
            .ok_or_else(|| "Select a project to chat in".to_owned())?;
        let project = self
            .factory
            .project(pid)
            .cloned()
            .ok_or_else(|| "Select a project to chat in".to_owned())?;

        let chat_id = match self.active_chat_id().cloned() {
            Some(id) => id,
            None => {
                let seq = chat_registry()
                    .lock()
                    .map(|mut reg| {
                        reg.seq += 1;
                        reg.seq
                    })
                    .unwrap_or(1);
                let id = format!("chat-{now}-{seq}");
                let agent = self.chat_new_agent.clone();
                let agent_name = agent.as_deref().and_then(|reference| {
                    let catalog = self
                        .catalogs
                        .get(&project.name)
                        .cloned()
                        .unwrap_or_else(|| load_catalog(self.home.as_ref(), &project.root_path));
                    catalog.resolve_agent(reference).ok().map(|a| a.name)
                });
                let mut session = ChatSession::new(
                    id.clone(),
                    pid,
                    project.name.clone(),
                    self.resolved_chat_new_provider(),
                    agent,
                    agent_name,
                    self.resolved_chat_new_sandbox(),
                    now,
                );
                session.workspace = self.resolved_chat_new_workspace();
                session.model = self.chat_new_model.clone();
                session.reasoning_effort = self.resolved_chat_new_effort();
                self.chat_sessions.insert(id.clone(), session);
                self.chat_selected.insert(pid, id.clone());
                self.reset_chat_composer();
                id
            }
        };

        let chat_dir = self.chat_runtime_dir(&chat_id);
        let worktree_dir = chat_dir.join("worktree");
        let branch_prefix = self
            .project_settings
            .get(&project.name)
            .map(|settings| settings.branch_prefix.clone())
            .unwrap_or_default();
        let session = self
            .chat_sessions
            .get_mut(&chat_id)
            .ok_or_else(|| "That chat no longer exists".to_owned())?;
        if session.is_running() {
            return Err(
                "Wait for the reply, or stop it, before sending another message".to_owned(),
            );
        }
        if session.is_archived() {
            return Err("This chat is archived. Unarchive it to keep chatting.".to_owned());
        }
        if self.chats_pruning.contains(&chat_id) {
            return Err(
                "Bruk is checking whether this chat's branch merged. Try again in a moment."
                    .to_owned(),
            );
        }
        let uploads = plan_chat_attachments(&chat_dir, attachment_sources)?;
        let attachments = uploads.iter().map(|u| u.attachment.clone()).collect();
        session.push_user_message_with_attachments(text, attachments, now);
        if session.workspace == ChatWorkspace::Worktree && session.worktree.is_none() {
            // Named after the first message; the first turn creates it.
            session.worktree = Some(ChatWorktree {
                path: worktree_dir,
                branch: chat_worktree_branch(&branch_prefix, &session.title, &chat_id),
            });
        }
        session.apply_event(&ChatEvent::TurnStarted, now);
        self.persist_chat(&chat_id);

        match self.chat_turn_request(&chat_id, text) {
            Ok(mut request) => {
                request.attachments = uploads;
                Ok((chat_id, request))
            }
            Err(message) => {
                if let Some(session) = self.chat_sessions.get_mut(&chat_id) {
                    session.apply_event(
                        &ChatEvent::TurnFinished {
                            status: ChatSessionStatus::Failed,
                            error: Some(message.clone()),
                        },
                        now,
                    );
                }
                self.persist_chat(&chat_id);
                Err(message)
            }
        }
    }

    fn chat_runtime_dir(&self, chat_id: &str) -> std::path::PathBuf {
        self.home
            .as_ref()
            .map(|home| home.chat_dir(chat_id))
            .unwrap_or_else(|| std::env::temp_dir().join("bruk-chats").join(chat_id))
    }

    /// What a chat's next turn (or a reading of its context window) runs with, resolved from
    /// the chat, its project and preferences.
    fn chat_turn_request(
        &mut self,
        chat_id: &str,
        prompt: &str,
    ) -> Result<ChatTurnRequest, String> {
        let session = self
            .chat_sessions
            .get(chat_id)
            .cloned()
            .ok_or_else(|| "That chat no longer exists".to_owned())?;
        let project = self
            .factory
            .project(session.project_id)
            .cloned()
            .ok_or_else(|| "That chat's project isn't in Bruk any more".to_owned())?;
        let catalog = self
            .catalogs
            .get(&project.name)
            .cloned()
            .unwrap_or_else(|| load_catalog(self.home.as_ref(), &project.root_path));
        let settings = self.project_settings.get(&project.name).cloned();
        let launch = self.provider_launch();
        let project_env = self.agent_env(settings.as_ref());
        let mcp = self.mcp_sources(&project.root_path, settings.as_ref());
        let secrets_cache = self.ensure_chat_secrets_cache(chat_id);
        let secret_resolver = if self.demo_mode {
            None
        } else {
            Some(Arc::clone(&self.secret_resolver))
        };
        let worktree = match (session.workspace, session.worktree.as_ref()) {
            (ChatWorkspace::Worktree, Some(worktree)) => Some(ChatWorktreeSetup {
                project_root: project.root_path.clone(),
                path: worktree.path.clone(),
                branch: worktree.branch.clone(),
            }),
            _ => None,
        };
        let working_dir = worktree
            .as_ref()
            .map(|setup| setup.path.clone())
            .unwrap_or_else(|| project.root_path.clone());

        Ok(ChatTurnRequest {
            session_id: chat_id.to_owned(),
            project_id: project.id,
            provider: session.provider,
            agent: session.agent.clone(),
            model: session.model.clone(),
            reasoning_effort: session.reasoning_effort.clone(),
            sandbox: session.sandbox,
            resume_session_id: session.provider_session_id.clone(),
            prompt: prompt.to_owned(),
            attachments: Vec::new(),
            working_dir,
            worktree,
            runtime_dir: self.chat_runtime_dir(chat_id),
            catalog,
            project_model_override: settings
                .as_ref()
                .map(|s| s.model_override.trim().to_owned())
                .filter(|m| !m.is_empty()),
            project_env,
            mcp,
            claude_binary: launch.claude_binary,
            claude_default_model: Some(self.preferences.claude.default_model.clone()),
            claude_settings: self.preferences.claude_spawn_settings.clone(),
            codex_binary: launch.codex_binary,
            codex_default_model: self
                .preferences
                .codex
                .as_ref()
                .map(|cx| cx.default_model.clone()),
            codex_settings: self.preferences.codex_spawn_settings.clone(),
            source_codex_home: launch.source_codex_home,
            antigravity_binary: launch.antigravity_binary,
            antigravity_default_model: self
                .preferences
                .antigravity
                .as_ref()
                .map(|ag| ag.default_model.clone()),
            secret_resolver,
            secrets_cache: Some(secrets_cache),
        })
    }
}

/// Opens a project's chats, showing the one last open there.
pub fn open_chats_for_project(id: ProjectId) {
    let mut selected_chat = None;
    state().update(|s| {
        s.selected_project = Some(id);
        s.expanded_projects.insert(id);
        s.active_view = ActiveView::Chat;
        s.status_message = None;
        selected_chat = s.active_chat_id().cloned();
    });
    if let Some(chat_id) = selected_chat {
        refresh_chat_git_status(chat_id);
    }
}

/// Opens a fresh composer in the selected (or first) project's chats.
pub fn start_new_chat() {
    state().update(|s| {
        if s.selected_project.is_none() {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
        }
        let Some(pid) = s.selected_project else {
            return;
        };
        s.expanded_projects.insert(pid);
        s.chat_selected.remove(&pid);
        s.reset_chat_composer();
        s.active_view = ActiveView::Chat;
        s.status_message = None;
    });
}

pub fn select_chat(id: &str) {
    let mut found = false;
    state().update(|s| {
        if let Some(pid) = s.chat_sessions.get(id).map(|chat| chat.project_id) {
            s.selected_project = Some(pid);
            s.chat_selected.insert(pid, id.to_owned());
            s.active_view = ActiveView::Chat;
            found = true;
        }
    });
    if found {
        refresh_chat_git_status(id.to_owned());
    }
}

/// The project's chats that aren't archived, most recently active first.
pub fn chat_summaries_for_project(id: ProjectId) -> Vec<ChatSessionSummary> {
    state().with(|s| {
        let mut chats: Vec<ChatSessionSummary> = s
            .chat_sessions
            .values()
            .filter(|chat| chat.project_id == id && !chat.is_archived())
            .map(ChatSession::summary)
            .collect();
        chats.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        chats
    })
}

/// The project's archived chats, most recently archived first.
pub fn archived_chat_summaries_for_project(id: ProjectId) -> Vec<ChatSessionSummary> {
    state().with(|s| {
        let mut chats: Vec<ChatSessionSummary> = s
            .chat_sessions
            .values()
            .filter(|chat| chat.project_id == id && chat.is_archived())
            .map(ChatSession::summary)
            .collect();
        chats.sort_by(|a, b| {
            b.archived_at
                .cmp(&a.archived_at)
                .then_with(|| b.updated_at.cmp(&a.updated_at))
                .then_with(|| b.id.cmp(&a.id))
        });
        chats
    })
}

/// How many chats a project has (archived ones aside), and how many are working on a reply.
pub fn chat_counts_for_project(id: ProjectId) -> (usize, usize) {
    state().with(|s| {
        s.chat_sessions
            .values()
            .filter(|chat| chat.project_id == id && !chat.is_archived())
            .fold((0, 0), |(total, running), chat| {
                (total + 1, running + usize::from(chat.is_running()))
            })
    })
}

pub fn active_chat_id() -> Option<String> {
    state().with(|s| s.active_chat_id().cloned())
}

pub fn active_chat_meta() -> Option<ActiveChatMeta> {
    state().with(|s| {
        let chat = s.active_chat_id().and_then(|id| s.chat_sessions.get(id))?;
        let is_git_repo = !s.using_sample_projects
            && s.factory
                .project(chat.project_id)
                .is_some_and(|project| project.root_path.join(".git").exists());
        let git_summary = s.chat_git_status.get(&chat.id).map(|git| ChatGitSummary {
            uncommitted_files: git.uncommitted_file_count(),
            additions: git.uncommitted_additions(),
            deletions: git.uncommitted_deletions(),
            commits_ahead: git.commits_ahead,
        });
        Some(ActiveChatMeta {
            id: chat.id.clone(),
            title: chat.display_title(),
            agent_label: chat.agent_label(),
            provider: chat.provider,
            model: chat.model.clone(),
            reasoning_effort: chat.reasoning_effort.clone(),
            sandbox: chat.sandbox,
            status: chat.status,
            message_count: chat.message_count(),
            total_tokens: chat.total_tokens,
            total_cost_cents: chat.total_cost_cents(),
            workspace: chat.workspace,
            worktree: chat.worktree.clone(),
            context: chat
                .context
                .as_ref()
                .map(|usage| (usage.used_tokens, usage.window_tokens)),
            is_git_repo,
            git_summary,
            archived_at: chat.archived_at,
        })
    })
}

pub fn active_chat_rows() -> Vec<ChatEntryRow> {
    state().with(|s| {
        let Some(chat) = s.active_chat_id().and_then(|id| s.chat_sessions.get(id)) else {
            return Vec::new();
        };
        chat.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| ChatEntryRow {
                chat_id: chat.id.clone(),
                index,
                provider: chat.provider,
                entry: entry.clone(),
            })
            .collect()
    })
}

/// Providers a chat can talk to: Claude, and Codex or Antigravity once added in Settings.
pub fn chat_provider_options() -> Vec<ProviderKind> {
    state().with(|s| {
        ProviderKind::CLI_PROVIDERS
            .iter()
            .copied()
            .filter(|p| s.chat_provider_allowed(*p))
            .collect()
    })
}

/// The selected project's agents as `(reference, name)`.
pub fn chat_agent_options() -> Vec<(String, String)> {
    let agents = |catalog: &BrukCatalog| -> Vec<(String, String)> {
        catalog
            .effective_agents()
            .into_iter()
            .map(|agent| (agent.id.clone(), agent.name.clone()))
            .collect()
    };
    state().with(|s| match s.catalogs.get(&s.selected_project_name()) {
        Some(catalog) => agents(catalog),
        None => agents(&BrukCatalog::builtin_only()),
    })
}

pub fn chat_new_provider() -> ProviderKind {
    state().with(AppState::resolved_chat_new_provider)
}

pub fn set_chat_new_provider(provider: ProviderKind) {
    state().update(|s| s.choose_chat_new_provider(provider));
}

/// Models the next new chat's provider offers, as `(value, name)`, from its CLI (the Model
/// Family Filter applied).
pub fn chat_model_options() -> Vec<(String, String)> {
    state().with(|s| {
        s.cli_account(s.resolved_chat_new_provider())
            .map(|account| {
                account
                    .visible_available_models()
                    .into_iter()
                    .map(|m| {
                        let name = if m.display_name.trim().is_empty() {
                            m.value.clone()
                        } else {
                            m.display_name.clone()
                        };
                        (m.value, name)
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// What the next new chat uses when no model is picked: its agent's model, the project's
/// override, or the provider's default.
pub fn chat_default_model_label() -> String {
    state().with(|s| {
        let provider = s.resolved_chat_new_provider();
        let agent_model = s
            .chat_new_agent
            .as_deref()
            .and_then(|r| s.active_catalog_clone().resolve_agent(r).ok())
            .and_then(|a| a.model)
            .filter(|m| !m.trim().is_empty());
        let project_model = s
            .selected_project_settings()
            .map(|settings| settings.model_override.trim().to_owned())
            .filter(|m| !m.is_empty());
        let provider_model = s
            .cli_account(provider)
            .map(|a| a.default_model.trim().to_owned())
            .filter(|m| !m.is_empty());
        agent_model
            .or(project_model)
            .or(provider_model)
            .unwrap_or_else(|| "provider default".to_owned())
    })
}

pub fn chat_new_model() -> Option<String> {
    state().with(|s| s.chat_new_model.clone())
}

pub fn set_chat_new_model(model: Option<String>) {
    state().update(|s| s.chat_new_model = model.filter(|m| !m.trim().is_empty()));
}

/// Effort levels the next new chat's provider takes, lowest first.
pub fn chat_effort_options() -> Vec<&'static str> {
    state().with(|s| provider_effort_levels(s.resolved_chat_new_provider()).to_vec())
}

/// What the next new chat uses when no effort is picked: its agent's, or the provider's.
pub fn chat_default_effort_label() -> String {
    state().with(|s| {
        s.chat_new_agent
            .as_deref()
            .and_then(|r| s.active_catalog_clone().resolve_agent(r).ok())
            .and_then(|a| a.reasoning_effort)
            .filter(|e| !e.trim().is_empty())
            .unwrap_or_else(|| "provider default".to_owned())
    })
}

pub fn chat_new_effort() -> Option<String> {
    state().with(AppState::resolved_chat_new_effort)
}

pub fn set_chat_new_effort(effort: Option<String>) {
    state().update(|s| s.chat_new_effort = effort.filter(|e| !e.trim().is_empty()));
}

pub fn chat_new_agent() -> Option<String> {
    state().with(|s| s.chat_new_agent.clone())
}

/// Picks the agent for the next new chat, adopting the provider and access it declares.
pub fn set_chat_new_agent(agent: Option<String>) {
    state().update(|s| {
        let resolved = agent
            .as_deref()
            .and_then(|reference| s.active_catalog_clone().resolve_agent(reference).ok());
        if let Some(resolved) = resolved {
            if let Some(provider) = resolved.provider.filter(|p| s.chat_provider_allowed(*p)) {
                s.choose_chat_new_provider(provider);
            }
            if let Some(sandbox) = resolved.sandbox {
                s.chat_new_sandbox = Some(sandbox);
            }
        }
        s.chat_new_agent = agent;
    });
}

pub fn chat_new_sandbox() -> ProjectSandboxMode {
    state().with(AppState::resolved_chat_new_sandbox)
}

pub fn set_chat_new_sandbox(mode: ProjectSandboxMode) {
    state().update(|s| s.chat_new_sandbox = Some(mode));
}

/// Changes what the open chat's agent may do, from its next message on.
pub fn set_active_chat_sandbox(mode: ProjectSandboxMode) {
    state().update(|s| {
        let Some(id) = s.active_chat_id().cloned() else {
            return;
        };
        if let Some(chat) = s.chat_sessions.get_mut(&id) {
            chat.sandbox = mode;
        }
        s.persist_chat(&id);
    });
}

/// Sends `text` in the open chat (starting a new one if none is open). False if nothing
/// was sent.
pub fn send_chat_message(text: &str) -> bool {
    let text = text.trim().to_owned();
    let now = current_unix_seconds();
    let mut prepared = None;
    state().update(|s| {
        if text.is_empty() && s.chat_pending_attachments.is_empty() {
            return;
        }
        let sources: Vec<std::path::PathBuf> = s
            .chat_pending_attachments
            .iter()
            .map(|a| a.path.clone())
            .collect();
        prepared = Some(s.begin_chat_turn(&text, &sources, now));
        if let Some(Err(message)) = &prepared {
            s.status_message = Some(message.clone());
        } else {
            s.status_message = None;
            s.chat_pending_attachments.clear();
        }
    });
    match prepared {
        Some(Ok((chat_id, request))) => {
            start_chat_turn(chat_id, request);
            true
        }
        _ => false,
    }
}

/// A file waiting in the composer to go with the next message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChatAttachment {
    pub path: std::path::PathBuf,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

impl PendingChatAttachment {
    pub fn is_image(&self) -> bool {
        is_model_image_type(&self.mime_type)
    }
}

/// Files waiting in the composer, in the order they were added.
pub fn chat_pending_attachments() -> Vec<PendingChatAttachment> {
    state().with(|s| s.chat_pending_attachments.clone())
}

/// Adds files to the next message; reports the ones that can't be attached (folders, missing
/// or too large files). A file already waiting is not added again.
pub fn add_chat_attachments(paths: Vec<std::path::PathBuf>) {
    let mut added = Vec::new();
    let mut problems = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        match std::fs::metadata(&path) {
            Ok(meta) if meta.is_dir() => problems.push(format!("{name} is a folder")),
            Ok(meta) if meta.len() > CHAT_ATTACHMENT_MAX_BYTES => problems.push(format!(
                "{name} is larger than {}",
                format_bytes(CHAT_ATTACHMENT_MAX_BYTES)
            )),
            Ok(meta) => added.push(PendingChatAttachment {
                mime_type: attachment_mime_type(&path).to_owned(),
                size_bytes: meta.len(),
                name,
                path,
            }),
            Err(err) => problems.push(format!("Couldn't read {name}: {err}")),
        }
    }
    state().update(|s| {
        for attachment in added {
            if !s
                .chat_pending_attachments
                .iter()
                .any(|a| a.path == attachment.path)
            {
                s.chat_pending_attachments.push(attachment);
            }
        }
        s.status_message =
            (!problems.is_empty()).then(|| format!("Couldn't attach: {}.", problems.join("; ")));
    });
}

/// Adds files given as `file:` URLs (a drop, a paste from Finder).
pub fn add_chat_attachment_uris(uris: &[String]) {
    let paths: Vec<std::path::PathBuf> = uris.iter().filter_map(|u| file_uri_to_path(u)).collect();
    if !paths.is_empty() {
        add_chat_attachments(paths);
    }
}

/// Attaches pasted bytes (a screenshot) by saving them to a file of their own first.
pub fn add_pasted_chat_attachment(name: &str, data: &[u8]) {
    let dir = std::env::temp_dir().join("bruk-pasted");
    let seq = chat_registry()
        .lock()
        .map(|mut reg| {
            reg.seq += 1;
            reg.seq
        })
        .unwrap_or(0);
    let name = std::path::Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "pasted".to_owned());
    // Its own folder keeps the file's name, which the chat shows.
    let path = dir
        .join(format!("{}-{seq}", current_unix_seconds()))
        .join(&name);
    let written = path
        .parent()
        .map(std::fs::create_dir_all)
        .unwrap_or(Ok(()))
        .and_then(|_| std::fs::write(&path, data));
    match written {
        Ok(()) => add_chat_attachments(vec![path]),
        Err(err) => set_status_message(format!("Couldn't attach the pasted {name}: {err}")),
    }
}

pub fn remove_chat_attachment(path: &std::path::Path) {
    state().update(|s| s.chat_pending_attachments.retain(|a| a.path != path));
}

/// The local path of a `file:` URL (percent-decoded); `None` for other URLs.
pub fn file_uri_to_path(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file://localhost/path` and `file:///path` both name `/path`.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2]))
        {
            decoded.push(hi * 16 + lo);
            i += 3;
            continue;
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(decoded)
        .ok()
        .map(std::path::PathBuf::from)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn start_chat_turn(chat_id: String, request: ChatTurnRequest) {
    let (control, rx) = spawn_chat_turn(request);
    if let Ok(mut reg) = chat_registry().lock() {
        reg.active.insert(chat_id.clone(), control);
    }

    // Pump turn events onto the store without blocking rendering; a burst of streamed text
    // lands as one update.
    let rx_shared = Arc::new(Mutex::new(rx));
    Owner::detached().run(|| {
        spawn(async move {
            loop {
                let rx_clone = rx_shared.clone();
                let batch = spawn_blocking_task(move || {
                    let guard = rx_clone.lock().ok()?;
                    let first = guard.recv().ok()?;
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    let mut batch = vec![first];
                    batch.extend(guard.try_iter());
                    Some(batch)
                })
                .await;
                let Some(batch) = batch else {
                    break;
                };
                let finished = batch
                    .iter()
                    .any(|event| matches!(event, ChatEvent::TurnFinished { .. }));
                let persist = finished
                    || batch
                        .iter()
                        .any(|event| matches!(event, ChatEvent::ProviderSessionStarted { .. }));
                let now = current_unix_seconds();
                let mut mcp_notifications = Vec::new();
                let mut mcp_cleared = Vec::new();
                state().update(|s| {
                    let Some(chat) = s.chat_sessions.get(&chat_id) else {
                        return;
                    };
                    let project_name = s
                        .factory
                        .project(chat.project_id)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    let (title, provider) = (chat.title.clone(), chat.provider);
                    for event in &batch {
                        match event {
                            ChatEvent::McpProblem(problem) => {
                                if let Some(n) = s.file_mcp_problem(
                                    McpProblemContext {
                                        project_name: &project_name,
                                        context_label: format!("Chat · {title}"),
                                        where_label: format!("Chat '{title}'"),
                                        run_number: 0,
                                        provider,
                                    },
                                    problem,
                                ) {
                                    mcp_notifications.push(n);
                                }
                            }
                            ChatEvent::McpServersReady(servers) => {
                                mcp_cleared
                                    .extend(s.clear_ready_mcp_servers(&project_name, servers));
                            }
                            ChatEvent::SecretProblem(problem) => {
                                if let Some(n) = s.file_secret_problem(
                                    McpProblemContext {
                                        project_name: &project_name,
                                        context_label: format!("Chat · {title}"),
                                        where_label: format!("Chat '{title}'"),
                                        run_number: 0,
                                        provider,
                                    },
                                    problem,
                                ) {
                                    mcp_notifications.push(n);
                                }
                            }
                            _ => {}
                        }
                    }
                    if finished {
                        s.flush_secret_resolver_updates();
                    }
                });
                post_mcp_notifications(mcp_notifications);
                close_notifications(mcp_cleared);
                state().update(|s| {
                    let Some(chat) = s.chat_sessions.get_mut(&chat_id) else {
                        return;
                    };
                    let mut used = ChatUsageDelta::default();
                    for event in &batch {
                        let delta = chat.apply_event(event, now);
                        used.tokens += delta.tokens;
                        used.cost_micros += delta.cost_micros;
                    }
                    if persist || !used.is_zero() {
                        s.persist_chat(&chat_id);
                    }
                    if !used.is_zero() {
                        s.record_chat_usage(&chat_id, used, now);
                        s.refresh_dashboard_dataset();
                    }
                });
                if finished {
                    // See how the turn left the context window and the git working tree.
                    refresh_chat_context(chat_id.clone());
                    refresh_chat_git_status(chat_id.clone());
                    break;
                }
            }
            if let Ok(mut reg) = chat_registry().lock() {
                reg.active.remove(&chat_id);
            }
            // A turn whose thread ended without reporting back must not look busy forever.
            state().update(|s| {
                if let Some(chat) = s.chat_sessions.get_mut(&chat_id)
                    && chat.is_running()
                {
                    log::error!(
                        target: "bruk::chat",
                        "chat.turn.unexpected_end chat_id={chat_id}"
                    );
                    chat.apply_event(
                        &ChatEvent::TurnFinished {
                            status: ChatSessionStatus::Failed,
                            error: Some("The reply ended unexpectedly.".to_owned()),
                        },
                        current_unix_seconds(),
                    );
                    s.persist_chat(&chat_id);
                }
            });
        });
    });
}

/// Reads a chat's context window in the background (Claude Code reports it for a fork of the
/// session; Codex records it). Does nothing before the chat has a provider session.
fn refresh_chat_context(chat_id: String) {
    let mut request = None;
    state().update(|s| {
        if s.using_sample_projects || s.chat_context_loading.contains(&chat_id) {
            return;
        }
        // Its worktree may be gone; the last reading stays shown.
        if s.chat_sessions
            .get(&chat_id)
            .is_none_or(ChatSession::is_archived)
        {
            return;
        }
        if let Ok(prepared) = s.chat_turn_request(&chat_id, "")
            && prepared.resume_session_id.is_some()
        {
            s.chat_context_loading.insert(chat_id.clone());
            s.chat_context_errors.remove(&chat_id);
            request = Some(prepared);
        }
    });
    let Some(request) = request else {
        return;
    };
    Owner::detached().run(|| {
        spawn(async move {
            let result =
                spawn_blocking_task(move || query_chat_context(&request, current_unix_seconds()))
                    .await;
            state().update(|s| {
                s.chat_context_loading.remove(&chat_id);
                match result {
                    Ok(usage) => {
                        if let Some(chat) = s.chat_sessions.get_mut(&chat_id) {
                            chat.apply_event(
                                &ChatEvent::ContextUsage(usage),
                                current_unix_seconds(),
                            );
                            s.persist_chat(&chat_id);
                        }
                    }
                    Err(message) => {
                        s.chat_context_errors.insert(chat_id.clone(), message);
                    }
                }
            });
        });
    });
}

pub fn chat_new_workspace() -> ChatWorkspace {
    state().with(AppState::resolved_chat_new_workspace)
}

pub fn set_chat_new_workspace(workspace: ChatWorkspace) {
    state().update(|s| s.chat_new_workspace = Some(workspace));
}

/// Whether the selected project can give a new chat its own worktree.
pub fn chat_worktree_available() -> bool {
    state().with(AppState::chat_worktree_available)
}

pub fn chat_context_open() -> bool {
    state().with(|s| s.chat_context_open)
}

/// Shows or hides the context-window panel, reading the open chat's window when it has none.
pub fn toggle_chat_context() {
    let mut opened = false;
    state().update(|s| {
        s.chat_context_open = !s.chat_context_open;
        opened = s.chat_context_open;
        if opened {
            s.chat_changes_open = false;
        }
    });
    if opened {
        let missing = state().with(|s| {
            s.active_chat_id()
                .and_then(|id| s.chat_sessions.get(id))
                .filter(|chat| chat.context.is_none() && !chat.is_running())
                .map(|chat| chat.id.clone())
        });
        if let Some(id) = missing {
            refresh_chat_context(id);
        }
    }
}

pub fn chat_changes_open() -> bool {
    state().with(|s| s.chat_changes_open)
}

/// Shows or hides the Changes / Git drawer for the open chat, refreshing git status on open.
pub fn toggle_chat_changes() {
    let mut opened = false;
    let mut active_id = None;
    state().update(|s| {
        s.chat_changes_open = !s.chat_changes_open;
        opened = s.chat_changes_open;
        if opened {
            s.chat_context_open = false;
            active_id = s.active_chat_id().cloned();
        }
    });
    if opened && let Some(id) = active_id {
        refresh_chat_git_status(id);
    }
}

/// Refreshes the open chat's git status and diffs.
pub fn refresh_active_chat_changes() {
    if let Some(id) = active_chat_id() {
        refresh_chat_git_status(id);
    }
}

pub fn active_chat_changes() -> Option<ActiveChatChanges> {
    state().with(|s| {
        let chat = s.active_chat_id().and_then(|id| s.chat_sessions.get(id))?;
        let project = s.factory.project(chat.project_id)?;
        let settings = s.project_settings.get(&project.name);
        let default_branch = settings
            .map(|ps| ps.default_branch.clone())
            .filter(|b| !b.trim().is_empty())
            .unwrap_or_else(|| "main".to_owned());
        let open_prs_as_draft = settings.is_some_and(|ps| ps.open_prs_as_draft);
        let (working_dir, is_worktree) = match (chat.workspace, chat.worktree.as_ref()) {
            (ChatWorkspace::Worktree, Some(wt)) => (wt.path.clone(), true),
            _ => (project.root_path.clone(), false),
        };
        Some(ActiveChatChanges {
            chat_id: chat.id.clone(),
            title: chat.display_title(),
            working_dir,
            is_worktree,
            default_branch,
            open_prs_as_draft,
            running: chat.is_running(),
            loading: s.chat_git_loading.contains(&chat.id),
            busy: s.chat_git_busy.contains(&chat.id),
            status: s.chat_git_status.get(&chat.id).cloned(),
            error: s.chat_git_errors.get(&chat.id).cloned(),
            feedback: s.chat_git_feedback.get(&chat.id).cloned(),
            pr_url: s.chat_pr_urls.get(&chat.id).cloned(),
        })
    })
}

/// Inspects the chat's git status and diffs on a background thread.
fn refresh_chat_git_status(chat_id: String) {
    let mut job = None;
    state().update(|s| {
        if s.using_sample_projects || s.chat_git_loading.contains(&chat_id) {
            return;
        }
        let Some(chat) = s.chat_sessions.get(&chat_id) else {
            return;
        };
        let Some(project) = s.factory.project(chat.project_id) else {
            return;
        };
        if !project.root_path.join(".git").exists() {
            return;
        }
        let default_branch = s
            .project_settings
            .get(&project.name)
            .map(|ps| ps.default_branch.clone())
            .filter(|b| !b.trim().is_empty())
            .unwrap_or_else(|| "main".to_owned());
        let (working_dir, is_worktree) = match (chat.workspace, chat.worktree.as_ref()) {
            (ChatWorkspace::Worktree, Some(wt)) => (wt.path.clone(), true),
            _ => (project.root_path.clone(), false),
        };
        if is_worktree && !working_dir.is_dir() {
            return;
        }
        s.chat_git_loading.insert(chat_id.clone());
        job = Some((working_dir, default_branch, is_worktree));
    });
    let Some((working_dir, default_branch, is_worktree)) = job else {
        return;
    };
    Owner::detached().run(|| {
        spawn(async move {
            let result = spawn_blocking_task(move || {
                inspect_chat_git_status(&working_dir, Some(&default_branch), is_worktree)
            })
            .await;
            state().update(|s| {
                s.chat_git_loading.remove(&chat_id);
                match result {
                    Ok(status) => {
                        s.chat_git_errors.remove(&chat_id);
                        s.chat_git_status.insert(chat_id.clone(), status);
                    }
                    Err(err) => {
                        s.chat_git_errors.insert(chat_id.clone(), err.to_string());
                    }
                }
            });
        });
    });
}

/// Sends a message to the active chat's agent asking it to review and commit the uncommitted changes.
pub fn ask_agent_to_commit_active_chat() -> bool {
    const COMMIT_PROMPT: &str = "Please review the uncommitted changes, stage what belongs in the commit, and create a git commit with a clear commit message.";
    let now = current_unix_seconds();
    let mut prepared = None;
    state().update(|s| {
        let Some(chat_id) = s.active_chat_id().cloned() else {
            return;
        };
        if s.using_sample_projects || s.chat_git_busy.contains(&chat_id) {
            return;
        }
        s.chat_git_errors.remove(&chat_id);
        s.chat_git_feedback.remove(&chat_id);
        prepared = Some(s.begin_chat_turn(COMMIT_PROMPT, &[], now));
        if let Some(Err(message)) = &prepared {
            s.chat_git_errors.insert(chat_id, message.clone());
            s.status_message = Some(message.clone());
        } else {
            s.status_message = None;
        }
    });
    match prepared {
        Some(Ok((chat_id, request))) => {
            start_chat_turn(chat_id, request);
            true
        }
        _ => false,
    }
}

/// Pushes the active chat's branch and opens a GitHub pull request via `gh pr create`.
pub fn open_active_chat_pr(title: String, body: String, draft: bool) {
    let mut job = None;
    state().update(|s| {
        let Some(chat_id) = s.active_chat_id().cloned() else {
            return;
        };
        if s.using_sample_projects || s.chat_git_busy.contains(&chat_id) {
            return;
        }
        let Some(chat) = s.chat_sessions.get(&chat_id) else {
            return;
        };
        let Some(project) = s.factory.project(chat.project_id) else {
            return;
        };
        let settings = s.project_settings.get(&project.name);
        let default_branch = settings
            .map(|ps| ps.default_branch.clone())
            .filter(|b| !b.trim().is_empty())
            .unwrap_or_else(|| "main".to_owned());
        let auth_mode = settings
            .map(|ps| ps.github_auth_mode)
            .unwrap_or(IssueSourceAuthMode::GitHubCli);
        let pat_id = settings.and_then(|ps| ps.github_pat_id.clone());
        let (working_dir, branch) = match (chat.workspace, chat.worktree.as_ref()) {
            (ChatWorkspace::Worktree, Some(wt)) => (wt.path.clone(), wt.branch.clone()),
            _ => (
                project.root_path.clone(),
                s.chat_git_status
                    .get(&chat_id)
                    .map(|g| g.branch.clone())
                    .unwrap_or_default(),
            ),
        };
        let ssh_auth_sock = {
            let configured = s.preferences.advanced.ssh_auth_sock.trim();
            if configured.is_empty() {
                None
            } else {
                Some(configured.to_owned())
            }
        };
        let path_env = s.claude_path_lookup.resolve_path_env();
        let resolver = Arc::clone(&s.secret_resolver);
        s.chat_git_busy.insert(chat_id.clone());
        s.chat_git_errors.remove(&chat_id);
        s.chat_git_feedback.remove(&chat_id);
        job = Some((
            chat_id,
            working_dir,
            branch,
            default_branch,
            title,
            body,
            draft,
            auth_mode,
            pat_id,
            resolver,
            ssh_auth_sock,
            path_env,
        ));
    });
    let Some((
        chat_id,
        working_dir,
        branch,
        default_branch,
        title,
        body,
        draft,
        auth_mode,
        pat_id,
        resolver,
        ssh_auth_sock,
        path_env,
    )) = job
    else {
        return;
    };
    Owner::detached().run(|| {
        spawn(async move {
            let result = spawn_blocking_task(move || {
                let (github_auth, redactor) =
                    resolve_github_auth_off_thread(auth_mode, pat_id.as_ref(), &resolver)?;
                let res = open_chat_pull_request(&ChatPullRequestRequest {
                    working_dir,
                    branch,
                    base_branch: Some(default_branch),
                    title,
                    body,
                    draft,
                    github_auth: Some(github_auth),
                    gh_binary: None,
                    ssh_auth_sock,
                    path_env,
                })
                .map_err(|err| redactor.redact(&err.to_string()))?;
                Ok::<_, String>(res)
            })
            .await;
            state().update(|s| {
                s.chat_git_busy.remove(&chat_id);
                match result {
                    Ok(pr) => {
                        let label = if pr.draft {
                            "Opened draft PR"
                        } else {
                            "Opened PR"
                        };
                        let note = if pr.url.is_empty() {
                            format!("{label} for {}", pr.branch)
                        } else {
                            format!("{label}: {}", pr.url)
                        };
                        if !pr.url.is_empty() {
                            s.chat_pr_urls.insert(chat_id.clone(), pr.url);
                        }
                        s.chat_git_feedback.insert(chat_id.clone(), note.clone());
                        s.status_message = Some(note);
                    }
                    Err(msg) => {
                        s.chat_git_errors.insert(chat_id.clone(), msg.clone());
                        s.status_message = Some(msg);
                    }
                }
            });
            refresh_chat_git_status(chat_id);
        });
    });
}

/// Applies the active worktree chat's changes (patch or commits) to the main project checkout.
pub fn apply_active_chat_to_checkout() {
    let mut job = None;
    state().update(|s| {
        let Some(chat_id) = s.active_chat_id().cloned() else {
            return;
        };
        if s.using_sample_projects || s.chat_git_busy.contains(&chat_id) {
            return;
        }
        let Some(chat) = s.chat_sessions.get(&chat_id) else {
            return;
        };
        let Some(project) = s.factory.project(chat.project_id) else {
            return;
        };
        let Some(worktree) = chat
            .worktree
            .as_ref()
            .filter(|_| chat.workspace == ChatWorkspace::Worktree)
        else {
            return;
        };
        let default_branch = s
            .project_settings
            .get(&project.name)
            .map(|ps| ps.default_branch.clone())
            .filter(|b| !b.trim().is_empty())
            .unwrap_or_else(|| "main".to_owned());
        let ssh_auth_sock = {
            let configured = s.preferences.advanced.ssh_auth_sock.trim();
            if configured.is_empty() {
                None
            } else {
                Some(configured.to_owned())
            }
        };
        s.chat_git_busy.insert(chat_id.clone());
        s.chat_git_errors.remove(&chat_id);
        s.chat_git_feedback.remove(&chat_id);
        job = Some((
            chat_id,
            project.root_path.clone(),
            worktree.path.clone(),
            default_branch,
            ssh_auth_sock,
        ));
    });
    let Some((chat_id, project_root, worktree_dir, default_branch, ssh_auth_sock)) = job else {
        return;
    };
    Owner::detached().run(|| {
        spawn(async move {
            let result = spawn_blocking_task(move || {
                apply_chat_worktree_to_checkout(
                    &project_root,
                    &worktree_dir,
                    Some(&default_branch),
                    ssh_auth_sock.as_deref(),
                )
            })
            .await;
            state().update(|s| {
                s.chat_git_busy.remove(&chat_id);
                match result {
                    Ok(applied) => {
                        s.chat_git_feedback
                            .insert(chat_id.clone(), applied.summary.clone());
                        s.status_message = Some(applied.summary);
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        s.chat_git_errors.insert(chat_id.clone(), msg.clone());
                        s.status_message = Some(msg);
                    }
                }
            });
            refresh_chat_git_status(chat_id);
        });
    });
}

/// Opens the active chat's working directory in Terminal.app.
pub fn open_active_chat_in_terminal() {
    let dir = state().with(|s| {
        let chat = s.active_chat_id().and_then(|id| s.chat_sessions.get(id))?;
        let project = s.factory.project(chat.project_id)?;
        Some(match (chat.workspace, chat.worktree.as_ref()) {
            (ChatWorkspace::Worktree, Some(wt)) => wt.path.clone(),
            _ => project.root_path.clone(),
        })
    });
    if let Some(dir) = dir
        && let Err(err) = open_dir_in_terminal(&dir)
    {
        set_status_message(err.to_string());
    }
}

/// Opens the pull request URL created from the active chat in the default browser.
pub fn open_active_chat_pr_url() {
    let url = state().with(|s| {
        let id = s.active_chat_id()?;
        s.chat_pr_urls.get(id).cloned()
    });
    if let Some(url) = url
        && let Err(err) = open_url_in_browser(&url)
    {
        set_status_message(err.to_string());
    }
}

/// Reads the open chat's context window again.
pub fn refresh_active_chat_context() {
    if let Some(id) = active_chat_id() {
        refresh_chat_context(id);
    }
}

pub fn active_chat_context() -> Option<ActiveChatContext> {
    state().with(|s| {
        let chat = s.active_chat_id().and_then(|id| s.chat_sessions.get(id))?;
        Some(ActiveChatContext {
            chat_id: chat.id.clone(),
            provider: chat.provider,
            usage: chat.context.clone(),
            loading: s.chat_context_loading.contains(&chat.id),
            error: s.chat_context_errors.get(&chat.id).cloned(),
            has_session: chat.provider_session_id.is_some(),
            running: chat.is_running(),
            tokens: chat.token_use(),
            recorded_tokens: chat.total_tokens,
        })
    })
}

/// Stops the open chat's running reply.
pub fn stop_active_chat() {
    let Some(id) = active_chat_id() else {
        return;
    };
    log::info!(target: "bruk::chat", "chat.stop_requested chat_id={id}");
    let control = chat_registry()
        .lock()
        .ok()
        .and_then(|reg| reg.active.get(&id).cloned());
    match control {
        Some(control) => control.stop(),
        // Nothing is running it any more: settle it here.
        None => state().update(|s| {
            if let Some(chat) = s.chat_sessions.get_mut(&id)
                && chat.is_running()
            {
                chat.apply_event(
                    &ChatEvent::TurnFinished {
                        status: ChatSessionStatus::Stopped,
                        error: None,
                    },
                    current_unix_seconds(),
                );
                s.persist_chat(&id);
            }
        }),
    }
}

/// Deletes a chat and its runtime files, stopping it first if it is replying.
pub fn delete_chat(id: &str) {
    let control = chat_registry()
        .lock()
        .ok()
        .and_then(|reg| reg.active.get(id).cloned());
    if let Some(control) = control {
        control.stop();
    }
    state().update(|s| {
        let Some(chat) = s.chat_sessions.remove(id) else {
            return;
        };
        log::info!(
            target: "bruk::chat",
            "chat.deleted chat_id={id} provider={} had_worktree={}",
            chat.provider.label(),
            chat.worktree.is_some()
        );
        if !s.using_sample_projects
            && let Some(db_arc) = s.db.as_ref()
            && let Ok(db) = db_arc.lock()
        {
            let _ = db.delete_chat_session(id);
        }
        // Remove the worktree through git (so the repository forgets it), then the chat's
        // folder; off the main thread, since a big checkout takes a moment.
        let worktree = chat
            .worktree
            .clone()
            .filter(|_| chat.workspace == ChatWorkspace::Worktree);
        let project_root = s
            .factory
            .project(chat.project_id)
            .map(|project| project.root_path.clone());
        let chat_dir = s.home.as_ref().map(|home| home.chat_dir(id));
        std::thread::spawn(move || {
            if let (Some(worktree), Some(root)) = (worktree.as_ref(), project_root.as_ref()) {
                let _ = remove_chat_worktree(root, &worktree.path);
            }
            if let Some(dir) = chat_dir {
                let _ = std::fs::remove_dir_all(dir);
            }
        });
        if s.chat_selected.get(&chat.project_id).map(String::as_str) == Some(id) {
            s.chat_selected.remove(&chat.project_id);
        }
        s.chat_context_loading.remove(id);
        s.chat_context_errors.remove(id);
        s.chat_git_status.remove(id);
        s.chat_git_loading.remove(id);
        s.chat_git_busy.remove(id);
        s.chat_git_errors.remove(id);
        s.chat_git_feedback.remove(id);
        s.chat_pr_urls.remove(id);
        s.chat_secrets_caches.remove(id);
        s.status_message = Some(match chat.worktree.as_ref() {
            Some(worktree) if chat.workspace == ChatWorkspace::Worktree => format!(
                "Deleted chat '{}' and its worktree. Branch {} is kept.",
                chat.display_title(),
                worktree.branch
            ),
            _ => format!("Deleted chat '{}'", chat.display_title()),
        });
        s.refresh_dashboard_dataset();
    });
}

/// Puts a chat away: it leaves the chat list for its Archived section and takes no messages
/// until unarchived. Its transcript and worktree stay; once its branch merges, Prune Merged
/// Worktrees cleans the worktree up like any other.
pub fn archive_chat(id: &str) {
    state().update(|s| {
        let Some(chat) = s.chat_sessions.get_mut(id) else {
            return;
        };
        if chat.is_archived() {
            return;
        }
        if chat.is_running() {
            s.status_message =
                Some("Wait for the reply, or stop it, before archiving the chat".to_owned());
            return;
        }
        chat.archived_at = Some(current_unix_seconds());
        let title = chat.display_title();
        log::info!(target: "bruk::chat", "chat.archived chat_id={id}");
        s.persist_chat(id);
        s.status_message = Some(format!("Archived chat '{title}'"));
    });
}

/// Takes an archived chat out of the archive. If its worktree was pruned, its next message
/// creates a new one from the checkout's `HEAD`.
pub fn unarchive_chat(id: &str) {
    state().update(|s| {
        let Some(chat) = s.chat_sessions.get_mut(id) else {
            return;
        };
        if chat.archived_at.take().is_none() {
            return;
        }
        let title = chat.display_title();
        log::info!(target: "bruk::chat", "chat.unarchived chat_id={id}");
        s.persist_chat(id);
        s.status_message = Some(format!("Unarchived chat '{title}'"));
    });
}

/// Whether the selected project can have its merged chat worktrees pruned (a git project).
pub fn can_prune_merged_worktrees() -> bool {
    state().with(|s| {
        !s.using_sample_projects
            && s.selected_project
                .and_then(|pid| s.factory.project(pid))
                .is_some_and(|project| project.root_path.join(".git").exists())
    })
}

/// Whether the selected project's merged chat worktrees are being pruned right now.
pub fn pruning_merged_worktrees() -> bool {
    state().with(|s| {
        s.selected_project
            .is_some_and(|pid| s.worktree_prune_projects.contains(&pid))
    })
}

/// A chat worktree checked for a merged branch.
struct WorktreePruneCandidate {
    chat_id: String,
    title: String,
    worktree: ChatWorktree,
}

enum WorktreePruneOutcome {
    /// The branch merged; the worktree is gone and the branch was cleaned up.
    Pruned {
        evidence: String,
        branch: MergedBranchCleanup,
    },
    /// The branch merged but the worktree has uncommitted changes, so it stays.
    MergedButDirty,
    NotMerged,
    /// Neither the worktree nor its branch exist (a first turn that never ran).
    NothingToPrune,
    Failed(String),
}

/// Tallies a prune for its status message.
#[derive(Default)]
struct WorktreePruneReport {
    archived: Vec<String>,
    dirty: Vec<String>,
    failed: Vec<(String, String)>,
    not_merged: usize,
    local_branches_deleted: usize,
    remote_branches_deleted: usize,
    /// `remote/branch` names kept because they're at other commits.
    remote_branches_kept: Vec<String>,
    branch_failures: Vec<(String, String)>,
}

/// Removes the worktree of every chat in the selected project whose branch has merged into
/// the project's default branch (locally, on `origin` after fetching it, or as a merged GitHub
/// pull request), deletes the branch locally and on its remote (when the remote branch is at
/// the same commit), and archives those chats. Worktrees with uncommitted changes, and chats
/// that are replying, are left alone.
pub fn prune_merged_worktrees() {
    let mut job = None;
    state().update(|s| {
        if s.using_sample_projects {
            return;
        }
        let Some(pid) = s.selected_project else {
            return;
        };
        if s.worktree_prune_projects.contains(&pid) {
            return;
        }
        let Some(project) = s.factory.project(pid).cloned() else {
            return;
        };
        if !project.root_path.join(".git").exists() {
            s.status_message = Some(format!(
                "{} isn't a git repository, so it has no worktrees to prune.",
                project.name
            ));
            return;
        }
        let candidates: Vec<WorktreePruneCandidate> = s
            .chat_sessions
            .values()
            .filter(|chat| {
                chat.project_id == pid && !chat.is_running() && !s.chat_git_busy.contains(&chat.id)
            })
            .filter_map(|chat| {
                chat.worktree_in_use()
                    // An archived chat counts while its worktree is still there.
                    .filter(|worktree| !chat.is_archived() || worktree.path.is_dir())
                    .map(|worktree| WorktreePruneCandidate {
                        chat_id: chat.id.clone(),
                        title: chat.display_title(),
                        worktree: worktree.clone(),
                    })
            })
            .collect();
        if candidates.is_empty() {
            s.status_message = Some(format!(
                "No chats in {} have a worktree to prune.",
                project.name
            ));
            return;
        }
        let settings = s.project_settings.get(&project.name);
        let default_branch = settings
            .map(|ps| ps.default_branch.clone())
            .filter(|b| !b.trim().is_empty())
            .unwrap_or_else(|| "main".to_owned());
        let auth_mode = settings
            .map(|ps| ps.github_auth_mode)
            .unwrap_or(IssueSourceAuthMode::GitHubCli);
        let pat_id = settings.and_then(|ps| ps.github_pat_id.clone());
        let ssh_auth_sock = Some(s.preferences.advanced.ssh_auth_sock.trim().to_owned())
            .filter(|sock| !sock.is_empty());
        let path_env = s.claude_path_lookup.resolve_path_env();
        let resolver = Arc::clone(&s.secret_resolver);
        s.worktree_prune_projects.insert(pid);
        s.chats_pruning
            .extend(candidates.iter().map(|c| c.chat_id.clone()));
        s.status_message = Some(format!(
            "Looking for merged branches among {} chat worktree{} in {}…",
            candidates.len(),
            if candidates.len() == 1 { "" } else { "s" },
            project.name
        ));
        log::info!(
            target: "bruk::chat",
            "chat.worktree.prune_started project={:?} candidates={} base={default_branch}",
            project.name,
            candidates.len()
        );
        job = Some((
            pid,
            project.name.clone(),
            project.root_path.clone(),
            candidates,
            default_branch,
            auth_mode,
            pat_id,
            resolver,
            ssh_auth_sock,
            path_env,
        ));
    });
    let Some((
        pid,
        project_name,
        project_root,
        candidates,
        default_branch,
        auth_mode,
        pat_id,
        resolver,
        ssh_auth_sock,
        path_env,
    )) = job
    else {
        return;
    };
    Owner::detached().run(|| {
        spawn(async move {
            let results = spawn_blocking_task(move || {
                if let Err(err) =
                    fetch_base_branch(&project_root, &default_branch, ssh_auth_sock.as_deref())
                {
                    log::warn!(
                        target: "bruk::chat",
                        "chat.worktree.prune_fetch_failed base={default_branch} err={err}"
                    );
                }
                // Without `gh` (or its auth), only merges git can see locally count.
                let github = resolve_github_auth_off_thread(auth_mode, pat_id.as_ref(), &resolver)
                    .ok()
                    .and_then(|(github_auth, _)| {
                        resolve_gh_binary(path_env.as_deref())
                            .ok()
                            .map(|gh_binary| GitHubPullRequestLookup {
                                gh_binary,
                                github_auth,
                                path_env: path_env.clone(),
                            })
                    });
                candidates
                    .into_iter()
                    .map(|candidate| {
                        let outcome = prune_chat_worktree_if_merged(
                            &project_root,
                            &default_branch,
                            &candidate,
                            github.as_ref(),
                            ssh_auth_sock.as_deref(),
                        );
                        (candidate, outcome)
                    })
                    .collect::<Vec<_>>()
            })
            .await;
            state().update(|s| {
                s.worktree_prune_projects.remove(&pid);
                let now = current_unix_seconds();
                let mut report = WorktreePruneReport::default();
                for (candidate, outcome) in results {
                    s.chats_pruning.remove(&candidate.chat_id);
                    match outcome {
                        WorktreePruneOutcome::Pruned { evidence, branch } => {
                            log::info!(
                                target: "bruk::chat",
                                "chat.worktree.pruned chat_id={} branch={} evidence={evidence:?} cleanup={branch:?}",
                                candidate.chat_id,
                                candidate.worktree.branch
                            );
                            report.tally_branch(&candidate.worktree.branch, branch);
                            let Some(chat) = s.chat_sessions.get_mut(&candidate.chat_id) else {
                                continue;
                            };
                            chat.archived_at.get_or_insert(now);
                            s.persist_chat(&candidate.chat_id);
                            s.chat_git_status.remove(&candidate.chat_id);
                            s.chat_git_errors.remove(&candidate.chat_id);
                            s.chat_git_feedback.remove(&candidate.chat_id);
                            report.archived.push(candidate.title);
                        }
                        WorktreePruneOutcome::MergedButDirty => {
                            report.dirty.push(candidate.title);
                        }
                        WorktreePruneOutcome::NotMerged => report.not_merged += 1,
                        WorktreePruneOutcome::NothingToPrune => {}
                        WorktreePruneOutcome::Failed(err) => {
                            log::warn!(
                                target: "bruk::chat",
                                "chat.worktree.prune_failed chat_id={} err={err}",
                                candidate.chat_id
                            );
                            report.failed.push((candidate.title, err));
                        }
                    }
                }
                s.status_message = Some(report.summary(&project_name));
            });
        });
    });
}

fn prune_chat_worktree_if_merged(
    project_root: &Path,
    default_branch: &str,
    candidate: &WorktreePruneCandidate,
    github: Option<&GitHubPullRequestLookup>,
    ssh_auth_sock: Option<&str>,
) -> WorktreePruneOutcome {
    let path = &candidate.worktree.path;
    let evidence = match chat_branch_merge_evidence(
        project_root,
        &candidate.worktree.branch,
        default_branch,
        github,
    ) {
        Ok(Some(evidence)) => evidence,
        Ok(None) => return WorktreePruneOutcome::NotMerged,
        // A worktree planned for a first turn that never ran has no branch yet.
        Err(_) if !path.exists() => return WorktreePruneOutcome::NothingToPrune,
        Err(err) => return WorktreePruneOutcome::Failed(err),
    };
    if path.is_dir() {
        match worktree_has_uncommitted_changes(path) {
            Ok(false) => {}
            Ok(true) => return WorktreePruneOutcome::MergedButDirty,
            Err(err) => return WorktreePruneOutcome::Failed(err),
        }
    }
    if let Err(err) = remove_chat_worktree(project_root, path) {
        return WorktreePruneOutcome::Failed(err);
    }
    WorktreePruneOutcome::Pruned {
        evidence: evidence.describe(),
        branch: delete_merged_branch(project_root, &candidate.worktree.branch, ssh_auth_sock),
    }
}

impl WorktreePruneReport {
    fn tally_branch(&mut self, branch: &str, cleanup: MergedBranchCleanup) {
        match cleanup.local {
            Ok(true) => self.local_branches_deleted += 1,
            Ok(false) => {}
            Err(err) => self.branch_failures.push((branch.to_owned(), err)),
        }
        match cleanup.remote {
            RemoteBranchCleanup::NotOnRemote => {}
            RemoteBranchCleanup::Deleted { .. } => self.remote_branches_deleted += 1,
            RemoteBranchCleanup::Diverged { remote, branch } => {
                self.remote_branches_kept.push(format!("{remote}/{branch}"));
            }
            RemoteBranchCleanup::Failed(err) => {
                self.branch_failures
                    .push((format!("{branch} on its remote"), err));
            }
        }
    }

    /// What the status bar says once the prune finishes.
    fn summary(&self, project_name: &str) -> String {
        fn quoted(titles: &[String]) -> String {
            titles
                .iter()
                .map(|t| format!("'{t}'"))
                .collect::<Vec<_>>()
                .join(", ")
        }
        fn plural(count: usize, one: &str, many: &str) -> String {
            format!("{count} {}", if count == 1 { one } else { many })
        }
        let mut parts = Vec::new();
        if self.archived.is_empty() {
            parts.push(format!(
                "No merged chat worktrees to prune in {project_name}."
            ));
        } else {
            parts.push(format!(
                "Pruned {} and archived {}: {}.",
                plural(self.archived.len(), "merged worktree", "merged worktrees"),
                if self.archived.len() == 1 {
                    "its chat"
                } else {
                    "their chats"
                },
                quoted(&self.archived)
            ));
        }
        match (self.local_branches_deleted, self.remote_branches_deleted) {
            (0, 0) => {}
            (local, 0) => parts.push(format!(
                "Deleted {}.",
                plural(local, "local branch", "local branches")
            )),
            (0, remote) => parts.push(format!(
                "Deleted {}.",
                plural(remote, "remote branch", "remote branches")
            )),
            (local, remote) => parts.push(format!(
                "Deleted {} and {}.",
                plural(local, "local branch", "local branches"),
                plural(remote, "remote branch", "remote branches")
            )),
        }
        if !self.remote_branches_kept.is_empty() {
            parts.push(format!(
                "Kept {} at other commits than the local branch.",
                self.remote_branches_kept.join(", ")
            ));
        }
        for (branch, err) in &self.branch_failures {
            parts.push(format!("Couldn't delete branch {branch}: {err}."));
        }
        if !self.dirty.is_empty() {
            parts.push(format!(
                "Kept {} with uncommitted changes: {}.",
                plural(self.dirty.len(), "merged worktree", "merged worktrees"),
                quoted(&self.dirty)
            ));
        }
        for (title, err) in &self.failed {
            parts.push(format!("Couldn't prune '{title}': {err}."));
        }
        if self.not_merged > 0 {
            parts.push(format!(
                "{} not merged yet.",
                plural(self.not_merged, "branch", "branches")
            ));
        }
        parts.join(" ")
    }
}

// --- Dashboard state --------------------------------------------------------------------

// --- Work Queue state -------------------------------------------------------------------

pub fn active_work_queue() -> ProjectWorkQueue {
    state().with(|s| {
        let name = s.selected_project_name();
        s.queue_for_name(&name)
    })
}

pub fn work_queue_for_project(id: ProjectId) -> ProjectWorkQueue {
    state().with(|s| {
        let name = s
            .factory
            .project(id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        s.queue_for_name(&name)
    })
}

pub fn work_queue_view_mode() -> WorkQueueViewMode {
    state().with(|s| s.work_queue_view_mode)
}

pub fn set_work_queue_view_mode(mode: WorkQueueViewMode) {
    state().update(|s| {
        s.work_queue_view_mode = mode;
    });
}

pub fn selected_work_queue_issue() -> Option<u32> {
    state().with(|s| s.work_queue_selected_issue)
}

pub fn select_work_queue_issue(issue_number: u32) {
    state().update(|s| {
        s.work_queue_selected_issue = Some(issue_number);
        s.work_queue_inspector_visible = true;
    });
}

pub fn work_queue_inspector_visible() -> bool {
    state().with(|s| s.work_queue_inspector_visible)
}

pub fn toggle_work_queue_inspector() {
    state().update(|s| {
        s.work_queue_inspector_visible = !s.work_queue_inspector_visible;
    });
}

pub fn work_queue_ad_hoc_open() -> bool {
    state().with(|s| s.work_queue_ad_hoc_open)
}

pub fn set_work_queue_ad_hoc_open(open: bool) {
    state().update(|s| {
        s.work_queue_ad_hoc_open = open;
    });
}

pub fn open_work_queue_ad_hoc() {
    state().update(|s| {
        if s.selected_project.is_none() {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
        }
        if let Some(pid) = s.selected_project {
            s.expanded_projects.insert(pid);
        }
        s.active_view = ActiveView::WorkQueue;
        s.work_queue_ad_hoc_open = true;
        s.sync_selected_issue_for_active_project();
    });
}

pub fn work_queue_unmatched_expanded() -> bool {
    state().with(|s| s.work_queue_unmatched_expanded)
}

pub fn work_queue_syncing() -> bool {
    state().with(|s| s.work_queue_syncing)
}

pub fn toggle_work_queue_unmatched_expanded() {
    state().update(|s| {
        s.work_queue_unmatched_expanded = !s.work_queue_unmatched_expanded;
    });
}

fn resolve_github_auth_off_thread(
    mode: IssueSourceAuthMode,
    pat_secret_id: Option<&SecretId>,
    resolver: &SecretResolver,
) -> Result<(GitHubAuth, SecretRedactor), String> {
    match mode {
        IssueSourceAuthMode::GitHubCli => Ok((GitHubAuth::GitHubCli, SecretRedactor::empty())),
        IssueSourceAuthMode::PersonalToken => {
            let Some(secret_id) = pat_secret_id else {
                return Err(
                    "No personal access token is selected for this project. Select or add a token in Project Settings, or switch to GitHub CLI."
                        .to_owned(),
                );
            };
            let cancel = CancellationToken::new();
            let secret = resolver
                .resolve_internal(secret_id, &cancel)
                .map_err(|failure| failure.to_problem(None).reason)?;
            let raw = secret.expose_secret().to_owned();
            if raw.trim().is_empty() {
                return Err(
                    "The selected GitHub personal access token is empty. Replace it in Secrets Settings."
                        .to_owned(),
                );
            }
            let redactor = SecretRedactor::from_secrets([("internal", &secret)]);
            Ok((GitHubAuth::PersonalToken(raw), redactor))
        }
    }
}

fn spawn_github_working_label_update(issue_number: u32, add: bool) {
    let job = state().with_untracked(|s| {
        if s.demo_mode {
            return None;
        }
        let settings = s.active_project_settings_clone();
        if settings.issue_source != IssueSourceKind::GitHubIssues
            || !settings.source_connected
            || !settings.add_working_label
            || settings.github_repo.trim().is_empty()
        {
            return None;
        }
        let path_env = s.claude_path_lookup.resolve_path_env();
        Some((
            settings.github_repo,
            settings.github_auth_mode,
            settings.github_pat_id,
            Arc::clone(&s.secret_resolver),
            path_env,
        ))
    });

    if let Some((repo, auth_mode, pat_id, resolver, path_env)) = job {
        Owner::detached().run(|| {
            spawn(async move {
                let result = spawn_blocking_task(move || {
                    let (auth, redactor) =
                        resolve_github_auth_off_thread(auth_mode, pat_id.as_ref(), &resolver)?;
                    let res = if add {
                        add_github_issue_label(
                            &repo,
                            issue_number,
                            AGENT_WORKING_LABEL,
                            &auth,
                            path_env.as_deref(),
                        )
                    } else {
                        remove_github_issue_label(
                            &repo,
                            issue_number,
                            AGENT_WORKING_LABEL,
                            &auth,
                            path_env.as_deref(),
                        )
                    };
                    res.map_err(|err| redactor.redact(&err.to_string()))
                })
                .await;
                state().update(|s| {
                    s.flush_secret_resolver_updates();
                    if let Err(err) = result {
                        let verb = if add { "add" } else { "remove" };
                        s.status_message = Some(format!(
                            "Could not {verb} '{AGENT_WORKING_LABEL}' label on #{issue_number}: {err}"
                        ));
                    }
                });
            });
        });
    }
}

pub fn start_work_queue_issue(issue_number: u32) {
    let mut started = false;
    let mut real_run_args: Option<(String, String, String, u32)> = None;
    state().update(|s| {
        let name = s.selected_project_name();
        let demo = s.demo_mode;
        let queue = s.ensure_queue_mut(&name);
        let issue_meta = queue
            .items
            .iter()
            .find(|i| i.issue_number == issue_number)
            .map(|i| (i.workflow_name.clone(), i.title.clone()));
        match queue.start_issue(issue_number) {
            Ok(run_number) => {
                s.work_queue_selected_issue = Some(issue_number);
                s.status_message = Some(format!(
                    "Started issue #{issue_number} as run #{run_number}"
                ));
                s.persist_active_work_queue();
                started = true;
                if !demo && let Some((wf_name, title)) = issue_meta {
                    real_run_args = Some((name, wf_name, title, issue_number));
                }
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
    if started {
        spawn_github_working_label_update(issue_number, true);
    }
    if let Some((project_name, wf_name, title, issue_no)) = real_run_args {
        let _ = spawn_real_workflow_run(
            &project_name,
            &wf_name,
            &format!("Issue #{issue_no}: {title}"),
            Some(issue_no),
            false,
        );
    }
}

pub fn start_next_queued_issue() {
    let mut started_issue = None;
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        match queue.start_next_queued() {
            Ok((issue_number, run_number)) => {
                s.active_view = ActiveView::WorkQueue;
                s.work_queue_selected_issue = Some(issue_number);
                s.status_message = Some(format!(
                    "Started next queued issue #{issue_number} as run #{run_number}"
                ));
                s.persist_active_work_queue();
                started_issue = Some(issue_number);
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
    if let Some(issue_number) = started_issue {
        spawn_github_working_label_update(issue_number, true);
    }
}

pub fn reassign_work_queue_workflow(issue_number: u32, workflow_name: &str) {
    let wf = workflow_name.to_owned();
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        match queue.reassign_workflow(issue_number, &wf) {
            Ok(()) => {
                s.status_message = Some(format!("Assigned #{issue_number} to workflow '{wf}'"));
                s.persist_active_work_queue();
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub fn skip_work_queue_issue(issue_number: u32) {
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        match queue.skip_issue(issue_number) {
            Ok(()) => {
                let next_selected = queue.default_selected_issue();
                s.work_queue_selected_issue = next_selected;
                s.status_message = Some(format!("Skipped issue #{issue_number}"));
                s.persist_active_work_queue();
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub fn approve_work_queue_review(issue_number: u32) {
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        match queue.approve_review(issue_number) {
            Ok(()) => {
                s.work_queue_selected_issue = Some(issue_number);
                s.status_message = Some(format!(
                    "Approved review gate for #{issue_number} — PR merged"
                ));
                s.persist_active_work_queue();
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub fn request_changes_work_queue_review(issue_number: u32) {
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        match queue.request_changes(issue_number) {
            Ok(()) => {
                s.work_queue_selected_issue = Some(issue_number);
                s.status_message = Some(format!(
                    "Requested changes on #{issue_number} — resumed agent run"
                ));
                s.persist_active_work_queue();
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub fn stop_work_queue_issue(issue_number: u32) {
    let mut stopped = false;
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        match queue.stop_in_progress(issue_number) {
            Ok(()) => {
                s.work_queue_selected_issue = Some(issue_number);
                s.status_message = Some(format!("Stopped run for #{issue_number} and re-queued"));
                s.persist_active_work_queue();
                stopped = true;
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
    if stopped {
        spawn_github_working_label_update(issue_number, false);
    }
}

pub fn requeue_work_queue_issue(issue_number: u32) {
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        match queue.requeue_issue(issue_number) {
            Ok(()) => {
                s.work_queue_selected_issue = Some(issue_number);
                s.status_message = Some(format!("Re-queued issue #{issue_number}"));
                s.persist_active_work_queue();
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub fn queue_unmatched_work_issue(issue_number: u32) {
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        match queue.queue_unmatched_issue(issue_number) {
            Ok(()) => {
                s.work_queue_selected_issue = Some(issue_number);
                s.status_message = Some(format!("Added issue #{issue_number} to Ready for agents"));
                s.persist_active_work_queue();
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub fn open_work_queue_issue_url(url: &str) {
    let url_owned = url.to_owned();
    let should_open = state().with_untracked(|s| {
        !s.demo_mode
            && matches!(s.claude_path_lookup, ClaudePathLookup::SystemPath)
            && (url_owned.starts_with("https://") || url_owned.starts_with("http://"))
    });
    set_status_message(format!("Issue URL: {url_owned}"));
    if should_open {
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("/usr/bin/open")
                .arg(&url_owned)
                .spawn();
        }
    }
}

pub fn sync_work_queue() {
    sync_github_issues_for_active_project(false);
}

pub fn test_and_sync_project_github_source() {
    sync_github_issues_for_active_project(true);
}

fn sync_github_issues_for_active_project(from_settings: bool) {
    struct GitHubSyncJob {
        project_name: String,
        raw_repo: String,
        filter_clauses: Vec<GitHubFilterClause>,
        auth_mode: IssueSourceAuthMode,
        pat_id: Option<SecretId>,
        resolver: Arc<SecretResolver>,
        path_env: Option<OsString>,
    }

    let mut background_job: Option<GitHubSyncJob> = None;

    state().update(|s| {
        if s.demo_mode {
            let name = s.selected_project_name();
            let queue = s.ensure_queue_mut(&name);
            let slug = queue.repo_slug.clone();
            match queue.sync_issues() {
                Some(imported) => {
                    s.work_queue_selected_issue = Some(imported.issue_number);
                    let msg = format!(
                        "Synced {slug} — queued #{} {}",
                        imported.issue_number, imported.title
                    );
                    s.status_message = Some(msg.clone());
                    if from_settings {
                        s.project_settings_error = None;
                        s.project_settings_feedback = Some(msg);
                    }
                }
                None => {
                    let msg = format!("Synced {slug} — work queue is up to date");
                    s.status_message = Some(msg.clone());
                    if from_settings {
                        s.project_settings_error = None;
                        s.project_settings_feedback = Some(msg);
                    }
                }
            }
            return;
        }

        if s.work_queue_syncing {
            return;
        }

        let settings = s.active_project_settings_clone();
        if settings.project_name.trim().is_empty() {
            let msg = "Select or register a project before syncing issues.".to_owned();
            s.status_message = Some(msg.clone());
            if from_settings {
                s.project_settings_error = Some(msg);
            }
            return;
        }

        if settings.issue_source != IssueSourceKind::GitHubIssues {
            let msg = format!(
                "{} live sync is not wired yet — switch to GitHub Issues.",
                settings.issue_source.title()
            );
            s.status_message = Some(msg.clone());
            if from_settings {
                s.project_settings_feedback = None;
                s.project_settings_error = Some(msg);
            }
            return;
        }

        let raw_repo = settings.github_repo.trim().to_owned();
        if raw_repo.is_empty() {
            let msg =
                "Configure a GitHub repository (owner/repo) in Project Settings first.".to_owned();
            s.status_message = Some(msg.clone());
            if from_settings {
                s.project_settings_feedback = None;
                s.project_settings_error = Some(msg);
            }
            return;
        }

        let filter_clauses = settings.effective_intake_filter_clauses();
        let path_env = s.claude_path_lookup.resolve_path_env();
        s.work_queue_syncing = true;
        s.project_settings_error = None;
        let syncing_msg = format!("Syncing GitHub issues for {raw_repo}…");
        s.status_message = Some(syncing_msg.clone());
        if from_settings {
            s.project_settings_feedback = Some(syncing_msg);
        }

        background_job = Some(GitHubSyncJob {
            project_name: settings.project_name,
            raw_repo,
            filter_clauses,
            auth_mode: settings.github_auth_mode,
            pat_id: settings.github_pat_id,
            resolver: Arc::clone(&s.secret_resolver),
            path_env,
        });
    });

    if let Some(GitHubSyncJob {
        project_name,
        raw_repo,
        filter_clauses,
        auth_mode,
        pat_id,
        resolver,
        path_env,
    }) = background_job
    {
        Owner::detached().run(|| {
            spawn(async move {
                let result = spawn_blocking_task(move || {
                    let (auth, redactor) =
                        resolve_github_auth_off_thread(auth_mode, pat_id.as_ref(), &resolver)?;
                    fetch_github_open_issues_with_filter(
                        &raw_repo,
                        &filter_clauses,
                        &auth,
                        path_env.as_deref(),
                    )
                    .map_err(|err| redactor.redact(&err.to_string()))
                })
                .await;

                let mut auto_started_issues = Vec::new();
                state().update(|s| {
                    s.work_queue_syncing = false;
                    s.flush_secret_resolver_updates();
                    if s.demo_mode {
                        return;
                    }
                    match result {
                        Ok(snapshot) => {
                            let open_count = snapshot.open_issues.len() as u32;
                            if let Some(settings) = s.project_settings.get_mut(&project_name) {
                                settings.apply_github_sync_success(
                                    &snapshot.repo,
                                    open_count,
                                    snapshot.synced_at_unix,
                                );
                            }
                            let settings_snapshot = s.project_settings.get(&project_name).cloned();
                            let diff = {
                                let queue = s.ensure_queue_mut(&project_name);
                                if let Some(ref settings) = settings_snapshot {
                                    apply_project_settings_to_queue(settings, queue);
                                }
                                let diff = queue.apply_github_snapshot(&snapshot);
                                if let Some(ref settings) = settings_snapshot
                                    && settings.auto_start_matching_work
                                    && !settings.archived
                                {
                                    while queue.busy_slots() < settings.max_concurrent_runs as usize
                                    {
                                        match queue.start_next_queued() {
                                            Ok((issue_no, _)) => auto_started_issues.push(issue_no),
                                            Err(_) => break,
                                        }
                                    }
                                }
                                diff
                            };

                            if s.selected_project_name() == project_name {
                                s.sync_selected_issue_for_active_project();
                            }
                            s.persist_project_settings_by_name(&project_name);
                            s.persist_work_queue_by_name(&project_name);

                            let summary = if !diff.newly_queued.is_empty() {
                                format!(
                                    "Synced {} — {} open ({} ready, {} newly queued, {} unmatched)",
                                    snapshot.repo,
                                    diff.total_open,
                                    diff.ready_count,
                                    diff.newly_queued.len(),
                                    diff.unmatched_count
                                )
                            } else {
                                format!(
                                    "Synced {} — {} open ({} ready, {} unmatched)",
                                    snapshot.repo,
                                    diff.total_open,
                                    diff.ready_count,
                                    diff.unmatched_count
                                )
                            };
                            s.status_message = Some(summary.clone());
                            s.project_settings_error = None;
                            s.project_settings_feedback = Some(summary);
                            log::info!(
                                target: "bruk::work_queue",
                                "work_queue.github_sync.done project={project_name:?} repo={} open={} ready={} newly_queued={} unmatched={} auto_started={}",
                                snapshot.repo,
                                diff.total_open,
                                diff.ready_count,
                                diff.newly_queued.len(),
                                diff.unmatched_count,
                                auto_started_issues.len()
                            );
                        }
                        Err(message) => {
                            log::warn!(
                                target: "bruk::work_queue",
                                "work_queue.github_sync.failed project={project_name:?} err={message}"
                            );
                            if let Some(settings) = s.project_settings.get_mut(&project_name) {
                                settings.apply_github_sync_error(&message);
                            }
                            s.persist_project_settings_by_name(&project_name);
                            s.status_message = Some(format!("GitHub sync failed: {message}"));
                            s.project_settings_feedback = None;
                            s.project_settings_error = Some(message);
                        }
                    }
                });

                for issue_no in auto_started_issues {
                    spawn_github_working_label_update(issue_no, true);
                }
            });
        });
    }
}

pub fn launch_ad_hoc_work_run(workflow_name: &str, title: &str) -> bool {
    let wf = workflow_name.to_owned();
    let prompt = title.to_owned();
    let mut launched = false;
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        match queue.launch_ad_hoc(&wf, &prompt) {
            Ok((issue_no, run_no)) => {
                s.work_queue_selected_issue = Some(issue_no);
                s.work_queue_ad_hoc_open = false;
                s.status_message = Some(format!(
                    "Started ad-hoc run #{run_no} ({wf}): {}",
                    prompt.trim()
                ));
                s.persist_active_work_queue();
                launched = true;
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
    launched
}

pub fn has_queued_work_issues() -> bool {
    state().with(|s| {
        let name = s.selected_project_name();
        s.queue_for_name(&name).waiting_count() > 0
    })
}

pub fn dashboard_snapshot() -> DashboardSnapshot {
    state().with(|s| {
        let query = DashboardQuery {
            granularity: s.dashboard_granularity,
            period_offset: s.dashboard_period_offset,
            project_filter: s.dashboard_project_filter.clone(),
        };
        let tracked: &[Project] = if s.using_sample_projects {
            &[]
        } else {
            s.factory.projects()
        };
        s.dashboard_dataset
            .snapshot(&query, tracked, s.factory.runs())
    })
}

pub fn dashboard_granularity() -> DashboardGranularity {
    state().with(|s| s.dashboard_granularity)
}

pub fn set_dashboard_granularity(granularity: DashboardGranularity) {
    state().update(|s| {
        if s.dashboard_granularity != granularity {
            s.dashboard_granularity = granularity;
            s.dashboard_period_offset = 0;
        }
    });
}

pub fn step_dashboard_period(delta: i32) {
    state().update(|s| {
        if !s.demo_mode {
            s.dashboard_period_offset = 0;
            return;
        }
        let (min_offset, max_offset) = match s.dashboard_granularity {
            DashboardGranularity::Week => (-2, 1),
            DashboardGranularity::Month => (-1, 0),
        };
        let next = (s.dashboard_period_offset + delta).clamp(min_offset, max_offset);
        s.dashboard_period_offset = next;
    });
}

pub fn dashboard_project_filter() -> Option<String> {
    state().with(|s| s.dashboard_project_filter.clone())
}

pub fn set_dashboard_project_filter(filter: Option<String>) {
    state().update(|s| {
        s.dashboard_project_filter = filter;
    });
}

pub fn dashboard_filter_options() -> Vec<String> {
    state().with(|s| {
        let tracked: &[Project] = if s.using_sample_projects {
            &[]
        } else {
            s.factory.projects()
        };
        s.dashboard_dataset.available_project_names(tracked)
    })
}

pub fn tokens_show_table() -> bool {
    state().with(|s| s.tokens_show_table)
}

pub fn toggle_tokens_show_table() {
    state().update(|s| {
        s.tokens_show_table = !s.tokens_show_table;
    });
}

// --- Inbox & Machines -------------------------------------------------------------------

pub fn inbox_state() -> InboxState {
    state().with(|s| s.inbox.clone())
}

pub fn pending_inbox_count() -> usize {
    state().with(|s| s.inbox.pending_count())
}

pub fn inbox_filter() -> InboxFilter {
    state().with(|s| s.inbox_filter)
}

pub fn set_inbox_filter(filter: InboxFilter) {
    state().update(|s| {
        s.inbox_filter = filter;
        s.inbox_waiting_rules_open = false;
        s.sync_inbox_selection_and_drafts();
    });
}

pub fn inbox_selected_id() -> Option<String> {
    state().with(|s| s.inbox_selected_id.clone())
}

pub fn inbox_selected_pending() -> Option<AttentionRequest> {
    state().with(|s| {
        s.inbox_selected_id
            .as_deref()
            .and_then(|id| s.inbox.pending_by_id(id))
            .cloned()
    })
}

pub fn inbox_selected_answered() -> Option<AnsweredInboxItem> {
    state().with(|s| {
        s.inbox_selected_id
            .as_deref()
            .and_then(|id| s.inbox.answered_by_id(id))
            .cloned()
    })
}

pub fn inbox_select_request(id: &str) {
    let target = id.to_owned();
    state().update(|s| {
        s.inbox_selected_id = Some(target);
        s.inbox_waiting_rules_open = false;
        s.inbox_feedback = None;
        let (cmd, pattern, choice) =
            draft_defaults_for_selection(&s.inbox, s.inbox_selected_id.as_deref());
        s.inbox_editing_command = false;
        s.inbox_edited_command = cmd;
        s.inbox_always_allow_open = false;
        s.inbox_always_allow_pattern = pattern;
        s.inbox_note_draft.clear();
        s.inbox_question_choice = choice;
    });
}

pub fn inbox_select_relative(delta: isize) {
    state().update(|s| {
        let items = s.inbox.pending_oldest_first(s.inbox_filter);
        if items.is_empty() {
            return;
        }
        let current_idx = s
            .inbox_selected_id
            .as_deref()
            .and_then(|id| items.iter().position(|r| r.id == id))
            .unwrap_or(0);
        let next_idx = if delta < 0 {
            current_idx.saturating_sub(delta.unsigned_abs())
        } else {
            (current_idx + delta as usize).min(items.len() - 1)
        };
        let next_id = items[next_idx].id.clone();
        s.inbox_selected_id = Some(next_id);
        let (cmd, pattern, choice) =
            draft_defaults_for_selection(&s.inbox, s.inbox_selected_id.as_deref());
        s.inbox_editing_command = false;
        s.inbox_edited_command = cmd;
        s.inbox_always_allow_open = false;
        s.inbox_always_allow_pattern = pattern;
        s.inbox_note_draft.clear();
        s.inbox_question_choice = choice;
    });
}

pub fn inbox_answered_expanded() -> bool {
    state().with(|s| s.inbox_answered_expanded)
}

pub fn toggle_inbox_answered_expanded() {
    state().update(|s| {
        s.inbox_answered_expanded = !s.inbox_answered_expanded;
    });
}

pub fn inbox_waiting_rules_open() -> bool {
    state().with(|s| s.inbox_waiting_rules_open)
}

pub fn toggle_inbox_waiting_rules() {
    state().update(|s| {
        s.inbox_waiting_rules_open = !s.inbox_waiting_rules_open;
    });
}

pub fn inbox_editing_command() -> bool {
    state().with(|s| s.inbox_editing_command)
}

pub fn toggle_inbox_editing_command() {
    state().update(|s| {
        if let Some(req) = s
            .inbox_selected_id
            .as_deref()
            .and_then(|id| s.inbox.pending_by_id(id))
            && let AttentionRequestBody::Permission(detail) = &req.body
        {
            s.inbox_editing_command = !s.inbox_editing_command;
            if s.inbox_editing_command && s.inbox_edited_command.trim().is_empty() {
                s.inbox_edited_command = detail.command.clone();
            }
        }
    });
}

pub fn inbox_edited_command() -> String {
    state().with(|s| s.inbox_edited_command.clone())
}

pub fn set_inbox_edited_command(cmd: String) {
    state().update(|s| {
        s.inbox_edited_command = cmd;
    });
}

pub fn inbox_always_allow_open() -> bool {
    state().with(|s| s.inbox_always_allow_open)
}

pub fn toggle_inbox_always_allow_open() {
    state().update(|s| {
        s.inbox_always_allow_open = !s.inbox_always_allow_open;
    });
}

pub fn inbox_always_allow_pattern() -> String {
    state().with(|s| s.inbox_always_allow_pattern.clone())
}

pub fn set_inbox_always_allow_pattern(pattern: String) {
    state().update(|s| {
        s.inbox_always_allow_pattern = pattern;
    });
}

pub fn inbox_note_draft() -> String {
    state().with(|s| s.inbox_note_draft.clone())
}

pub fn set_inbox_note_draft(note: String) {
    state().update(|s| {
        s.inbox_note_draft = note;
    });
}

pub fn inbox_question_choice() -> Option<String> {
    state().with(|s| s.inbox_question_choice.clone())
}

pub fn set_inbox_question_choice(choice: String) {
    state().update(|s| {
        s.inbox_question_choice = Some(choice);
    });
}

pub fn inbox_feedback() -> Option<String> {
    state().with(|s| s.inbox_feedback.clone())
}

pub fn inbox_quick_allow(id: &str) {
    let target = id.to_owned();
    let mut closed_id = None;
    state().update(|s| {
        if let Some(answered) = s.inbox.allow_once(&target, None, None) {
            closed_id = Some(answered.id.clone());
            s.inbox_feedback = Some(format!("Allowed: {}", answered.subtitle));
            s.status_message = Some(format!("Allowed '{}'", answered.subtitle));
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
        nsh_core::element::close_notification(id);
    }
}

pub fn inbox_quick_deny(id: &str) {
    let target = id.to_owned();
    let mut closed_id = None;
    state().update(|s| {
        if let Some(answered) = s.inbox.deny(&target, None) {
            closed_id = Some(answered.id.clone());
            s.inbox_feedback = Some(format!("Denied: {}", answered.subtitle));
            s.status_message = Some(format!("Denied '{}'", answered.subtitle));
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
        nsh_core::element::close_notification(id);
    }
}

pub fn inbox_allow_once() {
    let mut closed_id = None;
    state().update(|s| {
        let Some(id) = s.inbox_selected_id.clone() else {
            return;
        };
        let edited = if s.inbox_editing_command {
            Some(s.inbox_edited_command.clone())
        } else {
            None
        };
        let note = s.inbox_note_draft.clone();
        if let Some(answered) = s.inbox.allow_once(&id, edited.as_deref(), Some(&note)) {
            closed_id = Some(answered.id.clone());
            s.inbox_feedback = Some(format!("{} — {}", answered.outcome, answered.subtitle));
            s.status_message = Some(format!("{} '{}'", answered.outcome, answered.subtitle));
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
        nsh_core::element::close_notification(id);
    }
}

pub fn inbox_allow_for_run() {
    let mut closed_id = None;
    state().update(|s| {
        let Some(id) = s.inbox_selected_id.clone() else {
            return;
        };
        let edited = if s.inbox_editing_command {
            Some(s.inbox_edited_command.clone())
        } else {
            None
        };
        let note = s.inbox_note_draft.clone();
        if let Some(answered) = s.inbox.allow_for_run(&id, edited.as_deref(), Some(&note)) {
            closed_id = Some(answered.id.clone());
            s.inbox_feedback = Some(format!("{} — {}", answered.outcome, answered.subtitle));
            s.status_message = Some(format!("{} '{}'", answered.outcome, answered.subtitle));
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
        nsh_core::element::close_notification(id);
    }
}

pub fn inbox_always_allow(scope: AlwaysAllowScope) {
    let mut closed_id = None;
    state().update(|s| {
        let Some(id) = s.inbox_selected_id.clone() else {
            return;
        };
        let pattern = s.inbox_always_allow_pattern.clone();
        if let Some(answered) = s.inbox.always_allow(&id, &pattern, scope) {
            closed_id = Some(answered.id.clone());
            s.inbox_feedback = Some(answered.outcome.clone());
            s.status_message = Some(answered.outcome.clone());
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
        nsh_core::element::close_notification(id);
    }
}

pub fn inbox_deny_selected() {
    let mut closed_id = None;
    state().update(|s| {
        let Some(id) = s.inbox_selected_id.clone() else {
            return;
        };
        let note = s.inbox_note_draft.clone();
        if let Some(answered) = s.inbox.deny(&id, Some(&note)) {
            closed_id = Some(answered.id.clone());
            s.inbox_feedback = Some(format!("Denied: {}", answered.subtitle));
            s.status_message = Some(format!("Denied '{}'", answered.subtitle));
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
        nsh_core::element::close_notification(id);
    }
}

pub fn inbox_answer_question() {
    let mut closed_id = None;
    state().update(|s| {
        let Some(id) = s.inbox_selected_id.clone() else {
            return;
        };
        let answer = if !s.inbox_note_draft.trim().is_empty() {
            s.inbox_note_draft.trim().to_owned()
        } else {
            s.inbox_question_choice.clone().unwrap_or_default()
        };
        if let Some(answered) = s.inbox.answer_question(&id, &answer) {
            closed_id = Some(answered.id.clone());
            s.inbox_feedback = Some(answered.outcome.clone());
            s.status_message = Some(answered.outcome.clone());
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
        nsh_core::element::close_notification(id);
    }
}

pub fn inbox_let_agent_decide() {
    let mut closed_id = None;
    state().update(|s| {
        let Some(id) = s.inbox_selected_id.clone() else {
            return;
        };
        if let Some(answered) = s.inbox.let_agent_decide(&id) {
            closed_id = Some(answered.id.clone());
            s.inbox_feedback = Some(format!("Let {} decide", answered.agent.display_name()));
            s.status_message = Some(format!("Let {} decide", answered.agent.display_name()));
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
        nsh_core::element::close_notification(id);
    }
}

pub fn inbox_approve_review() {
    let mut closed_id = None;
    let mut note_for_runner = None;
    state().update(|s| {
        let Some(id) = s.inbox_selected_id.clone() else {
            return;
        };
        let note = s.inbox_note_draft.clone();
        if let Some(answered) = s.inbox.approve_review(&id, Some(&note)) {
            closed_id = Some(answered.id.clone());
            note_for_runner = Some(note);
            s.inbox_feedback = Some(format!("Approved review — {}", answered.context_label));
            s.status_message = Some(format!("Approved review for {}", answered.context_label));
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
        resolve_runner_gate_by_id(
            &id,
            GateDecision::Approve {
                note: note_for_runner.filter(|n| !n.trim().is_empty()),
            },
        );
        nsh_core::element::close_notification(id);
    }
}

pub fn inbox_request_changes() {
    let mut closed_id = None;
    let mut note_for_runner = None;
    state().update(|s| {
        let Some(id) = s.inbox_selected_id.clone() else {
            return;
        };
        let note = s.inbox_note_draft.clone();
        if let Some(answered) = s.inbox.request_changes(&id, Some(&note)) {
            closed_id = Some(answered.id.clone());
            note_for_runner = Some(note);
            s.inbox_feedback = Some(format!("Requested changes — {}", answered.context_label));
            s.status_message = Some(format!("Requested changes for {}", answered.context_label));
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
        resolve_runner_gate_by_id(
            &id,
            GateDecision::RequestChanges {
                note: note_for_runner.filter(|n| !n.trim().is_empty()),
            },
        );
        nsh_core::element::close_notification(id);
    }
}

pub fn inbox_grant_more_attempts() {
    let mut closed_id = None;
    state().update(|s| {
        let Some(id) = s.inbox_selected_id.clone() else {
            return;
        };
        let note = s.inbox_note_draft.clone();
        if let Some(answered) = s.inbox.grant_more_attempts(&id, 3, Some(&note)) {
            closed_id = Some(answered.id.clone());
            s.inbox_feedback = Some(format!(
                "Granted +3 attempts for {}",
                answered.context_label
            ));
            s.status_message = Some(format!(
                "Granted +3 attempts for {}",
                answered.context_label
            ));
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
        nsh_core::element::close_notification(id);
    }
}

pub fn inbox_stop_exhausted_run() {
    let mut closed_id = None;
    state().update(|s| {
        let Some(id) = s.inbox_selected_id.clone() else {
            return;
        };
        let note = s.inbox_note_draft.clone();
        if let Some(answered) = s.inbox.stop_exhausted_run(&id, Some(&note)) {
            closed_id = Some(answered.id.clone());
            s.inbox_feedback = Some(format!("Stopped {}", answered.context_label));
            s.status_message = Some(format!("Stopped {}", answered.context_label));
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
        nsh_core::element::close_notification(id);
    }
}

pub fn inbox_primary_answer() {
    let Some(req) = inbox_selected_pending() else {
        return;
    };
    match req.kind {
        AttentionRequestKind::Permission => inbox_allow_once(),
        AttentionRequestKind::Question => inbox_answer_question(),
        AttentionRequestKind::Review => inbox_approve_review(),
        AttentionRequestKind::LoopExhausted => inbox_grant_more_attempts(),
        AttentionRequestKind::Setup => inbox_fix_setup_item(),
    }
}

pub fn inbox_secondary_deny_shortcut() {
    let Some(req) = inbox_selected_pending() else {
        return;
    };
    match req.kind {
        AttentionRequestKind::Permission => inbox_deny_selected(),
        AttentionRequestKind::Question => inbox_let_agent_decide(),
        AttentionRequestKind::Review => inbox_request_changes(),
        AttentionRequestKind::LoopExhausted => inbox_stop_exhausted_run(),
        AttentionRequestKind::Setup => inbox_dismiss_setup_item(),
    }
}

pub fn inbox_open_selected_run() {
    let target = state().with(|s| {
        let id = s.inbox_selected_id.as_deref()?;
        if let Some(req) = s.inbox.pending_by_id(id) {
            Some((req.project_name.clone(), req.workflow_name.clone()))
        } else {
            s.inbox
                .answered_by_id(id)
                .map(|ans| (ans.project_name.clone(), ans.workflow_name.clone()))
        }
    });
    let Some((project_name, workflow_name)) = target else {
        return;
    };
    state().update(|s| {
        if let Some(project) = s.factory.projects().iter().find(|p| p.name == project_name) {
            s.selected_project = Some(project.id);
            s.expanded_projects.insert(project.id);
        }
        let step_index = s
            .ensure_workflow_run_mut(&project_name, &workflow_name)
            .active_step_index;
        s.active_workflow_run_key = (project_name, workflow_name);
        s.workflow_run_selected_step = step_index;
        s.workflow_run_tab = RunDetailTab::Output;
        s.workflow_run_command_palette_open = false;
        s.active_view = ActiveView::WorkflowRun;
        s.status_message = None;
    });
    main_window().focus();
}

pub fn toggle_pause_all_agents() {
    state().update(|s| {
        let paused = s.inbox.toggle_pause_all_agents();
        log::info!(target: "bruk::inbox", "inbox.pause_all_agents paused={paused}");
        let msg = if paused {
            "Paused all active agents".to_owned()
        } else {
            "Resumed all active agents".to_owned()
        };
        s.inbox_feedback = Some(msg.clone());
        s.status_message = Some(msg);
    });
}

pub fn toggle_snooze_notifications() {
    state().update(|s| {
        let snoozed = s.inbox.toggle_snooze_notifications();
        log::info!(target: "bruk::inbox", "inbox.snooze_notifications mins={snoozed:?}");
        let msg = if let Some(mins) = snoozed {
            format!("Snoozed notifications for {mins} minutes")
        } else {
            "Notifications resumed".to_owned()
        };
        s.inbox_feedback = Some(msg.clone());
        s.status_message = Some(msg);
    });
}

pub fn machines_fleet() -> MachinesFleet {
    state().with(|s| s.machines_fleet.clone())
}

pub fn machines() -> Vec<MachineRecord> {
    state().with(|s| s.machines_fleet.machines.clone())
}

pub fn selected_machine_id() -> Option<u64> {
    state().with(|s| {
        s.selected_machine_id
            .filter(|id| s.machines_fleet.machine(*id).is_some())
            .or_else(|| s.machines_fleet.default_selected_id())
    })
}

pub fn selected_machine() -> Option<MachineRecord> {
    state().with(|s| {
        let id = s
            .selected_machine_id
            .filter(|id| s.machines_fleet.machine(*id).is_some())
            .or_else(|| s.machines_fleet.default_selected_id())?;
        s.machines_fleet.machine(id).cloned()
    })
}

pub fn select_machine(id: u64) {
    state().update(|s| {
        if s.machines_fleet.machine(id).is_some() {
            s.selected_machine_id = Some(id);
            s.machines_changes_expanded = false;
            s.machines_settings_open = false;
            s.status_message = None;
        }
    });
}

pub fn disconnect_machine(id: u64) {
    state().update(|s| match s.machines_fleet.disconnect_machine(id) {
        Ok(machine) => {
            s.machines_changes_expanded = false;
            s.status_message = Some(format!(
                "Disconnected '{}'. Running sessions continue on the host.",
                machine.name
            ));
        }
        Err(err) => {
            s.status_message = Some(err.to_string());
        }
    });
}

pub fn reconnect_machine(id: u64) {
    state().update(|s| match s.machines_fleet.reconnect_machine(id) {
        Ok(machine) => {
            s.status_message = Some(format!("Reconnected machine '{}'", machine.name));
        }
        Err(err) => {
            s.status_message = Some(err.to_string());
        }
    });
}

pub fn open_machine_in_terminal(id: u64) {
    state().update(|s| {
        if let Some(machine) = s.machines_fleet.machine(id) {
            if machine.is_local {
                s.status_message = Some("Opened local terminal session on This Mac".to_owned());
            } else {
                s.status_message = Some(format!(
                    "Opened SSH terminal for {} ({})",
                    machine.ssh_target, machine.address_primary
                ));
            }
        }
    });
}

pub fn update_machine_core(id: u64) {
    state().update(|s| match s.machines_fleet.update_core(id) {
        Ok(machine) => {
            s.status_message = Some(format!(
                "Updated Bruk core on '{}' to {} (schema {})",
                machine.name, machine.core_version, machine.core_schema
            ));
        }
        Err(err) => {
            s.status_message = Some(err.to_string());
        }
    });
}

pub fn update_all_suggested_cores() {
    state().update(|s| {
        let updated = s.machines_fleet.update_all_suggested_cores();
        s.machines_update_cores_open = false;
        if updated == 0 {
            s.status_message = Some("All compatible cores are already up to date.".to_owned());
        } else if updated == 1 {
            s.status_message = Some(format!(
                "Updated 1 machine core to Bruk {} (schema {})",
                s.machines_fleet.bruk_version, s.machines_fleet.bruk_schema
            ));
        } else {
            s.status_message = Some(format!(
                "Updated {updated} machine cores to Bruk {} (schema {})",
                s.machines_fleet.bruk_version, s.machines_fleet.bruk_schema
            ));
        }
    });
}

pub fn check_bruk_update() {
    state().update(|s| {
        s.status_message = Some(
            "Bruk 1.0.0 (data schema 8) is available in Software Update — install to resume syncing gpu-01."
                .to_owned(),
        );
    });
}

pub fn add_machine(draft: NewMachineDraft) -> Result<(), String> {
    let mut outcome = Ok(());
    state().update(|s| match s.machines_fleet.add_machine(draft) {
        Ok(added) => {
            let id = added.id;
            let name = added.name.clone();
            s.selected_machine_id = Some(id);
            s.machines_add_drawer_open = false;
            s.machines_changes_expanded = false;
            s.machines_settings_open = false;
            s.status_message = Some(format!("Added machine '{name}' over SSH"));
        }
        Err(err) => {
            let msg = err.to_string();
            s.status_message = Some(msg.clone());
            outcome = Err(msg);
        }
    });
    outcome
}

pub fn toggle_machine_project_access(id: u64, project_name: &str) {
    let project = project_name.to_owned();
    state().update(
        |s| match s.machines_fleet.toggle_project_access(id, &project) {
            Ok(enabled) => {
                let machine_name = s
                    .machines_fleet
                    .machine(id)
                    .map(|m| m.name.clone())
                    .unwrap_or_default();
                s.status_message = Some(if enabled {
                    format!("Allowed '{project}' to run on '{machine_name}'")
                } else {
                    format!("Removed '{project}' from '{machine_name}'")
                });
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        },
    );
}

pub fn remove_machine(id: u64) {
    state().update(|s| match s.machines_fleet.remove_machine(id) {
        Ok(removed) => {
            if s.selected_machine_id == Some(id) {
                s.selected_machine_id = s.machines_fleet.default_selected_id();
            }
            s.machines_settings_open = false;
            s.machines_changes_expanded = false;
            s.status_message = Some(format!("Removed machine '{}'", removed.name));
        }
        Err(err) => {
            s.status_message = Some(err.to_string());
        }
    });
}

pub fn machines_changes_expanded() -> bool {
    state().with(|s| s.machines_changes_expanded)
}

pub fn toggle_machines_changes_expanded() {
    state().update(|s| {
        s.machines_changes_expanded = !s.machines_changes_expanded;
    });
}

pub fn machines_update_cores_open() -> bool {
    state().with(|s| s.machines_update_cores_open)
}

pub fn set_machines_update_cores_open(open: bool) {
    state().update(|s| {
        s.machines_update_cores_open = open;
        if open {
            s.machines_add_drawer_open = false;
        }
    });
}

pub fn machines_add_drawer_open() -> bool {
    state().with(|s| s.machines_add_drawer_open)
}

pub fn set_machines_add_drawer_open(open: bool) {
    state().update(|s| {
        s.machines_add_drawer_open = open;
        if open {
            s.machines_update_cores_open = false;
        }
    });
}

pub fn machines_settings_open() -> bool {
    state().with(|s| s.machines_settings_open)
}

pub fn toggle_machines_settings_open() {
    state().update(|s| {
        s.machines_settings_open = !s.machines_settings_open;
    });
}

pub fn open_machine_session(project_name: &str, workflow_name: &str, run_number: u32) {
    let project = project_name.to_owned();
    let workflow = workflow_name.to_owned();
    state().update(|s| {
        if let Some(pid) = s
            .factory
            .projects()
            .iter()
            .find(|p| p.name == project)
            .map(|p| p.id)
        {
            s.selected_project = Some(pid);
            s.expanded_projects.insert(pid);
        }
        let run = s.ensure_workflow_run_mut(&project, &workflow);
        run.run_number = run_number;
        let step_index = run.active_step_index;
        s.active_workflow_run_key = (project, workflow);
        s.workflow_run_selected_step = step_index;
        s.workflow_run_tab = RunDetailTab::Output;
        s.workflow_run_command_palette_open = false;
        s.active_view = ActiveView::WorkflowRun;
        s.status_message = None;
    });
}

// --- Workflows & Runs -------------------------------------------------------------------

pub fn workflow_ids() -> Vec<WorkflowId> {
    state().with(|s| s.factory.workflows().iter().map(|w| w.id).collect())
}

pub fn workflow_by_id(id: WorkflowId) -> Option<Workflow> {
    state().with(|s| s.factory.workflow(id).cloned())
}

pub fn selected_workflow_id() -> Option<WorkflowId> {
    state().with(|s| s.selected_workflow)
}

pub fn selected_workflow() -> Option<Workflow> {
    state().with(|s| {
        s.selected_workflow
            .and_then(|id| s.factory.workflow(id).cloned())
    })
}

pub fn select_workflow(id: WorkflowId) {
    state().update(|s| {
        s.selected_workflow = Some(id);
        s.status_message = None;
    });
}

pub fn run_ids_for_selected_project() -> Vec<RunId> {
    state().with(|s| {
        let Some(project_id) = s.selected_project else {
            return Vec::new();
        };
        s.factory
            .runs_for_project(project_id)
            .into_iter()
            .map(|run| run.id)
            .collect()
    })
}

pub fn run_by_id(id: RunId) -> Option<WorkflowRun> {
    state().with(|s| s.factory.run(id).cloned())
}

pub fn selected_run_id() -> Option<RunId> {
    state().with(|s| s.selected_run)
}

pub fn selected_run() -> Option<WorkflowRun> {
    state().with(|s| s.selected_run.and_then(|id| s.factory.run(id).cloned()))
}

pub fn select_run(id: RunId) {
    state().update(|s| {
        s.selected_run = Some(id);
    });
}

pub fn launch_run(task_prompt: &str) -> bool {
    let mut launched = false;
    state().update(|s| {
        let (Some(project_id), Some(workflow_id)) = (s.selected_project, s.selected_workflow)
        else {
            s.status_message = Some("Select a project and workflow first.".to_owned());
            return;
        };
        match s.factory.start_run(project_id, workflow_id, task_prompt) {
            Ok(run) => {
                let run_id = run.id;
                s.selected_run = Some(run_id);
                s.status_message = Some(format!("Started run #{run_id}"));
                launched = true;
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
    launched
}

pub fn advance_selected_run() {
    state().update(|s| {
        let Some(run_id) = s.selected_run else {
            return;
        };
        match s.factory.advance_run(run_id, &DryRunExecutor) {
            Ok(run) => {
                s.status_message = Some(format!("Run #{} is now {}", run.id, run.status.label()));
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub fn approve_selected_run_step() {
    let is_waiting = state().with_untracked(|s| {
        s.selected_run
            .and_then(|id| s.factory.run(id))
            .is_some_and(|run| run.status == RunStatus::WaitingForApproval)
    });
    if is_waiting {
        advance_selected_run();
    }
}

pub fn cancel_selected_run() {
    state().update(|s| {
        let Some(run_id) = s.selected_run else {
            return;
        };
        match s.factory.cancel_run(run_id) {
            Ok(run) => {
                s.status_message = Some(format!("Cancelled run #{}", run.id));
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub fn status_message() -> Option<String> {
    state().with(|s| s.status_message.clone())
}

pub fn set_status_message(message: impl Into<String>) {
    let message = message.into();
    state().update(|s| {
        s.status_message = Some(message);
    });
}

// --- Application Auto-Updater -----------------------------------------------------------

struct ThreadChannelState<T> {
    items: std::collections::VecDeque<T>,
    closed: bool,
    waker: Option<Waker>,
}

struct ThreadChannelTx<T>(Arc<Mutex<ThreadChannelState<T>>>);

impl<T> Clone for ThreadChannelTx<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T> ThreadChannelTx<T> {
    fn send(&self, item: T) {
        let waker = {
            let mut guard = self.0.lock().expect("channel lock poisoned");
            if guard.closed {
                return;
            }
            guard.items.push_back(item);
            guard.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn close(&self) {
        let waker = {
            let mut guard = self.0.lock().expect("channel lock poisoned");
            guard.closed = true;
            guard.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

struct ThreadChannelRx<T>(Arc<Mutex<ThreadChannelState<T>>>);

impl<T> ThreadChannelRx<T> {
    fn next(&self) -> ThreadChannelNext<'_, T> {
        ThreadChannelNext(self)
    }
}

struct ThreadChannelNext<'a, T>(&'a ThreadChannelRx<T>);

impl<T> Future for ThreadChannelNext<'_, T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut guard = self.0.0.lock().expect("channel lock poisoned");
        if let Some(item) = guard.items.pop_front() {
            Poll::Ready(Some(item))
        } else if guard.closed {
            Poll::Ready(None)
        } else {
            guard.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

fn thread_channel<T>() -> (ThreadChannelTx<T>, ThreadChannelRx<T>) {
    let shared = Arc::new(Mutex::new(ThreadChannelState {
        items: std::collections::VecDeque::new(),
        closed: false,
        waker: None,
    }));
    (ThreadChannelTx(shared.clone()), ThreadChannelRx(shared))
}

pub fn current_app_version() -> String {
    factory_updater::current_version()
}

pub fn updater_state() -> factory_updater::UpdaterState {
    state().with(|s| s.updater_state.clone())
}

pub fn updater_banner_visible() -> bool {
    state().with(|s| {
        !s.updater_banner_dismissed
            && matches!(
                s.updater_state,
                factory_updater::UpdaterState::Available(_)
                    | factory_updater::UpdaterState::Downloading { .. }
                    | factory_updater::UpdaterState::ReadyToRestart { .. }
                    | factory_updater::UpdaterState::Installing { .. }
                    | factory_updater::UpdaterState::StagedOnQuit { .. }
            )
    })
}

pub fn dismiss_updater_banner() {
    state().update(|s| {
        s.updater_banner_dismissed = true;
    });
}

/// Checks GitHub Releases in the background on startup when auto-checks are enabled and Bruk
/// is running from a packaged `.app` bundle. Never runs in dev builds, Demo Mode, or tests.
fn maybe_check_for_updates_on_launch() {
    let should_check = state().with_untracked(|s| {
        !s.demo_mode && s.preferences.general.auto_check_updates && !s.updater_state.is_busy()
    });
    if !should_check || !factory_updater::has_packaged_updater_config() {
        return;
    }
    run_update_check(true);
}

/// Triggers an explicit user-initiated check for updates.
pub fn check_for_updates() {
    run_update_check(false);
}

fn run_update_check(silent: bool) {
    let started = {
        let mut ok = false;
        state().update(|s| {
            if s.updater_state.is_busy() {
                return;
            }
            ok = true;
            if !silent {
                s.updater_state = factory_updater::UpdaterState::Checking;
                s.updater_banner_dismissed = false;
            }
        });
        ok
    };
    if !started {
        return;
    }

    Owner::detached().run(|| {
        spawn(async move {
            let outcome = spawn_blocking_task(factory_updater::check_for_update).await;
            state().update(|s| match outcome {
                Ok(factory_updater::CheckOutcome::Available(update)) => {
                    s.updater_state = factory_updater::UpdaterState::Available(update);
                    s.updater_banner_dismissed = false;
                }
                Ok(factory_updater::CheckOutcome::UpToDate { current_version }) => {
                    if !silent {
                        s.updater_state =
                            factory_updater::UpdaterState::UpToDate { current_version };
                    }
                }
                Ok(factory_updater::CheckOutcome::Unavailable(reason)) => {
                    if !silent {
                        s.updater_state = factory_updater::UpdaterState::Unavailable(reason);
                    }
                }
                Err(err) => {
                    log::warn!("updater: check failed: {err}");
                    if !silent {
                        s.updater_state = factory_updater::UpdaterState::Error(err);
                    }
                }
            });
        });
    });
}

/// Downloads and verifies the currently available release archive on a background thread.
pub fn download_available_update() {
    let update = {
        let mut target = None;
        state().update(|s| {
            if let factory_updater::UpdaterState::Available(avail) = &s.updater_state
                && avail.can_install
            {
                let cloned = avail.clone();
                s.updater_state = factory_updater::UpdaterState::Downloading {
                    update: cloned.clone(),
                    received: 0,
                    total: cloned.size,
                };
                s.updater_banner_dismissed = false;
                target = Some(cloned);
            }
        });
        target
    };
    let Some(update) = update else {
        return;
    };

    enum DownloadEvent {
        Progress { received: u64, total: Option<u64> },
        Finished(Result<std::path::PathBuf, String>),
    }

    let (tx, rx) = thread_channel::<DownloadEvent>();
    let worker_update = update.clone();
    std::thread::spawn(move || {
        let progress_tx = tx.clone();
        let result = factory_updater::download_update(&worker_update, move |received, total| {
            progress_tx.send(DownloadEvent::Progress { received, total });
        });
        tx.send(DownloadEvent::Finished(result));
        tx.close();
    });

    Owner::detached().run(|| {
        spawn(async move {
            while let Some(event) = rx.next().await {
                match event {
                    DownloadEvent::Progress { received, total } => {
                        state().update(|s| {
                            if let factory_updater::UpdaterState::Downloading {
                                received: cur_received,
                                total: cur_total,
                                ..
                            } = &mut s.updater_state
                            {
                                *cur_received = received;
                                if total.is_some() {
                                    *cur_total = total;
                                }
                            }
                        });
                    }
                    DownloadEvent::Finished(Ok(archive_path)) => {
                        state().update(|s| {
                            s.updater_state = factory_updater::UpdaterState::ReadyToRestart {
                                version: update.version.clone(),
                                notes: update.notes.clone(),
                                archive_path,
                            };
                            s.updater_banner_dismissed = false;
                        });
                    }
                    DownloadEvent::Finished(Err(err)) => {
                        log::error!("updater: download failed: {err}");
                        state().update(|s| {
                            s.updater_state = factory_updater::UpdaterState::Error(err);
                        });
                    }
                }
            }
        });
    });
}

/// Installs a downloaded update archive over the running `Bruk.app` and restarts the app.
pub fn install_and_restart_update() {
    let pending = {
        let mut target = None;
        state().update(|s| {
            if let factory_updater::UpdaterState::ReadyToRestart {
                version,
                archive_path,
                ..
            } = &s.updater_state
            {
                let version = version.clone();
                let archive_path = archive_path.clone();
                s.updater_state = factory_updater::UpdaterState::Installing {
                    version: version.clone(),
                };
                target = Some((version, archive_path));
            }
        });
        target
    };
    let Some((version, archive_path)) = pending else {
        return;
    };

    Owner::detached().run(|| {
        spawn(async move {
            let result = spawn_blocking_task(move || {
                factory_updater::install_downloaded_update(
                    &archive_path,
                    nsh_core::restart::remember_program,
                )
            })
            .await;
            match result {
                Ok(()) => {
                    log::info!("updater: installed Bruk {version}; restarting");
                    nsh_core::element::desktop::restart();
                }
                Err(err) => {
                    log::error!("updater: install failed: {err}");
                    state().update(|s| {
                        s.updater_state = factory_updater::UpdaterState::Error(err);
                    });
                }
            }
        });
    });
}

/// Unpacks and verifies the downloaded update now so it swaps in automatically when Bruk quits.
pub fn stage_update_for_quit() {
    let pending = {
        let mut target = None;
        state().update(|s| {
            if let factory_updater::UpdaterState::ReadyToRestart {
                version,
                archive_path,
                ..
            } = &s.updater_state
            {
                let version = version.clone();
                let archive_path = archive_path.clone();
                s.updater_state = factory_updater::UpdaterState::Installing {
                    version: version.clone(),
                };
                target = Some((version, archive_path));
            }
        });
        target
    };
    let Some((version, archive_path)) = pending else {
        return;
    };

    Owner::detached().run(|| {
        spawn(async move {
            let v = version.clone();
            let result = spawn_blocking_task(move || {
                factory_updater::stage_downloaded_update_for_quit(v, archive_path)
            })
            .await;
            state().update(|s| match result {
                Ok(()) => {
                    s.updater_state = factory_updater::UpdaterState::StagedOnQuit { version };
                }
                Err(err) => {
                    log::error!("updater: stage for quit failed: {err}");
                    s.updater_state = factory_updater::UpdaterState::Error(err);
                }
            });
        });
    });
}

/// Restarts the app immediately when an update has already been staged for quit; the `on_app_quit`
/// hook swaps the bundle in place before the restarted process launches.
pub fn restart_for_staged_update() {
    let is_staged = state().with_untracked(|s| {
        matches!(
            s.updater_state,
            factory_updater::UpdaterState::StagedOnQuit { .. }
        )
    });
    if is_staged {
        nsh_core::element::desktop::restart();
    }
}

/// Opens the release download URL in the user's default browser when in-place replacement is
/// unavailable (e.g. App Translocation or read-only install directory).
pub fn open_update_download_url() {
    let url = state().with_untracked(|s| match &s.updater_state {
        factory_updater::UpdaterState::Available(update) => Some(update.url.clone()),
        _ => None,
    });
    if let Some(url) = url {
        let answer = nsh_core::element::open_url(url);
        Owner::detached().run(|| {
            spawn(async move {
                let _ = answer.await;
            });
        });
    }
}

// --- Preferences Window & Settings ------------------------------------------------------

pub fn preferences_open() -> bool {
    state().with(|s| s.preferences_open)
}

pub fn preferences_focused() -> bool {
    state().with(|s| s.preferences_focused)
}

pub fn set_preferences_focused(focused: bool) {
    state().update(|s| {
        s.preferences_focused = focused;
    });
}

pub fn open_preferences() {
    state().update(|s| {
        s.preferences_open = true;
        s.preferences_focused = true;
        s.preferences_error = None;
    });
}

/// Falls back to the default tab when `tab` is hidden by a build flag.
fn available_preferences_tab(tab: PreferencesTab) -> PreferencesTab {
    if tab.is_available() {
        tab
    } else {
        PreferencesTab::default()
    }
}

pub fn open_preferences_tab(tab: PreferencesTab) {
    let tab = available_preferences_tab(tab);
    state().update(|s| {
        s.preferences_tab = tab;
        s.preferences_open = true;
        s.preferences_focused = true;
        s.preferences_error = None;
        s.preferences_feedback = None;
    });
}

pub fn close_preferences() {
    state().update(|s| {
        s.preferences_open = false;
        s.preferences_focused = false;
        s.preferences_error = None;
        s.preferences_feedback = None;
    });
}

pub fn preferences_tab() -> PreferencesTab {
    state().with(|s| s.preferences_tab)
}

pub fn set_preferences_tab(tab: PreferencesTab) {
    let tab = available_preferences_tab(tab);
    state().update(|s| {
        s.preferences_tab = tab;
        s.preferences_error = None;
        s.preferences_feedback = None;
    });
}

pub fn preferences() -> AppPreferences {
    state().with(|s| s.preferences.clone())
}

pub fn selected_provider_id() -> String {
    state().with(|s| s.selected_provider_id.clone())
}

pub fn select_provider(id: String) {
    state().update(|s| {
        s.selected_provider_id = id;
        s.preferences_error = None;
        s.preferences_feedback = None;
    });
}

pub fn preferences_error() -> Option<String> {
    state().with(|s| s.preferences_error.clone())
}

pub fn preferences_feedback() -> Option<String> {
    state().with(|s| s.preferences_feedback.clone())
}

pub fn clear_preferences_messages() {
    state().update(|s| {
        s.preferences_error = None;
        s.preferences_feedback = None;
    });
}

pub fn test_selected_provider() {
    let mut should_refresh_claude = false;
    let mut should_refresh_codex = false;
    let mut should_refresh_antigravity = false;
    state().update(|s| {
        let id = s.selected_provider_id.clone();
        if !s.demo_mode
            && s.preferences
                .antigravity
                .as_ref()
                .is_some_and(|account| account.id == id)
        {
            if let Some(probe) = s.refresh_antigravity_status(true) {
                antigravity_probe_feedback(s, &probe, "Verified");
                should_refresh_antigravity =
                    matches!(probe, AntigravityCliProbe::Authenticated { .. });
            }
            s.persist_preferences();
            return;
        }
        if !s.demo_mode && id == s.preferences.claude.id {
            match s.refresh_claude_status(true) {
                ClaudeCliProbe::Authenticated {
                    binary_path, email, ..
                } => {
                    s.preferences_error = None;
                    s.preferences_feedback = Some(format!(
                        "Verified Claude CLI at {} ({email}).",
                        binary_path.display()
                    ));
                    should_refresh_claude = true;
                }
                ClaudeCliProbe::NotAuthenticated { binary_path, .. } => {
                    s.preferences_error = None;
                    s.preferences_feedback = Some(format!(
                        "Found Claude CLI at {}, but no active system login was detected.",
                        binary_path.display()
                    ));
                }
                ClaudeCliProbe::BinaryNotFound { error } => {
                    s.preferences_feedback = None;
                    s.preferences_error = Some(error.to_string());
                }
                ClaudeCliProbe::ProbeFailed { message, .. } => {
                    s.preferences_feedback = None;
                    s.preferences_error = Some(message);
                }
            }
            s.persist_preferences();
            return;
        }
        if !s.demo_mode
            && s.preferences
                .codex
                .as_ref()
                .is_some_and(|codex| codex.id == id)
        {
            if let Some(probe) = s.refresh_codex_status(true) {
                match probe {
                    CodexCliProbe::Authenticated {
                        binary_path, email, ..
                    } => {
                        s.preferences_error = None;
                        s.preferences_feedback = Some(format!(
                            "Verified Codex CLI at {} ({email}).",
                            binary_path.display()
                        ));
                        should_refresh_codex = true;
                    }
                    CodexCliProbe::NotAuthenticated { binary_path, .. } => {
                        s.preferences_error = None;
                        s.preferences_feedback = Some(format!(
                            "Found Codex CLI at {}, but no active system login was detected.",
                            binary_path.display()
                        ));
                    }
                    CodexCliProbe::BinaryNotFound { error } => {
                        s.preferences_feedback = None;
                        s.preferences_error = Some(error.to_string());
                    }
                    CodexCliProbe::ProbeFailed { message, .. } => {
                        s.preferences_feedback = None;
                        s.preferences_error = Some(message);
                    }
                }
            }
            s.persist_preferences();
            return;
        }
        match s.preferences.test_provider(&id) {
            Ok(_) => {
                s.preferences_error = None;
                s.preferences_feedback = Some("Connection verified.".to_owned());
                s.persist_preferences();
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    if should_refresh_claude {
        refresh_claude_details_async();
    }
    if should_refresh_codex {
        refresh_codex_details_async();
    }
    if should_refresh_antigravity {
        refresh_antigravity_details_async();
    }
}

/// Reports an Antigravity probe in Preferences; `verb` starts the success message.
fn antigravity_probe_feedback(s: &mut AppState, probe: &AntigravityCliProbe, verb: &str) {
    match probe {
        AntigravityCliProbe::Authenticated { binary_path, .. } => {
            s.preferences_error = None;
            s.preferences_feedback = Some(format!(
                "{verb} Antigravity CLI at {}.",
                binary_path.display()
            ));
        }
        AntigravityCliProbe::NotAuthenticated { binary_path, .. } => {
            s.preferences_error = None;
            s.preferences_feedback = Some(format!(
                "Found Antigravity CLI at {}, but it isn't signed in.",
                binary_path.display()
            ));
        }
        AntigravityCliProbe::BinaryNotFound { error } => {
            s.preferences_feedback = None;
            s.preferences_error = Some(error.to_string());
        }
        AntigravityCliProbe::ProbeFailed { message, .. } => {
            s.preferences_feedback = None;
            s.preferences_error = Some(message.clone());
        }
    }
}

pub fn add_openrouter_key(
    name: &str,
    raw_api_key: &str,
    monthly_cap: &str,
    default_model: &str,
) -> bool {
    let mut added = false;
    state().update(|s| {
        let mut created_secret_id = None;
        if !s.demo_mode
            && let Some(db_arc) = s.db.as_ref()
            && let Ok(db) = db_arc.lock()
        {
            let trimmed_name = name.trim();
            if !trimmed_name.is_empty() && !raw_api_key.trim().is_empty() {
                let record = SecretRecord::new_keychain(
                    trimmed_name,
                    SecretKind::Internal {
                        purpose: InternalPurpose::OpenRouterKey,
                    },
                    current_unix_seconds(),
                );
                let secret = SecretString::from(raw_api_key.to_owned());
                match db.create_keychain_secret(s.secret_store.as_ref(), record, &secret, None) {
                    Ok(saved) => created_secret_id = Some(saved.id),
                    Err(err) => {
                        s.preferences_error =
                            Some(format!("Could not store API key in Keychain: {err}"));
                        return;
                    }
                }
            }
        }
        match s
            .preferences
            .add_openrouter_key(name, raw_api_key, monthly_cap, default_model)
        {
            Ok(new_id) => {
                if let Some(secret_id) = created_secret_id
                    && let Some(key) = s
                        .preferences
                        .openrouter_keys
                        .iter_mut()
                        .find(|k| k.id == new_id)
                {
                    key.secret_id = Some(secret_id);
                }
                s.selected_provider_id = new_id;
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Added OpenRouter key '{}'.", name.trim()));
                s.persist_preferences();
                added = true;
            }
            Err(err) => {
                if let Some(secret_id) = created_secret_id
                    && let Some(db_arc) = s.db.as_ref()
                    && let Ok(db) = db_arc.lock()
                {
                    let _ = db.delete_secret(s.secret_store.as_ref(), &secret_id);
                }
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    added
}

pub fn replace_selected_openrouter_key(raw_api_key: &str) -> bool {
    let mut updated = false;
    state().update(|s| {
        let id = s.selected_provider_id.clone();
        if !s.demo_mode
            && let Some(key) = s.preferences.openrouter_key(&id).cloned()
            && let Some(db_arc) = s.db.as_ref()
            && let Ok(db) = db_arc.lock()
        {
            let secret = SecretString::from(raw_api_key.to_owned());
            if let Some(secret_id) = key.secret_id.as_deref() {
                if let Err(err) =
                    db.replace_keychain_secret(s.secret_store.as_ref(), secret_id, &secret, None)
                {
                    s.preferences_error =
                        Some(format!("Could not update API key in Keychain: {err}"));
                    return;
                }
            } else {
                let record = SecretRecord::new_keychain(
                    &key.name,
                    SecretKind::Internal {
                        purpose: InternalPurpose::OpenRouterKey,
                    },
                    current_unix_seconds(),
                );
                match db.create_keychain_secret(s.secret_store.as_ref(), record, &secret, None) {
                    Ok(saved) => {
                        if let Some(k) = s
                            .preferences
                            .openrouter_keys
                            .iter_mut()
                            .find(|k| k.id == id)
                        {
                            k.secret_id = Some(saved.id);
                        }
                    }
                    Err(err) => {
                        s.preferences_error =
                            Some(format!("Could not store API key in Keychain: {err}"));
                        return;
                    }
                }
            }
        }
        match s
            .preferences
            .replace_openrouter_key_secret(&id, raw_api_key)
        {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some("API key updated in Keychain.".to_owned());
                s.persist_preferences();
                updated = true;
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    updated
}

pub fn update_selected_openrouter_cap(monthly_cap_input: &str) -> bool {
    let mut updated = false;
    state().update(|s| {
        let id = s.selected_provider_id.clone();
        match s.preferences.update_openrouter_cap(&id, monthly_cap_input) {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some("Monthly spending cap updated.".to_owned());
                s.persist_preferences();
                updated = true;
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    updated
}

pub fn update_selected_provider_model(model: &str) -> bool {
    let mut updated = false;
    state().update(|s| {
        let id = s.selected_provider_id.clone();
        match s.preferences.update_provider_model(&id, model) {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Default model set to {}.", model.trim()));
                s.persist_preferences();
                updated = true;
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    updated
}

pub fn make_selected_openrouter_default() {
    state().update(|s| {
        let id = s.selected_provider_id.clone();
        match s.preferences.make_default_openrouter_key(&id) {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some("Updated default OpenRouter key.".to_owned());
                s.persist_preferences();
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        }
    });
}

pub fn rename_selected_openrouter_key(new_name: &str) -> bool {
    let mut renamed = false;
    state().update(|s| {
        let id = s.selected_provider_id.clone();
        let linked_secret_id = s
            .preferences
            .openrouter_key(&id)
            .and_then(|k| k.secret_id.clone());
        match s.preferences.rename_openrouter_key(&id, new_name) {
            Ok(()) => {
                if !s.demo_mode
                    && let Some(secret_id) = linked_secret_id
                    && let Some(db_arc) = s.db.as_ref()
                    && let Ok(db) = db_arc.lock()
                    && let Ok(Some(mut record)) = db.get_secret(&secret_id)
                {
                    record.name = new_name.trim().to_owned();
                    let _ = db.update_secret_metadata(s.secret_store.as_ref(), record, None);
                }
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Renamed key to '{}'.", new_name.trim()));
                s.persist_preferences();
                renamed = true;
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    renamed
}

pub fn remove_selected_openrouter_key() {
    state().update(|s| {
        let id = s.selected_provider_id.clone();
        let linked_secret_id = s
            .preferences
            .openrouter_key(&id)
            .and_then(|k| k.secret_id.clone());
        match s.preferences.remove_openrouter_key(&id) {
            Ok(next_selected) => {
                if !s.demo_mode
                    && let Some(secret_id) = linked_secret_id
                    && let Some(db_arc) = s.db.as_ref()
                    && let Ok(db) = db_arc.lock()
                {
                    let _ = db.delete_secret(s.secret_store.as_ref(), &secret_id);
                }
                s.selected_provider_id = next_selected;
                s.preferences_error = None;
                s.preferences_feedback = Some("Removed OpenRouter key.".to_owned());
                s.persist_preferences();
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        }
    });
}

pub fn add_codex_account(cli_binary: &str, default_model: &str) -> bool {
    let mut added = false;
    let mut should_refresh_details = false;
    state().update(
        |s| match s.preferences.add_codex_account(cli_binary, default_model) {
            Ok(id) => {
                s.selected_provider_id = id;
                if !s.demo_mode
                    && let Some(CodexCliProbe::Authenticated { .. }) = s.refresh_codex_status(false)
                {
                    should_refresh_details = true;
                }
                s.preferences_error = None;
                s.preferences_feedback = Some("Added Codex CLI account.".to_owned());
                s.persist_preferences();
                added = true;
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        },
    );
    if should_refresh_details {
        refresh_codex_details_async();
    }
    added
}

pub fn remove_codex_account() {
    state().update(|s| match s.preferences.remove_codex_account() {
        Ok(next_selected) => {
            s.codex_probe_seq.fetch_add(1, Ordering::AcqRel);
            s.codex_details_loading = false;
            s.codex_sign_in_in_progress = false;
            s.selected_provider_id = next_selected;
            s.preferences_error = None;
            s.preferences_feedback = Some("Removed Codex CLI account.".to_owned());
            s.persist_preferences();
        }
        Err(err) => {
            s.preferences_error = Some(err.to_string());
        }
    });
}

pub fn add_antigravity_account(cli_binary: &str, default_model: &str) -> bool {
    let mut added = false;
    let mut should_refresh_usage = false;
    state().update(|s| {
        match s
            .preferences
            .add_antigravity_account(cli_binary, default_model)
        {
            Ok(id) => {
                s.selected_provider_id = id;
                if !s.demo_mode
                    && let Some(AntigravityCliProbe::Authenticated { .. }) =
                        s.refresh_antigravity_status(false)
                {
                    should_refresh_usage = true;
                }
                s.preferences_error = None;
                s.preferences_feedback = Some("Added Antigravity CLI account.".to_owned());
                s.persist_preferences();
                added = true;
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    if should_refresh_usage {
        refresh_antigravity_usage_async();
    }
    added
}

pub fn remove_antigravity_account() {
    state().update(|s| match s.preferences.remove_antigravity_account() {
        Ok(next_selected) => {
            s.antigravity_probe_seq.fetch_add(1, Ordering::AcqRel);
            s.antigravity_details_loading = false;
            s.antigravity_sign_in_in_progress = false;
            s.selected_provider_id = next_selected;
            s.preferences_error = None;
            s.preferences_feedback = Some("Removed Antigravity CLI account.".to_owned());
            s.persist_preferences();
        }
        Err(err) => {
            s.preferences_error = Some(err.to_string());
        }
    });
}

pub fn antigravity_sign_in_in_progress() -> bool {
    state().with(|s| s.antigravity_sign_in_in_progress)
}

pub fn antigravity_details_loading() -> bool {
    state().with(|s| s.antigravity_details_loading)
}

/// Lists the signed-in Antigravity account's models and quota on a background thread.
pub fn refresh_antigravity_details_async() {
    refresh_antigravity_async_inner(false, false);
}

/// Fetches only the signed-in Antigravity account's quota on a background thread (used right
/// after `probe_antigravity_cli`, which already fetched the live model list).
fn refresh_antigravity_usage_async() {
    refresh_antigravity_async_inner(true, false);
}

fn refresh_antigravity_usage_async_inner(from_boot: bool) {
    refresh_antigravity_async_inner(true, from_boot);
}

fn refresh_antigravity_async_inner(usage_only: bool, from_boot: bool) {
    let mut binary_to_query = None;
    state().update(|s| {
        if s.demo_mode || s.antigravity_details_loading {
            return;
        }
        let Some(account) = &mut s.preferences.antigravity else {
            return;
        };
        if !account.connected {
            return;
        }
        let binary = account.cli_binary.trim();
        if binary.is_empty() {
            return;
        }
        s.antigravity_details_loading = true;
        account.usage_warning = None;
        binary_to_query = Some(std::path::PathBuf::from(binary));
    });

    if let Some(binary_path) = binary_to_query {
        let expected_binary = binary_path.display().to_string();
        let phase = if usage_only {
            "boot.async.antigravity_usage"
        } else {
            "boot.async.antigravity_details"
        };
        let boot_span = factory_logging::boot_log::async_span(phase, from_boot);
        Owner::detached().run(|| {
            spawn(async move {
                let details = spawn_blocking_task(move || {
                    if usage_only {
                        fetch_antigravity_usage(&binary_path)
                    } else {
                        fetch_antigravity_details(&binary_path)
                    }
                })
                .await;
                let models_count = details.models.len();
                let has_usage = details.usage.is_some();
                let has_warning = details.warning.is_some();
                state().update(|s| {
                    s.antigravity_details_loading = false;
                    let still_matching = !s.demo_mode
                        && s.preferences
                            .antigravity
                            .as_ref()
                            .is_some_and(|a| a.connected && a.cli_binary == expected_binary);
                    if still_matching {
                        s.preferences.apply_antigravity_details(details);
                    }
                });
                boot_span.finish(format!(
                    "usage={has_usage} models={models_count} has_warning={has_warning}"
                ));
            });
        });
    }
}

/// Updates or clears the custom Antigravity CLI binary path and re-probes its login.
///
/// Returns `true` if the path was accepted (or cleared back to default `PATH`), `false` if the
/// entered path could not be resolved to an executable binary.
pub fn update_antigravity_cli_path(raw_path: &str) -> bool {
    let mut updated = false;
    let mut should_refresh_details = false;
    state().update(|s| {
        if s.preferences.antigravity.is_none() {
            return;
        }
        let trimmed = raw_path.trim();
        if s.demo_mode {
            s.preferences
                .set_antigravity_custom_path(Some(trimmed).filter(|t| !t.is_empty()));
            if let Some(account) = &mut s.preferences.antigravity {
                account.cli_binary = if trimmed.is_empty() { "agy" } else { trimmed }.to_owned();
            }
            s.preferences_error = None;
            s.preferences_feedback = Some(if trimmed.is_empty() {
                "Using Antigravity CLI from default PATH.".to_owned()
            } else {
                format!("Antigravity CLI path set to {trimmed}.")
            });
            s.persist_preferences();
            updated = true;
            return;
        }
        if !trimmed.is_empty() {
            let path_env = s.claude_path_lookup.resolve_path_env();
            if let Err(err) = resolve_antigravity_binary(Some(trimmed), path_env.as_deref()) {
                s.preferences_feedback = None;
                s.preferences_error = Some(err.to_string());
                return;
            }
        }
        s.preferences
            .set_antigravity_custom_path(Some(trimmed).filter(|t| !t.is_empty()));
        if let Some(probe) = s.refresh_antigravity_status(false) {
            should_refresh_details = matches!(probe, AntigravityCliProbe::Authenticated { .. });
            if trimmed.is_empty()
                && let AntigravityCliProbe::BinaryNotFound { error } = &probe
            {
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Cleared custom path. {error}"));
            } else {
                antigravity_probe_feedback(s, &probe, "Using");
            }
        }
        s.persist_preferences();
        updated = true;
    });
    if should_refresh_details {
        refresh_antigravity_usage_async();
    }
    updated
}

/// Clears any custom Antigravity CLI binary path and reverts to default `PATH` discovery.
pub fn reset_antigravity_cli_path() {
    let _ = update_antigravity_cli_path("");
}

/// Toggles visibility of an Antigravity model family (e.g. `"Flash"`) in the model picker.
pub fn toggle_antigravity_model_family(family: &str) {
    state().update(|s| {
        let now_visible = s.preferences.toggle_antigravity_model_family(family);
        s.preferences_error = None;
        s.preferences_feedback = Some(if now_visible {
            format!("Showing {family} models in selection.")
        } else {
            format!("Hiding {family} models from selection.")
        });
        s.persist_preferences();
    });
}

/// Opens Antigravity's Google sign-in on a background thread and refreshes the login state
/// when it finishes.
pub fn sign_in_antigravity() {
    let mut binary_to_run = None;
    state().update(|s| {
        if s.demo_mode {
            let _ = s.preferences.set_cli_connected("antigravity", true);
            s.preferences_error = None;
            s.preferences_feedback = Some("Signed in to Antigravity CLI.".to_owned());
            return;
        }
        if s.antigravity_sign_in_in_progress {
            return;
        }
        let Some(account) = &s.preferences.antigravity else {
            return;
        };
        let path_env = s.claude_path_lookup.resolve_path_env();
        match resolve_antigravity_binary(account.custom_cli_path.as_deref(), path_env.as_deref())
        {
            Ok(binary_path) => {
                s.antigravity_sign_in_in_progress = true;
                s.preferences_error = None;
                s.preferences_feedback = Some(
                    "Opening Antigravity sign-in in your browser... Bruk will update when login finishes."
                        .to_owned(),
                );
                binary_to_run = Some(binary_path);
            }
            Err(err) => {
                s.preferences_feedback = None;
                s.preferences_error = Some(err.to_string());
            }
        }
    });

    if let Some(binary_path) = binary_to_run {
        Owner::detached().run(|| {
            spawn(async move {
                let login_result =
                    spawn_blocking_task(move || run_antigravity_login(&binary_path)).await;
                let mut should_refresh_details = false;
                state().update(|s| {
                    s.antigravity_sign_in_in_progress = false;
                    if s.demo_mode || s.preferences.antigravity.is_none() {
                        return;
                    }
                    match login_result {
                        Ok(()) => {
                            if let Some(probe) = s.refresh_antigravity_status(false) {
                                if matches!(probe, AntigravityCliProbe::NotAuthenticated { .. }) {
                                    s.preferences_feedback = None;
                                    s.preferences_error = Some(
                                        "Antigravity sign-in finished, but no active login was detected."
                                            .to_owned(),
                                    );
                                } else {
                                    should_refresh_details = matches!(
                                        probe,
                                        AntigravityCliProbe::Authenticated { .. }
                                    );
                                    antigravity_probe_feedback(s, &probe, "Signed in to");
                                }
                            }
                        }
                        Err(err) => {
                            s.preferences_feedback = None;
                            s.preferences_error = Some(err);
                        }
                    }
                    s.persist_preferences();
                });
                if should_refresh_details {
                    refresh_antigravity_usage_async();
                }
            });
        });
    }
}

pub fn claude_sign_in_in_progress() -> bool {
    state().with(|s| s.claude_sign_in_in_progress)
}

pub fn claude_details_loading() -> bool {
    state().with(|s| s.claude_details_loading)
}

pub fn codex_sign_in_in_progress() -> bool {
    state().with(|s| s.codex_sign_in_in_progress)
}

pub fn codex_details_loading() -> bool {
    state().with(|s| s.codex_details_loading)
}

/// Fetches the live model catalog and remaining plan usage from the authenticated `codex` CLI
/// on a background thread without blocking the UI.
pub fn refresh_codex_details_async() {
    refresh_codex_details_async_inner(false);
}

fn refresh_codex_details_async_inner(from_boot: bool) {
    let mut binary_to_query = None;
    state().update(|s| {
        if s.demo_mode || s.codex_details_loading {
            return;
        }
        let Some(codex) = &mut s.preferences.codex else {
            return;
        };
        if !codex.connected {
            return;
        }
        let binary = codex.cli_binary.trim();
        if binary.is_empty() {
            return;
        }
        s.codex_details_loading = true;
        codex.usage_warning = None;
        binary_to_query = Some(std::path::PathBuf::from(binary));
    });

    if let Some(binary_path) = binary_to_query {
        let expected_binary = binary_path.display().to_string();
        let boot_span =
            factory_logging::boot_log::async_span("boot.async.codex_details", from_boot);
        Owner::detached().run(|| {
            spawn(async move {
                let details = spawn_blocking_task(move || fetch_codex_details(&binary_path)).await;
                let models_count = details.models.len();
                let has_warning = details.warning.is_some();
                state().update(|s| {
                    s.codex_details_loading = false;
                    let still_matching = !s.demo_mode
                        && s.preferences
                            .codex
                            .as_ref()
                            .is_some_and(|c| c.connected && c.cli_binary == expected_binary);
                    if !still_matching {
                        return;
                    }
                    s.preferences.apply_codex_details(details);
                });
                boot_span.finish(format!("models={models_count} has_warning={has_warning}"));
            });
        });
    }
}

/// Fetches the live model catalog and remaining plan usage from the authenticated `claude` CLI
/// on a background thread without blocking the UI.
pub fn refresh_claude_details_async() {
    refresh_claude_details_async_inner(false);
}

fn refresh_claude_details_async_inner(from_boot: bool) {
    let mut binary_to_query = None;
    state().update(|s| {
        if s.demo_mode || !s.preferences.claude.connected || s.claude_details_loading {
            return;
        }
        let binary = s.preferences.claude.cli_binary.trim();
        if binary.is_empty() {
            return;
        }
        s.claude_details_loading = true;
        s.preferences.claude.usage_warning = None;
        binary_to_query = Some(std::path::PathBuf::from(binary));
    });

    if let Some(binary_path) = binary_to_query {
        let expected_binary = binary_path.display().to_string();
        let boot_span =
            factory_logging::boot_log::async_span("boot.async.claude_details", from_boot);
        Owner::detached().run(|| {
            spawn(async move {
                let details = spawn_blocking_task(move || fetch_claude_details(&binary_path)).await;
                let models_count = details.models.len();
                let has_warning = details.warning.is_some();
                state().update(|s| {
                    s.claude_details_loading = false;
                    if s.demo_mode
                        || !s.preferences.claude.connected
                        || s.preferences.claude.cli_binary != expected_binary
                    {
                        return;
                    }
                    s.preferences.apply_claude_details(details);
                });
                boot_span.finish(format!("models={models_count} has_warning={has_warning}"));
            });
        });
    }
}

/// Updates or clears the custom Claude CLI binary path and re-probes system authentication.
///
/// Returns `true` if the path was accepted (or cleared back to default `PATH`), `false` if the
/// entered path could not be resolved to an executable binary.
pub fn update_claude_cli_path(raw_path: &str) -> bool {
    let mut updated = false;
    let mut should_refresh_details = false;
    state().update(|s| {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            s.preferences.set_claude_custom_path(None);
            if s.demo_mode {
                s.preferences.claude.cli_binary = "claude".to_owned();
                s.preferences_error = None;
                s.preferences_feedback = Some("Using Claude CLI from default PATH.".to_owned());
            } else {
                match s.refresh_claude_status(false) {
                    ClaudeCliProbe::Authenticated { binary_path, .. } => {
                        s.preferences_error = None;
                        s.preferences_feedback = Some(format!(
                            "Using default PATH — verified Claude CLI at {}.",
                            binary_path.display()
                        ));
                        should_refresh_details = true;
                    }
                    ClaudeCliProbe::NotAuthenticated { binary_path, .. } => {
                        s.preferences_error = None;
                        s.preferences_feedback = Some(format!(
                            "Using default PATH ({}) — sign in to verify authentication.",
                            binary_path.display()
                        ));
                    }
                    ClaudeCliProbe::BinaryNotFound { error } => {
                        s.preferences_error = None;
                        s.preferences_feedback = Some(format!("Cleared custom path. {}", error));
                    }
                    ClaudeCliProbe::ProbeFailed { message, .. } => {
                        s.preferences_feedback = None;
                        s.preferences_error = Some(message);
                    }
                }
            }
            s.persist_preferences();
            updated = true;
            return;
        }

        if s.demo_mode {
            s.preferences.set_claude_custom_path(Some(trimmed));
            s.preferences.claude.cli_binary = trimmed.to_owned();
            s.preferences_error = None;
            s.preferences_feedback = Some(format!("Claude CLI path set to {trimmed}."));
            s.persist_preferences();
            updated = true;
            return;
        }

        let path_env = s.claude_path_lookup.resolve_path_env();
        if let Err(err) = resolve_claude_binary(Some(trimmed), path_env.as_deref()) {
            s.preferences_feedback = None;
            s.preferences_error = Some(err.to_string());
            return;
        }

        s.preferences.set_claude_custom_path(Some(trimmed));
        match s.refresh_claude_status(false) {
            ClaudeCliProbe::Authenticated {
                binary_path, email, ..
            } => {
                s.preferences_error = None;
                s.preferences_feedback = Some(format!(
                    "Claude CLI verified at {} ({email}).",
                    binary_path.display()
                ));
                should_refresh_details = true;
            }
            ClaudeCliProbe::NotAuthenticated { binary_path, .. } => {
                s.preferences_error = None;
                s.preferences_feedback = Some(format!(
                    "Found Claude CLI at {}. Sign in to verify authentication.",
                    binary_path.display()
                ));
            }
            ClaudeCliProbe::BinaryNotFound { error } => {
                s.preferences_feedback = None;
                s.preferences_error = Some(error.to_string());
            }
            ClaudeCliProbe::ProbeFailed { message, .. } => {
                s.preferences_feedback = None;
                s.preferences_error = Some(message);
            }
        }
        s.persist_preferences();
        updated = true;
    });
    if should_refresh_details {
        refresh_claude_details_async();
    }
    updated
}

/// Clears any custom Claude CLI binary path override and reverts to default `PATH` discovery.
pub fn reset_claude_cli_path() {
    let _ = update_claude_cli_path("");
}

/// Toggles visibility of a Claude model family (e.g. `"Fable"`) in the model selection view.
pub fn toggle_claude_model_family(family: &str) {
    state().update(|s| {
        let now_visible = s.preferences.toggle_claude_model_family(family);
        s.preferences_error = None;
        s.preferences_feedback = Some(if now_visible {
            format!("Showing {family} models in selection.")
        } else {
            format!("Hiding {family} models from selection.")
        });
        s.persist_preferences();
    });
}

/// Returns the persisted custom Claude spawn settings.
pub fn claude_spawn_settings() -> ClaudeSpawnSettings {
    state().with(|s| s.preferences.claude_spawn_settings.clone())
}

/// Builds a live spawn preview plan for the current Claude binary, default model, and spawn settings.
pub fn claude_spawn_preview_plan() -> Result<ClaudeSpawnPlan, String> {
    state().with(|s| {
        s.preferences
            .build_claude_spawn_plan(&ClaudeSpawnOptions::default())
            .map_err(|err| err.to_string())
    })
}

/// Sets or clears (`None`) a boolean Claude spawn setting override.
pub fn set_claude_spawn_bool(spec_id: &str, value: Option<bool>) {
    state().update(|s| {
        let label = find_claude_setting_spec(spec_id)
            .map(|spec| spec.label)
            .unwrap_or(spec_id);
        match value {
            Some(flag) => {
                match s
                    .preferences
                    .claude_spawn_settings
                    .set_value(spec_id, ClaudeSettingValue::Bool(flag))
                {
                    Ok(()) => {
                        s.preferences_error = None;
                        s.preferences_feedback = Some(format!(
                            "Set {label} to {}.",
                            if flag { "On" } else { "Off" }
                        ));
                        s.persist_preferences();
                    }
                    Err(err) => {
                        s.preferences_feedback = None;
                        s.preferences_error = Some(err.to_string());
                    }
                }
            }
            None => {
                s.preferences.claude_spawn_settings.clear(spec_id);
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Reset {label} to CLI default."));
                s.persist_preferences();
            }
        }
    });
}

/// Sets or clears (`None`) a choice/enum Claude spawn setting override.
pub fn set_claude_spawn_choice(spec_id: &str, choice: Option<&str>) {
    state().update(|s| {
        let label = find_claude_setting_spec(spec_id)
            .map(|spec| spec.label)
            .unwrap_or(spec_id);
        match choice {
            Some(raw) => {
                match s
                    .preferences
                    .claude_spawn_settings
                    .set_from_text(spec_id, raw)
                {
                    Ok(()) => {
                        s.preferences_error = None;
                        s.preferences_feedback = Some(format!("Set {label} to '{raw}'."));
                        s.persist_preferences();
                    }
                    Err(err) => {
                        s.preferences_feedback = None;
                        s.preferences_error = Some(err.to_string());
                    }
                }
            }
            None => {
                s.preferences.claude_spawn_settings.clear(spec_id);
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Reset {label} to CLI default."));
                s.persist_preferences();
            }
        }
    });
}

/// Parses and saves a text-edited Claude spawn setting override. Returns `true` on success.
pub fn set_claude_spawn_text(spec_id: &str, raw: &str) -> bool {
    let mut ok = false;
    state().update(|s| {
        let label = find_claude_setting_spec(spec_id)
            .map(|spec| spec.label)
            .unwrap_or(spec_id);
        match s
            .preferences
            .claude_spawn_settings
            .set_from_text(spec_id, raw)
        {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Updated {label}."));
                s.persist_preferences();
                ok = true;
            }
            Err(err) => {
                s.preferences_feedback = None;
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    ok
}

/// Clears a single Claude spawn setting override back to the CLI default.
pub fn clear_claude_spawn_setting(spec_id: &str) {
    state().update(|s| {
        let label = find_claude_setting_spec(spec_id)
            .map(|spec| spec.label)
            .unwrap_or(spec_id);
        s.preferences.claude_spawn_settings.clear(spec_id);
        s.preferences_error = None;
        s.preferences_feedback = Some(format!("Reset {label} to CLI default."));
        s.persist_preferences();
    });
}

/// Adds or updates a Claude spawn environment variable override. Returns `true` on success.
pub fn set_claude_spawn_env_var(key: &str, value: &str) -> bool {
    let mut ok = false;
    state().update(|s| {
        let normalized = key.trim().to_owned();
        match s
            .preferences
            .claude_spawn_settings
            .set_env_var(&normalized, value)
        {
            Ok(()) => {
                if let Some(old_secret_id) = s
                    .preferences
                    .claude_spawn_settings
                    .secret_env_vars
                    .remove(&normalized)
                    && !s.demo_mode
                    && let Some(db_arc) = s.db.as_ref()
                    && let Ok(db) = db_arc.lock()
                {
                    let _ = db.delete_secret(s.secret_store.as_ref(), &old_secret_id);
                }
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Saved environment variable {normalized}."));
                s.persist_preferences();
                ok = true;
            }
            Err(err) => {
                s.preferences_feedback = None;
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    ok
}

/// Adds or updates a secret Claude spawn environment variable stored in Keychain.
pub fn set_claude_spawn_secret_env_var(key: &str, raw_value: &str) -> bool {
    let mut ok = false;
    state().update(|s| {
        let normalized = key.trim().to_owned();
        if let Err(err) = validate_env_name(&normalized) {
            s.preferences_feedback = None;
            s.preferences_error = Some(err.to_string());
            return;
        }
        if let Err(err) = validate_secret_value(raw_value) {
            s.preferences_feedback = None;
            s.preferences_error = Some(err.to_string());
            return;
        }
        if !s.demo_mode
            && let Some(db_arc) = s.db.as_ref()
            && let Ok(db) = db_arc.lock()
        {
            let secret = SecretString::from(raw_value.to_owned());
            let secret_id = if let Some(existing_id) = s
                .preferences
                .claude_spawn_settings
                .secret_env_vars
                .get(&normalized)
                .cloned()
            {
                match db.replace_keychain_secret(
                    s.secret_store.as_ref(),
                    &existing_id,
                    &secret,
                    None,
                ) {
                    Ok(rec) => rec.id,
                    Err(err) => {
                        s.preferences_error =
                            Some(format!("Could not update secret in Keychain: {err}"));
                        return;
                    }
                }
            } else {
                let record = SecretRecord::new_keychain(
                    format!("Claude · {normalized}"),
                    SecretKind::Internal {
                        purpose: InternalPurpose::ProviderSpawnEnv,
                    },
                    current_unix_seconds(),
                );
                match db.create_keychain_secret(s.secret_store.as_ref(), record, &secret, None) {
                    Ok(rec) => rec.id,
                    Err(err) => {
                        s.preferences_error =
                            Some(format!("Could not save secret in Keychain: {err}"));
                        return;
                    }
                }
            };
            s.preferences
                .claude_spawn_settings
                .env_vars
                .remove(&normalized);
            s.preferences
                .claude_spawn_settings
                .secret_env_vars
                .insert(normalized.clone(), secret_id);
        }
        s.preferences_error = None;
        s.preferences_feedback = Some(format!(
            "Saved secret environment variable {normalized} in Keychain."
        ));
        s.persist_preferences();
        ok = true;
    });
    ok
}

/// Removes a configured Claude spawn environment variable (plain or secret).
pub fn remove_claude_spawn_env_var(key: &str) {
    state().update(|s| {
        let removed_plain = s.preferences.claude_spawn_settings.remove_env_var(key);
        let removed_secret = s
            .preferences
            .claude_spawn_settings
            .secret_env_vars
            .remove(key);
        if let Some(secret_id) = removed_secret.as_deref()
            && !s.demo_mode
            && let Some(db_arc) = s.db.as_ref()
            && let Ok(db) = db_arc.lock()
        {
            let _ = db.delete_secret(s.secret_store.as_ref(), secret_id);
        }
        if removed_plain || removed_secret.is_some() {
            s.preferences_error = None;
            s.preferences_feedback = Some(format!("Removed environment variable {key}."));
            s.persist_preferences();
        }
    });
}

/// Updates extra raw CLI arguments appended when spawning Claude. Returns `true` on success.
pub fn set_claude_spawn_extra_cli_args(raw: &str) -> bool {
    let mut ok = false;
    state().update(
        |s| match s.preferences.claude_spawn_settings.set_extra_cli_args(raw) {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some(if raw.trim().is_empty() {
                    "Cleared extra Claude CLI arguments.".to_owned()
                } else {
                    "Updated extra Claude CLI arguments.".to_owned()
                });
                s.persist_preferences();
                ok = true;
            }
            Err(err) => {
                s.preferences_feedback = None;
                s.preferences_error = Some(err.to_string());
            }
        },
    );
    ok
}

/// Updates extra raw JSON merged into `--settings` when spawning Claude. Returns `true` on success.
pub fn set_claude_spawn_extra_settings_json(raw: &str) -> bool {
    let mut ok = false;
    state().update(|s| {
        match s
            .preferences
            .claude_spawn_settings
            .set_extra_settings_json(raw)
        {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some(if raw.trim().is_empty() {
                    "Cleared extra Claude --settings JSON.".to_owned()
                } else {
                    "Updated extra Claude --settings JSON.".to_owned()
                });
                s.persist_preferences();
                ok = true;
            }
            Err(err) => {
                s.preferences_feedback = None;
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    ok
}

/// Resets all custom Claude spawn settings back to CLI defaults.
pub fn reset_all_claude_spawn_settings() {
    state().update(|s| {
        s.preferences.claude_spawn_settings.reset_all();
        s.preferences_error = None;
        s.preferences_feedback =
            Some("Reset all Claude spawn settings to CLI defaults.".to_owned());
        s.persist_preferences();
    });
}

/// Returns the persisted custom Codex spawn settings.
pub fn codex_spawn_settings() -> CodexSpawnSettings {
    state().with(|s| s.preferences.codex_spawn_settings.clone())
}

/// Builds a live spawn preview plan for the current Codex binary, default model, and spawn settings.
pub fn codex_spawn_preview_plan() -> Result<CodexSpawnPlan, String> {
    state().with(|s| {
        s.preferences
            .build_codex_spawn_plan(&CodexSpawnOptions::default())
            .map_err(|err| err.to_string())
    })
}

/// Sets or clears (`None`) a boolean Codex spawn setting override.
pub fn set_codex_spawn_bool(spec_id: &str, value: Option<bool>) {
    state().update(|s| {
        let label = find_codex_setting_spec(spec_id)
            .map(|spec| spec.label)
            .unwrap_or(spec_id);
        match value {
            Some(flag) => {
                match s
                    .preferences
                    .codex_spawn_settings
                    .set_value(spec_id, CodexSettingValue::Bool(flag))
                {
                    Ok(()) => {
                        s.preferences_error = None;
                        s.preferences_feedback = Some(format!(
                            "Set {label} to {}.",
                            if flag { "On" } else { "Off" }
                        ));
                        s.persist_preferences();
                    }
                    Err(err) => {
                        s.preferences_feedback = None;
                        s.preferences_error = Some(err.to_string());
                    }
                }
            }
            None => {
                s.preferences.codex_spawn_settings.clear(spec_id);
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Reset {label} to CLI default."));
                s.persist_preferences();
            }
        }
    });
}

/// Sets or clears (`None`) a choice/enum Codex spawn setting override.
pub fn set_codex_spawn_choice(spec_id: &str, choice: Option<&str>) {
    state().update(|s| {
        let label = find_codex_setting_spec(spec_id)
            .map(|spec| spec.label)
            .unwrap_or(spec_id);
        match choice {
            Some(raw) => {
                match s
                    .preferences
                    .codex_spawn_settings
                    .set_from_text(spec_id, raw)
                {
                    Ok(()) => {
                        s.preferences_error = None;
                        s.preferences_feedback = Some(format!("Set {label} to '{raw}'."));
                        s.persist_preferences();
                    }
                    Err(err) => {
                        s.preferences_feedback = None;
                        s.preferences_error = Some(err.to_string());
                    }
                }
            }
            None => {
                s.preferences.codex_spawn_settings.clear(spec_id);
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Reset {label} to CLI default."));
                s.persist_preferences();
            }
        }
    });
}

/// Parses and saves a text-edited Codex spawn setting override. Returns `true` on success.
pub fn set_codex_spawn_text(spec_id: &str, raw: &str) -> bool {
    let mut ok = false;
    state().update(|s| {
        let label = find_codex_setting_spec(spec_id)
            .map(|spec| spec.label)
            .unwrap_or(spec_id);
        match s
            .preferences
            .codex_spawn_settings
            .set_from_text(spec_id, raw)
        {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Updated {label}."));
                s.persist_preferences();
                ok = true;
            }
            Err(err) => {
                s.preferences_feedback = None;
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    ok
}

/// Clears a single Codex spawn setting override back to the CLI default.
pub fn clear_codex_spawn_setting(spec_id: &str) {
    state().update(|s| {
        let label = find_codex_setting_spec(spec_id)
            .map(|spec| spec.label)
            .unwrap_or(spec_id);
        s.preferences.codex_spawn_settings.clear(spec_id);
        s.preferences_error = None;
        s.preferences_feedback = Some(format!("Reset {label} to CLI default."));
        s.persist_preferences();
    });
}

/// Adds or updates a Codex spawn environment variable override. Returns `true` on success.
pub fn set_codex_spawn_env_var(key: &str, value: &str) -> bool {
    let mut ok = false;
    state().update(|s| {
        let normalized = key.trim().to_owned();
        match s
            .preferences
            .codex_spawn_settings
            .set_env_var(&normalized, value)
        {
            Ok(()) => {
                if let Some(old_secret_id) = s
                    .preferences
                    .codex_spawn_settings
                    .secret_env_vars
                    .remove(&normalized)
                    && !s.demo_mode
                    && let Some(db_arc) = s.db.as_ref()
                    && let Ok(db) = db_arc.lock()
                {
                    let _ = db.delete_secret(s.secret_store.as_ref(), &old_secret_id);
                }
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Saved environment variable {normalized}."));
                s.persist_preferences();
                ok = true;
            }
            Err(err) => {
                s.preferences_feedback = None;
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    ok
}

/// Adds or updates a secret Codex spawn environment variable stored in Keychain.
pub fn set_codex_spawn_secret_env_var(key: &str, raw_value: &str) -> bool {
    let mut ok = false;
    state().update(|s| {
        let normalized = key.trim().to_owned();
        if let Err(err) = validate_env_name(&normalized) {
            s.preferences_feedback = None;
            s.preferences_error = Some(err.to_string());
            return;
        }
        if let Err(err) = validate_secret_value(raw_value) {
            s.preferences_feedback = None;
            s.preferences_error = Some(err.to_string());
            return;
        }
        if !s.demo_mode
            && let Some(db_arc) = s.db.as_ref()
            && let Ok(db) = db_arc.lock()
        {
            let secret = SecretString::from(raw_value.to_owned());
            let secret_id = if let Some(existing_id) = s
                .preferences
                .codex_spawn_settings
                .secret_env_vars
                .get(&normalized)
                .cloned()
            {
                match db.replace_keychain_secret(
                    s.secret_store.as_ref(),
                    &existing_id,
                    &secret,
                    None,
                ) {
                    Ok(rec) => rec.id,
                    Err(err) => {
                        s.preferences_error =
                            Some(format!("Could not update secret in Keychain: {err}"));
                        return;
                    }
                }
            } else {
                let record = SecretRecord::new_keychain(
                    format!("Codex · {normalized}"),
                    SecretKind::Internal {
                        purpose: InternalPurpose::ProviderSpawnEnv,
                    },
                    current_unix_seconds(),
                );
                match db.create_keychain_secret(s.secret_store.as_ref(), record, &secret, None) {
                    Ok(rec) => rec.id,
                    Err(err) => {
                        s.preferences_error =
                            Some(format!("Could not save secret in Keychain: {err}"));
                        return;
                    }
                }
            };
            s.preferences
                .codex_spawn_settings
                .env_vars
                .remove(&normalized);
            s.preferences
                .codex_spawn_settings
                .secret_env_vars
                .insert(normalized.clone(), secret_id);
        }
        s.preferences_error = None;
        s.preferences_feedback = Some(format!(
            "Saved secret environment variable {normalized} in Keychain."
        ));
        s.persist_preferences();
        ok = true;
    });
    ok
}

/// Removes a configured Codex spawn environment variable (plain or secret).
pub fn remove_codex_spawn_env_var(key: &str) {
    state().update(|s| {
        let removed_plain = s.preferences.codex_spawn_settings.remove_env_var(key);
        let removed_secret = s
            .preferences
            .codex_spawn_settings
            .secret_env_vars
            .remove(key);
        if let Some(secret_id) = removed_secret.as_deref()
            && !s.demo_mode
            && let Some(db_arc) = s.db.as_ref()
            && let Ok(db) = db_arc.lock()
        {
            let _ = db.delete_secret(s.secret_store.as_ref(), secret_id);
        }
        if removed_plain || removed_secret.is_some() {
            s.preferences_error = None;
            s.preferences_feedback = Some(format!("Removed environment variable {key}."));
            s.persist_preferences();
        }
    });
}

/// Updates extra raw CLI arguments appended when spawning Codex. Returns `true` on success.
pub fn set_codex_spawn_extra_cli_args(raw: &str) -> bool {
    let mut ok = false;
    state().update(
        |s| match s.preferences.codex_spawn_settings.set_extra_cli_args(raw) {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some(if raw.trim().is_empty() {
                    "Cleared extra Codex CLI arguments.".to_owned()
                } else {
                    "Updated extra Codex CLI arguments.".to_owned()
                });
                s.persist_preferences();
                ok = true;
            }
            Err(err) => {
                s.preferences_feedback = None;
                s.preferences_error = Some(err.to_string());
            }
        },
    );
    ok
}

/// Updates extra raw `-c` config.toml overrides when spawning Codex. Returns `true` on success.
pub fn set_codex_spawn_extra_config_overrides(raw: &str) -> bool {
    let mut ok = false;
    state().update(|s| {
        match s
            .preferences
            .codex_spawn_settings
            .set_extra_config_overrides(raw)
        {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some(if raw.trim().is_empty() {
                    "Cleared extra Codex -c config overrides.".to_owned()
                } else {
                    "Updated extra Codex -c config overrides.".to_owned()
                });
                s.persist_preferences();
                ok = true;
            }
            Err(err) => {
                s.preferences_feedback = None;
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    ok
}

/// Resets all custom Codex spawn settings back to CLI defaults.
pub fn reset_all_codex_spawn_settings() {
    state().update(|s| {
        s.preferences.codex_spawn_settings.reset_all();
        s.preferences_error = None;
        s.preferences_feedback = Some("Reset all Codex spawn settings to CLI defaults.".to_owned());
        s.persist_preferences();
    });
}

/// Launches `claude auth login` on a background thread and refreshes system login status when done.
pub fn sign_in_claude() {
    let mut binary_to_run = None;
    state().update(|s| {
        if s.demo_mode {
            let _ = s.preferences.set_cli_connected("claude", true);
            s.preferences_error = None;
            s.preferences_feedback = Some("Signed in to Claude CLI.".to_owned());
            return;
        }
        if s.claude_sign_in_in_progress {
            return;
        }
        let path_env = s.claude_path_lookup.resolve_path_env();
        match resolve_claude_binary(
            s.preferences.claude.custom_cli_path.as_deref(),
            path_env.as_deref(),
        ) {
            Ok(binary_path) => {
                s.claude_sign_in_in_progress = true;
                s.preferences_error = None;
                s.preferences_feedback = Some(
                    "Opening Claude sign-in in your browser... Bruk will update when login finishes."
                        .to_owned(),
                );
                binary_to_run = Some(binary_path);
            }
            Err(err) => {
                s.preferences_feedback = None;
                s.preferences_error = Some(err.to_string());
            }
        }
    });

    if let Some(binary_path) = binary_to_run {
        Owner::detached().run(|| {
            spawn(async move {
                let login_result =
                    spawn_blocking_task(move || run_claude_auth_login(&binary_path)).await;
                let mut should_refresh_details = false;
                state().update(|s| {
                    s.claude_sign_in_in_progress = false;
                    if s.demo_mode {
                        return;
                    }
                    match login_result {
                        Ok(()) => match s.refresh_claude_status(false) {
                            ClaudeCliProbe::Authenticated { email, .. } => {
                                s.preferences_error = None;
                                s.preferences_feedback =
                                    Some(format!("Signed in to Claude CLI as {email}."));
                                should_refresh_details = true;
                            }
                            ClaudeCliProbe::NotAuthenticated { .. } => {
                                s.preferences_feedback = None;
                                s.preferences_error = Some(
                                    "Claude sign-in finished, but no active login was detected."
                                        .to_owned(),
                                );
                            }
                            ClaudeCliProbe::BinaryNotFound { error } => {
                                s.preferences_feedback = None;
                                s.preferences_error = Some(error.to_string());
                            }
                            ClaudeCliProbe::ProbeFailed { message, .. } => {
                                s.preferences_feedback = None;
                                s.preferences_error = Some(message);
                            }
                        },
                        Err(err) => {
                            s.preferences_feedback = None;
                            s.preferences_error = Some(err);
                        }
                    }
                    s.persist_preferences();
                });
                if should_refresh_details {
                    refresh_claude_details_async();
                }
            });
        });
    }
}

/// Updates or clears the custom Codex CLI binary path and re-probes system authentication.
///
/// Returns `true` if the path was accepted (or cleared back to default `PATH`), `false` if the
/// entered path could not be resolved to an executable binary.
pub fn update_codex_cli_path(raw_path: &str) -> bool {
    let mut updated = false;
    let mut should_refresh_details = false;
    state().update(|s| {
        if s.preferences.codex.is_none() {
            return;
        }
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            s.preferences.set_codex_custom_path(None);
            if s.demo_mode {
                if let Some(codex) = &mut s.preferences.codex {
                    codex.cli_binary = "codex".to_owned();
                }
                s.preferences_error = None;
                s.preferences_feedback = Some("Using Codex CLI from default PATH.".to_owned());
            } else if let Some(probe) = s.refresh_codex_status(false) {
                match probe {
                    CodexCliProbe::Authenticated { binary_path, .. } => {
                        s.preferences_error = None;
                        s.preferences_feedback = Some(format!(
                            "Using default PATH — verified Codex CLI at {}.",
                            binary_path.display()
                        ));
                        should_refresh_details = true;
                    }
                    CodexCliProbe::NotAuthenticated { binary_path, .. } => {
                        s.preferences_error = None;
                        s.preferences_feedback = Some(format!(
                            "Using default PATH ({}) — sign in to verify authentication.",
                            binary_path.display()
                        ));
                    }
                    CodexCliProbe::BinaryNotFound { error } => {
                        s.preferences_error = None;
                        s.preferences_feedback = Some(format!("Cleared custom path. {error}"));
                    }
                    CodexCliProbe::ProbeFailed { message, .. } => {
                        s.preferences_feedback = None;
                        s.preferences_error = Some(message);
                    }
                }
            }
            s.persist_preferences();
            updated = true;
            return;
        }

        if s.demo_mode {
            s.preferences.set_codex_custom_path(Some(trimmed));
            if let Some(codex) = &mut s.preferences.codex {
                codex.cli_binary = trimmed.to_owned();
            }
            s.preferences_error = None;
            s.preferences_feedback = Some(format!("Codex CLI path set to {trimmed}."));
            s.persist_preferences();
            updated = true;
            return;
        }

        let path_env = s.claude_path_lookup.resolve_path_env();
        if let Err(err) = resolve_codex_binary(Some(trimmed), path_env.as_deref()) {
            s.preferences_feedback = None;
            s.preferences_error = Some(err.to_string());
            return;
        }

        s.preferences.set_codex_custom_path(Some(trimmed));
        if let Some(probe) = s.refresh_codex_status(false) {
            match probe {
                CodexCliProbe::Authenticated {
                    binary_path, email, ..
                } => {
                    s.preferences_error = None;
                    s.preferences_feedback = Some(format!(
                        "Codex CLI verified at {} ({email}).",
                        binary_path.display()
                    ));
                    should_refresh_details = true;
                }
                CodexCliProbe::NotAuthenticated { binary_path, .. } => {
                    s.preferences_error = None;
                    s.preferences_feedback = Some(format!(
                        "Found Codex CLI at {}. Sign in to verify authentication.",
                        binary_path.display()
                    ));
                }
                CodexCliProbe::BinaryNotFound { error } => {
                    s.preferences_feedback = None;
                    s.preferences_error = Some(error.to_string());
                }
                CodexCliProbe::ProbeFailed { message, .. } => {
                    s.preferences_feedback = None;
                    s.preferences_error = Some(message);
                }
            }
        }
        s.persist_preferences();
        updated = true;
    });
    if should_refresh_details {
        refresh_codex_details_async();
    }
    updated
}

/// Clears any custom Codex CLI binary path override and reverts to default `PATH` discovery.
pub fn reset_codex_cli_path() {
    let _ = update_codex_cli_path("");
}

/// Toggles visibility of a Codex model family (e.g. `"Luna"`) in the model selection view.
pub fn toggle_codex_model_family(family: &str) {
    state().update(|s| {
        let now_visible = s.preferences.toggle_codex_model_family(family);
        s.preferences_error = None;
        s.preferences_feedback = Some(if now_visible {
            format!("Showing {family} models in selection.")
        } else {
            format!("Hiding {family} models from selection.")
        });
        s.persist_preferences();
    });
}

/// Launches `codex login` on a background thread and refreshes system login status when done.
pub fn sign_in_codex() {
    let mut binary_to_run = None;
    state().update(|s| {
        if s.demo_mode {
            let _ = s.preferences.set_cli_connected("codex", true);
            s.preferences_error = None;
            s.preferences_feedback = Some("Signed in to Codex CLI.".to_owned());
            return;
        }
        if s.codex_sign_in_in_progress {
            return;
        }
        let Some(codex) = &s.preferences.codex else {
            return;
        };
        let path_env = s.claude_path_lookup.resolve_path_env();
        match resolve_codex_binary(codex.custom_cli_path.as_deref(), path_env.as_deref()) {
            Ok(binary_path) => {
                s.codex_sign_in_in_progress = true;
                s.preferences_error = None;
                s.preferences_feedback = Some(
                    "Opening Codex sign-in in your browser... Bruk will update when login finishes."
                        .to_owned(),
                );
                binary_to_run = Some(binary_path);
            }
            Err(err) => {
                s.preferences_feedback = None;
                s.preferences_error = Some(err.to_string());
            }
        }
    });

    if let Some(binary_path) = binary_to_run {
        Owner::detached().run(|| {
            spawn(async move {
                let login_result =
                    spawn_blocking_task(move || run_codex_login(&binary_path)).await;
                let mut should_refresh_details = false;
                state().update(|s| {
                    s.codex_sign_in_in_progress = false;
                    if s.demo_mode || s.preferences.codex.is_none() {
                        return;
                    }
                    match login_result {
                        Ok(()) => {
                            if let Some(probe) = s.refresh_codex_status(false) {
                                match probe {
                                    CodexCliProbe::Authenticated { email, .. } => {
                                        s.preferences_error = None;
                                        s.preferences_feedback =
                                            Some(format!("Signed in to Codex CLI as {email}."));
                                        should_refresh_details = true;
                                    }
                                    CodexCliProbe::NotAuthenticated { .. } => {
                                        s.preferences_feedback = None;
                                        s.preferences_error = Some(
                                            "Codex sign-in finished, but no active login was detected."
                                                .to_owned(),
                                        );
                                    }
                                    CodexCliProbe::BinaryNotFound { error } => {
                                        s.preferences_feedback = None;
                                        s.preferences_error = Some(error.to_string());
                                    }
                                    CodexCliProbe::ProbeFailed { message, .. } => {
                                        s.preferences_feedback = None;
                                        s.preferences_error = Some(message);
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            s.preferences_feedback = None;
                            s.preferences_error = Some(err);
                        }
                    }
                    s.persist_preferences();
                });
                if should_refresh_details {
                    refresh_codex_details_async();
                }
            });
        });
    }
}

pub fn set_appearance_mode(mode: AppearanceMode) {
    let mut updated = None;
    state().update(|s| {
        s.preferences.appearance.mode = mode;
        updated = Some(s.preferences.appearance.clone());
        s.persist_preferences();
    });
    if let Some(appearance) = updated {
        apply_appearance_settings(&appearance);
    }
}

pub fn set_ui_font(font: FontPreference) {
    let mut updated = None;
    state().update(|s| {
        s.preferences.appearance.ui_font = font;
        updated = Some(s.preferences.appearance.clone());
        s.persist_preferences();
    });
    if let Some(appearance) = updated {
        apply_appearance_settings(&appearance);
    }
}

pub fn set_mono_font(font: FontPreference) {
    let mut updated = None;
    state().update(|s| {
        s.preferences.appearance.mono_font = font;
        updated = Some(s.preferences.appearance.clone());
        s.persist_preferences();
    });
    if let Some(appearance) = updated {
        apply_appearance_settings(&appearance);
    }
}

#[derive(Clone, Default)]
struct FontPickerBridge {
    channel: factory_fonts::FontPickChannel,
    listening: bool,
}

fn ensure_font_picker_channel() -> factory_fonts::FontPickChannel {
    let bridge = global::<FontPickerBridge>();
    let (channel, start_listener) = bridge.with_untracked(|b| (b.channel.clone(), !b.listening));
    if start_listener {
        bridge.update(|b| b.listening = true);
        let listener_channel = channel.clone();
        spawn(async move {
            loop {
                let (target, family) = listener_channel.next().await;
                let pref = factory_fonts::normalize_picked_font(target, &family);
                match target {
                    factory_fonts::FontPickerTarget::Ui => set_ui_font(pref),
                    factory_fonts::FontPickerTarget::Mono => set_mono_font(pref),
                }
            }
        });
    }
    channel
}

pub fn open_ui_font_picker() {
    let current = state().with_untracked(|s| {
        s.preferences
            .appearance
            .ui_font
            .resolve_ui()
            .map(str::to_owned)
    });
    let channel = ensure_font_picker_channel();
    factory_fonts::open_font_panel(factory_fonts::FontPickerTarget::Ui, current, channel);
}

pub fn open_mono_font_picker() {
    let current = state().with_untracked(|s| {
        s.preferences
            .appearance
            .mono_font
            .resolve_mono()
            .map(str::to_owned)
    });
    let channel = ensure_font_picker_channel();
    factory_fonts::open_font_panel(factory_fonts::FontPickerTarget::Mono, current, channel);
}

pub fn close_font_picker() {
    factory_fonts::close_font_panel();
}

pub fn toggle_compact_sidebar() {
    state().update(|s| {
        s.preferences.appearance.compact_sidebar = !s.preferences.appearance.compact_sidebar;
        s.persist_preferences();
    });
}

pub fn set_monospace_font_size(size: u8) {
    state().update(|s| {
        s.preferences.appearance.monospace_font_size = size.clamp(11, 16);
        s.persist_preferences();
    });
}

pub fn set_default_landing_view(view: DefaultLandingView) {
    state().update(|s| {
        s.preferences.general.default_landing_view = view;
        s.persist_preferences();
    });
}

/// Sets where new projects are cloned. An empty value, or the Bruk folder's own `projects/`,
/// is stored as "follow the Bruk folder" so it keeps tracking the folder chosen at setup.
pub fn set_default_projects_dir(dir: &str) {
    state().update(|s| {
        let trimmed = dir.trim();
        let follows_home = trimmed.is_empty()
            || s.home
                .as_ref()
                .is_some_and(|home| expand_tilde(trimmed) == home.projects_dir());
        s.preferences.general.default_projects_dir = if follows_home {
            String::new()
        } else {
            trimmed.to_owned()
        };
        s.persist_preferences();
    });
}

/// The folder new projects are cloned into, shortened with `~` for display.
pub fn default_projects_dir_display() -> String {
    state().with(|s| {
        s.projects_dir()
            .map(|dir| display_with_tilde(&dir))
            .unwrap_or_default()
    })
}

/// The default projects directory, when it exists on disk (used to start folder pickers there).
pub fn default_projects_dir_path() -> Option<std::path::PathBuf> {
    state()
        .with_untracked(AppState::projects_dir)
        .filter(|dir| dir.is_dir())
}

pub fn toggle_general_flag(flag: GeneralFlag) {
    state().update(|s| {
        match flag {
            GeneralFlag::ConfirmBeforeCancel => {
                s.preferences.general.confirm_before_cancel =
                    !s.preferences.general.confirm_before_cancel;
            }
            GeneralFlag::NotifyHumanGate => {
                s.preferences.general.notify_on_human_gate =
                    !s.preferences.general.notify_on_human_gate;
            }
            GeneralFlag::NotifyRunFailure => {
                s.preferences.general.notify_on_run_failure =
                    !s.preferences.general.notify_on_run_failure;
            }
            GeneralFlag::AutoCheckUpdates => {
                s.preferences.general.auto_check_updates =
                    !s.preferences.general.auto_check_updates;
            }
        }
        s.persist_preferences();
    });
}

#[derive(Debug, Clone, Copy)]
pub enum GeneralFlag {
    ConfirmBeforeCancel,
    NotifyHumanGate,
    NotifyRunFailure,
    AutoCheckUpdates,
}

pub fn selected_prefs_machine_id() -> Option<u64> {
    state().with(|s| {
        s.selected_prefs_machine_id
            .filter(|id| s.machines_fleet.machine(*id).is_some())
            .or_else(|| s.machines_fleet.default_selected_id())
    })
}

pub fn selected_prefs_machine() -> Option<MachineRecord> {
    state().with(|s| {
        let id = s
            .selected_prefs_machine_id
            .filter(|id| s.machines_fleet.machine(*id).is_some())
            .or_else(|| s.machines_fleet.default_selected_id())?;
        s.machines_fleet.machine(id).cloned()
    })
}

pub fn select_prefs_machine(id: u64) {
    state().update(|s| {
        if s.machines_fleet.machine(id).is_some() {
            s.selected_prefs_machine_id = Some(id);
            s.preferences_error = None;
            s.preferences_feedback = None;
        }
    });
}

pub fn test_prefs_machine_connection(id: u64) {
    state().update(|s| match s.machines_fleet.test_connection(id) {
        Ok(msg) => {
            s.preferences_error = None;
            s.preferences_feedback = Some(msg);
        }
        Err(err) => {
            s.preferences_error = Some(err.to_string());
        }
    });
}

pub fn update_prefs_machine_user(id: u64, user: &str) -> bool {
    let mut ok = false;
    state().update(|s| match s.machines_fleet.set_ssh_user(id, user) {
        Ok(()) => {
            s.preferences_error = None;
            s.preferences_feedback = Some(format!("Updated SSH user to '{}'.", user.trim()));
            ok = true;
        }
        Err(err) => {
            s.preferences_error = Some(err.to_string());
        }
    });
    ok
}

pub fn update_prefs_machine_jump_host(id: u64, jump_host: &str) -> bool {
    let mut ok = false;
    state().update(|s| match s.machines_fleet.set_jump_host(id, jump_host) {
        Ok(()) => {
            s.preferences_error = None;
            let trimmed = jump_host.trim();
            s.preferences_feedback = Some(
                if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
                    "Cleared jump host.".to_owned()
                } else {
                    format!("Jump host set to '{trimmed}'.")
                },
            );
            ok = true;
        }
        Err(err) => {
            s.preferences_error = Some(err.to_string());
        }
    });
    ok
}

pub fn update_prefs_machine_ssh_key(
    id: u64,
    key_name: &str,
    key_type: &str,
    fingerprint: &str,
) -> bool {
    let mut ok = false;
    state().update(|s| {
        match s
            .machines_fleet
            .set_ssh_key(id, key_name, key_type, fingerprint)
        {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("SSH key set to '{}'.", key_name.trim()));
                ok = true;
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    ok
}

pub fn check_prefs_machine_core(id: u64) {
    state().update(|s| match s.machines_fleet.check_core_now(id) {
        Ok(msg) => {
            s.preferences_error = None;
            s.preferences_feedback = Some(msg);
        }
        Err(err) => {
            s.preferences_error = Some(err.to_string());
        }
    });
}

pub fn set_prefs_machine_core_update_policy(id: u64, policy: CoreUpdatePolicy) {
    state().update(
        |s| match s.machines_fleet.set_core_update_policy(id, policy) {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some(format!(
                    "Core update policy set to '{}'.",
                    policy.short_label()
                ));
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        },
    );
}

pub fn set_prefs_machine_agent_signed_in(id: u64, agent: MachineAgentKind, signed_in: bool) {
    state().update(
        |s| match s.machines_fleet.set_agent_signed_in(id, agent, signed_in) {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some(if signed_in {
                    format!("Signed in to {} inside Bruk.", agent.label())
                } else {
                    format!("Signed out of {} on this machine.", agent.label())
                });
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        },
    );
}

pub fn update_prefs_machine_max_sessions(id: u64, value_str: &str) -> bool {
    let parsed = value_str.trim().parse::<u32>();
    let mut ok = false;
    state().update(|s| match parsed {
        Ok(n) => match s.machines_fleet.set_max_sessions_at_once(id, n) {
            Ok(()) => {
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Sessions at once set to {n}."));
                ok = true;
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        },
        Err(_) => {
            s.preferences_error =
                Some("Sessions at once must be a number between 1 and 32.".to_owned());
        }
    });
    ok
}

pub fn toggle_prefs_machine_project(id: u64, project_name: &str) {
    let project = project_name.to_owned();
    state().update(
        |s| match s.machines_fleet.toggle_project_access(id, &project) {
            Ok(enabled) => {
                s.preferences_error = None;
                s.preferences_feedback = Some(if enabled {
                    format!("Allowed '{project}' on this machine.")
                } else {
                    format!("Removed '{project}' from this machine.")
                });
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        },
    );
}

pub fn add_prefs_machine(
    name: &str,
    ipv4: &str,
    port: &str,
    user: &str,
    platform: MachinePlatform,
) -> bool {
    let trimmed_name = name.trim();
    let trimmed_user = if user.trim().is_empty() {
        "bruk"
    } else {
        user.trim()
    };
    let trimmed_port = if port.trim().is_empty() {
        "22"
    } else {
        port.trim()
    };
    let address = if ipv4.trim().is_empty() {
        String::new()
    } else if ipv4.contains(':') {
        ipv4.trim().to_owned()
    } else {
        format!("{}:{trimmed_port}", ipv4.trim())
    };
    let ssh_target = if trimmed_name.is_empty() {
        String::new()
    } else {
        format!("{trimmed_user}@{trimmed_name}")
    };
    let draft = NewMachineDraft {
        name: trimmed_name.to_owned(),
        platform,
        os_label: String::new(),
        arch: String::new(),
        address,
        ssh_target,
        allowed_projects: available_machine_project_names(),
    };

    let mut added_ok = false;
    state().update(|s| match s.machines_fleet.add_machine(draft) {
        Ok(added) => {
            let new_id = added.id;
            let new_name = added.name.clone();
            s.selected_prefs_machine_id = Some(new_id);
            s.preferences_error = None;
            s.preferences_feedback = Some(format!("Added machine '{new_name}'."));
            added_ok = true;
        }
        Err(err) => {
            s.preferences_error = Some(err.to_string());
        }
    });
    added_ok
}

pub fn remove_prefs_machine(id: u64) -> bool {
    let mut removed_ok = false;
    state().update(|s| match s.machines_fleet.remove_machine(id) {
        Ok(removed) => {
            if s.selected_prefs_machine_id == Some(id) {
                s.selected_prefs_machine_id = s.machines_fleet.default_selected_id();
            }
            if s.selected_machine_id == Some(id) {
                s.selected_machine_id = s.machines_fleet.default_selected_id();
            }
            s.preferences_error = None;
            s.preferences_feedback = Some(format!("Removed machine '{}'.", removed.name));
            removed_ok = true;
        }
        Err(err) => {
            s.preferences_error = Some(err.to_string());
        }
    });
    removed_ok
}

pub fn available_machine_project_names() -> Vec<String> {
    state().with(|s| {
        let mut names: Vec<String> = s
            .factory
            .projects()
            .iter()
            .map(|p| p.name.clone())
            .collect();
        if s.demo_mode {
            for fallback in ["api-gateway", "marketing-site", "mobile-app"] {
                if !names.iter().any(|n| n == fallback) {
                    names.push(fallback.to_owned());
                }
            }
        }
        names
    })
}

pub fn toggle_allow_project_mcp_servers() {
    state().update(|s| {
        s.preferences.extensions.allow_project_mcp_servers =
            !s.preferences.extensions.allow_project_mcp_servers;
        s.persist_preferences();
    });
}

// --- MCP servers ------------------------------------------------------------------------

/// Whose servers an MCP list edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerScope {
    /// This user's servers for the selected project.
    Project,
    /// This user's servers for every project.
    User,
}

/// What Project Settings › MCP Servers shows for the selected project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMcpOverview {
    pub state: factory_config::mcp::ProjectMcpState,
    pub allow_project_servers: bool,
    pub approved: bool,
    /// A file was approved, but its contents changed since.
    pub changed_since_approval: bool,
    pub project_extras: Vec<factory_config::mcp::McpServerDef>,
    pub user_extras: Vec<factory_config::mcp::McpServerDef>,
}

pub fn project_mcp_overview() -> ProjectMcpOverview {
    state().with(|s| {
        let name = s.selected_project_name();
        let state = s
            .project_root_by_name(&name)
            .map(|root| factory_config::mcp::load_project_mcp(&root))
            .unwrap_or_default();
        let settings = s.project_settings.get(&name);
        let sources =
            factory_config::mcp::McpSources::gather(&state, settings, &s.preferences.extensions);
        let approved = sources.project_trusted();
        ProjectMcpOverview {
            changed_since_approval: !approved
                && sources.approved_fingerprint.is_some()
                && state.config().is_some(),
            state,
            allow_project_servers: s.preferences.extensions.allow_project_mcp_servers,
            approved,
            project_extras: sources.project_extras,
            user_extras: sources.user_extras,
        }
    })
}

pub fn user_mcp_servers() -> Vec<factory_config::mcp::McpServerDef> {
    state().with(|s| s.preferences.extensions.mcp_servers.clone())
}

/// Approves the project's current MCP file, so its servers load until it changes.
pub fn approve_project_mcp_servers() {
    let mut cleared = Vec::new();
    state().update(|s| {
        let name = s.selected_project_name();
        let Some(root) = s.project_root_by_name(&name) else {
            return;
        };
        let Some(config) = factory_config::mcp::load_project_mcp(&root)
            .config()
            .cloned()
        else {
            s.project_settings_error =
                Some("This project has no MCP servers to approve.".to_owned());
            return;
        };
        let settings = s.ensure_project_settings_mut();
        settings.approved_mcp_fingerprint = Some(config.fingerprint.clone());
        s.project_settings_error = None;
        s.project_settings_feedback = Some(format!(
            "Approved the MCP servers in {}. They load until the file changes.",
            config.file.relative_path()
        ));
        s.persist_active_project_settings();
        cleared = s.inbox.clear_mcp_problems(&name, |p| {
            p.fix == factory_config::mcp::McpFix::ApproveProjectServers
        });
        s.sync_inbox_selection_and_drafts();
    });
    close_notifications(cleared);
}

pub fn revoke_project_mcp_servers() {
    state().update(|s| {
        s.ensure_project_settings_mut().approved_mcp_fingerprint = None;
        s.project_settings_error = None;
        s.project_settings_feedback =
            Some("Project MCP servers won't load until you approve them again.".to_owned());
        s.persist_active_project_settings();
    });
}

/// Adds one of this user's servers from the add-server form, optionally moving marked inline
/// values into Keychain as `McpOnly` secrets and rewriting them to `${VAR}` (or `Bearer ${VAR}`).
pub fn add_mcp_server_with_secret_lines(
    scope: McpServerScope,
    name: &str,
    command_or_url: &str,
    args_text: &str,
    pairs_text: &str,
    secret_line_indices: &BTreeSet<usize>,
) -> bool {
    let mut added = false;
    state().update(|s| {
        let mut rewritten_lines = Vec::new();
        let mut extracted_count = 0usize;
        let is_url = command_or_url.trim().starts_with("http://")
            || command_or_url.trim().starts_with("https://");
        let target_project = match scope {
            McpServerScope::Project => {
                let pname = s.selected_project_name();
                s.project_id_by_name(&pname).map(|pid| (pid, pname))
            }
            McpServerScope::User => None,
        };
        let mut existing_names: BTreeSet<String> = s
            .secrets
            .iter()
            .filter_map(|rec| match rec.kind {
                SecretKind::Env { .. } => Some(rec.name.to_ascii_uppercase()),
                SecretKind::Internal { .. } => None,
            })
            .collect();

        for (line_idx, raw_line) in pairs_text.lines().enumerate() {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() || !secret_line_indices.contains(&line_idx) {
                rewritten_lines.push(raw_line.to_owned());
                continue;
            }
            let sep = if is_url { ':' } else { '=' };
            let Some((raw_k, raw_v)) = trimmed.split_once(sep) else {
                rewritten_lines.push(raw_line.to_owned());
                continue;
            };
            let k = raw_k.trim();
            let v = raw_v.trim();
            if v.is_empty() || contains_env_reference(v) {
                rewritten_lines.push(raw_line.to_owned());
                continue;
            }
            let (was_bearer, secret_val) = if is_url && k.eq_ignore_ascii_case("authorization") {
                if let Some(rest) = v
                    .strip_prefix("Bearer ")
                    .or_else(|| v.strip_prefix("bearer "))
                {
                    (true, rest.trim())
                } else {
                    (false, v)
                }
            } else {
                (false, v)
            };
            if secret_val.is_empty() {
                rewritten_lines.push(raw_line.to_owned());
                continue;
            }
            let base = format!("{}_{}", sanitize_env_segment(name), sanitize_env_segment(k));
            let mut env_name = base.clone();
            let mut suffix = 2usize;
            while existing_names.contains(&env_name) {
                env_name = format!("{base}_{suffix}");
                suffix += 1;
            }
            existing_names.insert(env_name.clone());

            if !s.demo_mode
                && let Some(db_arc) = s.db.as_ref()
                && let Ok(db) = db_arc.lock()
            {
                let (env_scope, proj_name_opt) = match &target_project {
                    Some((pid, pname)) => (EnvScope::Project(*pid), Some(pname.as_str())),
                    None => (
                        EnvScope::App {
                            shared_with: Sharing::AllProjects,
                        },
                        None,
                    ),
                };
                let record = SecretRecord::new_keychain(
                    &env_name,
                    SecretKind::Env {
                        scope: env_scope,
                        exposure: Exposure::McpOnly,
                    },
                    current_unix_seconds(),
                );
                let secret = SecretString::from(secret_val.to_owned());
                if let Err(err) = db.create_keychain_secret(
                    s.secret_store.as_ref(),
                    record,
                    &secret,
                    proj_name_opt,
                ) {
                    let msg = format!("Could not store MCP secret {env_name} in Keychain: {err}");
                    match scope {
                        McpServerScope::Project => s.project_settings_error = Some(msg),
                        McpServerScope::User => s.preferences_error = Some(msg),
                    }
                    return;
                }
                extracted_count += 1;
            }
            let replacement = if was_bearer {
                format!("{k}: Bearer ${{{env_name}}}")
            } else if is_url {
                format!("{k}: ${{{env_name}}}")
            } else {
                format!("{k}=${{{env_name}}}")
            };
            rewritten_lines.push(replacement);
        }

        let final_pairs = rewritten_lines.join("\n");
        let parsed = factory_config::mcp::McpServerDef::from_form(
            name,
            command_or_url,
            args_text,
            &final_pairs,
        );
        match parsed {
            Err(err) => match scope {
                McpServerScope::Project => s.project_settings_error = Some(err),
                McpServerScope::User => s.preferences_error = Some(err),
            },
            Ok(server) => {
                let message = if extracted_count > 0 {
                    format!(
                        "Added MCP server '{}' ({extracted_count} secret(s) stored in Keychain).",
                        server.name
                    )
                } else {
                    format!("Added MCP server '{}'.", server.name)
                };
                match scope {
                    McpServerScope::Project => {
                        s.ensure_project_settings_mut().upsert_mcp_server(server);
                        s.project_settings_error = None;
                        s.project_settings_feedback = Some(message);
                        s.persist_active_project_settings();
                    }
                    McpServerScope::User => {
                        let servers = &mut s.preferences.extensions.mcp_servers;
                        match servers.iter_mut().find(|x| x.name == server.name) {
                            Some(existing) => *existing = server,
                            None => servers.push(server),
                        }
                        s.preferences_error = None;
                        s.preferences_feedback = Some(message);
                        s.persist_preferences();
                    }
                }
                s.refresh_secrets_state();
                added = true;
            }
        }
    });
    added
}

pub fn remove_mcp_server(scope: McpServerScope, name: &str) {
    state().update(|s| match scope {
        McpServerScope::Project => {
            if s.ensure_project_settings_mut().remove_mcp_server(name) {
                s.project_settings_feedback = Some(format!("Removed MCP server '{name}'."));
                s.persist_active_project_settings();
            }
        }
        McpServerScope::User => {
            s.preferences
                .extensions
                .mcp_servers
                .retain(|x| x.name != name);
            s.preferences_feedback = Some(format!("Removed MCP server '{name}'."));
            s.persist_preferences();
        }
    });
}

pub fn toggle_mcp_server(scope: McpServerScope, name: &str) {
    state().update(|s| match scope {
        McpServerScope::Project => {
            if let Some(server) = s
                .ensure_project_settings_mut()
                .mcp_servers
                .iter_mut()
                .find(|x| x.name == name)
            {
                server.disabled = !server.disabled;
                s.persist_active_project_settings();
            }
        }
        McpServerScope::User => {
            if let Some(server) = s
                .preferences
                .extensions
                .mcp_servers
                .iter_mut()
                .find(|x| x.name == name)
            {
                server.disabled = !server.disabled;
                s.persist_preferences();
            }
        }
    });
}

/// Takes the user to where `fix` gets done (for notifications, banners and the Inbox).
pub fn apply_mcp_fix(project_name: Option<&str>, fix: &factory_config::mcp::McpFix) {
    use factory_config::mcp::{McpFix, McpOrigin};
    if let Some(name) = project_name {
        state().update(|s| {
            if let Some(project) = s.factory.projects().iter().find(|p| p.name == name) {
                s.selected_project = Some(project.id);
            }
        });
    }
    match fix {
        McpFix::SetEnvVar(key) => {
            open_project_settings_tab(ProjectSettingsTab::Environment);
            state().update(|s| {
                s.project_settings_feedback = Some(format!(
                    "Add {key} below (Add Variable) so MCP servers that need it can start."
                ));
            });
        }
        McpFix::ApproveProjectServers
        | McpFix::EditProjectFile(_)
        | McpFix::InstallCommand(_)
        | McpFix::EditExtra {
            origin: McpOrigin::ProjectExtra | McpOrigin::Project(_),
            ..
        } => open_project_settings_tab(ProjectSettingsTab::McpServers),
        McpFix::AllowProjectServers
        | McpFix::EditExtra {
            origin: McpOrigin::UserExtra,
            ..
        } => open_preferences_tab(PreferencesTab::Extensions),
        McpFix::ChangeProvider | McpFix::None => {}
    }
}

/// Where an MCP problem happened, for its Inbox item.
struct McpProblemContext<'a> {
    project_name: &'a str,
    /// The workflow or chat it happened in.
    context_label: String,
    /// The step's title, or the chat's.
    where_label: String,
    run_number: u32,
    provider: ProviderKind,
}

impl AppState {
    /// Files an MCP problem in the Inbox (once per problem and project) and returns the
    /// notification to post for it, if it's new.
    fn file_mcp_problem(
        &mut self,
        ctx: McpProblemContext<'_>,
        problem: &factory_config::mcp::McpProblem,
    ) -> Option<nsh_core::notifications::Notification> {
        let id = format!("mcp:{}:{}", ctx.project_name, mcp_problem_id(problem));
        if self.inbox.pending.iter().any(|r| r.id == id) {
            return None;
        }
        let title = problem.title();
        let stopped = if problem.blocking {
            format!("{} stopped: it needs this server.", ctx.where_label)
        } else {
            format!("{} ran without it.", ctx.where_label)
        };
        let agent = match ctx.provider {
            ProviderKind::Codex => AgentBadge::Codex,
            _ => AgentBadge::Claude,
        };
        let req = AttentionRequest {
            id: id.clone(),
            kind: AttentionRequestKind::Setup,
            agent,
            project_name: ctx.project_name.to_owned(),
            workflow_name: ctx.context_label.clone(),
            context_label: ctx.context_label.clone(),
            run_number: ctx.run_number,
            machine_name: "This Mac".to_owned(),
            inbox_title: title.clone(),
            inbox_subtitle: stopped.clone(),
            menubar_title: format!("{} · MCP server needs attention", ctx.project_name),
            menubar_subtitle: title.clone(),
            waiting_minutes: 0,
            past_reminder_threshold: false,
            status_pill: if problem.blocking {
                "Stopped work".to_owned()
            } else {
                "Needs setup".to_owned()
            },
            why_text: format!("{title}\n\n{stopped}"),
            deadline_badge: None,
            timeout_footer: "Clears by itself once the server starts.".to_owned(),
            note_placeholder: String::new(),
            body: AttentionRequestBody::McpProblem(factory_config::inbox::McpProblemDetail {
                problem: problem.clone(),
                stopped_work: problem.blocking,
            }),
        };
        self.inbox.pending.insert(0, req);
        if self.inbox_selected_id.is_none() {
            self.inbox_selected_id = self.inbox.default_selected_id();
        }
        self.sync_inbox_selection_and_drafts();
        if self.inbox.notifications_snoozed_minutes.is_some() {
            return None;
        }
        Some(nsh_core::notifications::Notification::new(
            id,
            format!("{}: MCP server needs attention", ctx.project_name),
            format!("{title} {stopped}"),
        ))
    }

    /// Files a secret problem in the Inbox (deduplicated per problem & project) and returns a
    /// notification to post when it is new.
    fn file_secret_problem(
        &mut self,
        ctx: McpProblemContext<'_>,
        problem: &SecretProblem,
    ) -> Option<nsh_core::notifications::Notification> {
        let digest =
            factory_config::mcp::fingerprint(problem.dedupe_key().as_bytes())[..16].to_owned();
        let id = format!("secret:{}:{digest}", ctx.project_name);
        if self.inbox.pending.iter().any(|r| r.id == id) {
            return None;
        }
        let title = problem.title();
        let subtitle = if problem.blocking {
            format!("{} stopped until this secret is fixed.", ctx.where_label)
        } else {
            format!("{} reported a secret setup notice.", ctx.where_label)
        };
        let agent = match ctx.provider {
            ProviderKind::Codex => AgentBadge::Codex,
            _ => AgentBadge::Claude,
        };
        let req = AttentionRequest {
            id: id.clone(),
            kind: AttentionRequestKind::Setup,
            agent,
            project_name: ctx.project_name.to_owned(),
            workflow_name: ctx.context_label.clone(),
            context_label: ctx.context_label.clone(),
            run_number: ctx.run_number,
            machine_name: "This Mac".to_owned(),
            inbox_title: title.clone(),
            inbox_subtitle: subtitle.clone(),
            menubar_title: format!("{} · Secret needs attention", ctx.project_name),
            menubar_subtitle: title.clone(),
            waiting_minutes: 0,
            past_reminder_threshold: false,
            status_pill: if problem.blocking {
                "Stopped work".to_owned()
            } else {
                "Needs setup".to_owned()
            },
            why_text: format!("{title}\n\n{subtitle}"),
            deadline_badge: None,
            timeout_footer: "Clears once the secret is fixed or dismissed.".to_owned(),
            note_placeholder: String::new(),
            body: AttentionRequestBody::SecretProblem(SecretProblemDetail {
                problem: problem.clone(),
                stopped_work: problem.blocking,
            }),
        };
        self.inbox.pending.insert(0, req);
        if self.inbox_selected_id.is_none() {
            self.inbox_selected_id = self.inbox.default_selected_id();
        }
        self.sync_inbox_selection_and_drafts();
        if self.inbox.notifications_snoozed_minutes.is_some() {
            return None;
        }
        Some(nsh_core::notifications::Notification::new(
            id,
            format!("{}: Secret needs attention", ctx.project_name),
            format!("{title} {subtitle}"),
        ))
    }

    /// Clears Inbox items about servers that have since reached an agent.
    fn clear_ready_mcp_servers(
        &mut self,
        project_name: &str,
        servers: &[(String, factory_config::mcp::McpOrigin)],
    ) -> Vec<String> {
        let cleared = self.inbox.clear_mcp_problems(project_name, |p| {
            servers.iter().any(|(name, origin)| {
                p.server.as_deref() == Some(name) && p.origin == Some(*origin)
            })
        });
        if !cleared.is_empty() {
            self.sync_inbox_selection_and_drafts();
        }
        cleared
    }
}

fn mcp_problem_id(problem: &factory_config::mcp::McpProblem) -> String {
    factory_config::mcp::fingerprint(problem.dedupe_key().as_bytes())[..16].to_owned()
}

/// Shows MCP problem notifications, asking for permission the first time.
fn post_mcp_notifications(notifications: Vec<nsh_core::notifications::Notification>) {
    if notifications.is_empty() {
        return;
    }
    Owner::detached().run(|| {
        spawn(async move {
            let _ = nsh_core::element::request_notification_permission().await;
            for notification in notifications {
                let _ = nsh_core::element::show_notification(notification).await;
            }
        });
    });
}

fn close_notifications(ids: Vec<String>) {
    for id in ids {
        nsh_core::element::close_notification(id);
    }
}

/// The Inbox's selected Setup item: its fix.
pub fn inbox_fix_setup_item() {
    let Some(req) = inbox_selected_pending() else {
        return;
    };
    match &req.body {
        AttentionRequestBody::McpProblem(detail) => {
            if detail.problem.fix.button_label().is_none() {
                inbox_dismiss_setup_item();
                return;
            }
            apply_mcp_fix(Some(&req.project_name), &detail.problem.fix);
        }
        AttentionRequestBody::SecretProblem(detail) => {
            if detail.problem.fix.button_label().is_none() {
                inbox_dismiss_setup_item();
                return;
            }
            apply_secret_fix(Some(&req.project_name), &detail.problem.fix);
        }
        _ => {}
    }
}

pub fn inbox_dismiss_setup_item() {
    let mut closed = None;
    state().update(|s| {
        let Some(id) = s.inbox_selected_id.clone() else {
            return;
        };
        if let Some(answered) = s.inbox.dismiss_setup_item(&id) {
            closed = Some(answered.id);
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed {
        nsh_core::element::close_notification(id);
    }
}

pub fn toggle_extension(id: &str) {
    state().update(|s| {
        s.preferences.toggle_extension(id);
        s.persist_preferences();
    });
}

pub fn toggle_verbose_step_logs() {
    state().update(|s| {
        s.preferences.advanced.verbose_step_logs = !s.preferences.advanced.verbose_step_logs;
        s.persist_preferences();
    });
}

pub fn toggle_keep_worktrees_on_failure() {
    state().update(|s| {
        s.preferences.advanced.keep_worktrees_on_failure =
            !s.preferences.advanced.keep_worktrees_on_failure;
        s.persist_preferences();
    });
}

/// Sets the SSH agent socket handed to agents. Empty clears it so agents inherit the detected one.
pub fn set_ssh_auth_sock(path: &str) {
    state().update(|s| {
        s.preferences.advanced.ssh_auth_sock = path.trim().to_owned();
        s.persist_preferences();
    });
}

/// The SSH agent socket Bruk inherited from its environment, if any.
pub fn detected_ssh_auth_sock() -> Option<String> {
    factory_config::detected_ssh_auth_sock()
}

pub fn step_log_retention_days(delta: i32) {
    state().update(|s| {
        let next = (s.preferences.advanced.log_retention_days as i32 + delta).clamp(7, 365) as u32;
        s.preferences.advanced.log_retention_days = next;
        s.persist_preferences();
    });
}

pub fn reset_preferences_to_defaults() {
    state().update(|s| {
        if s.demo_mode {
            let mut sample = AppPreferences::sample();
            sample.demo_mode = true;
            s.preferences = sample;
            s.saved_real_preferences = Some(AppPreferences::empty());
        } else {
            s.preferences = AppPreferences::empty();
            s.refresh_claude_status(false);
        }
        s.selected_provider_id = default_selected_provider_id(&s.preferences);
        s.preferences_error = None;
        s.preferences_feedback = Some("Restored default preferences.".to_owned());
        s.persist_preferences();
    });
    apply_appearance_settings(&AppearanceSettings::default());
    refresh_claude_details_async();
    refresh_codex_details_async();
    refresh_antigravity_details_async();
}

// --- Project Settings -------------------------------------------------------------------

pub fn open_project_settings() {
    state().update(|s| {
        if s.selected_project.is_none() {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
        }
        if let Some(pid) = s.selected_project {
            s.expanded_projects.insert(pid);
        }
        s.active_view = ActiveView::ProjectSettings;
        s.project_settings_error = None;
        s.project_settings_feedback = None;
        s.status_message = None;
    });
    main_window().focus();
}

pub fn open_project_settings_tab(tab: ProjectSettingsTab) {
    state().update(|s| {
        if s.selected_project.is_none() {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
        }
        if let Some(pid) = s.selected_project {
            s.expanded_projects.insert(pid);
        }
        s.project_settings_tab = tab;
        s.active_view = ActiveView::ProjectSettings;
        s.project_settings_error = None;
        s.project_settings_feedback = None;
        s.status_message = None;
    });
    main_window().focus();
}

pub fn project_settings_tab() -> ProjectSettingsTab {
    state().with(|s| s.project_settings_tab)
}

pub fn set_project_settings_tab(tab: ProjectSettingsTab) {
    state().update(|s| {
        s.project_settings_tab = tab;
        s.project_settings_error = None;
        s.project_settings_feedback = None;
    });
}

pub fn active_project_settings() -> ProjectSettings {
    state().with(AppState::active_project_settings_clone)
}

pub fn project_settings_error() -> Option<String> {
    state().with(|s| s.project_settings_error.clone())
}

pub fn project_settings_feedback() -> Option<String> {
    state().with(|s| s.project_settings_feedback.clone())
}

pub fn clear_project_settings_messages() {
    state().update(|s| {
        s.project_settings_error = None;
        s.project_settings_feedback = None;
    });
}

pub fn set_project_issue_source(source: IssueSourceKind) {
    state().update(|s| {
        let result = s.ensure_project_settings_mut().set_issue_source(source);
        match result {
            Ok(()) => {
                s.project_settings_error = None;
                s.project_settings_feedback = None;
                s.sync_queue_from_project_settings();
                s.persist_active_project_settings();
                s.persist_active_work_queue();
            }
            Err(err) => {
                s.project_settings_error = Some(err.to_string());
            }
        }
    });
}

pub fn set_project_github_repo(repo: &str) {
    let repo = repo.to_owned();
    state().update(|s| {
        let demo = s.demo_mode;
        let settings = s.ensure_project_settings_mut();
        if settings.github_repo != repo {
            settings.github_repo = repo;
            if !demo {
                settings.source_connected = false;
                settings.source_error = None;
            }
        }
        s.project_settings_error = None;
        s.sync_queue_from_project_settings();
        s.persist_active_project_settings();
        s.persist_active_work_queue();
    });
}

pub fn set_project_fallback_workflow(workflow: &str) {
    let workflow = workflow.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut()
            .set_fallback_workflow(&workflow);
        s.project_settings_error = None;
        s.sync_queue_from_project_settings();
        s.persist_active_project_settings();
        s.persist_active_work_queue();
    });
}

pub fn set_project_itsaplan_workspace(workspace: &str) {
    let workspace = workspace.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().itsaplan_workspace = workspace;
        s.project_settings_error = None;
        s.persist_active_project_settings();
    });
}

pub fn set_project_itsaplan_agent_member(member: &str) {
    let member = member.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().itsaplan_agent_member = member;
        s.project_settings_error = None;
        s.persist_active_project_settings();
    });
}

pub fn set_project_github_token(raw_token: &str) -> bool {
    let mut ok = false;
    state().update(|s| {
        let demo = s.demo_mode;
        let project_name = s.selected_project_name();
        match s.ensure_project_settings_mut().set_github_token(raw_token) {
            Ok(()) => {
                if !demo {
                    let saved = s.db.as_ref().and_then(|db_arc| {
                        db_arc.lock().ok().map(|db| {
                            let existing_pat_id = s
                                .project_settings
                                .get(&project_name)
                                .and_then(|ps| ps.github_pat_id.clone());
                            let secret = SecretString::from(raw_token.trim().to_owned());
                            if let Some(pat_id) = existing_pat_id {
                                db.replace_keychain_secret(
                                    s.secret_store.as_ref(),
                                    &pat_id,
                                    &secret,
                                    None,
                                )
                            } else {
                                let base_name = if project_name.trim().is_empty() {
                                    "GitHub PAT".to_owned()
                                } else {
                                    format!("{project_name} GitHub PAT")
                                };
                                let existing_names: BTreeSet<String> = s
                                    .secrets
                                    .iter()
                                    .filter(|r| {
                                        matches!(
                                            r.kind,
                                            SecretKind::Internal {
                                                purpose: InternalPurpose::GitHubPat
                                            }
                                        )
                                    })
                                    .map(|r| r.name.to_ascii_lowercase())
                                    .collect();
                                let mut name = base_name.clone();
                                let mut n = 2usize;
                                while existing_names.contains(&name.to_ascii_lowercase()) {
                                    name = format!("{base_name} ({n})");
                                    n += 1;
                                }
                                let record = SecretRecord::new_keychain(
                                    name,
                                    SecretKind::Internal {
                                        purpose: InternalPurpose::GitHubPat,
                                    },
                                    current_unix_seconds(),
                                );
                                db.create_keychain_secret(
                                    s.secret_store.as_ref(),
                                    record,
                                    &secret,
                                    None,
                                )
                            }
                        })
                    });
                    if let Some(res) = saved {
                        match res {
                            Ok(rec) => {
                                let settings = s.ensure_project_settings_mut();
                                settings.github_pat_id = Some(rec.id);
                                settings.source_connected = false;
                                settings.source_error = None;
                            }
                            Err(err) => {
                                s.project_settings_error =
                                    Some(format!("Could not save GitHub token in Keychain: {err}"));
                                return;
                            }
                        }
                    }
                }
                s.project_settings_error = None;
                s.project_settings_feedback =
                    Some("Saved personal access token in Keychain for this project.".to_owned());
                s.persist_active_project_settings();
                s.refresh_secrets_state();
                ok = true;
            }
            Err(err) => {
                s.project_settings_error = Some(err.to_string());
            }
        }
    });
    ok
}

/// Selects a reusable named App-level GitHub PAT for the active project.
pub fn select_project_github_pat(secret_id: &str) {
    state().update(|s| {
        let last4 = s
            .secrets
            .iter()
            .find(|r| r.id == secret_id)
            .and_then(|r| r.preview.as_deref())
            .map(|p| p.trim_start_matches('…').to_owned());
        let settings = s.ensure_project_settings_mut();
        settings.github_auth_mode = IssueSourceAuthMode::PersonalToken;
        settings.github_pat_id = Some(secret_id.to_owned());
        settings.github_token_last4 = last4;
        settings.source_connected = false;
        settings.source_error = None;
        s.project_settings_error = None;
        s.project_settings_feedback = Some("Selected GitHub personal access token.".to_owned());
        s.persist_active_project_settings();
        s.refresh_secrets_state();
    });
}

pub fn use_project_github_cli() {
    state().update(|s| {
        let demo = s.demo_mode;
        let settings = s.ensure_project_settings_mut();
        settings.use_github_cli();
        if !demo {
            settings.source_connected = false;
            settings.source_error = None;
        }
        s.project_settings_error = None;
        s.project_settings_feedback = Some("Using GitHub CLI (gh) sign-in.".to_owned());
        s.persist_active_project_settings();
    });
}

pub fn add_project_intake_rule(label: &str, workflow_name: &str) -> bool {
    let mut added = false;
    state().update(|s| {
        match s
            .ensure_project_settings_mut()
            .add_intake_rule(label, workflow_name)
        {
            Ok(_) => {
                s.project_settings_error = None;
                s.project_settings_feedback = Some(format!(
                    "Added rule: has label {} → {}",
                    label.trim(),
                    workflow_name.trim()
                ));
                s.sync_queue_from_project_settings();
                s.persist_active_project_settings();
                s.persist_active_work_queue();
                added = true;
            }
            Err(err) => {
                s.project_settings_error = Some(err.to_string());
            }
        }
    });
    added
}

pub fn add_project_intake_filter(clause: GitHubFilterClause) -> bool {
    let mut added = false;
    state().update(|s| {
        let token = clause.to_github_query_token();
        match s.ensure_project_settings_mut().add_intake_filter(clause) {
            Ok(()) => {
                s.project_settings_error = None;
                s.project_settings_feedback = Some(format!("Added search filter: {token}"));
                s.sync_queue_from_project_settings();
                s.persist_active_project_settings();
                s.persist_active_work_queue();
                added = true;
            }
            Err(err) => {
                s.project_settings_error = Some(err.to_string());
            }
        }
    });
    added
}

pub fn remove_project_intake_filter(index: usize) {
    state().update(|s| {
        if s.ensure_project_settings_mut().remove_intake_filter(index) {
            s.project_settings_error = None;
            s.project_settings_feedback = Some("Removed search filter.".to_owned());
            s.sync_queue_from_project_settings();
            s.persist_active_project_settings();
            s.persist_active_work_queue();
        }
    });
}

pub fn add_project_intake_rule_with_clauses(
    clauses: Vec<GitHubFilterClause>,
    workflow_name: &str,
) -> bool {
    let mut added = false;
    state().update(|s| {
        let condition = clauses
            .iter()
            .filter(|c| !c.is_empty())
            .map(GitHubFilterClause::human_summary)
            .collect::<Vec<_>>()
            .join(" and ");
        match s
            .ensure_project_settings_mut()
            .add_intake_rule_with_clauses(clauses, workflow_name)
        {
            Ok(_) => {
                s.project_settings_error = None;
                s.project_settings_feedback = Some(format!(
                    "Added rule: {condition} → {}",
                    workflow_name.trim()
                ));
                s.sync_queue_from_project_settings();
                s.persist_active_project_settings();
                s.persist_active_work_queue();
                added = true;
            }
            Err(err) => {
                s.project_settings_error = Some(err.to_string());
            }
        }
    });
    added
}

pub fn remove_project_intake_rule(rule_id: u64) {
    state().update(|s| {
        if s.ensure_project_settings_mut().remove_intake_rule(rule_id) {
            s.project_settings_error = None;
            s.project_settings_feedback = Some("Removed intake rule.".to_owned());
            s.sync_queue_from_project_settings();
            s.persist_active_project_settings();
            s.persist_active_work_queue();
        }
    });
}

pub fn set_project_max_concurrent_runs_str(value: &str) {
    state().update(|s| {
        match s
            .ensure_project_settings_mut()
            .set_max_concurrent_runs_str(value)
        {
            Ok(()) => {
                s.project_settings_error = None;
                s.sync_queue_from_project_settings();
                s.persist_active_project_settings();
                s.persist_active_work_queue();
            }
            Err(err) => {
                s.project_settings_error = Some(err.to_string());
            }
        }
    });
}

pub fn step_project_max_concurrent_runs(delta: i32) {
    state().update(|s| {
        s.ensure_project_settings_mut()
            .step_max_concurrent_runs(delta);
        s.project_settings_error = None;
        s.sync_queue_from_project_settings();
        s.persist_active_project_settings();
        s.persist_active_work_queue();
    });
}

pub fn toggle_project_auto_start() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.auto_start_matching_work = !settings.auto_start_matching_work;
        s.persist_active_project_settings();
    });
}

pub fn toggle_project_add_working_label() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.add_working_label = !settings.add_working_label;
        s.persist_active_project_settings();
    });
}

pub fn toggle_project_comment_summary() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.comment_with_run_summary = !settings.comment_with_run_summary;
        s.persist_active_project_settings();
    });
}

pub fn toggle_project_link_pr_closes_issue() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.link_pr_closes_issue = !settings.link_pr_closes_issue;
        s.persist_active_project_settings();
    });
}

pub fn set_project_default_branch(branch: &str) {
    let branch = branch.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().default_branch = branch;
        s.persist_active_project_settings();
    });
}

pub fn set_project_branch_prefix(prefix: &str) {
    let prefix = prefix.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().branch_prefix = prefix;
        s.persist_active_project_settings();
    });
}

pub fn toggle_project_isolated_worktrees() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.use_isolated_worktrees = !settings.use_isolated_worktrees;
        s.persist_active_project_settings();
    });
}

pub fn toggle_project_draft_prs() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.open_prs_as_draft = !settings.open_prs_as_draft;
        s.persist_active_project_settings();
    });
}

pub fn toggle_project_auto_cleanup_worktrees() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.auto_cleanup_worktrees = !settings.auto_cleanup_worktrees;
        s.persist_active_project_settings();
    });
}

pub fn toggle_project_archived() {
    state().update(|s| {
        let archived = {
            let settings = s.ensure_project_settings_mut();
            settings.archived = !settings.archived;
            settings.archived
        };
        s.project_settings_feedback = Some(if archived {
            "Project archived. Automatic intake is paused.".to_owned()
        } else {
            "Project restored from archive.".to_owned()
        });
        s.persist_active_project_settings();
    });
}

pub fn set_project_default_provider(provider: ProviderKind) {
    state().update(|s| {
        s.ensure_project_settings_mut().default_provider = provider;
        s.persist_active_project_settings();
    });
}

pub fn set_project_openrouter_key(key_id: Option<String>) {
    state().update(|s| {
        s.ensure_project_settings_mut().openrouter_key_id = key_id;
        s.persist_active_project_settings();
    });
}

pub fn set_project_model_override(model: &str) {
    let model = model.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().model_override = model;
        s.persist_active_project_settings();
    });
}

pub fn toggle_project_workflow(name: &str) {
    let name = name.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().toggle_workflow(&name);
        s.persist_active_project_settings();
    });
}

pub fn toggle_project_require_review_before_pr() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.require_review_before_pr = !settings.require_review_before_pr;
        s.persist_active_project_settings();
    });
}

pub fn toggle_project_auto_reload_workflows() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.auto_reload_workflows = !settings.auto_reload_workflows;
        s.persist_active_project_settings();
    });
}

pub fn add_project_env_var(key: &str, value: &str, is_secret: bool) -> bool {
    if is_secret {
        return create_project_keychain_secret(key, value, Exposure::McpOnly);
    }
    let mut added = false;
    state().update(|s| {
        match s
            .ensure_project_settings_mut()
            .add_env_var(key, value, false)
        {
            Ok(_) => {
                s.project_settings_error = None;
                s.project_settings_feedback =
                    Some(format!("Added environment variable {}.", key.trim()));
                s.persist_active_project_settings();
                added = true;
            }
            Err(err) => {
                s.project_settings_error = Some(err.to_string());
            }
        }
    });
    added
}

pub fn remove_project_env_var(id: u64) {
    state().update(|s| {
        if s.ensure_project_settings_mut().remove_env_var(id) {
            s.project_settings_error = None;
            s.project_settings_feedback = Some("Removed environment variable.".to_owned());
            s.persist_active_project_settings();
        }
    });
}

pub fn toggle_project_load_dotenv() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.load_dotenv_local = !settings.load_dotenv_local;
        s.persist_active_project_settings();
    });
}

pub fn toggle_project_inherit_host_path() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.inherit_host_path = !settings.inherit_host_path;
        s.persist_active_project_settings();
    });
}

pub fn step_project_max_loop_attempts(delta: i32) {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.max_loop_attempts =
            (settings.max_loop_attempts as i32 + delta).clamp(1, 20) as u32;
        s.persist_active_project_settings();
    });
}

pub fn step_project_step_timeout(delta: i32) {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.step_timeout_minutes =
            (settings.step_timeout_minutes as i32 + delta).clamp(5, 120) as u32;
        s.persist_active_project_settings();
    });
}

pub fn step_project_per_run_budget(delta_cents: i64) {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        let next = (settings.per_run_budget_cents as i64 + delta_cents).clamp(100, 10_000);
        settings.per_run_budget_cents = next as u64;
        s.persist_active_project_settings();
    });
}

pub fn step_project_monthly_budget(delta_cents: i64) {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        let next = (settings.monthly_budget_cents as i64 + delta_cents).clamp(1_000, 500_000);
        settings.monthly_budget_cents = next as u64;
        s.persist_active_project_settings();
    });
}

pub fn toggle_project_pause_on_budget() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.pause_on_budget_limit = !settings.pause_on_budget_limit;
        s.persist_active_project_settings();
    });
}

pub fn toggle_project_require_approval_for_ci() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.require_approval_for_ci = !settings.require_approval_for_ci;
        s.persist_active_project_settings();
    });
}

pub fn set_project_sandbox_mode(mode: ProjectSandboxMode) {
    state().update(|s| {
        s.ensure_project_settings_mut().sandbox_mode = mode;
        s.persist_active_project_settings();
    });
}

pub fn set_project_preferred_runner(runner: &str) {
    let runner = runner.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().preferred_runner = runner;
        s.persist_active_project_settings();
    });
}

fn apply_project_settings_to_queue(settings: &ProjectSettings, queue: &mut ProjectWorkQueue) {
    queue.max_agent_slots = settings.max_concurrent_runs;
    queue.repo_slug = settings.github_repo.trim().to_owned();
    queue.tracker_name = settings.issue_source.title().to_owned();
    let rules: Vec<IntakeRule> = settings
        .intake_rules
        .iter()
        .filter(|rule| !rule.is_fallback())
        .map(|rule| IntakeRule::with_clauses(rule.effective_clauses(), rule.workflow_name.clone()))
        .collect();
    let fallback = settings.fallback_workflow_name();
    queue.update_intake_rules(settings.intake_filters.clone(), rules, fallback);
}

// --- Workflows Overview state -----------------------------------------------------------

pub fn open_workflows_for_project(id: ProjectId) {
    state().update(|s| {
        let project_changed = s.selected_project != Some(id);
        s.selected_project = Some(id);
        s.expanded_projects.insert(id);
        s.active_view = ActiveView::Workflows;
        if project_changed {
            s.workflows_selected_id = None;
            s.workflows_search.clear();
            s.workflows_filter = WorkflowsFilter::All;
            s.workflows_new_drawer_open = false;
            s.workflows_import_drawer_open = false;
            s.workflows_editing_selected = false;
            s.workflows_ad_hoc_open = false;
            s.workflows_more_menu_open = false;
        }
        s.sync_selected_workflow_overview_for_active_project();
        s.status_message = None;
    });
}

pub fn open_new_workflow_drawer() {
    state().update(|s| {
        if s.selected_project.is_none() {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
        }
        if let Some(pid) = s.selected_project {
            s.expanded_projects.insert(pid);
        }
        s.active_view = ActiveView::Workflows;
        s.workflows_new_drawer_open = true;
        s.workflows_import_drawer_open = false;
        s.sync_selected_workflow_overview_for_active_project();
    });
}

pub fn open_import_workflow_drawer() {
    state().update(|s| {
        if s.selected_project.is_none() {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
        }
        if let Some(pid) = s.selected_project {
            s.expanded_projects.insert(pid);
        }
        s.active_view = ActiveView::Workflows;
        s.workflows_import_drawer_open = true;
        s.workflows_new_drawer_open = false;
        s.sync_selected_workflow_overview_for_active_project();
    });
}

pub fn active_workflows_overview() -> ProjectWorkflowsOverview {
    state().with(|s| {
        let name = s.selected_project_name();
        s.workflows_overview_for_name(&name)
    })
}

pub fn workflows_overview_for_project(id: ProjectId) -> ProjectWorkflowsOverview {
    state().with(|s| {
        let name = s
            .factory
            .project(id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        s.workflows_overview_for_name(&name)
    })
}

pub fn workflows_filter() -> WorkflowsFilter {
    state().with(|s| s.workflows_filter)
}

pub fn set_workflows_filter(filter: WorkflowsFilter) {
    state().update(|s| {
        s.workflows_filter = filter;
    });
}

pub fn workflows_sort() -> WorkflowsSort {
    state().with(|s| s.workflows_sort)
}

pub fn set_workflows_sort(sort: WorkflowsSort) {
    state().update(|s| {
        s.workflows_sort = sort;
    });
}

pub fn workflows_search() -> String {
    state().with(|s| s.workflows_search.clone())
}

pub fn set_workflows_search(query: &str) {
    let query = query.to_owned();
    state().update(|s| {
        s.workflows_search = query;
    });
}

pub fn reset_workflows_filters() {
    state().update(|s| {
        s.workflows_filter = WorkflowsFilter::All;
        s.workflows_search.clear();
    });
}

pub fn selected_workflow_overview_id() -> Option<String> {
    state().with(|s| s.workflows_selected_id.clone())
}

pub fn select_workflow_overview(id: &str) {
    let id = id.to_owned();
    state().update(|s| {
        s.workflows_selected_id = Some(id);
        s.workflows_inspector_visible = true;
        s.workflows_editing_selected = false;
        s.workflows_ad_hoc_open = false;
        s.workflows_more_menu_open = false;
    });
}

pub fn workflows_inspector_visible() -> bool {
    state().with(|s| s.workflows_inspector_visible)
}

pub fn toggle_workflows_inspector() {
    state().update(|s| {
        s.workflows_inspector_visible = !s.workflows_inspector_visible;
    });
}

pub fn workflows_new_drawer_open() -> bool {
    state().with(|s| s.workflows_new_drawer_open)
}

pub fn set_workflows_new_drawer_open(open: bool) {
    state().update(|s| {
        s.workflows_new_drawer_open = open;
        if open {
            s.workflows_import_drawer_open = false;
        }
    });
}

pub fn workflows_import_drawer_open() -> bool {
    state().with(|s| s.workflows_import_drawer_open)
}

pub fn set_workflows_import_drawer_open(open: bool) {
    state().update(|s| {
        s.workflows_import_drawer_open = open;
        if open {
            s.workflows_new_drawer_open = false;
        }
    });
}

pub fn workflows_editing_selected() -> bool {
    state().with(|s| s.workflows_editing_selected)
}

pub fn set_workflows_editing_selected(editing: bool) {
    state().update(|s| {
        s.workflows_editing_selected = editing;
        if editing {
            s.workflows_ad_hoc_open = false;
            s.workflows_more_menu_open = false;
        }
    });
}

pub fn workflows_ad_hoc_open() -> bool {
    state().with(|s| s.workflows_ad_hoc_open)
}

pub fn set_workflows_ad_hoc_open(open: bool) {
    state().update(|s| {
        s.workflows_ad_hoc_open = open;
        if open {
            s.workflows_editing_selected = false;
            s.workflows_more_menu_open = false;
        }
    });
}

pub fn workflows_more_menu_open() -> bool {
    state().with(|s| s.workflows_more_menu_open)
}

pub fn set_workflows_more_menu_open(open: bool) {
    state().update(|s| {
        s.workflows_more_menu_open = open;
        if open {
            s.workflows_editing_selected = false;
            s.workflows_ad_hoc_open = false;
        }
    });
}

pub fn reload_active_project_catalog() {
    state().update(|s| {
        if s.demo_mode {
            s.status_message = Some("Demo Mode uses built-in sample workflows".to_owned());
            return;
        }
        let project = s.selected_project_name();
        if project.is_empty() {
            return;
        }
        s.reload_catalog_for_project(&project);
        let wf_count = s
            .catalogs
            .get(&project)
            .map(|c| c.effective_workflows().len())
            .unwrap_or(0);
        s.status_message = Some(format!(
            "Reloaded .agents/bruk catalog ({wf_count} workflows)"
        ));
    });
}

pub fn active_catalog_agents() -> Vec<AgentDef> {
    state().with(|s| {
        s.active_catalog_clone()
            .effective_agents()
            .into_iter()
            .cloned()
            .collect()
    })
}

pub fn active_catalog_composites() -> Vec<CompositeDef> {
    state().with(|s| {
        s.active_catalog_clone()
            .effective_composites()
            .into_iter()
            .cloned()
            .collect()
    })
}

#[allow(dead_code)]
pub fn active_catalog_skills() -> Vec<SkillDef> {
    state().with(|s| {
        s.active_catalog_clone()
            .effective_skills()
            .into_iter()
            .cloned()
            .collect()
    })
}

pub fn create_overview_workflow(
    name: &str,
    description: &str,
    issue_label: Option<&str>,
    as_draft: bool,
) -> bool {
    let mut created = false;
    let name_owned = name.to_owned();
    let desc_owned = description.to_owned();
    let label_owned = issue_label.map(str::to_owned);
    state().update(|s| {
        let project = s.selected_project_name();
        if !s.demo_mode
            && let Some(project_root) = s.project_root_by_name(&project)
        {
            let trimmed_name = name_owned.trim();
            if trimmed_name.is_empty() {
                s.status_message = Some("Workflow name cannot be empty".to_owned());
                return;
            }
            let id = slugify_workflow_id(trimmed_name);
            let label = label_owned
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let wf = WorkflowDef {
                id: id.clone(),
                name: trimmed_name.to_owned(),
                description: desc_owned.trim().to_owned(),
                draft: as_draft,
                triggers: WorkflowTriggersDef {
                    work_queue: label.is_some(),
                    ad_hoc: true,
                    issue_labels: label.map(|l| vec![l.to_owned()]).unwrap_or_default(),
                },
                inputs: BTreeMap::new(),
                mcp_sources: Default::default(),
                nodes: vec![WorkflowNodeDef::AgentTask(AgentTaskStepDef {
                    id: "step_1".to_owned(),
                    name: "Run task".to_owned(),
                    agent: Some("implementer".to_owned()),
                    provider: Some(ProviderKind::Claude),
                    model: None,
                    skills: Vec::new(),
                    prompt: "Complete the requested task: {{ inputs.prompt }}".to_owned(),
                    inputs: BTreeMap::new(),
                    outputs: Vec::new(),
                    mcp: Vec::new(),
                })],
                origin: DefinitionOrigin::BuiltIn,
            };
            match BrukCatalog::save_project_workflow(&project_root, &wf) {
                Ok(_) => {
                    s.reload_catalog_for_project(&project);
                    s.workflows_selected_id = Some(id);
                    s.workflows_new_drawer_open = false;
                    s.workflows_filter = WorkflowsFilter::All;
                    s.workflows_search.clear();
                    s.status_message = Some(format!(
                        "Created workflow '{}' in .agents/bruk/workflows/{}.yaml",
                        wf.name, wf.id
                    ));
                    created = true;
                }
                Err(err) => {
                    s.status_message = Some(format!("Failed to write workflow file: {err}"));
                }
            }
            return;
        }
        let overview = s.ensure_workflows_overview_mut(&project);
        match overview.create_workflow(&name_owned, &desc_owned, label_owned.as_deref(), as_draft) {
            Ok(id) => {
                let display_name = overview
                    .workflow(&id)
                    .map(|w| w.name.clone())
                    .unwrap_or_else(|| name_owned.clone());
                s.workflows_selected_id = Some(id);
                s.workflows_new_drawer_open = false;
                s.workflows_filter = WorkflowsFilter::All;
                s.workflows_search.clear();
                s.status_message = Some(format!("Created workflow '{display_name}'"));
                created = true;
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
    created
}

pub fn create_workflow_from_template(template: WorkflowTemplateKind) {
    state().update(|s| {
        let project = s.selected_project_name();
        if !s.demo_mode
            && let Some(project_root) = s.project_root_by_name(&project)
        {
            let catalog = s.active_catalog_clone();
            let base_id = match template {
                WorkflowTemplateKind::Blank => "custom-workflow",
                WorkflowTemplateKind::FixAnIssue => "fix-issue",
                WorkflowTemplateKind::ReviewPullRequest => "review-pr",
            };
            let mut wf = catalog
                .resolve_workflow(&format!("builtin/{base_id}"))
                .cloned()
                .unwrap_or_else(|| WorkflowDef {
                    id: "custom-workflow".to_owned(),
                    name: "Custom workflow".to_owned(),
                    description: "Blank starter workflow".to_owned(),
                    draft: true,
                    triggers: WorkflowTriggersDef::default(),
                    inputs: BTreeMap::new(),
                    mcp_sources: Default::default(),
                    nodes: vec![WorkflowNodeDef::AgentTask(AgentTaskStepDef {
                        id: "step_1".to_owned(),
                        name: "Step 1".to_owned(),
                        agent: Some("planner".to_owned()),
                        provider: Some(ProviderKind::Claude),
                        model: None,
                        skills: Vec::new(),
                        prompt: "{{ inputs.prompt }}".to_owned(),
                        inputs: BTreeMap::new(),
                        outputs: Vec::new(),
                        mcp: Vec::new(),
                    })],
                    origin: DefinitionOrigin::BuiltIn,
                });
            let mut candidate = wf.id.clone();
            let mut suffix = 2u32;
            while catalog
                .resolve_workflow(&format!("project/{candidate}"))
                .is_some()
            {
                candidate = format!("{}-{suffix}", wf.id);
                suffix += 1;
            }
            if candidate != wf.id {
                wf.name = format!("{} ({})", wf.name, suffix - 1);
                wf.id = candidate.clone();
            }
            wf.draft = true;
            match BrukCatalog::save_project_workflow(&project_root, &wf) {
                Ok(_) => {
                    s.reload_catalog_for_project(&project);
                    s.workflows_selected_id = Some(wf.id.clone());
                    s.workflows_filter = WorkflowsFilter::All;
                    s.workflows_search.clear();
                    s.workflows_editing_selected = false;
                    s.workflows_ad_hoc_open = false;
                    s.workflows_more_menu_open = false;
                    s.status_message = Some(format!(
                        "Created '{}' in .agents/bruk/workflows/{}.yaml",
                        wf.name, wf.id
                    ));
                }
                Err(err) => {
                    s.status_message = Some(format!("Failed to write workflow file: {err}"));
                }
            }
            return;
        }
        let overview = s.ensure_workflows_overview_mut(&project);
        let id = overview.create_from_template(template);
        let display_name = overview
            .workflow(&id)
            .map(|w| w.name.clone())
            .unwrap_or_else(|| template.title().to_owned());
        s.workflows_selected_id = Some(id);
        s.workflows_filter = WorkflowsFilter::All;
        s.workflows_search.clear();
        s.workflows_editing_selected = false;
        s.workflows_ad_hoc_open = false;
        s.workflows_more_menu_open = false;
        s.status_message = Some(format!(
            "Created draft workflow '{display_name}' from template"
        ));
    });
}

pub fn import_overview_workflow(file_path: &str) -> bool {
    let mut imported = false;
    let path_owned = file_path.to_owned();
    state().update(|s| {
        let project = s.selected_project_name();
        let overview = s.ensure_workflows_overview_mut(&project);
        match overview.import_from_file(&path_owned) {
            Ok(id) => {
                let display_name = overview
                    .workflow(&id)
                    .map(|w| w.name.clone())
                    .unwrap_or_else(|| "Imported workflow".to_owned());
                s.workflows_selected_id = Some(id);
                s.workflows_import_drawer_open = false;
                s.workflows_filter = WorkflowsFilter::All;
                s.workflows_search.clear();
                s.status_message = Some(format!("Imported workflow '{display_name}'"));
                imported = true;
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
    imported
}

pub fn update_selected_overview_workflow(id: &str, name: &str, description: &str, is_draft: bool) {
    let id_owned = id.to_owned();
    let name_owned = name.to_owned();
    let desc_owned = description.to_owned();
    state().update(|s| {
        let project = s.selected_project_name();
        let overview = s.ensure_workflows_overview_mut(&project);
        match overview.update_workflow(&id_owned, &name_owned, &desc_owned, is_draft) {
            Ok(()) => {
                s.workflows_editing_selected = false;
                s.status_message = Some(format!("Saved changes to '{}'", name_owned.trim()));
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub fn launch_overview_ad_hoc_run(id: &str, summary: &str) -> bool {
    let mut started = false;
    let id_owned = id.to_owned();
    let summary_owned = summary.to_owned();
    let mut real_launch: Option<(String, String, String)> = None;
    state().update(|s| {
        let project = s.selected_project_name();
        if !s.demo_mode {
            if summary_owned.trim().is_empty() {
                s.status_message =
                    Some("Enter a task or issue reference for the ad-hoc run.".to_owned());
                return;
            }
            s.workflows_ad_hoc_open = false;
            real_launch = Some((project, id_owned.clone(), summary_owned.trim().to_owned()));
            started = true;
            return;
        }
        let overview = s.ensure_workflows_overview_mut(&project);
        let wf_name = overview
            .workflow(&id_owned)
            .map(|w| w.name.clone())
            .unwrap_or_else(|| id_owned.clone());
        match overview.launch_ad_hoc_run(&id_owned, &summary_owned) {
            Ok(run_no) => {
                s.workflows_ad_hoc_open = false;
                s.status_message = Some(format!("Started ad-hoc run #{run_no} ({wf_name})"));
                started = true;
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
    if let Some((project, wf_id, prompt)) = real_launch {
        return spawn_real_workflow_run(&project, &wf_id, &prompt, None, true);
    }
    started
}

pub fn duplicate_overview_workflow(id: &str) {
    let id_owned = id.to_owned();
    state().update(|s| {
        let project = s.selected_project_name();
        let overview = s.ensure_workflows_overview_mut(&project);
        match overview.duplicate_workflow(&id_owned) {
            Ok(copy_id) => {
                let copy_name = overview
                    .workflow(&copy_id)
                    .map(|w| w.name.clone())
                    .unwrap_or_else(|| copy_id.clone());
                s.workflows_selected_id = Some(copy_id);
                s.workflows_more_menu_open = false;
                s.status_message = Some(format!("Duplicated workflow as '{copy_name}'"));
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub fn duplicate_selected_overview_workflow() {
    let selected = state().with_untracked(|s| {
        let name = s.selected_project_name();
        let overview = s.workflows_overview_for_name(&name);
        s.workflows_selected_id
            .clone()
            .filter(|id| overview.workflow(id).is_some())
            .or_else(|| overview.default_selected_id())
    });
    if let Some(id) = selected {
        duplicate_overview_workflow(&id);
    }
}

pub fn toggle_overview_workflow_draft(id: &str) {
    let id_owned = id.to_owned();
    state().update(|s| {
        let project = s.selected_project_name();
        let overview = s.ensure_workflows_overview_mut(&project);
        let wf_name = overview
            .workflow(&id_owned)
            .map(|w| w.name.clone())
            .unwrap_or_else(|| id_owned.clone());
        match overview.toggle_draft(&id_owned) {
            Ok(is_draft) => {
                s.workflows_more_menu_open = false;
                let action = if is_draft {
                    "Moved to draft"
                } else {
                    "Published"
                };
                s.status_message = Some(format!("{action} '{wf_name}'"));
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub fn export_overview_workflow(id: &str) {
    let id_owned = id.to_owned();
    state().update(|s| {
        let project = s.selected_project_name();
        let overview = s.ensure_workflows_overview_mut(&project);
        let filename = format!("{id_owned}.bruk-workflow");
        let wf_name = overview
            .workflow(&id_owned)
            .map(|w| w.name.clone())
            .unwrap_or_else(|| id_owned.clone());
        s.workflows_more_menu_open = false;
        s.status_message = Some(format!("Exported '{wf_name}' to {filename}"));
    });
}

pub fn export_selected_overview_workflow() {
    let selected = state().with_untracked(|s| {
        let name = s.selected_project_name();
        let overview = s.workflows_overview_for_name(&name);
        s.workflows_selected_id
            .clone()
            .filter(|id| overview.workflow(id).is_some())
            .or_else(|| overview.default_selected_id())
    });
    if let Some(id) = selected {
        export_overview_workflow(&id);
    }
}

pub fn delete_overview_workflow(id: &str) {
    let id_owned = id.to_owned();
    state().update(|s| {
        let project = s.selected_project_name();
        if !s.demo_mode
            && let Some(project_root) = s.project_root_by_name(&project)
        {
            let catalog = s.active_catalog_clone();
            if let Some(wf) = catalog.resolve_workflow(&id_owned)
                && wf.origin.is_read_only()
            {
                s.status_message = Some(format!(
                    "'{}' comes from {} and is read-only. Edit it to create a project override.",
                    wf.name,
                    wf.origin.badge_label()
                ));
                s.workflows_more_menu_open = false;
                return;
            }
            match BrukCatalog::delete_project_workflow(&project_root, &id_owned) {
                Ok(true) => {
                    s.workflow_editors
                        .remove(&(project.clone(), id_owned.clone()));
                    s.reload_catalog_for_project(&project);
                    s.workflows_more_menu_open = false;
                    let reverted = s
                        .active_catalog_clone()
                        .resolve_workflow(&id_owned)
                        .map(|wf| wf.origin.badge_label());
                    s.status_message = Some(match reverted {
                        Some(layer) => format!(
                            "Removed project override for '{id_owned}' (reverted to {layer})"
                        ),
                        None => {
                            format!("Deleted workflow '{id_owned}' from .agents/bruk/workflows/")
                        }
                    });
                }
                Ok(false) => {
                    s.status_message = Some(format!("Workflow '{id_owned}' was not found on disk"));
                }
                Err(err) => {
                    s.status_message = Some(format!("Failed to delete workflow: {err}"));
                }
            }
            return;
        }
        let overview = s.ensure_workflows_overview_mut(&project);
        match overview.delete_workflow(&id_owned) {
            Ok(removed_name) => {
                let next_id = overview.default_selected_id();
                s.workflows_selected_id = next_id;
                s.workflows_more_menu_open = false;
                s.status_message = Some(format!("Deleted workflow '{removed_name}'"));
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub fn open_workflow_run_from_overview(workflow_name: &str) {
    let project_id = state().with_untracked(|s| {
        s.selected_project
            .or_else(|| s.factory.projects().first().map(|p| p.id))
    });
    if let Some(pid) = project_id {
        open_project_workflow(pid, workflow_name);
    }
}

// --- Visual Workflow Editor state -------------------------------------------------------

pub fn open_workflow_editor_from_overview(workflow_id: &str) {
    let target_id = workflow_id.to_owned();
    state().update(|s| {
        let project = s.selected_project_name();
        let demo = s.demo_mode;
        let catalog = if demo {
            None
        } else {
            Some(s.active_catalog_clone())
        };
        let entry = s
            .workflows_overview_for_name(&project)
            .workflow(&target_id)
            .cloned();
        let key = (project.clone(), target_id.clone());
        s.workflow_editors.entry(key.clone()).or_insert_with(|| {
            if let Some(cat) = catalog.as_ref()
                && let Some(wf_def) = cat.resolve_workflow(&target_id)
            {
                WorkflowEditorDocument::from_workflow_def(&project, wf_def, cat)
            } else if let Some(entry) = entry {
                WorkflowEditorDocument::from_overview_entry_with_demo(&project, &entry, demo)
            } else if demo {
                WorkflowEditorDocument::for_workflow_name(&project, &target_id)
            } else {
                WorkflowEditorDocument::empty(&project, &target_id)
            }
        });
        s.active_workflow_editor_key = key;
        s.workflow_editor_return_view = ActiveView::Workflows;
        s.workflow_editor_palette_search.clear();
        s.workflow_editor_inspector_visible = true;
        s.workflow_editor_history_open = false;
        s.workflow_editor_ad_hoc_open = false;
        s.workflow_editor_validation = None;
        s.active_view = ActiveView::WorkflowEditor;
        s.status_message = None;
    });
}

pub fn open_workflow_editor_for_selected_overview() {
    let selected = state().with_untracked(|s| {
        let name = s.selected_project_name();
        let overview = s.workflows_overview_for_name(&name);
        s.workflows_selected_id
            .clone()
            .filter(|id| overview.workflow(id).is_some())
            .or_else(|| overview.default_selected_id())
    });
    if let Some(id) = selected {
        open_workflow_editor_from_overview(&id);
    }
}

pub fn close_workflow_editor() {
    state().update(|s| {
        let target = match s.workflow_editor_return_view {
            ActiveView::WorkflowEditor => ActiveView::Workflows,
            other => other,
        };
        s.active_view = target;
        s.workflow_editor_history_open = false;
        s.workflow_editor_ad_hoc_open = false;
        s.workflow_editor_validation = None;
        s.status_message = None;
    });
}

pub fn open_workflows_from_editor() {
    state().update(|s| {
        s.active_view = ActiveView::Workflows;
        s.workflow_editor_history_open = false;
        s.workflow_editor_ad_hoc_open = false;
        s.workflow_editor_validation = None;
        s.status_message = None;
    });
}

pub fn active_workflow_editor() -> WorkflowEditorDocument {
    state().with(AppState::active_workflow_editor_clone)
}

pub fn workflow_editor_palette_search() -> String {
    state().with(|s| s.workflow_editor_palette_search.clone())
}

pub fn set_workflow_editor_palette_search(query: &str) {
    let query = query.to_owned();
    state().update(|s| {
        s.workflow_editor_palette_search = query;
    });
}

pub fn workflow_editor_inspector_visible() -> bool {
    state().with(|s| s.workflow_editor_inspector_visible)
}

pub fn toggle_workflow_editor_inspector() {
    state().update(|s| {
        s.workflow_editor_inspector_visible = !s.workflow_editor_inspector_visible;
    });
}

pub fn workflow_editor_history_open() -> bool {
    state().with(|s| s.workflow_editor_history_open)
}

pub fn toggle_workflow_editor_history() {
    state().update(|s| {
        s.workflow_editor_history_open = !s.workflow_editor_history_open;
        if s.workflow_editor_history_open {
            s.workflow_editor_ad_hoc_open = false;
        }
    });
}

pub fn workflow_editor_ad_hoc_open() -> bool {
    state().with(|s| s.workflow_editor_ad_hoc_open)
}

pub fn set_workflow_editor_ad_hoc_open(open: bool) {
    state().update(|s| {
        s.workflow_editor_ad_hoc_open = open;
        if open {
            s.workflow_editor_history_open = false;
        }
    });
}

pub fn workflow_editor_validation() -> Option<ValidationReport> {
    state().with(|s| s.workflow_editor_validation.clone())
}

pub fn dismiss_workflow_editor_validation() {
    state().update(|s| {
        s.workflow_editor_validation = None;
    });
}

pub fn select_workflow_editor_node(selection: EditorSelection) {
    state().update(|s| {
        s.ensure_workflow_editor_mut().selected = selection;
        s.workflow_editor_inspector_visible = true;
    });
}

pub fn add_workflow_editor_step(kind: StepTypeKind) {
    state().update(|s| {
        let doc = s.ensure_workflow_editor_mut();
        doc.add_from_palette(kind);
        let title = kind.title();
        s.workflow_editor_inspector_visible = true;
        s.workflow_editor_validation = None;
        s.status_message = Some(format!("Added '{title}' step to workflow"));
    });
}

pub fn add_workflow_editor_catalog_composite(composite_id: &str) {
    let comp_id = composite_id.to_owned();
    state().update(|s| {
        let catalog = s.active_catalog_clone();
        if let Some(composite) = catalog.resolve_composite(&comp_id).cloned() {
            let name = composite.name.clone();
            let doc = s.ensure_workflow_editor_mut();
            doc.add_catalog_composite(&composite);
            s.workflow_editor_inspector_visible = true;
            s.workflow_editor_validation = None;
            s.status_message = Some(format!("Added composite '{name}' to workflow"));
        }
    });
}

pub fn update_workflow_editor_step_agent(step_id: &str, agent_ref: Option<&str>) {
    let step_id = step_id.to_owned();
    let agent_owned = agent_ref.map(str::to_owned);
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .update_step_agent(&step_id, agent_owned.as_deref());
    });
}

pub fn update_workflow_editor_step_skills(step_id: &str, raw_skills: &str) {
    let step_id = step_id.to_owned();
    let raw = raw_skills.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .update_step_skills(&step_id, &raw);
    });
}

pub fn remove_workflow_editor_step(step_id: &str) {
    let step_id = step_id.to_owned();
    state().update(|s| {
        let doc = s.ensure_workflow_editor_mut();
        if doc.remove_step(&step_id) {
            s.workflow_editor_validation = None;
            s.status_message = Some("Removed step from workflow".to_owned());
        } else {
            s.status_message = Some("A workflow must keep at least one step.".to_owned());
        }
    });
}

pub fn set_workflow_editor_loop_name(loop_id: &str, name: &str) {
    let loop_id = loop_id.to_owned();
    let name = name.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .set_loop_name(&loop_id, &name);
    });
}

pub fn set_workflow_editor_loop_repeat(loop_id: &str, mode: LoopRepeatMode) {
    let loop_id = loop_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .set_loop_repeat_mode(&loop_id, mode);
    });
}

pub fn set_workflow_editor_loop_max_attempts(loop_id: &str, raw: &str) {
    let loop_id = loop_id.to_owned();
    let parsed = raw.trim().parse::<u32>().ok();
    if let Some(value) = parsed {
        state().update(|s| {
            s.ensure_workflow_editor_mut()
                .set_loop_max_attempts(&loop_id, value);
        });
    }
}

pub fn set_workflow_editor_loop_wait(loop_id: &str, wait: LoopWaitBetween) {
    let loop_id = loop_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .set_loop_wait_between(&loop_id, wait);
    });
}

pub fn toggle_workflow_editor_loop_pass_previous(loop_id: &str) {
    let loop_id = loop_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .toggle_loop_pass_previous(&loop_id);
    });
}

pub fn toggle_workflow_editor_loop_discard_changes(loop_id: &str) {
    let loop_id = loop_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .toggle_loop_discard_changes(&loop_id);
    });
}

pub fn set_workflow_editor_loop_on_exhausted(loop_id: &str, action: LoopExhaustedAction) {
    let loop_id = loop_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .set_loop_on_exhausted(&loop_id, action);
    });
}

pub fn add_workflow_editor_exit_condition(loop_id: &str) {
    let loop_id = loop_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .add_loop_exit_condition(&loop_id);
    });
}

pub fn remove_workflow_editor_exit_condition(loop_id: &str, condition_id: &str) {
    let loop_id = loop_id.to_owned();
    let condition_id = condition_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .remove_loop_exit_condition(&loop_id, &condition_id);
    });
}

pub fn update_workflow_editor_exit_condition(
    loop_id: &str,
    condition_id: &str,
    step_id: Option<String>,
    field: Option<ExitField>,
    operator: Option<ExitOperator>,
    expected: Option<String>,
) {
    let loop_id = loop_id.to_owned();
    let condition_id = condition_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut().update_loop_exit_condition(
            &loop_id,
            &condition_id,
            step_id,
            field,
            operator,
            expected,
        );
    });
}

pub fn update_workflow_editor_step_name(step_id: &str, name: &str) {
    let step_id = step_id.to_owned();
    let name = name.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .update_step_name(&step_id, &name);
    });
}

pub fn update_workflow_editor_step_detail(step_id: &str, detail: &str) {
    let step_id = step_id.to_owned();
    let detail = detail.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .update_step_detail(&step_id, &detail);
    });
}

pub fn update_workflow_editor_fallback_name(loop_id: &str, name: &str) {
    let loop_id = loop_id.to_owned();
    let name = name.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .update_fallback_name(&loop_id, &name);
    });
}

pub fn toggle_workflow_editor_trigger_work_queue() {
    state().update(|s| {
        s.ensure_workflow_editor_mut().toggle_trigger_work_queue();
    });
}

pub fn toggle_workflow_editor_trigger_ad_hoc() {
    state().update(|s| {
        s.ensure_workflow_editor_mut().toggle_trigger_ad_hoc();
    });
}

pub fn toggle_workflow_editor_mcp_project_only() {
    state().update(|s| {
        s.ensure_workflow_editor_mut().toggle_mcp_project_only();
    });
}

pub fn validate_workflow_editor() {
    state().update(|s| {
        let doc = s.active_workflow_editor_clone();
        let report = if s.demo_mode {
            doc.validate()
        } else {
            let catalog = s.active_catalog_clone();
            doc.validate_with_catalog(&catalog)
        };
        let summary = report.summary.clone();
        s.workflow_editor_validation = Some(report);
        s.status_message = Some(summary);
    });
}

pub fn save_workflow_editor() {
    state().update(|s| {
        let (project, workflow_id) = s.active_workflow_editor_key.clone();
        if !s.demo_mode
            && let Some(project_root) = s.project_root_by_name(&project)
        {
            let wf_def = s.ensure_workflow_editor_mut().to_workflow_def();
            let was_copy_on_edit = s
                .ensure_workflow_editor_mut()
                .origin
                .as_ref()
                .is_some_and(DefinitionOrigin::is_read_only);
            match BrukCatalog::save_project_workflow(&project_root, &wf_def) {
                Ok(saved_path) => {
                    let doc = s.ensure_workflow_editor_mut();
                    doc.origin = Some(DefinitionOrigin::Project {
                        file_path: saved_path,
                    });
                    doc.mark_saved();
                    let wf_name = doc.workflow_name.clone();
                    let wf_id = wf_def.id.clone();
                    s.reload_catalog_for_project(&project);
                    s.workflows_selected_id = Some(wf_id.clone());
                    s.workflow_editor_validation = None;
                    s.status_message = Some(if was_copy_on_edit {
                        format!(
                            "Saved project override '{wf_name}' to .agents/bruk/workflows/{wf_id}.yaml"
                        )
                    } else {
                        format!("Saved '{wf_name}' to .agents/bruk/workflows/{wf_id}.yaml")
                    });
                }
                Err(err) => {
                    log::error!(
                        target: "bruk::workflow",
                        "workflow.editor.save_failed project={project:?} workflow_id={workflow_id:?} err={err}"
                    );
                    s.status_message = Some(format!("Failed to save workflow: {err}"));
                }
            }
            return;
        }
        let (wf_name, step_groups, ad_hoc_allowed) = {
            let doc = s.ensure_workflow_editor_mut();
            doc.mark_saved();
            (
                doc.workflow_name.clone(),
                doc.to_step_groups(),
                doc.trigger.ad_hoc_allowed,
            )
        };
        let overview = s.ensure_workflows_overview_mut(&project);
        if let Some(entry) = overview.workflow_mut(&workflow_id) {
            entry.step_groups = step_groups;
            entry.ad_hoc_allowed = ad_hoc_allowed;
            entry.ad_hoc_label = if ad_hoc_allowed {
                "Allowed".to_owned()
            } else {
                "Disabled".to_owned()
            };
            entry.edited_label = "Just now by you".to_owned();
        }
        s.workflow_editor_validation = None;
        s.status_message = Some(format!("Saved workflow '{wf_name}'"));
    });
}

fn slugify_workflow_id(input: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_end_matches('-');
    if trimmed.is_empty() {
        "workflow".to_owned()
    } else {
        trimmed.to_owned()
    }
}

pub fn launch_editor_ad_hoc_run(summary: &str) -> bool {
    let prompt = summary.to_owned();
    let mut launched = false;
    let mut real_launch: Option<(String, String, String)> = None;
    state().update(|s| {
        let (project, workflow_id) = s.active_workflow_editor_key.clone();
        if !s.demo_mode {
            if prompt.trim().is_empty() {
                s.status_message =
                    Some("Enter a task or issue reference for the ad-hoc run.".to_owned());
                return;
            }
            s.workflow_editor_ad_hoc_open = false;
            real_launch = Some((project, workflow_id, prompt.trim().to_owned()));
            launched = true;
            return;
        }
        let wf_name = s.active_workflow_editor_clone().workflow_name;
        let overview = s.ensure_workflows_overview_mut(&project);
        match overview.launch_ad_hoc_run(&workflow_id, &prompt) {
            Ok(run_no) => {
                s.workflow_editor_ad_hoc_open = false;
                s.status_message = Some(format!("Started ad-hoc run #{run_no} ({wf_name})"));
                launched = true;
            }
            Err(_) => {
                // If the workflow doesn't exist in overview (e.g. ad-hoc fallback), still accept non-empty prompt.
                if prompt.trim().is_empty() {
                    s.status_message =
                        Some("Enter a task or issue reference for the ad-hoc run.".to_owned());
                } else {
                    s.workflow_editor_ad_hoc_open = false;
                    s.status_message =
                        Some(format!("Started ad-hoc run ({wf_name}): {}", prompt.trim()));
                    launched = true;
                }
            }
        }
    });
    if let Some((project, wf_id, prompt_text)) = real_launch {
        return spawn_real_workflow_run(&project, &wf_id, &prompt_text, None, true);
    }
    launched
}

fn workflow_requires_agent(workflow: &WorkflowDef, catalog: &BrukCatalog) -> bool {
    nodes_require_agent(&workflow.nodes, catalog)
}

fn nodes_require_agent(nodes: &[WorkflowNodeDef], catalog: &BrukCatalog) -> bool {
    nodes.iter().any(|node| match node {
        WorkflowNodeDef::AgentTask(_) => true,
        WorkflowNodeDef::Loop(lp) => nodes_require_agent(&lp.steps, catalog),
        WorkflowNodeDef::Composite(c) => catalog
            .resolve_composite(&c.uses)
            .is_some_and(|comp| nodes_require_agent(&comp.nodes, catalog)),
        _ => false,
    })
}

fn resolve_runner_gate_by_id(gate_id: &str, decision: GateDecision) {
    let entry = runner_registry()
        .lock()
        .ok()
        .and_then(|mut reg| reg.gates_by_id.remove(gate_id));
    if let Some((handle, _key)) = entry {
        handle.resolve_gate(decision);
    }
}

fn spawn_real_workflow_run(
    project_name: &str,
    workflow_id_or_name: &str,
    prompt: &str,
    issue_number: Option<u32>,
    open_run_view: bool,
) -> bool {
    let setup = state().with_untracked(|s| {
        let project = s
            .factory
            .projects()
            .iter()
            .find(|p| p.name == project_name)
            .cloned()?;
        let catalog = s
            .catalogs
            .get(project_name)
            .cloned()
            .unwrap_or_else(|| load_catalog(s.home.as_ref(), &project.root_path));
        let workflow = catalog
            .resolve_workflow_by_id_or_name(workflow_id_or_name)?
            .clone();
        let settings = s
            .project_settings
            .get(project_name)
            .cloned()
            .unwrap_or_else(|| {
                ProjectSettings::empty(
                    &project.name,
                    &project.root_path.display().to_string(),
                    &project.default_branch,
                )
            });

        let run_number =
            s.db.as_ref()
                .and_then(|arc| arc.lock().ok())
                .and_then(|db| db.next_run_number_for_project(project.id).ok())
                .unwrap_or(1);

        let ProviderLaunch {
            claude_binary,
            codex_binary,
            source_codex_home,
            antigravity_binary,
        } = s.provider_launch();

        let github_repo = if settings.github_repo.trim().is_empty() {
            None
        } else {
            Some(settings.github_repo.clone())
        };
        let secret_resolver = if s.demo_mode {
            None
        } else {
            Some(Arc::clone(&s.secret_resolver))
        };

        let project_env = s.agent_env(Some(&settings));
        let mut inputs = BTreeMap::new();
        inputs.insert("prompt".to_owned(), prompt.to_owned());
        if let Some(issue_no) = issue_number {
            inputs.insert("issue.number".to_owned(), issue_no.to_string());
        }

        let seq = runner_registry()
            .lock()
            .map(|mut reg| {
                reg.run_seq += 1;
                reg.run_seq
            })
            .unwrap_or(1);
        let run_id = format!("run-{}-{seq}", current_unix_seconds());
        let runtime_root = s.home.as_ref().map(|home| home.run_dir(&run_id));
        let started_at = current_unix_seconds();

        Some((
            WorkflowRunRequest {
                run_id,
                run_number,
                project_id: project.id,
                project_name: project.name.clone(),
                project_root: project.root_path.clone(),
                default_branch: project.default_branch.clone(),
                workflow,
                catalog,
                inputs,
                issue_number,
                github_repo,
                github_auth: None,
                github_auth_mode: settings.github_auth_mode,
                github_pat_secret_id: settings.github_pat_id.clone(),
                default_provider: settings.default_provider,
                default_sandbox: settings.sandbox_mode,
                model_override: if settings.model_override.trim().is_empty() {
                    None
                } else {
                    Some(settings.model_override.clone())
                },
                claude_binary,
                claude_default_model: Some(s.preferences.claude.default_model.clone()),
                claude_settings: s.preferences.claude_spawn_settings.clone(),
                codex_binary,
                codex_default_model: s
                    .preferences
                    .codex
                    .as_ref()
                    .map(|cx| cx.default_model.clone()),
                codex_settings: s.preferences.codex_spawn_settings.clone(),
                source_codex_home,
                antigravity_binary,
                antigravity_default_model: s
                    .preferences
                    .antigravity
                    .as_ref()
                    .map(|ag| ag.default_model.clone()),
                use_isolated_worktrees: settings.use_isolated_worktrees,
                branch_prefix: settings.branch_prefix.clone(),
                open_prs_as_draft: settings.open_prs_as_draft,
                link_pr_closes_issue: settings.link_pr_closes_issue,
                project_env,
                path_env: s.claude_path_lookup.resolve_path_env(),
                ssh_auth_sock: Some(s.preferences.advanced.ssh_auth_sock.trim().to_owned())
                    .filter(|sock| !sock.is_empty()),
                mcp: s.mcp_sources(&project.root_path, Some(&settings)),
                secret_resolver,
                runtime_root,
                budget_limit_cents: settings.per_run_budget_cents,
            },
            started_at,
        ))
    });

    let Some((request, started_at)) = setup else {
        log::warn!(
            target: "bruk::workflow",
            "workflow.launch.unresolved project={project_name:?} workflow={workflow_id_or_name:?}"
        );
        if open_run_view {
            state().update(|s| {
                s.status_message = Some(format!(
                    "Could not resolve workflow '{workflow_id_or_name}' for '{project_name}'"
                ));
            });
        }
        return false;
    };

    // When triggered implicitly from the work queue without a configured provider CLI binary,
    // keep the queue transition intact instead of immediately failing and yanking the working label.
    if !open_run_view
        && request.claude_binary.is_none()
        && request.codex_binary.is_none()
        && request.antigravity_binary.is_none()
        && workflow_requires_agent(&request.workflow, &request.catalog)
    {
        log::warn!(
            target: "bruk::workflow",
            "workflow.launch.skipped_no_provider project={project_name:?} workflow={:?}",
            request.workflow.name
        );
        return false;
    }

    let run_id = request.run_id.clone();
    let run_number = request.run_number;
    let project_id = request.project_id;
    let proj_name = request.project_name.clone();
    let wf_id = request.workflow.id.clone();
    let wf_name = request.workflow.name.clone();
    let default_provider_label = request.default_provider.label().to_owned();
    let budget_limit_cents = request.budget_limit_cents;
    let run_key = (proj_name.clone(), wf_name.clone());

    let (control, rx) = spawn_workflow_run(request);
    if let Ok(mut reg) = runner_registry().lock() {
        reg.active_by_key.insert(run_key.clone(), control.clone());
    }

    state().update(|s| {
        let mut initial_view = WorkflowRunViewData::empty(&proj_name, &wf_name);
        initial_view.run_number = run_number;
        initial_view.execution_state = RunExecutionState::Running;
        initial_view.subtitle = format!("Starting run #{run_number}…");
        s.workflow_runs_detail.insert(run_key.clone(), initial_view);
        s.active_workflow_run_key = run_key.clone();
        s.workflow_run_selected_step = 0;
        s.workflow_run_tab = RunDetailTab::Output;
        if open_run_view {
            s.active_view = ActiveView::WorkflowRun;
            s.status_message = Some(format!("Started run #{run_number} ({wf_name})"));
        }
    });

    // Pump runner events onto the reactive store thread without blocking UI rendering.
    let rx_shared = Arc::new(Mutex::new(rx));
    let control_for_gates = control;
    Owner::detached().run(|| {
        spawn(async move {
            let mut preflight_secret_problem: Option<SecretProblem> = None;
            loop {
                let rx_clone = rx_shared.clone();
                let next_event = spawn_blocking_task(move || {
                    rx_clone.lock().ok().and_then(|guard| guard.recv().ok())
                })
                .await;

                let Some(event) = next_event else {
                    break;
                };

                let is_finished = matches!(event, RunnerEvent::RunFinished { .. });
                match &event {
                    RunnerEvent::McpProblem {
                        step_title,
                        problem,
                        ..
                    } => {
                        let mut notification = None;
                        state().update(|s| {
                            notification = s.file_mcp_problem(
                                McpProblemContext {
                                    project_name: &proj_name,
                                    context_label: wf_name.clone(),
                                    where_label: format!("Step '{step_title}'"),
                                    run_number,
                                    provider: ProviderKind::Claude,
                                },
                                problem,
                            );
                        });
                        post_mcp_notifications(notification.into_iter().collect());
                    }
                    RunnerEvent::McpServersReady { servers } => {
                        let mut cleared = Vec::new();
                        state()
                            .update(|s| cleared = s.clear_ready_mcp_servers(&proj_name, servers));
                        close_notifications(cleared);
                    }
                    RunnerEvent::SecretProblem {
                        step_index,
                        step_title,
                        problem,
                    } => {
                        if (step_index.is_none() || *step_index == Some(0))
                            && preflight_secret_problem.is_none()
                        {
                            preflight_secret_problem = Some(problem.clone());
                        }
                        let mut notification = None;
                        state().update(|s| {
                            notification = s.file_secret_problem(
                                McpProblemContext {
                                    project_name: &proj_name,
                                    context_label: wf_name.clone(),
                                    where_label: format!("Step '{step_title}'"),
                                    run_number,
                                    provider: ProviderKind::Claude,
                                },
                                problem,
                            );
                        });
                        post_mcp_notifications(notification.into_iter().collect());
                    }
                    _ => {}
                }
                if let RunnerEvent::ApprovalRequested {
                    ref gate_id,
                    ref step_title,
                    ref prompt,
                    ref branch_name,
                    ref base_branch,
                    ref diff_summary,
                    ..
                } = event
                {
                    if let Ok(mut reg) = runner_registry().lock() {
                        reg.gates_by_id.insert(
                            gate_id.clone(),
                            (control_for_gates.clone(), run_key.clone()),
                        );
                    }
                    let req = AttentionRequest {
                        id: gate_id.clone(),
                        kind: AttentionRequestKind::Review,
                        agent: AgentBadge::Claude,
                        project_name: proj_name.clone(),
                        workflow_name: wf_name.clone(),
                        context_label: wf_name.clone(),
                        run_number,
                        machine_name: "This Mac".to_owned(),
                        inbox_title: step_title.clone(),
                        inbox_subtitle: prompt.clone(),
                        menubar_title: format!("{proj_name} · {step_title}"),
                        menubar_subtitle: format!("{wf_name} #{run_number}"),
                        waiting_minutes: 0,
                        past_reminder_threshold: false,
                        status_pill: "Waiting for review".to_owned(),
                        why_text: prompt.clone(),
                        deadline_badge: None,
                        timeout_footer: "Waiting for operator decision".to_owned(),
                        note_placeholder: "Optional review note…".to_owned(),
                        body: AttentionRequestBody::Review(ReviewDetail {
                            summary: prompt.clone(),
                            branch: branch_name.clone(),
                            base_branch: base_branch.clone(),
                            tests_summary: "Workflow paused at approval gate".to_owned(),
                            files: diff_summary.clone(),
                        }),
                    };
                    state().update(|s| {
                        s.inbox.pending.insert(0, req);
                        if s.inbox_selected_id.is_none() {
                            s.inbox_selected_id = s.inbox.default_selected_id();
                        }
                        s.sync_inbox_selection_and_drafts();
                    });
                }

                let mut finished_issue_cleanup = None;
                state().update(|s| {
                    let view = s
                        .workflow_runs_detail
                        .entry(run_key.clone())
                        .or_insert_with(|| WorkflowRunViewData::empty(&proj_name, &wf_name));
                    apply_runner_event_to_view_data(view, &event, budget_limit_cents);
                    s.workflow_run_selected_step = view.active_step_index;

                    if let RunnerEvent::RunFinished {
                        status,
                        total_duration_ms,
                        total_tokens,
                        total_cost_cents,
                        ref summary,
                        ..
                    } = event
                    {
                        let view_snapshot = view.clone();
                        let provider_name = if view_snapshot.active_provider_name.trim().is_empty()
                        {
                            default_provider_label.clone()
                        } else {
                            view_snapshot.active_provider_name.clone()
                        };
                        let persisted = PersistedWorkflowRun {
                            id: run_id.clone(),
                            project_id,
                            project_name: proj_name.clone(),
                            workflow_id: wf_id.clone(),
                            workflow_name: wf_name.clone(),
                            run_number,
                            status,
                            provider: provider_name,
                            issue_number,
                            branch_name: view_snapshot.branch_name.clone(),
                            total_tokens,
                            total_cost_cents,
                            duration_ms: total_duration_ms,
                            summary: summary.clone(),
                            view: view_snapshot,
                            started_at,
                            finished_at: Some(current_unix_seconds()),
                        };
                        if let Some(db_arc) = s.db.as_ref()
                            && let Ok(db) = db_arc.lock()
                        {
                            let _ = db.save_workflow_run(&persisted);
                            if let Ok(all_runs) = db.list_all_workflow_runs() {
                                let chat_usage = db.list_chat_usage().unwrap_or_default();
                                s.dashboard_dataset =
                                    DashboardDataset::from_usage(&all_runs, &chat_usage);
                            }
                        }

                        if let Some(overview) = s.workflows_overviews.get_mut(&proj_name)
                            && let Some(entry) = overview.workflow_mut(&wf_id)
                        {
                            let dot = match status {
                                RunExecutionState::Succeeded => RunOutcomeDot::Succeeded,
                                RunExecutionState::Failed | RunExecutionState::Stopped => {
                                    RunOutcomeDot::FailedOrStopped
                                }
                                _ => RunOutcomeDot::RunningOrWaiting,
                            };
                            if entry.last_ten_runs.len() >= 10 {
                                entry.last_ten_runs.remove(0);
                            }
                            entry.last_ten_runs.push(dot);
                            entry.last_run_caption = format!("Run #{run_number} · just now");
                            entry.last_run_order = 0;
                            entry.recent_runs.insert(
                                0,
                                WorkflowRecentRun {
                                    run_number,
                                    summary: summary.clone(),
                                    status: match status {
                                        RunExecutionState::Succeeded => RecentRunStatus::Done,
                                        _ => RecentRunStatus::Failed,
                                    },
                                },
                            );
                            entry.recent_runs.truncate(5);
                        }

                        s.flush_secret_resolver_updates();
                        if let Some(issue_no) = issue_number {
                            if status == RunExecutionState::Failed
                                && let Some(ref prob) = preflight_secret_problem
                            {
                                let reason = prob.title();
                                let queue = s.ensure_queue_mut(&proj_name);
                                queue.hold_item_for_setup(issue_no, &reason);
                                s.persist_work_queue_by_name(&proj_name);
                                let resume_problem = SecretProblem {
                                    project_id: Some(project_id),
                                    secret_id: prob.secret_id.clone(),
                                    secret_name: prob.secret_name.clone(),
                                    manager_id: prob.manager_id.clone(),
                                    reason: format!(
                                        "Queued work for {proj_name} is held in Needs Setup ({reason}). Fix the secret, then resume queued work."
                                    ),
                                    fix: SecretFix::ResumeQueuedWork { project_id },
                                    blocking: true,
                                };
                                let _ = s.file_secret_problem(
                                    McpProblemContext {
                                        project_name: &proj_name,
                                        context_label: wf_name.clone(),
                                        where_label: format!("Issue #{issue_no}"),
                                        run_number,
                                        provider: ProviderKind::Claude,
                                    },
                                    &resume_problem,
                                );
                            } else {
                                finished_issue_cleanup = Some(issue_no);
                            }
                        }
                        s.status_message = Some(summary.clone());
                    }
                });

                if let Some(issue_no) = finished_issue_cleanup {
                    spawn_github_working_label_update(issue_no, false);
                }

                if is_finished {
                    if let Ok(mut reg) = runner_registry().lock() {
                        reg.active_by_key.remove(&run_key);
                        reg.gates_by_id.retain(|_, (_, k)| *k != run_key);
                    }
                    break;
                }
            }
        });
    });

    true
}

// --- Secrets & Secret Managers ----------------------------------------------------------

/// Summary of an inherited App secret as seen from the active project.
#[derive(Debug, Clone)]
pub struct InheritedAppSecretView {
    pub record: SecretRecord,
    pub excluded: bool,
    pub overridden: bool,
}

/// Summary of a `${VAR}` required by any MCP server in the active project.
#[derive(Debug, Clone)]
pub struct NeededMcpVarView {
    pub name: String,
    pub servers: Vec<String>,
    pub status: NeededMcpVarStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NeededMcpVarStatus {
    ProjectSecret {
        source_label: String,
        needs_value: bool,
    },
    AppSecret {
        source_label: String,
        needs_value: bool,
    },
    PlainVar,
    ExcludedAppSecret,
    Missing,
}

pub fn secrets_store_status() -> StoreStatus {
    state().with(|s| s.secrets_store_status.clone())
}

pub fn secrets_feedback() -> Option<String> {
    state().with(|s| s.secrets_feedback.clone())
}

pub fn secrets_error() -> Option<String> {
    state().with(|s| s.secrets_error.clone())
}

pub fn clear_secrets_messages() {
    state().update(|s| {
        s.secrets_feedback = None;
        s.secrets_error = None;
    });
}

pub fn secrets_test_in_progress() -> bool {
    state().with(|s| s.secrets_test_in_progress)
}

pub fn secrets_test_result() -> Option<Result<String, String>> {
    state().with(|s| s.secrets_test_result.clone())
}

pub fn db_path_change_pending() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    state().with(|s| s.db_path_change_pending.clone())
}

pub fn legacy_db_files_found() -> Vec<std::path::PathBuf> {
    state().with(|s| s.legacy_db_files_found.clone())
}

pub fn take_secrets_prefill() -> (Option<String>, Option<EnvScope>) {
    let mut out = (None, None);
    state().update(|s| {
        out = (
            s.secrets_prefill_name.take(),
            s.secrets_prefill_scope.take(),
        );
    });
    out
}

pub fn app_env_secrets() -> Vec<SecretRecord> {
    state().with(|s| {
        s.secrets
            .iter()
            .filter(|r| {
                matches!(
                    r.kind,
                    SecretKind::Env {
                        scope: EnvScope::App { .. },
                        ..
                    }
                )
            })
            .cloned()
            .collect()
    })
}

pub fn github_pat_secrets() -> Vec<SecretRecord> {
    state().with(|s| {
        s.secrets
            .iter()
            .filter(|r| {
                matches!(
                    r.kind,
                    SecretKind::Internal {
                        purpose: InternalPurpose::GitHubPat
                    }
                )
            })
            .cloned()
            .collect()
    })
}

pub fn projects_using_github_pat(secret_id: &str) -> Vec<String> {
    state().with(|s| {
        s.project_settings
            .values()
            .filter(|ps| ps.github_pat_id.as_deref() == Some(secret_id))
            .map(|ps| ps.project_name.clone())
            .collect()
    })
}

/// Projects' MCP servers (tracked + user + project extra) referencing a secret's env var name.
pub fn mcp_servers_referencing_var(var_name: &str) -> Vec<String> {
    state().with(|s| {
        let mut names = BTreeSet::new();
        for server in &s.preferences.extensions.mcp_servers {
            if server.env_references().iter().any(|r| r == var_name) {
                names.insert(format!("{} (all projects)", server.name));
            }
        }
        for project in s.factory.projects() {
            if let Some(config) = factory_config::mcp::load_project_mcp(&project.root_path).config()
            {
                for server in &config.servers {
                    if server.env_references().iter().any(|r| r == var_name) {
                        names.insert(format!("{} ({})", server.name, project.name));
                    }
                }
            }
            if let Some(ps) = s.project_settings.get(&project.name) {
                for server in &ps.mcp_servers {
                    if server.env_references().iter().any(|r| r == var_name) {
                        names.insert(format!("{} ({})", server.name, project.name));
                    }
                }
            }
        }
        names.into_iter().collect()
    })
}

pub fn active_project_env_secrets() -> Vec<SecretRecord> {
    state().with(|s| {
        let pname = s.selected_project_name();
        let Some(pid) = s.project_id_by_name(&pname) else {
            return Vec::new();
        };
        s.secrets
            .iter()
            .filter(|r| {
                matches!(
                    r.kind,
                    SecretKind::Env {
                        scope: EnvScope::Project(id),
                        ..
                    } if id == pid
                )
            })
            .cloned()
            .collect()
    })
}

pub fn active_project_inherited_app_secrets() -> Vec<InheritedAppSecretView> {
    state().with(|s| {
        let pname = s.selected_project_name();
        let Some(pid) = s.project_id_by_name(&pname) else {
            return Vec::new();
        };
        let settings = s.project_settings.get(&pname);
        let excluded_ids = settings
            .map(|ps| ps.excluded_app_secrets.clone())
            .unwrap_or_default();
        let project_secret_names: BTreeSet<String> = s
            .secrets
            .iter()
            .filter(|r| {
                matches!(
                    r.kind,
                    SecretKind::Env {
                        scope: EnvScope::Project(id),
                        ..
                    } if id == pid
                )
            })
            .map(|r| r.name.to_ascii_uppercase())
            .collect();

        s.secrets
            .iter()
            .filter_map(|r| match &r.kind {
                SecretKind::Env {
                    scope: EnvScope::App { shared_with },
                    ..
                } if shared_with.includes_project(pid) => Some(InheritedAppSecretView {
                    record: r.clone(),
                    excluded: excluded_ids.contains(&r.id),
                    overridden: project_secret_names.contains(&r.name.to_ascii_uppercase()),
                }),
                _ => None,
            })
            .collect()
    })
}

/// Returns `${VAR}` names needed by any MCP server in the active project and their resolution status.
pub fn active_project_needed_mcp_vars() -> Vec<NeededMcpVarView> {
    state().with(|s| {
        let pname = s.selected_project_name();
        let pid = s.project_id_by_name(&pname);
        let settings = s.project_settings.get(&pname);
        let project_root = s.project_root_by_name(&pname);
        let mut by_var: BTreeMap<String, Vec<String>> = BTreeMap::new();
        if let Some(root) = project_root
            && let Some(config) = factory_config::mcp::load_project_mcp(&root).config()
        {
            for server in &config.servers {
                if !server.disabled {
                    for var in server.env_references() {
                        by_var.entry(var).or_default().push(server.name.clone());
                    }
                }
            }
        }
        if let Some(ps) = settings {
            for server in &ps.mcp_servers {
                if !server.disabled {
                    for var in server.env_references() {
                        by_var.entry(var).or_default().push(server.name.clone());
                    }
                }
            }
        }
        for server in &s.preferences.extensions.mcp_servers {
            if !server.disabled {
                for var in server.env_references() {
                    by_var.entry(var).or_default().push(server.name.clone());
                }
            }
        }

        let excluded_ids = settings
            .map(|ps| ps.excluded_app_secrets.clone())
            .unwrap_or_default();
        let plain_names: BTreeSet<String> = settings
            .map(|ps| {
                ps.env_vars
                    .iter()
                    .filter(|v| !v.is_secret && !v.key.trim().is_empty())
                    .map(|v| v.key.clone())
                    .collect()
            })
            .unwrap_or_default();

        by_var
            .into_iter()
            .map(|(name, mut servers)| {
                servers.sort();
                servers.dedup();
                let status = if let Some(pid) = pid
                    && let Some(rec) = s.secrets.iter().find(|r| {
                        matches!(
                            r.kind,
                            SecretKind::Env {
                                scope: EnvScope::Project(id),
                                ..
                            } if id == pid
                        ) && r.name.eq_ignore_ascii_case(&name)
                    }) {
                    NeededMcpVarStatus::ProjectSecret {
                        source_label: rec.source.badge_label().to_owned(),
                        needs_value: rec.state == SecretState::NeedsValue,
                    }
                } else if let Some(pid) = pid
                    && let Some(rec) = s.secrets.iter().find(|r| {
                        matches!(
                            &r.kind,
                            SecretKind::Env {
                                scope: EnvScope::App { shared_with },
                                ..
                            } if shared_with.includes_project(pid)
                        ) && r.name.eq_ignore_ascii_case(&name)
                    })
                {
                    if excluded_ids.contains(&rec.id) {
                        NeededMcpVarStatus::ExcludedAppSecret
                    } else {
                        NeededMcpVarStatus::AppSecret {
                            source_label: rec.source.badge_label().to_owned(),
                            needs_value: rec.state == SecretState::NeedsValue,
                        }
                    }
                } else if plain_names.contains(&name) {
                    NeededMcpVarStatus::PlainVar
                } else {
                    NeededMcpVarStatus::Missing
                };
                NeededMcpVarView {
                    name,
                    servers,
                    status,
                }
            })
            .collect()
    })
}

/// Checks if a `${VAR}` referenced by a project MCP server currently resolves to an inherited App secret.
pub fn var_resolves_to_inherited_app_secret(var_name: &str) -> bool {
    state().with(|s| {
        let pname = s.selected_project_name();
        let Some(pid) = s.project_id_by_name(&pname) else {
            return false;
        };
        let has_proj = s.secrets.iter().any(|r| {
            matches!(
                r.kind,
                SecretKind::Env {
                    scope: EnvScope::Project(id),
                    ..
                } if id == pid
            ) && r.name.eq_ignore_ascii_case(var_name)
        });
        if has_proj {
            return false;
        }
        let excluded = s
            .project_settings
            .get(&pname)
            .map(|ps| ps.excluded_app_secrets.clone())
            .unwrap_or_default();
        s.secrets.iter().any(|r| {
            matches!(
                &r.kind,
                SecretKind::Env {
                    scope: EnvScope::App { shared_with },
                    ..
                } if shared_with.includes_project(pid)
            ) && !excluded.contains(&r.id)
                && r.name.eq_ignore_ascii_case(var_name)
        })
    })
}

pub fn create_app_keychain_secret(
    name: &str,
    raw_value: &str,
    sharing: Sharing,
    exposure: Exposure,
) -> bool {
    let mut ok = false;
    state().update(|s| {
        if s.demo_mode {
            s.secrets_error = Some("Disable Demo Mode to add real Keychain secrets.".to_owned());
            return;
        }
        let trimmed = name.trim();
        let record = SecretRecord::new_keychain(
            trimmed,
            SecretKind::Env {
                scope: EnvScope::App {
                    shared_with: sharing,
                },
                exposure,
            },
            current_unix_seconds(),
        );
        let secret = SecretString::from(raw_value.to_owned());
        let Some(db_arc) = s.db.as_ref() else {
            s.secrets_error = Some("Database unavailable.".to_owned());
            return;
        };
        let res = db_arc
            .lock()
            .ok()
            .map(|db| db.create_keychain_secret(s.secret_store.as_ref(), record, &secret, None));
        match res {
            Some(Ok(saved)) => {
                let cleared = s.inbox.clear_secret_problems(|p| {
                    p.secret_name
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(&saved.name))
                });
                close_notifications(cleared);
                s.sync_inbox_selection_and_drafts();
                s.secrets_error = None;
                s.secrets_feedback =
                    Some(format!("Saved app secret '{}' in Keychain.", saved.name));
                log::info!(
                    target: "bruk::secrets",
                    "secrets.created id={} name={:?} scope=app source=keychain",
                    saved.id,
                    saved.name
                );
                s.refresh_secrets_state();
                ok = true;
            }
            Some(Err(err)) => {
                log::warn!(
                    target: "bruk::secrets",
                    "secrets.create_failed name={trimmed:?} scope=app source=keychain err={err}"
                );
                s.secrets_feedback = None;
                s.secrets_error = Some(err.to_string());
            }
            None => {
                s.secrets_error = Some("Could not lock database.".to_owned());
            }
        }
    });
    ok
}

pub fn create_app_manager_secret(
    name: &str,
    manager_id: &str,
    reference: &str,
    sharing: Sharing,
    exposure: Exposure,
) -> bool {
    let mut ok = false;
    state().update(|s| {
        if s.demo_mode {
            s.secrets_error = Some("Disable Demo Mode to add secret references.".to_owned());
            return;
        }
        let record = SecretRecord::new_manager_ref(
            name.trim(),
            SecretKind::Env {
                scope: EnvScope::App {
                    shared_with: sharing,
                },
                exposure,
            },
            manager_id.trim(),
            reference.trim(),
            current_unix_seconds(),
        );
        let Some(db_arc) = s.db.as_ref() else {
            s.secrets_error = Some("Database unavailable.".to_owned());
            return;
        };
        let res = db_arc
            .lock()
            .ok()
            .map(|db| db.create_manager_secret(record));
        match res {
            Some(Ok(saved)) => {
                let cleared = s.inbox.clear_secret_problems(|p| {
                    p.secret_name
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(&saved.name))
                });
                close_notifications(cleared);
                s.sync_inbox_selection_and_drafts();
                s.secrets_error = None;
                s.secrets_feedback = Some(format!("Saved manager secret '{}'.", saved.name));
                s.refresh_secrets_state();
                ok = true;
            }
            Some(Err(err)) => {
                s.secrets_feedback = None;
                s.secrets_error = Some(err.to_string());
            }
            None => {
                s.secrets_error = Some("Could not lock database.".to_owned());
            }
        }
    });
    ok
}

pub fn create_project_keychain_secret(name: &str, raw_value: &str, exposure: Exposure) -> bool {
    let mut ok = false;
    state().update(|s| {
        if s.demo_mode {
            s.project_settings_error =
                Some("Disable Demo Mode to add real Keychain secrets.".to_owned());
            return;
        }
        let pname = s.selected_project_name();
        let Some(pid) = s.project_id_by_name(&pname) else {
            s.project_settings_error = Some("No persisted project selected.".to_owned());
            return;
        };
        let record = SecretRecord::new_keychain(
            name.trim(),
            SecretKind::Env {
                scope: EnvScope::Project(pid),
                exposure,
            },
            current_unix_seconds(),
        );
        let secret = SecretString::from(raw_value.to_owned());
        let Some(db_arc) = s.db.as_ref() else {
            s.project_settings_error = Some("Database unavailable.".to_owned());
            return;
        };
        let res = db_arc.lock().ok().map(|db| {
            db.create_keychain_secret(s.secret_store.as_ref(), record, &secret, Some(&pname))
        });
        match res {
            Some(Ok(saved)) => {
                let cleared = s.inbox.clear_secret_problems(|p| {
                    p.secret_name
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(&saved.name))
                });
                close_notifications(cleared);
                s.sync_inbox_selection_and_drafts();
                s.project_settings_error = None;
                s.project_settings_feedback =
                    Some(format!("Saved secret '{}' in Keychain.", saved.name));
                log::info!(
                    target: "bruk::secrets",
                    "secrets.created id={} name={:?} scope=project project={pname:?} source=keychain",
                    saved.id,
                    saved.name
                );
                s.refresh_secrets_state();
                ok = true;
            }
            Some(Err(err)) => {
                log::warn!(
                    target: "bruk::secrets",
                    "secrets.create_failed name={:?} scope=project project={pname:?} source=keychain err={err}",
                    name.trim()
                );
                s.project_settings_feedback = None;
                s.project_settings_error = Some(err.to_string());
            }
            None => {
                s.project_settings_error = Some("Could not lock database.".to_owned());
            }
        }
    });
    ok
}

pub fn create_project_manager_secret(
    name: &str,
    manager_id: &str,
    reference: &str,
    exposure: Exposure,
) -> bool {
    let mut ok = false;
    state().update(|s| {
        if s.demo_mode {
            s.project_settings_error =
                Some("Disable Demo Mode to add secret references.".to_owned());
            return;
        }
        let pname = s.selected_project_name();
        let Some(pid) = s.project_id_by_name(&pname) else {
            s.project_settings_error = Some("No persisted project selected.".to_owned());
            return;
        };
        let record = SecretRecord::new_manager_ref(
            name.trim(),
            SecretKind::Env {
                scope: EnvScope::Project(pid),
                exposure,
            },
            manager_id.trim(),
            reference.trim(),
            current_unix_seconds(),
        );
        let Some(db_arc) = s.db.as_ref() else {
            s.project_settings_error = Some("Database unavailable.".to_owned());
            return;
        };
        let res = db_arc
            .lock()
            .ok()
            .map(|db| db.create_manager_secret(record));
        match res {
            Some(Ok(saved)) => {
                let cleared = s.inbox.clear_secret_problems(|p| {
                    p.secret_name
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(&saved.name))
                });
                close_notifications(cleared);
                s.sync_inbox_selection_and_drafts();
                s.project_settings_error = None;
                s.project_settings_feedback =
                    Some(format!("Saved manager secret '{}'.", saved.name));
                s.refresh_secrets_state();
                ok = true;
            }
            Some(Err(err)) => {
                s.project_settings_feedback = None;
                s.project_settings_error = Some(err.to_string());
            }
            None => {
                s.project_settings_error = Some("Could not lock database.".to_owned());
            }
        }
    });
    ok
}

pub fn create_github_pat_secret(name: &str, raw_token: &str) -> Option<SecretId> {
    let mut created = None;
    state().update(|s| {
        if s.demo_mode {
            s.secrets_error = Some("Disable Demo Mode to save real GitHub tokens.".to_owned());
            return;
        }
        let trimmed = name.trim();
        let record = SecretRecord::new_keychain(
            trimmed,
            SecretKind::Internal {
                purpose: InternalPurpose::GitHubPat,
            },
            current_unix_seconds(),
        );
        let secret = SecretString::from(raw_token.trim().to_owned());
        let Some(db_arc) = s.db.as_ref() else {
            s.secrets_error = Some("Database unavailable.".to_owned());
            return;
        };
        let res = db_arc
            .lock()
            .ok()
            .map(|db| db.create_keychain_secret(s.secret_store.as_ref(), record, &secret, None));
        match res {
            Some(Ok(saved)) => {
                created = Some(saved.id.clone());
                s.secrets_error = None;
                s.secrets_feedback =
                    Some(format!("Saved GitHub PAT '{}' in Keychain.", saved.name));
                s.refresh_secrets_state();
            }
            Some(Err(err)) => {
                s.secrets_feedback = None;
                s.secrets_error = Some(err.to_string());
            }
            None => {
                s.secrets_error = Some("Could not lock database.".to_owned());
            }
        }
    });
    created
}

pub fn replace_secret_value(id: &str, new_value: &str) -> bool {
    let mut ok = false;
    state().update(|s| {
        if s.demo_mode {
            s.secrets_error = Some("Disable Demo Mode to modify Keychain secrets.".to_owned());
            return;
        }
        let proj_name = s
            .secrets
            .iter()
            .find(|r| r.id == id)
            .and_then(|r| r.project_id())
            .and_then(|pid| s.factory.project(pid))
            .map(|p| p.name.clone());
        let secret = SecretString::from(new_value.to_owned());
        let Some(db_arc) = s.db.as_ref() else {
            s.secrets_error = Some("Database unavailable.".to_owned());
            return;
        };
        let res = db_arc.lock().ok().map(|db| {
            db.replace_keychain_secret(s.secret_store.as_ref(), id, &secret, proj_name.as_deref())
        });
        match res {
            Some(Ok(saved)) => {
                let cleared = s
                    .inbox
                    .clear_secret_problems(|p| p.secret_id.as_deref() == Some(&saved.id));
                close_notifications(cleared);
                s.sync_inbox_selection_and_drafts();
                s.secrets_error = None;
                s.secrets_feedback = Some(format!("Updated '{}' in Keychain.", saved.name));
                s.project_settings_error = None;
                s.project_settings_feedback =
                    Some(format!("Updated '{}' in Keychain.", saved.name));
                s.refresh_secrets_state();
                ok = true;
            }
            Some(Err(err)) => {
                s.secrets_feedback = None;
                s.secrets_error = Some(err.to_string());
                s.project_settings_error = Some(err.to_string());
            }
            None => {
                s.secrets_error = Some("Could not lock database.".to_owned());
            }
        }
    });
    ok
}

pub fn update_secret_exposure(id: &str, exposure: Exposure) {
    state().update(|s| {
        if s.demo_mode {
            return;
        }
        let Some(mut record) = s.secrets.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        if let SecretKind::Env {
            exposure: ref mut exp,
            ..
        } = record.kind
        {
            *exp = exposure;
        } else {
            return;
        }
        let proj_name = record
            .project_id()
            .and_then(|pid| s.factory.project(pid))
            .map(|p| p.name.clone());
        let outcome = s.db.as_ref().and_then(|db_arc| {
            db_arc.lock().ok().map(|db| {
                db.update_secret_metadata(s.secret_store.as_ref(), record, proj_name.as_deref())
            })
        });
        if let Some(res) = outcome {
            match res {
                Ok(saved) => {
                    s.secrets_error = None;
                    s.secrets_feedback = Some(format!(
                        "Updated exposure for '{}' to {}.",
                        saved.name,
                        exposure.label()
                    ));
                    s.project_settings_error = None;
                    s.project_settings_feedback = s.secrets_feedback.clone();
                    s.refresh_secrets_state();
                }
                Err(err) => {
                    s.secrets_error = Some(err.to_string());
                    s.project_settings_error = Some(err.to_string());
                }
            }
        }
    });
}

pub fn update_app_secret_sharing(id: &str, sharing: Sharing) {
    state().update(|s| {
        if s.demo_mode {
            return;
        }
        let Some(mut record) = s.secrets.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        if let SecretKind::Env {
            scope: EnvScope::App {
                shared_with: ref mut sh,
            },
            ..
        } = record.kind
        {
            *sh = sharing;
        } else {
            return;
        }
        let outcome = s.db.as_ref().and_then(|db_arc| {
            db_arc
                .lock()
                .ok()
                .map(|db| db.update_secret_metadata(s.secret_store.as_ref(), record, None))
        });
        if let Some(res) = outcome {
            match res {
                Ok(saved) => {
                    s.secrets_error = None;
                    s.secrets_feedback = Some(format!("Updated sharing for '{}'.", saved.name));
                    s.refresh_secrets_state();
                }
                Err(err) => {
                    s.secrets_error = Some(err.to_string());
                }
            }
        }
    });
}

pub fn delete_secret_by_id(id: &str) {
    state().update(|s| {
        if s.demo_mode {
            return;
        }
        let name = s
            .secrets
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| id.to_owned());
        let outcome = s.db.as_ref().and_then(|db_arc| {
            db_arc
                .lock()
                .ok()
                .map(|db| db.delete_secret(s.secret_store.as_ref(), id))
        });
        if let Some(res) = outcome {
            match res {
                Ok(_) => {
                    // Unlink from any project GitHub PAT reference.
                    let affected_projects: Vec<String> = s
                        .project_settings
                        .iter()
                        .filter(|(_, ps)| ps.github_pat_id.as_deref() == Some(id))
                        .map(|(k, _)| k.clone())
                        .collect();
                    for pname in affected_projects {
                        if let Some(ps) = s.project_settings.get_mut(&pname) {
                            ps.github_pat_id = None;
                            ps.github_token_last4 = None;
                        }
                        s.persist_project_settings_by_name(&pname);
                    }
                    s.secrets_error = None;
                    s.secrets_feedback = Some(format!("Deleted secret '{name}'."));
                    s.project_settings_error = None;
                    s.project_settings_feedback = Some(format!("Deleted secret '{name}'."));
                    log::info!(
                        target: "bruk::secrets",
                        "secrets.deleted id={id} name={name:?}"
                    );
                    s.refresh_secrets_state();
                }
                Err(err) => {
                    log::error!(
                        target: "bruk::secrets",
                        "secrets.delete_failed id={id} name={name:?} err={err}"
                    );
                    s.secrets_error = Some(err.to_string());
                    s.project_settings_error = Some(err.to_string());
                }
            }
        }
    });
}

pub fn toggle_project_exclude_app_secret(secret_id: &str) {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        let excluded = if settings.excluded_app_secrets.contains(secret_id) {
            settings.excluded_app_secrets.remove(secret_id);
            false
        } else {
            settings.excluded_app_secrets.insert(secret_id.to_owned());
            true
        };
        s.project_settings_error = None;
        s.project_settings_feedback = Some(if excluded {
            "Excluded app secret from this project.".to_owned()
        } else {
            "Re-included app secret in this project.".to_owned()
        });
        s.persist_active_project_settings();
        s.refresh_secrets_state();
    });
}

pub fn confirm_database_moved() {
    state().update(|s| {
        let outcome = s.db.as_ref().and_then(|db_arc| {
            db_arc.lock().ok().map(|db| {
                let res = db.confirm_database_moved();
                if res.is_ok() {
                    let _ = migrate_plaintext_secrets(&db, s.secret_store.as_ref(), false, false);
                    let _ = db.sweep_secret_tombstones(s.secret_store.as_ref(), false);
                }
                res
            })
        });
        if let Some(res) = outcome {
            match res {
                Ok(()) => {
                    log::info!(target: "bruk::secrets", "secrets.db_moved_confirmed");
                    s.db_path_change_pending = None;
                    s.secrets_error = None;
                    s.secrets_feedback =
                        Some("Confirmed database moved. Keychain bindings kept.".to_owned());
                    let cleared = s
                        .inbox
                        .clear_secret_problems(|p| p.reason.contains("Database moved or cloned"));
                    close_notifications(cleared);
                    s.sync_inbox_selection_and_drafts();
                    s.refresh_secrets_state();
                }
                Err(err) => {
                    log::error!(
                        target: "bruk::secrets",
                        "secrets.db_moved_failed err={err}"
                    );
                    s.secrets_error = Some(format!("Could not update database path: {err}"));
                }
            }
        }
    });
}

pub fn confirm_database_cloned() {
    state().update(|s| {
        let outcome = s.db.as_ref().and_then(|db_arc| {
            db_arc.lock().ok().map(|db| {
                let res = db.confirm_database_cloned();
                if res.is_ok() {
                    let _ = migrate_plaintext_secrets(&db, s.secret_store.as_ref(), false, false);
                }
                res
            })
        });
        if let Some(res) = outcome {
            match res {
                Ok(_) => {
                    log::info!(target: "bruk::secrets", "secrets.db_cloned_confirmed");
                    s.db_path_change_pending = None;
                    s.secrets_error = None;
                    s.secrets_feedback = Some(
                        "Confirmed database cloned. Assigned a fresh Keychain namespace; re-enter Keychain values below."
                            .to_owned(),
                    );
                    let cleared = s
                        .inbox
                        .clear_secret_problems(|p| p.reason.contains("Database moved or cloned"));
                    close_notifications(cleared);
                    s.sync_inbox_selection_and_drafts();
                    s.refresh_secrets_state();
                }
                Err(err) => {
                    log::error!(
                        target: "bruk::secrets",
                        "secrets.db_cloned_failed err={err}"
                    );
                    s.secrets_error = Some(format!("Could not reset cloned database: {err}"));
                }
            }
        }
    });
}

pub fn delete_legacy_databases_now() {
    state().update(|s| {
        let paths = s.legacy_db_files_found.clone();
        if paths.is_empty() {
            return;
        }
        match delete_legacy_database_files(&paths) {
            Ok(removed) => {
                log::info!(
                    target: "bruk::secrets",
                    "secrets.legacy_databases_deleted count={removed}"
                );
                s.legacy_db_files_found.clear();
                let cleared = s.inbox.clear_secret_problems(|p| {
                    matches!(p.fix, SecretFix::DeleteLegacyDatabases { .. })
                });
                close_notifications(cleared);
                s.sync_inbox_selection_and_drafts();
                s.secrets_error = None;
                s.secrets_feedback = Some(format!("Deleted {removed} legacy database file(s)."));
            }
            Err(err) => {
                log::error!(
                    target: "bruk::secrets",
                    "secrets.legacy_databases_delete_failed err={err}"
                );
                s.secrets_error = Some(format!("Could not delete legacy files: {err}"));
            }
        }
    });
}

// --- 1Password & Generic Command Managers -----------------------------------------------

pub fn set_one_password_enabled(enabled: bool) {
    state().update(|s| {
        s.preferences.secrets.one_password.enabled = enabled;
        s.secrets_error = None;
        s.secrets_feedback = Some(if enabled {
            "1Password secret manager enabled.".to_owned()
        } else {
            "1Password secret manager disabled.".to_owned()
        });
        s.persist_preferences();
    });
}

pub fn set_one_password_binary_path(path: &str) {
    state().update(|s| {
        let trimmed = path.trim();
        s.preferences.secrets.one_password.binary_path = if trimmed.is_empty() {
            "op".to_owned()
        } else {
            trimmed.to_owned()
        };
        s.persist_preferences();
    });
}

pub fn set_one_password_auth_mode(mode: OnePasswordAuthMode) {
    state().update(|s| {
        s.preferences.secrets.one_password.auth_mode = mode;
        s.secrets_error = None;
        s.secrets_feedback = Some(format!("1Password auth mode set to {}.", mode.label()));
        s.persist_preferences();
    });
}

pub fn set_one_password_desktop_account(account: &str) {
    state().update(|s| {
        s.preferences.secrets.one_password.desktop_account = account.trim().to_owned();
        s.persist_preferences();
    });
}

pub fn set_one_password_service_account_token(raw_token: &str) -> bool {
    let mut ok = false;
    state().update(|s| {
        if s.demo_mode {
            s.secrets_error = Some("Disable Demo Mode to save a 1Password token.".to_owned());
            return;
        }
        let secret = SecretString::from(raw_token.trim().to_owned());
        let Some(db_arc) = s.db.as_ref() else {
            s.secrets_error = Some("Database unavailable.".to_owned());
            return;
        };
        let existing_id = s
            .preferences
            .secrets
            .one_password
            .service_account_secret_id
            .clone();
        let res = db_arc.lock().ok().map(|db| {
            if let Some(id) = existing_id.as_deref() {
                db.replace_keychain_secret(s.secret_store.as_ref(), id, &secret, None)
            } else {
                let record = SecretRecord::new_keychain(
                    "1Password Service Account Token",
                    SecretKind::Internal {
                        purpose: InternalPurpose::ManagerAuth {
                            manager: ONE_PASSWORD_MANAGER_ID.to_owned(),
                        },
                    },
                    current_unix_seconds(),
                );
                db.create_keychain_secret(s.secret_store.as_ref(), record, &secret, None)
            }
        });
        match res {
            Some(Ok(saved)) => {
                s.preferences.secrets.one_password.service_account_secret_id = Some(saved.id);
                s.secrets_error = None;
                s.secrets_feedback =
                    Some("Saved 1Password service account token in Keychain.".to_owned());
                s.persist_preferences();
                ok = true;
            }
            Some(Err(err)) => {
                s.secrets_error = Some(err.to_string());
            }
            None => {
                s.secrets_error = Some("Could not lock database.".to_owned());
            }
        }
    });
    ok
}

pub fn test_one_password_connection_async() {
    let job = state().with_untracked(|s| {
        if s.secrets_test_in_progress {
            return None;
        }
        let cfg = s.preferences.secrets.one_password.clone();
        let store = Arc::clone(&s.secret_store);
        let db_id =
            s.db.as_ref()
                .and_then(|arc| arc.lock().ok())
                .and_then(|db| db.ensure_db_instance_id().ok())
                .unwrap_or_default();
        Some((cfg, store, db_id))
    });
    let Some((cfg, store, db_id)) = job else {
        return;
    };
    state().update(|s| {
        s.secrets_test_in_progress = true;
        s.secrets_test_result = None;
    });
    Owner::detached().run(|| {
        spawn(async move {
            let outcome = spawn_blocking_task(move || {
                let mgr = OnePasswordManager::new(cfg, store, db_id);
                let cancel = CancellationToken::new();
                match mgr.status(&cancel) {
                    ManagerStatus::Ready => {
                        Ok("1Password CLI is ready and authenticated.".to_owned())
                    }
                    ManagerStatus::Locked => {
                        Err("1Password CLI is locked. Unlock 1Password or configure a service account token.".to_owned())
                    }
                    ManagerStatus::NotInstalled => {
                        Err("1Password CLI (`op`) was not found on PATH or at the configured binary path.".to_owned())
                    }
                    ManagerStatus::AuthFailed(msg) => Err(if msg.is_empty() {
                        "1Password authentication failed.".to_owned()
                    } else {
                        msg
                    }),
                }
            })
            .await;
            state().update(|s| {
                s.secrets_test_in_progress = false;
                s.secrets_test_result = Some(outcome);
            });
        });
    });
}

pub fn test_one_password_reference_async(reference: &str) {
    let reference = reference.trim().to_owned();
    let job = state().with_untracked(|s| {
        if s.secrets_test_in_progress {
            return None;
        }
        let cfg = s.preferences.secrets.one_password.clone();
        let store = Arc::clone(&s.secret_store);
        let db_id =
            s.db.as_ref()
                .and_then(|arc| arc.lock().ok())
                .and_then(|db| db.ensure_db_instance_id().ok())
                .unwrap_or_default();
        Some((cfg, store, db_id))
    });
    let Some((cfg, store, db_id)) = job else {
        return;
    };
    state().update(|s| {
        s.secrets_test_in_progress = true;
        s.secrets_test_result = None;
    });
    Owner::detached().run(|| {
        spawn(async move {
            let outcome = spawn_blocking_task(move || {
                let mgr = OnePasswordManager::new(cfg, store, db_id);
                let cancel = CancellationToken::new();
                mgr.test_reference(&reference, &cancel)
                    .map(|()| format!("Reference '{reference}' resolved successfully."))
                    .map_err(|failure| failure.to_problem(None).reason)
            })
            .await;
            state().update(|s| {
                s.secrets_test_in_progress = false;
                s.secrets_test_result = Some(outcome);
            });
        });
    });
}

pub fn upsert_command_manager(config: CommandManagerConfig) -> bool {
    let mut ok = false;
    state().update(|s| {
        if config.id.trim().is_empty()
            || config.name.trim().is_empty()
            || config.program.trim().is_empty()
        {
            s.secrets_error =
                Some("Manager ID, display name, and program are required.".to_owned());
            return;
        }
        if config.id.eq_ignore_ascii_case("keychain")
            || config.id.eq_ignore_ascii_case(ONE_PASSWORD_MANAGER_ID)
        {
            s.secrets_error =
                Some("That manager ID is reserved for a built-in provider.".to_owned());
            return;
        }
        let list = &mut s.preferences.secrets.command_managers;
        match list.iter_mut().find(|m| m.id == config.id) {
            Some(existing) => *existing = config.clone(),
            None => list.push(config.clone()),
        }
        s.secrets_error = None;
        s.secrets_feedback = Some(format!("Saved command manager '{}'.", config.name));
        s.persist_preferences();
        ok = true;
    });
    ok
}

pub fn remove_command_manager(id: &str) {
    state().update(|s| {
        let removed_auth: Vec<SecretId> = s
            .preferences
            .secrets
            .command_managers
            .iter()
            .filter(|m| m.id == id)
            .flat_map(|m| m.auth_env.values().cloned())
            .collect();
        s.preferences
            .secrets
            .command_managers
            .retain(|m| m.id != id);
        if !s.demo_mode
            && let Some(db_arc) = s.db.as_ref()
            && let Ok(db) = db_arc.lock()
        {
            for secret_id in removed_auth {
                let _ = db.delete_secret(s.secret_store.as_ref(), &secret_id);
            }
        }
        s.secrets_error = None;
        s.secrets_feedback = Some(format!("Removed command manager '{id}'."));
        s.persist_preferences();
    });
}

pub fn available_secret_managers() -> Vec<(ManagerId, String)> {
    state().with(|s| {
        let mut out = Vec::new();
        if s.preferences.secrets.one_password.enabled {
            out.push((ONE_PASSWORD_MANAGER_ID.to_owned(), "1Password".to_owned()));
        }
        for mgr in &s.preferences.secrets.command_managers {
            if mgr.enabled {
                out.push((mgr.id.clone(), mgr.name.clone()));
            }
        }
        out
    })
}

/// Resumes all `NeedsSetup` items in the active (or specified) project's work queue.
pub fn resume_queued_work_for_project(project_id: ProjectId) {
    let mut resumed_issues = Vec::new();
    state().update(|s| {
        let Some(pname) = s.factory.project(project_id).map(|p| p.name.clone()) else {
            return;
        };
        let queue = s.ensure_queue_mut(&pname);
        resumed_issues = queue.resume_setup_held_items();
        s.persist_work_queue_by_name(&pname);
        let cleared = s.inbox.clear_secret_problems(|p| {
            matches!(p.fix, SecretFix::ResumeQueuedWork { project_id: pid } if pid == project_id)
        });
        close_notifications(cleared);
        s.sync_inbox_selection_and_drafts();
        s.status_message = Some(format!(
            "Resumed {} held issue(s) in {pname}.",
            resumed_issues.len()
        ));
    });
    if !resumed_issues.is_empty() {
        sync_github_issues_for_active_project(false);
    }
}

pub fn resume_work_queue_issue(issue_number: u32) {
    state().update(|s| {
        let pname = s.selected_project_name();
        let queue = s.ensure_queue_mut(&pname);
        if let Some(item) = queue
            .items
            .iter_mut()
            .find(|i| i.issue_number == issue_number)
            && item.stage == WorkQueueStage::NeedsSetup
        {
            item.stage = WorkQueueStage::ReadyForAgents;
            item.status_text = "Queued".to_owned();
        }
        s.persist_work_queue_by_name(&pname);
        s.status_message = Some(format!("Moved #{issue_number} back to Ready for agents."));
    });
}

/// Dispatches a `SecretFix` action from Inbox items or inline run/chat banners.
pub fn apply_secret_fix(project_name: Option<&str>, fix: &SecretFix) {
    if let Some(name) = project_name {
        state().update(|s| {
            if let Some(project) = s.factory.projects().iter().find(|p| p.name == name) {
                s.selected_project = Some(project.id);
            }
        });
    }
    match fix {
        SecretFix::AddSecret { name, project_id } => {
            state().update(|s| {
                s.secrets_prefill_name = Some(name.clone());
                s.secrets_prefill_scope = Some(match project_id {
                    Some(pid) => EnvScope::Project(*pid),
                    None => EnvScope::App {
                        shared_with: Sharing::AllProjects,
                    },
                });
            });
            if project_id.is_some() {
                open_project_settings_tab(ProjectSettingsTab::Environment);
                state().update(|s| {
                    s.project_settings_feedback = Some(format!(
                        "Add secret '{name}' below so agents and MCP servers can use it."
                    ));
                });
            } else {
                open_preferences_tab(PreferencesTab::Secrets);
                state().update(|s| {
                    s.secrets_feedback = Some(format!("Add app secret '{name}' below."));
                });
            }
        }
        SecretFix::EditSecret(secret_id) => {
            let scope_project = state().with_untracked(|s| {
                s.secrets
                    .iter()
                    .find(|r| r.id == *secret_id)
                    .and_then(|r| r.project_id())
            });
            if let Some(pid) = scope_project {
                state().update(|s| s.selected_project = Some(pid));
                open_project_settings_tab(ProjectSettingsTab::Environment);
            } else {
                open_preferences_tab(PreferencesTab::Secrets);
            }
        }
        SecretFix::UnlockManager(mgr_id) | SecretFix::OpenManagerSettings(mgr_id) => {
            open_preferences_tab(PreferencesTab::Secrets);
            if mgr_id == ONE_PASSWORD_MANAGER_ID {
                test_one_password_connection_async();
            }
        }
        SecretFix::OpenSecretsSettings => {
            open_preferences_tab(PreferencesTab::Secrets);
        }
        SecretFix::ResumeQueuedWork { project_id } => {
            resume_queued_work_for_project(*project_id);
        }
        SecretFix::DeleteLegacyDatabases { .. } => {
            delete_legacy_databases_now();
        }
        SecretFix::None => {}
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn build_for_mode_toggles_between_empty_and_demo_without_losing_real_projects() {
        let temp = TempDir::new().unwrap();
        let db_path = temp.path().join("factory.db");
        let db = ProjectDatabase::open(&db_path).unwrap();

        // Default fresh DB has Demo Mode disabled and empty data everywhere.
        let clean = AppState::from_database_result_with_path(
            ProjectDatabase::open(&db_path),
            Some(BrukHome::with_database_path(&db_path)),
            ClaudePathLookup::ExplicitPath(OsString::new()),
            Arc::new(MemoryStore::new()),
            Vec::new(),
        );
        assert!(!clean.demo_mode);
        assert!(clean.factory.projects().is_empty());
        assert!(clean.inbox.pending.is_empty());
        assert!(clean.machines_fleet.machines.is_empty());
        assert!(clean.deploy_watch_entries.is_empty());
        assert!(clean.preferences.openrouter_keys.is_empty());
        assert!(!clean.preferences.claude.connected);
        assert!(clean.preferences.codex.is_none());

        // Enabling Demo Mode populates sample data for visual testing.
        let mut demo_prefs = AppPreferences::empty();
        demo_prefs.demo_mode = true;
        db.save_preferences(&demo_prefs).unwrap();

        let mut demo = AppState::from_database_result_with_path(
            ProjectDatabase::open(&db_path),
            Some(BrukHome::with_database_path(&db_path)),
            ClaudePathLookup::ExplicitPath(OsString::new()),
            Arc::new(MemoryStore::new()),
            Vec::new(),
        );
        assert!(demo.demo_mode);
        assert!(demo.using_sample_projects);
        assert_eq!(demo.factory.projects().len(), 3);
        assert_eq!(demo.inbox.pending.len(), 5);
        assert_eq!(demo.machines_fleet.machines.len(), 5);
        assert_eq!(demo.deploy_watch_entries.len(), 2);
        assert_eq!(demo.preferences.openrouter_keys.len(), 2);
        assert!(demo.preferences.claude.connected);

        // Saving preferences while in Demo Mode must NOT leak sample providers/extensions to SQLite.
        demo.preferences.appearance.mode = AppearanceMode::Dark;
        demo.persist_preferences();
        let saved_in_db = db.load_preferences().unwrap().unwrap();
        assert!(saved_in_db.demo_mode);
        assert_eq!(saved_in_db.appearance.mode, AppearanceMode::Dark);
        assert!(!saved_in_db.claude.connected);
        assert!(saved_in_db.codex.is_none());
        assert!(saved_in_db.openrouter_keys.is_empty());
        assert!(saved_in_db.extensions.entries.is_empty());

        // Registering a real project and disabling Demo Mode preserves the real project with empty queues.
        db.register_project("real-app", "/workspace/real-app", "main", "")
            .unwrap();
        demo_prefs.demo_mode = false;
        db.save_preferences(&demo_prefs).unwrap();

        let restored = AppState::from_database_result_with_path(
            ProjectDatabase::open(&db_path),
            Some(BrukHome::with_database_path(&db_path)),
            ClaudePathLookup::ExplicitPath(OsString::new()),
            Arc::new(MemoryStore::new()),
            Vec::new(),
        );
        assert!(!restored.demo_mode);
        assert!(!restored.using_sample_projects);
        assert_eq!(restored.factory.projects().len(), 1);
        assert_eq!(restored.factory.projects()[0].name, "real-app");
        assert!(restored.inbox.pending.is_empty());
        assert!(restored.machines_fleet.machines.is_empty());
        assert!(restored.queue_for_name("real-app").items.is_empty());
        let real_workflows = restored.workflows_overview_for_name("real-app").workflows;
        assert_eq!(real_workflows.len(), 2);
        assert!(
            real_workflows
                .iter()
                .all(|w| w.origin == Some(DefinitionOrigin::BuiltIn) && w.recent_runs.is_empty())
        );
    }
}
