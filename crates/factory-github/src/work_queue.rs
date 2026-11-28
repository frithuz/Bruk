//! Per-project work queue domain model, intake rules, and mocked issue backlogs.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::github_intake::{
    GitHubFilterClause, GitHubFilterQualifier, GitHubIssue, GitHubSyncSnapshot,
    normalize_assignee_handle,
};

/// Display mode for the project work queue canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkQueueViewMode {
    #[default]
    Board,
    List,
}

impl WorkQueueViewMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Board => "Board",
            Self::List => "List",
        }
    }
}

/// Lifecycle column for an issue tracked in a project's work queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkQueueStage {
    ReadyForAgents,
    InProgress,
    NeedsYou,
    /// Unattended run paused before step 1 because a required secret or manager needs setup/unlock.
    NeedsSetup,
    DoneThisWeek,
}

impl WorkQueueStage {
    pub const ALL: &'static [Self] = &[
        Self::ReadyForAgents,
        Self::InProgress,
        Self::NeedsYou,
        Self::NeedsSetup,
        Self::DoneThisWeek,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ReadyForAgents => "Ready for agents",
            Self::InProgress => "In progress",
            Self::NeedsYou => "Needs you",
            Self::NeedsSetup => "Needs setup",
            Self::DoneThisWeek => "Done this week",
        }
    }
}

/// Maps one or more GitHub search filter clauses (label, assignee, author, milestone, etc.)
/// to a target workflow during intake triage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeRule {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub clauses: Vec<GitHubFilterClause>,
    pub workflow_name: String,
}

impl IntakeRule {
    pub fn by_label(label: impl Into<String>, workflow_name: impl Into<String>) -> Self {
        let label = label.into();
        Self {
            clauses: vec![GitHubFilterClause::new(
                GitHubFilterQualifier::Label,
                &label,
            )],
            label,
            workflow_name: workflow_name.into(),
        }
    }

    pub fn by_assignee(assignee: impl Into<String>, workflow_name: impl Into<String>) -> Self {
        let assignee = normalize_assignee_handle(&assignee.into());
        Self {
            label: String::new(),
            clauses: vec![GitHubFilterClause::new(
                GitHubFilterQualifier::Assignee,
                assignee,
            )],
            workflow_name: workflow_name.into(),
        }
    }

    pub fn with_clauses(
        clauses: Vec<GitHubFilterClause>,
        workflow_name: impl Into<String>,
    ) -> Self {
        let label = if clauses.len() == 1
            && clauses[0].qualifier == GitHubFilterQualifier::Label
            && !clauses[0].negated
        {
            clauses[0].value.clone()
        } else {
            String::new()
        };
        Self {
            label,
            clauses,
            workflow_name: workflow_name.into(),
        }
    }

    /// Returns the effective filter clauses evaluated by this rule.
    pub fn effective_clauses(&self) -> Vec<GitHubFilterClause> {
        if !self.clauses.is_empty() {
            self.clauses.clone()
        } else if !self.label.trim().is_empty() {
            vec![GitHubFilterClause::new(
                GitHubFilterQualifier::Label,
                self.label.trim(),
            )]
        } else {
            Vec::new()
        }
    }

    /// Evaluates whether all clauses of this rule match `issue`.
    pub fn matches_issue(&self, issue: &GitHubIssue) -> bool {
        let clauses = self.effective_clauses();
        if clauses.is_empty() {
            return false;
        }
        clauses
            .iter()
            .all(|clause| clause.matches_locally(issue) == Some(true))
    }

    /// Short matched-rule text shown in the Work Queue inspector.
    pub fn matched_rule_text(&self) -> String {
        let clauses = self.effective_clauses();
        if clauses.len() == 1
            && clauses[0].qualifier == GitHubFilterQualifier::Label
            && !clauses[0].negated
        {
            format!("label {} → {}", clauses[0].value, self.workflow_name)
        } else {
            let summary = clauses
                .iter()
                .map(GitHubFilterClause::human_summary)
                .collect::<Vec<_>>()
                .join(" and ");
            format!("{summary} → {}", self.workflow_name)
        }
    }
}

/// Sidebar summary of a workflow available inside a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorkflowSummary {
    pub name: String,
    pub ad_hoc_only: bool,
    #[serde(default)]
    pub has_failed_run: bool,
}

/// How the workflow run of a work queue item ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueRunOutcome {
    Succeeded,
    Failed,
    Stopped,
}

/// One issue card in a project's work queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkQueueItem {
    pub issue_number: u32,
    pub title: String,
    pub description: String,
    pub label: String,
    pub workflow_name: String,
    pub matched_rule: String,
    pub stage: WorkQueueStage,
    pub run_number: Option<u32>,
    pub status_text: String,
    pub agent_provider: Option<String>,
    pub issue_url: String,
}

/// An open issue in the tracker that currently does not match intake rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnmatchedIssue {
    pub issue_number: u32,
    pub title: String,
    pub labels: Vec<String>,
    pub reason: String,
}

/// Errors produced by work queue mutations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkQueueError {
    #[error("issue #{0} was not found in the work queue")]
    IssueNotFound(u32),
    #[error("no queued issues are ready to start")]
    NoQueuedIssues,
    #[error("issue #{0} cannot be started from its current stage")]
    InvalidStageTransition(u32),
    #[error("workflow name cannot be empty")]
    EmptyWorkflowName,
    #[error("task title cannot be empty")]
    EmptyTaskTitle,
}

/// Summary returned after applying a live GitHub open-issue snapshot to a [`ProjectWorkQueue`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubIntakeDiff {
    pub total_open: usize,
    pub newly_queued: Vec<u32>,
    pub ready_count: usize,
    pub unmatched_count: usize,
}

/// Complete work queue state for one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorkQueue {
    pub project_name: String,
    pub tracker_name: String,
    pub repo_slug: String,
    #[serde(default)]
    pub intake_filters: Vec<GitHubFilterClause>,
    pub intake_rules: Vec<IntakeRule>,
    pub fallback_workflow: String,
    pub workflows: Vec<ProjectWorkflowSummary>,
    pub items: Vec<WorkQueueItem>,
    pub unmatched_issues: Vec<UnmatchedIssue>,
    pub max_agent_slots: u32,
    pub next_run_number: u32,
    pub next_ad_hoc_issue_number: u32,
    sync_backlog: Vec<WorkQueueItem>,
    #[serde(default)]
    issue_labels: BTreeMap<u32, Vec<String>>,
    #[serde(default)]
    issue_snapshots: BTreeMap<u32, GitHubIssue>,
    #[serde(default)]
    unmatched_details: BTreeMap<u32, (String, String)>,
    #[serde(default)]
    skipped_issue_numbers: Vec<u32>,
}

impl ProjectWorkQueue {
    /// Clean empty work queue for a real project with no fake issues or rules.
    pub fn empty(project_name: &str) -> Self {
        Self {
            project_name: project_name.to_owned(),
            tracker_name: String::new(),
            repo_slug: String::new(),
            intake_filters: Vec::new(),
            intake_rules: Vec::new(),
            fallback_workflow: String::new(),
            workflows: Vec::new(),
            items: Vec::new(),
            unmatched_issues: Vec::new(),
            max_agent_slots: 2,
            next_run_number: 1,
            next_ad_hoc_issue_number: 1,
            sync_backlog: Vec::new(),
            issue_labels: BTreeMap::new(),
            issue_snapshots: BTreeMap::new(),
            unmatched_details: BTreeMap::new(),
            skipped_issue_numbers: Vec::new(),
        }
    }

    /// Builds a mocked work queue tailored to `project_name`.
    ///
    /// `"api-gateway"` matches the `04 WorkQueue.png` reference mockup verbatim;
    /// other sample and user-registered projects receive tailored mocked queues.
    pub fn for_project(project_name: &str) -> Self {
        match project_name {
            "api-gateway" => Self::sample_api_gateway(),
            "marketing-site" => Self::sample_marketing_site(),
            "mobile-app" => Self::sample_mobile_app(),
            other => Self::sample_generic(other),
        }
    }

