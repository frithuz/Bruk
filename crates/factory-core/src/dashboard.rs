//! Dashboard analytics, telemetry snapshots, inbox items, and machine status models.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{Project, RunStatus, WorkflowRun};

/// Accent color assigned to a project across the sidebar, charts, and tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectColor {
    Blue,
    Orange,
    Green,
    Purple,
    Teal,
    Rose,
}

impl ProjectColor {
    /// Deterministic palette color for a project name.
    pub fn for_project_name(name: &str) -> Self {
        match name {
            "api-gateway" => Self::Blue,
            "marketing-site" => Self::Orange,
            "mobile-app" => Self::Green,
            other => {
                let hash = other.bytes().fold(0usize, |acc, b| {
                    acc.wrapping_mul(31).wrapping_add(b as usize)
                });
                match hash % 6 {
                    0 => Self::Blue,
                    1 => Self::Orange,
                    2 => Self::Green,
                    3 => Self::Purple,
                    4 => Self::Teal,
                    _ => Self::Rose,
                }
            }
        }
    }
}

/// Time bucket granularity for the dashboard overview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DashboardGranularity {
    #[default]
    Week,
    Month,
}

impl DashboardGranularity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Week => "Week",
            Self::Month => "Month",
        }
    }
}

/// Query parameters controlling what the dashboard displays.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DashboardQuery {
    pub granularity: DashboardGranularity,
    /// Relative period offset where `0` is the default primary window (`Last week` / `November 2026`).
    pub period_offset: i32,
    /// Optional project name filter (`None` means `All projects`).
    pub project_filter: Option<String>,
}

/// One project's token contribution in a time bucket or summary breakdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTokens {
    pub project_name: String,
    pub color: ProjectColor,
    pub tokens: u64,
    pub share_pct: u8,
}

/// Stacked bar column for one day (Week view) or one week (Month view).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenBucket {
    pub label: String,
    pub segments: Vec<ProjectTokens>,
    pub total_tokens: u64,
}

/// Aggregated token usage for an LLM provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub provider_name: String,
    pub tokens: u64,
}

/// Per-project activity row shown in "Projects worked on".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectActivityRow {
    pub project_name: String,
    pub source: String,
    pub color: ProjectColor,
    pub issues_resolved: u32,
    pub prs_merged: u32,
    pub runs: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub est_cost_cents: u64,
}

impl ProjectActivityRow {
    pub fn success_rate_pct(&self) -> u8 {
        if self.runs == 0 {
            0
        } else {
            ((self.succeeded as f64 / self.runs as f64) * 100.0).round() as u8
        }
    }
}

/// One entry in the "Highest token runs" card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighTokenRun {
    pub rank: u8,
    pub title: String,
    pub project_name: String,
    pub subtitle: String,
    pub tokens: u64,
    pub est_cost_cents: u64,
    pub duration_minutes: u32,
    pub outcome_note: String,
}

/// Summary of human-in-the-loop response latency and wait impact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitMetrics {
    pub median_answer_minutes: u32,
    pub longest_wait_minutes: u32,
    pub total_waiting_tenths_hours: u32,
    pub requests_answered: u32,
    pub permissions: u32,
    pub questions: u32,
    pub reviews: u32,
    pub timed_out: u32,
}

impl WaitMetrics {
    pub fn summary_sentence(&self) -> String {
        if self.requests_answered == 0 {
            return "No agent requests required operator input in this period.".to_owned();
        }
        let timeout_clause = match self.timed_out {
            0 => String::new(),
            1 => " 1 question timed out and the agent decided.".to_owned(),
            n => format!(" {n} questions timed out and the agent decided."),
        };
        format!(
            "{} requests answered: {} permissions, {} questions, {} reviews.{}",
            self.requests_answered, self.permissions, self.questions, self.reviews, timeout_clause
        )
    }
}

/// Actionable callout shown in the "Worth a look" card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardInsight {
    pub headline: String,
    pub detail: String,
    pub project_name: Option<String>,
}

/// Category of an operator request waiting in the Inbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboxRequestKind {
    Permission,
    Question,
    Review,
}

impl InboxRequestKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Permission => "Permission",
            Self::Question => "Question",
            Self::Review => "Review",
        }
    }
}

/// An actionable agent request surfaced in the Inbox view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxItem {
    pub id: u64,
    pub kind: InboxRequestKind,
    pub project_name: String,
    pub run_label: String,
    pub title: String,
    pub detail: String,
    pub waiting_minutes: u32,
    pub resolved: bool,
}

/// Connection status for a worker machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineStatus {
    Connected,
    Disconnected,
}

impl MachineStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "Connected",
            Self::Disconnected => "Disconnected",
        }
    }
}

/// A local or remote compute host executing factory agent runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineNode {
    pub id: u64,
    pub name: String,
    pub spec: String,
    pub active_runs: u32,
    pub max_slots: u32,
    pub status: MachineStatus,
    pub last_seen: String,
}

/// Complete, view-ready snapshot of the Dashboard for a given query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardSnapshot {
    pub period_label: String,
    pub date_range_label: String,
    pub filter_label: String,
    pub can_step_prev: bool,
    pub can_step_next: bool,
    pub issues_resolved: u32,
    pub issues_delta_text: String,
    pub prs_merged: u32,
    pub prs_delta_text: String,
    pub total_runs: u32,
    pub succeeded_runs: u32,
    pub failed_runs: u32,
    pub total_tokens: u64,
    pub tokens_delta_text: String,
    pub est_cost_cents: u64,
    pub billed_cost_cents: u64,
    pub working_time_tenths_hours: u32,
    pub chart_max_tokens: u64,
    pub buckets: Vec<TokenBucket>,
    pub project_tokens: Vec<ProjectTokens>,
    pub providers: Vec<ProviderUsage>,
    pub provider_footnote: String,
    pub project_rows: Vec<ProjectActivityRow>,
    pub highest_token_runs: Vec<HighTokenRun>,
    pub wait_metrics: WaitMetrics,
    pub insights: Vec<DashboardInsight>,
}

impl DashboardSnapshot {
    /// True when the workspace has no recorded projects, runs, or token usage yet.
    pub fn is_empty(&self) -> bool {
        self.project_rows.is_empty() && !self.has_activity()
    }

    /// True when any runs, tokens, issues, or PRs have been recorded in this snapshot.
    pub fn has_activity(&self) -> bool {
        self.total_runs > 0
            || self.total_tokens > 0
            || self.issues_resolved > 0
            || self.prs_merged > 0
    }
}

/// Internal per-period dataset record.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PeriodData {
    offset: i32,
    period_label: String,
    date_range_label: String,
    issues_delta_prev: i32,
    prs_delta_prev: i32,
    tokens_delta_pct: i32,
    billed_cost_cents: u64,
    working_time_tenths_hours: u32,
    chart_max_tokens: u64,
    buckets: Vec<(String, Vec<(String, u64)>)>,
    providers: Vec<ProviderUsage>,
    project_rows: Vec<ProjectActivityRow>,
    highest_token_runs: Vec<HighTokenRun>,
    wait_metrics: WaitMetrics,
    insights: Vec<DashboardInsight>,
}

/// Multi-period telemetry store powering the Dashboard landing screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardDataset {
    weekly: Vec<PeriodData>,
    monthly: Vec<PeriodData>,
}

impl Default for DashboardDataset {
    fn default() -> Self {
        Self::empty()
    }
}

