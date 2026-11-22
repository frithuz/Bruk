//! Per-project Workflows overview domain model, run-outcome strip, step summaries,
//! template constructors, and mocked fixtures matching `12 Workflows.png`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Segmented filter tabs shown above the workflows table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowsFilter {
    #[default]
    All,
    Active,
    PicksUpIssues,
    Drafts,
}

impl WorkflowsFilter {
    pub const ALL: &'static [Self] = &[Self::All, Self::Active, Self::PicksUpIssues, Self::Drafts];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Active => "Active",
            Self::PicksUpIssues => "Picks up issues",
            Self::Drafts => "Drafts",
        }
    }

    pub fn test_id(self) -> &'static str {
        match self {
            Self::All => "wfo-filter-all",
            Self::Active => "wfo-filter-active",
            Self::PicksUpIssues => "wfo-filter-issues",
            Self::Drafts => "wfo-filter-drafts",
        }
    }
}

/// Sort options in the `Sort by` dropdown above the workflows table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowsSort {
    #[default]
    LastRun,
    Name,
    Status,
    AverageCost,
}

impl WorkflowsSort {
    pub const ALL: &'static [Self] = &[Self::LastRun, Self::Name, Self::Status, Self::AverageCost];

    pub fn value(self) -> &'static str {
        match self {
            Self::LastRun => "last_run",
            Self::Name => "name",
            Self::Status => "status",
            Self::AverageCost => "average_cost",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::LastRun => "Last run",
            Self::Name => "Name",
            Self::Status => "Status",
            Self::AverageCost => "Average cost",
        }
    }

    pub fn from_value(value: &str) -> Self {
        match value {
            "name" => Self::Name,
            "status" => Self::Status,
            "average_cost" => Self::AverageCost,
            _ => Self::LastRun,
        }
    }
}

/// Outcome cell rendered in the 10-slot `Last 10 runs` strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcomeDot {
    Succeeded,
    FailedOrStopped,
    RunningOrWaiting,
    NoneYet,
}

/// Visual tone for the `Now` column in the workflows table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNowTone {
    Warning,
    Running,
    Muted,
}

/// Visual tone for an individual run row in the inspector's `Recent runs` list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecentRunStatus {
    Running,
    NeedsYou,
    Done,
    Failed,
}

impl RecentRunStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::NeedsYou => "Needs you",
            Self::Done => "Done",
            Self::Failed => "Failed",
        }
    }
}

/// Trigger icon kind for entries in the `Starts from` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTriggerKind {
    WorkQueue,
    AdHoc,
}

/// One trigger line shown in the `Starts from` column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowTriggerLine {
    pub kind: WorkflowTriggerKind,
    pub label: String,
}

/// One step item rendered in the right-hand inspector's `Steps` section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStepPreview {
    pub number: u32,
    pub name: String,
    pub actor: String,
}

/// Either a standalone step or a loop block containing child steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowStepGroup {
    Single(WorkflowStepPreview),
    Loop {
        title: String,
        condition_label: String,
        steps: Vec<WorkflowStepPreview>,
    },
}

/// One recent run card displayed at the bottom of the workflow inspector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRecentRun {
    pub run_number: u32,
    pub summary: String,
    pub status: RecentRunStatus,
}

/// Starter templates presented in the `Start from a template` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTemplateKind {
    Blank,
    FixAnIssue,
    ReviewPullRequest,
}

impl WorkflowTemplateKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Blank => "Blank workflow",
            Self::FixAnIssue => "Fix an issue",
            Self::ReviewPullRequest => "Review a pull request",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Blank => "Start with one step",
            Self::FixAnIssue => "Plan, fix in a loop, review, PR",
            Self::ReviewPullRequest => "Two models compare notes",
        }
    }

    pub fn test_id(self) -> &'static str {
        match self {
            Self::Blank => "wfo-template-blank",
            Self::FixAnIssue => "wfo-template-fix-issue",
            Self::ReviewPullRequest => "wfo-template-review-pr",
        }
    }
}

/// One workflow row and its full inspector specification on the Workflows overview page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowOverviewEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub is_draft: bool,
    pub triggers: Vec<WorkflowTriggerLine>,
    /// Exactly 10 cells (left = oldest, right = most recent).
    pub last_ten_runs: Vec<RunOutcomeDot>,
    pub last_run_caption: String,
    /// Lower means more recent (used by `Last run` sort); `u32::MAX` for never run.
    pub last_run_order: u32,
    pub now_label: String,
    pub now_tone: WorkflowNowTone,
    pub active_runs: u32,
    pub needs_you_runs: u32,
    pub picks_up_label: String,
    pub ad_hoc_allowed: bool,
    pub ad_hoc_label: String,
    pub median_run_label: String,
    pub average_cost_cents: Option<u32>,
    pub edited_label: String,
    pub step_groups: Vec<WorkflowStepGroup>,
    pub recent_runs: Vec<WorkflowRecentRun>,
}

impl WorkflowOverviewEntry {
    pub fn picks_up_issues(&self) -> bool {
        self.triggers
            .iter()
            .any(|t| t.kind == WorkflowTriggerKind::WorkQueue)
    }

    pub fn is_active_now(&self) -> bool {
        self.active_runs > 0 || self.needs_you_runs > 0
    }

    pub fn average_cost_display(&self) -> String {
        match self.average_cost_cents {
            Some(cents) => format!("${}.{:02}", cents / 100, cents % 100),
            None => "—".to_owned(),
        }
    }

    pub fn status_sort_rank(&self) -> u8 {
        if self.needs_you_runs > 0 {
            0
        } else if self.active_runs > 0 {
            1
        } else if !self.is_draft {
            2
        } else {
            3
        }
    }

    pub fn matches_filter(&self, filter: WorkflowsFilter) -> bool {
        match filter {
            WorkflowsFilter::All => true,
            WorkflowsFilter::Active => self.is_active_now(),
            WorkflowsFilter::PicksUpIssues => self.picks_up_issues(),
            WorkflowsFilter::Drafts => self.is_draft,
        }
    }

    pub fn matches_query(&self, query: &str) -> bool {
        let q = query.trim().to_ascii_lowercase();
        if q.is_empty() {
            return true;
        }
        self.name.to_ascii_lowercase().contains(&q)
            || self.description.to_ascii_lowercase().contains(&q)
            || self.now_label.to_ascii_lowercase().contains(&q)
            || self
                .triggers
                .iter()
                .any(|t| t.label.to_ascii_lowercase().contains(&q))
    }

