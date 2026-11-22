//! Per-project work queue domain model, intake rules, and mocked issue backlogs.

use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    DoneThisWeek,
}

impl WorkQueueStage {
    pub const ALL: &'static [Self] = &[
        Self::ReadyForAgents,
        Self::InProgress,
        Self::NeedsYou,
        Self::DoneThisWeek,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ReadyForAgents => "Ready for agents",
            Self::InProgress => "In progress",
            Self::NeedsYou => "Needs you",
            Self::DoneThisWeek => "Done this week",
        }
    }
}

/// Maps an issue label to a target workflow during intake triage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeRule {
    pub label: String,
    pub workflow_name: String,
}

/// Sidebar summary of a workflow available inside a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorkflowSummary {
    pub name: String,
    pub ad_hoc_only: bool,
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

/// Complete work queue state for one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorkQueue {
    pub project_name: String,
    pub tracker_name: String,
    pub repo_slug: String,
    pub required_label: String,
    pub intake_rules: Vec<IntakeRule>,
    pub fallback_workflow: String,
    pub workflows: Vec<ProjectWorkflowSummary>,
    pub items: Vec<WorkQueueItem>,
    pub unmatched_issues: Vec<UnmatchedIssue>,
    pub max_agent_slots: u32,
    pub next_run_number: u32,
    pub next_ad_hoc_issue_number: u32,
    sync_backlog: Vec<WorkQueueItem>,
}