    /// Reference fixture for `api-gateway` matching `04 WorkQueue.png`.
    pub fn sample_api_gateway() -> Self {
        let slug = "acme/api-gateway";
        let url = |num: u32| format!("https://github.com/{slug}/issues/{num}");

        Self {
            project_name: "api-gateway".to_owned(),
            tracker_name: "GitHub Issues".to_owned(),
            repo_slug: slug.to_owned(),
            intake_filters: vec![GitHubFilterClause::new(
                GitHubFilterQualifier::Label,
                "agent-ready",
            )],
            intake_rules: vec![
                IntakeRule::by_label("bug", "Fix issue"),
                IntakeRule::by_label("docs", "Docs refresh"),
            ],
            fallback_workflow: "Triage new issues".to_owned(),
            workflows: vec![
                ProjectWorkflowSummary {
                    name: "Release prep".to_owned(),
                    ad_hoc_only: true,
                    has_failed_run: false,
                },
                ProjectWorkflowSummary {
                    name: "Fix issue".to_owned(),
                    ad_hoc_only: false,
                    has_failed_run: false,
                },
                ProjectWorkflowSummary {
                    name: "Triage new issues".to_owned(),
                    ad_hoc_only: false,
                    has_failed_run: false,
                },
                ProjectWorkflowSummary {
                    name: "Dependency upgrade".to_owned(),
                    ad_hoc_only: false,
                    has_failed_run: false,
                },
                ProjectWorkflowSummary {
                    name: "Docs refresh".to_owned(),
                    ad_hoc_only: false,
                    has_failed_run: true,
                },
            ],
            items: vec![
                WorkQueueItem {
                    issue_number: 331,
                    title: "Add request ID to error responses".to_owned(),
                    description: "Error bodies should include the X-Request-Id we already log, so support can match reports to traces.".to_owned(),
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    matched_rule: "label bug → Fix issue".to_owned(),
                    stage: WorkQueueStage::ReadyForAgents,
                    run_number: None,
                    status_text: "Next in queue".to_owned(),
                    agent_provider: None,
                    issue_url: url(331),
                },
                WorkQueueItem {
                    issue_number: 334,
                    title: "Retry once on 503 from the auth service".to_owned(),
                    description: "Transient 503 responses from the upstream auth service should be retried once with a short jittered backoff before surfacing 502 to callers.".to_owned(),
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    matched_rule: "label bug → Fix issue".to_owned(),
                    stage: WorkQueueStage::ReadyForAgents,
                    run_number: None,
                    status_text: "Queued".to_owned(),
                    agent_provider: None,
                    issue_url: url(334),
                },
                WorkQueueItem {
                    issue_number: 329,
                    title: "Document the rate-limit headers".to_owned(),
                    description: "Add X-RateLimit-Limit, X-RateLimit-Remaining, and Retry-After examples to the OpenAPI spec and developer guide.".to_owned(),
                    label: "docs".to_owned(),
                    workflow_name: "Docs refresh".to_owned(),
                    matched_rule: "label docs → Docs refresh".to_owned(),
                    stage: WorkQueueStage::ReadyForAgents,
                    run_number: None,
                    status_text: "Queued".to_owned(),
                    agent_provider: None,
                    issue_url: url(329),
                },
                WorkQueueItem {
                    issue_number: 325,
                    title: "Move CI to Node 24".to_owned(),
                    description: "Update GitHub Actions workflows, Dockerfile base image, and package engines from Node 20 LTS to Node 24.".to_owned(),
                    label: "chore".to_owned(),
                    workflow_name: "Dependency upgrade".to_owned(),
                    matched_rule: "manual override → Dependency upgrade".to_owned(),
                    stage: WorkQueueStage::InProgress,
                    run_number: Some(44),
                    status_text: "Codex · loop attempt 1 of 3".to_owned(),
                    agent_provider: Some("Codex".to_owned()),
                    issue_url: url(325),
                },
                WorkQueueItem {
                    issue_number: 322,
                    title: "Expose rate-limit headers".to_owned(),
                    description: "Forward Redis token bucket quota headers on all public gateway routes and add integration coverage.".to_owned(),
                    label: "feature".to_owned(),
                    workflow_name: "Triage new issues".to_owned(),
                    matched_rule: "fallback → Triage new issues".to_owned(),
                    stage: WorkQueueStage::InProgress,
                    run_number: Some(43),
                    status_text: "Claude · step 2 of 4".to_owned(),
                    agent_provider: Some("Claude".to_owned()),
                    issue_url: url(322),
                },
                WorkQueueItem {
                    issue_number: 318,
                    title: "Flaky session refresh test".to_owned(),
                    description: "Stabilize the concurrent token refresh integration test by replacing wall-clock sleeps with deterministic clock ticks.".to_owned(),
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    matched_rule: "label bug → Fix issue".to_owned(),
                    stage: WorkQueueStage::NeedsYou,
                    run_number: Some(41),
                    status_text: "Review gate".to_owned(),
                    agent_provider: Some("Claude".to_owned()),
                    issue_url: url(318),
                },
                WorkQueueItem {
                    issue_number: 310,
                    title: "Return 429 instead of 500 on burst".to_owned(),
                    description: "Map Redis limiter rejections cleanly to HTTP 429 with a Retry-After header instead of propagating an unhandled 500.".to_owned(),
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    matched_rule: "label bug → Fix issue".to_owned(),
                    stage: WorkQueueStage::DoneThisWeek,
                    run_number: Some(38),
                    status_text: "PR merged · issue closed".to_owned(),
                    agent_provider: Some("Claude".to_owned()),
                    issue_url: url(310),
                },
                WorkQueueItem {
                    issue_number: 307,
                    title: "Typo in auth error message".to_owned(),
                    description: "Fix spelling of 'unauthorized' in JWT verification middleware error payloads.".to_owned(),
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    matched_rule: "label bug → Fix issue".to_owned(),
                    stage: WorkQueueStage::DoneThisWeek,
                    run_number: Some(35),
                    status_text: "PR merged · issue closed".to_owned(),
                    agent_provider: Some("Codex".to_owned()),
                    issue_url: url(307),
                },
            ],
            unmatched_issues: vec![
                UnmatchedIssue {
                    issue_number: 338,
                    title: "Evaluate Envoy vs custom Rust ingress proxy".to_owned(),
                    labels: vec!["rfc".to_owned(), "architecture".to_owned()],
                    reason: "Missing label agent-ready".to_owned(),
                },
                UnmatchedIssue {
                    issue_number: 336,
                    title: "Add per-tenant circuit breaker metrics".to_owned(),
                    labels: vec!["feature".to_owned(), "observability".to_owned()],
                    reason: "Missing label agent-ready".to_owned(),
                },
                UnmatchedIssue {
                    issue_number: 333,
                    title: "Investigate occasional TLS handshake spike in eu-west".to_owned(),
                    labels: vec!["investigation".to_owned()],
                    reason: "Missing label agent-ready".to_owned(),
                },
                UnmatchedIssue {
                    issue_number: 330,
                    title: "Support multiple JWKS signing keys during rotation".to_owned(),
                    labels: vec!["security".to_owned(), "needs-design".to_owned()],
                    reason: "Missing label agent-ready".to_owned(),
                },
                UnmatchedIssue {
                    issue_number: 328,
                    title: "Audit deprecated v1 billing routes for removal".to_owned(),
                    labels: vec!["chore".to_owned()],
                    reason: "Missing label agent-ready".to_owned(),
                },
                UnmatchedIssue {
                    issue_number: 326,
                    title: "Benchmark gzip vs zstd response compression".to_owned(),
                    labels: vec!["perf".to_owned()],
                    reason: "Missing label agent-ready".to_owned(),
                },
                UnmatchedIssue {
                    issue_number: 324,
                    title: "Clarify SLA wording in partner webhook docs".to_owned(),
                    labels: vec!["docs".to_owned(), "needs-pm".to_owned()],
                    reason: "Missing label agent-ready".to_owned(),
                },
                UnmatchedIssue {
                    issue_number: 320,
                    title: "Migrate staging Redis cluster to Valkey 8".to_owned(),
                    labels: vec!["infra".to_owned()],
                    reason: "Missing label agent-ready".to_owned(),
                },
                UnmatchedIssue {
                    issue_number: 319,
                    title: "Draft Q4 API deprecation schedule".to_owned(),
                    labels: vec!["planning".to_owned()],
                    reason: "Missing label agent-ready".to_owned(),
                },
            ],
            max_agent_slots: 2,
            next_run_number: 45,
            next_ad_hoc_issue_number: 350,
            issue_labels: BTreeMap::new(),
            issue_snapshots: BTreeMap::new(),
            unmatched_details: BTreeMap::new(),
            skipped_issue_numbers: Vec::new(),
            sync_backlog: vec![
                WorkQueueItem {
                    issue_number: 340,
                    title: "Propagate traceparent header to downstream gRPC calls".to_owned(),
                    description: "Ensure incoming W3C traceparent headers are forwarded on gRPC metadata so spans link across services.".to_owned(),
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    matched_rule: "label bug → Fix issue".to_owned(),
                    stage: WorkQueueStage::ReadyForAgents,
                    run_number: None,
                    status_text: "Queued".to_owned(),
                    agent_provider: None,
                    issue_url: url(340),
                },
                WorkQueueItem {
                    issue_number: 342,
                    title: "Document webhook signature verification examples".to_owned(),
                    description: "Add Rust, TypeScript, and Go verification snippets to the webhook docs.".to_owned(),
                    label: "docs".to_owned(),
                    workflow_name: "Docs refresh".to_owned(),
                    matched_rule: "label docs → Docs refresh".to_owned(),
                    stage: WorkQueueStage::ReadyForAgents,
                    run_number: None,
                    status_text: "Queued".to_owned(),
                    agent_provider: None,
                    issue_url: url(342),
                },
            ],
        }
    }

