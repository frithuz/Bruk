//! Per-project settings domain model and placeholder fixtures.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use factory_github::github_intake::{
    DetectedGitRemote, GitHubFilterClause, GitHubFilterQualifier, build_github_search_query,
    current_unix_seconds, format_synced_relative,
};
use factory_types::{ProviderKind, SecretId, format_dollars_cents};

use crate::mcp::McpServerDef;

/// Left-rail navigation sections inside the Project Settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSettingsTab {
    General,
    #[default]
    IssuesAndIntake,
    Providers,
    Workflows,
    Environment,
    McpServers,
    AgentsAndLimits,
}

impl ProjectSettingsTab {
    pub const ALL: &'static [Self] = &[
        Self::General,
        Self::IssuesAndIntake,
        Self::Providers,
        Self::Workflows,
        Self::Environment,
        Self::McpServers,
        Self::AgentsAndLimits,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::IssuesAndIntake => "Issues & intake",
            Self::Providers => "Providers",
            Self::Workflows => "Workflows",
            Self::Environment => "Environment",
            Self::McpServers => "MCP servers",
            Self::AgentsAndLimits => "Agents & limits",
        }
    }
}

pub use factory_types::{IssueSourceAuthMode, IssueSourceKind, ProjectSandboxMode};

/// One ordered rule row in the Intake rules table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectIntakeRule {
    pub id: u64,
    /// Legacy/simple single-label shorthand (`Some(label)` matches `"has label <label>"`).
    pub label: Option<String>,
    /// General GitHub search filter clauses (AND-combined). When empty and `label` is `None`,
    /// this row acts as the catch-all fallback rule.
    #[serde(default)]
    pub clauses: Vec<GitHubFilterClause>,
    pub workflow_name: String,
}

impl ProjectIntakeRule {
    /// Returns the effective GitHub filter clauses evaluated by this rule.
    pub fn effective_clauses(&self) -> Vec<GitHubFilterClause> {
        if !self.clauses.is_empty() {
            self.clauses.clone()
        } else if let Some(label) = &self.label {
            vec![GitHubFilterClause::new(GitHubFilterQualifier::Label, label)]
        } else {
            Vec::new()
        }
    }

    pub fn condition_text(&self) -> String {
        let clauses = self.effective_clauses();
        if clauses.is_empty() {
            "matches anything else".to_owned()
        } else {
            clauses
                .iter()
                .map(GitHubFilterClause::human_summary)
                .collect::<Vec<_>>()
                .join(" and ")
        }
    }

    pub fn is_fallback(&self) -> bool {
        self.clauses.is_empty() && self.label.is_none()
    }
}

/// One environment variable injected into workflow steps for a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectEnvVar {
    pub id: u64,
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

impl ProjectEnvVar {
    /// Returns the raw value for plain variables or a masked placeholder for secrets.
    pub fn display_value(&self) -> String {
        if !self.is_secret {
            return self.value.clone();
        }
        let chars: Vec<char> = self.value.chars().collect();
        if chars.len() <= 4 {
            "••••••••".to_owned()
        } else {
            let tail: String = chars[chars.len() - 4..].iter().collect();
            format!("••••••••••••{tail}")
        }
    }
}

/// One workflow definition entry listed in `Project Settings › Workflows`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorkflowSetting {
    pub name: String,
    pub file_path: String,
    pub trigger_label: String,
    pub summary: String,
    pub enabled: bool,
}

/// Validation errors returned when editing Project Settings.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProjectSettingsError {
    #[error("personal access token cannot be empty")]
    EmptyToken,
    #[error("issue label cannot be empty")]
    EmptyRuleLabel,
    #[error("filter value cannot be empty")]
    EmptyFilterValue,
    #[error("a filter for '{0}' already exists")]
    DuplicateFilterClause(String),
    #[error("intake rule needs at least one filter condition")]
    EmptyRuleConditions,
    #[error("workflow name cannot be empty")]
    EmptyWorkflowName,
    #[error("a rule for label '{0}' already exists")]
    DuplicateRuleLabel(String),
    #[error("a rule matching '{0}' already exists")]
    DuplicateRuleCondition(String),
    #[error("runs at once must be a whole number between 1 and 16")]
    InvalidConcurrency,
    #[error("variable name cannot be empty")]
    EmptyEnvKey,
    #[error("environment variable '{0}' already exists")]
    DuplicateEnvKey(String),
    #[error("no issue source extensions are installed")]
    NoExtensionsInstalled,
}

