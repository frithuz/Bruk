//! Shared workflow, sandbox, issue-source, and unified-diff types.

use serde::{Deserialize, Serialize};

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

/// How a loop container determines repetition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopRepeatMode {
    UntilCheckPasses,
    ForEachItem,
    FixedCount,
}

impl LoopRepeatMode {
    pub const ALL: &'static [Self] = &[Self::UntilCheckPasses, Self::ForEachItem, Self::FixedCount];

    pub fn title(self) -> &'static str {
        match self {
            Self::UntilCheckPasses => "Until a check passes",
            Self::ForEachItem => "For each item in a list",
            Self::FixedCount => "A fixed number of times",
        }
    }

    pub fn hint(self) -> Option<&'static str> {
        match self {
            Self::UntilCheckPasses => Some("retry loop"),
            Self::ForEachItem => Some("e.g. each failing test"),
            Self::FixedCount => None,
        }
    }

    pub fn test_id(self) -> &'static str {
        match self {
            Self::UntilCheckPasses => "wfe-repeat-until",
            Self::ForEachItem => "wfe-repeat-foreach",
            Self::FixedCount => "wfe-repeat-fixed",
        }
    }

    pub fn badge_prefix(self) -> &'static str {
        match self {
            Self::UntilCheckPasses => "Repeat until",
            Self::ForEachItem => "For each",
            Self::FixedCount => "Repeat fixed",
        }
    }
}

/// Output field inspected by a loop exit condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitField {
    ExitCode,
    Stdout,
    Stderr,
    Status,
}

impl ExitField {
    pub const ALL: &'static [Self] = &[Self::ExitCode, Self::Stdout, Self::Stderr, Self::Status];

    pub fn value(self) -> &'static str {
        match self {
            Self::ExitCode => "exit_code",
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::Status => "status",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ExitCode => "exit code",
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::Status => "status",
        }
    }

    pub fn from_value(value: &str) -> Self {
        match value {
            "stdout" => Self::Stdout,
            "stderr" => Self::Stderr,
            "status" => Self::Status,
            _ => Self::ExitCode,
        }
    }
}

/// Comparison operator for a loop exit condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitOperator {
    Equals,
    NotEquals,
    Contains,
    LessThan,
}

impl ExitOperator {
    pub const ALL: &'static [Self] = &[
        Self::Equals,
        Self::NotEquals,
        Self::Contains,
        Self::LessThan,
    ];

    pub fn value(self) -> &'static str {
        match self {
            Self::Equals => "eq",
            Self::NotEquals => "neq",
            Self::Contains => "contains",
            Self::LessThan => "lt",
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Equals => "=",
            Self::NotEquals => "!=",
            Self::Contains => "~",
            Self::LessThan => "<",
        }
    }

    pub fn from_value(value: &str) -> Self {
        match value {
            "neq" => Self::NotEquals,
            "contains" => Self::Contains,
            "lt" => Self::LessThan,
            _ => Self::Equals,
        }
    }
}

/// Delay between consecutive loop attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopWaitBetween {
    None,
    FiveSeconds,
    FifteenSeconds,
    ThirtySeconds,
    OneMinute,
}

impl LoopWaitBetween {
    pub const ALL: &'static [Self] = &[
        Self::None,
        Self::FiveSeconds,
        Self::FifteenSeconds,
        Self::ThirtySeconds,
        Self::OneMinute,
    ];

    pub fn value(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::FiveSeconds => "5s",
            Self::FifteenSeconds => "15s",
            Self::ThirtySeconds => "30s",
            Self::OneMinute => "1m",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::FiveSeconds => "5 seconds",
            Self::FifteenSeconds => "15 seconds",
            Self::ThirtySeconds => "30 seconds",
            Self::OneMinute => "1 minute",
        }
    }

    pub fn from_value(value: &str) -> Self {
        match value {
            "5s" => Self::FiveSeconds,
            "15s" => Self::FifteenSeconds,
            "30s" => Self::ThirtySeconds,
            "1m" => Self::OneMinute,
            _ => Self::None,
        }
    }
}

/// Action taken when a loop exhausts its maximum attempts without meeting its exit condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopExhaustedAction {
    GoToFallbackStep,
    FailRun,
    AskApproval,
    ContinueNext,
}

impl LoopExhaustedAction {
    pub const ALL: &'static [Self] = &[
        Self::GoToFallbackStep,
        Self::FailRun,
        Self::AskApproval,
        Self::ContinueNext,
    ];

    pub fn value(self) -> &'static str {
        match self {
            Self::GoToFallbackStep => "go_to_fallback",
            Self::FailRun => "fail_run",
            Self::AskApproval => "ask_approval",
            Self::ContinueNext => "continue_next",
        }
    }

    pub fn label(self, fallback_name: &str) -> String {
        match self {
            Self::GoToFallbackStep => format!("Go to step: {fallback_name}"),
            Self::FailRun => "Fail workflow run".to_owned(),
            Self::AskApproval => "Wait for approval gate".to_owned(),
            Self::ContinueNext => "Continue to next step".to_owned(),
        }
    }

    pub fn from_value(value: &str) -> Self {
        match value {
            "fail_run" => Self::FailRun,
            "ask_approval" => Self::AskApproval,
            "continue_next" => Self::ContinueNext,
            _ => Self::GoToFallbackStep,
        }
    }
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

/// One modified file shown in a run's or chat's Diff tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffFileChange {
    pub path: String,
    pub additions: usize,
    pub deletions: usize,
    pub hunk_header: String,
    pub lines: Vec<DiffLine>,
}

/// Parses a unified git diff string into structured [`DiffFileChange`] entries.
pub fn parse_unified_diff(raw: &str) -> Vec<DiffFileChange> {
    let mut files = Vec::new();
    let mut current: Option<DiffFileChange> = None;

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(prev) = current.take() {
                files.push(prev);
            }
            let path = rest
                .split_whitespace()
                .nth(1)
                .map(|p| p.strip_prefix("b/").unwrap_or(p))
                .unwrap_or(rest)
                .to_owned();
            current = Some(DiffFileChange {
                path,
                additions: 0,
                deletions: 0,
                hunk_header: String::new(),
                lines: Vec::new(),
            });
            continue;
        }
        let Some(file) = current.as_mut() else {
            continue;
        };
        if line.starts_with("@@ ") {
            if file.hunk_header.is_empty() {
                file.hunk_header = line.to_owned();
            }
            continue;
        }
        if line.starts_with("+++ ")
            || line.starts_with("--- ")
            || line.starts_with("index ")
            || line.starts_with("new file ")
            || line.starts_with("deleted file ")
        {
            continue;
        }
        if let Some(added) = line.strip_prefix('+') {
            file.additions += 1;
            if file.lines.len() < 120 {
                file.lines.push(DiffLine {
                    kind: DiffLineKind::Added,
                    text: added.to_owned(),
                });
            }
        } else if let Some(removed) = line.strip_prefix('-') {
            file.deletions += 1;
            if file.lines.len() < 120 {
                file.lines.push(DiffLine {
                    kind: DiffLineKind::Removed,
                    text: removed.to_owned(),
                });
            }
        } else if let Some(ctx_line) = line.strip_prefix(' ')
            && file.lines.len() < 120
        {
            file.lines.push(DiffLine {
                kind: DiffLineKind::Context,
                text: ctx_line.to_owned(),
            });
        }
    }

    if let Some(last) = current {
        files.push(last);
    }
    files
}
