//! Live workflow run domain model, step transcripts, diffs, coverage, and mocked fixtures.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Active sub-tab inside the workflow run detail pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunDetailTab {
    #[default]
    Output,
    Diff,
    Coverage,
}

impl RunDetailTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Output => "Output",
            Self::Diff => "Diff",
            Self::Coverage => "Coverage",
        }
    }
}

/// Overall execution state of a detailed workflow run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunExecutionState {
    Running,
    Paused,
    NeedsApproval,
    Succeeded,
    Failed,
    Stopped,
}

impl RunExecutionState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Stopped)
    }
}

/// Lifecycle state of an individual step card in the run pipeline strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStepStatus {
    Done,
    Running,
    Paused,
    NeedsApproval,
    Queued,
    Failed,
    Stopped,
}

impl RunStepStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Done => "Done",
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::NeedsApproval => "Needs approval",
            Self::Queued => "Queued",
            Self::Failed => "Failed",
            Self::Stopped => "Stopped",
        }
    }
}

/// Visual tone for a tool call's right-aligned status indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatusTone {
    Neutral,
    Success,
    Running,
    Error,
}

/// Semantic tone for leading test/log badges inside expanded tool output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalBadgeTone {
    Pass,
    Runs,
    Fail,
    Info,
}

/// One structured line inside an expanded command or test output block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalLine {
    pub badge: Option<String>,
    pub badge_tone: TerminalBadgeTone,
    pub text: String,
    pub muted_suffix: Option<String>,
}

/// Summary banner shown above an agent turn when a loop step retries after a prior attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptBanner {
    pub summary: String,
    pub details: String,
}

/// One chronological entry rendered inside a step's Output transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepTranscriptEntry {
    AgentTurn {
        badge: String,
        attempt_banner: Option<AttemptBanner>,
        body: String,
    },
    ToolCall {
        verb: String,
        target: String,
        status_text: String,
        status_tone: ToolStatusTone,
        output_lines: Vec<TerminalLine>,
        expanded: bool,
    },
    UserSteering {
        message: String,
    },
    /// An MCP server that didn't reach the agent, shown as a banner in the step.
    McpProblem {
        problem: crate::mcp::McpProblem,
    },
}

/// Agent and execution sandbox metadata shown in the right-hand step inspector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepAgentInfo {
    pub agent_name: String,
    pub agent_scope: Option<String>,
    pub provider: String,
    pub model: String,
    pub sandbox: String,
    pub config: String,
    pub approvals: String,
}

/// Git checkout / worktree metadata shown in the right-hand step inspector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepGitInfo {
    pub mode_badge: String,
    pub machine: String,
    pub branch: String,
    pub checkout: String,
    pub working_dir: String,
}

/// Timing, loop attempt, and token usage metrics for a single step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepProgressInfo {
    pub loop_label: Option<String>,
    pub exit_when: Option<String>,
    pub elapsed: String,
    pub tokens: String,
}

/// Named input binding passed into a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepIoBinding {
    pub name: String,
    pub source: String,
}

/// Artifact output produced (or pending) by a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepOutputChip {
    pub name: String,
    pub pending: bool,
}

/// Complete detail model for one step in a workflow run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStepDetail {
    pub number: usize,
    pub title: String,
    pub status: RunStepStatus,
    pub duration: Option<String>,
    pub loop_badge: Option<String>,
    pub actor_badge: String,
    pub agent: StepAgentInfo,
    pub git: StepGitInfo,
    pub progress: StepProgressInfo,
    pub inputs: Vec<StepIoBinding>,
    pub outputs: Vec<StepOutputChip>,
    pub transcript: Vec<StepTranscriptEntry>,
    pub next_callout: Option<(String, String)>,
}

/// Line polarity inside a unified diff hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
}

/// One line inside a file diff preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
}

/// One modified file shown in the run's Diff tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffFileChange {
    pub path: String,
    pub additions: usize,
    pub deletions: usize,
    pub hunk_header: String,
    pub lines: Vec<DiffLine>,
}

/// One file row shown in the Coverage extension tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageFileRow {
    pub path: String,
    pub statements_tenths_pct: u16,
    pub branches_tenths_pct: u16,
    pub tests_count: usize,
    pub status: String,
}

/// Budget Guard extension widget state in the inspector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetGuardState {
    pub spent_cents: u64,
    pub limit_cents: u64,
    pub pause_when_reached: bool,
}

impl BudgetGuardState {
    pub fn ratio(&self) -> f32 {
        if self.limit_cents == 0 {
            return 0.0;
        }
        (self.spent_cents as f32 / self.limit_cents as f32).clamp(0.0, 1.0)
    }

