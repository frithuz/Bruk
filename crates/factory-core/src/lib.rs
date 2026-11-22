//! Core domain model and workflow execution engine for Software Factory.
//!
//! This crate is intentionally independent of any UI or platform framework so
//! workflow orchestration, persistence, and agent integrations can be tested
//! and reused across the macOS app, CLI tools, or background daemons.

pub mod engine;
pub mod model;

pub use engine::{DryRunExecutor, FactoryError, FactoryState, StepExecutor, StepOutcome};
pub use model::{
    LogEntry, LogLevel, Project, ProjectId, RunId, RunStatus, StepKind, StepRun, StepRunStatus,
    Workflow, WorkflowId, WorkflowRun, WorkflowStep,
};