    fn sample_marketing_site() -> Self {
        let slug = "acme/marketing-site";
        let url = |num: u32| format!("https://github.com/{slug}/issues/{num}");

        Self {
            project_name: "marketing-site".to_owned(),
            tracker_name: "GitHub Issues".to_owned(),
            repo_slug: slug.to_owned(),
            intake_filters: vec![GitHubFilterClause::new(
                GitHubFilterQualifier::Label,
                "agent-ready",
            )],
            intake_rules: vec![
                IntakeRule::by_label("bug", "Fix issue"),
                IntakeRule::by_label("docs", "Docs refresh"),
            ],
            fallback_workflow: "Triage new issues".to_owned(),
            workflows: vec![
                ProjectWorkflowSummary {
                    name: "Fix issue".to_owned(),
                    ad_hoc_only: false,
                    has_failed_run: false,
                },
                ProjectWorkflowSummary {
                    name: "Docs refresh".to_owned(),
                    ad_hoc_only: false,
                    has_failed_run: false,
                },
                ProjectWorkflowSummary {
                    name: "Triage new issues".to_owned(),
                    ad_hoc_only: false,
                    has_failed_run: false,
                },
                ProjectWorkflowSummary {
                    name: "Release prep".to_owned(),
                    ad_hoc_only: true,
                    has_failed_run: false,
                },
            ],
            items: vec![
                WorkQueueItem {
                    issue_number: 118,
                    title: "Preload hero font to fix mobile LCP regression".to_owned(),
                    description: "Add font preload links and subset latin glyphs so mobile LCP returns under 1.8s.".to_owned(),
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    matched_rule: "label bug → Fix issue".to_owned(),
                    stage: WorkQueueStage::ReadyForAgents,
                    run_number: None,
                    status_text: "Next in queue".to_owned(),
                    agent_provider: None,
                    issue_url: url(118),
                },
                WorkQueueItem {
                    issue_number: 115,
                    title: "Update enterprise pricing FAQ copy".to_owned(),
                    description: "Refresh annual billing FAQ answers and add SOC2 Type II compliance note.".to_owned(),
                    label: "docs".to_owned(),
                    workflow_name: "Docs refresh".to_owned(),
                    matched_rule: "label docs → Docs refresh".to_owned(),
                    stage: WorkQueueStage::InProgress,
                    run_number: Some(21),
                    status_text: "Claude · step 3 of 4".to_owned(),
                    agent_provider: Some("Claude".to_owned()),
                    issue_url: url(115),
                },
                WorkQueueItem {
                    issue_number: 112,
                    title: "Fix broken changelog RSS timestamps".to_owned(),
                    description: "Emit RFC 2822 dates in the generated changelog RSS feed.".to_owned(),
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    matched_rule: "label bug → Fix issue".to_owned(),
                    stage: WorkQueueStage::DoneThisWeek,
                    run_number: Some(19),
                    status_text: "PR merged · issue closed".to_owned(),
                    agent_provider: Some("Claude".to_owned()),
                    issue_url: url(112),
                },
            ],
            unmatched_issues: vec![
                UnmatchedIssue {
                    issue_number: 121,
                    title: "Explore interactive pricing calculator redesign".to_owned(),
                    labels: vec!["design".to_owned()],
                    reason: "Missing label agent-ready".to_owned(),
                },
                UnmatchedIssue {
                    issue_number: 119,
                    title: "Add Japanese localization scaffolding".to_owned(),
                    labels: vec!["i18n".to_owned()],
                    reason: "Missing label agent-ready".to_owned(),
                },
            ],
            max_agent_slots: 2,
            next_run_number: 22,
            next_ad_hoc_issue_number: 130,
            sync_backlog: Vec::new(),
            issue_labels: BTreeMap::new(),
            issue_snapshots: BTreeMap::new(),
            unmatched_details: BTreeMap::new(),
            skipped_issue_numbers: Vec::new(),
        }
    }

    fn sample_mobile_app() -> Self {
        let slug = "acme/mobile-app";
        let url = |num: u32| format!("https://linear.app/acme/issue/MOB-{num}");

        Self {
            project_name: "mobile-app".to_owned(),
            tracker_name: "Linear".to_owned(),
            repo_slug: slug.to_owned(),
            intake_filters: vec![GitHubFilterClause::new(
                GitHubFilterQualifier::Label,
                "agent-ready",
            )],
            intake_rules: vec![
                IntakeRule::by_label("bug", "Fix issue"),
                IntakeRule::by_label("docs", "Docs refresh"),
            ],
            fallback_workflow: "Triage new issues".to_owned(),
            workflows: vec![
                ProjectWorkflowSummary {
                    name: "Fix issue".to_owned(),
                    ad_hoc_only: false,
                    has_failed_run: false,
                },
                ProjectWorkflowSummary {
                    name: "Triage new issues".to_owned(),
                    ad_hoc_only: false,
                    has_failed_run: false,
                },
                ProjectWorkflowSummary {
                    name: "Dependency upgrade".to_owned(),
                    ad_hoc_only: false,
                    has_failed_run: false,
                },
                ProjectWorkflowSummary {
                    name: "Release prep".to_owned(),
                    ad_hoc_only: true,
                    has_failed_run: false,
                },
            ],
            items: vec![
                WorkQueueItem {
                    issue_number: 204,
                    title: "Fix crash when opening push notification from cold start".to_owned(),
                    description: "Guard deep-link router initialization until the auth store finishes hydrating.".to_owned(),
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    matched_rule: "label bug → Fix issue".to_owned(),
                    stage: WorkQueueStage::ReadyForAgents,
                    run_number: None,
                    status_text: "Next in queue".to_owned(),
                    agent_provider: None,
                    issue_url: url(204),
                },
                WorkQueueItem {
                    issue_number: 201,
                    title: "Match splash screen background in dark mode".to_owned(),
                    description: "Avoid brief white flash on launch when macOS/iOS dark appearance is active.".to_owned(),
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    matched_rule: "label bug → Fix issue".to_owned(),
                    stage: WorkQueueStage::NeedsYou,
                    run_number: Some(14),
                    status_text: "Review gate".to_owned(),
                    agent_provider: Some("Codex".to_owned()),
                    issue_url: url(201),
                },
                WorkQueueItem {
                    issue_number: 196,
                    title: "Cache timeline avatars offline".to_owned(),
                    description: "Persist decoded avatar thumbnails in the local disk cache with 7-day eviction.".to_owned(),
                    label: "feature".to_owned(),
                    workflow_name: "Triage new issues".to_owned(),
                    matched_rule: "fallback → Triage new issues".to_owned(),
                    stage: WorkQueueStage::DoneThisWeek,
                    run_number: Some(11),
                    status_text: "PR merged · issue closed".to_owned(),
                    agent_provider: Some("Claude".to_owned()),
                    issue_url: url(196),
                },
            ],
            unmatched_issues: vec![UnmatchedIssue {
                issue_number: 209,
                title: "Prototype watchOS companion complication".to_owned(),
                labels: vec!["idea".to_owned()],
                reason: "Missing label agent-ready".to_owned(),
            }],
            max_agent_slots: 2,
            next_run_number: 15,
            next_ad_hoc_issue_number: 220,
            sync_backlog: Vec::new(),
            issue_labels: BTreeMap::new(),
            issue_snapshots: BTreeMap::new(),
            unmatched_details: BTreeMap::new(),
            skipped_issue_numbers: Vec::new(),
        }
    }

    fn sample_generic(project_name: &str) -> Self {
        let slug = format!("acme/{project_name}");
        let issue_url = format!("https://github.com/{slug}/issues/12");

        Self {
            project_name: project_name.to_owned(),
            tracker_name: "GitHub Issues".to_owned(),
            repo_slug: slug,
            intake_filters: vec![GitHubFilterClause::new(
                GitHubFilterQualifier::Label,
                "agent-ready",
            )],
            intake_rules: vec![
                IntakeRule::by_label("bug", "Fix issue"),
                IntakeRule::by_label("docs", "Docs refresh"),
            ],
            fallback_workflow: "Triage new issues".to_owned(),
            workflows: vec![
                ProjectWorkflowSummary {
                    name: "Fix issue".to_owned(),
                    ad_hoc_only: false,
                    has_failed_run: false,
                },
                ProjectWorkflowSummary {
                    name: "Triage new issues".to_owned(),
                    ad_hoc_only: false,
                    has_failed_run: false,
                },
                ProjectWorkflowSummary {
                    name: "Docs refresh".to_owned(),
                    ad_hoc_only: false,
                    has_failed_run: false,
                },
                ProjectWorkflowSummary {
                    name: "Release prep".to_owned(),
                    ad_hoc_only: true,
                    has_failed_run: false,
                },
            ],
            items: vec![WorkQueueItem {
                issue_number: 12,
                title: format!("Add health-check telemetry for {project_name}"),
                description: "Expose structured liveness and readiness status checks with request correlation IDs.".to_owned(),
                label: "feature".to_owned(),
                workflow_name: "Triage new issues".to_owned(),
                matched_rule: "fallback → Triage new issues".to_owned(),
                stage: WorkQueueStage::ReadyForAgents,
                run_number: None,
                status_text: "Next in queue".to_owned(),
                agent_provider: None,
                issue_url,
            }],
            unmatched_issues: vec![UnmatchedIssue {
                issue_number: 15,
                title: "Review deployment checklist".to_owned(),
                labels: vec!["ops".to_owned()],
                reason: "Missing label agent-ready".to_owned(),
            }],
            max_agent_slots: 2,
            next_run_number: 1,
            next_ad_hoc_issue_number: 20,
            sync_backlog: Vec::new(),
            issue_labels: BTreeMap::new(),
            issue_snapshots: BTreeMap::new(),
            unmatched_details: BTreeMap::new(),
            skipped_issue_numbers: Vec::new(),
        }
    }

    /// Returns items currently in `stage`, preserving queue order.
    pub fn items_in_stage(&self, stage: WorkQueueStage) -> Vec<&WorkQueueItem> {
        self.items
            .iter()
            .filter(|item| item.stage == stage)
            .collect()
    }