impl DashboardDataset {
    /// Creates an empty dataset with no mocked telemetry or historical sample periods.
    pub fn empty() -> Self {
        let empty_wait = WaitMetrics {
            median_answer_minutes: 0,
            longest_wait_minutes: 0,
            total_waiting_tenths_hours: 0,
            requests_answered: 0,
            permissions: 0,
            questions: 0,
            reviews: 0,
            timed_out: 0,
        };
        Self {
            weekly: vec![PeriodData {
                offset: 0,
                period_label: "This week".to_owned(),
                date_range_label: "No activity yet".to_owned(),
                issues_delta_prev: 0,
                prs_delta_prev: 0,
                tokens_delta_pct: 0,
                billed_cost_cents: 0,
                working_time_tenths_hours: 0,
                chart_max_tokens: 0,
                buckets: Vec::new(),
                providers: Vec::new(),
                project_rows: Vec::new(),
                highest_token_runs: Vec::new(),
                wait_metrics: empty_wait.clone(),
                insights: Vec::new(),
            }],
            monthly: vec![PeriodData {
                offset: 0,
                period_label: "This month".to_owned(),
                date_range_label: "No activity yet".to_owned(),
                issues_delta_prev: 0,
                prs_delta_prev: 0,
                tokens_delta_pct: 0,
                billed_cost_cents: 0,
                working_time_tenths_hours: 0,
                chart_max_tokens: 0,
                buckets: Vec::new(),
                providers: Vec::new(),
                project_rows: Vec::new(),
                highest_token_runs: Vec::new(),
                wait_metrics: empty_wait,
                insights: Vec::new(),
            }],
        }
    }

    /// Aggregates real persisted workflow runs into live Dashboard telemetry.
    pub fn from_persisted_runs(runs: &[crate::storage::PersistedWorkflowRun]) -> Self {
        if runs.is_empty() {
            return Self::empty();
        }

        let mut total_cost_cents = 0u64;
        let mut total_duration_ms = 0u64;
        let mut by_provider: BTreeMap<String, u64> = BTreeMap::new();
        // (succeeded, failed, total, tokens, cost)
        let mut by_project: BTreeMap<String, (u32, u32, u32, u64, u64)> = BTreeMap::new();

        for run in runs {
            total_cost_cents = total_cost_cents.saturating_add(run.total_cost_cents);
            total_duration_ms = total_duration_ms.saturating_add(run.duration_ms);
            let prov_key = if run.provider.trim().is_empty() {
                "Claude".to_owned()
            } else {
                run.provider.clone()
            };
            *by_provider.entry(prov_key).or_insert(0) += run.total_tokens;

            let proj = by_project
                .entry(run.project_name.clone())
                .or_insert((0, 0, 0, 0, 0));
            match run.status {
                crate::workflow_run::RunExecutionState::Succeeded => proj.0 += 1,
                crate::workflow_run::RunExecutionState::Failed
                | crate::workflow_run::RunExecutionState::Stopped => proj.1 += 1,
                _ => {}
            }
            proj.2 += 1;
            proj.3 = proj.3.saturating_add(run.total_tokens);
            proj.4 = proj.4.saturating_add(run.total_cost_cents);
        }

        let project_rows: Vec<ProjectActivityRow> = by_project
            .iter()
            .map(
                |(name, (succeeded, failed, total, _tokens, cost))| ProjectActivityRow {
                    project_name: name.clone(),
                    source: "Local runner".to_owned(),
                    color: ProjectColor::for_project_name(name),
                    issues_resolved: *succeeded,
                    prs_merged: 0,
                    runs: *total,
                    succeeded: *succeeded,
                    failed: *failed,
                    est_cost_cents: *cost,
                },
            )
            .collect();

        let providers: Vec<ProviderUsage> = by_provider
            .into_iter()
            .map(|(name, tokens)| ProviderUsage {
                provider_name: name,
                tokens,
            })
            .collect();

        let mut sorted_runs: Vec<&crate::storage::PersistedWorkflowRun> = runs.iter().collect();
        sorted_runs.sort_by_key(|a| std::cmp::Reverse(a.total_tokens));
        let highest_token_runs: Vec<HighTokenRun> = sorted_runs
            .into_iter()
            .take(5)
            .enumerate()
            .map(|(idx, run)| HighTokenRun {
                rank: (idx + 1) as u8,
                title: format!("{} #{}", run.workflow_name, run.run_number),
                project_name: run.project_name.clone(),
                subtitle: run.provider.clone(),
                tokens: run.total_tokens,
                est_cost_cents: run.total_cost_cents,
                duration_minutes: ((run.duration_ms as f64 / 60_000.0).ceil() as u32).max(1),
                outcome_note: run.summary.clone(),
            })
            .collect();

        let bucket_entries: Vec<(String, u64)> = by_project
            .iter()
            .map(|(name, (_, _, _, tokens, _))| (name.clone(), *tokens))
            .collect();
        let total_tokens: u64 = bucket_entries.iter().map(|(_, t)| *t).sum();
        let chart_max = total_tokens.max(1000);
        let working_tenths = ((total_duration_ms as f64 / 360_000.0).round() as u32).max(1);

        let empty_wait = WaitMetrics {
            median_answer_minutes: 0,
            longest_wait_minutes: 0,
            total_waiting_tenths_hours: 0,
            requests_answered: 0,
            permissions: 0,
            questions: 0,
            reviews: 0,
            timed_out: 0,
        };
        let period = PeriodData {
            offset: 0,
            period_label: "This week".to_owned(),
            date_range_label: format!(
                "{} recorded run{}",
                runs.len(),
                if runs.len() == 1 { "" } else { "s" }
            ),
            issues_delta_prev: 0,
            prs_delta_prev: 0,
            tokens_delta_pct: 0,
            billed_cost_cents: total_cost_cents,
            working_time_tenths_hours: working_tenths,
            chart_max_tokens: chart_max,
            buckets: vec![("Recent".to_owned(), bucket_entries)],
            providers,
            project_rows,
            highest_token_runs,
            wait_metrics: empty_wait,
            insights: Vec::new(),
        };
        let mut monthly = period.clone();
        monthly.period_label = "This month".to_owned();
        Self {
            weekly: vec![period],
            monthly: vec![monthly],
        }
    }

    /// Creates a rich multi-period dataset matching the reference dashboard layout.
    pub fn sample() -> Self {
        Self {
            weekly: vec![
                Self::week_minus_two(),
                Self::week_minus_one(),
                Self::last_week_primary(),
                Self::this_week_in_progress(),
            ],
            monthly: vec![Self::month_october(), Self::month_november()],
        }
    }

    /// Sample starter projects shown when the user has not yet registered repositories in SQLite.
    pub fn sample_projects() -> Vec<Project> {
        vec![
            Project {
                id: 1,
                name: "api-gateway".to_owned(),
                root_path: "~/src/api-gateway".into(),
                default_branch: "main".to_owned(),
                description: "Edge routing, auth middleware, and rate limiting service (GitHub)"
                    .to_owned(),
            },
            Project {
                id: 2,
                name: "marketing-site".to_owned(),
                root_path: "~/src/marketing-site".into(),
                default_branch: "main".to_owned(),
                description: "Public site, pricing pages, and product documentation (itsaplan)"
                    .to_owned(),
            },
            Project {
                id: 3,
                name: "mobile-app".to_owned(),
                root_path: "~/src/mobile-app".into(),
                default_branch: "main".to_owned(),
                description: "React Native iOS and Android client (GitHub)".to_owned(),
            },
        ]
    }