    pub fn formatted_spent(&self) -> String {
        format_cents(self.spent_cents)
    }

    pub fn formatted_limit(&self) -> String {
        format_cents(self.limit_cents)
    }
}

/// Sidebar Deploy Watch extension status entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeployWatchEntry {
    pub environment: String,
    pub version: String,
}

/// Errors returned by interactive workflow run mutations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkflowRunError {
    #[error("steering instruction cannot be empty")]
    EmptySteeringInstruction,
    #[error("cannot steer a run that has already finished")]
    RunAlreadyTerminal,
}

/// Complete state of the detailed Workflow Run page (`02 Main.png`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRunViewData {
    pub project_name: String,
    pub workflow_name: String,
    pub run_number: u32,
    pub execution_state: RunExecutionState,
    pub active_step_index: usize,
    pub subtitle: String,
    pub worktree_name: String,
    pub branch_name: String,
    pub active_provider_name: String,
    pub attempt_summary: Option<String>,
    pub total_elapsed: String,
    pub active_extensions_count: usize,
    pub steps: Vec<RunStepDetail>,
    pub diffs: Vec<DiffFileChange>,
    pub coverage: Vec<CoverageFileRow>,
    pub budget_guard: BudgetGuardState,
}

impl WorkflowRunViewData {
    /// Clean empty workflow run view state when no run exists yet.
    pub fn empty(project_name: &str, workflow_name: &str) -> Self {
        Self {
            project_name: project_name.to_owned(),
            workflow_name: workflow_name.to_owned(),
            run_number: 0,
            execution_state: RunExecutionState::Stopped,
            active_step_index: 0,
            subtitle: "No runs recorded for this workflow yet".to_owned(),
            worktree_name: String::new(),
            branch_name: String::new(),
            active_provider_name: String::new(),
            attempt_summary: None,
            total_elapsed: "0:00".to_owned(),
            active_extensions_count: 0,
            steps: Vec::new(),
            diffs: Vec::new(),
            coverage: Vec::new(),
            budget_guard: BudgetGuardState {
                spent_cents: 0,
                limit_cents: 500,
                pause_when_reached: true,
            },
        }
    }

