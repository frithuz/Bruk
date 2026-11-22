//! Reactive per-surface application state backed by `factory-core` and SQLite.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex},
};

use factory_core::{
    AlwaysAllowScope, AnsweredInboxItem, AppPreferences, AppearanceMode, AttentionRequest,
    AttentionRequestBody, AttentionRequestKind, CoreUpdatePolicy, DashboardDataset,
    DashboardGranularity, DashboardQuery, DashboardSnapshot, DefaultLandingView, DeployWatchEntry,
    DryRunExecutor, EditorSelection, ExitField, ExitOperator, FactoryState, InboxFilter,
    InboxState, IntakeRule, IssueSourceKind, LoopExhaustedAction, LoopRepeatMode, LoopWaitBetween,
    MachineAgentKind, MachinePlatform, MachineRecord, MachinesFleet, NewMachineDraft,
    PreferencesTab, Project, ProjectDatabase, ProjectId, ProjectSandboxMode, ProjectSettings,
    ProjectSettingsTab, ProjectWorkQueue, ProjectWorkflowsOverview, ProviderKind, RunDetailTab,
    RunExecutionState, RunId, RunStatus, StepTypeKind, ValidationReport, WorkQueueViewMode,
    Workflow, WorkflowEditorDocument, WorkflowId, WorkflowRun, WorkflowRunViewData,
    WorkflowTemplateKind, WorkflowsFilter, WorkflowsSort,
};
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
    Workspace,
    RegisterProject,
}

#[derive(Clone)]
struct AppState {
    factory: FactoryState,
    db: Option<Arc<Mutex<ProjectDatabase>>>,
    db_path_display: String,
    using_sample_projects: bool,
    active_view: ActiveView,
    selected_project: Option<ProjectId>,
    expanded_projects: BTreeSet<ProjectId>,
    selected_workflow: Option<WorkflowId>,
    selected_run: Option<RunId>,
    work_queues: BTreeMap<String, ProjectWorkQueue>,
    work_queue_view_mode: WorkQueueViewMode,
    work_queue_selected_issue: Option<u32>,
    work_queue_inspector_visible: bool,
    work_queue_editing_intake: bool,
    work_queue_ad_hoc_open: bool,
    work_queue_unmatched_expanded: bool,
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
    project_settings_open: bool,
    project_settings_focused: bool,
    project_settings_tab: ProjectSettingsTab,
    project_settings_feedback: Option<String>,
    project_settings_error: Option<String>,
    sidebar_visible: bool,
    status_message: Option<String>,
    registration_error: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::from_database_result(ProjectDatabase::open_default())
    }
}