    /// Sample pending Inbox requests matching the `4` badge in the sidebar.
    pub fn sample_inbox() -> Vec<InboxItem> {
        vec![
            InboxItem {
                id: 1,
                kind: InboxRequestKind::Review,
                project_name: "marketing-site".to_owned(),
                run_label: "Docs refresh · run #10".to_owned(),
                title: "Approve updated pricing tier comparison table".to_owned(),
                detail: "Agent refreshed 14 docs pages and paused at the human review gate before opening PR #88.".to_owned(),
                waiting_minutes: 18,
                resolved: false,
            },
            InboxItem {
                id: 2,
                kind: InboxRequestKind::Permission,
                project_name: "api-gateway".to_owned(),
                run_label: "Fix issue · run #37".to_owned(),
                title: "Run integration test suite against staging Redis".to_owned(),
                detail: "Requests permission to execute `cargo test --test rate_limit_cluster` with staging credentials.".to_owned(),
                waiting_minutes: 9,
                resolved: false,
            },
            InboxItem {
                id: 3,
                kind: InboxRequestKind::Question,
                project_name: "mobile-app".to_owned(),
                run_label: "Dependency upgrade · run #7".to_owned(),
                title: "Keep legacy Reanimated 2 worklet shim for Android 11?".to_owned(),
                detail: "Dropping the shim removes 420 LOC and resolves the Metro bundler warning. Auto-decides in 25 min.".to_owned(),
                waiting_minutes: 34,
                resolved: false,
            },
            InboxItem {
                id: 4,
                kind: InboxRequestKind::Review,
                project_name: "api-gateway".to_owned(),
                run_label: "Issue #329 · run #38".to_owned(),
                title: "Review header normalization fallback behavior".to_owned(),
                detail: "Preserves mixed-case custom X-Forwarded-* headers while canonicalizing RFC 9110 standard headers.".to_owned(),
                waiting_minutes: 5,
                resolved: false,
            },
        ]
    }

    /// Sample worker machines matching the `1 disconnected` sidebar status.
    pub fn sample_machines() -> Vec<MachineNode> {
        vec![
            MachineNode {
                id: 1,
                name: "mac-studio-local".to_owned(),
                spec: "Apple M4 Max · 64 GB RAM · macOS 15".to_owned(),
                active_runs: 2,
                max_slots: 3,
                status: MachineStatus::Connected,
                last_seen: "Active now".to_owned(),
            },
            MachineNode {
                id: 2,
                name: "builder-us-east-01".to_owned(),
                spec: "16 vCPU · 32 GB RAM · Ubuntu 24.04".to_owned(),
                active_runs: 1,
                max_slots: 4,
                status: MachineStatus::Connected,
                last_seen: "Heartbeat 4s ago".to_owned(),
            },
            MachineNode {
                id: 3,
                name: "runner-arm64-02".to_owned(),
                spec: "8 vCPU · 16 GB RAM · Ubuntu 24.04".to_owned(),
                active_runs: 0,
                max_slots: 2,
                status: MachineStatus::Disconnected,
                last_seen: "Disconnected 42 min ago".to_owned(),
            },
        ]
    }

    /// Lists all distinct project names available for filtering on the dashboard.
    pub fn available_project_names(&self, extra_projects: &[Project]) -> Vec<String> {
        let mut names = Vec::new();
        for period in &self.weekly {
            for row in &period.project_rows {
                if !names.contains(&row.project_name) {
                    names.push(row.project_name.clone());
                }
            }
        }
        for project in extra_projects {
            if !names.contains(&project.name) {
                names.push(project.name.clone());
            }
        }
        names
    }

    /// Computes a view-ready [`DashboardSnapshot`] for `query`, merging live registered projects and runs.
    pub fn snapshot(
        &self,
        query: &DashboardQuery,
        tracked_projects: &[Project],
        live_runs: &[WorkflowRun],
    ) -> DashboardSnapshot {
        let periods = match query.granularity {
            DashboardGranularity::Week => &self.weekly,
            DashboardGranularity::Month => &self.monthly,
        };

        let index = periods
            .iter()
            .position(|p| p.offset == query.period_offset)
            .unwrap_or_else(|| periods.iter().position(|p| p.offset == 0).unwrap_or(0));
        let period = &periods[index];
        let can_step_prev = index > 0;
        let can_step_next = index + 1 < periods.len();

        let filter = query.project_filter.as_deref();
        let filter_label = filter.unwrap_or("All projects").to_owned();

        // Build project rows, merging any live runs and extra tracked projects for the primary period.
        let mut rows: Vec<ProjectActivityRow> = period.project_rows.clone();
        if period.offset == 0 {
            for project in tracked_projects {
                if !rows.iter().any(|r| r.project_name == project.name) {
                    rows.push(ProjectActivityRow {
                        project_name: project.name.clone(),
                        source: "Local".to_owned(),
                        color: ProjectColor::for_project_name(&project.name),
                        issues_resolved: 0,
                        prs_merged: 0,
                        runs: 0,
                        succeeded: 0,
                        failed: 0,
                        est_cost_cents: 0,
                    });
                }
            }
            for run in live_runs {
                if let Some(row) = rows.iter_mut().find(|r| r.project_name == run.project_name) {
                    row.runs += 1;
                    match run.status {
                        RunStatus::Succeeded => row.succeeded += 1,
                        RunStatus::Failed => row.failed += 1,
                        _ => {}
                    }
                }
            }
        }

        if let Some(target) = filter {
            rows.retain(|r| r.project_name == target);
        }

        // Compute per-bucket stacked bar data.
        let mut totals_by_project: BTreeMap<String, u64> = BTreeMap::new();
        let mut project_order: Vec<String> = Vec::new();
        for row in &rows {
            project_order.push(row.project_name.clone());
            totals_by_project
                .entry(row.project_name.clone())
                .or_insert(0);
        }

        let buckets: Vec<TokenBucket> = period
            .buckets
            .iter()
            .map(|(label, segs)| {
                let filtered_segs: Vec<ProjectTokens> = segs
                    .iter()
                    .filter(|(name, _)| filter.is_none_or(|f| f == name))
                    .map(|(name, tokens)| {
                        *totals_by_project.entry(name.clone()).or_insert(0) += *tokens;
                        if !project_order.contains(name) {
                            project_order.push(name.clone());
                        }
                        ProjectTokens {
                            project_name: name.clone(),
                            color: ProjectColor::for_project_name(name),
                            tokens: *tokens,
                            share_pct: 0,
                        }
                    })
                    .collect();
                let total_tokens = filtered_segs.iter().map(|s| s.tokens).sum();
                TokenBucket {
                    label: label.clone(),
                    segments: filtered_segs,
                    total_tokens,
                }
            })
            .collect();

        let total_tokens: u64 = buckets.iter().map(|b| b.total_tokens).sum();
        let period_all_tokens: u64 = period
            .buckets
            .iter()
            .flat_map(|(_, segs)| segs.iter().map(|(_, t)| *t))
            .sum();

        let project_tokens: Vec<ProjectTokens> = project_order
            .iter()
            .filter_map(|name| {
                let tokens = totals_by_project.get(name).copied().unwrap_or(0);
                if filter.is_none() && tokens == 0 {
                    return None;
                }
                let share_pct = if total_tokens == 0 {
                    0
                } else {
                    ((tokens as f64 / total_tokens as f64) * 100.0).round() as u8
                };
                Some(ProjectTokens {
                    project_name: name.clone(),
                    color: ProjectColor::for_project_name(name),
                    tokens,
                    share_pct,
                })
            })
            .collect();

        let share_ratio = if period_all_tokens == 0 {
            0.0
        } else {
            total_tokens as f64 / period_all_tokens as f64
        };

        let providers: Vec<ProviderUsage> = period
            .providers
            .iter()
            .map(|p| ProviderUsage {
                provider_name: p.provider_name.clone(),
                tokens: if filter.is_some() {
                    ((p.tokens as f64) * share_ratio).round() as u64
                } else {
                    p.tokens
                },
            })
            .collect();

        let issues_resolved: u32 = rows.iter().map(|r| r.issues_resolved).sum();
        let prs_merged: u32 = rows.iter().map(|r| r.prs_merged).sum();
        let total_runs: u32 = rows.iter().map(|r| r.runs).sum();
        let succeeded_runs: u32 = rows.iter().map(|r| r.succeeded).sum();
        let failed_runs: u32 = rows.iter().map(|r| r.failed).sum();
        let est_cost_cents: u64 = rows.iter().map(|r| r.est_cost_cents).sum();
        let billed_cost_cents = if filter.is_some() {
            ((period.billed_cost_cents as f64) * share_ratio).round() as u64
        } else {
            period.billed_cost_cents
        };
        let working_time_tenths_hours = if filter.is_some() {
            ((period.working_time_tenths_hours as f64) * share_ratio).round() as u32
        } else {
            period.working_time_tenths_hours
        };

        let period_unit = match query.granularity {
            DashboardGranularity::Week => "week",
            DashboardGranularity::Month => "month",
        };

        let has_baseline_data =
            !period.buckets.is_empty() || !period.project_rows.is_empty() || period_all_tokens > 0;

        let issues_delta_text = if filter.is_some() {
            format!("In {filter_label}")
        } else if !has_baseline_data && issues_resolved == 0 {
            "No activity yet".to_owned()
        } else {
            format_count_delta(period.issues_delta_prev, period_unit)
        };
        let prs_delta_text = if filter.is_some() {
            format!("In {filter_label}")
        } else if !has_baseline_data && prs_merged == 0 {
            "No activity yet".to_owned()
        } else {
            format_count_delta(period.prs_delta_prev, period_unit)
        };
        let tokens_delta_text = if filter.is_some() {
            format!(
                "{}% of workspace total",
                (share_ratio * 100.0).round() as u32
            )
        } else if !has_baseline_data && total_tokens == 0 {
            "No token usage yet".to_owned()
        } else {
            format_pct_delta(period.tokens_delta_pct, period_unit)
        };

        let provider_footnote = if providers.is_empty() && billed_cost_cents == 0 {
            "No provider usage recorded yet.".to_owned()
        } else {
            format!(
                "Claude and Codex use your subscriptions. OpenRouter billed {} this {}.",
                format_currency(billed_cost_cents),
                period_unit
            )
        };

        let highest_token_runs: Vec<HighTokenRun> = period
            .highest_token_runs
            .iter()
            .filter(|run| filter.is_none_or(|f| f == run.project_name))
            .enumerate()
            .map(|(idx, run)| {
                let mut cloned = run.clone();
                cloned.rank = (idx + 1) as u8;
                cloned
            })
            .collect();

        let wait_metrics = if filter.is_some() {
            let scaled_answered =
                ((period.wait_metrics.requests_answered as f64) * share_ratio).round() as u32;
            let scaled_perms =
                ((period.wait_metrics.permissions as f64) * share_ratio).round() as u32;
            let scaled_questions =
                ((period.wait_metrics.questions as f64) * share_ratio).round() as u32;
            let scaled_reviews = scaled_answered
                .saturating_sub(scaled_perms)
                .saturating_sub(scaled_questions);
            WaitMetrics {
                median_answer_minutes: period.wait_metrics.median_answer_minutes,
                longest_wait_minutes: ((period.wait_metrics.longest_wait_minutes as f64)
                    * share_ratio.max(0.35))
                .round() as u32,
                total_waiting_tenths_hours: ((period.wait_metrics.total_waiting_tenths_hours
                    as f64)
                    * share_ratio)
                    .round() as u32,
                requests_answered: scaled_answered,
                permissions: scaled_perms,
                questions: scaled_questions,
                reviews: scaled_reviews,
                timed_out: ((period.wait_metrics.timed_out as f64) * share_ratio).round() as u32,
            }
        } else {
            period.wait_metrics.clone()
        };

        let insights: Vec<DashboardInsight> = period
            .insights
            .iter()
            .filter(|insight| filter.is_none_or(|f| insight.project_name.as_deref() == Some(f)))
            .cloned()
            .collect();

        let peak_bucket = buckets.iter().map(|b| b.total_tokens).max().unwrap_or(0);
        let chart_max_tokens = if filter.is_some() {
            nice_chart_ceiling(peak_bucket)
        } else {
            period.chart_max_tokens.max(nice_chart_ceiling(peak_bucket))
        };

        DashboardSnapshot {
            period_label: period.period_label.clone(),
            date_range_label: period.date_range_label.clone(),
            filter_label,
            can_step_prev,
            can_step_next,
            issues_resolved,
            issues_delta_text,
            prs_merged,
            prs_delta_text,
            total_runs,
            succeeded_runs,
            failed_runs,
            total_tokens,
            tokens_delta_text,
            est_cost_cents,
            billed_cost_cents,
            working_time_tenths_hours,
            chart_max_tokens,
            buckets,
            project_tokens,
            providers,
            provider_footnote,
            project_rows: rows,
            highest_token_runs,
            wait_metrics,
            insights,
        }
    }