    /// True when no real or demo run has been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.run_number == 0 && self.steps.is_empty()
    }

    /// Builds a mocked detailed run for `(project_name, workflow_name)`.
    ///
    /// `("api-gateway", "Release prep")` matches the `02 Main.png` reference mockup verbatim.
    pub fn for_workflow(project_name: &str, workflow_name: &str) -> Self {
        match (project_name, workflow_name) {
            ("api-gateway", "Release prep") => Self::sample_release_prep(),
            ("api-gateway", "Fix issue") => Self::sample_fix_issue(),
            ("api-gateway", "Triage new issues") => Self::sample_triage_issues(),
            ("api-gateway", "Dependency upgrade") => Self::sample_dependency_upgrade(),
            ("api-gateway", "Docs refresh") => Self::sample_docs_refresh(),
            _ => Self::sample_generic(project_name, workflow_name),
        }
    }

    /// Reference fixture for `api-gateway` · `Release prep` · `Run #42` matching `02 Main.png`.
    pub fn sample_release_prep() -> Self {
        let git = StepGitInfo {
            mode_badge: "Worktree".to_owned(),
            machine: "This Mac".to_owned(),
            branch: "release/2.8".to_owned(),
            checkout: "Worktree of ~/Code/api-gat...".to_owned(),
            working_dir: "~/.bruk/worktrees/run-...".to_owned(),
        };

        let steps = vec![
            RunStepDetail {
                number: 1,
                title: "Gather changes".to_owned(),
                status: RunStepStatus::Done,
                duration: Some("0:41".to_owned()),
                loop_badge: None,
                actor_badge: "OpenRouter · Acme team".to_owned(),
                agent: StepAgentInfo {
                    agent_name: "release-collector".to_owned(),
                    agent_scope: Some("repository".to_owned()),
                    provider: "OpenRouter · Acme team".to_owned(),
                    model: "openrouter/auto".to_owned(),
                    sandbox: "Read only".to_owned(),
                    config: "Isolated · Bruk-managed".to_owned(),
                    approvals: "Auto-approve".to_owned(),
                },
                git: git.clone(),
                progress: StepProgressInfo {
                    loop_label: None,
                    exit_when: None,
                    elapsed: "0:41".to_owned(),
                    tokens: "14.8k in · 2.1k out".to_owned(),
                },
                inputs: vec![
                    StepIoBinding {
                        name: "version".to_owned(),
                        source: "Run input".to_owned(),
                    },
                    StepIoBinding {
                        name: "branch".to_owned(),
                        source: "Run input".to_owned(),
                    },
                ],
                outputs: vec![StepOutputChip {
                    name: "commit-summary".to_owned(),
                    pending: false,
                }],
                transcript: vec![
                    StepTranscriptEntry::AgentTurn {
                        badge: "OR".to_owned(),
                        attempt_banner: None,
                        body: "Collected 19 commits merged since tag v2.7.3 across routing, auth session handling, and Redis rate limiting.".to_owned(),
                    },
                    StepTranscriptEntry::ToolCall {
                        verb: "Ran".to_owned(),
                        target: "git log v2.7.3..HEAD --oneline".to_owned(),
                        status_text: "Exit 0".to_owned(),
                        status_tone: ToolStatusTone::Success,
                        output_lines: vec![
                            TerminalLine {
                                badge: Some("INFO".to_owned()),
                                badge_tone: TerminalBadgeTone::Info,
                                text: "19 commits grouped into 4 features, 6 fixes, 2 docs updates".to_owned(),
                                muted_suffix: None,
                            },
                        ],
                        expanded: false,
                    },
                ],
                next_callout: Some((
                    "Next: draft changelog".to_owned(),
                    "Claude will turn the commit summary into user-facing release notes for v2.8.0.".to_owned(),
                )),
            },
            RunStepDetail {
                number: 2,
                title: "Draft changelog".to_owned(),
                status: RunStepStatus::Done,
                duration: Some("1:12".to_owned()),
                loop_badge: None,
                actor_badge: "Claude".to_owned(),
                agent: StepAgentInfo {
                    agent_name: "release-writer".to_owned(),
                    agent_scope: Some("repository".to_owned()),
                    provider: "Claude CLI".to_owned(),
                    model: "claude-sonnet-4-6".to_owned(),
                    sandbox: "Workspace write".to_owned(),
                    config: "Isolated · Bruk-managed".to_owned(),
                    approvals: "On request".to_owned(),
                },
                git: git.clone(),
                progress: StepProgressInfo {
                    loop_label: None,
                    exit_when: None,
                    elapsed: "1:12".to_owned(),
                    tokens: "26.4k in · 4.9k out".to_owned(),
                },
                inputs: vec![
                    StepIoBinding {
                        name: "commit-summary".to_owned(),
                        source: "Step 1 output".to_owned(),
                    },
                    StepIoBinding {
                        name: "version".to_owned(),
                        source: "Run input".to_owned(),
                    },
                ],
                outputs: vec![StepOutputChip {
                    name: "changelog".to_owned(),
                    pending: false,
                }],
                transcript: vec![
                    StepTranscriptEntry::AgentTurn {
                        badge: "Cl".to_owned(),
                        attempt_banner: None,
                        body: "Drafted the v2.8.0 section in CHANGELOG.md highlighting rate-limit headers, HTTP 429 burst handling, and session refresh stability.".to_owned(),
                    },
                    StepTranscriptEntry::ToolCall {
                        verb: "Edited".to_owned(),
                        target: "CHANGELOG.md".to_owned(),
                        status_text: "+18 −0".to_owned(),
                        status_tone: ToolStatusTone::Neutral,
                        output_lines: Vec::new(),
                        expanded: false,
                    },
                ],
                next_callout: Some((
                    "Next: fix until tests pass".to_owned(),
                    "Codex will bump the package version and loop until the test suite passes cleanly.".to_owned(),
                )),
            },
            RunStepDetail {
                number: 3,
                title: "Fix until tests pass".to_owned(),
                status: RunStepStatus::Running,
                duration: Some("2:05".to_owned()),
                loop_badge: Some("Loop · attempt 2 of 5".to_owned()),
                actor_badge: "Codex".to_owned(),
                agent: StepAgentInfo {
                    agent_name: "implementer".to_owned(),
                    agent_scope: Some("repository".to_owned()),
                    provider: "Codex CLI".to_owned(),
                    model: "Pinned in agent".to_owned(),
                    sandbox: "Workspace write".to_owned(),
                    config: "Isolated · Bruk-managed".to_owned(),
                    approvals: "On request".to_owned(),
                },
                git: git.clone(),
                progress: StepProgressInfo {
                    loop_label: Some("Attempt 2 of 5".to_owned()),
                    exit_when: Some("0 failing tests".to_owned()),
                    elapsed: "2:05".to_owned(),
                    tokens: "48.2k in · 6.1k out".to_owned(),
                },
                inputs: vec![
                    StepIoBinding {
                        name: "changelog".to_owned(),
                        source: "Step 2 output".to_owned(),
                    },
                    StepIoBinding {
                        name: "version".to_owned(),
                        source: "Run input".to_owned(),
                    },
                ],
                outputs: vec![
                    StepOutputChip {
                        name: "diff".to_owned(),
                        pending: false,
                    },
                    StepOutputChip {
                        name: "test-report".to_owned(),
                        pending: true,
                    },
                ],
                transcript: vec![
                    StepTranscriptEntry::AgentTurn {
                        badge: "Cx".to_owned(),
                        attempt_banner: Some(AttemptBanner {
                            summary: "Attempt 1 · 2 tests failed · output passed to attempt 2".to_owned(),
                            details: "FAIL src/version.test.ts — expected APP_VERSION to equal \"2.7.3\", received \"2.8.0\" (2 assertions).".to_owned(),
                        }),
                        body: "Attempt 1 left two failing assertions in src/version.test.ts. I'll update the expected version string there and re-run the full suite.".to_owned(),
                    },
                    StepTranscriptEntry::ToolCall {
                        verb: "Edited".to_owned(),
                        target: "package.json".to_owned(),
                        status_text: "+1 −1".to_owned(),
                        status_tone: ToolStatusTone::Neutral,
                        output_lines: vec![
                            TerminalLine {
                                badge: Some("−".to_owned()),
                                badge_tone: TerminalBadgeTone::Fail,
                                text: "  \"version\": \"2.7.3\",".to_owned(),
                                muted_suffix: None,
                            },
                            TerminalLine {
                                badge: Some("+".to_owned()),
                                badge_tone: TerminalBadgeTone::Pass,
                                text: "  \"version\": \"2.8.0\",".to_owned(),
                                muted_suffix: None,
                            },
                        ],
                        expanded: false,
                    },
                    StepTranscriptEntry::ToolCall {
                        verb: "Ran".to_owned(),
                        target: "npm version 2.8.0 --no-git-tag-version".to_owned(),
                        status_text: "Exit 0".to_owned(),
                        status_tone: ToolStatusTone::Success,
                        output_lines: vec![TerminalLine {
                            badge: Some("INFO".to_owned()),
                            badge_tone: TerminalBadgeTone::Info,
                            text: "v2.8.0".to_owned(),
                            muted_suffix: None,
                        }],
                        expanded: false,
                    },
                    StepTranscriptEntry::ToolCall {
                        verb: "Running".to_owned(),
                        target: "npm test".to_owned(),
                        status_text: "0:38".to_owned(),
                        status_tone: ToolStatusTone::Running,
                        output_lines: vec![
                            TerminalLine {
                                badge: Some("PASS".to_owned()),
                                badge_tone: TerminalBadgeTone::Pass,
                                text: "src/router.test.ts".to_owned(),
                                muted_suffix: Some("(42 tests)".to_owned()),
                            },
                            TerminalLine {
                                badge: Some("PASS".to_owned()),
                                badge_tone: TerminalBadgeTone::Pass,
                                text: "src/auth/session.test.ts".to_owned(),
                                muted_suffix: Some("(18 tests)".to_owned()),
                            },
                            TerminalLine {
                                badge: Some("PASS".to_owned()),
                                badge_tone: TerminalBadgeTone::Pass,
                                text: "src/version.test.ts".to_owned(),
                                muted_suffix: Some("(3 tests)".to_owned()),
                            },
                            TerminalLine {
                                badge: Some("RUNS".to_owned()),
                                badge_tone: TerminalBadgeTone::Runs,
                                text: "src/ratelimit.test.ts".to_owned(),
                                muted_suffix: None,
                            },
                        ],
                        expanded: true,
                    },
                ],
                next_callout: Some((
                    "Next: approval gate".to_owned(),
                    "You'll get a notification to review the diff and test report before the pull request opens.".to_owned(),
                )),
            },
            RunStepDetail {
                number: 4,
                title: "Review gate".to_owned(),
                status: RunStepStatus::NeedsApproval,
                duration: None,
                loop_badge: None,
                actor_badge: "You".to_owned(),
                agent: StepAgentInfo {
                    agent_name: "human-review".to_owned(),
                    agent_scope: Some("gate".to_owned()),
                    provider: "You".to_owned(),
                    model: "Manual decision".to_owned(),
                    sandbox: "No execution".to_owned(),
                    config: "Required before PR".to_owned(),
                    approvals: "Operator sign-off".to_owned(),
                },
                git: git.clone(),
                progress: StepProgressInfo {
                    loop_label: None,
                    exit_when: Some("Approved by operator".to_owned()),
                    elapsed: "—".to_owned(),
                    tokens: "0 in · 0 out".to_owned(),
                },
                inputs: vec![
                    StepIoBinding {
                        name: "diff".to_owned(),
                        source: "Step 3 output".to_owned(),
                    },
                    StepIoBinding {
                        name: "test-report".to_owned(),
                        source: "Step 3 output".to_owned(),
                    },
                ],
                outputs: vec![StepOutputChip {
                    name: "approval".to_owned(),
                    pending: true,
                }],
                transcript: vec![StepTranscriptEntry::AgentTurn {
                    badge: "You".to_owned(),
                    attempt_banner: None,
                    body: "Waiting for Step 3 to finish the test suite before asking for your approval. Once ready, verify the v2.8.0 changelog, version bump, and test report to unlock PR creation.".to_owned(),
                }],
                next_callout: Some((
                    "Next: open pull request".to_owned(),
                    "After you approve, Claude will push release/2.8 and open the release pull request.".to_owned(),
                )),
            },
            RunStepDetail {
                number: 5,
                title: "Open pull request".to_owned(),
                status: RunStepStatus::Queued,
                duration: None,
                loop_badge: None,
                actor_badge: "Claude".to_owned(),
                agent: StepAgentInfo {
                    agent_name: "pr-publisher".to_owned(),
                    agent_scope: Some("repository".to_owned()),
                    provider: "Claude CLI".to_owned(),
                    model: "claude-sonnet-4-6".to_owned(),
                    sandbox: "Workspace write".to_owned(),
                    config: "Isolated · Bruk-managed".to_owned(),
                    approvals: "On request".to_owned(),
                },
                git,
                progress: StepProgressInfo {
                    loop_label: None,
                    exit_when: None,
                    elapsed: "—".to_owned(),
                    tokens: "0 in · 0 out".to_owned(),
                },
                inputs: vec![
                    StepIoBinding {
                        name: "changelog".to_owned(),
                        source: "Step 2 output".to_owned(),
                    },
                    StepIoBinding {
                        name: "approval".to_owned(),
                        source: "Step 4 output".to_owned(),
                    },
                ],
                outputs: vec![StepOutputChip {
                    name: "pull-request".to_owned(),
                    pending: true,
                }],
                transcript: vec![StepTranscriptEntry::AgentTurn {
                    badge: "Cl".to_owned(),
                    attempt_banner: None,
                    body: "Queued behind Review gate. Will push branch release/2.8 and open a GitHub pull request with the v2.8.0 changelog summary.".to_owned(),
                }],
                next_callout: None,
            },
        ];

        let diffs = vec![
            DiffFileChange {
                path: "package.json".to_owned(),
                additions: 1,
                deletions: 1,
                hunk_header: "@@ -1,6 +1,6 @@".to_owned(),
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Context,
                        text: " {".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Context,
                        text: "   \"name\": \"@acme/api-gateway\",".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Removed,
                        text: "-  \"version\": \"2.7.3\",".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "+  \"version\": \"2.8.0\",".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Context,
                        text: "   \"private\": true,".to_owned(),
                    },
                ],
            },
            DiffFileChange {
                path: "src/version.test.ts".to_owned(),
                additions: 2,
                deletions: 2,
                hunk_header: "@@ -8,10 +8,10 @@ describe('version endpoint', () => {".to_owned(),
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Context,
                        text: "   it('returns package release version', () => {".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Removed,
                        text: "-    expect(APP_VERSION).toBe('2.7.3');".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "+    expect(APP_VERSION).toBe('2.8.0');".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Context,
                        text: "   });".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Context,
                        text: "   it('formats X-Gateway-Version header', () => {".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Removed,
                        text: "-    expect(versionHeader()).toBe('api-gateway/2.7.3');".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "+    expect(versionHeader()).toBe('api-gateway/2.8.0');".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Context,
                        text: "   });".to_owned(),
                    },
                ],
            },
            DiffFileChange {
                path: "CHANGELOG.md".to_owned(),
                additions: 7,
                deletions: 0,
                hunk_header: "@@ -1,3 +1,10 @@".to_owned(),
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Context,
                        text: " # Changelog".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "+".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "+## [2.8.0] - 2026-11-21".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "+### Added".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "+- Expose X-RateLimit-* headers on public gateway routes (#322)"
                            .to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "+### Fixed".to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "+- Return HTTP 429 with Retry-After on Redis burst limits (#310)"
                            .to_owned(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "+- Stabilize concurrent session refresh integration test (#318)"
                            .to_owned(),
                    },
                ],
            },
        ];

        let coverage = vec![
            CoverageFileRow {
                path: "src/router.ts".to_owned(),
                statements_tenths_pct: 942,
                branches_tenths_pct: 886,
                tests_count: 42,
                status: "PASS".to_owned(),
            },
            CoverageFileRow {
                path: "src/auth/session.ts".to_owned(),
                statements_tenths_pct: 918,
                branches_tenths_pct: 840,
                tests_count: 18,
                status: "PASS".to_owned(),
            },
            CoverageFileRow {
                path: "src/version.ts".to_owned(),
                statements_tenths_pct: 1000,
                branches_tenths_pct: 1000,
                tests_count: 3,
                status: "PASS".to_owned(),
            },
            CoverageFileRow {
                path: "src/ratelimit.ts".to_owned(),
                statements_tenths_pct: 874,
                branches_tenths_pct: 795,
                tests_count: 14,
                status: "RUNS".to_owned(),
            },
        ];

        Self {
            project_name: "api-gateway".to_owned(),
            workflow_name: "Release prep".to_owned(),
            run_number: 42,
            execution_state: RunExecutionState::Running,
            active_step_index: 2,
            subtitle: "Started 4 min ago by you on This Mac · Ad-hoc run, no issue attached · Inputs: version 2.8.0, branch release/2.8".to_owned(),
            worktree_name: "run-42".to_owned(),
            branch_name: "release/2.8".to_owned(),
            active_provider_name: "Codex".to_owned(),
            attempt_summary: Some("attempt 2 of 5".to_owned()),
            total_elapsed: "4:02".to_owned(),
            active_extensions_count: 4,
            steps,
            diffs,
            coverage,
            budget_guard: BudgetGuardState {
                spent_cents: 84,
                limit_cents: 500,
                pause_when_reached: true,
            },
        }
    }

    fn sample_fix_issue() -> Self {
        let mut run = Self::sample_release_prep();
        run.workflow_name = "Fix issue".to_owned();
        run.run_number = 41;
        run.execution_state = RunExecutionState::NeedsApproval;
        run.active_step_index = 3;
        run.subtitle = "Started 14 min ago · Issue #318 Flaky session refresh test · Branch fix/session-refresh".to_owned();
        run.worktree_name = "run-41".to_owned();
        run.branch_name = "fix/session-refresh".to_owned();
        run.active_provider_name = "Claude".to_owned();
        run.attempt_summary = Some("review gate".to_owned());
        run.total_elapsed = "5:18".to_owned();
        run.budget_guard.spent_cents = 112;
        if let Some(step3) = run.steps.get_mut(2) {
            step3.status = RunStepStatus::Done;
            step3.duration = Some("2:44".to_owned());
        }
        run
    }

    fn sample_triage_issues() -> Self {
        let mut run = Self::sample_release_prep();
        run.workflow_name = "Triage new issues".to_owned();
        run.run_number = 43;
        run.execution_state = RunExecutionState::Running;
        run.active_step_index = 1;
        run.subtitle = "Started 2 min ago · Issue #322 Expose rate-limit headers · Branch feat/rate-limit-headers".to_owned();
        run.worktree_name = "run-43".to_owned();
        run.branch_name = "feat/rate-limit-headers".to_owned();
        run.active_provider_name = "Claude".to_owned();
        run.attempt_summary = Some("step 2 of 5".to_owned());
        run.total_elapsed = "1:54".to_owned();
        run.budget_guard.spent_cents = 46;
        if let Some(step2) = run.steps.get_mut(1) {
            step2.status = RunStepStatus::Running;
        }
        if let Some(step3) = run.steps.get_mut(2) {
            step3.status = RunStepStatus::Queued;
            step3.duration = None;
        }
        run
    }

    fn sample_dependency_upgrade() -> Self {
        let mut run = Self::sample_release_prep();
        run.workflow_name = "Dependency upgrade".to_owned();
        run.run_number = 44;
        run.execution_state = RunExecutionState::Running;
        run.active_step_index = 2;
        run.subtitle =
            "Started 6 min ago · Issue #325 Move CI to Node 24 · Branch chore/node-24".to_owned();
        run.worktree_name = "run-44".to_owned();
        run.branch_name = "chore/node-24".to_owned();
        run.active_provider_name = "Codex".to_owned();
        run.attempt_summary = Some("attempt 1 of 3".to_owned());
        run.total_elapsed = "3:19".to_owned();
        run.budget_guard.spent_cents = 68;
        run
    }

    fn sample_docs_refresh() -> Self {
        let mut run = Self::sample_release_prep();
        run.workflow_name = "Docs refresh".to_owned();
        run.run_number = 39;
        run.execution_state = RunExecutionState::Failed;
        run.active_step_index = 2;
        run.subtitle =
            "Ran 38 min ago · Scheduled docs sync · Branch docs/openapi-refresh".to_owned();
        run.worktree_name = "run-39".to_owned();
        run.branch_name = "docs/openapi-refresh".to_owned();
        run.active_provider_name = "Codex".to_owned();
        run.attempt_summary = Some("failed on step 3".to_owned());
        run.total_elapsed = "3:07".to_owned();
        run.budget_guard.spent_cents = 95;
        if let Some(step3) = run.steps.get_mut(2) {
            step3.status = RunStepStatus::Failed;
            step3.loop_badge = Some("Failed · 2 broken links".to_owned());
        }
        run
    }

    fn sample_generic(project_name: &str, workflow_name: &str) -> Self {
        let mut run = Self::sample_release_prep();
        run.project_name = project_name.to_owned();
        run.workflow_name = workflow_name.to_owned();
        run.run_number = 12;
        run.subtitle = format!(
            "Started 3 min ago by you on This Mac · Ad-hoc run on {project_name} · Branch main"
        );
        run.worktree_name = "run-12".to_owned();
        run.branch_name = "main".to_owned();
        for step in &mut run.steps {
            step.git.branch = "main".to_owned();
            step.git.checkout = format!("Worktree of ~/Code/{project_name}");
        }
        run
    }

    /// Sidebar Deploy Watch extension mock entries.
    pub fn sample_deploy_watch() -> Vec<DeployWatchEntry> {
        vec![
            DeployWatchEntry {
                environment: "staging".to_owned(),
                version: "2.7.3".to_owned(),
            },
            DeployWatchEntry {
                environment: "production".to_owned(),
                version: "2.7.2".to_owned(),
            },
        ]
    }

    /// Pill text shown beside `Run #42`.
    pub fn status_pill_label(&self) -> String {
        let step_num = (self.active_step_index + 1).min(self.steps.len());
        let total = self.steps.len();
        match self.execution_state {
            RunExecutionState::Running => format!("Running · step {step_num} of {total}"),
            RunExecutionState::Paused => format!("Paused · step {step_num} of {total}"),
            RunExecutionState::NeedsApproval => {
                format!("Needs approval · step {step_num} of {total}")
            }
            RunExecutionState::Succeeded => format!("Succeeded · {total} of {total}"),
            RunExecutionState::Failed => format!("Failed · step {step_num} of {total}"),
            RunExecutionState::Stopped => format!("Stopped · step {step_num} of {total}"),
        }
    }

    /// Footer status summary shown on the bottom-left bar.
    pub fn footer_summary(&self) -> String {
        let suffix = self
            .attempt_summary
            .as_deref()
            .map(|s| format!(" · {s}"))
            .unwrap_or_default();
        match self.execution_state {
            RunExecutionState::Running => format!(
                "{} running in worktree {} on {}{}",
                self.active_provider_name, self.worktree_name, self.branch_name, suffix
            ),
            RunExecutionState::Paused => format!(
                "{} paused in worktree {} on {}{}",
                self.active_provider_name, self.worktree_name, self.branch_name, suffix
            ),
            RunExecutionState::NeedsApproval => format!(
                "Waiting for your approval in worktree {} on {}",
                self.worktree_name, self.branch_name
            ),
            RunExecutionState::Succeeded => format!(
                "Run #{} completed in worktree {} on {}",
                self.run_number, self.worktree_name, self.branch_name
            ),
            RunExecutionState::Failed => format!(
                "Run #{} failed in worktree {} on {}",
                self.run_number, self.worktree_name, self.branch_name
            ),
            RunExecutionState::Stopped => format!(
                "Run #{} stopped in worktree {} on {}",
                self.run_number, self.worktree_name, self.branch_name
            ),
        }
    }

    /// Toggles pause/resume for a running workflow.
    pub fn toggle_pause(&mut self) -> bool {
        match self.execution_state {
            RunExecutionState::Running => {
                self.execution_state = RunExecutionState::Paused;
                if let Some(step) = self.steps.get_mut(self.active_step_index) {
                    step.status = RunStepStatus::Paused;
                }
                true
            }
            RunExecutionState::Paused => {
                self.execution_state = RunExecutionState::Running;
                if let Some(step) = self.steps.get_mut(self.active_step_index) {
                    step.status = RunStepStatus::Running;
                }
                true
            }
            _ => false,
        }
    }

    /// Stops a non-terminal run.
    pub fn stop(&mut self) -> bool {
        if self.execution_state.is_terminal() {
            return false;
        }
        self.execution_state = RunExecutionState::Stopped;
        if let Some(step) = self.steps.get_mut(self.active_step_index) {
            step.status = RunStepStatus::Stopped;
        }
        true
    }

    /// Appends a user steering instruction and simulated agent reply to the selected step.
    pub fn steer_step(
        &mut self,
        step_index: usize,
        instruction: &str,
    ) -> Result<(), WorkflowRunError> {
        let trimmed = instruction.trim();
        if trimmed.is_empty() {
            return Err(WorkflowRunError::EmptySteeringInstruction);
        }
        if self.execution_state.is_terminal() {
            return Err(WorkflowRunError::RunAlreadyTerminal);
        }
        let idx = step_index.min(self.steps.len().saturating_sub(1));
        if let Some(step) = self.steps.get_mut(idx) {
            let badge = match step.actor_badge.as_str() {
                "Codex" => "Cx",
                "Claude" => "Cl",
                "You" => "You",
                _ => "OR",
            }
            .to_owned();
            step.transcript.push(StepTranscriptEntry::UserSteering {
                message: trimmed.to_owned(),
            });
            step.transcript.push(StepTranscriptEntry::AgentTurn {
                badge,
                attempt_banner: None,
                body: format!(
                    "Got it — applying your steering instruction (\"{trimmed}\") to step {} ({}).",
                    step.number, step.title
                ),
            });
        }
        Ok(())
    }

    /// Approves the review gate step and marks the run succeeded.
    pub fn approve_review_gate(&mut self) -> bool {
        if self.execution_state.is_terminal() {
            return false;
        }
        for step in &mut self.steps {
            step.status = RunStepStatus::Done;
            if step.duration.is_none() {
                step.duration = Some("0:14".to_owned());
            }
            for output in &mut step.outputs {
                output.pending = false;
            }
        }
        self.execution_state = RunExecutionState::Succeeded;
        self.active_step_index = self.steps.len().saturating_sub(1);
        self.attempt_summary = None;
        true
    }

    /// Toggles the Budget Guard auto-pause checkbox.
    pub fn toggle_budget_pause(&mut self) {
        self.budget_guard.pause_when_reached = !self.budget_guard.pause_when_reached;
    }

    /// Formats a tenths-of-a-percent value such as `942` -> `"94.2%"`.
    pub fn format_tenths_pct(tenths: u16) -> String {
        format!("{}.{}%", tenths / 10, tenths % 10)
    }
}