impl AppState {
    fn from_database_result(result: Result<ProjectDatabase, factory_core::StorageError>) -> Self {
        let dashboard_dataset = DashboardDataset::sample();
        let inbox = InboxState::sample();
        let inbox_selected_id = inbox.default_selected_id();
        let (inbox_edited_command, inbox_always_allow_pattern, inbox_question_choice) =
            draft_defaults_for_selection(&inbox, inbox_selected_id.as_deref());
        let machines_fleet = MachinesFleet::sample();
        let selected_machine_id = machines_fleet.default_selected_id();

        match result {
            Ok(db) => {
                let db_path_display = db
                    .path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| ":memory:".to_owned());
                let (persisted_projects, mut status_message) = match db.list_projects() {
                    Ok(projects) => (projects, None),
                    Err(err) => (Vec::new(), Some(format!("Failed to load projects: {err}"))),
                };
                let preferences = match db.load_preferences() {
                    Ok(Some(prefs)) => prefs,
                    Ok(None) => AppPreferences::sample(),
                    Err(err) => {
                        if status_message.is_none() {
                            status_message = Some(format!("Failed to load preferences: {err}"));
                        }
                        AppPreferences::sample()
                    }
                };
                let selected_provider_id = default_selected_provider_id(&preferences);
                let active_view = landing_to_active_view(preferences.general.default_landing_view);
                let using_sample_projects = persisted_projects.is_empty();
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
                let mut workflows_overviews = BTreeMap::new();
                let mut project_settings = BTreeMap::new();
                for project in factory.projects() {
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
                            &project.root_path.display().to_string(),
                            &project.default_branch,
                        ),
                    );
                }
                let work_queue_selected_issue = selected_project
                    .and_then(|pid| factory.project(pid))
                    .and_then(|project| work_queues.get(&project.name))
                    .and_then(ProjectWorkQueue::default_selected_issue);
                let workflows_selected_id = selected_project
                    .and_then(|pid| factory.project(pid))
                    .and_then(|project| workflows_overviews.get(&project.name))
                    .and_then(ProjectWorkflowsOverview::default_selected_id);
                let mut workflow_runs_detail = BTreeMap::new();
                let default_run = WorkflowRunViewData::sample_release_prep();
                let default_step = default_run.active_step_index;
                workflow_runs_detail.insert(
                    ("api-gateway".to_owned(), "Release prep".to_owned()),
                    default_run,
                );
                Self {
                    factory,
                    db: Some(Arc::new(Mutex::new(db))),
                    db_path_display,
                    using_sample_projects,
                    active_view,
                    selected_project,
                    expanded_projects,
                    selected_workflow,
                    selected_run: None,
                    work_queues,
                    work_queue_view_mode: WorkQueueViewMode::Board,
                    work_queue_selected_issue,
                    work_queue_inspector_visible: true,
                    work_queue_editing_intake: false,
                    work_queue_ad_hoc_open: false,
                    work_queue_unmatched_expanded: false,
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
                    workflow_editors: BTreeMap::from([(
                        ("api-gateway".to_owned(), "fix-issue".to_owned()),
                        WorkflowEditorDocument::sample_fix_issue("api-gateway"),
                    )]),
                    active_workflow_editor_key: ("api-gateway".to_owned(), "fix-issue".to_owned()),
                    workflow_editor_return_view: ActiveView::Workflows,
                    workflow_editor_palette_search: String::new(),
                    workflow_editor_inspector_visible: true,
                    workflow_editor_history_open: false,
                    workflow_editor_ad_hoc_open: false,
                    workflow_editor_validation: None,
                    workflow_runs_detail,
                    active_workflow_run_key: ("api-gateway".to_owned(), "Release prep".to_owned()),
                    workflow_run_selected_step: default_step,
                    workflow_run_tab: RunDetailTab::Output,
                    workflow_run_inspector_visible: true,
                    workflow_run_command_palette_open: false,
                    deploy_watch_entries: WorkflowRunViewData::sample_deploy_watch(),
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
                    project_settings_open: false,
                    project_settings_focused: false,
                    project_settings_tab: ProjectSettingsTab::IssuesAndIntake,
                    project_settings_feedback: None,
                    project_settings_error: None,
                    sidebar_visible: true,
                    status_message,
                    registration_error: None,
                }
            }
            Err(err) => {
                let factory = FactoryState::with_projects(DashboardDataset::sample_projects());
                let selected_project = factory.projects().first().map(|project| project.id);
                let selected_workflow = factory.workflows().first().map(|workflow| workflow.id);
                let mut expanded_projects = BTreeSet::new();
                if let Some(pid) = selected_project {
                    expanded_projects.insert(pid);
                }
                let mut work_queues = BTreeMap::new();
                let mut workflows_overviews = BTreeMap::new();
                let mut project_settings = BTreeMap::new();
                for project in factory.projects() {
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
                            &project.root_path.display().to_string(),
                            &project.default_branch,
                        ),
                    );
                }
                let work_queue_selected_issue = selected_project
                    .and_then(|pid| factory.project(pid))
                    .and_then(|project| work_queues.get(&project.name))
                    .and_then(ProjectWorkQueue::default_selected_issue);
                let workflows_selected_id = selected_project
                    .and_then(|pid| factory.project(pid))
                    .and_then(|project| workflows_overviews.get(&project.name))
                    .and_then(ProjectWorkflowsOverview::default_selected_id);
                let mut workflow_runs_detail = BTreeMap::new();
                let default_run = WorkflowRunViewData::sample_release_prep();
                let default_step = default_run.active_step_index;
                workflow_runs_detail.insert(
                    ("api-gateway".to_owned(), "Release prep".to_owned()),
                    default_run,
                );
                let preferences = AppPreferences::sample();
                let selected_provider_id = default_selected_provider_id(&preferences);
                Self {
                    factory,
                    db: None,
                    db_path_display: "Unavailable".to_owned(),
                    using_sample_projects: true,
                    active_view: ActiveView::Dashboard,
                    selected_project,
                    expanded_projects,
                    selected_workflow,
                    selected_run: None,
                    work_queues,
                    work_queue_view_mode: WorkQueueViewMode::Board,
                    work_queue_selected_issue,
                    work_queue_inspector_visible: true,
                    work_queue_editing_intake: false,
                    work_queue_ad_hoc_open: false,
                    work_queue_unmatched_expanded: false,
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
                    workflow_editors: BTreeMap::from([(
                        ("api-gateway".to_owned(), "fix-issue".to_owned()),
                        WorkflowEditorDocument::sample_fix_issue("api-gateway"),
                    )]),
                    active_workflow_editor_key: ("api-gateway".to_owned(), "fix-issue".to_owned()),
                    workflow_editor_return_view: ActiveView::Workflows,
                    workflow_editor_palette_search: String::new(),
                    workflow_editor_inspector_visible: true,
                    workflow_editor_history_open: false,
                    workflow_editor_ad_hoc_open: false,
                    workflow_editor_validation: None,
                    workflow_runs_detail,
                    active_workflow_run_key: ("api-gateway".to_owned(), "Release prep".to_owned()),
                    workflow_run_selected_step: default_step,
                    workflow_run_tab: RunDetailTab::Output,
                    workflow_run_inspector_visible: true,
                    workflow_run_command_palette_open: false,
                    deploy_watch_entries: WorkflowRunViewData::sample_deploy_watch(),
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
                    project_settings_open: false,
                    project_settings_focused: false,
                    project_settings_tab: ProjectSettingsTab::IssuesAndIntake,
                    project_settings_feedback: None,
                    project_settings_error: None,
                    sidebar_visible: true,
                    status_message: Some(format!("Could not open SQLite database: {err}")),
                    registration_error: Some(format!("Database unavailable: {err}")),
                }
            }
        }
    }

    fn selected_project_name(&self) -> String {
        self.selected_project
            .and_then(|id| self.factory.project(id))
            .or_else(|| self.factory.projects().first())
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "api-gateway".to_owned())
    }

    fn ensure_queue_mut(&mut self, project_name: &str) -> &mut ProjectWorkQueue {
        self.work_queues
            .entry(project_name.to_owned())
            .or_insert_with(|| ProjectWorkQueue::for_project(project_name))
    }

    fn queue_for_name(&self, project_name: &str) -> ProjectWorkQueue {
        self.work_queues
            .get(project_name)
            .cloned()
            .unwrap_or_else(|| ProjectWorkQueue::for_project(project_name))
    }

    fn ensure_workflows_overview_mut(
        &mut self,
        project_name: &str,
    ) -> &mut ProjectWorkflowsOverview {
        self.workflows_overviews
            .entry(project_name.to_owned())
            .or_insert_with(|| ProjectWorkflowsOverview::for_project(project_name))
    }

    fn workflows_overview_for_name(&self, project_name: &str) -> ProjectWorkflowsOverview {
        self.workflows_overviews
            .get(project_name)
            .cloned()
            .unwrap_or_else(|| ProjectWorkflowsOverview::for_project(project_name))
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
                (
                    "api-gateway".to_owned(),
                    "/Users/jonathan/src/api-gateway".to_owned(),
                    "main".to_owned(),
                )
            })
    }

    fn ensure_project_settings_mut(&mut self) -> &mut ProjectSettings {
        let (name, path, branch) = self.selected_project_meta();
        self.project_settings
            .entry(name.clone())
            .or_insert_with(|| ProjectSettings::for_project(&name, &path, &branch))
    }

    fn active_project_settings_clone(&self) -> ProjectSettings {
        let (name, path, branch) = self.selected_project_meta();
        self.project_settings
            .get(&name)
            .cloned()
            .unwrap_or_else(|| ProjectSettings::for_project(&name, &path, &branch))
    }

    fn sync_queue_from_project_settings(&mut self) {
        let settings = self.active_project_settings_clone();
        let queue = self.ensure_queue_mut(&settings.project_name);
        queue.max_agent_slots = settings.max_concurrent_runs;
        if !settings.github_repo.trim().is_empty() {
            queue.repo_slug = settings.github_repo.trim().to_owned();
        }
        queue.tracker_name = settings.issue_source.title().to_owned();
        let rules: Vec<IntakeRule> = settings
            .intake_rules
            .iter()
            .filter_map(|rule| {
                rule.label.as_ref().map(|label| IntakeRule {
                    label: label.clone(),
                    workflow_name: rule.workflow_name.clone(),
                })
            })
            .collect();
        let fallback = settings
            .intake_rules
            .iter()
            .find(|rule| rule.is_fallback())
            .map(|rule| rule.workflow_name.clone())
            .unwrap_or_else(|| queue.fallback_workflow.clone());
        queue.update_intake_rules(settings.required_label.clone(), rules, fallback);
    }

    fn ensure_workflow_run_mut(
        &mut self,
        project_name: &str,
        workflow_name: &str,
    ) -> &mut WorkflowRunViewData {
        let key = (project_name.to_owned(), workflow_name.to_owned());
        self.workflow_runs_detail
            .entry(key)
            .or_insert_with(|| WorkflowRunViewData::for_workflow(project_name, workflow_name))
    }

    fn active_workflow_run_clone(&self) -> WorkflowRunViewData {
        let (ref project, ref workflow) = self.active_workflow_run_key;
        self.workflow_runs_detail
            .get(&self.active_workflow_run_key)
            .cloned()
            .unwrap_or_else(|| WorkflowRunViewData::for_workflow(project, workflow))
    }

    fn ensure_workflow_editor_mut(&mut self) -> &mut WorkflowEditorDocument {
        let key = self.active_workflow_editor_key.clone();
        self.workflow_editors
            .entry(key.clone())
            .or_insert_with(|| WorkflowEditorDocument::for_workflow_name(&key.0, &key.1))
    }

    fn active_workflow_editor_clone(&self) -> WorkflowEditorDocument {
        let (ref project, ref wf_id) = self.active_workflow_editor_key;
        self.workflow_editors
            .get(&self.active_workflow_editor_key)
            .cloned()
            .unwrap_or_else(|| WorkflowEditorDocument::for_workflow_name(project, wf_id))
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
        if let Some(db_arc) = self.db.as_ref()
            && let Ok(db) = db_arc.lock()
            && let Err(err) = db.save_preferences(&self.preferences)
        {
            self.preferences_error = Some(format!("Could not save preferences: {err}"));
        }
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

fn apply_appearance_mode(mode: AppearanceMode) {
    let appearance = match mode {
        AppearanceMode::System => Appearance::System,
        AppearanceMode::Light => Appearance::Light,
        AppearanceMode::Dark => Appearance::Dark,
    };
    nsh_core::element::set_color_scheme(appearance);
}

fn state() -> Signal<AppState> {
    global::<AppState>()
}

/// Initializes the per-surface store using the default SQLite DB in the user config folder.
pub(crate) fn init_default() {
    let mode = state().with_untracked(|s| s.preferences.appearance.mode);
    apply_appearance_mode(mode);
}

/// Initializes the per-surface store against an explicit SQLite DB path.
pub(crate) fn init_with_database_path(path: &Path) {
    let loaded = AppState::from_database_result(ProjectDatabase::open(path));
    let mode = loaded.preferences.appearance.mode;
    state().set(loaded);
    apply_appearance_mode(mode);
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
            s.work_queue_editing_intake = false;
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
            .unwrap_or_else(|| "api-gateway".to_owned());
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
    state().update(|s| {
        let (project, workflow) = s.active_workflow_run_key.clone();
        let run = s.ensure_workflow_run_mut(&project, &workflow);
        if run.stop() {
            let run_no = run.run_number;
            s.status_message = Some(format!("Stopped run #{run_no} ({workflow})"));
        }
    });
}

pub(crate) fn steer_workflow_run_step(instruction: &str) -> bool {
    let mut sent = false;
    state().update(|s| {
        let step_index = s.workflow_run_selected_step;
        let (project, workflow) = s.active_workflow_run_key.clone();
        let run = s.ensure_workflow_run_mut(&project, &workflow);
        match run.steer_step(step_index, instruction) {
            Ok(()) => {
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
    state().update(|s| {
        let (project, workflow) = s.active_workflow_run_key.clone();
        let run = s.ensure_workflow_run_mut(&project, &workflow);
        if run.approve_review_gate() {
            let run_no = run.run_number;
            s.workflow_run_selected_step = run.active_step_index;
            s.status_message = Some(format!("Approved review gate for run #{run_no}"));
        }
    });
}

pub(crate) fn edit_active_workflow() {
    state().update(|s| {
        let (project, workflow_name) = s.active_workflow_run_key.clone();
        let overview_entry = s
            .workflows_overview_for_name(&project)
            .workflows
            .into_iter()
            .find(|w| w.name.eq_ignore_ascii_case(&workflow_name));
        let key = if let Some(entry) = overview_entry {
            let wf_id = entry.id.clone();
            let key = (project.clone(), wf_id);
            s.workflow_editors
                .entry(key.clone())
                .or_insert_with(|| WorkflowEditorDocument::from_overview_entry(&project, &entry));
            key
        } else {
            let doc = WorkflowEditorDocument::for_workflow_name(&project, &workflow_name);
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

pub(crate) fn sidebar_provider_statuses() -> Vec<(&'static str, &'static str, bool)> {
    state().with(|s| {
        let run = s.active_workflow_run_clone();
        let running = run.execution_state == RunExecutionState::Running;
        let active_provider = run.active_provider_name.as_str();
        vec![
            (
                "Claude",
                if running && active_provider == "Claude" {
                    "Running"
                } else {
                    "Ready"
                },
                running && active_provider == "Claude",
            ),
            (
                "Codex",
                if running && active_provider == "Codex" {
                    "Running"
                } else {
                    "Ready"
                },
                running && active_provider == "Codex",
            ),
            ("OpenRouter · Acme team", "Ready", false),
        ]
    })
}

/// Persists a new project in SQLite and adds it to the reactive factory state.
pub(crate) fn register_project(name: &str, path: &str) -> bool {
    let mut created = false;
    state().update(|s| {
        let Some(db_arc) = s.db.as_ref() else {
            s.registration_error = Some("SQLite database is not available.".to_owned());
            return;
        };

        let result = match db_arc.lock() {
            Ok(db) => db.register_project(name, path, "main", ""),
            Err(_) => {
                s.registration_error = Some("SQLite database lock poisoned.".to_owned());
                return;
            }
        };

        match result {
            Ok(project) => {
                let id = project.id;
                let saved_name = project.name.clone();
                if s.using_sample_projects {
                    s.factory = FactoryState::with_projects(vec![project]);
                    s.using_sample_projects = false;
                    s.expanded_projects.clear();
                } else {
                    s.factory.insert_project(project);
                }
                s.work_queues
                    .entry(saved_name.clone())
                    .or_insert_with(|| ProjectWorkQueue::for_project(&saved_name));
                s.workflows_overviews
                    .entry(saved_name.clone())
                    .or_insert_with(|| ProjectWorkflowsOverview::for_project(&saved_name));
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
        if s.selected_project == Some(id) {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
            s.selected_run = s
                .selected_project
                .and_then(|pid| s.factory.runs_for_project(pid).first().map(|r| r.id));
        }
        s.registration_error = None;
        if let Some(name) = project_name {
            s.status_message = Some(format!("Removed project '{name}'"));
        }
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
            .unwrap_or_else(|| "api-gateway".to_owned());
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

pub(crate) fn work_queue_editing_intake() -> bool {
    state().with(|s| s.work_queue_editing_intake)
}

pub(crate) fn set_work_queue_editing_intake(editing: bool) {
    state().update(|s| {
        s.work_queue_editing_intake = editing;
        if editing {
            s.work_queue_ad_hoc_open = false;
        }
    });
}

pub(crate) fn open_work_queue_intake_editor() {
    state().update(|s| {
        if s.selected_project.is_none() {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
        }
        if let Some(pid) = s.selected_project {
            s.expanded_projects.insert(pid);
        }
        s.active_view = ActiveView::WorkQueue;
        s.work_queue_editing_intake = true;
        s.work_queue_ad_hoc_open = false;
        s.sync_selected_issue_for_active_project();
    });
}

pub(crate) fn work_queue_ad_hoc_open() -> bool {
    state().with(|s| s.work_queue_ad_hoc_open)
}

pub(crate) fn set_work_queue_ad_hoc_open(open: bool) {
    state().update(|s| {
        s.work_queue_ad_hoc_open = open;
        if open {
            s.work_queue_editing_intake = false;
        }
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
        s.work_queue_editing_intake = false;
        s.sync_selected_issue_for_active_project();
    });
}

pub(crate) fn work_queue_unmatched_expanded() -> bool {
    state().with(|s| s.work_queue_unmatched_expanded)
}

pub(crate) fn toggle_work_queue_unmatched_expanded() {
    state().update(|s| {
        s.work_queue_unmatched_expanded = !s.work_queue_unmatched_expanded;
    });
}

pub(crate) fn start_work_queue_issue(issue_number: u32) {
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        match queue.start_issue(issue_number) {
            Ok(run_number) => {
                s.work_queue_selected_issue = Some(issue_number);
                s.status_message = Some(format!(
                    "Started issue #{issue_number} as run #{run_number}"
                ));
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub(crate) fn start_next_queued_issue() {
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
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub(crate) fn reassign_work_queue_workflow(issue_number: u32, workflow_name: &str) {
    let wf = workflow_name.to_owned();
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        match queue.reassign_workflow(issue_number, &wf) {
            Ok(()) => {
                s.status_message = Some(format!("Assigned #{issue_number} to workflow '{wf}'"));
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
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub(crate) fn stop_work_queue_issue(issue_number: u32) {
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        match queue.stop_in_progress(issue_number) {
            Ok(()) => {
                s.work_queue_selected_issue = Some(issue_number);
                s.status_message = Some(format!("Stopped run for #{issue_number} and re-queued"));
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub(crate) fn requeue_work_queue_issue(issue_number: u32) {
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        match queue.requeue_issue(issue_number) {
            Ok(()) => {
                s.work_queue_selected_issue = Some(issue_number);
                s.status_message = Some(format!("Re-queued issue #{issue_number}"));
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
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
}

pub(crate) fn sync_work_queue() {
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        let slug = queue.repo_slug.clone();
        match queue.sync_issues() {
            Some(imported) => {
                s.work_queue_selected_issue = Some(imported.issue_number);
                s.status_message = Some(format!(
                    "Synced {slug} — queued #{} {}",
                    imported.issue_number, imported.title
                ));
            }
            None => {
                s.status_message = Some(format!("Synced {slug} — work queue is up to date"));
            }
        }
    });
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
                launched = true;
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
    launched
}

pub(crate) fn save_work_queue_intake_rules(
    required_label: String,
    rules: Vec<IntakeRule>,
    fallback_workflow: String,
) {
    state().update(|s| {
        let name = s.selected_project_name();
        let queue = s.ensure_queue_mut(&name);
        queue.update_intake_rules(required_label, rules, fallback_workflow);
        s.work_queue_editing_intake = false;
        s.status_message = Some(format!("Updated intake rules for '{name}'"));
    });
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
    state().update(|s| {
        let Some(id) = s.inbox_selected_id.clone() else {
            return;
        };
        let note = s.inbox_note_draft.clone();
        if let Some(answered) = s.inbox.approve_review(&id, Some(&note)) {
            closed_id = Some(answered.id.clone());
            s.inbox_feedback = Some(format!("Approved review — {}", answered.context_label));
            s.status_message = Some(format!("Approved review for {}", answered.context_label));
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
        nsh_core::element::close_notification(id);
    }
}

pub(crate) fn inbox_request_changes() {
    let mut closed_id = None;
    state().update(|s| {
        let Some(id) = s.inbox_selected_id.clone() else {
            return;
        };
        let note = s.inbox_note_draft.clone();
        if let Some(answered) = s.inbox.request_changes(&id, Some(&note)) {
            closed_id = Some(answered.id.clone());
            s.inbox_feedback = Some(format!("Requested changes — {}", answered.context_label));
            s.status_message = Some(format!("Requested changes for {}", answered.context_label));
            s.sync_inbox_selection_and_drafts();
        }
    });
    if let Some(id) = closed_id {
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
                "Updated Wald core on '{}' to {} (schema {})",
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
                "Updated 1 machine core to Wald {} (schema {})",
                s.machines_fleet.wald_version, s.machines_fleet.wald_schema
            ));
        } else {
            s.status_message = Some(format!(
                "Updated {updated} machine cores to Wald {} (schema {})",
                s.machines_fleet.wald_version, s.machines_fleet.wald_schema
            ));
        }
    });
}

pub(crate) fn check_wald_update() {
    state().update(|s| {
        s.status_message = Some(
            "Wald 1.0.0 (data schema 8) is available in Software Update — install to resume syncing gpu-01."
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
    state().update(|s| {
        let id = s.selected_provider_id.clone();
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

pub(crate) fn set_cli_account_connected(id: &str, connected: bool) {
    state().update(|s| match s.preferences.set_cli_connected(id, connected) {
        Ok(()) => {
            s.preferences_error = None;
            s.preferences_feedback = Some(if connected {
                "Signed in to CLI account.".to_owned()
            } else {
                "Signed out of CLI account.".to_owned()
            });
            s.persist_preferences();
        }
        Err(err) => {
            s.preferences_error = Some(err.to_string());
        }
    });
}

pub(crate) fn set_appearance_mode(mode: AppearanceMode) {
    state().update(|s| {
        s.preferences.appearance.mode = mode;
        s.persist_preferences();
    });
    apply_appearance_mode(mode);
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

pub(crate) fn set_default_projects_dir(dir: &str) {
    state().update(|s| {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            s.preferences.general.default_projects_dir = trimmed.to_owned();
            s.persist_preferences();
        }
    });
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
                    format!("Signed in to {} inside Wald.", agent.label())
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
        "wald"
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
        allowed_projects: vec![
            "api-gateway".to_owned(),
            "marketing-site".to_owned(),
            "mobile-app".to_owned(),
        ],
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
        for fallback in ["api-gateway", "marketing-site", "mobile-app"] {
            if !names.iter().any(|n| n == fallback) {
                names.push(fallback.to_owned());
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
        s.preferences = AppPreferences::sample();
        s.selected_provider_id = default_selected_provider_id(&s.preferences);
        s.preferences_error = None;
        s.preferences_feedback = Some("Restored default preferences.".to_owned());
        s.persist_preferences();
    });
    apply_appearance_mode(AppearanceMode::System);
}

// --- Project Settings Window ------------------------------------------------------------

pub(crate) fn project_settings_open() -> bool {
    state().with(|s| s.project_settings_open)
}

pub(crate) fn project_settings_focused() -> bool {
    state().with(|s| s.project_settings_focused)
}

pub(crate) fn set_project_settings_focused(focused: bool) {
    state().update(|s| {
        s.project_settings_focused = focused;
    });
}

pub(crate) fn open_project_settings() {
    state().update(|s| {
        if s.selected_project.is_none() {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
        }
        s.project_settings_open = true;
        s.project_settings_focused = true;
        s.project_settings_error = None;
        s.project_settings_feedback = None;
    });
}

pub(crate) fn open_project_settings_for_project(id: ProjectId) {
    state().update(|s| {
        s.selected_project = Some(id);
        s.expanded_projects.insert(id);
        s.project_settings_tab = ProjectSettingsTab::IssuesAndIntake;
        s.project_settings_open = true;
        s.project_settings_focused = true;
        s.project_settings_error = None;
        s.project_settings_feedback = None;
    });
}

pub(crate) fn open_project_settings_tab(tab: ProjectSettingsTab) {
    state().update(|s| {
        if s.selected_project.is_none() {
            s.selected_project = s.factory.projects().first().map(|p| p.id);
        }
        s.project_settings_tab = tab;
        s.project_settings_open = true;
        s.project_settings_focused = true;
        s.project_settings_error = None;
        s.project_settings_feedback = None;
    });
}

pub(crate) fn close_project_settings() {
    state().update(|s| {
        s.project_settings_open = false;
        s.project_settings_focused = false;
        s.project_settings_error = None;
        s.project_settings_feedback = None;
    });
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
        s.ensure_project_settings_mut().github_repo = repo;
        s.project_settings_error = None;
        s.sync_queue_from_project_settings();
    });
}

pub(crate) fn set_project_itsaplan_workspace(workspace: &str) {
    let workspace = workspace.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().itsaplan_workspace = workspace;
        s.project_settings_error = None;
    });
}

pub(crate) fn set_project_itsaplan_agent_member(member: &str) {
    let member = member.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().itsaplan_agent_member = member;
        s.project_settings_error = None;
    });
}

pub(crate) fn set_project_github_token(raw_token: &str) -> bool {
    let mut ok = false;
    state().update(
        |s| match s.ensure_project_settings_mut().set_github_token(raw_token) {
            Ok(()) => {
                s.project_settings_error = None;
                s.project_settings_feedback =
                    Some("Saved personal access token in macOS Keychain.".to_owned());
                ok = true;
            }
            Err(err) => {
                s.project_settings_error = Some(err.to_string());
            }
        },
    );
    ok
}

pub(crate) fn use_project_github_cli() {
    state().update(|s| {
        s.ensure_project_settings_mut().use_github_cli();
        s.project_settings_error = None;
        s.project_settings_feedback = Some("Using GitHub CLI (gh) sign-in.".to_owned());
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
    });
}

pub(crate) fn toggle_project_auto_start() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.auto_start_matching_work = !settings.auto_start_matching_work;
    });
}

pub(crate) fn toggle_project_add_working_label() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.add_working_label = !settings.add_working_label;
    });
}

pub(crate) fn toggle_project_comment_summary() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.comment_with_run_summary = !settings.comment_with_run_summary;
    });
}

pub(crate) fn toggle_project_link_pr_closes_issue() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.link_pr_closes_issue = !settings.link_pr_closes_issue;
    });
}

pub(crate) fn set_project_default_branch(branch: &str) {
    let branch = branch.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().default_branch = branch;
    });
}

pub(crate) fn set_project_branch_prefix(prefix: &str) {
    let prefix = prefix.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().branch_prefix = prefix;
    });
}

pub(crate) fn toggle_project_isolated_worktrees() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.use_isolated_worktrees = !settings.use_isolated_worktrees;
    });
}

pub(crate) fn toggle_project_draft_prs() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.open_prs_as_draft = !settings.open_prs_as_draft;
    });
}