    fn push_run_dot(&mut self, dot: RunOutcomeDot) {
        if self.last_ten_runs.len() >= 10 {
            self.last_ten_runs.remove(0);
        }
        self.last_ten_runs.push(dot);
        while self.last_ten_runs.len() < 10 {
            self.last_ten_runs.insert(0, RunOutcomeDot::NoneYet);
        }
    }

    fn refresh_now_label(&mut self) {
        if self.is_draft && self.active_runs == 0 && self.needs_you_runs == 0 {
            self.now_label = "Draft".to_owned();
            self.now_tone = WorkflowNowTone::Muted;
        } else if self.needs_you_runs > 0 {
            self.now_label = format!(
                "{} active · {} needs you",
                self.active_runs.max(self.needs_you_runs),
                self.needs_you_runs
            );
            self.now_tone = WorkflowNowTone::Warning;
        } else if self.active_runs > 0 {
            self.now_label = format!("{} running", self.active_runs);
            self.now_tone = WorkflowNowTone::Running;
        } else {
            self.now_label = "Idle".to_owned();
            self.now_tone = WorkflowNowTone::Muted;
        }
    }
}

/// Errors produced by workflow overview mutations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkflowsOverviewError {
    #[error("workflow name cannot be empty")]
    EmptyName,
    #[error("workflow '{0}' was not found")]
    NotFound(String),
    #[error("file path cannot be empty")]
    EmptyImportPath,
}

/// Complete workflows overview state for one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorkflowsOverview {
    pub project_name: String,
    pub workflows: Vec<WorkflowOverviewEntry>,
    /// Running count shown in the header subtitle (`api-gateway · 6 workflows · 2 running`).
    pub running_count: u32,
    pub next_run_number: u32,
    next_custom_seq: u32,
}

impl ProjectWorkflowsOverview {
    /// Clean empty workflows overview for a project with no workflows yet.
    pub fn empty(project_name: &str) -> Self {
        Self {
            project_name: project_name.to_owned(),
            workflows: Vec::new(),
            running_count: 0,
            next_run_number: 1,
            next_custom_seq: 1,
        }
    }

    /// Builds a mocked workflows overview tailored to `project_name`.
    ///
    /// `"api-gateway"` matches `12 Workflows.png` verbatim; other sample and
    /// user-registered projects receive tailored mocked workflow catalogs.
    pub fn for_project(project_name: &str) -> Self {
        match project_name {
            "api-gateway" => Self::sample_api_gateway(),
            "marketing-site" => Self::sample_marketing_site(),
            "mobile-app" => Self::sample_mobile_app(),
            other => Self::sample_generic(other),
        }
    }