impl ProjectWorkQueue {
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
            required_label: "agent-ready".to_owned(),
            intake_rules: vec![
                IntakeRule {
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                },
                IntakeRule {
                    label: "docs".to_owned(),
                    workflow_name: "Docs refresh".to_owned(),
                },
            ],
            fallback_workflow: "Triage new issues".to_owned(),
            workflows: vec![
                ProjectWorkflowSummary {
                    name: "Fix issue".to_owned(),
                    ad_hoc_only: false,
                },
                ProjectWorkflowSummary {
                    name: "Triage new issues".to_owned(),
                    ad_hoc_only: false,
                },
                ProjectWorkflowSummary {
                    name: "Dependency upgrade".to_owned(),
                    ad_hoc_only: false,
                },
                ProjectWorkflowSummary {
                    name: "Docs refresh".to_owned(),
                    ad_hoc_only: false,
                },
                ProjectWorkflowSummary {
                    name: "Release prep".to_owned(),
                    ad_hoc_only: true,
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
            required_label: "agent-ready".to_owned(),
            intake_rules: vec![
                IntakeRule {
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                },
                IntakeRule {
                    label: "docs".to_owned(),
                    workflow_name: "Docs refresh".to_owned(),
                },
            ],
            fallback_workflow: "Triage new issues".to_owned(),
            workflows: vec![
                ProjectWorkflowSummary {
                    name: "Fix issue".to_owned(),
                    ad_hoc_only: false,
                },
                ProjectWorkflowSummary {
                    name: "Docs refresh".to_owned(),
                    ad_hoc_only: false,
                },
                ProjectWorkflowSummary {
                    name: "Triage new issues".to_owned(),
                    ad_hoc_only: false,
                },
                ProjectWorkflowSummary {
                    name: "Release prep".to_owned(),
                    ad_hoc_only: true,
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
        }
    }

    fn sample_mobile_app() -> Self {
        let slug = "acme/mobile-app";
        let url = |num: u32| format!("https://linear.app/acme/issue/MOB-{num}");

        Self {
            project_name: "mobile-app".to_owned(),
            tracker_name: "Linear".to_owned(),
            repo_slug: slug.to_owned(),
            required_label: "agent-ready".to_owned(),
            intake_rules: vec![
                IntakeRule {
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                },
                IntakeRule {
                    label: "docs".to_owned(),
                    workflow_name: "Docs refresh".to_owned(),
                },
            ],
            fallback_workflow: "Triage new issues".to_owned(),
            workflows: vec![
                ProjectWorkflowSummary {
                    name: "Fix issue".to_owned(),
                    ad_hoc_only: false,
                },
                ProjectWorkflowSummary {
                    name: "Triage new issues".to_owned(),
                    ad_hoc_only: false,
                },
                ProjectWorkflowSummary {
                    name: "Dependency upgrade".to_owned(),
                    ad_hoc_only: false,
                },
                ProjectWorkflowSummary {
                    name: "Release prep".to_owned(),
                    ad_hoc_only: true,
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
        }
    }

    fn sample_generic(project_name: &str) -> Self {
        let slug = format!("acme/{project_name}");
        let issue_url = format!("https://github.com/{slug}/issues/12");

        Self {
            project_name: project_name.to_owned(),
            tracker_name: "GitHub Issues".to_owned(),
            repo_slug: slug,
            required_label: "agent-ready".to_owned(),
            intake_rules: vec![
                IntakeRule {
                    label: "bug".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                },
                IntakeRule {
                    label: "docs".to_owned(),
                    workflow_name: "Docs refresh".to_owned(),
                },
            ],
            fallback_workflow: "Triage new issues".to_owned(),
            workflows: vec![
                ProjectWorkflowSummary {
                    name: "Fix issue".to_owned(),
                    ad_hoc_only: false,
                },
                ProjectWorkflowSummary {
                    name: "Triage new issues".to_owned(),
                    ad_hoc_only: false,
                },
                ProjectWorkflowSummary {
                    name: "Docs refresh".to_owned(),
                    ad_hoc_only: false,
                },
                ProjectWorkflowSummary {
                    name: "Release prep".to_owned(),
                    ad_hoc_only: true,
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
        if let Some(rule) = self
            .intake_rules
            .iter()
            .find(|rule| rule.label.eq_ignore_ascii_case(label))
        {
            (
                rule.workflow_name.clone(),
                format!("label {} → {}", rule.label, rule.workflow_name),
            )
        } else {
            (
                self.fallback_workflow.clone(),
                format!("fallback → {}", self.fallback_workflow),
            )
        }
    }

    /// Plain-text intake rule summary sentence for accessibility and non-rich contexts.
    pub fn intake_summary_sentence(&self) -> String {
        let mut parts = Vec::new();
        for (idx, rule) in self.intake_rules.iter().enumerate() {
            let subject = match rule.label.as_str() {
                "bug" if idx == 0 => "Bugs go to".to_owned(),
                "bug" => "bugs to".to_owned(),
                other if idx == 0 => format!("{} go to", capitalize_first(other)),
                other => format!("{other} to"),
            };
            parts.push(format!("{subject} {}", rule.workflow_name));
        }
        if parts.is_empty() {
            format!(
                "Open issues labeled {} become work. Everything goes to {}.",
                self.required_label, self.fallback_workflow
            )
        } else {
            format!(
                "Open issues labeled {} become work. {}, everything else to {}.",
                self.required_label,
                parts.join(", "),
                self.fallback_workflow
            )
        }
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
        self.unmatched_issues.insert(
            0,
            UnmatchedIssue {
                issue_number: removed.issue_number,
                title: removed.title,
                labels: vec![removed.label],
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
        let primary_label = unmatched
            .labels
            .first()
            .cloned()
            .unwrap_or_else(|| "chore".to_owned());
        let (workflow_name, matched_rule) = self.match_workflow_for_label(&primary_label);
        let issue_url = format!(
            "https://github.com/{}/issues/{}",
            self.repo_slug, unmatched.issue_number
        );

        self.items.push(WorkQueueItem {
            issue_number: unmatched.issue_number,
            title: unmatched.title,
            description: format!(
                "Manually queued from unmatched tracker issues ({}).",
                unmatched.reason
            ),
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

    /// Updates intake configuration and re-matches non-overridden queued items.
    pub fn update_intake_rules(
        &mut self,
        required_label: String,
        rules: Vec<IntakeRule>,
        fallback_workflow: String,
    ) {
        let trimmed_req = required_label.trim();
        if !trimmed_req.is_empty() {
            let old_reason = format!("Missing label {}", self.required_label);
            let new_reason = format!("Missing label {trimmed_req}");
            self.required_label = trimmed_req.to_owned();
            for issue in &mut self.unmatched_issues {
                if issue.reason == old_reason {
                    issue.reason = new_reason.clone();
                }
            }
        }
        let trimmed_fallback = fallback_workflow.trim();
        if !trimmed_fallback.is_empty() {
            self.fallback_workflow = trimmed_fallback.to_owned();
        }
        self.intake_rules = rules
            .into_iter()
            .filter(|r| !r.label.trim().is_empty() && !r.workflow_name.trim().is_empty())
            .collect();

        let rules_snapshot = self.intake_rules.clone();
        let fallback_snapshot = self.fallback_workflow.clone();
        for item in &mut self.items {
            if item.stage == WorkQueueStage::ReadyForAgents
                && !item.matched_rule.starts_with("manual override")
            {
                if let Some(rule) = rules_snapshot
                    .iter()
                    .find(|rule| rule.label.eq_ignore_ascii_case(&item.label))
                {
                    item.workflow_name = rule.workflow_name.clone();
                    item.matched_rule = format!("label {} → {}", rule.label, rule.workflow_name);
                } else {
                    item.workflow_name = fallback_snapshot.clone();
                    item.matched_rule = format!("fallback → {fallback_snapshot}");
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
            "ready-for-bot".to_owned(),
            vec![IntakeRule {
                label: "bug".to_owned(),
                workflow_name: "Triage new issues".to_owned(),
            }],
            "Dependency upgrade".to_owned(),
        );

        assert_eq!(queue.required_label, "ready-for-bot");
        assert_eq!(queue.item(331).unwrap().workflow_name, "Triage new issues");
        assert_eq!(queue.item(329).unwrap().workflow_name, "Dependency upgrade");
    }
}
