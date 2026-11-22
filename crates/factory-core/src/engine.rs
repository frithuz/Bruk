use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{
    LogEntry, LogLevel, Project, ProjectId, RunId, RunStatus, StepKind, StepRun, StepRunStatus,
    Workflow, WorkflowId, WorkflowRun, WorkflowStep,
};

/// Errors returned by [`FactoryState`] operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FactoryError {
    #[error("project name cannot be empty")]
    EmptyProjectName,
    #[error("project path cannot be empty")]
    EmptyProjectPath,
    #[error("task prompt cannot be empty")]
    EmptyTaskPrompt,
    #[error("project {0} was not found")]
    ProjectNotFound(ProjectId),
    #[error("workflow {0} was not found")]
    WorkflowNotFound(WorkflowId),
    #[error("run {0} was not found")]
    RunNotFound(RunId),
    #[error("run {0} is already in terminal state {1:?}")]
    RunAlreadyFinished(RunId, RunStatus),
}

/// Outcome produced by a [`StepExecutor`] for a single workflow step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    Succeeded { output: String },
    WaitingForApproval { message: String },
    Failed { error: String },
}

/// Pluggable execution strategy for running a workflow step against a project.
pub trait StepExecutor {
    fn execute_step(
        &self,
        project: &Project,
        run: &WorkflowRun,
        step: &StepRun,
    ) -> StepOutcome;
}

/// Deterministic executor useful for local simulation, UI prototyping, and tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct DryRunExecutor;

impl StepExecutor for DryRunExecutor {
    fn execute_step(
        &self,
        project: &Project,
        run: &WorkflowRun,
        step: &StepRun,
    ) -> StepOutcome {
        match &step.kind {
            StepKind::AiAgent { model, .. } => StepOutcome::Succeeded {
                output: format!(
                    "[{model}] completed '{}' for '{}' ({})",
                    step.step_name, run.task_prompt, project.name
                ),
            },
            StepKind::ShellCommand { command } => StepOutcome::Succeeded {
                output: format!(
                    "$ {command} (in {}) -> ok",
                    project.root_path.display()
                ),
            },
            StepKind::HumanGate { prompt } => StepOutcome::WaitingForApproval {
                message: prompt.clone(),
            },
        }
    }
}

/// In-memory aggregate holding projects, workflow definitions, and runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactoryState {
    projects: Vec<Project>,
    workflows: Vec<Workflow>,
    runs: Vec<WorkflowRun>,
    next_project_id: ProjectId,
    next_workflow_id: WorkflowId,
    next_run_id: RunId,
}

impl Default for FactoryState {
    fn default() -> Self {
        Self::with_starter_catalog()
    }
}

impl FactoryState {
    /// Creates an empty factory with no projects, workflows, or runs.
    pub fn empty() -> Self {
        Self {
            projects: Vec::new(),
            workflows: Vec::new(),
            runs: Vec::new(),
            next_project_id: 1,
            next_workflow_id: 1,
            next_run_id: 1,
        }
    }