pub(crate) fn toggle_project_auto_cleanup_worktrees() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.auto_cleanup_worktrees = !settings.auto_cleanup_worktrees;
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
    });
}

pub(crate) fn set_project_default_provider(provider: ProviderKind) {
    state().update(|s| {
        s.ensure_project_settings_mut().default_provider = provider;
    });
}

pub(crate) fn set_project_openrouter_key(key_id: Option<String>) {
    state().update(|s| {
        s.ensure_project_settings_mut().openrouter_key_id = key_id;
    });
}

pub(crate) fn set_project_model_override(model: &str) {
    let model = model.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().model_override = model;
    });
}

pub(crate) fn toggle_project_workflow(name: &str) {
    let name = name.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().toggle_workflow(&name);
    });
}

pub(crate) fn toggle_project_require_review_before_pr() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.require_review_before_pr = !settings.require_review_before_pr;
    });
}

pub(crate) fn toggle_project_auto_reload_workflows() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.auto_reload_workflows = !settings.auto_reload_workflows;
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
        }
    });
}

pub(crate) fn toggle_project_load_dotenv() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.load_dotenv_local = !settings.load_dotenv_local;
    });
}

pub(crate) fn toggle_project_inherit_host_path() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.inherit_host_path = !settings.inherit_host_path;
    });
}