    fn last_week_primary() -> PeriodData {
        // Matches 01 Dashboard.png exact figures:
        // Total tokens = 18.4M (api-gateway: 11.4M / 62%, marketing-site: 3.6M / 20%, mobile-app: 3.4M / 18%)
        PeriodData {
            offset: 0,
            period_label: "Last week".to_owned(),
            date_range_label: "Mon Nov 9 – Sun Nov 15, 2026".to_owned(),
            issues_delta_prev: 4,
            prs_delta_prev: 2,
            tokens_delta_pct: 12,
            billed_cost_cents: 890,
            working_time_tenths_hours: 315,
            chart_max_tokens: 5_000_000,
            buckets: vec![
                (
                    "Mon".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 1_900_000),
                        ("marketing-site".to_owned(), 550_000),
                        ("mobile-app".to_owned(), 400_000),
                    ],
                ),
                (
                    "Tue".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 2_400_000),
                        ("marketing-site".to_owned(), 650_000),
                        ("mobile-app".to_owned(), 500_000),
                    ],
                ),
                (
                    "Wed".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 2_100_000),
                        ("marketing-site".to_owned(), 950_000),
                        ("mobile-app".to_owned(), 600_000),
                    ],
                ),
                (
                    "Thu".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 2_600_000),
                        ("marketing-site".to_owned(), 700_000),
                        ("mobile-app".to_owned(), 1_150_000),
                    ],
                ),
                (
                    "Fri".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 1_850_000),
                        ("marketing-site".to_owned(), 700_000),
                        ("mobile-app".to_owned(), 700_000),
                    ],
                ),
                (
                    "Sat".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 400_000),
                        ("marketing-site".to_owned(), 50_000),
                        ("mobile-app".to_owned(), 50_000),
                    ],
                ),
                (
                    "Sun".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 150_000),
                        ("marketing-site".to_owned(), 0),
                        ("mobile-app".to_owned(), 0),
                    ],
                ),
            ],
            providers: vec![
                ProviderUsage {
                    provider_name: "Claude".to_owned(),
                    tokens: 9_100_000,
                },
                ProviderUsage {
                    provider_name: "Codex".to_owned(),
                    tokens: 6_800_000,
                },
                ProviderUsage {
                    provider_name: "OpenRouter".to_owned(),
                    tokens: 2_500_000,
                },
            ],
            project_rows: vec![
                ProjectActivityRow {
                    project_name: "api-gateway".to_owned(),
                    source: "GitHub".to_owned(),
                    color: ProjectColor::Blue,
                    issues_resolved: 9,
                    prs_merged: 7,
                    runs: 31,
                    succeeded: 28,
                    failed: 3,
                    est_cost_cents: 3910,
                },
                ProjectActivityRow {
                    project_name: "marketing-site".to_owned(),
                    source: "itsaplan".to_owned(),
                    color: ProjectColor::Orange,
                    issues_resolved: 3,
                    prs_merged: 3,
                    runs: 12,
                    succeeded: 11,
                    failed: 1,
                    est_cost_cents: 1240,
                },
                ProjectActivityRow {
                    project_name: "mobile-app".to_owned(),
                    source: "GitHub".to_owned(),
                    color: ProjectColor::Green,
                    issues_resolved: 2,
                    prs_merged: 1,
                    runs: 9,
                    succeeded: 7,
                    failed: 2,
                    est_cost_cents: 1170,
                },
            ],
            highest_token_runs: vec![
                HighTokenRun {
                    rank: 1,
                    title: "Dependency upgrade · run #6".to_owned(),
                    project_name: "mobile-app".to_owned(),
                    subtitle: "Upgrade react-native-reanimated".to_owned(),
                    tokens: 1_380_000,
                    est_cost_cents: 465,
                    duration_minutes: 52,
                    outcome_note: "5 loop attempts".to_owned(),
                },
                HighTokenRun {
                    rank: 2,
                    title: "Fix issue · run #36".to_owned(),
                    project_name: "api-gateway".to_owned(),
                    subtitle: "Issue #301 · Header casing".to_owned(),
                    tokens: 1_050_000,
                    est_cost_cents: 340,
                    duration_minutes: 41,
                    outcome_note: "Failed after 5 attempts".to_owned(),
                },
                HighTokenRun {
                    rank: 3,
                    title: "Docs refresh · run #10".to_owned(),
                    project_name: "marketing-site".to_owned(),
                    subtitle: "Pricing pages refresh".to_owned(),
                    tokens: 860_000,
                    est_cost_cents: 210,
                    duration_minutes: 24,
                    outcome_note: "Read 140 pages for context".to_owned(),
                },
            ],
            wait_metrics: WaitMetrics {
                median_answer_minutes: 6,
                longest_wait_minutes: 130,
                total_waiting_tenths_hours: 34,
                requests_answered: 38,
                permissions: 21,
                questions: 9,
                reviews: 8,
                timed_out: 3,
            },
            insights: vec![
                DashboardInsight {
                    headline: "Docs refresh failed 2 of its last 3 runs".to_owned(),
                    detail: "Both stopped at the review gate with requested changes.".to_owned(),
                    project_name: Some("marketing-site".to_owned()),
                },
                DashboardInsight {
                    headline: "Dependency upgrade ran out of attempts twice".to_owned(),
                    detail: "Its loop limit is 3; both runs needed 5 or more.".to_owned(),
                    project_name: Some("mobile-app".to_owned()),
                },
                DashboardInsight {
                    headline: "#329 has been queued for 3 days".to_owned(),
                    detail: "api-gateway agent slots were full most of the week. Consider allowing 3 runs at once.".to_owned(),
                    project_name: Some("api-gateway".to_owned()),
                },
            ],
        }
    }

    fn week_minus_one() -> PeriodData {
        PeriodData {
            offset: -1,
            period_label: "2 weeks ago".to_owned(),
            date_range_label: "Mon Nov 2 – Sun Nov 8, 2026".to_owned(),
            issues_delta_prev: 1,
            prs_delta_prev: 1,
            tokens_delta_pct: 6,
            billed_cost_cents: 740,
            working_time_tenths_hours: 274,
            chart_max_tokens: 5_000_000,
            buckets: vec![
                (
                    "Mon".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 1_650_000),
                        ("marketing-site".to_owned(), 450_000),
                        ("mobile-app".to_owned(), 350_000),
                    ],
                ),
                (
                    "Tue".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 2_050_000),
                        ("marketing-site".to_owned(), 550_000),
                        ("mobile-app".to_owned(), 450_000),
                    ],
                ),
                (
                    "Wed".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 1_900_000),
                        ("marketing-site".to_owned(), 700_000),
                        ("mobile-app".to_owned(), 550_000),
                    ],
                ),
                (
                    "Thu".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 2_200_000),
                        ("marketing-site".to_owned(), 600_000),
                        ("mobile-app".to_owned(), 900_000),
                    ],
                ),
                (
                    "Fri".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 1_750_000),
                        ("marketing-site".to_owned(), 550_000),
                        ("mobile-app".to_owned(), 600_000),
                    ],
                ),
                (
                    "Sat".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 600_000),
                        ("marketing-site".to_owned(), 150_000),
                        ("mobile-app".to_owned(), 150_000),
                    ],
                ),
                (
                    "Sun".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 250_000),
                        ("marketing-site".to_owned(), 0),
                        ("mobile-app".to_owned(), 0),
                    ],
                ),
            ],
            providers: vec![
                ProviderUsage {
                    provider_name: "Claude".to_owned(),
                    tokens: 8_200_000,
                },
                ProviderUsage {
                    provider_name: "Codex".to_owned(),
                    tokens: 6_100_000,
                },
                ProviderUsage {
                    provider_name: "OpenRouter".to_owned(),
                    tokens: 2_100_000,
                },
            ],
            project_rows: vec![
                ProjectActivityRow {
                    project_name: "api-gateway".to_owned(),
                    source: "GitHub".to_owned(),
                    color: ProjectColor::Blue,
                    issues_resolved: 6,
                    prs_merged: 5,
                    runs: 26,
                    succeeded: 24,
                    failed: 2,
                    est_cost_cents: 3320,
                },
                ProjectActivityRow {
                    project_name: "marketing-site".to_owned(),
                    source: "itsaplan".to_owned(),
                    color: ProjectColor::Orange,
                    issues_resolved: 2,
                    prs_merged: 2,
                    runs: 10,
                    succeeded: 9,
                    failed: 1,
                    est_cost_cents: 1080,
                },
                ProjectActivityRow {
                    project_name: "mobile-app".to_owned(),
                    source: "GitHub".to_owned(),
                    color: ProjectColor::Green,
                    issues_resolved: 2,
                    prs_merged: 2,
                    runs: 8,
                    succeeded: 7,
                    failed: 1,
                    est_cost_cents: 1090,
                },
            ],
            highest_token_runs: vec![
                HighTokenRun {
                    rank: 1,
                    title: "Schema migration · run #24".to_owned(),
                    project_name: "api-gateway".to_owned(),
                    subtitle: "Migrate rate-limit buckets to v2".to_owned(),
                    tokens: 1_180_000,
                    est_cost_cents: 385,
                    duration_minutes: 46,
                    outcome_note: "Succeeded on 3rd attempt".to_owned(),
                },
                HighTokenRun {
                    rank: 2,
                    title: "Navigation refactor · run #4".to_owned(),
                    project_name: "mobile-app".to_owned(),
                    subtitle: "Typed deep-link routes".to_owned(),
                    tokens: 920_000,
                    est_cost_cents: 295,
                    duration_minutes: 38,
                    outcome_note: "Succeeded · 1 review round".to_owned(),
                },
                HighTokenRun {
                    rank: 3,
                    title: "Changelog automation · run #8".to_owned(),
                    project_name: "marketing-site".to_owned(),
                    subtitle: "Weekly release notes pipeline".to_owned(),
                    tokens: 740_000,
                    est_cost_cents: 185,
                    duration_minutes: 21,
                    outcome_note: "Succeeded on 1st attempt".to_owned(),
                },
            ],
            wait_metrics: WaitMetrics {
                median_answer_minutes: 8,
                longest_wait_minutes: 105,
                total_waiting_tenths_hours: 29,
                requests_answered: 31,
                permissions: 17,
                questions: 8,
                reviews: 6,
                timed_out: 2,
            },
            insights: vec![
                DashboardInsight {
                    headline: "api-gateway test suite slowed by 35%".to_owned(),
                    detail: "Docker fixture startup added ~4 min per run on builder-us-east-01."
                        .to_owned(),
                    project_name: Some("api-gateway".to_owned()),
                },
                DashboardInsight {
                    headline: "mobile-app iOS simulator cache hit 94%".to_owned(),
                    detail: "Saved ~18 min across 8 runs after enabling derived-data caching."
                        .to_owned(),
                    project_name: Some("mobile-app".to_owned()),
                },
            ],
        }
    }

    fn week_minus_two() -> PeriodData {
        PeriodData {
            offset: -2,
            period_label: "3 weeks ago".to_owned(),
            date_range_label: "Mon Oct 26 – Sun Nov 1, 2026".to_owned(),
            issues_delta_prev: -1,
            prs_delta_prev: 0,
            tokens_delta_pct: -4,
            billed_cost_cents: 680,
            working_time_tenths_hours: 252,
            chart_max_tokens: 5_000_000,
            buckets: vec![
                (
                    "Mon".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 1_450_000),
                        ("marketing-site".to_owned(), 400_000),
                        ("mobile-app".to_owned(), 350_000),
                    ],
                ),
                (
                    "Tue".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 1_900_000),
                        ("marketing-site".to_owned(), 500_000),
                        ("mobile-app".to_owned(), 400_000),
                    ],
                ),
                (
                    "Wed".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 1_800_000),
                        ("marketing-site".to_owned(), 650_000),
                        ("mobile-app".to_owned(), 500_000),
                    ],
                ),
                (
                    "Thu".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 2_000_000),
                        ("marketing-site".to_owned(), 550_000),
                        ("mobile-app".to_owned(), 750_000),
                    ],
                ),
                (
                    "Fri".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 1_600_000),
                        ("marketing-site".to_owned(), 500_000),
                        ("mobile-app".to_owned(), 550_000),
                    ],
                ),
                (
                    "Sat".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 450_000),
                        ("marketing-site".to_owned(), 100_000),
                        ("mobile-app".to_owned(), 150_000),
                    ],
                ),
                (
                    "Sun".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 200_000),
                        ("marketing-site".to_owned(), 0),
                        ("mobile-app".to_owned(), 0),
                    ],
                ),
            ],
            providers: vec![
                ProviderUsage {
                    provider_name: "Claude".to_owned(),
                    tokens: 7_600_000,
                },
                ProviderUsage {
                    provider_name: "Codex".to_owned(),
                    tokens: 5_400_000,
                },
                ProviderUsage {
                    provider_name: "OpenRouter".to_owned(),
                    tokens: 1_800_000,
                },
            ],
            project_rows: vec![
                ProjectActivityRow {
                    project_name: "api-gateway".to_owned(),
                    source: "GitHub".to_owned(),
                    color: ProjectColor::Blue,
                    issues_resolved: 5,
                    prs_merged: 5,
                    runs: 22,
                    succeeded: 20,
                    failed: 2,
                    est_cost_cents: 2980,
                },
                ProjectActivityRow {
                    project_name: "marketing-site".to_owned(),
                    source: "itsaplan".to_owned(),
                    color: ProjectColor::Orange,
                    issues_resolved: 2,
                    prs_merged: 2,
                    runs: 9,
                    succeeded: 8,
                    failed: 1,
                    est_cost_cents: 980,
                },
                ProjectActivityRow {
                    project_name: "mobile-app".to_owned(),
                    source: "GitHub".to_owned(),
                    color: ProjectColor::Green,
                    issues_resolved: 2,
                    prs_merged: 1,
                    runs: 7,
                    succeeded: 6,
                    failed: 1,
                    est_cost_cents: 940,
                },
            ],
            highest_token_runs: vec![HighTokenRun {
                rank: 1,
                title: "OpenTelemetry tracing · run #19".to_owned(),
                project_name: "api-gateway".to_owned(),
                subtitle: "Propagate traceparent across gRPC proxies".to_owned(),
                tokens: 1_040_000,
                est_cost_cents: 325,
                duration_minutes: 39,
                outcome_note: "Succeeded on 2nd attempt".to_owned(),
            }],
            wait_metrics: WaitMetrics {
                median_answer_minutes: 7,
                longest_wait_minutes: 95,
                total_waiting_tenths_hours: 25,
                requests_answered: 27,
                permissions: 15,
                questions: 7,
                reviews: 5,
                timed_out: 1,
            },
            insights: vec![DashboardInsight {
                headline: "All scheduled maintenance runs passed".to_owned(),
                detail: "Zero retries needed across nightly dependency and lint sweeps.".to_owned(),
                project_name: None,
            }],
        }
    }

    fn this_week_in_progress() -> PeriodData {
        PeriodData {
            offset: 1,
            period_label: "This week".to_owned(),
            date_range_label: "Mon Nov 16 – Sun Nov 22, 2026".to_owned(),
            issues_delta_prev: -5,
            prs_delta_prev: -4,
            tokens_delta_pct: -28,
            billed_cost_cents: 590,
            working_time_tenths_hours: 218,
            chart_max_tokens: 5_000_000,
            buckets: vec![
                (
                    "Mon".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 2_050_000),
                        ("marketing-site".to_owned(), 600_000),
                        ("mobile-app".to_owned(), 450_000),
                    ],
                ),
                (
                    "Tue".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 2_250_000),
                        ("marketing-site".to_owned(), 500_000),
                        ("mobile-app".to_owned(), 550_000),
                    ],
                ),
                (
                    "Wed".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 1_900_000),
                        ("marketing-site".to_owned(), 700_000),
                        ("mobile-app".to_owned(), 650_000),
                    ],
                ),
                (
                    "Thu".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 1_750_000),
                        ("marketing-site".to_owned(), 450_000),
                        ("mobile-app".to_owned(), 550_000),
                    ],
                ),
                (
                    "Fri".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 650_000),
                        ("marketing-site".to_owned(), 150_000),
                        ("mobile-app".to_owned(), 100_000),
                    ],
                ),
                (
                    "Sat".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 0),
                        ("marketing-site".to_owned(), 0),
                        ("mobile-app".to_owned(), 0),
                    ],
                ),
                (
                    "Sun".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 0),
                        ("marketing-site".to_owned(), 0),
                        ("mobile-app".to_owned(), 0),
                    ],
                ),
            ],
            providers: vec![
                ProviderUsage {
                    provider_name: "Claude".to_owned(),
                    tokens: 6_500_000,
                },
                ProviderUsage {
                    provider_name: "Codex".to_owned(),
                    tokens: 5_000_000,
                },
                ProviderUsage {
                    provider_name: "OpenRouter".to_owned(),
                    tokens: 1_800_000,
                },
            ],
            project_rows: vec![
                ProjectActivityRow {
                    project_name: "api-gateway".to_owned(),
                    source: "GitHub".to_owned(),
                    color: ProjectColor::Blue,
                    issues_resolved: 6,
                    prs_merged: 4,
                    runs: 21,
                    succeeded: 19,
                    failed: 2,
                    est_cost_cents: 2740,
                },
                ProjectActivityRow {
                    project_name: "marketing-site".to_owned(),
                    source: "itsaplan".to_owned(),
                    color: ProjectColor::Orange,
                    issues_resolved: 2,
                    prs_merged: 2,
                    runs: 8,
                    succeeded: 7,
                    failed: 1,
                    est_cost_cents: 840,
                },
                ProjectActivityRow {
                    project_name: "mobile-app".to_owned(),
                    source: "GitHub".to_owned(),
                    color: ProjectColor::Green,
                    issues_resolved: 1,
                    prs_merged: 1,
                    runs: 7,
                    succeeded: 6,
                    failed: 1,
                    est_cost_cents: 890,
                },
            ],
            highest_token_runs: vec![
                HighTokenRun {
                    rank: 1,
                    title: "Fix issue · run #38".to_owned(),
                    project_name: "api-gateway".to_owned(),
                    subtitle: "Issue #329 · Forwarded header canonicalization".to_owned(),
                    tokens: 940_000,
                    est_cost_cents: 305,
                    duration_minutes: 36,
                    outcome_note: "Waiting in Inbox for review".to_owned(),
                },
                HighTokenRun {
                    rank: 2,
                    title: "Dependency upgrade · run #7".to_owned(),
                    project_name: "mobile-app".to_owned(),
                    subtitle: "React Native 0.78 worklet cleanup".to_owned(),
                    tokens: 810_000,
                    est_cost_cents: 260,
                    duration_minutes: 29,
                    outcome_note: "Waiting in Inbox for answer".to_owned(),
                },
            ],
            wait_metrics: WaitMetrics {
                median_answer_minutes: 5,
                longest_wait_minutes: 54,
                total_waiting_tenths_hours: 19,
                requests_answered: 24,
                permissions: 13,
                questions: 6,
                reviews: 5,
                timed_out: 1,
            },
            insights: vec![DashboardInsight {
                headline: "4 requests are waiting in Inbox right now".to_owned(),
                detail: "Answering the two api-gateway items will unblock queued runs.".to_owned(),
                project_name: Some("api-gateway".to_owned()),
            }],
        }
    }

    fn month_november() -> PeriodData {
        PeriodData {
            offset: 0,
            period_label: "November 2026".to_owned(),
            date_range_label: "Sun Nov 1 – Mon Nov 30, 2026".to_owned(),
            issues_delta_prev: 9,
            prs_delta_prev: 7,
            tokens_delta_pct: 18,
            billed_cost_cents: 2910,
            working_time_tenths_hours: 1059,
            chart_max_tokens: 20_000_000,
            buckets: vec![
                (
                    "Wk 1".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 9_400_000),
                        ("marketing-site".to_owned(), 2_700_000),
                        ("mobile-app".to_owned(), 2_700_000),
                    ],
                ),
                (
                    "Wk 2".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 10_400_000),
                        ("marketing-site".to_owned(), 3_000_000),
                        ("mobile-app".to_owned(), 3_000_000),
                    ],
                ),
                (
                    "Wk 3".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 11_400_000),
                        ("marketing-site".to_owned(), 3_600_000),
                        ("mobile-app".to_owned(), 3_400_000),
                    ],
                ),
                (
                    "Wk 4".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 8_600_000),
                        ("marketing-site".to_owned(), 2_400_000),
                        ("mobile-app".to_owned(), 2_300_000),
                    ],
                ),
            ],
            providers: vec![
                ProviderUsage {
                    provider_name: "Claude".to_owned(),
                    tokens: 31_400_000,
                },
                ProviderUsage {
                    provider_name: "Codex".to_owned(),
                    tokens: 23_300_000,
                },
                ProviderUsage {
                    provider_name: "OpenRouter".to_owned(),
                    tokens: 8_200_000,
                },
            ],
            project_rows: vec![
                ProjectActivityRow {
                    project_name: "api-gateway".to_owned(),
                    source: "GitHub".to_owned(),
                    color: ProjectColor::Blue,
                    issues_resolved: 26,
                    prs_merged: 21,
                    runs: 100,
                    succeeded: 91,
                    failed: 9,
                    est_cost_cents: 12950,
                },
                ProjectActivityRow {
                    project_name: "marketing-site".to_owned(),
                    source: "itsaplan".to_owned(),
                    color: ProjectColor::Orange,
                    issues_resolved: 9,
                    prs_merged: 9,
                    runs: 39,
                    succeeded: 35,
                    failed: 4,
                    est_cost_cents: 4140,
                },
                ProjectActivityRow {
                    project_name: "mobile-app".to_owned(),
                    source: "GitHub".to_owned(),
                    color: ProjectColor::Green,
                    issues_resolved: 7,
                    prs_merged: 5,
                    runs: 31,
                    succeeded: 26,
                    failed: 5,
                    est_cost_cents: 4090,
                },
            ],
            highest_token_runs: vec![
                HighTokenRun {
                    rank: 1,
                    title: "Dependency upgrade · run #6".to_owned(),
                    project_name: "mobile-app".to_owned(),
                    subtitle: "Upgrade react-native-reanimated".to_owned(),
                    tokens: 1_380_000,
                    est_cost_cents: 465,
                    duration_minutes: 52,
                    outcome_note: "5 loop attempts".to_owned(),
                },
                HighTokenRun {
                    rank: 2,
                    title: "Schema migration · run #24".to_owned(),
                    project_name: "api-gateway".to_owned(),
                    subtitle: "Migrate rate-limit buckets to v2".to_owned(),
                    tokens: 1_180_000,
                    est_cost_cents: 385,
                    duration_minutes: 46,
                    outcome_note: "Succeeded on 3rd attempt".to_owned(),
                },
                HighTokenRun {
                    rank: 3,
                    title: "Fix issue · run #36".to_owned(),
                    project_name: "api-gateway".to_owned(),
                    subtitle: "Issue #301 · Header casing".to_owned(),
                    tokens: 1_050_000,
                    est_cost_cents: 340,
                    duration_minutes: 41,
                    outcome_note: "Failed after 5 attempts".to_owned(),
                },
            ],
            wait_metrics: WaitMetrics {
                median_answer_minutes: 7,
                longest_wait_minutes: 130,
                total_waiting_tenths_hours: 107,
                requests_answered: 120,
                permissions: 66,
                questions: 30,
                reviews: 24,
                timed_out: 7,
            },
            insights: vec![
                DashboardInsight {
                    headline: "Monthly throughput rose 18% with steady 89% success rate".to_owned(),
                    detail: "152 of 170 runs completed without manual intervention.".to_owned(),
                    project_name: None,
                },
                DashboardInsight {
                    headline: "Dependency upgrade ran out of attempts twice".to_owned(),
                    detail: "Raising the loop limit from 3 to 5 avoids wasted partial runs."
                        .to_owned(),
                    project_name: Some("mobile-app".to_owned()),
                },
                DashboardInsight {
                    headline: "api-gateway slots saturated during Thu–Fri peaks".to_owned(),
                    detail: "Average queue wait reached 4.2 hours during peak PR merges."
                        .to_owned(),
                    project_name: Some("api-gateway".to_owned()),
                },
            ],
        }
    }

    fn month_october() -> PeriodData {
        PeriodData {
            offset: -1,
            period_label: "October 2026".to_owned(),
            date_range_label: "Thu Oct 1 – Sat Oct 31, 2026".to_owned(),
            issues_delta_prev: 5,
            prs_delta_prev: 4,
            tokens_delta_pct: 9,
            billed_cost_cents: 2420,
            working_time_tenths_hours: 894,
            chart_max_tokens: 20_000_000,
            buckets: vec![
                (
                    "Wk 1".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 7_900_000),
                        ("marketing-site".to_owned(), 2_200_000),
                        ("mobile-app".to_owned(), 2_100_000),
                    ],
                ),
                (
                    "Wk 2".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 8_500_000),
                        ("marketing-site".to_owned(), 2_500_000),
                        ("mobile-app".to_owned(), 2_400_000),
                    ],
                ),
                (
                    "Wk 3".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 9_100_000),
                        ("marketing-site".to_owned(), 2_800_000),
                        ("mobile-app".to_owned(), 2_600_000),
                    ],
                ),
                (
                    "Wk 4".to_owned(),
                    vec![
                        ("api-gateway".to_owned(), 8_200_000),
                        ("marketing-site".to_owned(), 2_600_000),
                        ("mobile-app".to_owned(), 2_300_000),
                    ],
                ),
            ],
            providers: vec![
                ProviderUsage {
                    provider_name: "Claude".to_owned(),
                    tokens: 26_800_000,
                },
                ProviderUsage {
                    provider_name: "Codex".to_owned(),
                    tokens: 19_400_000,
                },
                ProviderUsage {
                    provider_name: "OpenRouter".to_owned(),
                    tokens: 7_000_000,
                },
            ],
            project_rows: vec![
                ProjectActivityRow {
                    project_name: "api-gateway".to_owned(),
                    source: "GitHub".to_owned(),
                    color: ProjectColor::Blue,
                    issues_resolved: 20,
                    prs_merged: 16,
                    runs: 82,
                    succeeded: 74,
                    failed: 8,
                    est_cost_cents: 10820,
                },
                ProjectActivityRow {
                    project_name: "marketing-site".to_owned(),
                    source: "itsaplan".to_owned(),
                    color: ProjectColor::Orange,
                    issues_resolved: 8,
                    prs_merged: 7,
                    runs: 34,
                    succeeded: 31,
                    failed: 3,
                    est_cost_cents: 3650,
                },
                ProjectActivityRow {
                    project_name: "mobile-app".to_owned(),
                    source: "GitHub".to_owned(),
                    color: ProjectColor::Green,
                    issues_resolved: 5,
                    prs_merged: 5,
                    runs: 26,
                    succeeded: 22,
                    failed: 4,
                    est_cost_cents: 3410,
                },
            ],
            highest_token_runs: vec![HighTokenRun {
                rank: 1,
                title: "Auth middleware v3 · run #14".to_owned(),
                project_name: "api-gateway".to_owned(),
                subtitle: "Support OAuth 2.1 PKCE token exchange".to_owned(),
                tokens: 1_240_000,
                est_cost_cents: 410,
                duration_minutes: 49,
                outcome_note: "Succeeded on 2nd attempt".to_owned(),
            }],
            wait_metrics: WaitMetrics {
                median_answer_minutes: 8,
                longest_wait_minutes: 115,
                total_waiting_tenths_hours: 94,
                requests_answered: 102,
                permissions: 57,
                questions: 25,
                reviews: 20,
                timed_out: 5,
            },
            insights: vec![DashboardInsight {
                headline: "Review turnaround improved by 22% over September".to_owned(),
                detail: "Median operator response dropped from 10 min to 8 min.".to_owned(),
                project_name: None,
            }],
        }
    }
}