    /// Creates a factory pre-populated with sample projects and starter AI workflows.
    pub fn with_starter_catalog() -> Self {
        let mut state = Self::empty();

        let _ = state.add_project(
            "software-factory",
            PathBuf::from("."),
            "main",
            "Rust + Native Shell macOS desktop workspace",
        );

        state.add_workflow(
            "Feature Implementation",
            "Plan the change, write code in a worktree, verify with tests, and prepare for review.",
            vec![
                WorkflowStep {
                    id: "plan".to_owned(),
                    name: "Plan Architecture".to_owned(),
                    kind: StepKind::AiAgent {
                        model: "reasoning-agent".to_owned(),
                        prompt_template: "Inspect the repository and draft an implementation plan for: {{task}}".to_owned(),
                    },
                },
                WorkflowStep {
                    id: "implement".to_owned(),
                    name: "Implement Changes".to_owned(),
                    kind: StepKind::AiAgent {
                        model: "coding-agent".to_owned(),
                        prompt_template: "Implement the approved plan with tests for: {{task}}".to_owned(),
                    },
                },
                WorkflowStep {
                    id: "verify".to_owned(),
                    name: "Run Test Suite".to_owned(),
                    kind: StepKind::ShellCommand {
                        command: "cargo test --workspace".to_owned(),
                    },
                },
                WorkflowStep {
                    id: "review".to_owned(),
                    name: "Human Review Gate".to_owned(),
                    kind: StepKind::HumanGate {
                        prompt: "Review the diff and approve before merging.".to_owned(),
                    },
                },
            ],
        );

        state.add_workflow(
            "Bug Triage & Fix",
            "Reproduce the failure, isolate root cause, apply a targeted fix, and verify regression tests.",
            vec![
                WorkflowStep {
                    id: "reproduce".to_owned(),
                    name: "Reproduce & Diagnose".to_owned(),
                    kind: StepKind::AiAgent {
                        model: "coding-agent".to_owned(),
                        prompt_template: "Locate the root cause and write a failing test for: {{task}}".to_owned(),
                    },
                },
                WorkflowStep {
                    id: "fix".to_owned(),
                    name: "Apply Fix".to_owned(),
                    kind: StepKind::AiAgent {
                        model: "coding-agent".to_owned(),
                        prompt_template: "Fix the bug cleanly and keep the diff minimal.".to_owned(),
                    },
                },
                WorkflowStep {
                    id: "check".to_owned(),
                    name: "Verify Build & Tests".to_owned(),
                    kind: StepKind::ShellCommand {
                        command: "cargo test --workspace".to_owned(),
                    },
                },
            ],
        );

        state.add_workflow(
            "Codebase Audit",
            "Analyze architecture, security posture, and test gaps without modifying files.",
            vec![
                WorkflowStep {
                    id: "scan".to_owned(),
                    name: "Static Checks".to_owned(),
                    kind: StepKind::ShellCommand {
                        command: "cargo clippy --workspace --all-targets".to_owned(),
                    },
                },
                WorkflowStep {
                    id: "audit".to_owned(),
                    name: "Generate Findings Report".to_owned(),
                    kind: StepKind::AiAgent {
                        model: "review-agent".to_owned(),
                        prompt_template: "Summarize findings and prioritized recommendations for: {{task}}".to_owned(),
                    },
                },
            ],
        );

        state
    }

    pub fn projects(&self) -> &[Project] {
        &self.projects
    }

    pub fn project(&self, id: ProjectId) -> Option<&Project> {
        self.projects.iter().find(|project| project.id == id)
    }

    pub fn workflows(&self) -> &[Workflow] {
        &self.workflows
    }

    pub fn workflow(&self, id: WorkflowId) -> Option<&Workflow> {
        self.workflows.iter().find(|workflow| workflow.id == id)
    }

    pub fn runs(&self) -> &[WorkflowRun] {
        &self.runs
    }

    pub fn run(&self, id: RunId) -> Option<&WorkflowRun> {
        self.runs.iter().find(|run| run.id == id)
    }

    pub fn runs_for_project(&self, project_id: ProjectId) -> Vec<&WorkflowRun> {
        self.runs
            .iter()
            .filter(|run| run.project_id == project_id)
            .collect()
    }

