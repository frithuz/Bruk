//! Visual Workflow Editor domain model, step palette catalog, loop configuration,
//! validation logic, and mocked fixtures matching `13 WorkflowEditor.png`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use factory_config::mcp::McpSourcesPolicy;
use factory_types::ProviderKind;

use crate::{
    definitions::{
        AgentTaskStepDef, ApprovalGateStepDef, BrukCatalog, CommentOnIssueStepDef, CompositeDef,
        CompositeStepDef, ConditionStepDef, DefinitionOrigin, LoopExitConditionDef, LoopNodeDef,
        OpenPullRequestStepDef, SetIssueStatusStepDef, ShellCommandStepDef, WorkflowDef,
        WorkflowNodeDef, WorkflowTriggersDef,
    },
    workflows_overview::{
        WorkflowOverviewEntry, WorkflowStepGroup, WorkflowStepPreview, WorkflowTriggerKind,
    },
};

/// Category grouping shown in the left-hand step palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaletteCategory {
    Agents,
    Control,
    Composites,
    Issues,
    FromExtensions,
}

impl PaletteCategory {
    pub const ALL: &'static [Self] = &[
        Self::Agents,
        Self::Control,
        Self::Composites,
        Self::Issues,
        Self::FromExtensions,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::Control => "Control",
            Self::Composites => "Composites",
            Self::Issues => "Issues",
            Self::FromExtensions => "From extensions",
        }
    }
}

/// Step type entries available in the Workflow Editor palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepTypeKind {
    ClaudeTask,
    CodexTask,
    AntigravityTask,
    ModelCall,
    Loop,
    Condition,
    ApprovalGate,
    ShellCommand,
    CompositeFixUntilTestsPass,
    CompositeReviewAndOpenPr,
    CompositeTwoModelReview,
    CommentOnIssue,
    SetIssueStatus,
    ExtensionPostUpdate,
}

impl StepTypeKind {
    pub const ALL: &'static [Self] = &[
        Self::ClaudeTask,
        Self::CodexTask,
        Self::AntigravityTask,
        Self::ModelCall,
        Self::Loop,
        Self::Condition,
        Self::ApprovalGate,
        Self::ShellCommand,
        Self::CompositeFixUntilTestsPass,
        Self::CompositeReviewAndOpenPr,
        Self::CompositeTwoModelReview,
        Self::CommentOnIssue,
        Self::SetIssueStatus,
        Self::ExtensionPostUpdate,
    ];

    pub fn category(self) -> PaletteCategory {
        match self {
            Self::ClaudeTask | Self::CodexTask | Self::AntigravityTask | Self::ModelCall => {
                PaletteCategory::Agents
            }
            Self::Loop | Self::Condition | Self::ApprovalGate | Self::ShellCommand => {
                PaletteCategory::Control
            }
            Self::CompositeFixUntilTestsPass
            | Self::CompositeReviewAndOpenPr
            | Self::CompositeTwoModelReview => PaletteCategory::Composites,
            Self::CommentOnIssue | Self::SetIssueStatus => PaletteCategory::Issues,
            Self::ExtensionPostUpdate => PaletteCategory::FromExtensions,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::ClaudeTask => "Claude task",
            Self::CodexTask => "Codex task",
            Self::AntigravityTask => "Antigravity task",
            Self::ModelCall => "Model call",
            Self::Loop => "Loop",
            Self::Condition => "Condition",
            Self::ApprovalGate => "Approval gate",
            Self::ShellCommand => "Shell command",
            Self::CompositeFixUntilTestsPass => "Fix until tests pass",
            Self::CompositeReviewAndOpenPr => "Review and open PR",
            Self::CompositeTwoModelReview => "Two-model review",
            Self::CommentOnIssue => "Comment on issue",
            Self::SetIssueStatus => "Set issue status",
            Self::ExtensionPostUpdate => "Post update",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::ClaudeTask => "Claude Code in the project",
            Self::CodexTask => "Codex CLI in the project",
            Self::AntigravityTask => "Antigravity CLI in the project",
            Self::ModelCall => "One OpenRouter request",
            Self::Loop => "Repeat steps until a check passes",
            Self::Condition => "Branch on an output",
            Self::ApprovalGate => "Waits for a person",
            Self::ShellCommand => "Allow-listed commands only",
            Self::CompositeFixUntilTestsPass => "Repository · plan → diff, tests",
            Self::CompositeReviewAndOpenPr => "Repository · diff → pr_url",
            Self::CompositeTwoModelReview => "Review Kit · diff → findings",
            Self::CommentOnIssue => "Uses the project's issue source",
            Self::SetIssueStatus => "Label or column",
            Self::ExtensionPostUpdate => "Chat Notifier",
        }
    }

    /// Short monogram displayed inside the palette badge, or `None` when rendered as an icon.
    pub fn badge_monogram(self) -> Option<&'static str> {
        match self {
            Self::ClaudeTask => Some("Cl"),
            Self::CodexTask => Some("Cx"),
            Self::AntigravityTask => Some("Ag"),
            Self::ModelCall => Some("OR"),
            Self::Loop => None,
            Self::Condition => Some("If"),
            Self::ApprovalGate => Some("Ap"),
            Self::ShellCommand => Some("Sh"),
            Self::CompositeFixUntilTestsPass
            | Self::CompositeReviewAndOpenPr
            | Self::CompositeTwoModelReview => None,
            Self::CommentOnIssue => Some("Co"),
            Self::SetIssueStatus => Some("St"),
            Self::ExtensionPostUpdate => Some("CN"),
        }
    }

    pub fn is_composite(self) -> bool {
        matches!(
            self,
            Self::CompositeFixUntilTestsPass
                | Self::CompositeReviewAndOpenPr
                | Self::CompositeTwoModelReview
        )
    }

    pub fn default_actor_badge(self) -> &'static str {
        match self {
            Self::ClaudeTask => "Claude",
            Self::CodexTask => "Codex",
            Self::AntigravityTask => "Antigravity",
            Self::ModelCall => "OpenRouter",
            Self::Loop => "Loop",
            Self::Condition => "Condition",
            Self::ApprovalGate => "You",
            Self::ShellCommand => "Shell · npm test",
            Self::CompositeFixUntilTestsPass => "Composite",
            Self::CompositeReviewAndOpenPr => "Composite",
            Self::CompositeTwoModelReview => "Review Kit",
            Self::CommentOnIssue => "Issue source",
            Self::SetIssueStatus => "Issue source",
            Self::ExtensionPostUpdate => "Chat Notifier",
        }
    }

    pub fn test_id(self) -> &'static str {
        match self {
            Self::ClaudeTask => "wfe-palette-claude-task",
            Self::CodexTask => "wfe-palette-codex-task",
            Self::AntigravityTask => "wfe-palette-antigravity-task",
            Self::ModelCall => "wfe-palette-model-call",
            Self::Loop => "wfe-palette-loop",
            Self::Condition => "wfe-palette-condition",
            Self::ApprovalGate => "wfe-palette-approval-gate",
            Self::ShellCommand => "wfe-palette-shell-command",
            Self::CompositeFixUntilTestsPass => "wfe-palette-comp-fix-tests",
            Self::CompositeReviewAndOpenPr => "wfe-palette-comp-review-pr",
            Self::CompositeTwoModelReview => "wfe-palette-comp-two-model",
            Self::CommentOnIssue => "wfe-palette-comment-issue",
            Self::SetIssueStatus => "wfe-palette-set-issue-status",
            Self::ExtensionPostUpdate => "wfe-palette-post-update",
        }
    }

    pub fn matches_query(self, query: &str) -> bool {
        let q = query.trim().to_ascii_lowercase();
        if q.is_empty() {
            return true;
        }
        self.title().to_ascii_lowercase().contains(&q)
            || self.subtitle().to_ascii_lowercase().contains(&q)
            || self.category().label().to_ascii_lowercase().contains(&q)
    }
}

pub use factory_types::{
    ExitField, ExitOperator, LoopExhaustedAction, LoopRepeatMode, LoopWaitBetween,
};

/// One structured exit condition evaluated after each loop attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopExitCondition {
    pub id: String,
    pub step_id: String,
    pub field: ExitField,
    pub operator: ExitOperator,
    pub expected: String,
}

/// Trigger header configuration displayed in the `Starts from` block at the top of the canvas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorTriggerConfig {
    pub work_queue_enabled: bool,
    pub work_queue_label: String,
    pub ad_hoc_allowed: bool,
    pub inputs_summary: String,
}