    /// Count of items in `stage`.
    pub fn stage_count(&self, stage: WorkQueueStage) -> usize {
        self.items.iter().filter(|item| item.stage == stage).count()
    }

    /// Active count shown in the sidebar badge (`Ready for agents` + `In progress`).
    pub fn active_queue_count(&self) -> usize {
        self.stage_count(WorkQueueStage::ReadyForAgents)
            + self.stage_count(WorkQueueStage::InProgress)
    }

    /// Short tracker label for sidebar badge (`GitHub`, `Linear`, `itsaplan`, or empty when unconfigured).
    pub fn tracker_short_label(&self) -> &'static str {
        if self.tracker_name.trim().is_empty() {
            ""
        } else if self.tracker_name.contains("Linear") {
            "Linear"
        } else if self.tracker_name.contains("itsaplan") {
            "itsaplan"
        } else {
            "GitHub"
        }
    }

    /// Total historical run count shown beside the sidebar `Run history` entry.
    pub fn total_run_count(&self) -> u32 {
        if self.items.is_empty() && self.next_run_number <= 1 {
            return 0;
        }
        match self.project_name.as_str() {
            "api-gateway" => 42,
            "marketing-site" => 21,
            "mobile-app" => 14,
            _ => self.next_run_number.saturating_sub(1),
        }
    }

    /// Number of currently executing agent runs.
    pub fn busy_slots(&self) -> usize {
        self.stage_count(WorkQueueStage::InProgress)
    }

    /// Number of issues waiting in `Ready for agents`.
    pub fn waiting_count(&self) -> usize {
        self.stage_count(WorkQueueStage::ReadyForAgents)
    }

    /// Footer label shown at the bottom of the sidebar (`2 of 2 busy · 3 waiting`).
    pub fn agent_slots_summary(&self) -> String {
        format!(
            "{} of {} busy · {} waiting",
            self.busy_slots(),
            self.max_agent_slots,
            self.waiting_count()
        )
    }

    /// 1-based queue position `(pos, total)` if `issue_number` is in `ReadyForAgents`.
    pub fn queue_position(&self, issue_number: u32) -> Option<(usize, usize)> {
        let ready = self.items_in_stage(WorkQueueStage::ReadyForAgents);
        let total = ready.len();
        ready
            .iter()
            .position(|item| item.issue_number == issue_number)
            .map(|idx| (idx + 1, total))
    }

    /// Starts text shown in the inspector for a queued issue.
    pub fn starts_label(&self) -> &'static str {
        if self.busy_slots() >= self.max_agent_slots as usize {
            "When an agent slot frees"
        } else {
            "Immediately"
        }
    }

    /// Active run count for a sidebar workflow entry.
    pub fn running_count_for_workflow(&self, workflow_name: &str) -> usize {
        self.items
            .iter()
            .filter(|item| {
                item.workflow_name == workflow_name
                    && matches!(
                        item.stage,
                        WorkQueueStage::InProgress | WorkQueueStage::NeedsYou
                    )
            })
            .count()
    }

    /// Looks up an item by issue number.
    pub fn item(&self, issue_number: u32) -> Option<&WorkQueueItem> {
        self.items
            .iter()
            .find(|item| item.issue_number == issue_number)
    }

    /// Default issue selected when opening the queue.
    pub fn default_selected_issue(&self) -> Option<u32> {
        for stage in WorkQueueStage::ALL {
            if let Some(item) = self.items.iter().find(|item| item.stage == *stage) {
                return Some(item.issue_number);
            }
        }
        None
    }

    /// Resolves `(workflow_name, matched_rule)` for a given issue label.
    pub fn match_workflow_for_label(&self, label: &str) -> (String, String) {
        self.match_workflow_for_labels(&[label.to_owned()])
            .unwrap_or_else(|| {
                let fallback = if self.fallback_workflow.trim().is_empty() {
                    "Unassigned".to_owned()
                } else {
                    self.fallback_workflow.clone()
                };
                let rule = if self.fallback_workflow.trim().is_empty() {
                    "no workflow rule".to_owned()
                } else {
                    format!("fallback → {fallback}")
                };
                (fallback, rule)
            })
    }

    /// Returns all active project-level intake filter clauses.
    pub fn effective_intake_filter_clauses(&self) -> Vec<GitHubFilterClause> {
        let mut clauses: Vec<GitHubFilterClause> = Vec::new();
        for filter in &self.intake_filters {
            if filter.is_empty() {
                continue;
            }
            let key = filter.dedupe_key();
            if !clauses.iter().any(|existing| existing.dedupe_key() == key) {
                clauses.push(filter.clone());
            }
        }
        clauses
    }

    /// Evaluates project-level intake filters against `issue`.
    ///
    /// Returns `None` when `issue` passes all filters (or when `server_filtered` already satisfied
    /// server-side clauses), or `Some(reason)` describing the first failing clause.
    fn first_intake_filter_failure(
        &self,
        issue: &GitHubIssue,
        server_filtered: bool,
    ) -> Option<String> {
        for clause in self.effective_intake_filter_clauses() {
            match clause.matches_locally(issue) {
                Some(true) => {}
                Some(false) => return Some(clause.mismatch_reason()),
                None => {
                    if !server_filtered {
                        return Some(clause.mismatch_reason());
                    }
                }
            }
        }
        None
    }

    /// Resolves `(workflow_name, matched_rule)` against a list of issue labels.
    ///
    /// Returns `None` when specific intake rules are configured without a fallback workflow
    /// and none of `labels` matches any rule.
    pub fn match_workflow_for_labels(&self, labels: &[String]) -> Option<(String, String)> {
        let synthetic = GitHubIssue {
            number: 0,
            title: String::new(),
            body: String::new(),
            labels: labels.to_vec(),
            assignees: Vec::new(),
            author: None,
            milestone: None,
            comments_count: 0,
            created_at: None,
            url: String::new(),
            state: "OPEN".to_owned(),
            updated_at: None,
        };
        self.match_workflow_for_issue(&synthetic)
    }

    /// Resolves `(workflow_name, matched_rule)` against a full [`GitHubIssue`] (labels, assignees,
    /// author, milestone, title/body text, comments, dates, exclusions, etc.).
    pub fn match_workflow_for_issue(&self, issue: &GitHubIssue) -> Option<(String, String)> {
        for rule in &self.intake_rules {
            if rule.matches_issue(issue) {
                return Some((rule.workflow_name.clone(), rule.matched_rule_text()));
            }
        }
        let fallback = self.fallback_workflow.trim();
        if !fallback.is_empty() {
            return Some((fallback.to_owned(), format!("fallback → {fallback}")));
        }
        if self.intake_rules.is_empty() {
            return Some((
                "Unassigned".to_owned(),
                "no workflow rule (assign in inspector)".to_owned(),
            ));
        }
        None
    }

    /// Picks the most descriptive primary badge for an issue card.
    fn pick_primary_badge(&self, issue: &GitHubIssue) -> String {
        for rule in &self.intake_rules {
            if rule.matches_issue(issue) {
                let clauses = rule.effective_clauses();
                if let Some(first) = clauses.first() {
                    match first.qualifier {
                        GitHubFilterQualifier::Label if !first.negated => {
                            if let Some(matched) = issue
                                .labels
                                .iter()
                                .find(|label| label.eq_ignore_ascii_case(&first.value))
                            {
                                return matched.clone();
                            }
                        }
                        GitHubFilterQualifier::Assignee if !first.negated => {
                            let target = normalize_assignee_handle(&first.value);
                            if let Some(matched) = issue.assignees.iter().find(|a| {
                                target == "*"
                                    || normalize_assignee_handle(a).eq_ignore_ascii_case(&target)
                            }) {
                                return format!("@{matched}");
                            }
                        }
                        _ => {}
                    }
                }
                break;
            }
        }
        if let Some(other) = issue.labels.iter().find(|label| {
            !self.intake_filters.iter().any(|f| {
                f.qualifier == GitHubFilterQualifier::Label
                    && !f.negated
                    && f.value.eq_ignore_ascii_case(label)
            })
        }) {
            return other.clone();
        }
        if let Some(assignee) = issue.assignees.first() {
            return format!("@{assignee}");
        }
        issue
            .labels
            .first()
            .cloned()
            .unwrap_or_else(|| "issue".to_owned())
    }

    /// Applies a live [`GitHubSyncSnapshot`] (including server-side `--search` results) to this queue.
    pub fn apply_github_snapshot(&mut self, snapshot: &GitHubSyncSnapshot) -> GitHubIntakeDiff {
        self.apply_github_issues_internal(
            &snapshot.repo,
            &snapshot.open_issues,
            snapshot.server_filtered,
        )
    }

    /// Applies a live open-issues slice from GitHub to this work queue.
    ///
    /// Preserves in-flight items (`InProgress`, `NeedsYou`, `DoneThisWeek`), ad-hoc items,
    /// manual workflow overrides, and explicitly skipped issue numbers while reconciling
    /// `ReadyForAgents` and `unmatched_issues` against the current intake filters and rules.
    pub fn apply_github_issues(
        &mut self,
        repo_slug: &str,
        issues: &[GitHubIssue],
    ) -> GitHubIntakeDiff {
        self.apply_github_issues_internal(repo_slug, issues, false)
    }

    fn apply_github_issues_internal(
        &mut self,
        repo_slug: &str,
        issues: &[GitHubIssue],
        server_filtered: bool,
    ) -> GitHubIntakeDiff {
        let slug = repo_slug.trim();
        if !slug.is_empty() {
            self.repo_slug = slug.to_owned();
        }
        self.tracker_name = "GitHub Issues".to_owned();

        let open_numbers: BTreeSet<u32> = issues.iter().map(|i| i.number).collect();

        // Remove ReadyForAgents tracker items that were closed remotely on GitHub (or no longer
        // match a server-side search filter), keeping ad-hoc and already-active work.
        self.items.retain(|item| {
            item.stage != WorkQueueStage::ReadyForAgents
                || item.label == "ad-hoc"
                || open_numbers.contains(&item.issue_number)
        });

        let mut next_unmatched = Vec::new();
        let mut newly_queued = Vec::new();

        for issue in issues {
            self.issue_labels.insert(issue.number, issue.labels.clone());
            self.issue_snapshots.insert(issue.number, issue.clone());
            let description = if issue.body.trim().is_empty() {
                "No description provided.".to_owned()
            } else {
                issue.body.clone()
            };
            let primary_badge = self.pick_primary_badge(issue);

            // Check if this issue is already tracked in `self.items`.
            if let Some(existing_idx) = self
                .items
                .iter()
                .position(|item| item.issue_number == issue.number)
            {
                let stage = self.items[existing_idx].stage;
                let is_manual = self.items[existing_idx]
                    .matched_rule
                    .starts_with("manual override")
                    || self.items[existing_idx]
                        .description
                        .starts_with("Manually queued");

                self.items[existing_idx].title = issue.title.clone();
                if !is_manual || !issue.body.trim().is_empty() {
                    self.items[existing_idx].description = description.clone();
                }
                self.items[existing_idx].issue_url = issue.url.clone();
                if self.items[existing_idx].label != "ad-hoc" {
                    self.items[existing_idx].label = primary_badge;
                }

                if stage == WorkQueueStage::ReadyForAgents && !is_manual {
                    if let Some(reason) = self.first_intake_filter_failure(issue, server_filtered) {
                        self.items.remove(existing_idx);
                        self.unmatched_details
                            .insert(issue.number, (description, issue.url.clone()));
                        next_unmatched.push(UnmatchedIssue {
                            issue_number: issue.number,
                            title: issue.title.clone(),
                            labels: issue.labels.clone(),
                            reason,
                        });
                    } else if let Some((workflow_name, matched_rule)) =
                        self.match_workflow_for_issue(issue)
                    {
                        self.items[existing_idx].workflow_name = workflow_name;
                        self.items[existing_idx].matched_rule = matched_rule;
                    } else {
                        self.items.remove(existing_idx);
                        self.unmatched_details
                            .insert(issue.number, (description, issue.url.clone()));
                        next_unmatched.push(UnmatchedIssue {
                            issue_number: issue.number,
                            title: issue.title.clone(),
                            labels: issue.labels.clone(),
                            reason: "No matching intake rule".to_owned(),
                        });
                    }
                }
                continue;
            }

            // Issue is not in `self.items` yet.
            if self.skipped_issue_numbers.contains(&issue.number) {
                self.unmatched_details
                    .insert(issue.number, (description, issue.url.clone()));
                next_unmatched.push(UnmatchedIssue {
                    issue_number: issue.number,
                    title: issue.title.clone(),
                    labels: issue.labels.clone(),
                    reason: "Skipped from queue".to_owned(),
                });
                continue;
            }

            if let Some(reason) = self.first_intake_filter_failure(issue, server_filtered) {
                self.unmatched_details
                    .insert(issue.number, (description, issue.url.clone()));
                next_unmatched.push(UnmatchedIssue {
                    issue_number: issue.number,
                    title: issue.title.clone(),
                    labels: issue.labels.clone(),
                    reason,
                });
                continue;
            }

            let Some((workflow_name, matched_rule)) = self.match_workflow_for_issue(issue) else {
                self.unmatched_details
                    .insert(issue.number, (description, issue.url.clone()));
                next_unmatched.push(UnmatchedIssue {
                    issue_number: issue.number,
                    title: issue.title.clone(),
                    labels: issue.labels.clone(),
                    reason: "No matching intake rule".to_owned(),
                });
                continue;
            };

            self.unmatched_details.remove(&issue.number);
            self.items.push(WorkQueueItem {
                issue_number: issue.number,
                title: issue.title.clone(),
                description,
                label: primary_badge,
                workflow_name,
                matched_rule,
                stage: WorkQueueStage::ReadyForAgents,
                run_number: None,
                status_text: "Queued".to_owned(),
                agent_provider: None,
                issue_url: issue.url.clone(),
            });
            newly_queued.push(issue.number);
        }

        // Advance `next_ad_hoc_issue_number` past any real GitHub issue numbers so ad-hoc IDs never collide.
        if let Some(max_issue) = issues.iter().map(|i| i.number).max()
            && self.next_ad_hoc_issue_number <= max_issue
        {
            self.next_ad_hoc_issue_number = max_issue + 1;
        }

        self.unmatched_issues = next_unmatched;
        self.refresh_ready_labels();

        GitHubIntakeDiff {
            total_open: issues.len(),
            newly_queued,
            ready_count: self.stage_count(WorkQueueStage::ReadyForAgents),
            unmatched_count: self.unmatched_issues.len(),
        }
    }

    /// Plain-text intake rule summary sentence for accessibility and non-rich contexts.
    pub fn intake_summary_sentence(&self) -> String {
        let filters = self.effective_intake_filter_clauses();
        if filters.is_empty()
            && self.intake_rules.is_empty()
            && self.fallback_workflow.trim().is_empty()
        {
            return "No issue source or intake rules configured yet. Connect an issue source in Project Settings or add intake rules.".to_owned();
        }
        let intake_prefix = if filters.is_empty() {
            "All open issues become work.".to_owned()
        } else if filters.len() == 1
            && filters[0].qualifier == GitHubFilterQualifier::Label
            && !filters[0].negated
        {
            format!("Open issues labeled {} become work.", filters[0].value)
        } else {
            let summary = filters
                .iter()
                .map(GitHubFilterClause::human_summary)
                .collect::<Vec<_>>()
                .join(" and ");
            format!("Open issues matching {summary} become work.")
        };

        let mut parts = Vec::new();
        for (idx, rule) in self.intake_rules.iter().enumerate() {
            let clauses = rule.effective_clauses();
            let subject = if clauses.len() == 1
                && clauses[0].qualifier == GitHubFilterQualifier::Label
                && !clauses[0].negated
            {
                match clauses[0].value.as_str() {
                    "bug" if idx == 0 => "Bugs go to".to_owned(),
                    "bug" => "bugs to".to_owned(),
                    other if idx == 0 => format!("{} go to", capitalize_first(other)),
                    other => format!("{other} to"),
                }
            } else {
                let cond = clauses
                    .iter()
                    .map(GitHubFilterClause::human_summary)
                    .collect::<Vec<_>>()
                    .join(" and ");
                if idx == 0 {
                    format!("{} go to", capitalize_first(&cond))
                } else {
                    format!("{cond} to")
                }
            };
            parts.push(format!("{subject} {}", rule.workflow_name));
        }
        if parts.is_empty() {
            let fallback = if self.fallback_workflow.trim().is_empty() {
                "your default workflow"
            } else {
                self.fallback_workflow.as_str()
            };
            format!("{intake_prefix} Everything goes to {fallback}.")
        } else if self.fallback_workflow.trim().is_empty() {
            format!("{intake_prefix} {}.", parts.join(", "))
        } else {
            format!(
                "{intake_prefix} {}, everything else to {}.",
                parts.join(", "),
                self.fallback_workflow
            )
        }
    }

    /// Puts a queued issue into the `NeedsSetup` hold state when preflight fails (e.g. a locked
    /// secret manager or missing value), preventing retry storms on subsequent GitHub syncs while
    /// retaining its queue entry and GitHub labels.
    pub fn hold_item_for_setup(&mut self, issue_number: u32, reason: &str) -> bool {
        let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.issue_number == issue_number)
        else {
            return false;
        };
        item.stage = WorkQueueStage::NeedsSetup;
        item.status_text = format!("Needs setup: {reason}");
        true
    }

    /// Moves all `NeedsSetup` held items back to `ReadyForAgents` and returns their issue numbers.
    pub fn resume_setup_held_items(&mut self) -> Vec<u32> {
        let mut resumed = Vec::new();
        for item in &mut self.items {
            if item.stage == WorkQueueStage::NeedsSetup {
                item.stage = WorkQueueStage::ReadyForAgents;
                item.status_text = "Queued".to_owned();
                resumed.push(item.issue_number);
            }
        }
        resumed
    }

    /// Starts a `ReadyForAgents` issue immediately, assigning a new run number.
    pub fn start_issue(&mut self, issue_number: u32) -> Result<u32, WorkQueueError> {
        let idx = self
            .items
            .iter()
            .position(|item| item.issue_number == issue_number)
            .ok_or(WorkQueueError::IssueNotFound(issue_number))?;

        if self.items[idx].stage != WorkQueueStage::ReadyForAgents {
            return Err(WorkQueueError::InvalidStageTransition(issue_number));
        }

        let run_number = self.next_run_number;
        self.next_run_number += 1;

        let item = &mut self.items[idx];
        item.stage = WorkQueueStage::InProgress;
        item.run_number = Some(run_number);
        item.agent_provider = Some("Claude".to_owned());
        item.status_text = "Claude · step 1 of 4".to_owned();

        self.refresh_ready_labels();
        Ok(run_number)
    }

    /// Starts the first issue in `ReadyForAgents`.
    pub fn start_next_queued(&mut self) -> Result<(u32, u32), WorkQueueError> {
        let next_issue = self
            .items_in_stage(WorkQueueStage::ReadyForAgents)
            .first()
            .map(|item| item.issue_number)
            .ok_or(WorkQueueError::NoQueuedIssues)?;
        let run_number = self.start_issue(next_issue)?;
        Ok((next_issue, run_number))
    }

    /// Overrides the workflow assigned to a queued issue.
    pub fn reassign_workflow(
        &mut self,
        issue_number: u32,
        workflow_name: &str,
    ) -> Result<(), WorkQueueError> {
        let workflow_name = workflow_name.trim();
        if workflow_name.is_empty() {
            return Err(WorkQueueError::EmptyWorkflowName);
        }
        let item = self
            .items
            .iter_mut()
            .find(|item| item.issue_number == issue_number)
            .ok_or(WorkQueueError::IssueNotFound(issue_number))?;

        item.workflow_name = workflow_name.to_owned();
        item.matched_rule = format!("manual override → {workflow_name}");
        Ok(())
    }

    /// Removes a queued issue from `ReadyForAgents` and records it under unmatched issues.
    pub fn skip_issue(&mut self, issue_number: u32) -> Result<(), WorkQueueError> {
        let idx = self
            .items
            .iter()
            .position(|item| item.issue_number == issue_number)
            .ok_or(WorkQueueError::IssueNotFound(issue_number))?;

        if self.items[idx].stage != WorkQueueStage::ReadyForAgents {
            return Err(WorkQueueError::InvalidStageTransition(issue_number));
        }

        let removed = self.items.remove(idx);
        if !self.skipped_issue_numbers.contains(&removed.issue_number) {
            self.skipped_issue_numbers.push(removed.issue_number);
        }
        let labels = self
            .issue_labels
            .get(&removed.issue_number)
            .cloned()
            .unwrap_or_else(|| vec![removed.label.clone()]);
        self.unmatched_details.insert(
            removed.issue_number,
            (removed.description.clone(), removed.issue_url.clone()),
        );
        self.unmatched_issues.insert(
            0,
            UnmatchedIssue {
                issue_number: removed.issue_number,
                title: removed.title,
                labels,
                reason: "Skipped from queue".to_owned(),
            },
        );
        self.refresh_ready_labels();
        Ok(())
    }

    /// Approves a `NeedsYou` review gate and moves the issue to `DoneThisWeek`.
    pub fn approve_review(&mut self, issue_number: u32) -> Result<(), WorkQueueError> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.issue_number == issue_number)
            .ok_or(WorkQueueError::IssueNotFound(issue_number))?;

        if item.stage != WorkQueueStage::NeedsYou {
            return Err(WorkQueueError::InvalidStageTransition(issue_number));
        }

        item.stage = WorkQueueStage::DoneThisWeek;
        item.status_text = "PR merged · issue closed".to_owned();
        Ok(())
    }

    /// Sends a `NeedsYou` item back to `InProgress` to address review feedback.
    pub fn request_changes(&mut self, issue_number: u32) -> Result<(), WorkQueueError> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.issue_number == issue_number)
            .ok_or(WorkQueueError::IssueNotFound(issue_number))?;

        if item.stage != WorkQueueStage::NeedsYou {
            return Err(WorkQueueError::InvalidStageTransition(issue_number));
        }

        item.stage = WorkQueueStage::InProgress;
        let provider = item.agent_provider.as_deref().unwrap_or("Claude");
        item.status_text = format!("{provider} · addressing review feedback");
        Ok(())
    }

    /// Stops an `InProgress` run and moves the issue back to `ReadyForAgents`.
    pub fn stop_in_progress(&mut self, issue_number: u32) -> Result<(), WorkQueueError> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.issue_number == issue_number)
            .ok_or(WorkQueueError::IssueNotFound(issue_number))?;

        if item.stage != WorkQueueStage::InProgress {
            return Err(WorkQueueError::InvalidStageTransition(issue_number));
        }

        item.stage = WorkQueueStage::ReadyForAgents;
        item.run_number = None;
        item.agent_provider = None;
        self.refresh_ready_labels();
        Ok(())
    }

    /// Moves an In progress item on when its workflow run ends: to Done when the run succeeded or
    /// failed (Re-queue Issue runs it again), or back to Ready for agents when it was stopped.
    /// Returns whether the item moved; an item no longer In progress is left alone.
    pub fn finish_item_run(
        &mut self,
        issue_number: u32,
        run_number: u32,
        outcome: QueueRunOutcome,
    ) -> bool {
        let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.issue_number == issue_number)
        else {
            return false;
        };
        if item.stage != WorkQueueStage::InProgress {
            return false;
        }
        match outcome {
            QueueRunOutcome::Succeeded => {
                item.stage = WorkQueueStage::DoneThisWeek;
                item.status_text = format!("Run #{run_number} succeeded");
            }
            QueueRunOutcome::Failed => {
                item.stage = WorkQueueStage::DoneThisWeek;
                item.status_text = format!("Run #{run_number} failed");
            }
            QueueRunOutcome::Stopped => {
                item.stage = WorkQueueStage::ReadyForAgents;
                item.status_text = "Queued".to_owned();
                item.run_number = None;
                item.agent_provider = None;
            }
        }
        self.refresh_ready_labels();
        true
    }

    /// Moves items left In progress by a run that is no longer going (Bruk quit during it) back to
    /// Ready for agents, and returns their issue numbers.
    pub fn requeue_interrupted_runs(&mut self, still_running: impl Fn(u32) -> bool) -> Vec<u32> {
        let mut requeued = Vec::new();
        for item in &mut self.items {
            if item.stage == WorkQueueStage::InProgress && !still_running(item.issue_number) {
                item.stage = WorkQueueStage::ReadyForAgents;
                item.status_text = "Queued".to_owned();
                item.run_number = None;
                item.agent_provider = None;
                requeued.push(item.issue_number);
            }
        }
        if !requeued.is_empty() {
            self.refresh_ready_labels();
        }
        requeued
    }

    /// Re-queues a `DoneThisWeek` issue back into `ReadyForAgents`.
    pub fn requeue_issue(&mut self, issue_number: u32) -> Result<(), WorkQueueError> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.issue_number == issue_number)
            .ok_or(WorkQueueError::IssueNotFound(issue_number))?;

        if item.stage != WorkQueueStage::DoneThisWeek {
            return Err(WorkQueueError::InvalidStageTransition(issue_number));
        }

        item.stage = WorkQueueStage::ReadyForAgents;
        item.run_number = None;
        item.agent_provider = None;
        self.refresh_ready_labels();
        Ok(())
    }

    /// Promotes an unmatched issue into `ReadyForAgents` using current intake rules.
    pub fn queue_unmatched_issue(&mut self, issue_number: u32) -> Result<(), WorkQueueError> {
        let idx = self
            .unmatched_issues
            .iter()
            .position(|issue| issue.issue_number == issue_number)
            .ok_or(WorkQueueError::IssueNotFound(issue_number))?;

        let unmatched = self.unmatched_issues.remove(idx);
        self.skipped_issue_numbers
            .retain(|num| *num != unmatched.issue_number);
        let synthetic_issue = self
            .issue_snapshots
            .get(&unmatched.issue_number)
            .cloned()
            .unwrap_or_else(|| GitHubIssue {
                number: unmatched.issue_number,
                title: unmatched.title.clone(),
                body: String::new(),
                labels: unmatched.labels.clone(),
                assignees: Vec::new(),
                author: None,
                milestone: None,
                comments_count: 0,
                created_at: None,
                url: String::new(),
                state: "OPEN".to_owned(),
                updated_at: None,
            });
        let primary_label = self.pick_primary_badge(&synthetic_issue);
        let (workflow_name, matched_rule) = self
            .match_workflow_for_issue(&synthetic_issue)
            .unwrap_or_else(|| self.match_workflow_for_label(&primary_label));
        let (description, issue_url) = self
            .unmatched_details
            .remove(&unmatched.issue_number)
            .unwrap_or_else(|| {
                (
                    format!(
                        "Manually queued from unmatched tracker issues ({}).",
                        unmatched.reason
                    ),
                    format!(
                        "https://github.com/{}/issues/{}",
                        self.repo_slug, unmatched.issue_number
                    ),
                )
            });

        self.items.push(WorkQueueItem {
            issue_number: unmatched.issue_number,
            title: unmatched.title,
            description,
            label: primary_label,
            workflow_name,
            matched_rule,
            stage: WorkQueueStage::ReadyForAgents,
            run_number: None,
            status_text: "Queued".to_owned(),
            agent_provider: None,
            issue_url,
        });
        self.refresh_ready_labels();
        Ok(())
    }

    /// Simulates syncing from the issue tracker, pulling the next mocked issue if available.
    pub fn sync_issues(&mut self) -> Option<WorkQueueItem> {
        if self.sync_backlog.is_empty() {
            return None;
        }
        let mut next = self.sync_backlog.remove(0);
        let (workflow_name, matched_rule) = self.match_workflow_for_label(&next.label);
        next.workflow_name = workflow_name;
        next.matched_rule = matched_rule;
        self.items.push(next.clone());
        self.refresh_ready_labels();
        Some(next)
    }

    /// Launches an ad-hoc run directly into `InProgress`.
    pub fn launch_ad_hoc(
        &mut self,
        workflow_name: &str,
        title: &str,
    ) -> Result<(u32, u32), WorkQueueError> {
        let workflow_name = workflow_name.trim();
        if workflow_name.is_empty() {
            return Err(WorkQueueError::EmptyWorkflowName);
        }
        let title = title.trim();
        if title.is_empty() {
            return Err(WorkQueueError::EmptyTaskTitle);
        }

        let issue_number = self.next_ad_hoc_issue_number;
        self.next_ad_hoc_issue_number += 1;
        let run_number = self.next_run_number;
        self.next_run_number += 1;

        self.items.insert(
            0,
            WorkQueueItem {
                issue_number,
                title: title.to_owned(),
                description: format!("Ad-hoc run launched with workflow '{workflow_name}'."),
                label: "ad-hoc".to_owned(),
                workflow_name: workflow_name.to_owned(),
                matched_rule: format!("ad-hoc → {workflow_name}"),
                stage: WorkQueueStage::InProgress,
                run_number: Some(run_number),
                status_text: "Claude · step 1 of 4".to_owned(),
                agent_provider: Some("Claude".to_owned()),
                issue_url: format!(
                    "https://github.com/{}/issues/{issue_number}",
                    self.repo_slug
                ),
            },
        );
        Ok((issue_number, run_number))
    }

    /// Updates intake filters, routing rules, and fallback workflow, then re-matches queued items.
    pub fn update_intake_rules(
        &mut self,
        intake_filters: Vec<GitHubFilterClause>,
        rules: Vec<IntakeRule>,
        fallback_workflow: String,
    ) {
        let old_single_label = if self.intake_filters.len() == 1
            && self.intake_filters[0].qualifier == GitHubFilterQualifier::Label
            && !self.intake_filters[0].negated
        {
            Some(self.intake_filters[0].value.clone())
        } else {
            None
        };
        self.intake_filters = intake_filters
            .into_iter()
            .filter(|clause| !clause.is_empty())
            .collect();
        if let (Some(old_label), Some(new_clause)) = (old_single_label, self.intake_filters.first())
            && self.intake_filters.len() == 1
            && new_clause.qualifier == GitHubFilterQualifier::Label
            && !new_clause.negated
        {
            let old_reason = format!("Missing label {old_label}");
            let new_reason = format!("Missing label {}", new_clause.value);
            for issue in &mut self.unmatched_issues {
                if issue.reason == old_reason {
                    issue.reason = new_reason.clone();
                }
            }
        }
        self.fallback_workflow = fallback_workflow.trim().to_owned();
        self.intake_rules = rules
            .into_iter()
            .filter(|r| !r.effective_clauses().is_empty() && !r.workflow_name.trim().is_empty())
            .collect();

        let rules_snapshot = self.intake_rules.clone();
        let fallback_snapshot = self.fallback_workflow.clone();
        let labels_snapshot = self.issue_labels.clone();
        let snapshots = self.issue_snapshots.clone();
        for item in &mut self.items {
            if item.stage == WorkQueueStage::ReadyForAgents
                && !item.matched_rule.starts_with("manual override")
            {
                let issue = snapshots
                    .get(&item.issue_number)
                    .cloned()
                    .unwrap_or_else(|| {
                        let item_labels = labels_snapshot
                            .get(&item.issue_number)
                            .cloned()
                            .unwrap_or_else(|| vec![item.label.clone()]);
                        GitHubIssue {
                            number: item.issue_number,
                            title: item.title.clone(),
                            body: item.description.clone(),
                            labels: item_labels,
                            assignees: Vec::new(),
                            author: None,
                            milestone: None,
                            comments_count: 0,
                            created_at: None,
                            url: item.issue_url.clone(),
                            state: "OPEN".to_owned(),
                            updated_at: None,
                        }
                    });
                if let Some(rule) = rules_snapshot
                    .iter()
                    .find(|rule| rule.matches_issue(&issue))
                {
                    item.workflow_name = rule.workflow_name.clone();
                    item.matched_rule = rule.matched_rule_text();
                } else if !fallback_snapshot.is_empty() {
                    item.workflow_name = fallback_snapshot.clone();
                    item.matched_rule = format!("fallback → {fallback_snapshot}");
                } else if rules_snapshot.is_empty() {
                    item.workflow_name = "Unassigned".to_owned();
                    item.matched_rule = "no workflow rule (assign in inspector)".to_owned();
                }
            }
        }
    }

    fn refresh_ready_labels(&mut self) {
        let mut first_seen = false;
        for item in &mut self.items {
            if item.stage == WorkQueueStage::ReadyForAgents {
                if !first_seen {
                    item.status_text = "Next in queue".to_owned();
                    first_seen = true;
                } else {
                    item.status_text = "Queued".to_owned();
                }
            }
        }
    }
}