pub(crate) fn step_project_max_loop_attempts(delta: i32) {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.max_loop_attempts =
            (settings.max_loop_attempts as i32 + delta).clamp(1, 20) as u32;
    });
}

pub(crate) fn step_project_step_timeout(delta: i32) {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.step_timeout_minutes =
            (settings.step_timeout_minutes as i32 + delta).clamp(5, 120) as u32;
    });
}

pub(crate) fn step_project_per_run_budget(delta_cents: i64) {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        let next = (settings.per_run_budget_cents as i64 + delta_cents).clamp(100, 10_000);
        settings.per_run_budget_cents = next as u64;
    });
}

pub(crate) fn step_project_monthly_budget(delta_cents: i64) {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        let next = (settings.monthly_budget_cents as i64 + delta_cents).clamp(1_000, 500_000);
        settings.monthly_budget_cents = next as u64;
    });
}

pub(crate) fn toggle_project_pause_on_budget() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.pause_on_budget_limit = !settings.pause_on_budget_limit;
    });
}

pub(crate) fn toggle_project_require_approval_for_ci() {
    state().update(|s| {
        let settings = s.ensure_project_settings_mut();
        settings.require_approval_for_ci = !settings.require_approval_for_ci;
    });
}

pub(crate) fn set_project_sandbox_mode(mode: ProjectSandboxMode) {
    state().update(|s| {
        s.ensure_project_settings_mut().sandbox_mode = mode;
    });
}

