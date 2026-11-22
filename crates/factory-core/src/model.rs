use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Stable identifier for a registered software project.
pub type ProjectId = u64;

/// Stable identifier for a workflow definition.
pub type WorkflowId = u64;

/// Stable identifier for an execution of a workflow against a project.
pub type RunId = u64;

/// A target codebase or workspace managed by the software factory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub root_path: PathBuf,
    pub default_branch: String,
    pub description: String,
}

/// The execution kind of a workflow step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepKind {
    /// Invoke an AI coding agent with a prompt template.
    AiAgent {
        model: String,
        prompt_template: String,
    },
    /// Execute a verification or build command in the project workspace.
    ShellCommand { command: String },
    /// Pause for a human review or approval decision.
    HumanGate { prompt: String },
}

impl StepKind {
    /// Short human-readable label for badges and summaries.
    pub fn label(&self) -> &'static str {
        match self {
            Self::AiAgent { .. } => "AI Agent",
            Self::ShellCommand { .. } => "Command",
            Self::HumanGate { .. } => "Approval",
        }
    }
}

/// A single ordered step inside a workflow definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub name: String,
    pub kind: StepKind,
}

/// A reusable AI workflow template that can be executed against any project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workflow {
    pub id: WorkflowId,
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
}

/// Lifecycle state of a workflow run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    WaitingForApproval,
    Succeeded,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::WaitingForApproval => "Awaiting Approval",
            Self::Succeeded => "Succeeded",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// Lifecycle state of an individual step within a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepRunStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
}

impl StepRunStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Running => "Running",
            Self::Succeeded => "Done",
            Self::Failed => "Failed",
            Self::Skipped => "Skipped",
        }
    }
}

/// Runtime state recorded for one step inside a [`WorkflowRun`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRun {
    pub step_id: String,
    pub step_name: String,
    pub kind: StepKind,
    pub status: StepRunStatus,
    pub output: Option<String>,
}

/// Severity of a log line emitted during workflow execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// A structured log line associated with a workflow run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub step_name: String,
    pub level: LogLevel,
    pub message: String,
}

/// Concrete execution instance of a [`Workflow`] against a [`Project`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: RunId,
    pub project_id: ProjectId,
    pub project_name: String,
    pub workflow_id: WorkflowId,
    pub workflow_name: String,
    pub task_prompt: String,
    pub status: RunStatus,
    pub steps: Vec<StepRun>,
    pub logs: Vec<LogEntry>,
}

impl WorkflowRun {
    /// Number of steps that have completed successfully.
    pub fn completed_steps(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.status == StepRunStatus::Succeeded)
            .count()
    }
}