/// Individual step node on the visual canvas (either top-level or inside a loop).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorStep {
    pub id: String,
    pub name: String,
    pub kind: StepTypeKind,
    pub actor_badge: String,
    pub subtitle: Option<String>,
    pub detail: String,
    /// Optional agent reference (`planner`, `builtin/implementer`, `acme/reviewer`) for agent tasks.
    #[serde(default)]
    pub agent_ref: Option<String>,
    /// Optional composite reference (`fix-until-tests-pass`, `acme/deploy`) for composite steps.
    #[serde(default)]
    pub composite_ref: Option<String>,
    /// Extra step-level skills injected alongside the agent's own skills.
    #[serde(default)]
    pub skills: Vec<String>,
    /// MCP servers the step needs (`mcp:`), kept so saving from the editor doesn't drop them.
    #[serde(default)]
    pub mcp: Vec<String>,
}

/// Fallback branch card rendered beside the main flow when a loop runs out of attempts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorFallbackStep {
    pub name: String,
    pub action_label: String,
    pub outcome_label: String,
}

/// Visual loop container wrapping one or more child steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorLoopBlock {
    pub id: String,
    pub name: String,
    pub repeat_mode: LoopRepeatMode,
    pub exit_conditions: Vec<LoopExitCondition>,
    pub max_attempts: u32,
    pub wait_between: LoopWaitBetween,
    pub pass_previous_outputs: bool,
    pub discard_file_changes: bool,
    pub on_exhausted: LoopExhaustedAction,
    pub fallback: EditorFallbackStep,
    pub steps: Vec<EditorStep>,
}

impl EditorLoopBlock {
    pub fn header_badge_label(&self) -> String {
        match self.repeat_mode {
            LoopRepeatMode::UntilCheckPasses => {
                format!("Repeat until · max {}", self.max_attempts)
            }
            LoopRepeatMode::ForEachItem => format!("For each · max {}", self.max_attempts),
            LoopRepeatMode::FixedCount => format!("Repeat · {} times", self.max_attempts),
        }
    }

    pub fn exit_expression(&self) -> String {
        if self.exit_conditions.is_empty() {
            return "no exit condition configured".to_owned();
        }
        self.exit_conditions
            .iter()
            .map(|cond| {
                let step_slug = self
                    .steps
                    .iter()
                    .find(|s| s.id == cond.step_id)
                    .map(|s| slugify_identifier(&s.name))
                    .unwrap_or_else(|| "step".to_owned());
                format!(
                    "steps.{step_slug}.{} {} {}",
                    cond.field.value(),
                    cond.operator.symbol(),
                    cond.expected
                )
            })
            .collect::<Vec<_>>()
            .join(" && ")
    }

    pub fn exhausted_prefix_and_target(&self) -> (&'static str, String) {
        match self.on_exhausted {
            LoopExhaustedAction::GoToFallbackStep => ("Go to", self.fallback.name.clone()),
            LoopExhaustedAction::FailRun => ("Action", "Fail workflow run".to_owned()),
            LoopExhaustedAction::AskApproval => ("Action", "Wait for approval".to_owned()),
            LoopExhaustedAction::ContinueNext => ("Action", "Continue to next step".to_owned()),
        }
    }
}

/// Top-level node in the visual workflow spine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditorNode {
    Step(EditorStep),
    Loop(EditorLoopBlock),
}

/// Currently selected element in the Visual Workflow Editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditorSelection {
    Trigger,
    Step(String),
    Loop(String),
    Fallback(String),
}

/// Recorded revision entry shown in the Version History drawer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorRevision {
    pub version: u32,
    pub label: String,
    pub timestamp: String,
}

/// Result of running `Validate` on a workflow document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub is_valid: bool,
    pub summary: String,
    pub issues: Vec<String>,
}

/// Complete editable workflow document backing the Visual Workflow Editor page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowEditorDocument {
    pub project_name: String,
    pub workflow_id: String,
    pub workflow_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_draft: bool,
    /// `project` runs every step with only the project's MCP servers.
    #[serde(default)]
    pub mcp_sources: McpSourcesPolicy,
    #[serde(default)]
    pub origin: Option<DefinitionOrigin>,
    pub has_unsaved_changes: bool,
    pub trigger: EditorTriggerConfig,
    pub nodes: Vec<EditorNode>,
    pub selected: EditorSelection,
    pub revisions: Vec<EditorRevision>,
    next_id_counter: u32,
}

impl WorkflowEditorDocument {
    /// Reference `Fix issue` workflow matching `13 WorkflowEditor.png`.
    pub fn sample_fix_issue(project_name: &str) -> Self {
        Self {
            project_name: project_name.to_owned(),
            workflow_id: "fix-issue".to_owned(),
            workflow_name: "Fix issue".to_owned(),
            description: String::new(),
            is_draft: false,
            mcp_sources: McpSourcesPolicy::All,
            origin: None,
            has_unsaved_changes: true,
            trigger: EditorTriggerConfig {
                work_queue_enabled: true,
                work_queue_label: "Work queue · intake rules".to_owned(),
                ad_hoc_allowed: true,
                inputs_summary: "inputs: issue (optional for ad-hoc) · branch".to_owned(),
            },
            nodes: vec![
                EditorNode::Step(EditorStep {
                    id: "step-read-plan".to_owned(),
                    name: "Read issue & plan".to_owned(),
                    kind: StepTypeKind::ClaudeTask,
                    actor_badge: "Claude".to_owned(),
                    subtitle: None,
                    detail: "Inspect the linked issue, reproduce the bug, and draft a concise fix plan.".to_owned(),
                    agent_ref: None,
                    composite_ref: None,
                    skills: Vec::new(),
                    mcp: Vec::new(),
                }),
                EditorNode::Loop(EditorLoopBlock {
                    id: "loop-fix-tests".to_owned(),
                    name: "Fix until tests pass".to_owned(),
                    repeat_mode: LoopRepeatMode::UntilCheckPasses,
                    exit_conditions: vec![LoopExitCondition {
                        id: "cond-1".to_owned(),
                        step_id: "step-run-tests".to_owned(),
                        field: ExitField::ExitCode,
                        operator: ExitOperator::Equals,
                        expected: "0".to_owned(),
                    }],
                    max_attempts: 5,
                    wait_between: LoopWaitBetween::None,
                    pass_previous_outputs: true,
                    discard_file_changes: false,
                    on_exhausted: LoopExhaustedAction::GoToFallbackStep,
                    fallback: EditorFallbackStep {
                        name: "Report failure".to_owned(),
                        action_label: "Comment on issue".to_owned(),
                        outcome_label: "stop run".to_owned(),
                    },
                    steps: vec![
                        EditorStep {
                            id: "step-implement-fix".to_owned(),
                            name: "Implement fix".to_owned(),
                            kind: StepTypeKind::CodexTask,
                            actor_badge: "Codex".to_owned(),
                            subtitle: Some("reads $loop.previous on attempt 2+".to_owned()),
                            detail: "Apply the planned code change and address any failing test diagnostics from prior attempts.".to_owned(),
                            agent_ref: None,
                            composite_ref: None,
                            skills: Vec::new(),
                            mcp: Vec::new(),
                        },
                        EditorStep {
                            id: "step-run-tests".to_owned(),
                            name: "Run tests".to_owned(),
                            kind: StepTypeKind::ShellCommand,
                            actor_badge: "Shell · npm test".to_owned(),
                            subtitle: None,
                            detail: "npm test".to_owned(),
                            agent_ref: None,
                            composite_ref: None,
                            skills: Vec::new(),
                            mcp: Vec::new(),
                        },
                    ],
                }),
                EditorNode::Step(EditorStep {
                    id: "step-review-gate".to_owned(),
                    name: "Review gate".to_owned(),
                    kind: StepTypeKind::ApprovalGate,
                    actor_badge: "You".to_owned(),
                    subtitle: None,
                    detail: "Pause for human diff review before opening a pull request.".to_owned(),
                    agent_ref: None,
                    composite_ref: None,
                    skills: Vec::new(),
                    mcp: Vec::new(),
                }),
                EditorNode::Step(EditorStep {
                    id: "step-open-pr".to_owned(),
                    name: "Open pull request".to_owned(),
                    kind: StepTypeKind::ClaudeTask,
                    actor_badge: "Claude".to_owned(),
                    subtitle: None,
                    detail: "Push the fix branch and open a pull request referencing the source issue.".to_owned(),
                    agent_ref: None,
                    composite_ref: None,
                    skills: Vec::new(),
                    mcp: Vec::new(),
                }),
                EditorNode::Step(EditorStep {
                    id: "step-comment-issue".to_owned(),
                    name: "Comment on issue".to_owned(),
                    kind: StepTypeKind::CommentOnIssue,
                    actor_badge: "Issue source".to_owned(),
                    subtitle: None,
                    detail: "Post a summary comment with the pull request link on the originating issue.".to_owned(),
                    agent_ref: None,
                    composite_ref: None,
                    skills: Vec::new(),
                    mcp: Vec::new(),
                }),
            ],
            selected: EditorSelection::Loop("loop-fix-tests".to_owned()),
            revisions: vec![
                EditorRevision {
                    version: 4,
                    label: "Working draft · updated loop exit condition".to_owned(),
                    timestamp: "Unsaved".to_owned(),
                },
                EditorRevision {
                    version: 3,
                    label: "Added Report failure fallback branch".to_owned(),
                    timestamp: "Today · 08:12".to_owned(),
                },
                EditorRevision {
                    version: 2,
                    label: "Switched Implement fix step to Codex".to_owned(),
                    timestamp: "Yesterday · 16:40".to_owned(),
                },
            ],
            next_id_counter: 10,
        }
    }