pub(crate) fn set_project_preferred_runner(runner: &str) {
    let runner = runner.to_owned();
    state().update(|s| {
        s.ensure_project_settings_mut().preferred_runner = runner;
    });
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
            .unwrap_or_else(|| "api-gateway".to_owned());
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
    state().update(|s| {
        let project = s.selected_project_name();
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
        let filename = format!("{id_owned}.wald-workflow");
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
        let entry = s
            .workflows_overview_for_name(&project)
            .workflow(&target_id)
            .cloned();
        let key = (project.clone(), target_id.clone());
        s.workflow_editors.entry(key.clone()).or_insert_with(|| {
            if let Some(entry) = entry {
                WorkflowEditorDocument::from_overview_entry(&project, &entry)
            } else {
                WorkflowEditorDocument::for_workflow_name(&project, &target_id)
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
        let report = s.active_workflow_editor_clone().validate();
        let summary = report.summary.clone();
        s.workflow_editor_validation = Some(report);
        s.status_message = Some(summary);
    });
}

pub(crate) fn save_workflow_editor() {
    state().update(|s| {
        let (project, workflow_id) = s.active_workflow_editor_key.clone();
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

pub(crate) fn launch_editor_ad_hoc_run(summary: &str) -> bool {
    let prompt = summary.to_owned();
    let mut launched = false;
    state().update(|s| {
        let (project, workflow_id) = s.active_workflow_editor_key.clone();
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
    launched
}