/// Complete settings state for a single project (initialized with realistic placeholder data).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub project_name: String,
    pub root_path: String,
    pub default_branch: String,
    pub git_remote: String,
    pub branch_prefix: String,
    pub use_isolated_worktrees: bool,
    pub open_prs_as_draft: bool,
    pub auto_cleanup_worktrees: bool,
    pub archived: bool,

    // Issues & intake
    pub issue_source: IssueSourceKind,
    pub github_repo: String,
    pub github_auth_mode: IssueSourceAuthMode,
    /// Legacy preview field kept for backward-compat deserialization; replaced by `github_pat_id`.
    #[serde(default)]
    pub github_token_last4: Option<String>,
    /// Reusable named App-level GitHub PAT (`Internal { purpose: GitHubPat }`) selected for this project.
    #[serde(default)]
    pub github_pat_id: Option<SecretId>,
    pub itsaplan_workspace: String,
    pub itsaplan_agent_member: String,
    pub source_connected: bool,
    pub open_issues_count: u32,
    pub last_synced_label: String,
    #[serde(default)]
    pub last_synced_at: Option<i64>,
    #[serde(default)]
    pub source_error: Option<String>,
    #[serde(default)]
    pub intake_filters: Vec<GitHubFilterClause>,
    pub intake_rules: Vec<ProjectIntakeRule>,
    pub max_concurrent_runs: u32,
    pub auto_start_matching_work: bool,
    pub add_working_label: bool,
    pub comment_with_run_summary: bool,
    pub link_pr_closes_issue: bool,

    // Providers
    pub default_provider: ProviderKind,
    pub openrouter_key_id: Option<String>,
    pub model_override: String,

    // Workflows
    pub workflows_dir: String,
    pub workflows: Vec<ProjectWorkflowSetting>,
    pub require_review_before_pr: bool,
    pub auto_reload_workflows: bool,

    // Environment
    pub env_vars: Vec<ProjectEnvVar>,
    /// App-scoped Env secrets explicitly excluded from this project (opt-out of `AllProjects`).
    #[serde(default)]
    pub excluded_app_secrets: BTreeSet<SecretId>,
    pub load_dotenv_local: bool,
    pub inherit_host_path: bool,

    // Agents & limits
    pub max_loop_attempts: u32,
    pub step_timeout_minutes: u32,
    pub per_run_budget_cents: u64,
    pub monthly_budget_cents: u64,
    pub pause_on_budget_limit: bool,
    pub require_approval_for_ci: bool,
    pub sandbox_mode: ProjectSandboxMode,
    pub preferred_runner: String,

    // MCP servers
    /// Servers this user adds for this project on top of the project's tracked ones.
    #[serde(default)]
    pub mcp_servers: Vec<McpServerDef>,
    /// Fingerprint of the project's MCP file the user approved; servers load only while the
    /// file still matches it.
    #[serde(default)]
    pub approved_mcp_fingerprint: Option<String>,

    next_rule_id: u64,
    next_env_id: u64,
}

impl ProjectSettings {
    /// Clean initial settings for a real project with no mocked issue counts, rules, or secrets.
    pub fn empty(project_name: &str, root_path: &str, default_branch: &str) -> Self {
        let branch = if default_branch.trim().is_empty() {
            "main"
        } else {
            default_branch.trim()
        };
        Self {
            project_name: project_name.to_owned(),
            root_path: root_path.to_owned(),
            default_branch: branch.to_owned(),
            git_remote: String::new(),
            branch_prefix: "agent/".to_owned(),
            use_isolated_worktrees: true,
            open_prs_as_draft: false,
            auto_cleanup_worktrees: true,
            archived: false,

            issue_source: IssueSourceKind::GitHubIssues,
            github_repo: String::new(),
            github_auth_mode: IssueSourceAuthMode::GitHubCli,
            github_token_last4: None,
            github_pat_id: None,
            itsaplan_workspace: String::new(),
            itsaplan_agent_member: String::new(),
            source_connected: false,
            open_issues_count: 0,
            last_synced_label: "Never synced".to_owned(),
            last_synced_at: None,
            source_error: None,
            intake_filters: Vec::new(),
            intake_rules: Vec::new(),
            max_concurrent_runs: 2,
            auto_start_matching_work: false,
            add_working_label: true,
            comment_with_run_summary: true,
            link_pr_closes_issue: true,

            default_provider: ProviderKind::Claude,
            openrouter_key_id: None,
            model_override: String::new(),

            workflows_dir: ".agents/bruk".to_owned(),
            workflows: Vec::new(),
            require_review_before_pr: true,
            auto_reload_workflows: true,

            env_vars: Vec::new(),
            excluded_app_secrets: BTreeSet::new(),
            load_dotenv_local: false,
            inherit_host_path: true,

            max_loop_attempts: 5,
            step_timeout_minutes: 30,
            per_run_budget_cents: 500,
            monthly_budget_cents: 12_000,
            pause_on_budget_limit: true,
            require_approval_for_ci: true,
            sandbox_mode: ProjectSandboxMode::WorkspaceWrite,
            preferred_runner: "Any available machine".to_owned(),

            mcp_servers: Vec::new(),
            approved_mcp_fingerprint: None,
            next_rule_id: 1,
            next_env_id: 1,
        }
    }