fn format_cents(cents: u64) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_release_prep_matches_reference_mockup() {
        let run = WorkflowRunViewData::for_workflow("api-gateway", "Release prep");

        assert_eq!(run.project_name, "api-gateway");
        assert_eq!(run.workflow_name, "Release prep");
        assert_eq!(run.run_number, 42);
        assert_eq!(run.status_pill_label(), "Running · step 3 of 5");
        assert_eq!(run.steps.len(), 5);
        assert_eq!(run.diffs.len(), 3);
        assert_eq!(run.coverage.len(), 4);
        assert_eq!(run.steps[0].title, "Gather changes");
        assert_eq!(run.steps[1].title, "Draft changelog");
        assert_eq!(run.steps[2].title, "Fix until tests pass");
        assert_eq!(run.steps[3].title, "Review gate");
        assert_eq!(run.steps[4].title, "Open pull request");
        assert_eq!(run.budget_guard.formatted_spent(), "$0.84");
        assert_eq!(run.budget_guard.formatted_limit(), "$5.00");
        assert!(run.budget_guard.pause_when_reached);
        assert_eq!(
            run.footer_summary(),
            "Codex running in worktree run-42 on release/2.8 · attempt 2 of 5"
        );
    }

    #[test]
    fn pause_resume_steer_budget_and_stop_update_run_state() {
        let mut run = WorkflowRunViewData::for_workflow("api-gateway", "Release prep");

        assert!(run.toggle_pause());
        assert_eq!(run.execution_state, RunExecutionState::Paused);
        assert_eq!(run.status_pill_label(), "Paused · step 3 of 5");
        assert_eq!(run.steps[2].status, RunStepStatus::Paused);

        assert!(run.toggle_pause());
        assert_eq!(run.execution_state, RunExecutionState::Running);
        assert_eq!(run.steps[2].status, RunStepStatus::Running);

        let before = run.steps[2].transcript.len();
        run.steer_step(2, "skip the e2e suite").unwrap();
        assert_eq!(run.steps[2].transcript.len(), before + 2);

        run.toggle_budget_pause();
        assert!(!run.budget_guard.pause_when_reached);

        assert!(run.stop());
        assert_eq!(run.execution_state, RunExecutionState::Stopped);
        assert_eq!(run.status_pill_label(), "Stopped · step 3 of 5");
        assert_eq!(
            run.steer_step(2, "more work"),
            Err(WorkflowRunError::RunAlreadyTerminal)
        );
    }

    #[test]
    fn approve_review_gate_completes_all_steps() {
        let mut run = WorkflowRunViewData::for_workflow("api-gateway", "Release prep");
        assert!(run.approve_review_gate());
        assert_eq!(run.execution_state, RunExecutionState::Succeeded);
        assert_eq!(run.status_pill_label(), "Succeeded · 5 of 5");
        assert!(
            run.steps
                .iter()
                .all(|step| step.status == RunStepStatus::Done)
        );
    }
}
