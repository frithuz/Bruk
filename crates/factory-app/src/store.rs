//! Reactive per-surface application state backed by `factory-core` and SQLite.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use factory_core::{
    DashboardDataset, DashboardGranularity, DashboardQuery, DashboardSnapshot, DryRunExecutor,
    FactoryState, InboxItem, MachineNode, MachineStatus, Project, ProjectDatabase, ProjectId,
    RunId, Workflow, WorkflowId, WorkflowRun,
};
use nsh_core::prelude::*;

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
                let (persisted_projects, status_message) = match db.list_projects() {
                    Ok(projects) => (projects, None),
                    Err(err) => (Vec::new(), Some(format!("Failed to load projects: {err}"))),
                };
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
                    status_message,
                    registration_error: None,
                }
            }
            Err(err) => {
                let factory = FactoryState::with_projects(DashboardDataset::sample_projects());
                let selected_project = factory.projects().first().map(|project| project.id);
                let selected_workflow = factory.workflows().first().map(|workflow| workflow.id);
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
                    status_message: Some(format!("Could not open SQLite database: {err}")),
                    registration_error: Some(format!("Database unavailable: {err}")),
                }
            }
        }
    }
}

fn state() -> Signal<AppState> {
    global::<AppState>()
}

/// Initializes the per-surface store using the default SQLite DB in the user config folder.
pub(crate) fn init_default() {
    let _ = state();
}

/// Initializes the per-surface store against an explicit SQLite DB path.
pub(crate) fn init_with_database_path(path: &Path) {
    let loaded = AppState::from_database_result(ProjectDatabase::open(path));
    state().set(loaded);
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