    /// Builds placeholder settings for `project_name`.
    ///
    /// `"api-gateway"` matches the `05 ProjectSettings.png` reference mockup verbatim;
    /// other sample or registered projects receive tailored placeholder settings.
    pub fn for_project(project_name: &str, root_path: &str, default_branch: &str) -> Self {
        let branch = if default_branch.trim().is_empty() {
            "main"
        } else {
            default_branch.trim()
        };
        let path = if root_path.trim().is_empty() {
            format!("/Users/jonathan/src/{project_name}")
        } else {
            root_path.to_owned()
        };

        match project_name {
            "api-gateway" => Self::sample_api_gateway(path, branch.to_owned()),
            "marketing-site" => Self::sample_marketing_site(path, branch.to_owned()),
            "mobile-app" => Self::sample_mobile_app(path, branch.to_owned()),
            other => Self::sample_generic(other, path, branch.to_owned()),
        }
    }

    /// Reference fixture for `api-gateway` matching `05 ProjectSettings.png`.
    pub fn sample_api_gateway(root_path: String, default_branch: String) -> Self {
        Self {
            project_name: "api-gateway".to_owned(),
            root_path,
            default_branch,
            git_remote: "origin · git@github.com:acme/api-gateway.git".to_owned(),
            branch_prefix: "agent/".to_owned(),
            use_isolated_worktrees: true,
            open_prs_as_draft: false,
            auto_cleanup_worktrees: true,
            archived: false,

            issue_source: IssueSourceKind::GitHubIssues,
            github_repo: "acme/api-gateway".to_owned(),
            github_auth_mode: IssueSourceAuthMode::GitHubCli,
            github_token_last4: None,
            github_pat_id: None,
            itsaplan_workspace: "acme-engineering".to_owned(),
            itsaplan_agent_member: "@factory-bot".to_owned(),
            source_connected: true,
            open_issues_count: 14,
            last_synced_label: "synced 1 min ago".to_owned(),
            last_synced_at: None,
            source_error: None,
            intake_filters: vec![GitHubFilterClause::new(
                GitHubFilterQualifier::Label,
                "agent-ready",
            )],
            intake_rules: vec![
                ProjectIntakeRule {
                    id: 1,
                    label: Some("bug".to_owned()),
                    clauses: Vec::new(),
                    workflow_name: "Fix issue".to_owned(),
                },
                ProjectIntakeRule {
                    id: 2,
                    label: Some("docs".to_owned()),
                    clauses: Vec::new(),
                    workflow_name: "Docs refresh".to_owned(),
                },
                ProjectIntakeRule {
                    id: 3,
                    label: None,
                    clauses: Vec::new(),
                    workflow_name: "Triage new issues".to_owned(),
                },
            ],
            max_concurrent_runs: 2,
            auto_start_matching_work: true,
            add_working_label: true,
            comment_with_run_summary: true,
            link_pr_closes_issue: true,

            default_provider: ProviderKind::Claude,
            openrouter_key_id: Some("openrouter-acme".to_owned()),
            model_override: String::new(),

            workflows_dir: ".agents/bruk".to_owned(),
            workflows: vec![
                ProjectWorkflowSetting {
                    name: "Fix issue".to_owned(),
                    file_path: ".agents/bruk/workflows/fix-issue.yaml".to_owned(),
                    trigger_label: "Intake".to_owned(),
                    summary: "4 steps · Claude / Codex".to_owned(),
                    enabled: true,
                },
                ProjectWorkflowSetting {
                    name: "Docs refresh".to_owned(),
                    file_path: ".agents/bruk/workflows/docs-refresh.yaml".to_owned(),
                    trigger_label: "Intake".to_owned(),
                    summary: "3 steps · Claude".to_owned(),
                    enabled: true,
                },
                ProjectWorkflowSetting {
                    name: "Triage new issues".to_owned(),
                    file_path: ".agents/bruk/workflows/triage-new-issues.yaml".to_owned(),
                    trigger_label: "Intake".to_owned(),
                    summary: "2 steps · OpenRouter".to_owned(),
                    enabled: true,
                },
                ProjectWorkflowSetting {
                    name: "Dependency upgrade".to_owned(),
                    file_path: ".agents/bruk/workflows/dependency-upgrade.yaml".to_owned(),
                    trigger_label: "Intake".to_owned(),
                    summary: "4 steps · Codex".to_owned(),
                    enabled: true,
                },
                ProjectWorkflowSetting {
                    name: "Release prep".to_owned(),
                    file_path: ".agents/bruk/workflows/release-prep.yaml".to_owned(),
                    trigger_label: "Ad-hoc only".to_owned(),
                    summary: "5 steps · Claude / Codex".to_owned(),
                    enabled: true,
                },
            ],
            require_review_before_pr: true,
            auto_reload_workflows: true,

            env_vars: vec![
                ProjectEnvVar {
                    id: 1,
                    key: "NODE_ENV".to_owned(),
                    value: "test".to_owned(),
                    is_secret: false,
                },
                ProjectEnvVar {
                    id: 2,
                    key: "LOG_LEVEL".to_owned(),
                    value: "debug".to_owned(),
                    is_secret: false,
                },
                ProjectEnvVar {
                    id: 3,
                    key: "REDIS_URL".to_owned(),
                    value: "redis://127.0.0.1:6379/1".to_owned(),
                    is_secret: false,
                },
                ProjectEnvVar {
                    id: 4,
                    key: "STAGING_API_TOKEN".to_owned(),
                    value: "sf_stg_live_token_9b4e".to_owned(),
                    is_secret: true,
                },
            ],
            excluded_app_secrets: BTreeSet::new(),
            load_dotenv_local: true,
            inherit_host_path: true,

            max_loop_attempts: 5,
            step_timeout_minutes: 15,
            per_run_budget_cents: 500,
            monthly_budget_cents: 12_000,
            pause_on_budget_limit: true,
            require_approval_for_ci: true,
            sandbox_mode: ProjectSandboxMode::WorkspaceWrite,
            preferred_runner: "Auto".to_owned(),

            mcp_servers: Vec::new(),
            approved_mcp_fingerprint: None,
            next_rule_id: 4,
            next_env_id: 5,
        }
    }

