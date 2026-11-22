//! Reactive per-surface application state backed by `factory-core` and SQLite.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex},
};

use factory_core::{
    AppPreferences, AppearanceMode, DashboardDataset, DashboardGranularity, DashboardQuery,
    DashboardSnapshot, DefaultLandingView, DeployWatchEntry, DryRunExecutor, FactoryState,
    InboxItem, IntakeRule, IssueSourceKind, MachineNode, MachineStatus, PreferencesTab, Project,
    ProjectDatabase, ProjectId, ProjectSandboxMode, ProjectSettings, ProjectSettingsTab,
    ProjectWorkQueue, ProjectWorkflowsOverview, ProviderKind, RunDetailTab, RunExecutionState,
    RunId, RunStatus, WorkQueueViewMode, Workflow, WorkflowId, WorkflowRun, WorkflowRunViewData,
    WorkflowTemplateKind, WorkflowsFilter, WorkflowsSort,
};
use nsh_core::{platform::Appearance, prelude::*};

/// Primary screen shown in the main content area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveView {
    Dashboard,
    Inbox,
    Machines,
    WorkQueue,
    Workflows,
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
    inbox_items: Vec<InboxItem>,
    machines: Vec<MachineNode>,
    preferences: AppPreferences,
    preferences_open: bool,
    preferences_focused: bool,
    preferences_tab: PreferencesTab,
    selected_provider_id: String,
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
        let inbox_items = DashboardDataset::sample_inbox();
        let machines = DashboardDataset::sample_machines();

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
                    inbox_items,
                    machines,
                    preferences,
                    preferences_open: false,
                    preferences_focused: false,
                    preferences_tab: PreferencesTab::Providers,
                    selected_provider_id,
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
                    inbox_items,
                    machines,
                    preferences,
                    preferences_open: false,
                    preferences_focused: false,
                    preferences_tab: PreferencesTab::Providers,
                    selected_provider_id,
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

    fn persist_preferences(&mut self) {
        if let Some(db_arc) = self.db.as_ref()
            && let Ok(db) = db_arc.lock()
            && let Err(err) = db.save_preferences(&self.preferences)
        {
            self.preferences_error = Some(format!("Could not save preferences: {err}"));
        }
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
        s.active_view = ActiveView::Inbox;
        s.status_message = None;
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
        let (_, workflow) = s.active_workflow_run_key.clone();
        s.status_message = Some(format!("Opened '{workflow}' workflow definition"));
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

pub(crate) fn inbox_items() -> Vec<InboxItem> {
    state().with(|s| s.inbox_items.clone())
}

pub(crate) fn pending_inbox_count() -> usize {
    state().with(|s| s.inbox_items.iter().filter(|item| !item.resolved).count())
}

pub(crate) fn resolve_inbox_item(id: u64) {
    state().update(|s| {
        if let Some(item) = s.inbox_items.iter_mut().find(|item| item.id == id) {
            item.resolved = true;
            s.status_message = Some(format!("Answered '{}'", item.title));
        }
    });
}

pub(crate) fn machines() -> Vec<MachineNode> {
    state().with(|s| s.machines.clone())
}

pub(crate) fn disconnected_machine_count() -> usize {
    state().with(|s| {
        s.machines
            .iter()
            .filter(|m| m.status == MachineStatus::Disconnected)
            .count()
    })
}

pub(crate) fn reconnect_machine(id: u64) {
    state().update(|s| {
        if let Some(machine) = s.machines.iter_mut().find(|m| m.id == id) {
            machine.status = MachineStatus::Connected;
            machine.last_seen = "Reconnected just now".to_owned();
            s.status_message = Some(format!("Reconnected machine '{}'", machine.name));
        }
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

pub(crate) fn step_max_concurrent_runs(delta: i32) {
    state().update(|s| {
        let next = (s.preferences.machines.max_concurrent_runs as i32 + delta).clamp(1, 16) as u32;
        s.preferences.machines.max_concurrent_runs = next;
        s.persist_preferences();
    });
}

pub(crate) fn step_heartbeat_interval(delta: i32) {
    state().update(|s| {
        let next =
            (s.preferences.machines.heartbeat_interval_secs as i32 + delta).clamp(5, 120) as u32;
        s.preferences.machines.heartbeat_interval_secs = next;
        s.persist_preferences();
    });
}

pub(crate) fn toggle_auto_reconnect_runners() {
    state().update(|s| {
        s.preferences.machines.auto_reconnect_runners =
            !s.preferences.machines.auto_reconnect_runners;
        s.persist_preferences();
    });
}

pub(crate) fn toggle_prefer_local_machine() {
    state().update(|s| {
        s.preferences.machines.prefer_local_machine = !s.preferences.machines.prefer_local_machine;
        s.persist_preferences();
    });
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