    /// Builds an editable workflow document from an overview entry, using the rich
    /// `Fix issue` reference fixture whenever the entry is `Fix issue`.
    pub fn from_overview_entry(project_name: &str, entry: &WorkflowOverviewEntry) -> Self {
        Self::from_overview_entry_with_demo(project_name, entry, true)
    }

    /// Builds an editable workflow document from an overview entry, optionally using
    /// the mocked `Fix issue` demo fixture when `use_demo_fixture` is enabled.
    pub fn from_overview_entry_with_demo(
        project_name: &str,
        entry: &WorkflowOverviewEntry,
        use_demo_fixture: bool,
    ) -> Self {
        if use_demo_fixture
            && (entry.id == "fix-issue" || entry.name.eq_ignore_ascii_case("Fix issue"))
        {
            let mut doc = Self::sample_fix_issue(project_name);
            doc.workflow_id = entry.id.clone();
            doc.workflow_name = entry.name.clone();
            return doc;
        }

        let mut counter: u32 = 1;
        let mut nodes = Vec::new();
        let mut default_selection: Option<EditorSelection> = None;

        for group in &entry.step_groups {
            match group {
                WorkflowStepGroup::Single(step) => {
                    let id = format!("step-{counter}");
                    counter += 1;
                    if default_selection.is_none() {
                        default_selection = Some(EditorSelection::Step(id.clone()));
                    }
                    nodes.push(EditorNode::Step(EditorStep {
                        id,
                        name: step.name.clone(),
                        kind: infer_step_kind(&step.actor, &step.name),
                        actor_badge: step.actor.clone(),
                        subtitle: None,
                        detail: format!("Execute '{}' via {}.", step.name, step.actor),
                        agent_ref: None,
                        composite_ref: None,
                        skills: Vec::new(),
                        mcp: Vec::new(),
                    }));
                }
                WorkflowStepGroup::Loop { title, steps, .. } => {
                    let loop_id = format!("loop-{counter}");
                    counter += 1;
                    let mut inner_steps = Vec::new();
                    for child in steps {
                        let sid = format!("step-{counter}");
                        counter += 1;
                        inner_steps.push(EditorStep {
                            id: sid,
                            name: child.name.clone(),
                            kind: infer_step_kind(&child.actor, &child.name),
                            actor_badge: child.actor.clone(),
                            subtitle: if inner_steps.is_empty() {
                                Some("reads $loop.previous on attempt 2+".to_owned())
                            } else {
                                None
                            },
                            detail: format!("Execute '{}' via {}.", child.name, child.actor),
                            agent_ref: None,
                            composite_ref: None,
                            skills: Vec::new(),
                            mcp: Vec::new(),
                        });
                    }
                    let exit_step_id = inner_steps
                        .last()
                        .map(|s| s.id.clone())
                        .unwrap_or_else(|| "step-1".to_owned());
                    let clean_title = title.split(" · ").next().unwrap_or(title).trim().to_owned();
                    default_selection = Some(EditorSelection::Loop(loop_id.clone()));
                    nodes.push(EditorNode::Loop(EditorLoopBlock {
                        id: loop_id,
                        name: clean_title,
                        repeat_mode: LoopRepeatMode::UntilCheckPasses,
                        exit_conditions: vec![LoopExitCondition {
                            id: format!("cond-{counter}"),
                            step_id: exit_step_id,
                            field: ExitField::ExitCode,
                            operator: ExitOperator::Equals,
                            expected: "0".to_owned(),
                        }],
                        max_attempts: 5,
                        wait_between: LoopWaitBetween::None,
                        pass_previous_outputs: true,
                        discard_file_changes: false,
                        on_exhausted: LoopExhaustedAction::GoToFallbackStep,
                        fallback: EditorFallbackStep {
                            name: "Report failure".to_owned(),
                            action_label: "Comment on issue".to_owned(),
                            outcome_label: "stop run".to_owned(),
                        },
                        steps: inner_steps,
                    }));
                }
            }
        }

        let work_queue_enabled = entry
            .triggers
            .iter()
            .any(|t| t.kind == WorkflowTriggerKind::WorkQueue);

        Self {
            project_name: project_name.to_owned(),
            workflow_id: entry.id.clone(),
            workflow_name: entry.name.clone(),
            description: String::new(),
            is_draft: false,
            mcp_sources: McpSourcesPolicy::All,
            origin: None,
            has_unsaved_changes: false,
            trigger: EditorTriggerConfig {
                work_queue_enabled,
                work_queue_label: if work_queue_enabled {
                    "Work queue · intake rules".to_owned()
                } else {
                    "Work queue disabled".to_owned()
                },
                ad_hoc_allowed: entry.ad_hoc_allowed,
                inputs_summary: if work_queue_enabled {
                    "inputs: issue (optional for ad-hoc) · branch".to_owned()
                } else {
                    "inputs: branch · target_ref".to_owned()
                },
            },
            nodes,
            selected: default_selection.unwrap_or(EditorSelection::Trigger),
            revisions: vec![EditorRevision {
                version: 1,
                label: format!("Initial '{}' workflow definition", entry.name),
                timestamp: entry.edited_label.clone(),
            }],
            next_id_counter: counter + 10,
        }
    }

    /// Clean blank workflow document with no demo steps or fake revision history.
    pub fn empty(project_name: &str, workflow_name: &str) -> Self {
        let name = if workflow_name.trim().is_empty() {
            "Untitled workflow"
        } else {
            workflow_name.trim()
        };
        Self {
            project_name: project_name.to_owned(),
            workflow_id: slugify_kebab(name),
            workflow_name: name.to_owned(),
            description: String::new(),
            is_draft: false,
            mcp_sources: McpSourcesPolicy::All,
            origin: None,
            has_unsaved_changes: false,
            trigger: EditorTriggerConfig {
                work_queue_enabled: false,
                work_queue_label: "Work queue disabled".to_owned(),
                ad_hoc_allowed: true,
                inputs_summary: "inputs: branch".to_owned(),
            },
            nodes: Vec::new(),
            selected: EditorSelection::Trigger,
            revisions: Vec::new(),
            next_id_counter: 1,
        }
    }

    /// Fallback constructor when opening an editor by workflow name without an overview entry.
    pub fn for_workflow_name(project_name: &str, workflow_name: &str) -> Self {
        if workflow_name.eq_ignore_ascii_case("Fix issue") {
            return Self::sample_fix_issue(project_name);
        }
        let mut doc = Self::sample_fix_issue(project_name);
        doc.workflow_id = slugify_kebab(workflow_name);
        doc.workflow_name = workflow_name.to_owned();
        doc.has_unsaved_changes = false;
        doc
    }

    /// Returns the step number (1-indexed) for a given step ID across top-level and loop steps.
    pub fn step_number(&self, step_id: &str) -> Option<u32> {
        let mut num = 1u32;
        for node in &self.nodes {
            match node {
                EditorNode::Step(step) => {
                    if step.id == step_id {
                        return Some(num);
                    }
                    num += 1;
                }
                EditorNode::Loop(block) => {
                    for step in &block.steps {
                        if step.id == step_id {
                            return Some(num);
                        }
                        num += 1;
                    }
                }
            }
        }
        None
    }

    /// Returns the `(first_step_number, last_step_number)` wrapped by a loop block.
    pub fn loop_step_range(&self, loop_id: &str) -> Option<(u32, u32)> {
        let mut num = 1u32;
        for node in &self.nodes {
            match node {
                EditorNode::Step(_) => {
                    num += 1;
                }
                EditorNode::Loop(block) => {
                    let count = block.steps.len() as u32;
                    if block.id == loop_id {
                        return if count == 0 {
                            None
                        } else {
                            Some((num, num + count - 1))
                        };
                    }
                    num += count;
                }
            }
        }
        None
    }