    fn sample_marketing_site(root_path: String, default_branch: String) -> Self {
        let mut settings = Self::sample_api_gateway(root_path, default_branch);
        settings.project_name = "marketing-site".to_owned();
        settings.git_remote = "origin · git@github.com:acme/marketing-site.git".to_owned();
        settings.github_repo = "acme/marketing-site".to_owned();
        settings.open_issues_count = 6;
        settings.last_synced_label = "synced 3 min ago".to_owned();
        settings.monthly_budget_cents = 6_000;
        settings
    }

    fn sample_mobile_app(root_path: String, default_branch: String) -> Self {
        let mut settings = Self::sample_api_gateway(root_path, default_branch);
        settings.project_name = "mobile-app".to_owned();
        settings.git_remote = "origin · git@github.com:acme/mobile-app.git".to_owned();
        settings.github_repo = "acme/mobile-app".to_owned();
        settings.open_issues_count = 9;
        settings.last_synced_label = "synced 2 min ago".to_owned();
        settings.default_provider = ProviderKind::Codex;
        settings.openrouter_key_id = Some("openrouter-personal".to_owned());
        settings.monthly_budget_cents = 8_500;
        settings
    }

    fn sample_generic(project_name: &str, root_path: String, default_branch: String) -> Self {
        let mut settings = Self::sample_api_gateway(root_path, default_branch);
        settings.project_name = project_name.to_owned();
        settings.git_remote = format!("origin · git@github.com:acme/{project_name}.git");
        settings.github_repo = format!("acme/{project_name}");
        settings.open_issues_count = 4;
        settings.last_synced_label = "synced just now".to_owned();
        settings.openrouter_key_id = None;
        settings
    }

    /// Window title matching `api-gateway — Project Settings`.
    pub fn window_title(&self) -> String {
        format!("{} — Project Settings", self.project_name)
    }

    /// Subtitle displayed below the `Intake rules` heading.
    pub fn intake_rules_subtitle(&self) -> String {
        let clauses = self.effective_intake_filter_clauses();
        if clauses.is_empty() {
            "All open issues become work. The first matching rule picks the workflow.".to_owned()
        } else if clauses.len() == 1
            && clauses[0].qualifier == GitHubFilterQualifier::Label
            && !clauses[0].negated
        {
            format!(
                "Open issues labeled {} become work. The first matching rule picks the workflow.",
                clauses[0].value
            )
        } else {
            let summary = clauses
                .iter()
                .map(GitHubFilterClause::human_summary)
                .collect::<Vec<_>>()
                .join(" and ");
            format!(
                "Open issues matching {summary} become work. The first matching rule picks the workflow."
            )
        }
    }

    /// Returns all active non-empty GitHub search filter clauses configured for intake.
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

    /// Renders the combined GitHub issue search query for all active project intake filters.
    pub fn effective_github_search_query(&self) -> String {
        build_github_search_query(&self.effective_intake_filter_clauses())
    }

    /// Adds an optional GitHub search filter clause to this project's intake filter.
    pub fn add_intake_filter(
        &mut self,
        clause: GitHubFilterClause,
    ) -> Result<(), ProjectSettingsError> {
        if clause.is_empty() {
            return Err(ProjectSettingsError::EmptyFilterValue);
        }
        let key = clause.dedupe_key();
        if self
            .effective_intake_filter_clauses()
            .iter()
            .any(|existing| existing.dedupe_key() == key)
        {
            return Err(ProjectSettingsError::DuplicateFilterClause(
                clause.to_github_query_token(),
            ));
        }
        self.intake_filters.push(clause);
        Ok(())
    }

