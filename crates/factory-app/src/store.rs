//! Reactive per-surface application state backed by `factory-core` and SQLite.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use factory_core::{
    AppPreferences, AppearanceMode, DashboardDataset, DashboardGranularity, DashboardQuery,
    DashboardSnapshot, DefaultLandingView, DryRunExecutor, FactoryState, InboxItem, MachineNode,
    MachineStatus, PreferencesTab, Project, ProjectDatabase, ProjectId, RunId, RunStatus, Workflow,
    WorkflowId, WorkflowRun,
};
use nsh_core::{platform::Appearance, prelude::*};

/// Primary screen shown in the main content area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveView {
    Dashboard,
    Inbox,
    Machines,
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
    selected_workflow: Option<WorkflowId>,
    selected_run: Option<RunId>,
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
                Self {
                    factory,
                    db: Some(Arc::new(Mutex::new(db))),
                    db_path_display,
                    using_sample_projects,
                    active_view,
                    selected_project,
                    selected_workflow,
                    selected_run: None,
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
                    sidebar_visible: true,
                    status_message,
                    registration_error: None,
                }
            }
            Err(err) => {
                let factory = FactoryState::with_projects(DashboardDataset::sample_projects());
                let selected_project = factory.projects().first().map(|project| project.id);
                let selected_workflow = factory.workflows().first().map(|workflow| workflow.id);
                let preferences = AppPreferences::sample();
                let selected_provider_id = default_selected_provider_id(&preferences);
                Self {
                    factory,
                    db: None,
                    db_path_display: "Unavailable".to_owned(),
                    using_sample_projects: true,
                    active_view: ActiveView::Dashboard,
                    selected_project,
                    selected_workflow,
                    selected_run: None,
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
                    sidebar_visible: true,
                    status_message: Some(format!("Could not open SQLite database: {err}")),
                    registration_error: Some(format!("Database unavailable: {err}")),
                }
            }
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
        s.active_view = ActiveView::Workspace;
        s.selected_run = s.factory.runs_for_project(id).first().map(|run| run.id);
        s.status_message = None;
    });
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
                } else {
                    s.factory.insert_project(project);
                }
                s.selected_project = Some(id);
                s.selected_run = None;
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