fn capitalize_first(input: &str) -> String {
    let mut chars = input.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_runs_move_their_items_out_of_in_progress() {
        let mut queue = ProjectWorkQueue::empty("app");
        let (done, _) = queue.launch_ad_hoc("Fix issue", "Works").unwrap();
        let (failed, _) = queue.launch_ad_hoc("Fix issue", "Breaks").unwrap();
        let (stopped, _) = queue.launch_ad_hoc("Fix issue", "Stopped").unwrap();
        assert_eq!(queue.busy_slots(), 3);

        assert!(queue.finish_item_run(done, 7, QueueRunOutcome::Succeeded));
        assert!(queue.finish_item_run(failed, 8, QueueRunOutcome::Failed));
        assert!(queue.finish_item_run(stopped, 9, QueueRunOutcome::Stopped));
        assert_eq!(queue.busy_slots(), 0);

        fn item(queue: &ProjectWorkQueue, n: u32) -> &WorkQueueItem {
            queue.items.iter().find(|i| i.issue_number == n).unwrap()
        }
        assert_eq!(item(&queue, done).stage, WorkQueueStage::DoneThisWeek);
        assert_eq!(item(&queue, done).status_text, "Run #7 succeeded");
        assert_eq!(item(&queue, failed).stage, WorkQueueStage::DoneThisWeek);
        assert_eq!(item(&queue, failed).status_text, "Run #8 failed");
        assert_eq!(item(&queue, stopped).stage, WorkQueueStage::ReadyForAgents);
        // Only an item still In progress moves.
        assert!(!queue.finish_item_run(done, 7, QueueRunOutcome::Failed));
        assert_eq!(item(&queue, done).status_text, "Run #7 succeeded");
    }

    #[test]
    fn items_whose_run_is_gone_are_requeued() {
        let mut queue = ProjectWorkQueue::empty("app");
        let (gone, _) = queue.launch_ad_hoc("Fix issue", "Interrupted").unwrap();
        let (live, _) = queue.launch_ad_hoc("Fix issue", "Still going").unwrap();

        assert_eq!(queue.requeue_interrupted_runs(|n| n == live), vec![gone]);
        assert_eq!(queue.busy_slots(), 1);
        assert_eq!(queue.waiting_count(), 1);
    }

    #[test]
    fn sample_api_gateway_matches_reference_mockup() {
        let queue = ProjectWorkQueue::for_project("api-gateway");

        assert_eq!(queue.tracker_name, "GitHub Issues");
        assert_eq!(queue.repo_slug, "acme/api-gateway");
        assert_eq!(queue.stage_count(WorkQueueStage::ReadyForAgents), 3);
        assert_eq!(queue.stage_count(WorkQueueStage::InProgress), 2);
        assert_eq!(queue.stage_count(WorkQueueStage::NeedsYou), 1);
        assert_eq!(queue.stage_count(WorkQueueStage::DoneThisWeek), 2);
        assert_eq!(queue.active_queue_count(), 5);
        assert_eq!(queue.unmatched_issues.len(), 9);
        assert_eq!(queue.agent_slots_summary(), "2 of 2 busy · 3 waiting");
        assert_eq!(queue.default_selected_issue(), Some(331));
        assert_eq!(queue.queue_position(331), Some((1, 3)));
        assert_eq!(queue.starts_label(), "When an agent slot frees");
        assert_eq!(
            queue.intake_summary_sentence(),
            "Open issues labeled agent-ready become work. Bugs go to Fix issue, docs to Docs refresh, everything else to Triage new issues."
        );
    }

    #[test]
    fn start_skip_reassign_and_review_transitions_update_counts() {
        let mut queue = ProjectWorkQueue::for_project("api-gateway");

        // Reassign workflow on #331.
        queue.reassign_workflow(331, "Triage new issues").unwrap();
        let item = queue.item(331).unwrap();
        assert_eq!(item.workflow_name, "Triage new issues");
        assert_eq!(item.matched_rule, "manual override → Triage new issues");

        // Start #331 -> moves to InProgress as run #45, #334 becomes Next in queue.
        let run_num = queue.start_issue(331).unwrap();
        assert_eq!(run_num, 45);
        assert_eq!(queue.stage_count(WorkQueueStage::ReadyForAgents), 2);
        assert_eq!(queue.stage_count(WorkQueueStage::InProgress), 3);
        assert_eq!(queue.item(334).unwrap().status_text, "Next in queue");
        assert_eq!(queue.queue_position(334), Some((1, 2)));

        // Skip #334 -> moves to unmatched issues.
        queue.skip_issue(334).unwrap();
        assert_eq!(queue.stage_count(WorkQueueStage::ReadyForAgents), 1);
        assert_eq!(queue.unmatched_issues.len(), 10);
        assert_eq!(queue.unmatched_issues[0].issue_number, 334);

        // Approve review gate on #318 -> moves to DoneThisWeek.
        queue.approve_review(318).unwrap();
        assert_eq!(queue.stage_count(WorkQueueStage::NeedsYou), 0);
        assert_eq!(queue.stage_count(WorkQueueStage::DoneThisWeek), 3);
        assert_eq!(
            queue.item(318).unwrap().status_text,
            "PR merged · issue closed"
        );
    }

    #[test]
    fn sync_unmatched_queue_and_ad_hoc_launch_work() {
        let mut queue = ProjectWorkQueue::for_project("api-gateway");

        let synced = queue.sync_issues().unwrap();
        assert_eq!(synced.issue_number, 340);
        assert_eq!(queue.stage_count(WorkQueueStage::ReadyForAgents), 4);

        queue.queue_unmatched_issue(338).unwrap();
        assert_eq!(queue.unmatched_issues.len(), 8);
        assert_eq!(queue.stage_count(WorkQueueStage::ReadyForAgents), 5);

        let (issue_no, run_no) = queue
            .launch_ad_hoc("Release prep", "Cut v2.4.0 release candidate")
            .unwrap();
        assert_eq!(issue_no, 350);
        assert_eq!(run_no, 45);
        assert_eq!(queue.stage_count(WorkQueueStage::InProgress), 3);
    }

    #[test]
    fn update_intake_rules_rematches_queued_issues() {
        let mut queue = ProjectWorkQueue::for_project("api-gateway");
        queue.update_intake_rules(
            vec![GitHubFilterClause::new(
                GitHubFilterQualifier::Label,
                "ready-for-bot",
            )],
            vec![IntakeRule::by_label("bug", "Triage new issues")],
            "Dependency upgrade".to_owned(),
        );

        assert_eq!(queue.intake_filters.len(), 1);
        assert_eq!(queue.intake_filters[0].value, "ready-for-bot");
        assert_eq!(queue.item(331).unwrap().workflow_name, "Triage new issues");
        assert_eq!(queue.item(329).unwrap().workflow_name, "Dependency upgrade");
    }

    #[test]
    fn applies_live_github_issues_and_preserves_in_flight_and_skipped_items() {
        let mut queue = ProjectWorkQueue::empty("my-app");
        queue.update_intake_rules(
            vec![GitHubFilterClause::new(
                GitHubFilterQualifier::Label,
                "agent-ready",
            )],
            vec![
                IntakeRule::by_label("bug", "Fix issue"),
                IntakeRule::by_label("docs", "Docs refresh"),
            ],
            "Triage new issues".to_owned(),
        );

        let issues = vec![
            GitHubIssue {
                number: 10,
                title: "Crash on empty payload".to_owned(),
                body: "Panics when body is empty.".to_owned(),
                labels: vec!["agent-ready".to_owned(), "bug".to_owned()],
                assignees: Vec::new(),
                author: Some("alice".to_owned()),
                milestone: None,
                comments_count: 0,
                created_at: None,
                url: "https://github.com/acme/my-app/issues/10".to_owned(),
                state: "OPEN".to_owned(),
                updated_at: None,
            },
            GitHubIssue {
                number: 11,
                title: "Clarify CLI flags".to_owned(),
                body: "".to_owned(),
                labels: vec!["agent-ready".to_owned()],
                assignees: Vec::new(),
                author: None,
                milestone: None,
                comments_count: 0,
                created_at: None,
                url: "https://github.com/acme/my-app/issues/11".to_owned(),
                state: "OPEN".to_owned(),
                updated_at: None,
            },
            GitHubIssue {
                number: 12,
                title: "RFC: plugin system".to_owned(),
                body: "Needs design.".to_owned(),
                labels: vec!["rfc".to_owned()],
                assignees: Vec::new(),
                author: None,
                milestone: None,
                comments_count: 0,
                created_at: None,
                url: "https://github.com/acme/my-app/issues/12".to_owned(),
                state: "OPEN".to_owned(),
                updated_at: None,
            },
        ];

        let diff = queue.apply_github_issues("acme/my-app", &issues);
        assert_eq!(diff.total_open, 3);
        assert_eq!(diff.newly_queued, vec![10, 11]);
        assert_eq!(diff.ready_count, 2);
        assert_eq!(diff.unmatched_count, 1);
        assert_eq!(queue.item(10).unwrap().workflow_name, "Fix issue");
        assert_eq!(queue.item(10).unwrap().label, "bug");
        assert_eq!(queue.item(11).unwrap().workflow_name, "Triage new issues");
        assert_eq!(queue.unmatched_issues[0].issue_number, 12);
        assert_eq!(
            queue.unmatched_issues[0].reason,
            "Missing label agent-ready"
        );

        // Start #10, skip #11, and re-sync: #10 stays InProgress and #11 stays skipped.
        queue.start_issue(10).unwrap();
        queue.skip_issue(11).unwrap();
        let diff2 = queue.apply_github_issues("acme/my-app", &issues);
        assert!(diff2.newly_queued.is_empty());
        assert_eq!(queue.item(10).unwrap().stage, WorkQueueStage::InProgress);
        assert!(queue.item(11).is_none());
        assert_eq!(queue.unmatched_issues.len(), 2);
    }

    #[test]
    fn routes_and_filters_issues_by_assignee_and_multi_clause_github_filters() {
        let mut queue = ProjectWorkQueue::empty("my-app");
        queue.update_intake_rules(
            vec![GitHubFilterClause::negated(
                GitHubFilterQualifier::Label,
                "blocked",
            )],
            vec![
                IntakeRule::with_clauses(
                    vec![
                        GitHubFilterClause::new(GitHubFilterQualifier::Assignee, "@factory-bot"),
                        GitHubFilterClause::new(GitHubFilterQualifier::Label, "bug"),
                    ],
                    "Urgent Bugfix",
                ),
                IntakeRule::by_assignee("@factory-bot", "Agent Triage"),
                IntakeRule::with_clauses(
                    vec![GitHubFilterClause::new(
                        GitHubFilterQualifier::Milestone,
                        "v2.0",
                    )],
                    "Release Prep",
                ),
            ],
            String::new(),
        );

        let issues = vec![
            GitHubIssue {
                number: 201,
                title: "Fix memory leak".to_owned(),
                body: String::new(),
                labels: vec!["bug".to_owned()],
                assignees: vec!["factory-bot".to_owned()],
                author: Some("alice".to_owned()),
                milestone: None,
                comments_count: 0,
                created_at: None,
                url: "https://github.com/acme/my-app/issues/201".to_owned(),
                state: "OPEN".to_owned(),
                updated_at: None,
            },
            GitHubIssue {
                number: 202,
                title: "Investigate flaky test".to_owned(),
                body: String::new(),
                labels: Vec::new(),
                assignees: vec!["factory-bot".to_owned()],
                author: None,
                milestone: None,
                comments_count: 0,
                created_at: None,
                url: "https://github.com/acme/my-app/issues/202".to_owned(),
                state: "OPEN".to_owned(),
                updated_at: None,
            },
            GitHubIssue {
                number: 203,
                title: "Cut v2.0 changelog".to_owned(),
                body: String::new(),
                labels: Vec::new(),
                assignees: Vec::new(),
                author: None,
                milestone: Some("v2.0".to_owned()),
                comments_count: 0,
                created_at: None,
                url: "https://github.com/acme/my-app/issues/203".to_owned(),
                state: "OPEN".to_owned(),
                updated_at: None,
            },
            GitHubIssue {
                number: 204,
                title: "Blocked migration".to_owned(),
                body: String::new(),
                labels: vec!["blocked".to_owned()],
                assignees: vec!["factory-bot".to_owned()],
                author: None,
                milestone: None,
                comments_count: 0,
                created_at: None,
                url: "https://github.com/acme/my-app/issues/204".to_owned(),
                state: "OPEN".to_owned(),
                updated_at: None,
            },
        ];

        let diff = queue.apply_github_issues("acme/my-app", &issues);
        assert_eq!(diff.ready_count, 3);
        assert_eq!(diff.unmatched_count, 1);
        assert_eq!(queue.item(201).unwrap().workflow_name, "Urgent Bugfix");
        assert_eq!(
            queue.item(201).unwrap().matched_rule,
            "assigned to @factory-bot and has label bug → Urgent Bugfix"
        );
        assert_eq!(queue.item(202).unwrap().workflow_name, "Agent Triage");
        assert_eq!(queue.item(202).unwrap().label, "@factory-bot");
        assert_eq!(queue.item(203).unwrap().workflow_name, "Release Prep");
        assert_eq!(queue.unmatched_issues[0].issue_number, 204);
        assert_eq!(
            queue.unmatched_issues[0].reason,
            "Has excluded label blocked"
        );
    }
}