    /// Reference fixture for `api-gateway` matching `12 Workflows.png`.
    pub fn sample_api_gateway() -> Self {
        use RunOutcomeDot::{
            FailedOrStopped as F, NoneYet as N, RunningOrWaiting as R, Succeeded as S,
        };

        Self {
            project_name: "api-gateway".to_owned(),
            running_count: 2,
            next_run_number: 45,
            next_custom_seq: 1,
            workflows: vec![
                WorkflowOverviewEntry {
                    id: "fix-issue".to_owned(),
                    name: "Fix issue".to_owned(),
                    description: "Plans, fixes until tests pass, then opens a PR".to_owned(),
                    is_draft: false,
                    triggers: vec![
                        WorkflowTriggerLine {
                            kind: WorkflowTriggerKind::WorkQueue,
                            label: "Work queue · bug".to_owned(),
                        },
                        WorkflowTriggerLine {
                            kind: WorkflowTriggerKind::AdHoc,
                            label: "Ad-hoc".to_owned(),
                        },
                    ],
                    last_ten_runs: vec![S, S, F, S, S, S, R, S, S, S],
                    last_run_caption: "Last run 3 min ago".to_owned(),
                    last_run_order: 1,
                    now_label: "2 active · 1 needs you".to_owned(),
                    now_tone: WorkflowNowTone::Warning,
                    active_runs: 2,
                    needs_you_runs: 1,
                    picks_up_label: "Issues labeled bug".to_owned(),
                    ad_hoc_allowed: true,
                    ad_hoc_label: "Allowed".to_owned(),
                    median_run_label: "14 min".to_owned(),
                    average_cost_cents: Some(112),
                    edited_label: "Today by you".to_owned(),
                    step_groups: vec![
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 1,
                            name: "Read issue & plan".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                        WorkflowStepGroup::Loop {
                            title: "Fix until tests pass · max 5".to_owned(),
                            condition_label: "until pass".to_owned(),
                            steps: vec![
                                WorkflowStepPreview {
                                    number: 2,
                                    name: "Implement fix".to_owned(),
                                    actor: "Codex".to_owned(),
                                },
                                WorkflowStepPreview {
                                    number: 3,
                                    name: "Run tests".to_owned(),
                                    actor: "Shell".to_owned(),
                                },
                            ],
                        },
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 4,
                            name: "Review gate".to_owned(),
                            actor: "You".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 5,
                            name: "Open pull request".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 6,
                            name: "Comment on issue".to_owned(),
                            actor: "Issue source".to_owned(),
                        }),
                    ],
                    recent_runs: vec![
                        WorkflowRecentRun {
                            run_number: 44,
                            summary: "Issue #325 · Move CI to Node 24".to_owned(),
                            status: RecentRunStatus::Running,
                        },
                        WorkflowRecentRun {
                            run_number: 41,
                            summary: "Issue #318 · Flaky session test".to_owned(),
                            status: RecentRunStatus::NeedsYou,
                        },
                        WorkflowRecentRun {
                            run_number: 38,
                            summary: "Issue #310 · 429 on burst".to_owned(),
                            status: RecentRunStatus::Done,
                        },
                        WorkflowRecentRun {
                            run_number: 36,
                            summary: "Issue #301 · Header casing".to_owned(),
                            status: RecentRunStatus::Failed,
                        },
                    ],
                },
                WorkflowOverviewEntry {
                    id: "triage-new-issues".to_owned(),
                    name: "Triage new issues".to_owned(),
                    description: "Labels, asks for repro steps, sizes the work".to_owned(),
                    is_draft: false,
                    triggers: vec![WorkflowTriggerLine {
                        kind: WorkflowTriggerKind::WorkQueue,
                        label: "Work queue · other".to_owned(),
                    }],
                    last_ten_runs: vec![S, S, S, S, S, S, S, S, S, R],
                    last_run_caption: "Last run 6 min ago".to_owned(),
                    last_run_order: 2,
                    now_label: "1 running".to_owned(),
                    now_tone: WorkflowNowTone::Running,
                    active_runs: 1,
                    needs_you_runs: 0,
                    picks_up_label: "All other labeled issues".to_owned(),
                    ad_hoc_allowed: false,
                    ad_hoc_label: "Queue only".to_owned(),
                    median_run_label: "4 min".to_owned(),
                    average_cost_cents: Some(28),
                    edited_label: "3 days ago by you".to_owned(),
                    step_groups: vec![
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 1,
                            name: "Classify issue & area".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 2,
                            name: "Check repro details".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 3,
                            name: "Apply labels & size".to_owned(),
                            actor: "Issue source".to_owned(),
                        }),
                    ],
                    recent_runs: vec![
                        WorkflowRecentRun {
                            run_number: 43,
                            summary: "Issue #322 · Expose rate-limit headers".to_owned(),
                            status: RecentRunStatus::Running,
                        },
                        WorkflowRecentRun {
                            run_number: 39,
                            summary: "Issue #314 · Add OIDC audience check".to_owned(),
                            status: RecentRunStatus::Done,
                        },
                        WorkflowRecentRun {
                            run_number: 35,
                            summary: "Issue #305 · Timeout on cold start".to_owned(),
                            status: RecentRunStatus::Done,
                        },
                    ],
                },
                WorkflowOverviewEntry {
                    id: "dependency-upgrade".to_owned(),
                    name: "Dependency upgrade".to_owned(),
                    description: "Upgrades one dependency and fixes fallout".to_owned(),
                    is_draft: false,
                    triggers: vec![
                        WorkflowTriggerLine {
                            kind: WorkflowTriggerKind::WorkQueue,
                            label: "Work queue · chore".to_owned(),
                        },
                        WorkflowTriggerLine {
                            kind: WorkflowTriggerKind::AdHoc,
                            label: "Ad-hoc".to_owned(),
                        },
                    ],
                    last_ten_runs: vec![N, N, F, S, S, S, S, F, S, R],
                    last_run_caption: "Last run 22 min ago".to_owned(),
                    last_run_order: 3,
                    now_label: "1 running".to_owned(),
                    now_tone: WorkflowNowTone::Running,
                    active_runs: 1,
                    needs_you_runs: 0,
                    picks_up_label: "Issues labeled chore".to_owned(),
                    ad_hoc_allowed: true,
                    ad_hoc_label: "Allowed".to_owned(),
                    median_run_label: "19 min".to_owned(),
                    average_cost_cents: Some(184),
                    edited_label: "Last week by you".to_owned(),
                    step_groups: vec![
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 1,
                            name: "Bump package & lockfile".to_owned(),
                            actor: "Shell".to_owned(),
                        }),
                        WorkflowStepGroup::Loop {
                            title: "Repair breaking changes · max 3".to_owned(),
                            condition_label: "until pass".to_owned(),
                            steps: vec![
                                WorkflowStepPreview {
                                    number: 2,
                                    name: "Update call sites".to_owned(),
                                    actor: "Codex".to_owned(),
                                },
                                WorkflowStepPreview {
                                    number: 3,
                                    name: "Run test suite".to_owned(),
                                    actor: "Shell".to_owned(),
                                },
                            ],
                        },
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 4,
                            name: "Open upgrade PR".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                    ],
                    recent_runs: vec![
                        WorkflowRecentRun {
                            run_number: 40,
                            summary: "Bump fastify 4.28 → 5.1".to_owned(),
                            status: RecentRunStatus::Running,
                        },
                        WorkflowRecentRun {
                            run_number: 34,
                            summary: "Upgrade redis client to 4.7".to_owned(),
                            status: RecentRunStatus::Done,
                        },
                        WorkflowRecentRun {
                            run_number: 31,
                            summary: "Migrate ESLint flat config".to_owned(),
                            status: RecentRunStatus::Failed,
                        },
                    ],
                },
                WorkflowOverviewEntry {
                    id: "docs-refresh".to_owned(),
                    name: "Docs refresh".to_owned(),
                    description: "Updates docs pages affected by recent changes".to_owned(),
                    is_draft: false,
                    triggers: vec![
                        WorkflowTriggerLine {
                            kind: WorkflowTriggerKind::WorkQueue,
                            label: "Work queue · docs".to_owned(),
                        },
                        WorkflowTriggerLine {
                            kind: WorkflowTriggerKind::AdHoc,
                            label: "Ad-hoc".to_owned(),
                        },
                    ],
                    last_ten_runs: vec![N, N, N, N, S, S, S, S, S, F],
                    last_run_caption: "Failed yesterday".to_owned(),
                    last_run_order: 4,
                    now_label: "Idle".to_owned(),
                    now_tone: WorkflowNowTone::Muted,
                    active_runs: 0,
                    needs_you_runs: 0,
                    picks_up_label: "Issues labeled docs".to_owned(),
                    ad_hoc_allowed: true,
                    ad_hoc_label: "Allowed".to_owned(),
                    median_run_label: "7 min".to_owned(),
                    average_cost_cents: Some(46),
                    edited_label: "Nov 10 by you".to_owned(),
                    step_groups: vec![
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 1,
                            name: "Diff recent API changes".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 2,
                            name: "Update OpenAPI & guides".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 3,
                            name: "Validate docs build".to_owned(),
                            actor: "Shell".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 4,
                            name: "Open pull request".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                    ],
                    recent_runs: vec![
                        WorkflowRecentRun {
                            run_number: 37,
                            summary: "Issue #315 · Document webhook retries".to_owned(),
                            status: RecentRunStatus::Failed,
                        },
                        WorkflowRecentRun {
                            run_number: 33,
                            summary: "Issue #308 · Rate-limit error schema".to_owned(),
                            status: RecentRunStatus::Done,
                        },
                    ],
                },
                WorkflowOverviewEntry {
                    id: "release-prep".to_owned(),
                    name: "Release prep".to_owned(),
                    description: "Changelog, version bump, release PR".to_owned(),
                    is_draft: false,
                    triggers: vec![WorkflowTriggerLine {
                        kind: WorkflowTriggerKind::AdHoc,
                        label: "Ad-hoc only".to_owned(),
                    }],
                    last_ten_runs: vec![N, N, N, N, N, N, N, S, S, R],
                    last_run_caption: "Last run 4 min ago".to_owned(),
                    last_run_order: 5,
                    now_label: "1 running".to_owned(),
                    now_tone: WorkflowNowTone::Running,
                    active_runs: 1,
                    needs_you_runs: 0,
                    picks_up_label: "None (manual only)".to_owned(),
                    ad_hoc_allowed: true,
                    ad_hoc_label: "Ad-hoc only".to_owned(),
                    median_run_label: "11 min".to_owned(),
                    average_cost_cents: Some(84),
                    edited_label: "2 days ago by you".to_owned(),
                    step_groups: vec![
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 1,
                            name: "Gather changes".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 2,
                            name: "Draft changelog".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                        WorkflowStepGroup::Loop {
                            title: "Fix until tests pass · max 5".to_owned(),
                            condition_label: "until pass".to_owned(),
                            steps: vec![
                                WorkflowStepPreview {
                                    number: 3,
                                    name: "Bump version & fix tests".to_owned(),
                                    actor: "Codex".to_owned(),
                                },
                                WorkflowStepPreview {
                                    number: 4,
                                    name: "Run release checks".to_owned(),
                                    actor: "Shell".to_owned(),
                                },
                            ],
                        },
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 5,
                            name: "Review gate".to_owned(),
                            actor: "You".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 6,
                            name: "Open pull request".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                    ],
                    recent_runs: vec![
                        WorkflowRecentRun {
                            run_number: 42,
                            summary: "Prepare release v2.8.0".to_owned(),
                            status: RecentRunStatus::Running,
                        },
                        WorkflowRecentRun {
                            run_number: 29,
                            summary: "Prepare release v2.7.3".to_owned(),
                            status: RecentRunStatus::Done,
                        },
                        WorkflowRecentRun {
                            run_number: 24,
                            summary: "Prepare release v2.7.2".to_owned(),
                            status: RecentRunStatus::Done,
                        },
                    ],
                },
                WorkflowOverviewEntry {
                    id: "security-audit".to_owned(),
                    name: "Security audit".to_owned(),
                    description: "Reviews auth code paths and reports findings".to_owned(),
                    is_draft: true,
                    triggers: vec![WorkflowTriggerLine {
                        kind: WorkflowTriggerKind::AdHoc,
                        label: "Ad-hoc".to_owned(),
                    }],
                    last_ten_runs: vec![N, N, N, N, N, N, N, N, N, N],
                    last_run_caption: "Never run".to_owned(),
                    last_run_order: u32::MAX,
                    now_label: "Draft".to_owned(),
                    now_tone: WorkflowNowTone::Muted,
                    active_runs: 0,
                    needs_you_runs: 0,
                    picks_up_label: "None (draft)".to_owned(),
                    ad_hoc_allowed: true,
                    ad_hoc_label: "Allowed".to_owned(),
                    median_run_label: "—".to_owned(),
                    average_cost_cents: None,
                    edited_label: "Yesterday by you".to_owned(),
                    step_groups: vec![
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 1,
                            name: "Map auth & token paths".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 2,
                            name: "Cross-check findings".to_owned(),
                            actor: "Codex".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 3,
                            name: "Summarize report".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                    ],
                    recent_runs: Vec::new(),
                },
            ],
        }
    }

    fn sample_marketing_site() -> Self {
        use RunOutcomeDot::{
            FailedOrStopped as F, NoneYet as N, RunningOrWaiting as R, Succeeded as S,
        };

        Self {
            project_name: "marketing-site".to_owned(),
            running_count: 1,
            next_run_number: 23,
            next_custom_seq: 1,
            workflows: vec![
                WorkflowOverviewEntry {
                    id: "fix-issue".to_owned(),
                    name: "Fix issue".to_owned(),
                    description: "Resolves layout, copy, and link regressions".to_owned(),
                    is_draft: false,
                    triggers: vec![
                        WorkflowTriggerLine {
                            kind: WorkflowTriggerKind::WorkQueue,
                            label: "Work queue · bug".to_owned(),
                        },
                        WorkflowTriggerLine {
                            kind: WorkflowTriggerKind::AdHoc,
                            label: "Ad-hoc".to_owned(),
                        },
                    ],
                    last_ten_runs: vec![N, N, S, S, S, F, S, S, S, R],
                    last_run_caption: "Last run 11 min ago".to_owned(),
                    last_run_order: 1,
                    now_label: "1 running".to_owned(),
                    now_tone: WorkflowNowTone::Running,
                    active_runs: 1,
                    needs_you_runs: 0,
                    picks_up_label: "Issues labeled bug".to_owned(),
                    ad_hoc_allowed: true,
                    ad_hoc_label: "Allowed".to_owned(),
                    median_run_label: "8 min".to_owned(),
                    average_cost_cents: Some(64),
                    edited_label: "Today by you".to_owned(),
                    step_groups: vec![
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 1,
                            name: "Inspect page & assets".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 2,
                            name: "Apply fix & build".to_owned(),
                            actor: "Codex".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 3,
                            name: "Open pull request".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                    ],
                    recent_runs: vec![
                        WorkflowRecentRun {
                            run_number: 22,
                            summary: "Issue #118 · Pricing tier toggle".to_owned(),
                            status: RecentRunStatus::Running,
                        },
                        WorkflowRecentRun {
                            run_number: 20,
                            summary: "Issue #112 · OG image metadata".to_owned(),
                            status: RecentRunStatus::Done,
                        },
                    ],
                },
                WorkflowOverviewEntry {
                    id: "content-update".to_owned(),
                    name: "Content update".to_owned(),
                    description: "Publishes release notes and changelog entries".to_owned(),
                    is_draft: false,
                    triggers: vec![
                        WorkflowTriggerLine {
                            kind: WorkflowTriggerKind::WorkQueue,
                            label: "Work queue · content".to_owned(),
                        },
                        WorkflowTriggerLine {
                            kind: WorkflowTriggerKind::AdHoc,
                            label: "Ad-hoc".to_owned(),
                        },
                    ],
                    last_ten_runs: vec![N, N, N, N, N, S, S, S, S, S],
                    last_run_caption: "Last run 2 hours ago".to_owned(),
                    last_run_order: 2,
                    now_label: "Idle".to_owned(),
                    now_tone: WorkflowNowTone::Muted,
                    active_runs: 0,
                    needs_you_runs: 0,
                    picks_up_label: "Issues labeled content".to_owned(),
                    ad_hoc_allowed: true,
                    ad_hoc_label: "Allowed".to_owned(),
                    median_run_label: "5 min".to_owned(),
                    average_cost_cents: Some(32),
                    edited_label: "4 days ago by you".to_owned(),
                    step_groups: vec![
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 1,
                            name: "Draft MDX updates".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 2,
                            name: "Run link checker".to_owned(),
                            actor: "Shell".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 3,
                            name: "Review gate".to_owned(),
                            actor: "You".to_owned(),
                        }),
                    ],
                    recent_runs: vec![WorkflowRecentRun {
                        run_number: 19,
                        summary: "Issue #109 · Launch blog banner".to_owned(),
                        status: RecentRunStatus::Done,
                    }],
                },
            ],
        }
    }

    fn sample_mobile_app() -> Self {
        use RunOutcomeDot::{NoneYet as N, RunningOrWaiting as R, Succeeded as S};

        Self {
            project_name: "mobile-app".to_owned(),
            running_count: 1,
            next_run_number: 19,
            next_custom_seq: 1,
            workflows: vec![
                WorkflowOverviewEntry {
                    id: "fix-issue".to_owned(),
                    name: "Fix issue".to_owned(),
                    description: "Fixes iOS and Android bugs with simulator verification"
                        .to_owned(),
                    is_draft: false,
                    triggers: vec![
                        WorkflowTriggerLine {
                            kind: WorkflowTriggerKind::WorkQueue,
                            label: "Work queue · bug".to_owned(),
                        },
                        WorkflowTriggerLine {
                            kind: WorkflowTriggerKind::AdHoc,
                            label: "Ad-hoc".to_owned(),
                        },
                    ],
                    last_ten_runs: vec![N, N, N, S, S, S, S, S, S, R],
                    last_run_caption: "Last run 8 min ago".to_owned(),
                    last_run_order: 1,
                    now_label: "1 running".to_owned(),
                    now_tone: WorkflowNowTone::Running,
                    active_runs: 1,
                    needs_you_runs: 0,
                    picks_up_label: "Issues labeled bug".to_owned(),
                    ad_hoc_allowed: true,
                    ad_hoc_label: "Allowed".to_owned(),
                    median_run_label: "16 min".to_owned(),
                    average_cost_cents: Some(135),
                    edited_label: "Today by you".to_owned(),
                    step_groups: vec![
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 1,
                            name: "Read crash log & plan".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 2,
                            name: "Implement fix & run tests".to_owned(),
                            actor: "Codex".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 3,
                            name: "Review gate".to_owned(),
                            actor: "You".to_owned(),
                        }),
                    ],
                    recent_runs: vec![WorkflowRecentRun {
                        run_number: 18,
                        summary: "Issue #84 · Push token refresh".to_owned(),
                        status: RecentRunStatus::Running,
                    }],
                },
                WorkflowOverviewEntry {
                    id: "release-prep".to_owned(),
                    name: "Release prep".to_owned(),
                    description: "Bumps build number, TestFlight notes, and tag".to_owned(),
                    is_draft: false,
                    triggers: vec![WorkflowTriggerLine {
                        kind: WorkflowTriggerKind::AdHoc,
                        label: "Ad-hoc only".to_owned(),
                    }],
                    last_ten_runs: vec![N, N, N, N, N, N, N, N, S, S],
                    last_run_caption: "Last run 3 days ago".to_owned(),
                    last_run_order: 2,
                    now_label: "Idle".to_owned(),
                    now_tone: WorkflowNowTone::Muted,
                    active_runs: 0,
                    needs_you_runs: 0,
                    picks_up_label: "None (manual only)".to_owned(),
                    ad_hoc_allowed: true,
                    ad_hoc_label: "Ad-hoc only".to_owned(),
                    median_run_label: "12 min".to_owned(),
                    average_cost_cents: Some(92),
                    edited_label: "Last week by you".to_owned(),
                    step_groups: vec![
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 1,
                            name: "Gather release notes".to_owned(),
                            actor: "Claude".to_owned(),
                        }),
                        WorkflowStepGroup::Single(WorkflowStepPreview {
                            number: 2,
                            name: "Bump build & verify".to_owned(),
                            actor: "Shell".to_owned(),
                        }),
                    ],
                    recent_runs: vec![WorkflowRecentRun {
                        run_number: 16,
                        summary: "Prepare TestFlight 1.14.0".to_owned(),
                        status: RecentRunStatus::Done,
                    }],
                },
            ],
        }
    }

    fn sample_generic(project_name: &str) -> Self {
        use RunOutcomeDot::NoneYet as N;

        Self {
            project_name: project_name.to_owned(),
            running_count: 0,
            next_run_number: 1,
            next_custom_seq: 1,
            workflows: vec![WorkflowOverviewEntry {
                id: "fix-issue".to_owned(),
                name: "Fix issue".to_owned(),
                description: "Plans, fixes until tests pass, then opens a PR".to_owned(),
                is_draft: false,
                triggers: vec![
                    WorkflowTriggerLine {
                        kind: WorkflowTriggerKind::WorkQueue,
                        label: "Work queue · bug".to_owned(),
                    },
                    WorkflowTriggerLine {
                        kind: WorkflowTriggerKind::AdHoc,
                        label: "Ad-hoc".to_owned(),
                    },
                ],
                last_ten_runs: vec![N; 10],
                last_run_caption: "Never run".to_owned(),
                last_run_order: u32::MAX,
                now_label: "Idle".to_owned(),
                now_tone: WorkflowNowTone::Muted,
                active_runs: 0,
                needs_you_runs: 0,
                picks_up_label: "Issues labeled bug".to_owned(),
                ad_hoc_allowed: true,
                ad_hoc_label: "Allowed".to_owned(),
                median_run_label: "—".to_owned(),
                average_cost_cents: None,
                edited_label: "Today by you".to_owned(),
                step_groups: vec![
                    WorkflowStepGroup::Single(WorkflowStepPreview {
                        number: 1,
                        name: "Read issue & plan".to_owned(),
                        actor: "Claude".to_owned(),
                    }),
                    WorkflowStepGroup::Single(WorkflowStepPreview {
                        number: 2,
                        name: "Implement & verify".to_owned(),
                        actor: "Codex".to_owned(),
                    }),
                    WorkflowStepGroup::Single(WorkflowStepPreview {
                        number: 3,
                        name: "Open pull request".to_owned(),
                        actor: "Claude".to_owned(),
                    }),
                ],
                recent_runs: Vec::new(),
            }],
        }
    }

    pub fn header_subtitle(&self) -> String {
        let count = self.workflows.len();
        if count == 0 {
            return format!("{} · No workflows yet", self.project_name);
        }
        let noun = if count == 1 { "workflow" } else { "workflows" };
        format!(
            "{} · {} {} · {} running",
            self.project_name, count, noun, self.running_count
        )
    }

    pub fn default_selected_id(&self) -> Option<String> {
        self.workflows.first().map(|w| w.id.clone())
    }

    pub fn workflow(&self, id: &str) -> Option<&WorkflowOverviewEntry> {
        self.workflows.iter().find(|w| w.id == id)
    }

    pub fn workflow_mut(&mut self, id: &str) -> Option<&mut WorkflowOverviewEntry> {
        self.workflows.iter_mut().find(|w| w.id == id)
    }

    pub fn filtered_and_sorted(
        &self,
        filter: WorkflowsFilter,
        sort: WorkflowsSort,
        query: &str,
    ) -> Vec<WorkflowOverviewEntry> {
        let mut items: Vec<WorkflowOverviewEntry> = self
            .workflows
            .iter()
            .filter(|w| w.matches_filter(filter) && w.matches_query(query))
            .cloned()
            .collect();

        match sort {
            WorkflowsSort::LastRun => {
                items.sort_by_key(|w| w.last_run_order);
            }
            WorkflowsSort::Name => {
                items.sort_by(|a, b| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                });
            }
            WorkflowsSort::Status => {
                items.sort_by_key(|w| (w.status_sort_rank(), w.last_run_order));
            }
            WorkflowsSort::AverageCost => {
                items.sort_by(|a, b| {
                    b.average_cost_cents
                        .unwrap_or(0)
                        .cmp(&a.average_cost_cents.unwrap_or(0))
                        .then_with(|| a.last_run_order.cmp(&b.last_run_order))
                });
            }
        }
        items
    }

    /// Creates a new custom workflow and returns its generated ID.
    pub fn create_workflow(
        &mut self,
        name: &str,
        description: &str,
        issue_label: Option<&str>,
        as_draft: bool,
    ) -> Result<String, WorkflowsOverviewError> {
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            return Err(WorkflowsOverviewError::EmptyName);
        }
        let trimmed_desc = if description.trim().is_empty() {
            "Custom workflow ready for configuration".to_owned()
        } else {
            description.trim().to_owned()
        };
        let id = self.unique_id(trimmed_name);

        let mut triggers = Vec::new();
        let picks_up_label = match issue_label.map(str::trim).filter(|s| !s.is_empty()) {
            Some(label) => {
                triggers.push(WorkflowTriggerLine {
                    kind: WorkflowTriggerKind::WorkQueue,
                    label: format!("Work queue · {label}"),
                });
                format!("Issues labeled {label}")
            }
            None => {
                if as_draft {
                    "None (draft)".to_owned()
                } else {
                    "None (manual only)".to_owned()
                }
            }
        };
        triggers.push(WorkflowTriggerLine {
            kind: WorkflowTriggerKind::AdHoc,
            label: if triggers.is_empty() {
                "Ad-hoc only".to_owned()
            } else {
                "Ad-hoc".to_owned()
            },
        });

        let mut entry = WorkflowOverviewEntry {
            id: id.clone(),
            name: trimmed_name.to_owned(),
            description: trimmed_desc,
            is_draft: as_draft,
            triggers,
            last_ten_runs: vec![RunOutcomeDot::NoneYet; 10],
            last_run_caption: "Never run".to_owned(),
            last_run_order: u32::MAX,
            now_label: if as_draft {
                "Draft".to_owned()
            } else {
                "Idle".to_owned()
            },
            now_tone: WorkflowNowTone::Muted,
            active_runs: 0,
            needs_you_runs: 0,
            picks_up_label,
            ad_hoc_allowed: true,
            ad_hoc_label: "Allowed".to_owned(),
            median_run_label: "—".to_owned(),
            average_cost_cents: None,
            edited_label: "Just now by you".to_owned(),
            step_groups: vec![WorkflowStepGroup::Single(WorkflowStepPreview {
                number: 1,
                name: "Analyze task & plan".to_owned(),
                actor: "Claude".to_owned(),
            })],
            recent_runs: Vec::new(),
        };
        entry.refresh_now_label();
        self.workflows.push(entry);
        Ok(id)
    }

    /// Creates a new draft workflow from one of the starter template cards.
    pub fn create_from_template(&mut self, template: WorkflowTemplateKind) -> String {
        let (base_name, desc, step_groups, triggers, picks_up_label) = match template {
            WorkflowTemplateKind::Blank => (
                "Untitled workflow",
                "Start with one step and grow from there",
                vec![WorkflowStepGroup::Single(WorkflowStepPreview {
                    number: 1,
                    name: "Run agent step".to_owned(),
                    actor: "Claude".to_owned(),
                })],
                vec![WorkflowTriggerLine {
                    kind: WorkflowTriggerKind::AdHoc,
                    label: "Ad-hoc".to_owned(),
                }],
                "None (draft)".to_owned(),
            ),
            WorkflowTemplateKind::FixAnIssue => (
                "Fix an issue",
                "Plan, fix in a loop, review, PR",
                vec![
                    WorkflowStepGroup::Single(WorkflowStepPreview {
                        number: 1,
                        name: "Read issue & plan".to_owned(),
                        actor: "Claude".to_owned(),
                    }),
                    WorkflowStepGroup::Loop {
                        title: "Fix until tests pass · max 5".to_owned(),
                        condition_label: "until pass".to_owned(),
                        steps: vec![
                            WorkflowStepPreview {
                                number: 2,
                                name: "Implement fix".to_owned(),
                                actor: "Codex".to_owned(),
                            },
                            WorkflowStepPreview {
                                number: 3,
                                name: "Run tests".to_owned(),
                                actor: "Shell".to_owned(),
                            },
                        ],
                    },
                    WorkflowStepGroup::Single(WorkflowStepPreview {
                        number: 4,
                        name: "Review gate".to_owned(),
                        actor: "You".to_owned(),
                    }),
                    WorkflowStepGroup::Single(WorkflowStepPreview {
                        number: 5,
                        name: "Open pull request".to_owned(),
                        actor: "Claude".to_owned(),
                    }),
                ],
                vec![
                    WorkflowTriggerLine {
                        kind: WorkflowTriggerKind::WorkQueue,
                        label: "Work queue · bug".to_owned(),
                    },
                    WorkflowTriggerLine {
                        kind: WorkflowTriggerKind::AdHoc,
                        label: "Ad-hoc".to_owned(),
                    },
                ],
                "Issues labeled bug".to_owned(),
            ),
            WorkflowTemplateKind::ReviewPullRequest => (
                "Review a pull request",
                "Two models compare notes on a pull request diff",
                vec![
                    WorkflowStepGroup::Single(WorkflowStepPreview {
                        number: 1,
                        name: "Primary diff review".to_owned(),
                        actor: "Claude".to_owned(),
                    }),
                    WorkflowStepGroup::Single(WorkflowStepPreview {
                        number: 2,
                        name: "Second-opinion audit".to_owned(),
                        actor: "Codex".to_owned(),
                    }),
                    WorkflowStepGroup::Single(WorkflowStepPreview {
                        number: 3,
                        name: "Synthesize & comment".to_owned(),
                        actor: "Claude".to_owned(),
                    }),
                ],
                vec![WorkflowTriggerLine {
                    kind: WorkflowTriggerKind::AdHoc,
                    label: "Ad-hoc".to_owned(),
                }],
                "None (draft)".to_owned(),
            ),
        };

        let name = self.unique_display_name(base_name);
        let id = self.unique_id(&name);
        self.workflows.push(WorkflowOverviewEntry {
            id: id.clone(),
            name,
            description: desc.to_owned(),
            is_draft: true,
            triggers,
            last_ten_runs: vec![RunOutcomeDot::NoneYet; 10],
            last_run_caption: "Never run".to_owned(),
            last_run_order: u32::MAX,
            now_label: "Draft".to_owned(),
            now_tone: WorkflowNowTone::Muted,
            active_runs: 0,
            needs_you_runs: 0,
            picks_up_label,
            ad_hoc_allowed: true,
            ad_hoc_label: "Allowed".to_owned(),
            median_run_label: "—".to_owned(),
            average_cost_cents: None,
            edited_label: "Just now by you".to_owned(),
            step_groups,
            recent_runs: Vec::new(),
        });
        id
    }

    /// Imports a mocked `.bruk-workflow` file and adds it to this project.
    pub fn import_from_file(&mut self, file_name: &str) -> Result<String, WorkflowsOverviewError> {
        let trimmed = file_name.trim();
        if trimmed.is_empty() {
            return Err(WorkflowsOverviewError::EmptyImportPath);
        }
        let stem = trimmed
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(trimmed)
            .trim_end_matches(".bruk-workflow")
            .trim_end_matches(".yaml")
            .trim_end_matches(".yml");
        let pretty = humanize_slug(if stem.is_empty() {
            "Imported workflow"
        } else {
            stem
        });
        let name = self.unique_display_name(&pretty);
        let id = self.unique_id(&name);

        self.workflows.push(WorkflowOverviewEntry {
            id: id.clone(),
            name,
            description: format!("Imported from {trimmed}"),
            is_draft: true,
            triggers: vec![WorkflowTriggerLine {
                kind: WorkflowTriggerKind::AdHoc,
                label: "Ad-hoc".to_owned(),
            }],
            last_ten_runs: vec![RunOutcomeDot::NoneYet; 10],
            last_run_caption: "Never run".to_owned(),
            last_run_order: u32::MAX,
            now_label: "Draft".to_owned(),
            now_tone: WorkflowNowTone::Muted,
            active_runs: 0,
            needs_you_runs: 0,
            picks_up_label: "None (imported draft)".to_owned(),
            ad_hoc_allowed: true,
            ad_hoc_label: "Allowed".to_owned(),
            median_run_label: "—".to_owned(),
            average_cost_cents: None,
            edited_label: "Imported just now".to_owned(),
            step_groups: vec![
                WorkflowStepGroup::Single(WorkflowStepPreview {
                    number: 1,
                    name: "Inspect workspace".to_owned(),
                    actor: "Claude".to_owned(),
                }),
                WorkflowStepGroup::Single(WorkflowStepPreview {
                    number: 2,
                    name: "Run verification suite".to_owned(),
                    actor: "Shell".to_owned(),
                }),
                WorkflowStepGroup::Single(WorkflowStepPreview {
                    number: 3,
                    name: "Review gate".to_owned(),
                    actor: "You".to_owned(),
                }),
            ],
            recent_runs: Vec::new(),
        });
        Ok(id)
    }

    /// Updates a workflow's editable metadata fields inline.
    pub fn update_workflow(
        &mut self,
        id: &str,
        name: &str,
        description: &str,
        is_draft: bool,
    ) -> Result<(), WorkflowsOverviewError> {
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            return Err(WorkflowsOverviewError::EmptyName);
        }
        let Some(entry) = self.workflows.iter_mut().find(|w| w.id == id) else {
            return Err(WorkflowsOverviewError::NotFound(id.to_owned()));
        };
        entry.name = trimmed_name.to_owned();
        if !description.trim().is_empty() {
            entry.description = description.trim().to_owned();
        }
        entry.is_draft = is_draft;
        entry.edited_label = "Just now by you".to_owned();
        entry.refresh_now_label();
        Ok(())
    }

    /// Starts an ad-hoc run for `id`, updating its recent runs, 10-run strip, and running count.
    pub fn launch_ad_hoc_run(
        &mut self,
        id: &str,
        summary: &str,
    ) -> Result<u32, WorkflowsOverviewError> {
        let run_number = self.next_run_number;
        let Some(entry) = self.workflows.iter_mut().find(|w| w.id == id) else {
            return Err(WorkflowsOverviewError::NotFound(id.to_owned()));
        };
        self.next_run_number += 1;
        self.running_count += 1;

        let run_summary = if summary.trim().is_empty() {
            format!("Ad-hoc · {}", entry.name)
        } else {
            summary.trim().to_owned()
        };
        entry.is_draft = false;
        entry.active_runs += 1;
        entry.last_run_caption = "Last run just now".to_owned();
        entry.last_run_order = 0;
        entry.push_run_dot(RunOutcomeDot::RunningOrWaiting);
        entry.recent_runs.insert(
            0,
            WorkflowRecentRun {
                run_number,
                summary: run_summary,
                status: RecentRunStatus::Running,
            },
        );
        entry.refresh_now_label();
        Ok(run_number)
    }

    /// Duplicates an existing workflow as a draft copy and returns the copy's ID.
    pub fn duplicate_workflow(&mut self, id: &str) -> Result<String, WorkflowsOverviewError> {
        let Some(source) = self.workflows.iter().find(|w| w.id == id).cloned() else {
            return Err(WorkflowsOverviewError::NotFound(id.to_owned()));
        };
        let copy_name = self.unique_display_name(&format!("{} copy", source.name));
        let copy_id = self.unique_id(&copy_name);
        let mut copy = source;
        copy.id = copy_id.clone();
        copy.name = copy_name;
        copy.is_draft = true;
        copy.last_ten_runs = vec![RunOutcomeDot::NoneYet; 10];
        copy.last_run_caption = "Never run".to_owned();
        copy.last_run_order = u32::MAX;
        copy.active_runs = 0;
        copy.needs_you_runs = 0;
        copy.edited_label = "Just now by you".to_owned();
        copy.recent_runs.clear();
        copy.refresh_now_label();
        self.workflows.push(copy);
        Ok(copy_id)
    }

    /// Toggles draft status for `id`.
    pub fn toggle_draft(&mut self, id: &str) -> Result<bool, WorkflowsOverviewError> {
        let Some(entry) = self.workflows.iter_mut().find(|w| w.id == id) else {
            return Err(WorkflowsOverviewError::NotFound(id.to_owned()));
        };
        entry.is_draft = !entry.is_draft;
        entry.edited_label = "Just now by you".to_owned();
        entry.refresh_now_label();
        Ok(entry.is_draft)
    }

    /// Deletes a workflow from the project overview.
    pub fn delete_workflow(&mut self, id: &str) -> Result<String, WorkflowsOverviewError> {
        let Some(pos) = self.workflows.iter().position(|w| w.id == id) else {
            return Err(WorkflowsOverviewError::NotFound(id.to_owned()));
        };
        let removed = self.workflows.remove(pos);
        if removed.active_runs > 0 {
            self.running_count = self.running_count.saturating_sub(1);
        }
        Ok(removed.name)
    }

    fn unique_display_name(&self, base: &str) -> String {
        if !self
            .workflows
            .iter()
            .any(|w| w.name.eq_ignore_ascii_case(base))
        {
            return base.to_owned();
        }
        let mut idx = 2u32;
        loop {
            let candidate = format!("{base} {idx}");
            if !self
                .workflows
                .iter()
                .any(|w| w.name.eq_ignore_ascii_case(&candidate))
            {
                return candidate;
            }
            idx += 1;
        }
    }

    fn unique_id(&mut self, name: &str) -> String {
        let slug: String = name
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_owned();
        let base = if slug.is_empty() {
            let seq = self.next_custom_seq;
            self.next_custom_seq += 1;
            format!("workflow-{seq}")
        } else {
            slug
        };
        if !self.workflows.iter().any(|w| w.id == base) {
            return base;
        }
        let mut n = 2u32;
        loop {
            let candidate = format!("{base}-{n}");
            if !self.workflows.iter().any(|w| w.id == candidate) {
                return candidate;
            }
            n += 1;
        }
    }
}

