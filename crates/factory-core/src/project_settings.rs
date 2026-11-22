//! Per-project settings domain model and placeholder fixtures.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::preferences::{ProviderKind, format_dollars_cents};

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
    AgentsAndLimits,
}

impl ProjectSettingsTab {
    pub const ALL: &'static [Self] = &[
        Self::General,
        Self::IssuesAndIntake,
        Self::Providers,
        Self::Workflows,
        Self::Environment,
        Self::AgentsAndLimits,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::IssuesAndIntake => "Issues & intake",
            Self::Providers => "Providers",
            Self::Workflows => "Workflows",
            Self::Environment => "Environment",
            Self::AgentsAndLimits => "Agents & limits",
        }
    }
}

/// Issue source integration selectable for a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSourceKind {
    #[default]
    GitHubIssues,
    ItsAPlan,
    Extension,
}

impl IssueSourceKind {
    pub fn badge(self) -> &'static str {
        match self {
            Self::GitHubIssues => "GH",
            Self::ItsAPlan => "IP",
            Self::Extension => "+",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::GitHubIssues => "GitHub Issues",
            Self::ItsAPlan => "itsaplan",
            Self::Extension => "From an extension",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::GitHubIssues => "Issues and labels from one repository",
            Self::ItsAPlan => "Work items delegated to an agent member",
            Self::Extension => "No issue source extensions installed",
        }
    }
}

/// Sign-in method used by the GitHub Issues source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSourceAuthMode {
    #[default]
    GitHubCli,
    PersonalToken,
}

/// One ordered rule row in the Intake rules table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectIntakeRule {
    pub id: u64,
    /// `Some(label)` matches `"has label <label>"`; `None` is the catch-all fallback row.
    pub label: Option<String>,
    pub workflow_name: String,
}

impl ProjectIntakeRule {
    pub fn condition_text(&self) -> String {
        match &self.label {
            Some(label) => format!("has label {label}"),
            None => "matches anything else".to_owned(),
        }
    }

    pub fn is_fallback(&self) -> bool {
        self.label.is_none()
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

/// Sandbox policy applied to agent steps running inside the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSandboxMode {
    ReadOnly,
    #[default]
    WorkspaceWrite,
    FullAccess,
}

impl ProjectSandboxMode {
    pub const ALL: &'static [Self] = &[Self::ReadOnly, Self::WorkspaceWrite, Self::FullAccess];

    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "Read-only",
            Self::WorkspaceWrite => "Workspace write",
            Self::FullAccess => "Full access",
        }
    }
}

/// Validation errors returned when editing Project Settings.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProjectSettingsError {
    #[error("personal access token cannot be empty")]
    EmptyToken,
    #[error("issue label cannot be empty")]
    EmptyRuleLabel,
    #[error("workflow name cannot be empty")]
    EmptyWorkflowName,
    #[error("a rule for label '{0}' already exists")]
    DuplicateRuleLabel(String),
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
    pub github_token_last4: Option<String>,
    pub itsaplan_workspace: String,
    pub itsaplan_agent_member: String,
    pub source_connected: bool,
    pub open_issues_count: u32,
    pub last_synced_label: String,
    pub required_label: String,
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

    next_rule_id: u64,
    next_env_id: u64,
}

impl ProjectSettings {
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
            itsaplan_workspace: "acme-engineering".to_owned(),
            itsaplan_agent_member: "@factory-bot".to_owned(),
            source_connected: true,
            open_issues_count: 14,
            last_synced_label: "synced 1 min ago".to_owned(),
            required_label: "agent-ready".to_owned(),
            intake_rules: vec![
                ProjectIntakeRule {
                    id: 1,
                    label: Some("bug".to_owned()),
                    workflow_name: "Fix issue".to_owned(),
                },
                ProjectIntakeRule {
                    id: 2,
                    label: Some("docs".to_owned()),
                    workflow_name: "Docs refresh".to_owned(),
                },
                ProjectIntakeRule {
                    id: 3,
                    label: None,
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

            workflows_dir: ".factory/workflows".to_owned(),
            workflows: vec![
                ProjectWorkflowSetting {
                    name: "Fix issue".to_owned(),
                    file_path: ".factory/workflows/fix-issue.yaml".to_owned(),
                    trigger_label: "Intake".to_owned(),
                    summary: "4 steps · Claude / Codex".to_owned(),
                    enabled: true,
                },
                ProjectWorkflowSetting {
                    name: "Docs refresh".to_owned(),
                    file_path: ".factory/workflows/docs-refresh.yaml".to_owned(),
                    trigger_label: "Intake".to_owned(),
                    summary: "3 steps · Claude".to_owned(),
                    enabled: true,
                },
                ProjectWorkflowSetting {
                    name: "Triage new issues".to_owned(),
                    file_path: ".factory/workflows/triage-new-issues.yaml".to_owned(),
                    trigger_label: "Intake".to_owned(),
                    summary: "2 steps · OpenRouter".to_owned(),
                    enabled: true,
                },
                ProjectWorkflowSetting {
                    name: "Dependency upgrade".to_owned(),
                    file_path: ".factory/workflows/dependency-upgrade.yaml".to_owned(),
                    trigger_label: "Intake".to_owned(),
                    summary: "4 steps · Codex".to_owned(),
                    enabled: true,
                },
                ProjectWorkflowSetting {
                    name: "Release prep".to_owned(),
                    file_path: ".factory/workflows/release-prep.yaml".to_owned(),
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
        format!(
            "Open issues labeled {} become work. The first matching rule picks the workflow.",
            self.required_label
        )
    }

    /// Human-readable status text shown in the Issue source details card.
    pub fn source_status_text(&self) -> String {
        match self.issue_source {
            IssueSourceKind::GitHubIssues => {
                if self.github_repo.trim().is_empty() {
                    "Not configured · enter owner/repo".to_owned()
                } else {
                    format!(
                        "Connected · {} open issues · {}",
                        self.open_issues_count, self.last_synced_label
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

    /// Sign-in summary displayed in the `Sign-in` row of the Issue source card.
    pub fn sign_in_summary(&self) -> String {
        match self.issue_source {
            IssueSourceKind::GitHubIssues => match self.github_auth_mode {
                IssueSourceAuthMode::GitHubCli => "Your GitHub CLI (gh) sign-in".to_owned(),
                IssueSourceAuthMode::PersonalToken => {
                    let tail = self.github_token_last4.as_deref().unwrap_or("4a9f");
                    format!("Personal access token (ghp_••••••••••••{tail})")
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
}
