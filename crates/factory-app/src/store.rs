//! Reactive per-surface application state backed by `factory-core` and SQLite.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    future::Future,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use factory_core::{
    AGENT_WORKING_LABEL, AgentBadge, AgentDef, AgentTaskStepDef, AlwaysAllowScope,
    AnsweredInboxItem, AntigravityCliProbe, AppPreferences, AppearanceMode, AppearanceSettings,
    AttentionRequest, AttentionRequestBody, AttentionRequestKind, BrukCatalog, BrukHome,
    CatalogLoadOptions, ChatContextUsage, ChatEvent, ChatSession, ChatSessionStatus,
    ChatSessionSummary, ChatTurnControl, ChatTurnRequest, ChatUsageDelta, ChatWorkspace,
    ChatWorktree, ChatWorktreeSetup, ClaudeCliProbe, ClaudeSettingValue, ClaudeSpawnOptions,
    ClaudeSpawnPlan, ClaudeSpawnSettings, CodexCliProbe, CodexSettingValue, CodexSpawnOptions,
    CodexSpawnPlan, CodexSpawnSettings, CompositeDef, CoreUpdatePolicy, DashboardDataset,
    DashboardGranularity, DashboardQuery, DashboardSnapshot, DefaultLandingView, DefinitionOrigin,
    DeployWatchEntry, DryRunExecutor, EditorSelection, ExitField, ExitOperator, FactoryState,
    FontPreference, GateDecision, GitHubAuth, GitHubFilterClause, HomeEnvironment, HomeError,
    HomeResolution, InboxFilter, InboxState, IntakeRule, IssueSourceAuthMode, IssueSourceKind,
    LoopExhaustedAction, LoopRepeatMode, LoopWaitBetween, MachineAgentKind, MachinePlatform,
    MachineRecord, MachinesFleet, NewMachineDraft, PersistedWorkflowRun, PreferencesTab, Project,
    ProjectDatabase, ProjectId, ProjectSandboxMode, ProjectSettings, ProjectSettingsTab,
    ProjectWorkQueue, ProjectWorkflowsOverview, ProviderKind, RecentRunStatus, ReviewDetail,
    RunControlHandle, RunDetailTab, RunExecutionState, RunId, RunOutcomeDot, RunStatus,
    RunnerEvent, SkillDef, StepTypeKind, StorageError, ValidationReport, WorkQueueViewMode,
    Workflow, WorkflowDef, WorkflowEditorDocument, WorkflowId, WorkflowNodeDef, WorkflowRecentRun,
    WorkflowRun, WorkflowRunRequest, WorkflowRunViewData, WorkflowTemplateKind,
    WorkflowTriggersDef, WorkflowsFilter, WorkflowsSort, add_github_issue_label,
    apply_runner_event_to_view_data, chat_worktree_branch, clone_repository, current_unix_seconds,
    detect_git_remote_info, display_with_tilde, fetch_antigravity_details, fetch_claude_details,
    fetch_codex_details, fetch_github_open_issues_with_filter, find_claude_setting_spec,
    find_codex_setting_spec, parse_clone_source, probe_antigravity_cli, probe_claude_cli,
    probe_codex_cli, project_dir_name, query_chat_context, remove_chat_worktree,
    remove_github_issue_label, resolve_antigravity_binary, resolve_claude_binary,
    resolve_codex_binary, run_antigravity_login, run_claude_auth_login, run_codex_login,
    spawn_chat_turn, spawn_workflow_run,
};

const GITHUB_PAT_SECRET_KEY: &str = "github_pat";

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
pub(crate) enum ActiveView {
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
pub(crate) enum ClaudePathLookup {
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
    demo_mode: bool,
    using_sample_projects: bool,
    saved_real_preferences: Option<AppPreferences>,
    claude_path_lookup: ClaudePathLookup,
    claude_sign_in_in_progress: bool,
    claude_details_loading: bool,
    codex_sign_in_in_progress: bool,
    codex_details_loading: bool,
    antigravity_sign_in_in_progress: bool,
    antigravity_details_loading: bool,
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
    chat_new_sandbox: Option<ProjectSandboxMode>,
    chat_new_workspace: Option<ChatWorkspace>,
    /// Whether chats show their context-window panel.
    chat_context_open: bool,
    /// Chats whose context window is being read.
    chat_context_loading: BTreeSet<String>,
    /// Why reading a chat's context window failed.
    chat_context_errors: BTreeMap<String, String>,
    sidebar_visible: bool,
    status_message: Option<String>,
    registration_error: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::from_environment(HomeEnvironment::system(), ClaudePathLookup::SystemPath)
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
    ) -> Self {
        match environment {
            Ok(environment) => match environment.resolve() {
                HomeResolution::Ready(home) => Self::from_home(home, claude_path_lookup),
                HomeResolution::NeedsSetup { suggested_root } => {
                    Self::awaiting_home_setup(environment, &suggested_root, claude_path_lookup)
                }
            },
            Err(err) => Self::from_database_result_with_path(
                Err(StorageError::Home(err)),
                None,
                claude_path_lookup,
            ),
        }
    }

    fn from_home(home: BrukHome, claude_path_lookup: ClaudePathLookup) -> Self {
        let result = ProjectDatabase::open_home(&home);
        Self::from_database_result_with_path(result, Some(home), claude_path_lookup)
    }