    /// Removes an optional project intake filter clause by index.
    pub fn remove_intake_filter(&mut self, index: usize) -> bool {
        if index < self.intake_filters.len() {
            self.intake_filters.remove(index);
            true
        } else {
            false
        }
    }

    /// Clears all optional additional project intake filters.
    pub fn clear_intake_filters(&mut self) {
        self.intake_filters.clear();
    }

    /// Human-readable status text shown in the Issue source details card.
    pub fn source_status_text(&self) -> String {
        match self.issue_source {
            IssueSourceKind::GitHubIssues => {
                if self.github_repo.trim().is_empty() {
                    "Not configured · enter owner/repo".to_owned()
                } else if let Some(err) = self
                    .source_error
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    format!("Sync error · {err}")
                } else if !self.source_connected {
                    "Ready to verify · press Test & Sync".to_owned()
                } else {
                    let synced = match self.last_synced_at {
                        Some(ts) => format_synced_relative(ts, current_unix_seconds()),
                        None => self.last_synced_label.clone(),
                    };
                    format!(
                        "Connected · {} open issues · {}",
                        self.open_issues_count, synced
                    )
                }
            }
            IssueSourceKind::ItsAPlan => {
                if self.itsaplan_workspace.trim().is_empty() {
                    "Not configured · enter workspace".to_owned()
                } else {
                    format!(
                        "Connected · {} delegated items · {}",
                        self.open_issues_count, self.last_synced_label
                    )
                }
            }
            IssueSourceKind::Extension => "No issue source extensions installed".to_owned(),
        }
    }

    /// Applies auto-detected git remote metadata when the fields have not yet been configured.
    pub fn apply_detected_git_remote(&mut self, detected: &DetectedGitRemote) {
        if self.git_remote.trim().is_empty() {
            self.git_remote = detected.remote_display.clone();
        }
        if self.github_repo.trim().is_empty()
            && let Some(slug) = &detected.github_repo
        {
            self.github_repo = slug.clone();
        }
    }

    /// Records a successful live GitHub Issues sync.
    pub fn apply_github_sync_success(
        &mut self,
        normalized_repo: &str,
        open_issues_count: u32,
        synced_at_unix: i64,
    ) {
        self.github_repo = normalized_repo.to_owned();
        self.source_connected = true;
        self.source_error = None;
        self.open_issues_count = open_issues_count;
        self.last_synced_at = Some(synced_at_unix);
        self.last_synced_label = format_synced_relative(synced_at_unix, current_unix_seconds());
    }

    /// Records a failed GitHub Issues sync without clobbering the configured repository slug.
    pub fn apply_github_sync_error(&mut self, error: impl Into<String>) {
        self.source_connected = false;
        self.source_error = Some(error.into());
    }

    /// Returns the workflow name configured on the catch-all fallback intake rule, if any.
    pub fn fallback_workflow_name(&self) -> String {
        self.intake_rules
            .iter()
            .find(|rule| rule.is_fallback())
            .map(|rule| rule.workflow_name.clone())
            .unwrap_or_default()
    }

    /// Sets or updates the catch-all fallback intake rule (clears it if blank).
    pub fn set_fallback_workflow(&mut self, workflow_name: &str) {
        let trimmed = workflow_name.trim();
        if trimmed.is_empty() {
            self.intake_rules.retain(|rule| !rule.is_fallback());
            return;
        }
        if let Some(existing) = self.intake_rules.iter_mut().find(|rule| rule.is_fallback()) {
            existing.workflow_name = trimmed.to_owned();
        } else {
            let id = self.next_rule_id;
            self.next_rule_id += 1;
            self.intake_rules.push(ProjectIntakeRule {
                id,
                label: None,
                clauses: Vec::new(),
                workflow_name: trimmed.to_owned(),
            });
        }
    }

    /// Sign-in summary displayed in the `Sign-in` row of the Issue source card.
    pub fn sign_in_summary(&self) -> String {
        self.sign_in_summary_with_pat(None, None)
    }

    /// Sign-in summary showing the selected named GitHub PAT when available.
    pub fn sign_in_summary_with_pat(
        &self,
        pat_name: Option<&str>,
        pat_preview: Option<&str>,
    ) -> String {
        match self.issue_source {
            IssueSourceKind::GitHubIssues => match self.github_auth_mode {
                IssueSourceAuthMode::GitHubCli => "Your GitHub CLI (gh) sign-in".to_owned(),
                IssueSourceAuthMode::PersonalToken => {
                    if let Some(name) = pat_name {
                        match pat_preview {
                            Some(preview) => format!("Personal access token · {name} ({preview})"),
                            None => format!("Personal access token · {name}"),
                        }
                    } else if let Some(tail) = self.github_token_last4.as_deref() {
                        format!("Personal access token (ghp_••••••••••••{tail})")
                    } else if self.github_pat_id.is_some() {
                        "Personal access token (configured)".to_owned()
                    } else {
                        "Personal access token (not selected)".to_owned()
                    }
                }
            },
            IssueSourceKind::ItsAPlan => {
                format!(
                    "Signed in as {} via workspace token",
                    self.itsaplan_agent_member
                )
            }
            IssueSourceKind::Extension => "Managed by extension".to_owned(),
        }
    }

    /// Formatted per-run budget cap (e.g. `$5.00`).
    pub fn formatted_per_run_budget(&self) -> String {
        format_dollars_cents(self.per_run_budget_cents)
    }

    /// Formatted monthly project budget cap (e.g. `$120.00`).
    pub fn formatted_monthly_budget(&self) -> String {
        format_dollars_cents(self.monthly_budget_cents)
    }

    /// Selects a supported issue source.
    pub fn set_issue_source(
        &mut self,
        source: IssueSourceKind,
    ) -> Result<(), ProjectSettingsError> {
        if source == IssueSourceKind::Extension {
            return Err(ProjectSettingsError::NoExtensionsInstalled);
        }
        self.issue_source = source;
        Ok(())
    }

    /// Switches GitHub sign-in to a masked personal access token.
    pub fn set_github_token(&mut self, raw_token: &str) -> Result<(), ProjectSettingsError> {
        let trimmed = raw_token.trim();
        if trimmed.is_empty() {
            return Err(ProjectSettingsError::EmptyToken);
        }
        let chars: Vec<char> = trimmed.chars().collect();
        let tail: String = if chars.len() >= 4 {
            chars[chars.len() - 4..].iter().collect()
        } else {
            trimmed.to_owned()
        };
        self.github_auth_mode = IssueSourceAuthMode::PersonalToken;
        self.github_token_last4 = Some(tail);
        self.source_connected = true;
        Ok(())
    }

    /// Switches GitHub sign-in back to the local GitHub CLI session.
    pub fn use_github_cli(&mut self) {
        self.github_auth_mode = IssueSourceAuthMode::GitHubCli;
        self.source_connected = true;
    }

    /// Adds a new label intake rule right before the catch-all fallback row.
    pub fn add_intake_rule(
        &mut self,
        label: &str,
        workflow_name: &str,
    ) -> Result<u64, ProjectSettingsError> {
        let label = label.trim();
        if label.is_empty() {
            return Err(ProjectSettingsError::EmptyRuleLabel);
        }
        let workflow_name = workflow_name.trim();
        if workflow_name.is_empty() {
            return Err(ProjectSettingsError::EmptyWorkflowName);
        }
        if self.intake_rules.iter().any(|rule| {
            rule.label
                .as_deref()
                .is_some_and(|existing| existing.eq_ignore_ascii_case(label))
        }) {
            return Err(ProjectSettingsError::DuplicateRuleLabel(label.to_owned()));
        }

        let id = self.next_rule_id;
        self.next_rule_id += 1;
        let new_rule = ProjectIntakeRule {
            id,
            label: Some(label.to_owned()),
            clauses: vec![GitHubFilterClause::new(GitHubFilterQualifier::Label, label)],
            workflow_name: workflow_name.to_owned(),
        };

        if let Some(fallback_pos) = self.intake_rules.iter().position(|r| r.is_fallback()) {
            self.intake_rules.insert(fallback_pos, new_rule);
        } else {
            self.intake_rules.push(new_rule);
        }
        Ok(id)
    }

    /// Adds a general GitHub search-filter intake rule (single or multi-clause AND)
    /// right before any catch-all fallback row.
    pub fn add_intake_rule_with_clauses(
        &mut self,
        clauses: Vec<GitHubFilterClause>,
        workflow_name: &str,
    ) -> Result<u64, ProjectSettingsError> {
        let cleaned: Vec<GitHubFilterClause> = clauses
            .into_iter()
            .filter(|clause| !clause.is_empty())
            .collect();
        if cleaned.is_empty() {
            return Err(ProjectSettingsError::EmptyRuleConditions);
        }

        // Preserve existing single positive-label error messages for callers.
        if cleaned.len() == 1
            && cleaned[0].qualifier == GitHubFilterQualifier::Label
            && !cleaned[0].negated
        {
            return self.add_intake_rule(&cleaned[0].value, workflow_name);
        }

        let workflow_name = workflow_name.trim();
        if workflow_name.is_empty() {
            return Err(ProjectSettingsError::EmptyWorkflowName);
        }

        let candidate_keys: Vec<String> =
            cleaned.iter().map(GitHubFilterClause::dedupe_key).collect();
        let candidate_summary = cleaned
            .iter()
            .map(GitHubFilterClause::human_summary)
            .collect::<Vec<_>>()
            .join(" and ");

        if self.intake_rules.iter().any(|existing| {
            let existing_keys: Vec<String> = existing
                .effective_clauses()
                .iter()
                .map(GitHubFilterClause::dedupe_key)
                .collect();
            existing_keys == candidate_keys
        }) {
            return Err(ProjectSettingsError::DuplicateRuleCondition(
                candidate_summary,
            ));
        }

        let id = self.next_rule_id;
        self.next_rule_id += 1;
        let new_rule = ProjectIntakeRule {
            id,
            label: None,
            clauses: cleaned,
            workflow_name: workflow_name.to_owned(),
        };

        if let Some(fallback_pos) = self.intake_rules.iter().position(|r| r.is_fallback()) {
            self.intake_rules.insert(fallback_pos, new_rule);
        } else {
            self.intake_rules.push(new_rule);
        }
        Ok(id)
    }

    /// Removes a non-fallback intake rule by id.
    pub fn remove_intake_rule(&mut self, id: u64) -> bool {
        if let Some(pos) = self
            .intake_rules
            .iter()
            .position(|rule| rule.id == id && !rule.is_fallback())
        {
            self.intake_rules.remove(pos);
            true
        } else {
            false
        }
    }

    /// Parses and updates `max_concurrent_runs` from a text field value.
    pub fn set_max_concurrent_runs_str(&mut self, value: &str) -> Result<(), ProjectSettingsError> {
        let trimmed = value.trim();
        let parsed: u32 = trimmed
            .parse()
            .map_err(|_| ProjectSettingsError::InvalidConcurrency)?;
        if !(1..=16).contains(&parsed) {
            return Err(ProjectSettingsError::InvalidConcurrency);
        }
        self.max_concurrent_runs = parsed;
        Ok(())
    }

    /// Steps `max_concurrent_runs` up or down within `1..=16`.
    pub fn step_max_concurrent_runs(&mut self, delta: i32) {
        self.max_concurrent_runs = (self.max_concurrent_runs as i32 + delta).clamp(1, 16) as u32;
    }

    /// Adds a project environment variable.
    pub fn add_env_var(
        &mut self,
        key: &str,
        value: &str,
        is_secret: bool,
    ) -> Result<u64, ProjectSettingsError> {
        let key = key.trim();
        if key.is_empty() {
            return Err(ProjectSettingsError::EmptyEnvKey);
        }
        if self
            .env_vars
            .iter()
            .any(|v| v.key.eq_ignore_ascii_case(key))
        {
            return Err(ProjectSettingsError::DuplicateEnvKey(key.to_owned()));
        }
        let id = self.next_env_id;
        self.next_env_id += 1;
        self.env_vars.push(ProjectEnvVar {
            id,
            key: key.to_owned(),
            value: value.trim().to_owned(),
            is_secret,
        });
        Ok(id)
    }

    /// Adds (or replaces, by name) one of this user's servers for the project.
    pub fn upsert_mcp_server(&mut self, server: McpServerDef) {
        match self.mcp_servers.iter_mut().find(|s| s.name == server.name) {
            Some(existing) => *existing = server,
            None => self.mcp_servers.push(server),
        }
    }

    pub fn remove_mcp_server(&mut self, name: &str) -> bool {
        let before = self.mcp_servers.len();
        self.mcp_servers.retain(|s| s.name != name);
        self.mcp_servers.len() != before
    }

    /// Removes a project environment variable by id.
    pub fn remove_env_var(&mut self, id: u64) -> bool {
        let before = self.env_vars.len();
        self.env_vars.retain(|v| v.id != id);
        self.env_vars.len() != before
    }

    /// Toggles whether a project workflow is enabled.
    pub fn toggle_workflow(&mut self, name: &str) {
        if let Some(wf) = self.workflows.iter_mut().find(|w| w.name == name) {
            wf.enabled = !wf.enabled;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_api_gateway_matches_reference_project_settings_mockup() {
        let settings =
            ProjectSettings::for_project("api-gateway", "/workspace/api-gateway", "main");

        assert_eq!(settings.window_title(), "api-gateway — Project Settings");
        assert_eq!(settings.issue_source, IssueSourceKind::GitHubIssues);
        assert_eq!(settings.github_repo, "acme/api-gateway");
        assert_eq!(settings.sign_in_summary(), "Your GitHub CLI (gh) sign-in");
        assert_eq!(
            settings.source_status_text(),
            "Connected · 14 open issues · synced 1 min ago"
        );
        assert_eq!(
            settings.intake_rules_subtitle(),
            "Open issues labeled agent-ready become work. The first matching rule picks the workflow."
        );
        assert_eq!(settings.intake_rules.len(), 3);
        assert_eq!(settings.intake_rules[0].condition_text(), "has label bug");
        assert_eq!(settings.intake_rules[0].workflow_name, "Fix issue");
        assert_eq!(settings.intake_rules[1].condition_text(), "has label docs");
        assert_eq!(settings.intake_rules[1].workflow_name, "Docs refresh");
        assert_eq!(
            settings.intake_rules[2].condition_text(),
            "matches anything else"
        );
        assert_eq!(settings.intake_rules[2].workflow_name, "Triage new issues");
        assert_eq!(settings.max_concurrent_runs, 2);
        assert!(settings.auto_start_matching_work);
        assert!(settings.add_working_label);
        assert!(settings.comment_with_run_summary);
        assert!(settings.link_pr_closes_issue);
    }

    #[test]
    fn adding_intake_rule_inserts_before_fallback_and_validates_duplicates() {
        let mut settings =
            ProjectSettings::for_project("api-gateway", "/workspace/api-gateway", "main");

        let id = settings
            .add_intake_rule("security", "Fix issue")
            .expect("rule should be added");
        assert_eq!(settings.intake_rules.len(), 4);
        assert_eq!(
            settings.intake_rules[2].condition_text(),
            "has label security"
        );
        assert!(settings.intake_rules[3].is_fallback());

        let dup = settings.add_intake_rule("BUG", "Docs refresh");
        assert_eq!(
            dup,
            Err(ProjectSettingsError::DuplicateRuleLabel("BUG".to_owned()))
        );

        assert!(settings.remove_intake_rule(id));
        assert_eq!(settings.intake_rules.len(), 3);
        // Fallback row cannot be removed.
        let fallback_id = settings.intake_rules[2].id;
        assert!(!settings.remove_intake_rule(fallback_id));
    }

    #[test]
    fn token_auth_concurrency_and_env_var_mutations_validate_cleanly() {
        let mut settings =
            ProjectSettings::for_project("api-gateway", "/workspace/api-gateway", "main");

        assert_eq!(
            settings.set_github_token("   "),
            Err(ProjectSettingsError::EmptyToken)
        );
        settings.set_github_token("ghp_demo_secret_77ab").unwrap();
        assert_eq!(
            settings.sign_in_summary(),
            "Personal access token (ghp_••••••••••••77ab)"
        );
        settings.use_github_cli();
        assert_eq!(settings.sign_in_summary(), "Your GitHub CLI (gh) sign-in");

        assert_eq!(
            settings.set_max_concurrent_runs_str("0"),
            Err(ProjectSettingsError::InvalidConcurrency)
        );
        settings.set_max_concurrent_runs_str("4").unwrap();
        assert_eq!(settings.max_concurrent_runs, 4);

        let env_id = settings
            .add_env_var("WEBHOOK_SECRET", "whsec_abcdef1234", true)
            .unwrap();
        assert_eq!(
            settings.env_vars.last().unwrap().display_value(),
            "••••••••••••1234"
        );
        assert!(settings.remove_env_var(env_id));
    }

    #[test]
    fn supports_optional_github_search_filters_and_assignee_or_multi_clause_intake_rules() {
        let mut settings = ProjectSettings::empty("my-service", "/repo/my-service", "main");
        assert!(settings.effective_intake_filter_clauses().is_empty());
        settings
            .add_intake_filter(GitHubFilterClause::new(
                GitHubFilterQualifier::Assignee,
                "@factory-bot",
            ))
            .unwrap();
        settings
            .add_intake_filter(GitHubFilterClause::negated(
                GitHubFilterQualifier::Label,
                "blocked",
            ))
            .unwrap();
        assert_eq!(
            settings.effective_github_search_query(),
            "assignee:factory-bot -label:blocked"
        );
        assert_eq!(
            settings.intake_rules_subtitle(),
            "Open issues matching assigned to @factory-bot and excludes label blocked become work. The first matching rule picks the workflow."
        );

        settings.set_fallback_workflow("Triage new issues");
        let rule_id = settings
            .add_intake_rule_with_clauses(
                vec![
                    GitHubFilterClause::new(GitHubFilterQualifier::Assignee, "@alice"),
                    GitHubFilterClause::new(GitHubFilterQualifier::Milestone, "v2.0"),
                ],
                "Release prep",
            )
            .unwrap();
        assert_eq!(settings.intake_rules.len(), 2);
        assert_eq!(
            settings.intake_rules[0].condition_text(),
            "assigned to @alice and milestone v2.0"
        );
        assert!(settings.intake_rules[1].is_fallback());

        // Duplicate multi-clause condition is rejected.
        assert!(matches!(
            settings.add_intake_rule_with_clauses(
                vec![
                    GitHubFilterClause::new(GitHubFilterQualifier::Assignee, "ALICE"),
                    GitHubFilterClause::new(GitHubFilterQualifier::Milestone, "V2.0"),
                ],
                "Other",
            ),
            Err(ProjectSettingsError::DuplicateRuleCondition(_))
        ));

        assert!(settings.remove_intake_rule(rule_id));
        assert!(settings.remove_intake_filter(0));
        assert_eq!(settings.effective_github_search_query(), "-label:blocked");
    }
}
