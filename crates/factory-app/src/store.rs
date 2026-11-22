//! Reactive per-surface application state backed by `factory-core`.

use std::path::PathBuf;

use factory_core::{
    DryRunExecutor, FactoryState, Project, ProjectId, RunId, Workflow, WorkflowId, WorkflowRun,
};
use nsh_core::prelude::*;

#[derive(Clone)]
struct AppState {
    factory: FactoryState,
    selected_project: Option<ProjectId>,
    selected_workflow: Option<WorkflowId>,
    selected_run: Option<RunId>,
    status_message: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        let factory = FactoryState::with_starter_catalog();
        let selected_project = factory.projects().first().map(|project| project.id);
        let selected_workflow = factory.workflows().first().map(|workflow| workflow.id);
        Self {
            factory,
            selected_project,
            selected_workflow,
            selected_run: None,
            status_message: None,
        }
    }
}

fn state() -> Signal<AppState> {
    global::<AppState>()
}

pub(crate) fn project_ids() -> Vec<ProjectId> {
    state().with(|s| s.factory.projects().iter().map(|p| p.id).collect())
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
        s.selected_run = s
            .factory
            .runs_for_project(id)
            .first()
            .map(|run| run.id);
        s.status_message = None;
    });
}

pub(crate) fn add_project(name: &str, path: &str, branch: &str) -> bool {
    let mut created = false;
    state().update(|s| {
        match s
            .factory
            .add_project(name, PathBuf::from(path.trim()), branch, "")
        {
            Ok(project) => {
                let id = project.id;
                s.selected_project = Some(id);
                s.selected_run = None;
                s.status_message = Some(format!("Added project '{}'", project.name));
                created = true;
            }
            Err(err) => {
                s.status_message = Some(err.to_string());
            }
        }
    });
    created
}

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
                s.status_message = Some(format!(
                    "Run #{} is now {}",
                    run.id,
                    run.status.label()
                ));
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