/// Formats token counts compactly (e.g. `18.4M`, `1.38M`, `0.86M`, `450K`, `0`).
pub fn format_tokens(tokens: u64) -> String {
    if tokens == 0 {
        return "0".to_owned();
    }
    if tokens >= 10_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000_000 {
        let millions = tokens as f64 / 1_000_000.0;
        let hundredths = (millions * 100.0).round() as u64 % 10;
        if hundredths == 0 {
            format!("{millions:.1}M")
        } else {
            format!("{millions:.2}M")
        }
    } else if tokens >= 100_000 {
        format!("{:.2}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.0}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

/// Formats chart axis tick values cleanly (e.g. `5M`, `2.5M`, `0`).
pub fn format_axis_tokens(tokens: u64) -> String {
    if tokens == 0 {
        return "0".to_owned();
    }
    if tokens >= 1_000_000 {
        let millions = tokens as f64 / 1_000_000.0;
        if (millions - millions.round()).abs() < 0.01 {
            format!("{}M", millions.round() as u64)
        } else {
            format!("{millions:.1}M")
        }
    } else if tokens >= 1_000 {
        format!("{}K", (tokens as f64 / 1_000.0).round() as u64)
    } else {
        tokens.to_string()
    }
}

/// Formats cents as USD currency (e.g. `6320` -> `"$63.20"`).
pub fn format_currency(cents: u64) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}

/// Formats tenths of an hour (e.g. `315` -> `"31.5 h"`, `34` -> `"3.4 h"`).
pub fn format_tenths_hours(tenths: u32) -> String {
    format!("{}.{} h", tenths / 10, tenths % 10)
}

/// Formats minutes into human-readable wait durations (e.g. `6` -> `"6 min"`, `130` -> `"2 h 10 m"`).
pub fn format_wait_minutes(minutes: u32) -> String {
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        let hours = minutes / 60;
        let rem = minutes % 60;
        if rem == 0 {
            format!("{hours} h")
        } else {
            format!("{hours} h {rem} m")
        }
    }
}

fn format_count_delta(delta: i32, unit: &str) -> String {
    match delta.cmp(&0) {
        std::cmp::Ordering::Greater => format!("{delta} more than the {unit} before"),
        std::cmp::Ordering::Less => format!("{} fewer than the {unit} before", -delta),
        std::cmp::Ordering::Equal => format!("Same as the {unit} before"),
    }
}

fn format_pct_delta(pct: i32, unit: &str) -> String {
    match pct.cmp(&0) {
        std::cmp::Ordering::Greater => format!("Up {pct}% on the {unit} before"),
        std::cmp::Ordering::Less => format!("Down {}% on the {unit} before", -pct),
        std::cmp::Ordering::Equal => format!("Flat on the {unit} before"),
    }
}

fn nice_chart_ceiling(peak: u64) -> u64 {
    if peak == 0 {
        return 1_000_000;
    }
    let candidates = [
        500_000, 1_000_000, 2_000_000, 3_000_000, 4_000_000, 5_000_000, 10_000_000, 15_000_000,
        20_000_000, 25_000_000,
    ];
    for candidate in candidates {
        if peak <= candidate {
            return candidate;
        }
    }
    peak.div_ceil(5_000_000) * 5_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_last_week_snapshot_matches_reference_metrics() {
        let dataset = DashboardDataset::sample();
        let snap = dataset.snapshot(&DashboardQuery::default(), &[], &[]);

        assert_eq!(snap.period_label, "Last week");
        assert_eq!(snap.date_range_label, "Mon Nov 9 – Sun Nov 15, 2026");
        assert_eq!(snap.issues_resolved, 14);
        assert_eq!(snap.issues_delta_text, "4 more than the week before");
        assert_eq!(snap.prs_merged, 11);
        assert_eq!(snap.prs_delta_text, "2 more than the week before");
        assert_eq!(snap.total_runs, 52);
        assert_eq!(snap.succeeded_runs, 46);
        assert_eq!(snap.failed_runs, 6);
        assert_eq!(format_tokens(snap.total_tokens), "18.4M");
        assert_eq!(snap.tokens_delta_text, "Up 12% on the week before");
        assert_eq!(format_currency(snap.est_cost_cents), "$63.20");
        assert_eq!(format_currency(snap.billed_cost_cents), "$8.90");
        assert_eq!(
            format_tenths_hours(snap.working_time_tenths_hours),
            "31.5 h"
        );

        assert_eq!(snap.project_tokens.len(), 3);
        assert_eq!(snap.project_tokens[0].project_name, "api-gateway");
        assert_eq!(format_tokens(snap.project_tokens[0].tokens), "11.4M");
        assert_eq!(snap.project_tokens[0].share_pct, 62);
        assert_eq!(snap.project_tokens[1].project_name, "marketing-site");
        assert_eq!(format_tokens(snap.project_tokens[1].tokens), "3.6M");
        assert_eq!(snap.project_tokens[1].share_pct, 20);
        assert_eq!(snap.project_tokens[2].project_name, "mobile-app");
        assert_eq!(format_tokens(snap.project_tokens[2].tokens), "3.4M");
        assert_eq!(snap.project_tokens[2].share_pct, 18);

        assert_eq!(
            format_wait_minutes(snap.wait_metrics.median_answer_minutes),
            "6 min"
        );
        assert_eq!(
            format_wait_minutes(snap.wait_metrics.longest_wait_minutes),
            "2 h 10 m"
        );
        assert_eq!(
            format_tenths_hours(snap.wait_metrics.total_waiting_tenths_hours),
            "3.4 h"
        );
        assert_eq!(
            snap.wait_metrics.summary_sentence(),
            "38 requests answered: 21 permissions, 9 questions, 8 reviews. 3 questions timed out and the agent decided."
        );
    }

    #[test]
    fn filters_snapshot_to_single_project() {
        let dataset = DashboardDataset::sample();
        let query = DashboardQuery {
            granularity: DashboardGranularity::Week,
            period_offset: 0,
            project_filter: Some("api-gateway".to_owned()),
        };
        let snap = dataset.snapshot(&query, &[], &[]);

        assert_eq!(snap.filter_label, "api-gateway");
        assert_eq!(snap.issues_resolved, 9);
        assert_eq!(snap.prs_merged, 7);
        assert_eq!(snap.total_runs, 31);
        assert_eq!(format_tokens(snap.total_tokens), "11.4M");
        assert_eq!(snap.project_rows.len(), 1);
        assert_eq!(snap.highest_token_runs.len(), 1);
        assert_eq!(snap.highest_token_runs[0].project_name, "api-gateway");
        assert_eq!(snap.insights.len(), 1);
    }

    #[test]
    fn switches_to_month_granularity_and_previous_periods() {
        let dataset = DashboardDataset::sample();
        let month_snap = dataset.snapshot(
            &DashboardQuery {
                granularity: DashboardGranularity::Month,
                period_offset: 0,
                project_filter: None,
            },
            &[],
            &[],
        );
        assert_eq!(month_snap.period_label, "November 2026");
        assert_eq!(month_snap.buckets.len(), 4);
        assert!(month_snap.can_step_prev);
        assert!(!month_snap.can_step_next);
    }
}