    fn awaiting_home_setup(
        environment: HomeEnvironment,
        suggested_root: &Path,
        claude_path_lookup: ClaudePathLookup,
    ) -> Self {
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
        result: Result<ProjectDatabase, factory_core::StorageError>,
        home: Option<BrukHome>,
        claude_path_lookup: ClaudePathLookup,
    ) -> Self {
        let (db, db_path_display, persisted_projects, persisted_prefs, status_message, reg_error) =
            match result {
                Ok(db) => {
                    let db_path_display = db
                        .path()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| ":memory:".to_owned());
                    let (projects, mut status_message) = match db.list_projects() {
                        Ok(projects) => (projects, None),
                        Err(err) => (Vec::new(), Some(format!("Failed to load projects: {err}"))),
                    };
                    let preferences = match db.load_preferences() {
                        Ok(Some(prefs)) => prefs,
                        Ok(None) => AppPreferences::empty(),
                        Err(err) => {
                            if status_message.is_none() {
                                status_message = Some(format!("Failed to load preferences: {err}"));
                            }
                            AppPreferences::empty()
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
                Err(err) => (
                    None,
                    "Unavailable".to_owned(),
                    Vec::new(),
                    AppPreferences::empty(),
                    Some(format!("Could not open SQLite database: {err}")),
                    Some(format!("Database unavailable: {err}")),
                ),
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
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_for_mode(
        db: Option<Arc<Mutex<ProjectDatabase>>>,
        db_path_display: String,
        persisted_projects: Vec<Project>,
        persisted_prefs: AppPreferences,
        demo_mode: bool,
        status_message: Option<String>,
        registration_error: Option<String>,
        claude_path_lookup: ClaudePathLookup,
        home: Option<BrukHome>,
    ) -> Self {
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
            let mut clean = persisted_prefs;
            clean.demo_mode = false;
            let path_env = claude_path_lookup.resolve_path_env();
            let probe =
                probe_claude_cli(clean.claude.custom_cli_path.as_deref(), path_env.as_deref());
            clean.apply_claude_probe(&probe, false);
            if let Some(codex) = &clean.codex {
                let codex_probe =
                    probe_codex_cli(codex.custom_cli_path.as_deref(), path_env.as_deref());
                clean.apply_codex_probe(&codex_probe, false);
            }
            if let Some(antigravity) = &clean.antigravity {
                let probe = probe_antigravity_cli(
                    antigravity.custom_cli_path.as_deref(),
                    path_env.as_deref(),
                );
                clean.apply_antigravity_probe(&probe, false);
            }
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
                    ProjectSettings::for_project(&project.name, &path_str, &project.default_branch),
                );
            } else {
                let loaded_settings = db
                    .as_ref()
                    .and_then(|arc| arc.lock().ok())
                    .and_then(|guard| guard.load_project_settings(project.id).ok().flatten());
                let mut settings = loaded_settings.unwrap_or_else(|| {
                    ProjectSettings::empty(&project.name, &path_str, &project.default_branch)
                });
                if (settings.git_remote.trim().is_empty() || settings.github_repo.trim().is_empty())
                    && let Some(detected) = detect_git_remote_info(&project.root_path)
                {
                    let before_repo = settings.github_repo.clone();
                    let before_remote = settings.git_remote.clone();
                    settings.apply_detected_git_remote(&detected);
                    if (settings.github_repo != before_repo || settings.git_remote != before_remote)
                        && let Some(db_arc) = db.as_ref()
                        && let Ok(guard) = db_arc.lock()
                    {
                        let _ = guard.save_project_settings(project.id, &settings);
                    }
                }

                let catalog = load_catalog(home.as_ref(), &project.root_path);
                settings.sync_workflows_from_catalog(&catalog);

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

                work_queues.insert(project.name.clone(), queue);
                workflows_overviews.insert(
                    project.name.clone(),
                    ProjectWorkflowsOverview::from_catalog(&project.name, &catalog),
                );
                catalogs.insert(project.name.clone(), catalog);
                project_settings.insert(project.name.clone(), settings);
            }
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
            let persisted_runs = db
                .as_ref()
                .and_then(|arc| arc.lock().ok())
                .and_then(|guard| guard.list_all_workflow_runs().ok())
                .unwrap_or_default();
            let chat_usage = db
                .as_ref()
                .and_then(|arc| arc.lock().ok())
                .and_then(|guard| guard.list_chat_usage().ok())
                .unwrap_or_default();
            let dashboard = DashboardDataset::from_usage(&persisted_runs, &chat_usage);
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
            load_chat_sessions(db.as_ref())
        };

        let inbox_selected_id = inbox.default_selected_id();
        let (inbox_edited_command, inbox_always_allow_pattern, inbox_question_choice) =
            draft_defaults_for_selection(&inbox, inbox_selected_id.as_deref());
        let selected_machine_id = machines_fleet.default_selected_id();

        Self {
            factory,
            db,
            db_path_display,
            home,
            home_setup: None,
            clone_source_input: String::new(),
            clone_name_input: String::new(),
            clone_in_progress: false,
            demo_mode,
            using_sample_projects,
            saved_real_preferences,
            claude_path_lookup,
            claude_sign_in_in_progress: false,
            claude_details_loading: false,
            codex_sign_in_in_progress: false,
            codex_details_loading: false,
            antigravity_sign_in_in_progress: false,
            antigravity_details_loading: false,
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
            chat_new_sandbox: None,
            chat_new_workspace: None,
            chat_context_open: false,
            chat_context_loading: BTreeSet::new(),
            chat_context_errors: BTreeMap::new(),
            sidebar_visible: true,
            status_message,
            registration_error,
        }
    }

    /// Where new projects are cloned: the custom directory from preferences, or the Bruk
    /// folder's `projects/`.
    fn projects_dir(&self) -> Option<std::path::PathBuf> {
        let general = &self.preferences.general;
        match self.home.as_ref() {
            Some(home) => Some(general.projects_dir(home)),
            None if !general.projects_dir_follows_home() => {
                Some(factory_core::expand_tilde(&general.default_projects_dir))
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
            settings.sync_workflows_from_catalog(&catalog);
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
        {
            let _ = db.save_project_work_queue(project_id, &queue);
        }
    }

    fn persist_active_work_queue(&mut self) {
        let name = self.selected_project_name();
        self.persist_work_queue_by_name(&name);
    }

    fn save_github_pat_for_active_project(&mut self, token: &str) {
        let name = self.selected_project_name();
        let Some(project_id) = self.project_id_by_name(&name) else {
            return;
        };
        if let Some(db_arc) = self.db.as_ref()
            && let Ok(db) = db_arc.lock()
            && let Err(err) =
                db.save_project_secret(project_id, GITHUB_PAT_SECRET_KEY, token.trim())
        {
            self.project_settings_error = Some(format!("Could not save GitHub token: {err}"));
        }
    }

    fn resolve_github_auth_for_project(&self, project_name: &str) -> Result<GitHubAuth, String> {
        let settings = self
            .project_settings
            .get(project_name)
            .cloned()
            .unwrap_or_else(|| self.active_project_settings_clone());
        match settings.github_auth_mode {
            IssueSourceAuthMode::GitHubCli => Ok(GitHubAuth::GitHubCli),
            IssueSourceAuthMode::PersonalToken => {
                let Some(project_id) = self.project_id_by_name(project_name) else {
                    return Err("Project is not persisted in SQLite.".to_owned());
                };
                let token = self
                    .db
                    .as_ref()
                    .and_then(|arc| arc.lock().ok())
                    .and_then(|db| {
                        db.load_project_secret(project_id, GITHUB_PAT_SECRET_KEY)
                            .ok()
                    })
                    .flatten()
                    .filter(|t| !t.trim().is_empty());
                match token {
                    Some(tok) => Ok(GitHubAuth::PersonalToken(tok)),
                    None => Err(
                        "No personal access token is saved for this project. Add a token or switch to GitHub CLI."
                            .to_owned(),
                    ),
                }
            }
        }
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
            self.preferences_error = Some(format!("Could not save preferences: {err}"));
        }
    }

    fn refresh_claude_status(&mut self, explicit_test: bool) -> ClaudeCliProbe {
        let path_env = self.claude_path_lookup.resolve_path_env();
        let probe = probe_claude_cli(
            self.preferences.claude.custom_cli_path.as_deref(),
            path_env.as_deref(),
        );
        self.preferences.apply_claude_probe(&probe, explicit_test);
        probe
    }

    fn refresh_codex_status(&mut self, explicit_test: bool) -> Option<CodexCliProbe> {
        let custom_path = self.preferences.codex.as_ref()?.custom_cli_path.clone();
        let path_env = self.claude_path_lookup.resolve_path_env();
        let probe = probe_codex_cli(custom_path.as_deref(), path_env.as_deref());
        self.preferences.apply_codex_probe(&probe, explicit_test);
        Some(probe)
    }

    fn refresh_antigravity_status(&mut self, explicit_test: bool) -> Option<AntigravityCliProbe> {
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
        DefaultLandingView::Machines => ActiveView::Machines,
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
    crate::fonts::apply_font_tokens(appearance);
}

fn state() -> Signal<AppState> {
    global::<AppState>()
}

/// Initializes the per-surface store from the Bruk folder configured on this machine.
pub(crate) fn init_default() {
    let appearance = state().with_untracked(|s| s.preferences.appearance.clone());
    apply_appearance_settings(&appearance);
    refresh_claude_details_async();
    refresh_codex_details_async();
    refresh_antigravity_details_async();
}

/// Initializes the per-surface store against an explicit SQLite DB path.
pub(crate) fn init_with_database_path(path: &Path) {
    init_with_database_path_and_claude_path(path, ClaudePathLookup::SystemPath);
}

/// Initializes the per-surface store against an explicit SQLite DB path and Claude `PATH` lookup.
pub(crate) fn init_with_database_path_and_claude_path(
    path: &Path,
    claude_path_lookup: ClaudePathLookup,
) {
    let loaded = AppState::from_database_result_with_path(
        ProjectDatabase::open(path),
        Some(BrukHome::with_database_path(path)),
        claude_path_lookup,
    );
    install_state(loaded);
}

/// Initializes the per-surface store by resolving the Bruk folder from an explicit environment
/// (first-launch setup runs when no folder is configured there).
pub(crate) fn init_with_home_environment(
    environment: HomeEnvironment,
    claude_path_lookup: ClaudePathLookup,
) {
    install_state(AppState::from_environment(
        Ok(environment),
        claude_path_lookup,
    ));
}

fn install_state(loaded: AppState) {
    let appearance = loaded.preferences.appearance.clone();
    state().set(loaded);
    apply_appearance_settings(&appearance);
    refresh_claude_details_async();
    refresh_codex_details_async();
    refresh_antigravity_details_async();
}

// --- Bruk folder ------------------------------------------------------------------------

pub(crate) fn needs_home_setup() -> bool {
    state().with(|s| s.home_setup.is_some())
}

pub(crate) fn home_setup_root_input() -> String {
    state().with(|s| {
        s.home_setup
            .as_ref()
            .map(|d| d.root_input.clone())
            .unwrap_or_default()
    })
}

pub(crate) fn set_home_setup_root_input(value: &str) {
    state().update(|s| {
        if let Some(draft) = s.home_setup.as_mut() {
            draft.root_input = value.to_owned();
            draft.error = None;
        }
    });
}

/// Whether the folder typed in setup already holds a Bruk database (it will be reused as-is).
pub(crate) fn home_setup_target_has_database() -> bool {
    state().with(|s| {
        s.home_setup.as_ref().is_some_and(|d| {
            let trimmed = d.root_input.trim();
            !trimmed.is_empty()
                && BrukHome::at(factory_core::expand_tilde(trimmed)).is_initialized()
        })
    })
}

pub(crate) fn home_setup_legacy_database() -> Option<String> {
    state().with(|s| {
        s.home_setup
            .as_ref()
            .and_then(|d| d.legacy_database.as_deref())
            .map(display_with_tilde)
    })
}

pub(crate) fn home_setup_import_legacy() -> bool {
    state().with(|s| s.home_setup.as_ref().is_some_and(|d| d.import_legacy))
}

pub(crate) fn toggle_home_setup_import_legacy() {
    state().update(|s| {
        if let Some(draft) = s.home_setup.as_mut() {
            draft.import_legacy = !draft.import_legacy;
        }
    });
}

pub(crate) fn home_setup_error() -> Option<String> {
    state().with(|s| s.home_setup.as_ref().and_then(|d| d.error.clone()))
}

/// Creates (or adopts) the chosen Bruk folder, then loads the app from it.
pub(crate) fn complete_home_setup() {
    let Some((draft, claude_path_lookup)) = state().with_untracked(|s| {
        s.home_setup
            .clone()
            .map(|draft| (draft, s.claude_path_lookup.clone()))
    }) else {
        return;
    };
    match draft
        .environment
        .set_up(&draft.root_input, draft.import_legacy)
    {
        Ok(setup) => {
            let root_display = display_with_tilde(setup.home.root());
            let mut loaded = AppState::from_home(setup.home, claude_path_lookup);
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
        Err(err) => state().update(|s| {
            if let Some(draft) = s.home_setup.as_mut() {
                draft.error = Some(err.to_string());
            }
        }),
    }
}

/// Paths inside the active Bruk folder: (folder, database, cloned projects, run workspaces).
pub(crate) fn home_paths_display() -> Option<(String, String, String, String)> {
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

pub(crate) fn home_root() -> Option<std::path::PathBuf> {
    state().with_untracked(|s| s.home.as_ref().map(|home| home.root().to_path_buf()))
}

pub(crate) fn home_root_display() -> String {
    state().with(|s| {
        s.home
            .as_ref()
            .map(|home| display_with_tilde(home.root()))
            .unwrap_or_else(|| "Unavailable".to_owned())
    })
}

/// Whether a project's clone lives in the Bruk folder (as opposed to an external clone).
pub(crate) fn project_is_managed(id: ProjectId) -> bool {
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

pub(crate) fn clear_registration_error() {
    state().update(|s| s.registration_error = None);
}

pub(crate) fn clone_source_input() -> String {
    state().with(|s| s.clone_source_input.clone())
}

fn suggested_clone_name(source: &str) -> String {
    parse_clone_source(source)
        .map(|parsed| parsed.suggested_name)
        .unwrap_or_default()
}

/// Updates the repository field, keeping the project name in step with it until the user
/// types a name of their own.
pub(crate) fn set_clone_source_input(value: &str) {
    state().update(|s| {
        let name_follows_source = s.clone_name_input.trim().is_empty()
            || s.clone_name_input == suggested_clone_name(&s.clone_source_input);
        s.clone_source_input = value.to_owned();
        if name_follows_source {
            s.clone_name_input = suggested_clone_name(value);
        }
    });
}

pub(crate) fn clone_name_input() -> String {
    state().with(|s| s.clone_name_input.clone())
}

pub(crate) fn set_clone_name_input(value: &str) {
    state().update(|s| s.clone_name_input = value.to_owned());
}

pub(crate) fn clone_in_progress() -> bool {
    state().with(|s| s.clone_in_progress)
}

pub(crate) fn clone_destination_caption() -> String {
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
pub(crate) fn clone_and_register_project() {
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

pub(crate) fn demo_mode() -> bool {
    state().with(|s| s.demo_mode)
}

pub(crate) fn set_demo_mode(enabled: bool) {
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
            real.demo_mode = false;
            real
        };

        let status = if enabled {
            Some("Demo Mode enabled — showing sample data".to_owned())
        } else {
            Some("Demo Mode disabled".to_owned())
        };

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
        refresh_claude_details_async();
        refresh_codex_details_async();
        refresh_antigravity_details_async();
    }
}

pub(crate) fn toggle_demo_mode() {
    let current = state().with_untracked(|s| s.demo_mode);
    set_demo_mode(!current);
}

pub(crate) fn sidebar_visible() -> bool {
    state().with(|s| s.sidebar_visible)
}

pub(crate) fn toggle_sidebar() {
    state().update(|s| {
        s.sidebar_visible = !s.sidebar_visible;
    });
}

pub(crate) fn active_view() -> ActiveView {
    state().with(|s| s.active_view)
}

pub(crate) fn open_dashboard() {
    state().update(|s| {
        s.active_view = ActiveView::Dashboard;
        s.status_message = None;
    });
}

pub(crate) fn open_inbox() {
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

pub(crate) fn open_inbox_for_request(id: &str) {
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

pub(crate) fn menubar_popover_open() -> bool {
    state().with(|s| s.menubar_popover_open)
}

pub(crate) fn menubar_anchor_bounds() -> Option<Bounds<Points>> {
    state().with(|s| s.menubar_anchor_bounds)
}

pub(crate) fn open_menubar_popover(bounds: Option<Bounds<Points>>) {
    state().update(|s| {
        s.menubar_anchor_bounds = bounds;
        s.menubar_popover_open = true;
    });
}

pub(crate) fn toggle_menubar_popover(bounds: Option<Bounds<Points>>) {
    state().update(|s| {
        if s.menubar_popover_open {
            s.menubar_popover_open = false;
        } else {
            s.menubar_anchor_bounds = bounds;
            s.menubar_popover_open = true;
        }
    });
}

pub(crate) fn close_menubar_popover() {
    state().update(|s| {
        s.menubar_popover_open = false;
    });
}

pub(crate) fn open_machines() {
    state().update(|s| {
        s.active_view = ActiveView::Machines;
        s.status_message = None;
    });
}

pub(crate) fn open_register_project() {
    state().update(|s| {
        s.active_view = ActiveView::RegisterProject;
        s.registration_error = None;
    });
}

pub(crate) fn open_workspace() {
    state().update(|s| {
        if s.selected_project.is_none() {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
        }
        s.active_view = ActiveView::Workspace;
    });
}

pub(crate) fn db_path_display() -> String {
    state().with(|s| s.db_path_display.clone())
}

pub(crate) fn registration_error() -> Option<String> {
    state().with(|s| s.registration_error.clone())
}

pub(crate) fn project_ids() -> Vec<ProjectId> {
    state().with(|s| s.factory.projects().iter().map(|p| p.id).collect())
}

pub(crate) fn recent_projects() -> Vec<(ProjectId, String)> {
    state().with(|s| {
        s.factory
            .projects()
            .iter()
            .map(|p| (p.id, p.name.clone()))
            .collect()
    })
}

pub(crate) fn tracked_project_ids() -> Vec<ProjectId> {
    state().with(|s| {
        if s.using_sample_projects {
            Vec::new()
        } else {
            s.factory.projects().iter().map(|p| p.id).collect()
        }
    })
}

pub(crate) fn project_by_id(id: ProjectId) -> Option<Project> {
    state().with(|s| s.factory.project(id).cloned())
}

pub(crate) fn selected_project_id() -> Option<ProjectId> {
    state().with(|s| s.selected_project)
}

pub(crate) fn selected_project() -> Option<Project> {
    state().with(|s| {
        s.selected_project
            .and_then(|id| s.factory.project(id).cloned())
    })
}

pub(crate) fn select_project(id: ProjectId) {
    state().update(|s| {
        s.selected_project = Some(id);
        s.expanded_projects.insert(id);
        s.active_view = ActiveView::Workspace;
        s.selected_run = s.factory.runs_for_project(id).first().map(|run| run.id);
        s.sync_selected_issue_for_active_project();
        s.status_message = None;
    });
}

pub(crate) fn is_project_expanded(id: ProjectId) -> bool {
    state().with(|s| s.expanded_projects.contains(&id))
}

pub(crate) fn toggle_project_expanded(id: ProjectId) {
    state().update(|s| {
        if !s.expanded_projects.remove(&id) {
            s.expanded_projects.insert(id);
        }
    });
}

pub(crate) fn open_work_queue() {
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

pub(crate) fn open_work_queue_for_project(id: ProjectId) {
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

pub(crate) fn open_project_workflow(project_id: ProjectId, workflow_name: &str) {
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

pub(crate) fn open_run_history_for_project(project_id: ProjectId) {
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

pub(crate) fn active_workflow_run() -> WorkflowRunViewData {
    state().with(AppState::active_workflow_run_clone)
}

pub(crate) fn active_workflow_run_key() -> (String, String) {
    state().with(|s| s.active_workflow_run_key.clone())
}

pub(crate) fn workflow_run_selected_step() -> usize {
    state().with(|s| s.workflow_run_selected_step)
}

pub(crate) fn select_workflow_run_step(index: usize) {
    state().update(|s| {
        let len = s.active_workflow_run_clone().steps.len();
        if len > 0 {
            s.workflow_run_selected_step = index.min(len - 1);
        }
    });
}

pub(crate) fn workflow_run_tab() -> RunDetailTab {
    state().with(|s| s.workflow_run_tab)
}

pub(crate) fn set_workflow_run_tab(tab: RunDetailTab) {
    state().update(|s| {
        s.workflow_run_tab = tab;
    });
}

pub(crate) fn workflow_run_inspector_visible() -> bool {
    state().with(|s| s.workflow_run_inspector_visible)
}

pub(crate) fn toggle_workflow_run_inspector() {
    state().update(|s| {
        s.workflow_run_inspector_visible = !s.workflow_run_inspector_visible;
    });
}

pub(crate) fn workflow_run_command_palette_open() -> bool {
    state().with(|s| s.workflow_run_command_palette_open)
}

pub(crate) fn toggle_workflow_run_command_palette() {
    state().update(|s| {
        s.workflow_run_command_palette_open = !s.workflow_run_command_palette_open;
    });
}

pub(crate) fn set_workflow_run_command_palette_open(open: bool) {
    state().update(|s| {
        s.workflow_run_command_palette_open = open;
    });
}

pub(crate) fn toggle_pause_workflow_run() {
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
            s.status_message = Some(format!("{state_label} run #{run_no} ({workflow})"));
        }
    });
}

pub(crate) fn stop_workflow_run() {
    let mut stop_key = None;
    state().update(|s| {
        let (project, workflow) = s.active_workflow_run_key.clone();
        stop_key = Some((project.clone(), workflow.clone()));
        let run = s.ensure_workflow_run_mut(&project, &workflow);
        if run.stop() {
            let run_no = run.run_number;
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

pub(crate) fn steer_workflow_run_step(instruction: &str) -> bool {
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

pub(crate) fn toggle_workflow_run_budget_pause() {
    state().update(|s| {
        let (project, workflow) = s.active_workflow_run_key.clone();
        let run = s.ensure_workflow_run_mut(&project, &workflow);
        run.toggle_budget_pause();
    });
}

pub(crate) fn approve_workflow_run_review_gate() {
    let mut approve_key = None;
    state().update(|s| {
        let (project, workflow) = s.active_workflow_run_key.clone();
        approve_key = Some((project.clone(), workflow.clone()));
        let run = s.ensure_workflow_run_mut(&project, &workflow);
        if run.approve_review_gate() {
            let run_no = run.run_number;
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

pub(crate) fn edit_active_workflow() {
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

pub(crate) fn deploy_watch_entries() -> Vec<DeployWatchEntry> {
    state().with(|s| s.deploy_watch_entries.clone())
}

pub(crate) fn sidebar_provider_statuses() -> Vec<(String, &'static str, bool)> {
    state().with(|s| {
        let run = s.active_workflow_run_clone();
        let running = !run.is_empty() && run.execution_state == RunExecutionState::Running;
        let active_provider = run.active_provider_name.as_str();
        let mut rows = Vec::new();
        if s.preferences.claude.connected {
            let is_running = running && active_provider == "Claude";
            rows.push((
                "Claude".to_owned(),
                if is_running { "Running" } else { "Ready" },
                is_running,
            ));
        }
        if s.preferences.codex.as_ref().is_some_and(|c| c.connected) {
            let is_running = running && active_provider == "Codex";
            rows.push((
                "Codex".to_owned(),
                if is_running { "Running" } else { "Ready" },
                is_running,
            ));
        }
        if s.preferences
            .antigravity
            .as_ref()
            .is_some_and(|a| a.connected)
        {
            let is_running = running && active_provider == "Antigravity";
            rows.push((
                "Antigravity".to_owned(),
                if is_running { "Running" } else { "Ready" },
                is_running,
            ));
        }
        let openrouter_key = s
            .preferences
            .openrouter_key("openrouter-acme")
            .or_else(|| s.preferences.openrouter_keys.iter().find(|k| k.is_default))
            .or_else(|| s.preferences.openrouter_keys.first());
        if let Some(key) = openrouter_key {
            rows.push((format!("OpenRouter · {}", key.name), "Ready", false));
        }
        rows
    })
}

/// Persists a new project in SQLite and adds it to the reactive factory state.
pub(crate) fn register_project(name: &str, path: &str) -> bool {
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
                    clean.sync_workflows_from_catalog(&catalog);
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
                s.registration_error = Some(message.clone());
                s.status_message = Some(message);
            }
        }
    });
    created
}

/// Opens an existing project matching `path`, or registers it in SQLite and opens its workspace.
pub(crate) fn open_or_register_project_path(path: &Path) -> bool {
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
pub(crate) fn remove_project(id: ProjectId) {
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
            && let Err(err) = db.delete_project(id)
        {
            let message = format!("Failed to remove project: {err}");
            s.registration_error = Some(message.clone());
            s.status_message = Some(message);
            return;
        }

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

/// What a chat's header and composer show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveChatMeta {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) agent_label: String,
    pub(crate) provider: ProviderKind,
    pub(crate) sandbox: ProjectSandboxMode,
    pub(crate) status: ChatSessionStatus,
    pub(crate) message_count: usize,
    pub(crate) total_tokens: u64,
    pub(crate) total_cost_cents: u64,
    pub(crate) workspace: ChatWorkspace,
    pub(crate) worktree: Option<ChatWorktree>,
    /// Tokens in the context window and its size, after the latest turn.
    pub(crate) context: Option<(u64, u64)>,
}

/// What the open chat's context panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveChatContext {
    pub(crate) chat_id: String,
    pub(crate) provider: ProviderKind,
    pub(crate) usage: Option<ChatContextUsage>,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    /// The provider has a session to measure (a turn has run).
    pub(crate) has_session: bool,
    pub(crate) running: bool,
}

/// One row of the open chat's transcript, keyed by chat and position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatEntryRow {
    pub(crate) chat_id: String,
    pub(crate) index: usize,
    pub(crate) provider: ProviderKind,
    pub(crate) entry: factory_core::ChatEntry,
}

impl AppState {
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
                .map(|cx| factory_core::expand_tilde(&cx.config_dir))
                .unwrap_or_else(|| factory_core::expand_tilde("~/.codex")),
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

    fn reset_chat_composer(&mut self) {
        self.chat_new_provider = None;
        self.chat_new_agent = None;
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
        {
            let _ = db.save_chat_session(session);
        }
    }

    /// Records the user's message in the open chat (starting one with the composer's choices
    /// when none is open) and resolves the turn to run.
    fn begin_chat_turn(
        &mut self,
        text: &str,
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
                self.chat_sessions.insert(id.clone(), session);
                self.chat_selected.insert(pid, id.clone());
                self.reset_chat_composer();
                id
            }
        };

        let worktree_dir = self.chat_runtime_dir(&chat_id).join("worktree");
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
        session.push_user_message(text, now);
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
            Ok(request) => Ok((chat_id, request)),
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
    fn chat_turn_request(&self, chat_id: &str, prompt: &str) -> Result<ChatTurnRequest, String> {
        let session = self
            .chat_sessions
            .get(chat_id)
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
        let settings = self.project_settings.get(&project.name);
        let launch = self.provider_launch();
        let project_env = settings
            .map(|settings| {
                settings
                    .env_vars
                    .iter()
                    .filter(|var| !var.key.trim().is_empty())
                    .map(|var| (var.key.clone(), var.value.clone()))
                    .collect()
            })
            .unwrap_or_default();
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
            provider: session.provider,
            agent: session.agent.clone(),
            model: session.model.clone(),
            sandbox: session.sandbox,
            resume_session_id: session.provider_session_id.clone(),
            prompt: prompt.to_owned(),
            working_dir,
            worktree,
            runtime_dir: self.chat_runtime_dir(chat_id),
            catalog,
            project_model_override: settings
                .map(|s| s.model_override.trim().to_owned())
                .filter(|m| !m.is_empty()),
            project_env,
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
        })
    }
}

/// Opens a project's chats, showing the one last open there.
pub(crate) fn open_chats_for_project(id: ProjectId) {
    state().update(|s| {
        s.selected_project = Some(id);
        s.expanded_projects.insert(id);
        s.active_view = ActiveView::Chat;
        s.status_message = None;
    });
}

/// Opens a fresh composer in the selected (or first) project's chats.
pub(crate) fn start_new_chat() {
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

pub(crate) fn select_chat(id: &str) {
    state().update(|s| {
        if let Some(pid) = s.chat_sessions.get(id).map(|chat| chat.project_id) {
            s.selected_project = Some(pid);
            s.chat_selected.insert(pid, id.to_owned());
            s.active_view = ActiveView::Chat;
        }
    });
}

/// The project's chats, most recently active first.
pub(crate) fn chat_summaries_for_project(id: ProjectId) -> Vec<ChatSessionSummary> {
    state().with(|s| {
        let mut chats: Vec<ChatSessionSummary> = s
            .chat_sessions
            .values()
            .filter(|chat| chat.project_id == id)
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

/// How many chats a project has, and how many are working on a reply.
pub(crate) fn chat_counts_for_project(id: ProjectId) -> (usize, usize) {
    state().with(|s| {
        s.chat_sessions
            .values()
            .filter(|chat| chat.project_id == id)
            .fold((0, 0), |(total, running), chat| {
                (total + 1, running + usize::from(chat.is_running()))
            })
    })
}

pub(crate) fn active_chat_id() -> Option<String> {
    state().with(|s| s.active_chat_id().cloned())
}

pub(crate) fn active_chat_meta() -> Option<ActiveChatMeta> {
    state().with(|s| {
        let chat = s.active_chat_id().and_then(|id| s.chat_sessions.get(id))?;
        Some(ActiveChatMeta {
            id: chat.id.clone(),
            title: chat.display_title(),
            agent_label: chat.agent_label(),
            provider: chat.provider,
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
        })
    })
}

pub(crate) fn active_chat_rows() -> Vec<ChatEntryRow> {
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
pub(crate) fn chat_provider_options() -> Vec<ProviderKind> {
    state().with(|s| {
        ProviderKind::CLI_PROVIDERS
            .iter()
            .copied()
            .filter(|p| s.chat_provider_allowed(*p))
            .collect()
    })
}

/// The selected project's agents as `(reference, name)`.
pub(crate) fn chat_agent_options() -> Vec<(String, String)> {
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

pub(crate) fn chat_new_provider() -> ProviderKind {
    state().with(AppState::resolved_chat_new_provider)
}

pub(crate) fn set_chat_new_provider(provider: ProviderKind) {
    state().update(|s| s.chat_new_provider = Some(provider));
}

pub(crate) fn chat_new_agent() -> Option<String> {
    state().with(|s| s.chat_new_agent.clone())
}

/// Picks the agent for the next new chat, adopting the provider and access it declares.
pub(crate) fn set_chat_new_agent(agent: Option<String>) {
    state().update(|s| {
        let resolved = agent
            .as_deref()
            .and_then(|reference| s.active_catalog_clone().resolve_agent(reference).ok());
        if let Some(resolved) = resolved {
            if let Some(provider) = resolved.provider.filter(|p| s.chat_provider_allowed(*p)) {
                s.chat_new_provider = Some(provider);
            }
            if let Some(sandbox) = resolved.sandbox {
                s.chat_new_sandbox = Some(sandbox);
            }
        }
        s.chat_new_agent = agent;
    });
}

pub(crate) fn chat_new_sandbox() -> ProjectSandboxMode {
    state().with(AppState::resolved_chat_new_sandbox)
}

pub(crate) fn set_chat_new_sandbox(mode: ProjectSandboxMode) {
    state().update(|s| s.chat_new_sandbox = Some(mode));
}

/// Changes what the open chat's agent may do, from its next message on.
pub(crate) fn set_active_chat_sandbox(mode: ProjectSandboxMode) {
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
pub(crate) fn send_chat_message(text: &str) -> bool {
    let text = text.trim().to_owned();
    if text.is_empty() {
        return false;
    }
    let now = current_unix_seconds();
    let mut prepared = None;
    state().update(|s| {
        prepared = Some(s.begin_chat_turn(&text, now));
        if let Some(Err(message)) = &prepared {
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
                    // See how the turn left the context window.
                    refresh_chat_context(chat_id.clone());
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

pub(crate) fn chat_new_workspace() -> ChatWorkspace {
    state().with(AppState::resolved_chat_new_workspace)
}

pub(crate) fn set_chat_new_workspace(workspace: ChatWorkspace) {
    state().update(|s| s.chat_new_workspace = Some(workspace));
}

/// Whether the selected project can give a new chat its own worktree.
pub(crate) fn chat_worktree_available() -> bool {
    state().with(AppState::chat_worktree_available)
}

pub(crate) fn chat_context_open() -> bool {
    state().with(|s| s.chat_context_open)
}

/// Shows or hides the context-window panel, reading the open chat's window when it has none.
pub(crate) fn toggle_chat_context() {
    let mut opened = false;
    state().update(|s| {
        s.chat_context_open = !s.chat_context_open;
        opened = s.chat_context_open;
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

/// Reads the open chat's context window again.
pub(crate) fn refresh_active_chat_context() {
    if let Some(id) = active_chat_id() {
        refresh_chat_context(id);
    }
}

pub(crate) fn active_chat_context() -> Option<ActiveChatContext> {
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
        })
    })
}

/// Stops the open chat's running reply.
pub(crate) fn stop_active_chat() {
    let Some(id) = active_chat_id() else {
        return;
    };
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
pub(crate) fn delete_chat(id: &str) {
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

// --- Dashboard state --------------------------------------------------------------------

// --- Work Queue state -------------------------------------------------------------------

pub(crate) fn active_work_queue() -> ProjectWorkQueue {
    state().with(|s| {
        let name = s.selected_project_name();
        s.queue_for_name(&name)
    })
}

pub(crate) fn work_queue_for_project(id: ProjectId) -> ProjectWorkQueue {
    state().with(|s| {
        let name = s
            .factory
            .project(id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        s.queue_for_name(&name)
    })
}

pub(crate) fn work_queue_view_mode() -> WorkQueueViewMode {
    state().with(|s| s.work_queue_view_mode)
}

pub(crate) fn set_work_queue_view_mode(mode: WorkQueueViewMode) {
    state().update(|s| {
        s.work_queue_view_mode = mode;
    });
}

pub(crate) fn selected_work_queue_issue() -> Option<u32> {
    state().with(|s| s.work_queue_selected_issue)
}

pub(crate) fn select_work_queue_issue(issue_number: u32) {
    state().update(|s| {
        s.work_queue_selected_issue = Some(issue_number);
        s.work_queue_inspector_visible = true;
    });
}

pub(crate) fn work_queue_inspector_visible() -> bool {
    state().with(|s| s.work_queue_inspector_visible)
}

pub(crate) fn toggle_work_queue_inspector() {
    state().update(|s| {
        s.work_queue_inspector_visible = !s.work_queue_inspector_visible;
    });
}

pub(crate) fn work_queue_ad_hoc_open() -> bool {
    state().with(|s| s.work_queue_ad_hoc_open)
}

pub(crate) fn set_work_queue_ad_hoc_open(open: bool) {
    state().update(|s| {
        s.work_queue_ad_hoc_open = open;
    });
}

pub(crate) fn open_work_queue_ad_hoc() {
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

pub(crate) fn work_queue_unmatched_expanded() -> bool {
    state().with(|s| s.work_queue_unmatched_expanded)
}

pub(crate) fn work_queue_syncing() -> bool {
    state().with(|s| s.work_queue_syncing)
}

pub(crate) fn toggle_work_queue_unmatched_expanded() {
    state().update(|s| {
        s.work_queue_unmatched_expanded = !s.work_queue_unmatched_expanded;
    });
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
        let auth = s
            .resolve_github_auth_for_project(&settings.project_name)
            .ok()?;
        let path_env = s.claude_path_lookup.resolve_path_env();
        Some((settings.github_repo, auth, path_env))
    });

    if let Some((repo, auth, path_env)) = job {
        Owner::detached().run(|| {
            spawn(async move {
                let result = spawn_blocking_task(move || {
                    if add {
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
                    }
                })
                .await;
                if let Err(err) = result {
                    state().update(|s| {
                        let verb = if add { "add" } else { "remove" };
                        s.status_message = Some(format!(
                            "Could not {verb} '{AGENT_WORKING_LABEL}' label on #{issue_number}: {err}"
                        ));
                    });
                }
            });
        });
    }
}

pub(crate) fn start_work_queue_issue(issue_number: u32) {
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

pub(crate) fn start_next_queued_issue() {
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

pub(crate) fn reassign_work_queue_workflow(issue_number: u32, workflow_name: &str) {
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

pub(crate) fn skip_work_queue_issue(issue_number: u32) {
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

pub(crate) fn approve_work_queue_review(issue_number: u32) {
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

pub(crate) fn request_changes_work_queue_review(issue_number: u32) {
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

pub(crate) fn stop_work_queue_issue(issue_number: u32) {
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

pub(crate) fn requeue_work_queue_issue(issue_number: u32) {
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

pub(crate) fn queue_unmatched_work_issue(issue_number: u32) {
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

pub(crate) fn open_work_queue_issue_url(url: &str) {
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

pub(crate) fn sync_work_queue() {
    sync_github_issues_for_active_project(false);
}

pub(crate) fn test_and_sync_project_github_source() {
    sync_github_issues_for_active_project(true);
}

fn sync_github_issues_for_active_project(from_settings: bool) {
    struct GitHubSyncJob {
        project_name: String,
        raw_repo: String,
        filter_clauses: Vec<GitHubFilterClause>,
        auth: GitHubAuth,
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

        let auth = match s.resolve_github_auth_for_project(&settings.project_name) {
            Ok(auth) => auth,
            Err(err) => {
                s.ensure_project_settings_mut()
                    .apply_github_sync_error(&err);
                s.persist_active_project_settings();
                s.status_message = Some(err.clone());
                if from_settings {
                    s.project_settings_feedback = None;
                    s.project_settings_error = Some(err);
                }
                return;
            }
        };

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
            auth,
            path_env,
        });
    });

    if let Some(GitHubSyncJob {
        project_name,
        raw_repo,
        filter_clauses,
        auth,
        path_env,
    }) = background_job
    {
        Owner::detached().run(|| {
            spawn(async move {
                let result = spawn_blocking_task(move || {
                    fetch_github_open_issues_with_filter(
                        &raw_repo,
                        &filter_clauses,
                        &auth,
                        path_env.as_deref(),
                    )
                })
                .await;

                let mut auto_started_issues = Vec::new();
                state().update(|s| {
                    s.work_queue_syncing = false;
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
                        }
                        Err(err) => {
                            let message = err.to_string();
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

pub(crate) fn launch_ad_hoc_work_run(workflow_name: &str, title: &str) -> bool {
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

pub(crate) fn has_queued_work_issues() -> bool {
    state().with(|s| {
        let name = s.selected_project_name();
        s.queue_for_name(&name).waiting_count() > 0
    })
}

pub(crate) fn dashboard_snapshot() -> DashboardSnapshot {
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

pub(crate) fn dashboard_granularity() -> DashboardGranularity {
    state().with(|s| s.dashboard_granularity)
}

pub(crate) fn set_dashboard_granularity(granularity: DashboardGranularity) {
    state().update(|s| {
        if s.dashboard_granularity != granularity {
            s.dashboard_granularity = granularity;
            s.dashboard_period_offset = 0;
        }
    });
}

pub(crate) fn step_dashboard_period(delta: i32) {
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

pub(crate) fn dashboard_project_filter() -> Option<String> {
    state().with(|s| s.dashboard_project_filter.clone())
}

pub(crate) fn set_dashboard_project_filter(filter: Option<String>) {
    state().update(|s| {
        s.dashboard_project_filter = filter;
    });
}

pub(crate) fn dashboard_filter_options() -> Vec<String> {
    state().with(|s| {
        let tracked: &[Project] = if s.using_sample_projects {
            &[]
        } else {
            s.factory.projects()
        };
        s.dashboard_dataset.available_project_names(tracked)
    })
}

pub(crate) fn tokens_show_table() -> bool {
    state().with(|s| s.tokens_show_table)
}

pub(crate) fn toggle_tokens_show_table() {
    state().update(|s| {
        s.tokens_show_table = !s.tokens_show_table;
    });
}

// --- Inbox & Machines -------------------------------------------------------------------

pub(crate) fn inbox_state() -> InboxState {
    state().with(|s| s.inbox.clone())
}

pub(crate) fn pending_inbox_count() -> usize {
    state().with(|s| s.inbox.pending_count())
}

pub(crate) fn inbox_filter() -> InboxFilter {
    state().with(|s| s.inbox_filter)
}

pub(crate) fn set_inbox_filter(filter: InboxFilter) {
    state().update(|s| {
        s.inbox_filter = filter;
        s.inbox_waiting_rules_open = false;
        s.sync_inbox_selection_and_drafts();
    });
}

pub(crate) fn inbox_selected_id() -> Option<String> {
    state().with(|s| s.inbox_selected_id.clone())
}

pub(crate) fn inbox_selected_pending() -> Option<AttentionRequest> {
    state().with(|s| {
        s.inbox_selected_id
            .as_deref()
            .and_then(|id| s.inbox.pending_by_id(id))
            .cloned()
    })
}

pub(crate) fn inbox_selected_answered() -> Option<AnsweredInboxItem> {
    state().with(|s| {
        s.inbox_selected_id
            .as_deref()
            .and_then(|id| s.inbox.answered_by_id(id))
            .cloned()
    })
}

pub(crate) fn inbox_select_request(id: &str) {
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

pub(crate) fn inbox_select_relative(delta: isize) {
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

pub(crate) fn inbox_answered_expanded() -> bool {
    state().with(|s| s.inbox_answered_expanded)
}

pub(crate) fn toggle_inbox_answered_expanded() {
    state().update(|s| {
        s.inbox_answered_expanded = !s.inbox_answered_expanded;
    });
}

pub(crate) fn inbox_waiting_rules_open() -> bool {
    state().with(|s| s.inbox_waiting_rules_open)
}

pub(crate) fn toggle_inbox_waiting_rules() {
    state().update(|s| {
        s.inbox_waiting_rules_open = !s.inbox_waiting_rules_open;
    });
}

pub(crate) fn inbox_editing_command() -> bool {
    state().with(|s| s.inbox_editing_command)
}

pub(crate) fn toggle_inbox_editing_command() {
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

pub(crate) fn inbox_edited_command() -> String {
    state().with(|s| s.inbox_edited_command.clone())
}

pub(crate) fn set_inbox_edited_command(cmd: String) {
    state().update(|s| {
        s.inbox_edited_command = cmd;
    });
}

pub(crate) fn inbox_always_allow_open() -> bool {
    state().with(|s| s.inbox_always_allow_open)
}

pub(crate) fn toggle_inbox_always_allow_open() {
    state().update(|s| {
        s.inbox_always_allow_open = !s.inbox_always_allow_open;
    });
}

pub(crate) fn inbox_always_allow_pattern() -> String {
    state().with(|s| s.inbox_always_allow_pattern.clone())
}

pub(crate) fn set_inbox_always_allow_pattern(pattern: String) {
    state().update(|s| {
        s.inbox_always_allow_pattern = pattern;
    });
}

pub(crate) fn inbox_note_draft() -> String {
    state().with(|s| s.inbox_note_draft.clone())
}

pub(crate) fn set_inbox_note_draft(note: String) {
    state().update(|s| {
        s.inbox_note_draft = note;
    });
}

pub(crate) fn inbox_question_choice() -> Option<String> {
    state().with(|s| s.inbox_question_choice.clone())
}

pub(crate) fn set_inbox_question_choice(choice: String) {
    state().update(|s| {
        s.inbox_question_choice = Some(choice);
    });
}

pub(crate) fn inbox_feedback() -> Option<String> {
    state().with(|s| s.inbox_feedback.clone())
}

pub(crate) fn inbox_quick_allow(id: &str) {
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

pub(crate) fn inbox_quick_deny(id: &str) {
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

pub(crate) fn inbox_allow_once() {
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

pub(crate) fn inbox_allow_for_run() {
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

pub(crate) fn inbox_always_allow(scope: AlwaysAllowScope) {
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

pub(crate) fn inbox_deny_selected() {
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

pub(crate) fn inbox_answer_question() {
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

pub(crate) fn inbox_let_agent_decide() {
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

pub(crate) fn inbox_approve_review() {
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

pub(crate) fn inbox_request_changes() {
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

pub(crate) fn inbox_grant_more_attempts() {
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

pub(crate) fn inbox_stop_exhausted_run() {
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

pub(crate) fn inbox_primary_answer() {
    let Some(req) = inbox_selected_pending() else {
        return;
    };
    match req.kind {
        AttentionRequestKind::Permission => inbox_allow_once(),
        AttentionRequestKind::Question => inbox_answer_question(),
        AttentionRequestKind::Review => inbox_approve_review(),
        AttentionRequestKind::LoopExhausted => inbox_grant_more_attempts(),
    }
}

pub(crate) fn inbox_secondary_deny_shortcut() {
    let Some(req) = inbox_selected_pending() else {
        return;
    };
    match req.kind {
        AttentionRequestKind::Permission => inbox_deny_selected(),
        AttentionRequestKind::Question => inbox_let_agent_decide(),
        AttentionRequestKind::Review => inbox_request_changes(),
        AttentionRequestKind::LoopExhausted => inbox_stop_exhausted_run(),
    }
}

pub(crate) fn inbox_open_selected_run() {
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

pub(crate) fn toggle_pause_all_agents() {
    state().update(|s| {
        let paused = s.inbox.toggle_pause_all_agents();
        let msg = if paused {
            "Paused all active agents".to_owned()
        } else {
            "Resumed all active agents".to_owned()
        };
        s.inbox_feedback = Some(msg.clone());
        s.status_message = Some(msg);
    });
}

pub(crate) fn toggle_snooze_notifications() {
    state().update(|s| {
        let snoozed = s.inbox.toggle_snooze_notifications();
        let msg = if let Some(mins) = snoozed {
            format!("Snoozed notifications for {mins} minutes")
        } else {
            "Notifications resumed".to_owned()
        };
        s.inbox_feedback = Some(msg.clone());
        s.status_message = Some(msg);
    });
}

pub(crate) fn machines_fleet() -> MachinesFleet {
    state().with(|s| s.machines_fleet.clone())
}

pub(crate) fn machines() -> Vec<MachineRecord> {
    state().with(|s| s.machines_fleet.machines.clone())
}

pub(crate) fn selected_machine_id() -> Option<u64> {
    state().with(|s| {
        s.selected_machine_id
            .filter(|id| s.machines_fleet.machine(*id).is_some())
            .or_else(|| s.machines_fleet.default_selected_id())
    })
}

pub(crate) fn selected_machine() -> Option<MachineRecord> {
    state().with(|s| {
        let id = s
            .selected_machine_id
            .filter(|id| s.machines_fleet.machine(*id).is_some())
            .or_else(|| s.machines_fleet.default_selected_id())?;
        s.machines_fleet.machine(id).cloned()
    })
}

pub(crate) fn select_machine(id: u64) {
    state().update(|s| {
        if s.machines_fleet.machine(id).is_some() {
            s.selected_machine_id = Some(id);
            s.machines_changes_expanded = false;
            s.machines_settings_open = false;
            s.status_message = None;
        }
    });
}

pub(crate) fn disconnect_machine(id: u64) {
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

pub(crate) fn reconnect_machine(id: u64) {
    state().update(|s| match s.machines_fleet.reconnect_machine(id) {
        Ok(machine) => {
            s.status_message = Some(format!("Reconnected machine '{}'", machine.name));
        }
        Err(err) => {
            s.status_message = Some(err.to_string());
        }
    });
}

pub(crate) fn open_machine_in_terminal(id: u64) {
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

pub(crate) fn update_machine_core(id: u64) {
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

pub(crate) fn update_all_suggested_cores() {
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

pub(crate) fn check_bruk_update() {
    state().update(|s| {
        s.status_message = Some(
            "Bruk 1.0.0 (data schema 8) is available in Software Update — install to resume syncing gpu-01."
                .to_owned(),
        );
    });
}

pub(crate) fn add_machine(draft: NewMachineDraft) -> Result<(), String> {
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

pub(crate) fn toggle_machine_project_access(id: u64, project_name: &str) {
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

pub(crate) fn remove_machine(id: u64) {
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

pub(crate) fn machines_changes_expanded() -> bool {
    state().with(|s| s.machines_changes_expanded)
}

pub(crate) fn toggle_machines_changes_expanded() {
    state().update(|s| {
        s.machines_changes_expanded = !s.machines_changes_expanded;
    });
}

pub(crate) fn machines_update_cores_open() -> bool {
    state().with(|s| s.machines_update_cores_open)
}

pub(crate) fn set_machines_update_cores_open(open: bool) {
    state().update(|s| {
        s.machines_update_cores_open = open;
        if open {
            s.machines_add_drawer_open = false;
        }
    });
}

pub(crate) fn machines_add_drawer_open() -> bool {
    state().with(|s| s.machines_add_drawer_open)
}

pub(crate) fn set_machines_add_drawer_open(open: bool) {
    state().update(|s| {
        s.machines_add_drawer_open = open;
        if open {
            s.machines_update_cores_open = false;
        }
    });
}

pub(crate) fn machines_settings_open() -> bool {
    state().with(|s| s.machines_settings_open)
}

pub(crate) fn toggle_machines_settings_open() {
    state().update(|s| {
        s.machines_settings_open = !s.machines_settings_open;
    });
}

pub(crate) fn open_machine_session(project_name: &str, workflow_name: &str, run_number: u32) {
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

pub(crate) fn workflow_ids() -> Vec<WorkflowId> {
    state().with(|s| s.factory.workflows().iter().map(|w| w.id).collect())
}

pub(crate) fn workflow_by_id(id: WorkflowId) -> Option<Workflow> {
    state().with(|s| s.factory.workflow(id).cloned())
}

pub(crate) fn selected_workflow_id() -> Option<WorkflowId> {
    state().with(|s| s.selected_workflow)
}

pub(crate) fn selected_workflow() -> Option<Workflow> {
    state().with(|s| {
        s.selected_workflow
            .and_then(|id| s.factory.workflow(id).cloned())
    })
}

pub(crate) fn select_workflow(id: WorkflowId) {
    state().update(|s| {
        s.selected_workflow = Some(id);
        s.status_message = None;
    });
}

pub(crate) fn run_ids_for_selected_project() -> Vec<RunId> {
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

pub(crate) fn run_by_id(id: RunId) -> Option<WorkflowRun> {
    state().with(|s| s.factory.run(id).cloned())
}

pub(crate) fn selected_run_id() -> Option<RunId> {
    state().with(|s| s.selected_run)
}

pub(crate) fn selected_run() -> Option<WorkflowRun> {
    state().with(|s| s.selected_run.and_then(|id| s.factory.run(id).cloned()))
}

pub(crate) fn select_run(id: RunId) {
    state().update(|s| {
        s.selected_run = Some(id);
    });
}

pub(crate) fn launch_run(task_prompt: &str) -> bool {
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

pub(crate) fn advance_selected_run() {
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

pub(crate) fn approve_selected_run_step() {
    let is_waiting = state().with_untracked(|s| {
        s.selected_run
            .and_then(|id| s.factory.run(id))
            .is_some_and(|run| run.status == RunStatus::WaitingForApproval)
    });
    if is_waiting {
        advance_selected_run();
    }
}

pub(crate) fn cancel_selected_run() {
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

pub(crate) fn status_message() -> Option<String> {
    state().with(|s| s.status_message.clone())
}

pub(crate) fn set_status_message(message: impl Into<String>) {
    let message = message.into();
    state().update(|s| {
        s.status_message = Some(message);
    });
}

// --- Preferences Window & Settings ------------------------------------------------------

pub(crate) fn preferences_open() -> bool {
    state().with(|s| s.preferences_open)
}

pub(crate) fn preferences_focused() -> bool {
    state().with(|s| s.preferences_focused)
}

pub(crate) fn set_preferences_focused(focused: bool) {
    state().update(|s| {
        s.preferences_focused = focused;
    });
}

pub(crate) fn open_preferences() {
    state().update(|s| {
        s.preferences_open = true;
        s.preferences_focused = true;
        s.preferences_error = None;
    });
}

pub(crate) fn open_preferences_tab(tab: PreferencesTab) {
    state().update(|s| {
        s.preferences_tab = tab;
        s.preferences_open = true;
        s.preferences_focused = true;
        s.preferences_error = None;
        s.preferences_feedback = None;
    });
}

pub(crate) fn close_preferences() {
    state().update(|s| {
        s.preferences_open = false;
        s.preferences_focused = false;
        s.preferences_error = None;
        s.preferences_feedback = None;
    });
}

pub(crate) fn preferences_tab() -> PreferencesTab {
    state().with(|s| s.preferences_tab)
}

pub(crate) fn set_preferences_tab(tab: PreferencesTab) {
    state().update(|s| {
        s.preferences_tab = tab;
        s.preferences_error = None;
        s.preferences_feedback = None;
    });
}

pub(crate) fn preferences() -> AppPreferences {
    state().with(|s| s.preferences.clone())
}

pub(crate) fn selected_provider_id() -> String {
    state().with(|s| s.selected_provider_id.clone())
}

pub(crate) fn select_provider(id: String) {
    state().update(|s| {
        s.selected_provider_id = id;
        s.preferences_error = None;
        s.preferences_feedback = None;
    });
}

pub(crate) fn preferences_error() -> Option<String> {
    state().with(|s| s.preferences_error.clone())
}

pub(crate) fn preferences_feedback() -> Option<String> {
    state().with(|s| s.preferences_feedback.clone())
}

pub(crate) fn clear_preferences_messages() {
    state().update(|s| {
        s.preferences_error = None;
        s.preferences_feedback = None;
    });
}

pub(crate) fn test_selected_provider() {
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

pub(crate) fn add_openrouter_key(
    name: &str,
    raw_api_key: &str,
    monthly_cap: &str,
    default_model: &str,
) -> bool {
    let mut added = false;
    state().update(|s| {
        match s
            .preferences
            .add_openrouter_key(name, raw_api_key, monthly_cap, default_model)
        {
            Ok(new_id) => {
                s.selected_provider_id = new_id;
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Added OpenRouter key '{}'.", name.trim()));
                s.persist_preferences();
                added = true;
            }
            Err(err) => {
                s.preferences_error = Some(err.to_string());
            }
        }
    });
    added
}

pub(crate) fn replace_selected_openrouter_key(raw_api_key: &str) -> bool {
    let mut updated = false;
    state().update(|s| {
        let id = s.selected_provider_id.clone();
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

pub(crate) fn update_selected_openrouter_cap(monthly_cap_input: &str) -> bool {
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

pub(crate) fn update_selected_provider_model(model: &str) -> bool {
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

pub(crate) fn make_selected_openrouter_default() {
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

pub(crate) fn rename_selected_openrouter_key(new_name: &str) -> bool {
    let mut renamed = false;
    state().update(|s| {
        let id = s.selected_provider_id.clone();
        match s.preferences.rename_openrouter_key(&id, new_name) {
            Ok(()) => {
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

pub(crate) fn remove_selected_openrouter_key() {
    state().update(|s| {
        let id = s.selected_provider_id.clone();
        match s.preferences.remove_openrouter_key(&id) {
            Ok(next_selected) => {
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

pub(crate) fn add_codex_account(cli_binary: &str, default_model: &str) -> bool {
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

pub(crate) fn remove_codex_account() {
    state().update(|s| match s.preferences.remove_codex_account() {
        Ok(next_selected) => {
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

pub(crate) fn add_antigravity_account(cli_binary: &str, default_model: &str) -> bool {
    let mut added = false;
    let mut should_refresh_details = false;
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
                    should_refresh_details = true;
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
    if should_refresh_details {
        refresh_antigravity_details_async();
    }
    added
}

pub(crate) fn remove_antigravity_account() {
    state().update(|s| match s.preferences.remove_antigravity_account() {
        Ok(next_selected) => {
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

pub(crate) fn antigravity_sign_in_in_progress() -> bool {
    state().with(|s| s.antigravity_sign_in_in_progress)
}

pub(crate) fn antigravity_details_loading() -> bool {
    state().with(|s| s.antigravity_details_loading)
}

/// Lists the signed-in Antigravity account's models on a background thread.
pub(crate) fn refresh_antigravity_details_async() {
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
        Owner::detached().run(|| {
            spawn(async move {
                let details =
                    spawn_blocking_task(move || fetch_antigravity_details(&binary_path)).await;
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
            });
        });
    }
}

/// Updates or clears the custom Antigravity CLI binary path and re-probes its login.
///
/// Returns `true` if the path was accepted (or cleared back to default `PATH`), `false` if the
/// entered path could not be resolved to an executable binary.
pub(crate) fn update_antigravity_cli_path(raw_path: &str) -> bool {
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
        refresh_antigravity_details_async();
    }
    updated
}

/// Clears any custom Antigravity CLI binary path and reverts to default `PATH` discovery.
pub(crate) fn reset_antigravity_cli_path() {
    let _ = update_antigravity_cli_path("");
}

/// Toggles visibility of an Antigravity model family (e.g. `"Flash"`) in the model picker.
pub(crate) fn toggle_antigravity_model_family(family: &str) {
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
pub(crate) fn sign_in_antigravity() {
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
                    refresh_antigravity_details_async();
                }
            });
        });
    }
}

pub(crate) fn claude_sign_in_in_progress() -> bool {
    state().with(|s| s.claude_sign_in_in_progress)
}

pub(crate) fn claude_details_loading() -> bool {
    state().with(|s| s.claude_details_loading)
}

pub(crate) fn codex_sign_in_in_progress() -> bool {
    state().with(|s| s.codex_sign_in_in_progress)
}

pub(crate) fn codex_details_loading() -> bool {
    state().with(|s| s.codex_details_loading)
}

/// Fetches the live model catalog and remaining plan usage from the authenticated `codex` CLI
/// on a background thread without blocking the UI.
pub(crate) fn refresh_codex_details_async() {
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
        Owner::detached().run(|| {
            spawn(async move {
                let details = spawn_blocking_task(move || fetch_codex_details(&binary_path)).await;
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
            });
        });
    }
}

/// Fetches the live model catalog and remaining plan usage from the authenticated `claude` CLI
/// on a background thread without blocking the UI.
pub(crate) fn refresh_claude_details_async() {
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
        Owner::detached().run(|| {
            spawn(async move {
                let details = spawn_blocking_task(move || fetch_claude_details(&binary_path)).await;
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
            });
        });
    }
}

/// Updates or clears the custom Claude CLI binary path and re-probes system authentication.
///
/// Returns `true` if the path was accepted (or cleared back to default `PATH`), `false` if the
/// entered path could not be resolved to an executable binary.
pub(crate) fn update_claude_cli_path(raw_path: &str) -> bool {
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
pub(crate) fn reset_claude_cli_path() {
    let _ = update_claude_cli_path("");
}

/// Toggles visibility of a Claude model family (e.g. `"Fable"`) in the model selection view.
pub(crate) fn toggle_claude_model_family(family: &str) {
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
pub(crate) fn claude_spawn_settings() -> ClaudeSpawnSettings {
    state().with(|s| s.preferences.claude_spawn_settings.clone())
}

/// Builds a live spawn preview plan for the current Claude binary, default model, and spawn settings.
pub(crate) fn claude_spawn_preview_plan() -> Result<ClaudeSpawnPlan, String> {
    state().with(|s| {
        s.preferences
            .build_claude_spawn_plan(&ClaudeSpawnOptions::default())
            .map_err(|err| err.to_string())
    })
}

/// Sets or clears (`None`) a boolean Claude spawn setting override.
pub(crate) fn set_claude_spawn_bool(spec_id: &str, value: Option<bool>) {
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
pub(crate) fn set_claude_spawn_choice(spec_id: &str, choice: Option<&str>) {
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
pub(crate) fn set_claude_spawn_text(spec_id: &str, raw: &str) -> bool {
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
pub(crate) fn clear_claude_spawn_setting(spec_id: &str) {
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
pub(crate) fn set_claude_spawn_env_var(key: &str, value: &str) -> bool {
    let mut ok = false;
    state().update(
        |s| match s.preferences.claude_spawn_settings.set_env_var(key, value) {
            Ok(()) => {
                let normalized = key.trim();
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Saved environment variable {normalized}."));
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

/// Removes a configured Claude spawn environment variable.
pub(crate) fn remove_claude_spawn_env_var(key: &str) {
    state().update(|s| {
        if s.preferences.claude_spawn_settings.remove_env_var(key) {
            s.preferences_error = None;
            s.preferences_feedback = Some(format!("Removed environment variable {key}."));
            s.persist_preferences();
        }
    });
}

/// Updates extra raw CLI arguments appended when spawning Claude. Returns `true` on success.
pub(crate) fn set_claude_spawn_extra_cli_args(raw: &str) -> bool {
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
pub(crate) fn set_claude_spawn_extra_settings_json(raw: &str) -> bool {
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
pub(crate) fn reset_all_claude_spawn_settings() {
    state().update(|s| {
        s.preferences.claude_spawn_settings.reset_all();
        s.preferences_error = None;
        s.preferences_feedback =
            Some("Reset all Claude spawn settings to CLI defaults.".to_owned());
        s.persist_preferences();
    });
}

/// Returns the persisted custom Codex spawn settings.
pub(crate) fn codex_spawn_settings() -> CodexSpawnSettings {
    state().with(|s| s.preferences.codex_spawn_settings.clone())
}

/// Builds a live spawn preview plan for the current Codex binary, default model, and spawn settings.
pub(crate) fn codex_spawn_preview_plan() -> Result<CodexSpawnPlan, String> {
    state().with(|s| {
        s.preferences
            .build_codex_spawn_plan(&CodexSpawnOptions::default())
            .map_err(|err| err.to_string())
    })
}

/// Sets or clears (`None`) a boolean Codex spawn setting override.
pub(crate) fn set_codex_spawn_bool(spec_id: &str, value: Option<bool>) {
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
pub(crate) fn set_codex_spawn_choice(spec_id: &str, choice: Option<&str>) {
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
pub(crate) fn set_codex_spawn_text(spec_id: &str, raw: &str) -> bool {
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
pub(crate) fn clear_codex_spawn_setting(spec_id: &str) {
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
pub(crate) fn set_codex_spawn_env_var(key: &str, value: &str) -> bool {
    let mut ok = false;
    state().update(
        |s| match s.preferences.codex_spawn_settings.set_env_var(key, value) {
            Ok(()) => {
                let normalized = key.trim();
                s.preferences_error = None;
                s.preferences_feedback = Some(format!("Saved environment variable {normalized}."));
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

/// Removes a configured Codex spawn environment variable.
pub(crate) fn remove_codex_spawn_env_var(key: &str) {
    state().update(|s| {
        if s.preferences.codex_spawn_settings.remove_env_var(key) {
            s.preferences_error = None;
            s.preferences_feedback = Some(format!("Removed environment variable {key}."));
            s.persist_preferences();
        }
    });
}

/// Updates extra raw CLI arguments appended when spawning Codex. Returns `true` on success.
pub(crate) fn set_codex_spawn_extra_cli_args(raw: &str) -> bool {
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
pub(crate) fn set_codex_spawn_extra_config_overrides(raw: &str) -> bool {
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
pub(crate) fn reset_all_codex_spawn_settings() {
    state().update(|s| {
        s.preferences.codex_spawn_settings.reset_all();
        s.preferences_error = None;
        s.preferences_feedback = Some("Reset all Codex spawn settings to CLI defaults.".to_owned());
        s.persist_preferences();
    });
}

/// Launches `claude auth login` on a background thread and refreshes system login status when done.
pub(crate) fn sign_in_claude() {
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
pub(crate) fn update_codex_cli_path(raw_path: &str) -> bool {
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
pub(crate) fn reset_codex_cli_path() {
    let _ = update_codex_cli_path("");
}

/// Toggles visibility of a Codex model family (e.g. `"Luna"`) in the model selection view.
pub(crate) fn toggle_codex_model_family(family: &str) {
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
pub(crate) fn sign_in_codex() {
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

pub(crate) fn set_appearance_mode(mode: AppearanceMode) {
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

pub(crate) fn set_ui_font(font: FontPreference) {
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

pub(crate) fn set_mono_font(font: FontPreference) {
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
    channel: crate::fonts::FontPickChannel,
    listening: bool,
}

fn ensure_font_picker_channel() -> crate::fonts::FontPickChannel {
    let bridge = global::<FontPickerBridge>();
    let (channel, start_listener) = bridge.with_untracked(|b| (b.channel.clone(), !b.listening));
    if start_listener {
        bridge.update(|b| b.listening = true);
        let listener_channel = channel.clone();
        spawn(async move {
            loop {
                let (target, family) = listener_channel.next().await;
                let pref = crate::fonts::normalize_picked_font(target, &family);
                match target {
                    crate::fonts::FontPickerTarget::Ui => set_ui_font(pref),
                    crate::fonts::FontPickerTarget::Mono => set_mono_font(pref),
                }
            }
        });
    }
    channel
}

pub(crate) fn open_ui_font_picker() {
    let current = state().with_untracked(|s| {
        s.preferences
            .appearance
            .ui_font
            .resolve_ui()
            .map(str::to_owned)
    });
    let channel = ensure_font_picker_channel();
    crate::fonts::open_font_panel(crate::fonts::FontPickerTarget::Ui, current, channel);
}

pub(crate) fn open_mono_font_picker() {
    let current = state().with_untracked(|s| {
        s.preferences
            .appearance
            .mono_font
            .resolve_mono()
            .map(str::to_owned)
    });
    let channel = ensure_font_picker_channel();
    crate::fonts::open_font_panel(crate::fonts::FontPickerTarget::Mono, current, channel);
}

pub(crate) fn close_font_picker() {
    crate::fonts::close_font_panel();
}

pub(crate) fn toggle_compact_sidebar() {
    state().update(|s| {
        s.preferences.appearance.compact_sidebar = !s.preferences.appearance.compact_sidebar;
        s.persist_preferences();
    });
}

pub(crate) fn set_monospace_font_size(size: u8) {
    state().update(|s| {
        s.preferences.appearance.monospace_font_size = size.clamp(11, 16);
        s.persist_preferences();
    });
}

pub(crate) fn set_default_landing_view(view: DefaultLandingView) {
    state().update(|s| {
        s.preferences.general.default_landing_view = view;
        s.persist_preferences();
    });
}

/// Sets where new projects are cloned. An empty value, or the Bruk folder's own `projects/`,
/// is stored as "follow the Bruk folder" so it keeps tracking the folder chosen at setup.
pub(crate) fn set_default_projects_dir(dir: &str) {
    state().update(|s| {
        let trimmed = dir.trim();
        let follows_home = trimmed.is_empty()
            || s.home
                .as_ref()
                .is_some_and(|home| factory_core::expand_tilde(trimmed) == home.projects_dir());
        s.preferences.general.default_projects_dir = if follows_home {
            String::new()
        } else {
            trimmed.to_owned()
        };
        s.persist_preferences();
    });
}

/// The folder new projects are cloned into, shortened with `~` for display.
pub(crate) fn default_projects_dir_display() -> String {
    state().with(|s| {
        s.projects_dir()
            .map(|dir| display_with_tilde(&dir))
            .unwrap_or_default()
    })
}

/// The default projects directory, when it exists on disk (used to start folder pickers there).
pub(crate) fn default_projects_dir_path() -> Option<std::path::PathBuf> {
    state()
        .with_untracked(AppState::projects_dir)
        .filter(|dir| dir.is_dir())
}

pub(crate) fn toggle_general_flag(flag: GeneralFlag) {
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
pub(crate) enum GeneralFlag {
    ConfirmBeforeCancel,
    NotifyHumanGate,
    NotifyRunFailure,
    AutoCheckUpdates,
}

pub(crate) fn selected_prefs_machine_id() -> Option<u64> {
    state().with(|s| {
        s.selected_prefs_machine_id
            .filter(|id| s.machines_fleet.machine(*id).is_some())
            .or_else(|| s.machines_fleet.default_selected_id())
    })
}

pub(crate) fn selected_prefs_machine() -> Option<MachineRecord> {
    state().with(|s| {
        let id = s
            .selected_prefs_machine_id
            .filter(|id| s.machines_fleet.machine(*id).is_some())
            .or_else(|| s.machines_fleet.default_selected_id())?;
        s.machines_fleet.machine(id).cloned()
    })
}

pub(crate) fn select_prefs_machine(id: u64) {
    state().update(|s| {
        if s.machines_fleet.machine(id).is_some() {
            s.selected_prefs_machine_id = Some(id);
            s.preferences_error = None;
            s.preferences_feedback = None;
        }
    });
}

pub(crate) fn test_prefs_machine_connection(id: u64) {
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

pub(crate) fn update_prefs_machine_user(id: u64, user: &str) -> bool {
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

pub(crate) fn update_prefs_machine_jump_host(id: u64, jump_host: &str) -> bool {
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

pub(crate) fn update_prefs_machine_ssh_key(
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

pub(crate) fn check_prefs_machine_core(id: u64) {
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

pub(crate) fn set_prefs_machine_core_update_policy(id: u64, policy: CoreUpdatePolicy) {
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

pub(crate) fn set_prefs_machine_agent_signed_in(id: u64, agent: MachineAgentKind, signed_in: bool) {
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

pub(crate) fn update_prefs_machine_max_sessions(id: u64, value_str: &str) -> bool {
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

pub(crate) fn toggle_prefs_machine_project(id: u64, project_name: &str) {
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

pub(crate) fn add_prefs_machine(
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

pub(crate) fn remove_prefs_machine(id: u64) -> bool {
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

pub(crate) fn available_machine_project_names() -> Vec<String> {
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

pub(crate) fn toggle_allow_project_mcp_servers() {
    state().update(|s| {
        s.preferences.extensions.allow_project_mcp_servers =
            !s.preferences.extensions.allow_project_mcp_servers;
        s.persist_preferences();
    });
}

pub(crate) fn toggle_extension(id: &str) {
    state().update(|s| {
        s.preferences.toggle_extension(id);
        s.persist_preferences();
    });
}

pub(crate) fn toggle_verbose_step_logs() {
    state().update(|s| {
        s.preferences.advanced.verbose_step_logs = !s.preferences.advanced.verbose_step_logs;
        s.persist_preferences();
    });
}

pub(crate) fn toggle_keep_worktrees_on_failure() {
    state().update(|s| {
        s.preferences.advanced.keep_worktrees_on_failure =
            !s.preferences.advanced.keep_worktrees_on_failure;
        s.persist_preferences();
    });
}

pub(crate) fn step_log_retention_days(delta: i32) {
    state().update(|s| {
        let next = (s.preferences.advanced.log_retention_days as i32 + delta).clamp(7, 365) as u32;
        s.preferences.advanced.log_retention_days = next;
        s.persist_preferences();
    });
}

pub(crate) fn reset_preferences_to_defaults() {
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

pub(crate) fn open_project_settings() {
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

pub(crate) fn open_project_settings_for_project(id: ProjectId) {
    state().update(|s| {
        s.selected_project = Some(id);
        s.expanded_projects.insert(id);
        s.project_settings_tab = ProjectSettingsTab::IssuesAndIntake;
        s.active_view = ActiveView::ProjectSettings;
        s.project_settings_error = None;
        s.project_settings_feedback = None;
        s.status_message = None;
    });
    main_window().focus();
}

pub(crate) fn open_project_settings_tab(tab: ProjectSettingsTab) {
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

pub(crate) fn project_settings_tab() -> ProjectSettingsTab {
    state().with(|s| s.project_settings_tab)
}

pub(crate) fn set_project_settings_tab(tab: ProjectSettingsTab) {
    state().update(|s| {
        s.project_settings_tab = tab;
        s.project_settings_error = None;
        s.project_settings_feedback = None;
    });
}

pub(crate) fn active_project_settings() -> ProjectSettings {
    state().with(AppState::active_project_settings_clone)
}

pub(crate) fn project_settings_error() -> Option<String> {
    state().with(|s| s.project_settings_error.clone())
}

pub(crate) fn project_settings_feedback() -> Option<String> {
    state().with(|s| s.project_settings_feedback.clone())
}

pub(crate) fn clear_project_settings_messages() {
    state().update(|s| {
        s.project_settings_error = None;
        s.project_settings_feedback = None;
    });
}

pub(crate) fn set_project_issue_source(source: IssueSourceKind) {
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

pub(crate) fn set_project_github_repo(repo: &str) {
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

pub(crate) fn set_project_fallback_workflow(workflow: &str) {
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

pub(crate) fn set_project_itsaplan_workspace(workspace: &str) {
    let workspace = workspace.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().itsaplan_workspace = workspace;
        s.project_settings_error = None;
        s.persist_active_project_settings();
    });
}

pub(crate) fn set_project_itsaplan_agent_member(member: &str) {
    let member = member.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().itsaplan_agent_member = member;
        s.project_settings_error = None;
        s.persist_active_project_settings();
    });
}

pub(crate) fn set_project_github_token(raw_token: &str) -> bool {
    let mut ok = false;
    state().update(|s| {
        let demo = s.demo_mode;
        match s.ensure_project_settings_mut().set_github_token(raw_token) {
            Ok(()) => {
                if !demo {
                    let settings = s.ensure_project_settings_mut();
                    settings.source_connected = false;
                    settings.source_error = None;
                    s.save_github_pat_for_active_project(raw_token);
                }
                s.project_settings_error = None;
                s.project_settings_feedback =
                    Some("Saved personal access token for this project.".to_owned());
                s.persist_active_project_settings();
                ok = true;
            }
            Err(err) => {
                s.project_settings_error = Some(err.to_string());
            }
        }
    });
    ok
}

pub(crate) fn use_project_github_cli() {
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

pub(crate) fn add_project_intake_rule(label: &str, workflow_name: &str) -> bool {
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

pub(crate) fn add_project_intake_filter(clause: GitHubFilterClause) -> bool {
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

pub(crate) fn remove_project_intake_filter(index: usize) {
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

pub(crate) fn add_project_intake_rule_with_clauses(
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

pub(crate) fn remove_project_intake_rule(rule_id: u64) {
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

pub(crate) fn set_project_max_concurrent_runs_str(value: &str) {
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

pub(crate) fn step_project_max_concurrent_runs(delta: i32) {
    state().update(|s| {
        s.ensure_project_settings_mut()
            .step_max_concurrent_runs(delta);
        s.project_settings_error = None;
        s.sync_queue_from_project_settings();
        s.persist_active_project_settings();
        s.persist_active_work_queue();
    });
}

pub(crate) fn toggle_project_auto_start() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.auto_start_matching_work = !settings.auto_start_matching_work;
        s.persist_active_project_settings();
    });
}

pub(crate) fn toggle_project_add_working_label() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.add_working_label = !settings.add_working_label;
        s.persist_active_project_settings();
    });
}

pub(crate) fn toggle_project_comment_summary() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.comment_with_run_summary = !settings.comment_with_run_summary;
        s.persist_active_project_settings();
    });
}

pub(crate) fn toggle_project_link_pr_closes_issue() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.link_pr_closes_issue = !settings.link_pr_closes_issue;
        s.persist_active_project_settings();
    });
}

pub(crate) fn set_project_default_branch(branch: &str) {
    let branch = branch.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().default_branch = branch;
        s.persist_active_project_settings();
    });
}

pub(crate) fn set_project_branch_prefix(prefix: &str) {
    let prefix = prefix.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().branch_prefix = prefix;
        s.persist_active_project_settings();
    });
}

pub(crate) fn toggle_project_isolated_worktrees() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.use_isolated_worktrees = !settings.use_isolated_worktrees;
        s.persist_active_project_settings();
    });
}

pub(crate) fn toggle_project_draft_prs() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.open_prs_as_draft = !settings.open_prs_as_draft;
        s.persist_active_project_settings();
    });
}

pub(crate) fn toggle_project_auto_cleanup_worktrees() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.auto_cleanup_worktrees = !settings.auto_cleanup_worktrees;
        s.persist_active_project_settings();
    });
}

pub(crate) fn toggle_project_archived() {
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

pub(crate) fn set_project_default_provider(provider: ProviderKind) {
    state().update(|s| {
        s.ensure_project_settings_mut().default_provider = provider;
        s.persist_active_project_settings();
    });
}

pub(crate) fn set_project_openrouter_key(key_id: Option<String>) {
    state().update(|s| {
        s.ensure_project_settings_mut().openrouter_key_id = key_id;
        s.persist_active_project_settings();
    });
}

pub(crate) fn set_project_model_override(model: &str) {
    let model = model.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().model_override = model;
        s.persist_active_project_settings();
    });
}

pub(crate) fn toggle_project_workflow(name: &str) {
    let name = name.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().toggle_workflow(&name);
        s.persist_active_project_settings();
    });
}

pub(crate) fn toggle_project_require_review_before_pr() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.require_review_before_pr = !settings.require_review_before_pr;
        s.persist_active_project_settings();
    });
}

pub(crate) fn toggle_project_auto_reload_workflows() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.auto_reload_workflows = !settings.auto_reload_workflows;
        s.persist_active_project_settings();
    });
}

pub(crate) fn add_project_env_var(key: &str, value: &str, is_secret: bool) -> bool {
    let mut added = false;
    state().update(|s| {
        match s
            .ensure_project_settings_mut()
            .add_env_var(key, value, is_secret)
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

pub(crate) fn remove_project_env_var(id: u64) {
    state().update(|s| {
        if s.ensure_project_settings_mut().remove_env_var(id) {
            s.project_settings_error = None;
            s.project_settings_feedback = Some("Removed environment variable.".to_owned());
            s.persist_active_project_settings();
        }
    });
}

pub(crate) fn toggle_project_load_dotenv() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.load_dotenv_local = !settings.load_dotenv_local;
        s.persist_active_project_settings();
    });
}

pub(crate) fn toggle_project_inherit_host_path() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.inherit_host_path = !settings.inherit_host_path;
        s.persist_active_project_settings();
    });
}

pub(crate) fn step_project_max_loop_attempts(delta: i32) {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.max_loop_attempts =
            (settings.max_loop_attempts as i32 + delta).clamp(1, 20) as u32;
        s.persist_active_project_settings();
    });
}

pub(crate) fn step_project_step_timeout(delta: i32) {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.step_timeout_minutes =
            (settings.step_timeout_minutes as i32 + delta).clamp(5, 120) as u32;
        s.persist_active_project_settings();
    });
}

pub(crate) fn step_project_per_run_budget(delta_cents: i64) {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        let next = (settings.per_run_budget_cents as i64 + delta_cents).clamp(100, 10_000);
        settings.per_run_budget_cents = next as u64;
        s.persist_active_project_settings();
    });
}

pub(crate) fn step_project_monthly_budget(delta_cents: i64) {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        let next = (settings.monthly_budget_cents as i64 + delta_cents).clamp(1_000, 500_000);
        settings.monthly_budget_cents = next as u64;
        s.persist_active_project_settings();
    });
}

pub(crate) fn toggle_project_pause_on_budget() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.pause_on_budget_limit = !settings.pause_on_budget_limit;
        s.persist_active_project_settings();
    });
}

pub(crate) fn toggle_project_require_approval_for_ci() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.require_approval_for_ci = !settings.require_approval_for_ci;
        s.persist_active_project_settings();
    });
}

pub(crate) fn set_project_sandbox_mode(mode: ProjectSandboxMode) {
    state().update(|s| {
        s.ensure_project_settings_mut().sandbox_mode = mode;
        s.persist_active_project_settings();
    });
}

pub(crate) fn set_project_preferred_runner(runner: &str) {
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

pub(crate) fn open_workflows_for_project(id: ProjectId) {
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

pub(crate) fn open_new_workflow_drawer() {
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

pub(crate) fn open_import_workflow_drawer() {
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

pub(crate) fn active_workflows_overview() -> ProjectWorkflowsOverview {
    state().with(|s| {
        let name = s.selected_project_name();
        s.workflows_overview_for_name(&name)
    })
}

pub(crate) fn workflows_overview_for_project(id: ProjectId) -> ProjectWorkflowsOverview {
    state().with(|s| {
        let name = s
            .factory
            .project(id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        s.workflows_overview_for_name(&name)
    })
}

pub(crate) fn workflows_filter() -> WorkflowsFilter {
    state().with(|s| s.workflows_filter)
}

pub(crate) fn set_workflows_filter(filter: WorkflowsFilter) {
    state().update(|s| {
        s.workflows_filter = filter;
    });
}

pub(crate) fn workflows_sort() -> WorkflowsSort {
    state().with(|s| s.workflows_sort)
}

pub(crate) fn set_workflows_sort(sort: WorkflowsSort) {
    state().update(|s| {
        s.workflows_sort = sort;
    });
}

pub(crate) fn workflows_search() -> String {
    state().with(|s| s.workflows_search.clone())
}

pub(crate) fn set_workflows_search(query: &str) {
    let query = query.to_owned();
    state().update(|s| {
        s.workflows_search = query;
    });
}

pub(crate) fn reset_workflows_filters() {
    state().update(|s| {
        s.workflows_filter = WorkflowsFilter::All;
        s.workflows_search.clear();
    });
}

pub(crate) fn selected_workflow_overview_id() -> Option<String> {
    state().with(|s| s.workflows_selected_id.clone())
}

pub(crate) fn select_workflow_overview(id: &str) {
    let id = id.to_owned();
    state().update(|s| {
        s.workflows_selected_id = Some(id);
        s.workflows_inspector_visible = true;
        s.workflows_editing_selected = false;
        s.workflows_ad_hoc_open = false;
        s.workflows_more_menu_open = false;
    });
}

pub(crate) fn workflows_inspector_visible() -> bool {
    state().with(|s| s.workflows_inspector_visible)
}

pub(crate) fn toggle_workflows_inspector() {
    state().update(|s| {
        s.workflows_inspector_visible = !s.workflows_inspector_visible;
    });
}

pub(crate) fn workflows_new_drawer_open() -> bool {
    state().with(|s| s.workflows_new_drawer_open)
}

pub(crate) fn set_workflows_new_drawer_open(open: bool) {
    state().update(|s| {
        s.workflows_new_drawer_open = open;
        if open {
            s.workflows_import_drawer_open = false;
        }
    });
}

pub(crate) fn workflows_import_drawer_open() -> bool {
    state().with(|s| s.workflows_import_drawer_open)
}

pub(crate) fn set_workflows_import_drawer_open(open: bool) {
    state().update(|s| {
        s.workflows_import_drawer_open = open;
        if open {
            s.workflows_new_drawer_open = false;
        }
    });
}

pub(crate) fn workflows_editing_selected() -> bool {
    state().with(|s| s.workflows_editing_selected)
}

pub(crate) fn set_workflows_editing_selected(editing: bool) {
    state().update(|s| {
        s.workflows_editing_selected = editing;
        if editing {
            s.workflows_ad_hoc_open = false;
            s.workflows_more_menu_open = false;
        }
    });
}

pub(crate) fn workflows_ad_hoc_open() -> bool {
    state().with(|s| s.workflows_ad_hoc_open)
}

pub(crate) fn set_workflows_ad_hoc_open(open: bool) {
    state().update(|s| {
        s.workflows_ad_hoc_open = open;
        if open {
            s.workflows_editing_selected = false;
            s.workflows_more_menu_open = false;
        }
    });
}

pub(crate) fn workflows_more_menu_open() -> bool {
    state().with(|s| s.workflows_more_menu_open)
}

pub(crate) fn set_workflows_more_menu_open(open: bool) {
    state().update(|s| {
        s.workflows_more_menu_open = open;
        if open {
            s.workflows_editing_selected = false;
            s.workflows_ad_hoc_open = false;
        }
    });
}

pub(crate) fn reload_active_project_catalog() {
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

pub(crate) fn active_catalog_agents() -> Vec<AgentDef> {
    state().with(|s| {
        s.active_catalog_clone()
            .effective_agents()
            .into_iter()
            .cloned()
            .collect()
    })
}

pub(crate) fn active_catalog_composites() -> Vec<CompositeDef> {
    state().with(|s| {
        s.active_catalog_clone()
            .effective_composites()
            .into_iter()
            .cloned()
            .collect()
    })
}

#[allow(dead_code)]
pub(crate) fn active_catalog_skills() -> Vec<SkillDef> {
    state().with(|s| {
        s.active_catalog_clone()
            .effective_skills()
            .into_iter()
            .cloned()
            .collect()
    })
}

pub(crate) fn create_overview_workflow(
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

pub(crate) fn create_workflow_from_template(template: WorkflowTemplateKind) {
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

pub(crate) fn import_overview_workflow(file_path: &str) -> bool {
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

pub(crate) fn update_selected_overview_workflow(
    id: &str,
    name: &str,
    description: &str,
    is_draft: bool,
) {
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

pub(crate) fn launch_overview_ad_hoc_run(id: &str, summary: &str) -> bool {
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

pub(crate) fn duplicate_overview_workflow(id: &str) {
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

pub(crate) fn duplicate_selected_overview_workflow() {
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

pub(crate) fn toggle_overview_workflow_draft(id: &str) {
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

pub(crate) fn export_overview_workflow(id: &str) {
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

pub(crate) fn export_selected_overview_workflow() {
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

pub(crate) fn delete_overview_workflow(id: &str) {
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

pub(crate) fn open_workflow_run_from_overview(workflow_name: &str) {
    let project_id = state().with_untracked(|s| {
        s.selected_project
            .or_else(|| s.factory.projects().first().map(|p| p.id))
    });
    if let Some(pid) = project_id {
        open_project_workflow(pid, workflow_name);
    }
}

// --- Visual Workflow Editor state -------------------------------------------------------

pub(crate) fn open_workflow_editor_from_overview(workflow_id: &str) {
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

pub(crate) fn open_workflow_editor_for_selected_overview() {
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

pub(crate) fn close_workflow_editor() {
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

pub(crate) fn open_workflows_from_editor() {
    state().update(|s| {
        s.active_view = ActiveView::Workflows;
        s.workflow_editor_history_open = false;
        s.workflow_editor_ad_hoc_open = false;
        s.workflow_editor_validation = None;
        s.status_message = None;
    });
}

pub(crate) fn active_workflow_editor() -> WorkflowEditorDocument {
    state().with(AppState::active_workflow_editor_clone)
}

pub(crate) fn workflow_editor_palette_search() -> String {
    state().with(|s| s.workflow_editor_palette_search.clone())
}

pub(crate) fn set_workflow_editor_palette_search(query: &str) {
    let query = query.to_owned();
    state().update(|s| {
        s.workflow_editor_palette_search = query;
    });
}

pub(crate) fn workflow_editor_inspector_visible() -> bool {
    state().with(|s| s.workflow_editor_inspector_visible)
}

pub(crate) fn toggle_workflow_editor_inspector() {
    state().update(|s| {
        s.workflow_editor_inspector_visible = !s.workflow_editor_inspector_visible;
    });
}

pub(crate) fn workflow_editor_history_open() -> bool {
    state().with(|s| s.workflow_editor_history_open)
}

pub(crate) fn toggle_workflow_editor_history() {
    state().update(|s| {
        s.workflow_editor_history_open = !s.workflow_editor_history_open;
        if s.workflow_editor_history_open {
            s.workflow_editor_ad_hoc_open = false;
        }
    });
}

pub(crate) fn workflow_editor_ad_hoc_open() -> bool {
    state().with(|s| s.workflow_editor_ad_hoc_open)
}

pub(crate) fn set_workflow_editor_ad_hoc_open(open: bool) {
    state().update(|s| {
        s.workflow_editor_ad_hoc_open = open;
        if open {
            s.workflow_editor_history_open = false;
        }
    });
}

pub(crate) fn workflow_editor_validation() -> Option<ValidationReport> {
    state().with(|s| s.workflow_editor_validation.clone())
}

pub(crate) fn dismiss_workflow_editor_validation() {
    state().update(|s| {
        s.workflow_editor_validation = None;
    });
}

pub(crate) fn select_workflow_editor_node(selection: EditorSelection) {
    state().update(|s| {
        s.ensure_workflow_editor_mut().selected = selection;
        s.workflow_editor_inspector_visible = true;
    });
}

pub(crate) fn add_workflow_editor_step(kind: StepTypeKind) {
    state().update(|s| {
        let doc = s.ensure_workflow_editor_mut();
        doc.add_from_palette(kind);
        let title = kind.title();
        s.workflow_editor_inspector_visible = true;
        s.workflow_editor_validation = None;
        s.status_message = Some(format!("Added '{title}' step to workflow"));
    });
}

pub(crate) fn add_workflow_editor_catalog_composite(composite_id: &str) {
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

pub(crate) fn update_workflow_editor_step_agent(step_id: &str, agent_ref: Option<&str>) {
    let step_id = step_id.to_owned();
    let agent_owned = agent_ref.map(str::to_owned);
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .update_step_agent(&step_id, agent_owned.as_deref());
    });
}

pub(crate) fn update_workflow_editor_step_skills(step_id: &str, raw_skills: &str) {
    let step_id = step_id.to_owned();
    let raw = raw_skills.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .update_step_skills(&step_id, &raw);
    });
}

pub(crate) fn remove_workflow_editor_step(step_id: &str) {
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

pub(crate) fn set_workflow_editor_loop_name(loop_id: &str, name: &str) {
    let loop_id = loop_id.to_owned();
    let name = name.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .set_loop_name(&loop_id, &name);
    });
}

pub(crate) fn set_workflow_editor_loop_repeat(loop_id: &str, mode: LoopRepeatMode) {
    let loop_id = loop_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .set_loop_repeat_mode(&loop_id, mode);
    });
}

pub(crate) fn set_workflow_editor_loop_max_attempts(loop_id: &str, raw: &str) {
    let loop_id = loop_id.to_owned();
    let parsed = raw.trim().parse::<u32>().ok();
    if let Some(value) = parsed {
        state().update(|s| {
            s.ensure_workflow_editor_mut()
                .set_loop_max_attempts(&loop_id, value);
        });
    }
}

pub(crate) fn set_workflow_editor_loop_wait(loop_id: &str, wait: LoopWaitBetween) {
    let loop_id = loop_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .set_loop_wait_between(&loop_id, wait);
    });
}

pub(crate) fn toggle_workflow_editor_loop_pass_previous(loop_id: &str) {
    let loop_id = loop_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .toggle_loop_pass_previous(&loop_id);
    });
}

pub(crate) fn toggle_workflow_editor_loop_discard_changes(loop_id: &str) {
    let loop_id = loop_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .toggle_loop_discard_changes(&loop_id);
    });
}

pub(crate) fn set_workflow_editor_loop_on_exhausted(loop_id: &str, action: LoopExhaustedAction) {
    let loop_id = loop_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .set_loop_on_exhausted(&loop_id, action);
    });
}

pub(crate) fn add_workflow_editor_exit_condition(loop_id: &str) {
    let loop_id = loop_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .add_loop_exit_condition(&loop_id);
    });
}

pub(crate) fn remove_workflow_editor_exit_condition(loop_id: &str, condition_id: &str) {
    let loop_id = loop_id.to_owned();
    let condition_id = condition_id.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .remove_loop_exit_condition(&loop_id, &condition_id);
    });
}

pub(crate) fn update_workflow_editor_exit_condition(
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

pub(crate) fn update_workflow_editor_step_name(step_id: &str, name: &str) {
    let step_id = step_id.to_owned();
    let name = name.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .update_step_name(&step_id, &name);
    });
}

pub(crate) fn update_workflow_editor_step_detail(step_id: &str, detail: &str) {
    let step_id = step_id.to_owned();
    let detail = detail.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .update_step_detail(&step_id, &detail);
    });
}

pub(crate) fn update_workflow_editor_fallback_name(loop_id: &str, name: &str) {
    let loop_id = loop_id.to_owned();
    let name = name.to_owned();
    state().update(|s| {
        s.ensure_workflow_editor_mut()
            .update_fallback_name(&loop_id, &name);
    });
}

pub(crate) fn toggle_workflow_editor_trigger_work_queue() {
    state().update(|s| {
        s.ensure_workflow_editor_mut().toggle_trigger_work_queue();
    });
}

pub(crate) fn toggle_workflow_editor_trigger_ad_hoc() {
    state().update(|s| {
        s.ensure_workflow_editor_mut().toggle_trigger_ad_hoc();
    });
}

pub(crate) fn validate_workflow_editor() {
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

pub(crate) fn save_workflow_editor() {
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

pub(crate) fn launch_editor_ad_hoc_run(summary: &str) -> bool {
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

        let github_auth = s.resolve_github_auth_for_project(project_name).ok();
        let github_repo = if settings.github_repo.trim().is_empty() {
            None
        } else {
            Some(settings.github_repo.clone())
        };

        let mut project_env = BTreeMap::new();
        for ev in &settings.env_vars {
            if !ev.key.trim().is_empty() {
                project_env.insert(ev.key.clone(), ev.value.clone());
            }
        }
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
                github_auth,
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
                runtime_root,
                budget_limit_cents: settings.per_run_budget_cents,
            },
            started_at,
        ))
    });

    let Some((request, started_at)) = setup else {
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

                        if let Some(issue_no) = issue_number {
                            finished_issue_cleanup = Some(issue_no);
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