    /// Registers a new project workspace.
    pub fn add_project(
        &mut self,
        name: impl Into<String>,
        root_path: impl Into<PathBuf>,
        default_branch: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<&Project, FactoryError> {
        let name = name.into().trim().to_owned();
        if name.is_empty() {
            return Err(FactoryError::EmptyProjectName);
        }

        let root_path = root_path.into();
        if root_path.as_os_str().is_empty() {
            return Err(FactoryError::EmptyProjectPath);
        }

        let branch = default_branch.into().trim().to_owned();
        let default_branch = if branch.is_empty() {
            "main".to_owned()
        } else {
            branch
        };

        let id = self.next_project_id;
        self.next_project_id += 1;

        self.projects.push(Project {
            id,
            name,
            root_path,
            default_branch,
            description: description.into().trim().to_owned(),
        });

        Ok(self.projects.last().expect("project just pushed"))
    }

    /// Adds a workflow definition to the catalog.
    pub fn add_workflow(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        steps: Vec<WorkflowStep>,
    ) -> &Workflow {
        let id = self.next_workflow_id;
        self.next_workflow_id += 1;
        self.workflows.push(Workflow {
            id,
            name: name.into(),
            description: description.into(),
            steps,
        });
        self.workflows.last().expect("workflow just pushed")
    }

    /// Creates a new workflow run for a project and marks it queued/running.
    pub fn start_run(
        &mut self,
        project_id: ProjectId,
        workflow_id: WorkflowId,
        task_prompt: impl Into<String>,
    ) -> Result<&WorkflowRun, FactoryError> {
        let task_prompt = task_prompt.into().trim().to_owned();
        if task_prompt.is_empty() {
            return Err(FactoryError::EmptyTaskPrompt);
        }

        let project = self
            .project(project_id)
            .ok_or(FactoryError::ProjectNotFound(project_id))?
            .clone();
        let workflow = self
            .workflow(workflow_id)
            .ok_or(FactoryError::WorkflowNotFound(workflow_id))?
            .clone();

        let id = self.next_run_id;
        self.next_run_id += 1;

        let steps = workflow
            .steps
            .iter()
            .map(|step| StepRun {
                step_id: step.id.clone(),
                step_name: step.name.clone(),
                kind: step.kind.clone(),
                status: StepRunStatus::Pending,
                output: None,
            })
            .collect();

        let initial_log = LogEntry {
            step_name: "Orchestrator".to_owned(),
            level: LogLevel::Info,
            message: format!(
                "Queued '{}' against '{}' for task: {}",
                workflow.name, project.name, task_prompt
            ),
        };

        self.runs.insert(
            0,
            WorkflowRun {
                id,
                project_id: project.id,
                project_name: project.name,
                workflow_id: workflow.id,
                workflow_name: workflow.name,
                task_prompt,
                status: RunStatus::Queued,
                steps,
                logs: vec![initial_log],
            },
        );

        Ok(&self.runs[0])
    }

    /// Executes the next pending step in `run_id` using `executor`.
    ///
    /// If the run is currently [`RunStatus::WaitingForApproval`], calling this
    /// approves the waiting gate step and marks it succeeded.
    pub fn advance_run(
        &mut self,
        run_id: RunId,
        executor: &dyn StepExecutor,
    ) -> Result<&WorkflowRun, FactoryError> {
        let run_index = self
            .runs
            .iter()
            .position(|run| run.id == run_id)
            .ok_or(FactoryError::RunNotFound(run_id))?;

        let status = self.runs[run_index].status;
        if status.is_terminal() {
            return Err(FactoryError::RunAlreadyFinished(run_id, status));
        }

        // If waiting on a human approval gate, approve that step first.
        if status == RunStatus::WaitingForApproval {
            let run = &mut self.runs[run_index];
            if let Some(step) = run
                .steps
                .iter_mut()
                .find(|step| step.status == StepRunStatus::Running)
            {
                step.status = StepRunStatus::Succeeded;
                step.output = Some("Approved by operator.".to_owned());
                run.logs.push(LogEntry {
                    step_name: step.step_name.clone(),
                    level: LogLevel::Info,
                    message: "Approval granted.".to_owned(),
                });
            }
            if run
                .steps
                .iter()
                .all(|step| step.status == StepRunStatus::Succeeded)
            {
                run.status = RunStatus::Succeeded;
                run.logs.push(LogEntry {
                    step_name: "Orchestrator".to_owned(),
                    level: LogLevel::Info,
                    message: "Workflow completed successfully.".to_owned(),
                });
            } else {
                run.status = RunStatus::Running;
            }
            return Ok(&self.runs[run_index]);
        }

        let project_id = self.runs[run_index].project_id;
        let project = self
            .project(project_id)
            .ok_or(FactoryError::ProjectNotFound(project_id))?
            .clone();

        let next_step_index = self.runs[run_index]
            .steps
            .iter()
            .position(|step| step.status == StepRunStatus::Pending);

        let Some(step_index) = next_step_index else {
            self.runs[run_index].status = RunStatus::Succeeded;
            return Ok(&self.runs[run_index]);
        };

        self.runs[run_index].status = RunStatus::Running;
        self.runs[run_index].steps[step_index].status = StepRunStatus::Running;

        let run_snapshot = self.runs[run_index].clone();
        let step_snapshot = run_snapshot.steps[step_index].clone();
        let outcome = executor.execute_step(&project, &run_snapshot, &step_snapshot);

        let run = &mut self.runs[run_index];
        let step = &mut run.steps[step_index];
        match outcome {
            StepOutcome::Succeeded { output } => {
                step.status = StepRunStatus::Succeeded;
                step.output = Some(output.clone());
                let step_name = step.step_name.clone();
                run.logs.push(LogEntry {
                    step_name,
                    level: LogLevel::Info,
                    message: output,
                });
                if run
                    .steps
                    .iter()
                    .all(|item| item.status == StepRunStatus::Succeeded)
                {
                    run.status = RunStatus::Succeeded;
                    run.logs.push(LogEntry {
                        step_name: "Orchestrator".to_owned(),
                        level: LogLevel::Info,
                        message: "Workflow completed successfully.".to_owned(),
                    });
                }
            }
            StepOutcome::WaitingForApproval { message } => {
                step.status = StepRunStatus::Running;
                step.output = Some(message.clone());
                let step_name = step.step_name.clone();
                run.status = RunStatus::WaitingForApproval;
                run.logs.push(LogEntry {
                    step_name,
                    level: LogLevel::Warn,
                    message,
                });
            }
            StepOutcome::Failed { error } => {
                step.status = StepRunStatus::Failed;
                step.output = Some(error.clone());
                let step_name = step.step_name.clone();
                run.status = RunStatus::Failed;
                run.logs.push(LogEntry {
                    step_name,
                    level: LogLevel::Error,
                    message: error,
                });
            }
        }

        Ok(&self.runs[run_index])
    }

    /// Cancels an active or queued run and marks remaining pending steps skipped.
    pub fn cancel_run(&mut self, run_id: RunId) -> Result<&WorkflowRun, FactoryError> {
        let run_index = self
            .runs
            .iter()
            .position(|run| run.id == run_id)
            .ok_or(FactoryError::RunNotFound(run_id))?;

        let status = self.runs[run_index].status;
        if status.is_terminal() {
            return Err(FactoryError::RunAlreadyFinished(run_id, status));
        }

        let run = &mut self.runs[run_index];
        run.status = RunStatus::Cancelled;
        for step in &mut run.steps {
            if matches!(step.status, StepRunStatus::Pending | StepRunStatus::Running) {
                step.status = StepRunStatus::Skipped;
            }
        }
        run.logs.push(LogEntry {
            step_name: "Orchestrator".to_owned(),
            level: LogLevel::Warn,
            message: "Run cancelled by operator.".to_owned(),
        });

        Ok(&self.runs[run_index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_project_and_rejects_empty_fields() {
        let mut state = FactoryState::empty();
        assert_eq!(
            state.add_project("  ", "/tmp/demo", "main", ""),
            Err(FactoryError::EmptyProjectName)
        );
        assert_eq!(
            state.add_project("demo", "", "main", ""),
            Err(FactoryError::EmptyProjectPath)
        );

        let project = state
            .add_project("demo", "/tmp/demo", "", "Sample repo")
            .unwrap();
        assert_eq!(project.id, 1);
        assert_eq!(project.default_branch, "main");
    }

    #[test]
    fn runs_workflow_through_human_gate_to_completion() {
        let mut state = FactoryState::with_starter_catalog();
        let project_id = state.projects()[0].id;
        let workflow_id = state.workflows()[0].id;

        let run_id = state
            .start_run(project_id, workflow_id, "Add metrics sidebar")
            .unwrap()
            .id;

        // 3 automated steps + 1 human gate
        let run = state.advance_run(run_id, &DryRunExecutor).unwrap();
        assert_eq!(run.status, RunStatus::Running);
        assert_eq!(run.completed_steps(), 1);

        let run = state.advance_run(run_id, &DryRunExecutor).unwrap();
        assert_eq!(run.completed_steps(), 2);

        let run = state.advance_run(run_id, &DryRunExecutor).unwrap();
        assert_eq!(run.completed_steps(), 3);

        let run = state.advance_run(run_id, &DryRunExecutor).unwrap();
        assert_eq!(run.status, RunStatus::WaitingForApproval);

        // Approving the waiting gate completes the run.
        let run = state.advance_run(run_id, &DryRunExecutor).unwrap();
        assert_eq!(run.status, RunStatus::Succeeded);
        assert_eq!(run.completed_steps(), 4);
    }

    #[test]
    fn cancels_active_run_and_skips_remaining_steps() {
        let mut state = FactoryState::with_starter_catalog();
        let project_id = state.projects()[0].id;
        let workflow_id = state.workflows()[1].id;

        let run_id = state
            .start_run(project_id, workflow_id, "Fix panic on empty config")
            .unwrap()
            .id;
        let _ = state.advance_run(run_id, &DryRunExecutor).unwrap();

        let cancelled = state.cancel_run(run_id).unwrap();
        assert_eq!(cancelled.status, RunStatus::Cancelled);
        assert_eq!(cancelled.steps[0].status, StepRunStatus::Succeeded);
        assert_eq!(cancelled.steps[1].status, StepRunStatus::Skipped);
        assert_eq!(cancelled.steps[2].status, StepRunStatus::Skipped);
    }

    #[test]
    fn round_trips_state_via_json() {
        let state = FactoryState::with_starter_catalog();
        let encoded = serde_json::to_string(&state).unwrap();
        let decoded: FactoryState = serde_json::from_str(&encoded).unwrap();
        assert_eq!(state, decoded);
    }
}