fn humanize_slug(raw: &str) -> String {
    let words: Vec<String> = raw
        .split(['-', '_', ' '])
        .filter(|part| !part.is_empty())
        .enumerate()
        .map(|(idx, word)| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) if idx == 0 => {
                    format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
                }
                Some(first) => format!("{}{}", first.to_ascii_lowercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect();
    if words.is_empty() {
        "Imported workflow".to_owned()
    } else {
        words.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_api_gateway_matches_reference_mockup() {
        let overview = ProjectWorkflowsOverview::sample_api_gateway();
        assert_eq!(
            overview.header_subtitle(),
            "api-gateway · 6 workflows · 2 running"
        );
        assert_eq!(overview.workflows.len(), 6);
        assert_eq!(overview.workflows[0].name, "Fix issue");
        assert_eq!(overview.workflows[0].now_label, "2 active · 1 needs you");
        assert_eq!(overview.workflows[0].last_ten_runs.len(), 10);
        assert_eq!(overview.workflows[0].average_cost_display(), "$1.12");
        assert_eq!(overview.workflows[0].recent_runs.len(), 4);
        assert_eq!(overview.workflows[5].name, "Security audit");
        assert!(overview.workflows[5].is_draft);
        assert_eq!(overview.workflows[5].average_cost_display(), "—");
    }

    #[test]
    fn filter_tabs_and_search_narrow_workflows_accurately() {
        let overview = ProjectWorkflowsOverview::sample_api_gateway();

        assert_eq!(
            overview
                .filtered_and_sorted(WorkflowsFilter::All, WorkflowsSort::LastRun, "")
                .len(),
            6
        );
        assert_eq!(
            overview
                .filtered_and_sorted(WorkflowsFilter::Active, WorkflowsSort::LastRun, "")
                .len(),
            4
        );
        assert_eq!(
            overview
                .filtered_and_sorted(WorkflowsFilter::PicksUpIssues, WorkflowsSort::LastRun, "")
                .len(),
            4
        );
        let drafts =
            overview.filtered_and_sorted(WorkflowsFilter::Drafts, WorkflowsSort::LastRun, "");
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].name, "Security audit");

        let searched =
            overview.filtered_and_sorted(WorkflowsFilter::All, WorkflowsSort::LastRun, "changelog");
        assert_eq!(searched.len(), 1);
        assert_eq!(searched[0].name, "Release prep");
    }

    #[test]
    fn sorting_orders_by_name_status_and_cost() {
        let overview = ProjectWorkflowsOverview::sample_api_gateway();

        let by_name = overview.filtered_and_sorted(WorkflowsFilter::All, WorkflowsSort::Name, "");
        assert_eq!(by_name[0].name, "Dependency upgrade");
        assert_eq!(by_name[5].name, "Triage new issues");

        let by_cost =
            overview.filtered_and_sorted(WorkflowsFilter::All, WorkflowsSort::AverageCost, "");
        assert_eq!(by_cost[0].name, "Dependency upgrade");
        assert_eq!(by_cost[1].name, "Fix issue");
        assert_eq!(by_cost[5].name, "Security audit");
    }

    #[test]
    fn template_import_adhoc_and_duplicate_mutations_work() {
        let mut overview = ProjectWorkflowsOverview::sample_api_gateway();

        let pr_id = overview.create_from_template(WorkflowTemplateKind::ReviewPullRequest);
        assert_eq!(overview.workflows.len(), 7);
        assert_eq!(
            overview.workflow(&pr_id).unwrap().name,
            "Review a pull request"
        );

        let imp_id = overview
            .import_from_file("canary-deploy.bruk-workflow")
            .unwrap();
        assert_eq!(overview.workflow(&imp_id).unwrap().name, "Canary deploy");

        let run_no = overview
            .launch_ad_hoc_run("fix-issue", "Verify retry jitter")
            .unwrap();
        assert_eq!(run_no, 45);
        assert_eq!(overview.running_count, 3);
        let fix = overview.workflow("fix-issue").unwrap();
        assert_eq!(fix.recent_runs[0].run_number, 45);
        assert_eq!(fix.last_ten_runs[9], RunOutcomeDot::RunningOrWaiting);

        let copy_id = overview.duplicate_workflow("docs-refresh").unwrap();
        assert_eq!(
            overview.workflow(&copy_id).unwrap().name,
            "Docs refresh copy"
        );
        assert!(overview.workflow(&copy_id).unwrap().is_draft);
    }
}