    /// Returns the currently highlighted palette step type matching the active selection.
    pub fn highlighted_palette_kind(&self) -> Option<StepTypeKind> {
        match &self.selected {
            EditorSelection::Loop(_) => Some(StepTypeKind::Loop),
            EditorSelection::Step(step_id) => self.find_step(step_id).map(|s| s.kind),
            EditorSelection::Fallback(_) => Some(StepTypeKind::CommentOnIssue),
            EditorSelection::Trigger => None,
        }
    }

    pub fn find_step(&self, step_id: &str) -> Option<&EditorStep> {
        for node in &self.nodes {
            match node {
                EditorNode::Step(step) if step.id == step_id => return Some(step),
                EditorNode::Loop(block) => {
                    if let Some(step) = block.steps.iter().find(|s| s.id == step_id) {
                        return Some(step);
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub fn find_step_mut(&mut self, step_id: &str) -> Option<&mut EditorStep> {
        for node in &mut self.nodes {
            match node {
                EditorNode::Step(step) if step.id == step_id => return Some(step),
                EditorNode::Loop(block) => {
                    if let Some(step) = block.steps.iter_mut().find(|s| s.id == step_id) {
                        return Some(step);
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub fn find_loop(&self, loop_id: &str) -> Option<&EditorLoopBlock> {
        self.nodes.iter().find_map(|node| match node {
            EditorNode::Loop(block) if block.id == loop_id => Some(block),
            _ => None,
        })
    }

    pub fn find_loop_mut(&mut self, loop_id: &str) -> Option<&mut EditorLoopBlock> {
        self.nodes.iter_mut().find_map(|node| match node {
            EditorNode::Loop(block) if block.id == loop_id => Some(block),
            _ => None,
        })
    }

    pub fn total_step_count(&self) -> usize {
        self.nodes
            .iter()
            .map(|node| match node {
                EditorNode::Step(_) => 1,
                EditorNode::Loop(block) => block.steps.len(),
            })
            .sum()
    }

    pub fn loop_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| matches!(node, EditorNode::Loop(_)))
            .count()
    }

    /// Appends a new step or loop block from the left palette and selects it.
    pub fn add_from_palette(&mut self, kind: StepTypeKind) -> String {
        let id_num = self.next_id_counter;
        self.next_id_counter += 1;

        if kind == StepTypeKind::Loop {
            let loop_id = format!("loop-{id_num}");
            let step_a_id = format!("step-{id_num}-a");
            let step_b_id = format!("step-{id_num}-b");
            self.nodes.push(EditorNode::Loop(EditorLoopBlock {
                id: loop_id.clone(),
                name: "Retry until check passes".to_owned(),
                repeat_mode: LoopRepeatMode::UntilCheckPasses,
                exit_conditions: vec![LoopExitCondition {
                    id: format!("cond-{id_num}"),
                    step_id: step_b_id.clone(),
                    field: ExitField::ExitCode,
                    operator: ExitOperator::Equals,
                    expected: "0".to_owned(),
                }],
                max_attempts: 3,
                wait_between: LoopWaitBetween::None,
                pass_previous_outputs: true,
                discard_file_changes: false,
                on_exhausted: LoopExhaustedAction::GoToFallbackStep,
                fallback: EditorFallbackStep {
                    name: "Report failure".to_owned(),
                    action_label: "Comment on issue".to_owned(),
                    outcome_label: "stop run".to_owned(),
                },
                steps: vec![
                    EditorStep {
                        id: step_a_id,
                        name: "Apply adjustment".to_owned(),
                        kind: StepTypeKind::CodexTask,
                        actor_badge: "Codex".to_owned(),
                        subtitle: Some("reads $loop.previous on attempt 2+".to_owned()),
                        detail: "Apply the next fix iteration.".to_owned(),
                        agent_ref: None,
                        composite_ref: None,
                        skills: Vec::new(),
                        mcp: Vec::new(),
                    },
                    EditorStep {
                        id: step_b_id,
                        name: "Verify check".to_owned(),
                        kind: StepTypeKind::ShellCommand,
                        actor_badge: "Shell · npm test".to_owned(),
                        subtitle: None,
                        detail: "npm test".to_owned(),
                        agent_ref: None,
                        composite_ref: None,
                        skills: Vec::new(),
                        mcp: Vec::new(),
                    },
                ],
            }));
            self.selected = EditorSelection::Loop(loop_id.clone());
            self.has_unsaved_changes = true;
            return loop_id;
        }

        let step_id = format!("step-{id_num}");
        self.nodes.push(EditorNode::Step(EditorStep {
            id: step_id.clone(),
            name: kind.title().to_owned(),
            kind,
            actor_badge: kind.default_actor_badge().to_owned(),
            subtitle: None,
            detail: kind.subtitle().to_owned(),
            agent_ref: None,
            composite_ref: None,
            skills: Vec::new(),
            mcp: Vec::new(),
        }));
        self.selected = EditorSelection::Step(step_id.clone());
        self.has_unsaved_changes = true;
        step_id
    }

    pub fn remove_step(&mut self, step_id: &str) -> bool {
        if self.total_step_count() <= 1 {
            return false;
        }
        let mut removed = false;
        self.nodes.retain_mut(|node| match node {
            EditorNode::Step(step) => {
                if step.id == step_id {
                    removed = true;
                    false
                } else {
                    true
                }
            }
            EditorNode::Loop(block) => {
                let before = block.steps.len();
                if before > 1 {
                    block.steps.retain(|s| s.id != step_id);
                    if block.steps.len() != before {
                        removed = true;
                        let fallback_step = block.steps.last().map(|s| s.id.clone());
                        if let Some(fallback_id) = fallback_step {
                            for cond in &mut block.exit_conditions {
                                if cond.step_id == step_id {
                                    cond.step_id = fallback_id.clone();
                                }
                            }
                        }
                    }
                }
                true
            }
        });
        if removed {
            self.has_unsaved_changes = true;
            self.selected = self
                .nodes
                .first()
                .map(|node| match node {
                    EditorNode::Step(s) => EditorSelection::Step(s.id.clone()),
                    EditorNode::Loop(l) => EditorSelection::Loop(l.id.clone()),
                })
                .unwrap_or(EditorSelection::Trigger);
        }
        removed
    }

    pub fn set_loop_name(&mut self, loop_id: &str, name: &str) {
        if let Some(block) = self.find_loop_mut(loop_id)
            && block.name != name
        {
            block.name = name.to_owned();
            self.has_unsaved_changes = true;
        }
    }

    pub fn set_loop_repeat_mode(&mut self, loop_id: &str, mode: LoopRepeatMode) {
        if let Some(block) = self.find_loop_mut(loop_id)
            && block.repeat_mode != mode
        {
            block.repeat_mode = mode;
            self.has_unsaved_changes = true;
        }
    }

    pub fn set_loop_max_attempts(&mut self, loop_id: &str, max_attempts: u32) {
        let clamped = max_attempts.clamp(1, 50);
        if let Some(block) = self.find_loop_mut(loop_id)
            && block.max_attempts != clamped
        {
            block.max_attempts = clamped;
            self.has_unsaved_changes = true;
        }
    }

    pub fn set_loop_wait_between(&mut self, loop_id: &str, wait: LoopWaitBetween) {
        if let Some(block) = self.find_loop_mut(loop_id)
            && block.wait_between != wait
        {
            block.wait_between = wait;
            self.has_unsaved_changes = true;
        }
    }

    pub fn toggle_loop_pass_previous(&mut self, loop_id: &str) {
        if let Some(block) = self.find_loop_mut(loop_id) {
            block.pass_previous_outputs = !block.pass_previous_outputs;
            if let Some(first_step) = block.steps.first_mut() {
                first_step.subtitle = if block.pass_previous_outputs {
                    Some("reads $loop.previous on attempt 2+".to_owned())
                } else {
                    None
                };
            }
            self.has_unsaved_changes = true;
        }
    }

    pub fn toggle_loop_discard_changes(&mut self, loop_id: &str) {
        if let Some(block) = self.find_loop_mut(loop_id) {
            block.discard_file_changes = !block.discard_file_changes;
            self.has_unsaved_changes = true;
        }
    }

    pub fn set_loop_on_exhausted(&mut self, loop_id: &str, action: LoopExhaustedAction) {
        if let Some(block) = self.find_loop_mut(loop_id)
            && block.on_exhausted != action
        {
            block.on_exhausted = action;
            self.has_unsaved_changes = true;
        }
    }

    pub fn add_loop_exit_condition(&mut self, loop_id: &str) {
        let next_id = self.next_id_counter;
        self.next_id_counter += 1;
        if let Some(block) = self.find_loop_mut(loop_id) {
            let default_step_id = block
                .steps
                .last()
                .map(|s| s.id.clone())
                .unwrap_or_else(|| "step-1".to_owned());
            block.exit_conditions.push(LoopExitCondition {
                id: format!("cond-{next_id}"),
                step_id: default_step_id,
                field: ExitField::Status,
                operator: ExitOperator::Equals,
                expected: "ok".to_owned(),
            });
            self.has_unsaved_changes = true;
        }
    }

    pub fn remove_loop_exit_condition(&mut self, loop_id: &str, condition_id: &str) {
        if let Some(block) = self.find_loop_mut(loop_id)
            && block.exit_conditions.len() > 1
        {
            let before = block.exit_conditions.len();
            block.exit_conditions.retain(|c| c.id != condition_id);
            if block.exit_conditions.len() != before {
                self.has_unsaved_changes = true;
            }
        }
    }

    pub fn update_loop_exit_condition(
        &mut self,
        loop_id: &str,
        condition_id: &str,
        step_id: Option<String>,
        field: Option<ExitField>,
        operator: Option<ExitOperator>,
        expected: Option<String>,
    ) {
        if let Some(block) = self.find_loop_mut(loop_id)
            && let Some(cond) = block
                .exit_conditions
                .iter_mut()
                .find(|c| c.id == condition_id)
        {
            let mut changed = false;
            if let Some(sid) = step_id
                && cond.step_id != sid
            {
                cond.step_id = sid;
                changed = true;
            }
            if let Some(f) = field
                && cond.field != f
            {
                cond.field = f;
                changed = true;
            }
            if let Some(op) = operator
                && cond.operator != op
            {
                cond.operator = op;
                changed = true;
            }
            if let Some(exp) = expected
                && cond.expected != exp
            {
                cond.expected = exp;
                changed = true;
            }
            if changed {
                self.has_unsaved_changes = true;
            }
        }
    }

    pub fn update_step_name(&mut self, step_id: &str, name: &str) {
        if let Some(step) = self.find_step_mut(step_id)
            && step.name != name
        {
            step.name = name.to_owned();
            self.has_unsaved_changes = true;
        }
    }

    pub fn update_step_detail(&mut self, step_id: &str, detail: &str) {
        if let Some(step) = self.find_step_mut(step_id)
            && step.detail != detail
        {
            step.detail = detail.to_owned();
            if step.kind == StepTypeKind::ShellCommand {
                let cmd = detail.trim();
                step.actor_badge = if cmd.is_empty() {
                    "Shell".to_owned()
                } else {
                    format!("Shell · {cmd}")
                };
            }
            self.has_unsaved_changes = true;
        }
    }

    pub fn update_fallback_name(&mut self, loop_id: &str, name: &str) {
        if let Some(block) = self.find_loop_mut(loop_id)
            && block.fallback.name != name
        {
            block.fallback.name = name.to_owned();
            self.has_unsaved_changes = true;
        }
    }

    pub fn toggle_trigger_work_queue(&mut self) {
        self.trigger.work_queue_enabled = !self.trigger.work_queue_enabled;
        self.has_unsaved_changes = true;
    }

    pub fn toggle_trigger_ad_hoc(&mut self) {
        self.trigger.ad_hoc_allowed = !self.trigger.ad_hoc_allowed;
        self.has_unsaved_changes = true;
    }

    /// Switches between giving steps the user's own MCP servers too, or only the project's.
    pub fn toggle_mcp_project_only(&mut self) {
        self.mcp_sources = match self.mcp_sources {
            McpSourcesPolicy::All => McpSourcesPolicy::Project,
            McpSourcesPolicy::Project => McpSourcesPolicy::All,
        };
        self.has_unsaved_changes = true;
    }

    /// Validates the workflow graph and returns structured diagnostics.
    pub fn validate(&self) -> ValidationReport {
        let mut issues = Vec::new();
        if self.workflow_name.trim().is_empty() {
            issues.push("Workflow name cannot be empty.".to_owned());
        }
        if !self.trigger.work_queue_enabled && !self.trigger.ad_hoc_allowed {
            issues.push("Enable at least one trigger (Work queue or Ad-hoc runs).".to_owned());
        }
        if self.total_step_count() == 0 {
            issues.push("Workflow must include at least one step.".to_owned());
        }
        for node in &self.nodes {
            match node {
                EditorNode::Step(step) => {
                    if step.name.trim().is_empty() {
                        issues.push("Every step must have a name.".to_owned());
                    }
                }
                EditorNode::Loop(block) => {
                    if block.name.trim().is_empty() {
                        issues.push("Every loop block must have a name.".to_owned());
                    }
                    if block.steps.is_empty() {
                        issues.push(format!("Loop '{}' has no inner steps.", block.name));
                    }
                    if block.max_attempts == 0 {
                        issues.push(format!(
                            "Loop '{}' must allow at least 1 attempt.",
                            block.name
                        ));
                    }
                    if block.repeat_mode == LoopRepeatMode::UntilCheckPasses
                        && block.exit_conditions.is_empty()
                    {
                        issues.push(format!(
                            "Loop '{}' requires at least one exit condition.",
                            block.name
                        ));
                    }
                }
            }
        }

        let is_valid = issues.is_empty();
        let summary = if is_valid {
            let steps = self.total_step_count();
            let loops = self.loop_count();
            if loops == 1 {
                format!("Valid workflow — {steps} steps, 1 loop, all exit conditions resolved.")
            } else if loops > 1 {
                format!(
                    "Valid workflow — {steps} steps, {loops} loops, all exit conditions resolved."
                )
            } else {
                format!("Valid workflow — {steps} steps, ready to run.")
            }
        } else {
            format!(
                "Found {} validation issue{}.",
                issues.len(),
                if issues.len() == 1 { "" } else { "s" }
            )
        };

        ValidationReport {
            is_valid,
            summary,
            issues,
        }
    }

    /// Marks the document saved and records a snapshot in its revision history.
    pub fn mark_saved(&mut self) {
        if self.has_unsaved_changes {
            let next_version = self
                .revisions
                .first()
                .map(|r| {
                    if r.timestamp == "Unsaved" {
                        r.version
                    } else {
                        r.version + 1
                    }
                })
                .unwrap_or(1);
            if self
                .revisions
                .first()
                .is_some_and(|r| r.timestamp == "Unsaved")
            {
                self.revisions.remove(0);
            }
            self.revisions.insert(
                0,
                EditorRevision {
                    version: next_version,
                    label: format!(
                        "Saved '{}' ({} steps)",
                        self.workflow_name,
                        self.total_step_count()
                    ),
                    timestamp: "Just now".to_owned(),
                },
            );
            self.has_unsaved_changes = false;
        }
    }

    /// Converts the edited visual workflow back into `WorkflowStepGroup` previews for the
    /// Workflows overview inspector.
    pub fn to_step_groups(&self) -> Vec<WorkflowStepGroup> {
        let mut num = 1u32;
        let mut groups = Vec::new();
        for node in &self.nodes {
            match node {
                EditorNode::Step(step) => {
                    groups.push(WorkflowStepGroup::Single(WorkflowStepPreview {
                        number: num,
                        name: step.name.clone(),
                        actor: step.actor_badge.clone(),
                    }));
                    num += 1;
                }
                EditorNode::Loop(block) => {
                    let mut child_previews = Vec::new();
                    for step in &block.steps {
                        child_previews.push(WorkflowStepPreview {
                            number: num,
                            name: step.name.clone(),
                            actor: step.actor_badge.clone(),
                        });
                        num += 1;
                    }
                    let cond_label = match block.repeat_mode {
                        LoopRepeatMode::UntilCheckPasses => "until pass",
                        LoopRepeatMode::ForEachItem => "for each",
                        LoopRepeatMode::FixedCount => "fixed",
                    };
                    groups.push(WorkflowStepGroup::Loop {
                        title: format!("{} · max {}", block.name, block.max_attempts),
                        condition_label: cond_label.to_owned(),
                        steps: child_previews,
                    });
                }
            }
        }
        groups
    }

    /// Builds a [`WorkflowEditorDocument`] directly from a catalog [`WorkflowDef`].
    pub fn from_workflow_def(
        project_name: &str,
        workflow: &WorkflowDef,
        catalog: &BrukCatalog,
    ) -> Self {
        let mut nodes = Vec::new();
        let mut default_selection: Option<EditorSelection> = None;

        for node in &workflow.nodes {
            match node {
                WorkflowNodeDef::Loop(lp) => {
                    let steps: Vec<EditorStep> = lp
                        .steps
                        .iter()
                        .enumerate()
                        .map(|(idx, child)| {
                            let mut step = editor_step_from_node(child, catalog);
                            if idx == 0 && lp.pass_previous_outputs && step.subtitle.is_none() {
                                step.subtitle =
                                    Some("reads $loop.previous on attempt 2+".to_owned());
                            }
                            step
                        })
                        .collect();
                    let exit_conditions = lp
                        .exit_conditions
                        .iter()
                        .enumerate()
                        .map(|(idx, cond)| LoopExitCondition {
                            id: format!("cond-{}", idx + 1),
                            step_id: cond.step.clone(),
                            field: cond.field,
                            operator: cond.operator,
                            expected: cond.expected.clone(),
                        })
                        .collect();
                    let fallback = match lp.fallback.as_deref() {
                        Some(WorkflowNodeDef::CommentOnIssue(s)) => EditorFallbackStep {
                            name: s.name.clone(),
                            action_label: "Comment on issue".to_owned(),
                            outcome_label: "stop run".to_owned(),
                        },
                        Some(other) => EditorFallbackStep {
                            name: other.name().to_owned(),
                            action_label: "Fallback step".to_owned(),
                            outcome_label: "stop run".to_owned(),
                        },
                        None => EditorFallbackStep {
                            name: "Report failure".to_owned(),
                            action_label: "Comment on issue".to_owned(),
                            outcome_label: "stop run".to_owned(),
                        },
                    };
                    if default_selection.is_none() {
                        default_selection = Some(EditorSelection::Loop(lp.id.clone()));
                    }
                    nodes.push(EditorNode::Loop(EditorLoopBlock {
                        id: lp.id.clone(),
                        name: lp.name.clone(),
                        repeat_mode: lp.repeat,
                        exit_conditions,
                        max_attempts: lp.max_attempts.max(1),
                        wait_between: lp.wait_between,
                        pass_previous_outputs: lp.pass_previous_outputs,
                        discard_file_changes: lp.discard_file_changes,
                        on_exhausted: lp.on_exhausted,
                        fallback,
                        steps,
                    }));
                }
                other => {
                    let step = editor_step_from_node(other, catalog);
                    if default_selection.is_none() {
                        default_selection = Some(EditorSelection::Step(step.id.clone()));
                    }
                    nodes.push(EditorNode::Step(step));
                }
            }
        }

        let inputs_summary = if workflow.inputs.is_empty() {
            if workflow.triggers.work_queue {
                "inputs: issue (optional for ad-hoc) · branch".to_owned()
            } else {
                "inputs: prompt · branch".to_owned()
            }
        } else {
            let names: Vec<&str> = workflow.inputs.keys().map(String::as_str).collect();
            format!("inputs: {}", names.join(" · "))
        };

        let origin_path = workflow
            .origin
            .display_path("workflows", &format!("{}.yaml", workflow.id));

        Self {
            project_name: project_name.to_owned(),
            workflow_id: workflow.id.clone(),
            workflow_name: workflow.name.clone(),
            description: workflow.description.clone(),
            is_draft: workflow.draft,
            mcp_sources: workflow.mcp_sources,
            origin: Some(workflow.origin.clone()),
            has_unsaved_changes: false,
            trigger: EditorTriggerConfig {
                work_queue_enabled: workflow.triggers.work_queue,
                work_queue_label: if workflow.triggers.work_queue {
                    "Work queue · intake rules".to_owned()
                } else {
                    "Work queue disabled".to_owned()
                },
                ad_hoc_allowed: workflow.triggers.ad_hoc,
                inputs_summary,
            },
            nodes,
            selected: default_selection.unwrap_or(EditorSelection::Trigger),
            revisions: vec![EditorRevision {
                version: 1,
                label: format!("Loaded from {origin_path}"),
                timestamp: workflow.origin.badge_label(),
            }],
            next_id_counter: 20,
        }
    }

    /// Converts this editor document back into a serializable [`WorkflowDef`] for
    /// `.agents/bruk/workflows/<id>.yaml`.
    pub fn to_workflow_def(&self) -> WorkflowDef {
        let id = {
            let clean = slugify_kebab(&self.workflow_id);
            if clean.is_empty() {
                slugify_kebab(&self.workflow_name)
            } else {
                clean
            }
        };
        let nodes = self
            .nodes
            .iter()
            .map(editor_node_to_workflow_node)
            .collect();
        WorkflowDef {
            id,
            name: self.workflow_name.clone(),
            description: self.description.clone(),
            draft: self.is_draft,
            triggers: WorkflowTriggersDef {
                work_queue: self.trigger.work_queue_enabled,
                ad_hoc: self.trigger.ad_hoc_allowed,
                issue_labels: Vec::new(),
            },
            inputs: BTreeMap::new(),
            mcp_sources: self.mcp_sources,
            nodes,
            origin: self.origin.clone().unwrap_or_default(),
        }
    }

    /// Appends a real composite from the catalog to the workflow spine and selects it.
    pub fn add_catalog_composite(&mut self, composite: &CompositeDef) -> String {
        let id_num = self.next_id_counter;
        self.next_id_counter += 1;
        let step_id = format!("step-{id_num}");
        self.nodes.push(EditorNode::Step(EditorStep {
            id: step_id.clone(),
            name: composite.name.clone(),
            kind: StepTypeKind::CompositeFixUntilTestsPass,
            actor_badge: format!("Composite · {}", composite.id),
            subtitle: Some(composite.origin.badge_label()),
            detail: composite.description.clone(),
            agent_ref: None,
            composite_ref: Some(composite.id.clone()),
            skills: Vec::new(),
            mcp: Vec::new(),
        }));
        self.selected = EditorSelection::Step(step_id.clone());
        self.has_unsaved_changes = true;
        step_id
    }

    /// Assigns or clears the agent reference for an agent step.
    pub fn update_step_agent(&mut self, step_id: &str, agent_ref: Option<&str>) {
        if let Some(step) = self.find_step_mut(step_id) {
            let normalized = agent_ref
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            if step.agent_ref != normalized {
                step.agent_ref = normalized.clone();
                let provider_prefix = match step.kind {
                    StepTypeKind::CodexTask => "Codex",
                    StepTypeKind::AntigravityTask => "Antigravity",
                    StepTypeKind::ClaudeTask => "Claude",
                    _ => "Agent",
                };
                step.actor_badge = match normalized {
                    Some(agent) => format!("{provider_prefix} · {agent}"),
                    None => provider_prefix.to_owned(),
                };
                self.has_unsaved_changes = true;
            }
        }
    }

    /// Updates extra step-level skills from a comma-separated input string.
    pub fn update_step_skills(&mut self, step_id: &str, raw_skills: &str) {
        if let Some(step) = self.find_step_mut(step_id) {
            let parsed: Vec<String> = raw_skills
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            if step.skills != parsed {
                step.skills = parsed;
                self.has_unsaved_changes = true;
            }
        }
    }

    /// Runs structural editor validation plus catalog reference checks for agents, skills, and composites.
    pub fn validate_with_catalog(&self, catalog: &BrukCatalog) -> ValidationReport {
        let mut report = self.validate();
        for node in &self.nodes {
            let steps: &[EditorStep] = match node {
                EditorNode::Step(s) => std::slice::from_ref(s),
                EditorNode::Loop(lp) => &lp.steps,
            };
            for step in steps {
                if let Some(agent_ref) = step.agent_ref.as_deref()
                    && let Err(err) = catalog.resolve_agent(agent_ref)
                {
                    report.issues.push(format!("Step '{}': {err}", step.name));
                }
                for skill_ref in &step.skills {
                    if catalog.resolve_skill(skill_ref).is_none() {
                        report.issues.push(format!(
                            "Step '{}' references unknown skill '{skill_ref}'.",
                            step.name
                        ));
                    }
                }
                if let Some(comp_ref) = step.composite_ref.as_deref()
                    && catalog.resolve_composite(comp_ref).is_none()
                {
                    report.issues.push(format!(
                        "Step '{}' references unknown composite '{comp_ref}'.",
                        step.name
                    ));
                }
            }
        }
        report.is_valid = report.issues.is_empty();
        if !report.is_valid {
            report.summary = format!(
                "Found {} validation issue{}.",
                report.issues.len(),
                if report.issues.len() == 1 { "" } else { "s" }
            );
        }
        report
    }
}

fn editor_step_from_node(node: &WorkflowNodeDef, catalog: &BrukCatalog) -> EditorStep {
    match node {
        WorkflowNodeDef::AgentTask(step) => {
            let resolved_provider = step.provider.or_else(|| {
                step.agent
                    .as_deref()
                    .and_then(|a| catalog.resolve_agent(a).ok())
                    .and_then(|r| r.provider)
            });
            let kind = match resolved_provider {
                Some(ProviderKind::Codex) => StepTypeKind::CodexTask,
                Some(ProviderKind::Antigravity) => StepTypeKind::AntigravityTask,
                Some(ProviderKind::OpenRouter) => StepTypeKind::ModelCall,
                _ => StepTypeKind::ClaudeTask,
            };
            let provider_label = resolved_provider.map(|p| p.label()).unwrap_or("Claude");
            let actor_badge = match step.agent.as_deref() {
                Some(agent) => format!("{provider_label} · {agent}"),
                None => provider_label.to_owned(),
            };
            let subtitle = if !step.skills.is_empty() {
                Some(format!("skills: {}", step.skills.join(", ")))
            } else {
                None
            };
            EditorStep {
                id: step.id.clone(),
                name: step.name.clone(),
                kind,
                actor_badge,
                subtitle,
                detail: step.prompt.clone(),
                agent_ref: step.agent.clone(),
                composite_ref: None,
                skills: step.skills.clone(),
                mcp: step.mcp.clone(),
            }
        }
        WorkflowNodeDef::ShellCommand(step) => {
            let cmd = step.command.trim();
            EditorStep {
                id: step.id.clone(),
                name: step.name.clone(),
                kind: StepTypeKind::ShellCommand,
                actor_badge: if cmd.is_empty() {
                    "Shell".to_owned()
                } else {
                    format!("Shell · {cmd}")
                },
                subtitle: None,
                detail: step.command.clone(),
                agent_ref: None,
                composite_ref: None,
                skills: Vec::new(),
                mcp: Vec::new(),
            }
        }
        WorkflowNodeDef::ApprovalGate(step) => EditorStep {
            id: step.id.clone(),
            name: step.name.clone(),
            kind: StepTypeKind::ApprovalGate,
            actor_badge: "You".to_owned(),
            subtitle: None,
            detail: step.prompt.clone(),
            agent_ref: None,
            composite_ref: None,
            skills: Vec::new(),
            mcp: Vec::new(),
        },
        WorkflowNodeDef::Composite(step) => {
            let comp = catalog.resolve_composite(&step.uses);
            let kind = match step.uses.as_str() {
                "review-and-open-pr" | "builtin/review-and-open-pr" => {
                    StepTypeKind::CompositeReviewAndOpenPr
                }
                "two-model-review" | "builtin/two-model-review" => {
                    StepTypeKind::CompositeTwoModelReview
                }
                _ => StepTypeKind::CompositeFixUntilTestsPass,
            };
            EditorStep {
                id: step.id.clone(),
                name: step.name.clone(),
                kind,
                actor_badge: format!("Composite · {}", step.uses),
                subtitle: comp.map(|c| c.origin.badge_label()),
                detail: comp
                    .map(|c| c.description.clone())
                    .unwrap_or_else(|| format!("Uses composite '{}'", step.uses)),
                agent_ref: None,
                composite_ref: Some(step.uses.clone()),
                skills: Vec::new(),
                mcp: Vec::new(),
            }
        }
        WorkflowNodeDef::CommentOnIssue(step) => EditorStep {
            id: step.id.clone(),
            name: step.name.clone(),
            kind: StepTypeKind::CommentOnIssue,
            actor_badge: "Issue source".to_owned(),
            subtitle: None,
            detail: step.body.clone(),
            agent_ref: None,
            composite_ref: None,
            skills: Vec::new(),
            mcp: Vec::new(),
        },
        WorkflowNodeDef::SetIssueStatus(step) => EditorStep {
            id: step.id.clone(),
            name: step.name.clone(),
            kind: StepTypeKind::SetIssueStatus,
            actor_badge: "Issue source".to_owned(),
            subtitle: None,
            detail: step.add_labels.join(", "),
            agent_ref: None,
            composite_ref: None,
            skills: Vec::new(),
            mcp: Vec::new(),
        },
        WorkflowNodeDef::OpenPullRequest(step) => EditorStep {
            id: step.id.clone(),
            name: step.name.clone(),
            kind: StepTypeKind::CompositeReviewAndOpenPr,
            actor_badge: "GitHub PR".to_owned(),
            subtitle: None,
            detail: step.title.clone(),
            agent_ref: None,
            composite_ref: None,
            skills: Vec::new(),
            mcp: Vec::new(),
        },
        WorkflowNodeDef::Condition(step) => EditorStep {
            id: step.id.clone(),
            name: step.name.clone(),
            kind: StepTypeKind::Condition,
            actor_badge: "Condition".to_owned(),
            subtitle: None,
            detail: step.expression.clone(),
            agent_ref: None,
            composite_ref: None,
            skills: Vec::new(),
            mcp: Vec::new(),
        },
        WorkflowNodeDef::Loop(lp) => EditorStep {
            id: lp.id.clone(),
            name: lp.name.clone(),
            kind: StepTypeKind::Loop,
            actor_badge: "Loop".to_owned(),
            subtitle: None,
            detail: String::new(),
            agent_ref: None,
            composite_ref: None,
            skills: Vec::new(),
            mcp: Vec::new(),
        },
    }
}

fn editor_node_to_workflow_node(node: &EditorNode) -> WorkflowNodeDef {
    match node {
        EditorNode::Step(step) => editor_step_to_workflow_node(step),
        EditorNode::Loop(lp) => WorkflowNodeDef::Loop(LoopNodeDef {
            id: lp.id.clone(),
            name: lp.name.clone(),
            repeat: lp.repeat_mode,
            max_attempts: lp.max_attempts.max(1),
            wait_between: lp.wait_between,
            pass_previous_outputs: lp.pass_previous_outputs,
            discard_file_changes: lp.discard_file_changes,
            exit_conditions: lp
                .exit_conditions
                .iter()
                .map(|c| LoopExitConditionDef {
                    step: c.step_id.clone(),
                    field: c.field,
                    operator: c.operator,
                    expected: c.expected.clone(),
                })
                .collect(),
            on_exhausted: lp.on_exhausted,
            fallback: Some(Box::new(WorkflowNodeDef::CommentOnIssue(
                CommentOnIssueStepDef {
                    id: format!("{}_fallback", slugify_identifier(&lp.id)),
                    name: lp.fallback.name.clone(),
                    body: format!("Loop '{}' exhausted max attempts.", lp.name),
                },
            ))),
            steps: lp.steps.iter().map(editor_step_to_workflow_node).collect(),
        }),
    }
}

fn editor_step_to_workflow_node(step: &EditorStep) -> WorkflowNodeDef {
    if let Some(comp_ref) = step.composite_ref.as_deref() {
        return WorkflowNodeDef::Composite(CompositeStepDef {
            id: step.id.clone(),
            name: step.name.clone(),
            uses: comp_ref.to_owned(),
            with: BTreeMap::new(),
        });
    }
    match step.kind {
        StepTypeKind::ClaudeTask => WorkflowNodeDef::AgentTask(AgentTaskStepDef {
            id: step.id.clone(),
            name: step.name.clone(),
            agent: step.agent_ref.clone(),
            provider: Some(ProviderKind::Claude),
            model: None,
            skills: step.skills.clone(),
            prompt: step.detail.clone(),
            inputs: BTreeMap::new(),
            outputs: Vec::new(),
            mcp: step.mcp.clone(),
        }),
        StepTypeKind::CodexTask => WorkflowNodeDef::AgentTask(AgentTaskStepDef {
            id: step.id.clone(),
            name: step.name.clone(),
            agent: step.agent_ref.clone(),
            provider: Some(ProviderKind::Codex),
            model: None,
            skills: step.skills.clone(),
            prompt: step.detail.clone(),
            inputs: BTreeMap::new(),
            outputs: Vec::new(),
            mcp: step.mcp.clone(),
        }),
        StepTypeKind::AntigravityTask => WorkflowNodeDef::AgentTask(AgentTaskStepDef {
            id: step.id.clone(),
            name: step.name.clone(),
            agent: step.agent_ref.clone(),
            provider: Some(ProviderKind::Antigravity),
            model: None,
            skills: step.skills.clone(),
            prompt: step.detail.clone(),
            inputs: BTreeMap::new(),
            outputs: Vec::new(),
            mcp: step.mcp.clone(),
        }),
        StepTypeKind::ModelCall => WorkflowNodeDef::AgentTask(AgentTaskStepDef {
            id: step.id.clone(),
            name: step.name.clone(),
            agent: step.agent_ref.clone(),
            provider: Some(ProviderKind::OpenRouter),
            model: None,
            skills: step.skills.clone(),
            prompt: step.detail.clone(),
            inputs: BTreeMap::new(),
            outputs: Vec::new(),
            mcp: step.mcp.clone(),
        }),
        StepTypeKind::ShellCommand => WorkflowNodeDef::ShellCommand(ShellCommandStepDef {
            id: step.id.clone(),
            name: step.name.clone(),
            command: step.detail.clone(),
        }),
        StepTypeKind::ApprovalGate => WorkflowNodeDef::ApprovalGate(ApprovalGateStepDef {
            id: step.id.clone(),
            name: step.name.clone(),
            prompt: step.detail.clone(),
        }),
        StepTypeKind::CommentOnIssue | StepTypeKind::ExtensionPostUpdate => {
            WorkflowNodeDef::CommentOnIssue(CommentOnIssueStepDef {
                id: step.id.clone(),
                name: step.name.clone(),
                body: step.detail.clone(),
            })
        }
        StepTypeKind::SetIssueStatus => WorkflowNodeDef::SetIssueStatus(SetIssueStatusStepDef {
            id: step.id.clone(),
            name: step.name.clone(),
            add_labels: step
                .detail
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
            remove_labels: Vec::new(),
        }),
        StepTypeKind::Condition => WorkflowNodeDef::Condition(ConditionStepDef {
            id: step.id.clone(),
            name: step.name.clone(),
            expression: step.detail.clone(),
        }),
        StepTypeKind::CompositeFixUntilTestsPass => WorkflowNodeDef::Composite(CompositeStepDef {
            id: step.id.clone(),
            name: step.name.clone(),
            uses: "fix-until-tests-pass".to_owned(),
            with: BTreeMap::new(),
        }),
        StepTypeKind::CompositeReviewAndOpenPr => {
            if step.actor_badge == "GitHub PR" {
                WorkflowNodeDef::OpenPullRequest(OpenPullRequestStepDef {
                    id: step.id.clone(),
                    name: step.name.clone(),
                    title: step.detail.clone(),
                    body: String::new(),
                    draft: None,
                })
            } else {
                WorkflowNodeDef::Composite(CompositeStepDef {
                    id: step.id.clone(),
                    name: step.name.clone(),
                    uses: "review-and-open-pr".to_owned(),
                    with: BTreeMap::new(),
                })
            }
        }
        StepTypeKind::CompositeTwoModelReview => WorkflowNodeDef::Composite(CompositeStepDef {
            id: step.id.clone(),
            name: step.name.clone(),
            uses: "two-model-review".to_owned(),
            with: BTreeMap::new(),
        }),
        StepTypeKind::Loop => WorkflowNodeDef::ShellCommand(ShellCommandStepDef {
            id: step.id.clone(),
            name: step.name.clone(),
            command: step.detail.clone(),
        }),
    }
}

fn infer_step_kind(actor: &str, name: &str) -> StepTypeKind {
    let actor_lower = actor.to_ascii_lowercase();
    let name_lower = name.to_ascii_lowercase();
    if actor_lower.contains("codex") {
        StepTypeKind::CodexTask
    } else if actor_lower.contains("antigravity") {
        StepTypeKind::AntigravityTask
    } else if actor_lower.contains("claude") {
        StepTypeKind::ClaudeTask
    } else if actor_lower.contains("openrouter") {
        StepTypeKind::ModelCall
    } else if actor_lower.contains("shell") {
        StepTypeKind::ShellCommand
    } else if actor_lower == "you"
        || name_lower.contains("review gate")
        || name_lower.contains("approval")
    {
        StepTypeKind::ApprovalGate
    } else if actor_lower.contains("issue") {
        if name_lower.contains("status") || name_lower.contains("label") {
            StepTypeKind::SetIssueStatus
        } else {
            StepTypeKind::CommentOnIssue
        }
    } else {
        StepTypeKind::ClaudeTask
    }
}

fn slugify_identifier(input: &str) -> String {
    let mut out = String::new();
    let mut prev_sep = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if !prev_sep && !out.is_empty() {
            out.push('_');
            prev_sep = true;
        }
    }
    let trimmed = out.trim_end_matches('_');
    if trimmed.is_empty() {
        "step".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn slugify_kebab(input: &str) -> String {
    slugify_identifier(input).replace('_', "-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflows_overview::ProjectWorkflowsOverview;

    #[test]
    fn sample_fix_issue_matches_reference_screenshot_structure() {
        let doc = WorkflowEditorDocument::sample_fix_issue("api-gateway");
        assert_eq!(doc.project_name, "api-gateway");
        assert_eq!(doc.workflow_name, "Fix issue");
        assert!(doc.has_unsaved_changes);
        assert_eq!(doc.total_step_count(), 6);
        assert_eq!(doc.loop_count(), 1);
        assert_eq!(doc.loop_step_range("loop-fix-tests"), Some((2, 3)));
        assert_eq!(doc.step_number("step-read-plan"), Some(1));
        assert_eq!(doc.step_number("step-implement-fix"), Some(2));
        assert_eq!(doc.step_number("step-run-tests"), Some(3));
        assert_eq!(doc.step_number("step-review-gate"), Some(4));
        assert_eq!(doc.step_number("step-open-pr"), Some(5));
        assert_eq!(doc.step_number("step-comment-issue"), Some(6));

        let loop_block = doc.find_loop("loop-fix-tests").unwrap();
        assert_eq!(loop_block.header_badge_label(), "Repeat until · max 5");
        assert_eq!(
            loop_block.exit_expression(),
            "steps.run_tests.exit_code = 0"
        );
        assert_eq!(
            loop_block.exhausted_prefix_and_target(),
            ("Go to", "Report failure".to_owned())
        );
        assert!(loop_block.pass_previous_outputs);
        assert!(!loop_block.discard_file_changes);
        assert_eq!(doc.highlighted_palette_kind(), Some(StepTypeKind::Loop));
    }

    #[test]
    fn loop_mutations_update_badges_expressions_and_unsaved_state() {
        let mut doc = WorkflowEditorDocument::sample_fix_issue("api-gateway");
        doc.mark_saved();
        assert!(!doc.has_unsaved_changes);

        doc.set_loop_max_attempts("loop-fix-tests", 8);
        assert!(doc.has_unsaved_changes);
        assert_eq!(
            doc.find_loop("loop-fix-tests")
                .unwrap()
                .header_badge_label(),
            "Repeat until · max 8"
        );

        doc.add_loop_exit_condition("loop-fix-tests");
        let expr = doc.find_loop("loop-fix-tests").unwrap().exit_expression();
        assert!(expr.contains("steps.run_tests.exit_code = 0 && "));

        doc.toggle_loop_pass_previous("loop-fix-tests");
        assert!(
            doc.find_step("step-implement-fix")
                .unwrap()
                .subtitle
                .is_none()
        );

        doc.toggle_loop_discard_changes("loop-fix-tests");
        assert!(
            doc.find_loop("loop-fix-tests")
                .unwrap()
                .discard_file_changes
        );
    }

    #[test]
    fn add_palette_step_and_validate_document() {
        let mut doc = WorkflowEditorDocument::sample_fix_issue("api-gateway");
        let report = doc.validate();
        assert!(report.is_valid);
        assert!(report.summary.contains("6 steps"));

        let new_id = doc.add_from_palette(StepTypeKind::ExtensionPostUpdate);
        assert_eq!(doc.total_step_count(), 7);
        assert_eq!(doc.selected, EditorSelection::Step(new_id.clone()));
        assert_eq!(doc.step_number(&new_id), Some(7));

        let groups = doc.to_step_groups();
        assert_eq!(groups.len(), 6); // 5 top-level steps + 1 loop wrapping 2 steps
    }

    #[test]
    fn palette_search_filters_by_title_subtitle_and_category() {
        assert!(StepTypeKind::CodexTask.matches_query("codex"));
        assert!(StepTypeKind::ExtensionPostUpdate.matches_query("chat notifier"));
        assert!(StepTypeKind::ApprovalGate.matches_query("control"));
        assert!(!StepTypeKind::ClaudeTask.matches_query("chat notifier"));
    }

    #[test]
    fn builds_editor_document_from_other_overview_workflows() {
        let overview = ProjectWorkflowsOverview::for_project("api-gateway");
        let entry = overview.workflow("release-prep").unwrap();
        let doc = WorkflowEditorDocument::from_overview_entry("api-gateway", entry);
        assert_eq!(doc.workflow_name, "Release prep");
        assert!(!doc.has_unsaved_changes);
        assert!(doc.total_step_count() >= 4);
    }
}
