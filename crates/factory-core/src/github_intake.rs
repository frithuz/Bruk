//! GitHub Issues intake client and search-filter builder backed by the local GitHub CLI (`gh`).
//!
//! Supports both the user's existing `gh auth login` session and per-project
//! personal access tokens (passed via `GH_TOKEN` on the spawned process without
//! mutating the user's global `gh` configuration), plus composable GitHub issue
//! search filters (labels, assignees, authors, milestones, text, dates, counts,
//! exclusions, and raw GitHub search queries).

use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default executable filename searched on `PATH`.
#[cfg(windows)]
pub const GH_BINARY_NAME: &str = "gh.exe";
/// Default executable filename searched on `PATH`.
#[cfg(not(windows))]
pub const GH_BINARY_NAME: &str = "gh";

/// Label added to GitHub issues while an agent run is active.
pub const AGENT_WORKING_LABEL: &str = "agent-working";

/// JSON fields requested from `gh issue list --json`.
const GH_ISSUE_JSON_FIELDS: &str =
    "number,title,body,labels,assignees,author,milestone,comments,createdAt,updatedAt,url,state";

/// Authentication strategy used when invoking `gh`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubAuth {
    /// Use the user's existing `gh` CLI login session.
    GitHubCli,
    /// Authenticate a single invocation with a per-project personal access token via `GH_TOKEN`.
    PersonalToken(String),
}

/// Errors returned while resolving `gh`, syncing GitHub issues, or updating an issue.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GitHubIntakeError {
    #[error(
        "GitHub CLI ('gh') was not found on PATH. Install it from https://cli.github.com and sign in with `gh auth login`."
    )]
    GhNotFoundOnPath,
    #[error("GitHub repository must be in 'owner/repo' format (got '{0}').")]
    InvalidRepoSlug(String),
    #[error(
        "No personal access token is saved for this project. Add a token or switch to GitHub CLI."
    )]
    MissingToken,
    #[error("GitHub authentication failed: {0}")]
    NotAuthenticated(String),
    #[error("GitHub CLI command `{command}` failed: {message}")]
    CommandFailed { command: String, message: String },
    #[error("Failed to parse GitHub CLI output: {0}")]
    ParseError(String),
}

/// Git remote metadata detected from a local repository checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedGitRemote {
    /// Display string such as `"origin · git@github.com:acme/api-gateway.git"`.
    pub remote_display: String,
    /// Parsed GitHub `"owner/repo"` slug when `origin` points at `github.com`.
    pub github_repo: Option<String>,
}

/// Supported GitHub issue search qualifiers that can be composed into project intake filters
/// and per-rule workflow routing conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitHubFilterQualifier {
    #[default]
    Label,
    Assignee,
    Author,
    Milestone,
    Mentions,
    Involves,
    Commenter,
    IssueType,
    No,
    InTitle,
    InBody,
    Text,
    Comments,
    Created,
    Updated,
    Linked,
    Project,
    RawQuery,
}

impl GitHubFilterQualifier {
    pub const ALL: &'static [Self] = &[
        Self::Label,
        Self::Assignee,
        Self::Author,
        Self::Milestone,
        Self::Mentions,
        Self::Involves,
        Self::Commenter,
        Self::IssueType,
        Self::No,
        Self::InTitle,
        Self::InBody,
        Self::Text,
        Self::Comments,
        Self::Created,
        Self::Updated,
        Self::Linked,
        Self::Project,
        Self::RawQuery,
    ];

    pub fn value(self) -> &'static str {
        match self {
            Self::Label => "label",
            Self::Assignee => "assignee",
            Self::Author => "author",
            Self::Milestone => "milestone",
            Self::Mentions => "mentions",
            Self::Involves => "involves",
            Self::Commenter => "commenter",
            Self::IssueType => "issue_type",
            Self::No => "no",
            Self::InTitle => "in_title",
            Self::InBody => "in_body",
            Self::Text => "text",
            Self::Comments => "comments",
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Linked => "linked",
            Self::Project => "project",
            Self::RawQuery => "raw_query",
        }
    }

    pub fn from_value(raw: &str) -> Self {
        match raw {
            "assignee" => Self::Assignee,
            "author" => Self::Author,
            "milestone" => Self::Milestone,
            "mentions" => Self::Mentions,
            "involves" => Self::Involves,
            "commenter" => Self::Commenter,
            "issue_type" | "type" => Self::IssueType,
            "no" => Self::No,
            "in_title" => Self::InTitle,
            "in_body" => Self::InBody,
            "text" => Self::Text,
            "comments" => Self::Comments,
            "created" => Self::Created,
            "updated" => Self::Updated,
            "linked" => Self::Linked,
            "project" => Self::Project,
            "raw_query" | "raw" => Self::RawQuery,
            _ => Self::Label,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Label => "Has label",
            Self::Assignee => "Assigned to",
            Self::Author => "Author",
            Self::Milestone => "Milestone",
            Self::Mentions => "Mentions",
            Self::Involves => "Involves",
            Self::Commenter => "Commenter",
            Self::IssueType => "Issue type",
            Self::No => "Missing (no:)",
            Self::InTitle => "Title contains",
            Self::InBody => "Body contains",
            Self::Text => "Text contains",
            Self::Comments => "Comments",
            Self::Created => "Created date",
            Self::Updated => "Updated date",
            Self::Linked => "Linked",
            Self::Project => "Project",
            Self::RawQuery => "GitHub search query",
        }
    }

    pub fn field_label(self) -> &'static str {
        match self {
            Self::Label => "Issue label",
            Self::Assignee => "GitHub assignee",
            Self::Author => "Author username",
            Self::Milestone => "Milestone title",
            Self::Mentions => "Mentioned username",
            Self::Involves => "Involved username",
            Self::Commenter => "Commenter username",
            Self::IssueType => "Issue type",
            Self::No => "Missing field",
            Self::InTitle => "Title text",
            Self::InBody => "Body text",
            Self::Text => "Search text",
            Self::Comments => "Comment count",
            Self::Created => "Created filter",
            Self::Updated => "Updated filter",
            Self::Linked => "Linked resource",
            Self::Project => "Project slug",
            Self::RawQuery => "GitHub search expression",
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            Self::Label => "e.g. security",
            Self::Assignee => "e.g. @octocat or @me",
            Self::Author => "e.g. @dependabot[bot]",
            Self::Milestone => "e.g. v2.0",
            Self::Mentions => "e.g. @octocat",
            Self::Involves => "e.g. @octocat",
            Self::Commenter => "e.g. @octocat",
            Self::IssueType => "e.g. Bug",
            Self::No => "assignee, label, milestone, project",
            Self::InTitle => "e.g. timeout",
            Self::InBody => "e.g. stack trace",
            Self::Text => "e.g. rate limit",
            Self::Comments => "e.g. >2, 0, 1..5",
            Self::Created => "e.g. >=2026-01-01",
            Self::Updated => "e.g. >2026-11-01",
            Self::Linked => "e.g. pr",
            Self::Project => "e.g. acme/1",
            Self::RawQuery => "e.g. -label:wontfix reactions:>3",
        }
    }

    pub fn supports_negation(self) -> bool {
        !matches!(self, Self::No | Self::RawQuery)
    }
}

/// Normalizes a GitHub username/handle by trimming whitespace and stripping a leading `@`
/// (preserving the special GitHub keyword `@me` and wildcard `*`).
pub fn normalize_assignee_handle(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("@me") {
        "@me".to_owned()
    } else if trimmed == "*" {
        "*".to_owned()
    } else {
        trimmed.trim_start_matches('@').trim().to_owned()
    }
}

/// One structured GitHub issue search filter clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubFilterClause {
    pub qualifier: GitHubFilterQualifier,
    pub value: String,
    #[serde(default)]
    pub negated: bool,
}

impl GitHubFilterClause {
    pub fn new(qualifier: GitHubFilterQualifier, value: impl Into<String>) -> Self {
        Self::with_negation(qualifier, value, false)
    }

    pub fn negated(qualifier: GitHubFilterQualifier, value: impl Into<String>) -> Self {
        Self::with_negation(qualifier, value, true)
    }

    pub fn with_negation(
        qualifier: GitHubFilterQualifier,
        value: impl Into<String>,
        negated: bool,
    ) -> Self {
        let raw = value.into();
        let normalized_value = match qualifier {
            GitHubFilterQualifier::Assignee
            | GitHubFilterQualifier::Author
            | GitHubFilterQualifier::Mentions
            | GitHubFilterQualifier::Involves
            | GitHubFilterQualifier::Commenter => normalize_assignee_handle(&raw),
            _ => raw.trim().to_owned(),
        };
        Self {
            qualifier,
            value: normalized_value,
            negated: negated && qualifier.supports_negation(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.value.trim().is_empty()
    }

    /// Canonical key used for case-insensitive duplicate detection.
    pub fn dedupe_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.qualifier.value(),
            self.negated,
            self.value.trim().to_ascii_lowercase()
        )
    }

    /// Formats this clause into GitHub issue search query syntax (for `--search` and query preview).
    pub fn to_github_query_token(&self) -> String {
        let val = self.value.trim();
        if val.is_empty() {
            return String::new();
        }
        let neg = if self.negated && self.qualifier.supports_negation() {
            "-"
        } else {
            ""
        };
        match self.qualifier {
            GitHubFilterQualifier::Label => format!("{neg}label:{}", quote_gh_value(val)),
            GitHubFilterQualifier::Assignee => format!("{neg}assignee:{}", quote_gh_value(val)),
            GitHubFilterQualifier::Author => format!("{neg}author:{}", quote_gh_value(val)),
            GitHubFilterQualifier::Milestone => format!("{neg}milestone:{}", quote_gh_value(val)),
            GitHubFilterQualifier::Mentions => format!("{neg}mentions:{}", quote_gh_value(val)),
            GitHubFilterQualifier::Involves => format!("{neg}involves:{}", quote_gh_value(val)),
            GitHubFilterQualifier::Commenter => format!("{neg}commenter:{}", quote_gh_value(val)),
            GitHubFilterQualifier::IssueType => format!("{neg}type:{}", quote_gh_value(val)),
            GitHubFilterQualifier::No => format!("no:{}", quote_gh_value(val)),
            GitHubFilterQualifier::InTitle => format!("{neg}{} in:title", quote_gh_value(val)),
            GitHubFilterQualifier::InBody => format!("{neg}{} in:body", quote_gh_value(val)),
            GitHubFilterQualifier::Text => format!("{neg}{}", quote_gh_value(val)),
            GitHubFilterQualifier::Comments => format!("{neg}comments:{val}"),
            GitHubFilterQualifier::Created => format!("{neg}created:{val}"),
            GitHubFilterQualifier::Updated => format!("{neg}updated:{val}"),
            GitHubFilterQualifier::Linked => format!("{neg}linked:{}", quote_gh_value(val)),
            GitHubFilterQualifier::Project => format!("{neg}project:{}", quote_gh_value(val)),
            GitHubFilterQualifier::RawQuery => val.to_owned(),
        }
    }

    /// Human-readable description shown in intake rule tables and summaries.
    pub fn human_summary(&self) -> String {
        let val = self.value.trim();
        let handle = format_handle_display(val);
        match (self.qualifier, self.negated) {
            (GitHubFilterQualifier::Label, false) => format!("has label {val}"),
            (GitHubFilterQualifier::Label, true) => format!("excludes label {val}"),
            (GitHubFilterQualifier::Assignee, false) if val == "*" => "has any assignee".to_owned(),
            (GitHubFilterQualifier::Assignee, false) => format!("assigned to {handle}"),
            (GitHubFilterQualifier::Assignee, true) => format!("not assigned to {handle}"),
            (GitHubFilterQualifier::Author, false) => format!("authored by {handle}"),
            (GitHubFilterQualifier::Author, true) => format!("not authored by {handle}"),
            (GitHubFilterQualifier::Milestone, false) if val == "*" => {
                "has any milestone".to_owned()
            }
            (GitHubFilterQualifier::Milestone, false) => format!("milestone {val}"),
            (GitHubFilterQualifier::Milestone, true) => format!("not in milestone {val}"),
            (GitHubFilterQualifier::Mentions, false) => format!("mentions {handle}"),
            (GitHubFilterQualifier::Mentions, true) => format!("does not mention {handle}"),
            (GitHubFilterQualifier::Involves, false) => format!("involves {handle}"),
            (GitHubFilterQualifier::Involves, true) => format!("does not involve {handle}"),
            (GitHubFilterQualifier::Commenter, false) => format!("commented by {handle}"),
            (GitHubFilterQualifier::Commenter, true) => format!("not commented by {handle}"),
            (GitHubFilterQualifier::IssueType, false) => format!("type {val}"),
            (GitHubFilterQualifier::IssueType, true) => format!("not type {val}"),
            (GitHubFilterQualifier::No, _) => format!("has no {val}"),
            (GitHubFilterQualifier::InTitle, false) => format!("title contains \"{val}\""),
            (GitHubFilterQualifier::InTitle, true) => format!("title excludes \"{val}\""),
            (GitHubFilterQualifier::InBody, false) => format!("body contains \"{val}\""),
            (GitHubFilterQualifier::InBody, true) => format!("body excludes \"{val}\""),
            (GitHubFilterQualifier::Text, false) => format!("contains \"{val}\""),
            (GitHubFilterQualifier::Text, true) => format!("excludes \"{val}\""),
            (GitHubFilterQualifier::Comments, false) => format!("comments {val}"),
            (GitHubFilterQualifier::Comments, true) => format!("comments not {val}"),
            (GitHubFilterQualifier::Created, false) => format!("created {val}"),
            (GitHubFilterQualifier::Created, true) => format!("not created {val}"),
            (GitHubFilterQualifier::Updated, false) => format!("updated {val}"),
            (GitHubFilterQualifier::Updated, true) => format!("not updated {val}"),
            (GitHubFilterQualifier::Linked, false) => format!("linked {val}"),
            (GitHubFilterQualifier::Linked, true) => format!("not linked {val}"),
            (GitHubFilterQualifier::Project, false) => format!("in project {val}"),
            (GitHubFilterQualifier::Project, true) => format!("not in project {val}"),
            (GitHubFilterQualifier::RawQuery, _) => val.to_owned(),
        }
    }

    /// Short explanation displayed in the Unmatched Issues drawer when an issue fails this clause.
    pub fn mismatch_reason(&self) -> String {
        let val = self.value.trim();
        let handle = format_handle_display(val);
        match (self.qualifier, self.negated) {
            (GitHubFilterQualifier::Label, false) => format!("Missing label {val}"),
            (GitHubFilterQualifier::Label, true) => format!("Has excluded label {val}"),
            (GitHubFilterQualifier::Assignee, false) if val == "*" => "Unassigned".to_owned(),
            (GitHubFilterQualifier::Assignee, false) => format!("Not assigned to {handle}"),
            (GitHubFilterQualifier::Assignee, true) => format!("Assigned to excluded {handle}"),
            (GitHubFilterQualifier::Author, false) => format!("Not authored by {handle}"),
            (GitHubFilterQualifier::Author, true) => format!("Authored by excluded {handle}"),
            (GitHubFilterQualifier::Milestone, false) => format!("Not in milestone {val}"),
            (GitHubFilterQualifier::Milestone, true) => format!("In excluded milestone {val}"),
            (GitHubFilterQualifier::No, _) => format!("Already has {val}"),
            (GitHubFilterQualifier::InTitle, false) => format!("Title missing \"{val}\""),
            (GitHubFilterQualifier::InTitle, true) => format!("Title contains \"{val}\""),
            (GitHubFilterQualifier::InBody, false) => format!("Body missing \"{val}\""),
            (GitHubFilterQualifier::InBody, true) => format!("Body contains \"{val}\""),
            (GitHubFilterQualifier::Text, false) => format!("Missing text \"{val}\""),
            (GitHubFilterQualifier::Text, true) => format!("Contains excluded \"{val}\""),
            _ => format!("Does not match {}", self.to_github_query_token()),
        }
    }

    /// Returns `true` when accurate evaluation requires GitHub's `--search` engine
    /// (for example `@me`, `project:`, `linked:`, `involves:`, `commenter:`, `type:`,
    /// or a raw query containing non-local qualifiers).
    pub fn requires_server_search(&self) -> bool {
        let val = self.value.trim();
        if val.is_empty() {
            return false;
        }
        match self.qualifier {
            GitHubFilterQualifier::Label
            | GitHubFilterQualifier::Milestone
            | GitHubFilterQualifier::InTitle
            | GitHubFilterQualifier::InBody
            | GitHubFilterQualifier::Text
            | GitHubFilterQualifier::Comments
            | GitHubFilterQualifier::Created
            | GitHubFilterQualifier::Updated => false,
            GitHubFilterQualifier::Assignee | GitHubFilterQualifier::Author => {
                val.eq_ignore_ascii_case("@me")
            }
            GitHubFilterQualifier::No => !matches!(
                val.to_ascii_lowercase().as_str(),
                "label" | "assignee" | "milestone"
            ),
            GitHubFilterQualifier::Mentions
            | GitHubFilterQualifier::Involves
            | GitHubFilterQualifier::Commenter
            | GitHubFilterQualifier::IssueType
            | GitHubFilterQualifier::Linked
            | GitHubFilterQualifier::Project => true,
            GitHubFilterQualifier::RawQuery => {
                let parsed = parse_raw_query_clauses(val);
                parsed.is_empty() || parsed.iter().any(Self::requires_server_search)
            }
        }
    }

    /// Evaluates this clause locally against a fetched [`GitHubIssue`].
    ///
    /// Returns `Some(true/false)` when deterministically evaluable from the issue's fields,
    /// or `None` when GitHub's server-side search engine is required.
    pub fn matches_locally(&self, issue: &GitHubIssue) -> Option<bool> {
        let val = self.value.trim();
        if val.is_empty() {
            return Some(true);
        }

        let positive = match self.qualifier {
            GitHubFilterQualifier::Label => issue
                .labels
                .iter()
                .any(|label| label.eq_ignore_ascii_case(val)),
            GitHubFilterQualifier::Assignee => {
                if val.eq_ignore_ascii_case("@me") {
                    return None;
                }
                if val == "*" {
                    !issue.assignees.is_empty()
                } else {
                    let target = normalize_assignee_handle(val);
                    issue
                        .assignees
                        .iter()
                        .any(|a| normalize_assignee_handle(a).eq_ignore_ascii_case(&target))
                }
            }
            GitHubFilterQualifier::Author => {
                if val.eq_ignore_ascii_case("@me") {
                    return None;
                }
                let target = normalize_assignee_handle(val);
                issue
                    .author
                    .as_deref()
                    .is_some_and(|a| normalize_assignee_handle(a).eq_ignore_ascii_case(&target))
            }
            GitHubFilterQualifier::Milestone => {
                if val == "*" {
                    issue.milestone.is_some()
                } else {
                    issue
                        .milestone
                        .as_deref()
                        .is_some_and(|m| m.eq_ignore_ascii_case(val))
                }
            }
            GitHubFilterQualifier::No => match val.to_ascii_lowercase().as_str() {
                "label" => return Some(issue.labels.is_empty()),
                "assignee" => return Some(issue.assignees.is_empty()),
                "milestone" => return Some(issue.milestone.is_none()),
                _ => return None,
            },
            GitHubFilterQualifier::InTitle => issue
                .title
                .to_ascii_lowercase()
                .contains(&val.to_ascii_lowercase()),
            GitHubFilterQualifier::InBody => issue
                .body
                .to_ascii_lowercase()
                .contains(&val.to_ascii_lowercase()),
            GitHubFilterQualifier::Text => {
                let needle = val.to_ascii_lowercase();
                issue.title.to_ascii_lowercase().contains(&needle)
                    || issue.body.to_ascii_lowercase().contains(&needle)
            }
            GitHubFilterQualifier::Comments => matches_numeric_expr(issue.comments_count, val)?,
            GitHubFilterQualifier::Created => {
                let date = issue.created_at.as_deref()?;
                matches_date_expr(date, val)?
            }
            GitHubFilterQualifier::Updated => {
                let date = issue.updated_at.as_deref()?;
                matches_date_expr(date, val)?
            }
            GitHubFilterQualifier::Mentions => {
                let mention = format!("@{}", normalize_assignee_handle(val).to_ascii_lowercase());
                if issue.body.to_ascii_lowercase().contains(&mention) {
                    true
                } else {
                    return None;
                }
            }
            GitHubFilterQualifier::RawQuery => {
                let parsed = parse_raw_query_clauses(val);
                if parsed.is_empty() {
                    return None;
                }
                let mut unknown = false;
                for sub in parsed {
                    match sub.matches_locally(issue) {
                        Some(false) => return Some(false),
                        Some(true) => {}
                        None => unknown = true,
                    }
                }
                return if unknown { None } else { Some(true) };
            }
            GitHubFilterQualifier::Involves
            | GitHubFilterQualifier::Commenter
            | GitHubFilterQualifier::IssueType
            | GitHubFilterQualifier::Linked
            | GitHubFilterQualifier::Project => return None,
        };

        Some(if self.negated { !positive } else { positive })
    }
}

/// Builds a GitHub issue search query string from an ordered list of filter clauses.
pub fn build_github_search_query(clauses: &[GitHubFilterClause]) -> String {
    clauses
        .iter()
        .map(GitHubFilterClause::to_github_query_token)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_gh_value(value: &str) -> String {
    if value.chars().any(char::is_whitespace) {
        let escaped = value.replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        value.to_owned()
    }
}

fn format_handle_display(val: &str) -> String {
    if val.eq_ignore_ascii_case("@me") {
        "@me".to_owned()
    } else if val == "*" {
        "*".to_owned()
    } else {
        format!("@{}", val.trim_start_matches('@'))
    }
}

fn matches_numeric_expr(actual: u32, expr: &str) -> Option<bool> {
    let trimmed = expr.trim();
    if let Some((lo_str, hi_str)) = trimmed.split_once("..") {
        let lo_ok = if lo_str.trim().is_empty() || lo_str.trim() == "*" {
            true
        } else {
            actual >= lo_str.trim().parse::<u32>().ok()?
        };
        let hi_ok = if hi_str.trim().is_empty() || hi_str.trim() == "*" {
            true
        } else {
            actual <= hi_str.trim().parse::<u32>().ok()?
        };
        return Some(lo_ok && hi_ok);
    }
    if let Some(rest) = trimmed.strip_prefix(">=") {
        return Some(actual >= rest.trim().parse::<u32>().ok()?);
    }
    if let Some(rest) = trimmed.strip_prefix("<=") {
        return Some(actual <= rest.trim().parse::<u32>().ok()?);
    }
    if let Some(rest) = trimmed.strip_prefix('>') {
        return Some(actual > rest.trim().parse::<u32>().ok()?);
    }
    if let Some(rest) = trimmed.strip_prefix('<') {
        return Some(actual < rest.trim().parse::<u32>().ok()?);
    }
    Some(actual == trimmed.parse::<u32>().ok()?)
}

fn matches_date_expr(iso_timestamp: &str, expr: &str) -> Option<bool> {
    let day = iso_timestamp.get(..10)?;
    let trimmed = expr.trim();
    if let Some((lo, hi)) = trimmed.split_once("..") {
        let lo_ok = lo.trim().is_empty() || lo.trim() == "*" || day >= lo.trim();
        let hi_ok = hi.trim().is_empty() || hi.trim() == "*" || day <= hi.trim();
        return Some(lo_ok && hi_ok);
    }
    if let Some(rest) = trimmed.strip_prefix(">=") {
        return Some(day >= rest.trim());
    }
    if let Some(rest) = trimmed.strip_prefix("<=") {
        return Some(day <= rest.trim());
    }
    if let Some(rest) = trimmed.strip_prefix('>') {
        return Some(day > rest.trim());
    }
    if let Some(rest) = trimmed.strip_prefix('<') {
        return Some(day < rest.trim());
    }
    Some(day == trimmed)
}

/// Parses space-delimited GitHub search tokens (respecting quoted values) into structured clauses
/// so standard qualifiers inside a `RawQuery` can also be evaluated locally.
fn parse_raw_query_clauses(raw: &str) -> Vec<GitHubFilterClause> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in raw.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    let mut clauses = Vec::new();
    for token in tokens {
        let (negated, rest) = if let Some(stripped) = token.strip_prefix('-') {
            (true, stripped)
        } else {
            (false, token.as_str())
        };
        if let Some((key, val)) = rest.split_once(':') {
            let key_lower = key.to_ascii_lowercase();
            let qualifier = match key_lower.as_str() {
                "label" => Some(GitHubFilterQualifier::Label),
                "assignee" => Some(GitHubFilterQualifier::Assignee),
                "author" => Some(GitHubFilterQualifier::Author),
                "milestone" => Some(GitHubFilterQualifier::Milestone),
                "mentions" => Some(GitHubFilterQualifier::Mentions),
                "involves" => Some(GitHubFilterQualifier::Involves),
                "commenter" => Some(GitHubFilterQualifier::Commenter),
                "type" => Some(GitHubFilterQualifier::IssueType),
                "no" => Some(GitHubFilterQualifier::No),
                "comments" => Some(GitHubFilterQualifier::Comments),
                "created" => Some(GitHubFilterQualifier::Created),
                "updated" => Some(GitHubFilterQualifier::Updated),
                "linked" => Some(GitHubFilterQualifier::Linked),
                "project" => Some(GitHubFilterQualifier::Project),
                "is" if val.eq_ignore_ascii_case("open") || val.eq_ignore_ascii_case("issue") => {
                    continue;
                }
                _ => None,
            };
            if let Some(q) = qualifier {
                clauses.push(GitHubFilterClause::with_negation(q, val, negated));
            } else {
                // Unknown server-side qualifier (e.g. `reactions:>3`, `sort:updated-desc`).
                return Vec::new();
            }
        } else {
            clauses.push(GitHubFilterClause::with_negation(
                GitHubFilterQualifier::Text,
                rest,
                negated,
            ));
        }
    }
    clauses
}

/// One open issue returned by `gh issue list --json ...`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubIssue {
    pub number: u32,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub milestone: Option<String>,
    #[serde(default)]
    pub comments_count: u32,
    #[serde(default)]
    pub created_at: Option<String>,
    pub url: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// Snapshot produced by syncing open issues from a GitHub repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubSyncSnapshot {
    pub repo: String,
    pub open_issues: Vec<GitHubIssue>,
    pub synced_at_unix: i64,
    pub search_query: String,
    pub server_filtered: bool,
}

/// Normalizes and validates a GitHub repository reference into `"owner/repo"`.
///
/// Accepts plain `"owner/repo"` slugs as well as `https://github.com/owner/repo(.git)`
/// and `git@github.com:owner/repo(.git)` URLs.
pub fn normalize_github_repo_slug(raw: &str) -> Result<String, GitHubIntakeError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(GitHubIntakeError::InvalidRepoSlug(String::new()));
    }

    if let Some(parsed) = parse_github_repo_slug(trimmed) {
        return Ok(parsed);
    }

    let stripped = trimmed
        .trim_matches('/')
        .strip_suffix(".git")
        .unwrap_or(trimmed.trim_matches('/'));
    let mut parts = stripped.split('/');
    let Some(owner) = parts.next() else {
        return Err(GitHubIntakeError::InvalidRepoSlug(trimmed.to_owned()));
    };
    let Some(repo) = parts.next() else {
        return Err(GitHubIntakeError::InvalidRepoSlug(trimmed.to_owned()));
    };
    if parts.next().is_some() || !is_valid_slug_segment(owner) || !is_valid_slug_segment(repo) {
        return Err(GitHubIntakeError::InvalidRepoSlug(trimmed.to_owned()));
    }

    Ok(format!("{owner}/{repo}"))
}

/// Parses a GitHub `"owner/repo"` slug from an HTTPS or SSH `github.com` remote URL.
pub fn parse_github_repo_slug(remote_url: &str) -> Option<String> {
    let trimmed = remote_url.trim();
    let rest = trimmed
        .strip_prefix("git@github.com:")
        .or_else(|| trimmed.strip_prefix("ssh://git@github.com/"))
        .or_else(|| trimmed.strip_prefix("https://github.com/"))
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .or_else(|| trimmed.strip_prefix("github.com/"))?;

    let cleaned = rest.trim_end_matches('/');
    let cleaned = cleaned.strip_suffix(".git").unwrap_or(cleaned);
    let mut parts = cleaned.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if parts.next().is_some() || !is_valid_slug_segment(owner) || !is_valid_slug_segment(repo) {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

fn is_valid_slug_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

/// Reads `origin`'s URL from a local git repository and extracts a GitHub `owner/repo` slug if present.
pub fn detect_git_remote_info(root_path: &Path) -> Option<DetectedGitRemote> {
    if !root_path.exists() {
        return None;
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(root_path)
        .args(["remote", "get-url", "origin"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if url.is_empty() {
        return None;
    }

    let github_repo = parse_github_repo_slug(&url);
    Some(DetectedGitRemote {
        remote_display: format!("origin · {url}"),
        github_repo,
    })
}

/// Resolves the `gh` executable from the provided `PATH`.
pub fn resolve_gh_binary(path_env: Option<&OsStr>) -> Result<PathBuf, GitHubIntakeError> {
    if let Some(paths) = path_env {
        for dir in env::split_paths(paths) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let candidate = dir.join(GH_BINARY_NAME);
            if is_executable_file(&candidate) {
                return Ok(candidate);
            }
        }
    }

    Err(GitHubIntakeError::GhNotFoundOnPath)
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn build_gh_command(binary: &Path, auth: &GitHubAuth) -> Result<Command, GitHubIntakeError> {
    let mut cmd = Command::new(binary);
    cmd.env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    match auth {
        GitHubAuth::GitHubCli => {}
        GitHubAuth::PersonalToken(token) => {
            let trimmed = token.trim();
            if trimmed.is_empty() {
                return Err(GitHubIntakeError::MissingToken);
            }
            cmd.env("GH_TOKEN", trimmed);
        }
    }

    Ok(cmd)
}

fn classify_gh_failure(command: &str, stderr: &str, stdout: &str) -> GitHubIntakeError {
    let raw = if !stderr.trim().is_empty() {
        stderr.trim()
    } else if !stdout.trim().is_empty() {
        stdout.trim()
    } else {
        "command exited with a non-zero status"
    };
    let compact = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    let lower = compact.to_ascii_lowercase();

    if lower.contains("gh auth login")
        || lower.contains("not logged into")
        || lower.contains("http 401")
        || lower.contains("bad credentials")
        || lower.contains("authentication")
        || lower.contains("gh_token")
    {
        GitHubIntakeError::NotAuthenticated(compact)
    } else {
        GitHubIntakeError::CommandFailed {
            command: command.to_owned(),
            message: compact,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawGhLabel {
    Named { name: String },
    Plain(String),
}

impl RawGhLabel {
    fn into_name(self) -> String {
        match self {
            Self::Named { name } => name,
            Self::Plain(name) => name,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawGhUser {
    Actor { login: String },
    Plain(String),
}

impl RawGhUser {
    fn into_login(self) -> String {
        let raw = match self {
            Self::Actor { login } => login,
            Self::Plain(login) => login,
        };
        normalize_assignee_handle(&raw)
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawGhMilestone {
    Object { title: String },
    Plain(String),
}

impl RawGhMilestone {
    fn into_title(self) -> String {
        match self {
            Self::Object { title } => title.trim().to_owned(),
            Self::Plain(title) => title.trim().to_owned(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawGhComments {
    List(Vec<serde_json::Value>),
    Count(u32),
}

impl RawGhComments {
    fn count(self) -> u32 {
        match self {
            Self::List(items) => items.len() as u32,
            Self::Count(n) => n,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawGhIssue {
    number: u32,
    title: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    labels: Vec<RawGhLabel>,
    #[serde(default)]
    assignees: Vec<RawGhUser>,
    #[serde(default)]
    author: Option<RawGhUser>,
    #[serde(default)]
    milestone: Option<RawGhMilestone>,
    #[serde(default)]
    comments: Option<RawGhComments>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    updated_at: Option<String>,
}

/// Parses the JSON array output of `gh issue list --json ...`.
pub fn parse_github_issues_json(
    repo: &str,
    raw_json: &str,
) -> Result<Vec<GitHubIssue>, GitHubIntakeError> {
    let raw_items: Vec<RawGhIssue> = serde_json::from_str(raw_json)
        .map_err(|err| GitHubIntakeError::ParseError(err.to_string()))?;

    Ok(raw_items
        .into_iter()
        .map(|item| {
            let url = if item.url.trim().is_empty() {
                format!("https://github.com/{repo}/issues/{}", item.number)
            } else {
                item.url
            };
            let labels = item
                .labels
                .into_iter()
                .map(RawGhLabel::into_name)
                .map(|l| l.trim().to_owned())
                .filter(|l| !l.is_empty())
                .collect();
            let assignees = item
                .assignees
                .into_iter()
                .map(RawGhUser::into_login)
                .filter(|a| !a.is_empty())
                .collect();
            let author = item
                .author
                .map(RawGhUser::into_login)
                .filter(|a| !a.is_empty());
            let milestone = item
                .milestone
                .map(RawGhMilestone::into_title)
                .filter(|m| !m.is_empty());
            let comments_count = item.comments.map(RawGhComments::count).unwrap_or(0);
            GitHubIssue {
                number: item.number,
                title: item.title.trim().to_owned(),
                body: item.body.unwrap_or_default().trim().to_owned(),
                labels,
                assignees,
                author,
                milestone,
                comments_count,
                created_at: item.created_at,
                url,
                state: if item.state.trim().is_empty() {
                    "OPEN".to_owned()
                } else {
                    item.state
                },
                updated_at: item.updated_at,
            }
        })
        .collect())
}

/// Fetches open issues from a GitHub repository via `gh issue list`.
pub fn fetch_github_open_issues(
    repo: &str,
    auth: &GitHubAuth,
    path_env: Option<&OsStr>,
) -> Result<GitHubSyncSnapshot, GitHubIntakeError> {
    fetch_github_open_issues_with_filter(repo, &[], auth, path_env)
}

/// Fetches open issues from a GitHub repository via `gh issue list`, passing `--search <query>`
/// to GitHub when any clause in `filter_clauses` requires server-side evaluation.
pub fn fetch_github_open_issues_with_filter(
    repo: &str,
    filter_clauses: &[GitHubFilterClause],
    auth: &GitHubAuth,
    path_env: Option<&OsStr>,
) -> Result<GitHubSyncSnapshot, GitHubIntakeError> {
    let normalized_repo = normalize_github_repo_slug(repo)?;
    let gh = resolve_gh_binary(path_env)?;
    let mut cmd = build_gh_command(&gh, auth)?;
    let search_query = build_github_search_query(filter_clauses);
    let server_filtered = !search_query.is_empty()
        && filter_clauses
            .iter()
            .any(GitHubFilterClause::requires_server_search);

    cmd.args([
        "issue",
        "list",
        "--repo",
        &normalized_repo,
        "--state",
        "open",
        "--limit",
        "100",
    ]);
    if server_filtered {
        cmd.args(["--search", &search_query]);
    }
    cmd.args(["--json", GH_ISSUE_JSON_FIELDS]);

    let output = cmd
        .output()
        .map_err(|err| GitHubIntakeError::CommandFailed {
            command: "gh issue list".to_owned(),
            message: err.to_string(),
        })?;

    if !output.status.success() {
        return Err(classify_gh_failure(
            "gh issue list",
            &String::from_utf8_lossy(&output.stderr),
            &String::from_utf8_lossy(&output.stdout),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let open_issues = parse_github_issues_json(&normalized_repo, &stdout)?;
    Ok(GitHubSyncSnapshot {
        repo: normalized_repo,
        open_issues,
        synced_at_unix: current_unix_seconds(),
        search_query,
        server_filtered,
    })
}

/// Adds a label to a GitHub issue via `gh issue edit --add-label`.
pub fn add_github_issue_label(
    repo: &str,
    issue_number: u32,
    label: &str,
    auth: &GitHubAuth,
    path_env: Option<&OsStr>,
) -> Result<(), GitHubIntakeError> {
    let normalized_repo = normalize_github_repo_slug(repo)?;
    let gh = resolve_gh_binary(path_env)?;
    let mut cmd = build_gh_command(&gh, auth)?;
    cmd.args([
        "issue",
        "edit",
        &issue_number.to_string(),
        "--repo",
        &normalized_repo,
        "--add-label",
        label.trim(),
    ]);

    let output = cmd
        .output()
        .map_err(|err| GitHubIntakeError::CommandFailed {
            command: "gh issue edit".to_owned(),
            message: err.to_string(),
        })?;

    if !output.status.success() {
        return Err(classify_gh_failure(
            "gh issue edit",
            &String::from_utf8_lossy(&output.stderr),
            &String::from_utf8_lossy(&output.stdout),
        ));
    }
    Ok(())
}

/// Removes a label from a GitHub issue via `gh issue edit --remove-label`.
pub fn remove_github_issue_label(
    repo: &str,
    issue_number: u32,
    label: &str,
    auth: &GitHubAuth,
    path_env: Option<&OsStr>,
) -> Result<(), GitHubIntakeError> {
    let normalized_repo = normalize_github_repo_slug(repo)?;
    let gh = resolve_gh_binary(path_env)?;
    let mut cmd = build_gh_command(&gh, auth)?;
    cmd.args([
        "issue",
        "edit",
        &issue_number.to_string(),
        "--repo",
        &normalized_repo,
        "--remove-label",
        label.trim(),
    ]);

    let output = cmd
        .output()
        .map_err(|err| GitHubIntakeError::CommandFailed {
            command: "gh issue edit".to_owned(),
            message: err.to_string(),
        })?;

    if !output.status.success() {
        return Err(classify_gh_failure(
            "gh issue edit",
            &String::from_utf8_lossy(&output.stderr),
            &String::from_utf8_lossy(&output.stdout),
        ));
    }
    Ok(())
}

/// Posts a comment on a GitHub issue via `gh issue comment`.
pub fn comment_on_github_issue(
    repo: &str,
    issue_number: u32,
    body: &str,
    auth: &GitHubAuth,
    path_env: Option<&OsStr>,
) -> Result<(), GitHubIntakeError> {
    let normalized_repo = normalize_github_repo_slug(repo)?;
    let gh = resolve_gh_binary(path_env)?;
    let mut cmd = build_gh_command(&gh, auth)?;
    cmd.args([
        "issue",
        "comment",
        &issue_number.to_string(),
        "--repo",
        &normalized_repo,
        "--body",
        body,
    ]);

    let output = cmd
        .output()
        .map_err(|err| GitHubIntakeError::CommandFailed {
            command: "gh issue comment".to_owned(),
            message: err.to_string(),
        })?;

    if !output.status.success() {
        return Err(classify_gh_failure(
            "gh issue comment",
            &String::from_utf8_lossy(&output.stderr),
            &String::from_utf8_lossy(&output.stdout),
        ));
    }
    Ok(())
}

/// Formats a relative sync label (e.g. `"synced just now"`, `"synced 3 min ago"`).
pub fn format_synced_relative(synced_at_unix: i64, now_unix: i64) -> String {
    let elapsed = (now_unix - synced_at_unix).max(0);
    if elapsed < 45 {
        "synced just now".to_owned()
    } else if elapsed < 3600 {
        let mins = (elapsed / 60).max(1);
        format!("synced {mins} min ago")
    } else if elapsed < 86_400 {
        let hours = elapsed / 3600;
        format!("synced {hours} hr ago")
    } else {
        let days = elapsed / 86_400;
        if days == 1 {
            "synced 1 day ago".to_owned()
        } else {
            format!("synced {days} days ago")
        }
    }
}

pub fn current_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_repo_slugs_and_github_urls() {
        assert_eq!(
            normalize_github_repo_slug("acme/api-gateway").unwrap(),
            "acme/api-gateway"
        );
        assert_eq!(
            normalize_github_repo_slug("https://github.com/acme/api-gateway.git").unwrap(),
            "acme/api-gateway"
        );
        assert_eq!(
            normalize_github_repo_slug("git@github.com:acme/api-gateway.git").unwrap(),
            "acme/api-gateway"
        );
        assert_eq!(
            normalize_github_repo_slug("ssh://git@github.com/acme/api-gateway/").unwrap(),
            "acme/api-gateway"
        );
        assert!(normalize_github_repo_slug("").is_err());
        assert!(normalize_github_repo_slug("just-a-repo").is_err());
        assert!(normalize_github_repo_slug("acme/repo/extra").is_err());
        assert!(parse_github_repo_slug("git@gitlab.com:acme/repo.git").is_none());
    }

    #[test]
    fn parses_gh_issue_list_json_with_assignees_author_milestone_and_comments() {
        let json = r#"[
            {
                "number": 42,
                "title": "Fix timeout on retry",
                "body": "Retries do not apply jitter.",
                "labels": [{"name": "agent-ready"}, {"name": "bug"}],
                "assignees": [{"login": "octocat"}, {"login": "alice"}],
                "author": {"login": "bob"},
                "milestone": {"title": "v2.0"},
                "comments": [{"id": "c1"}, {"id": "c2"}, {"id": "c3"}],
                "createdAt": "2026-11-20T09:00:00Z",
                "url": "https://github.com/acme/api/issues/42",
                "state": "OPEN",
                "updatedAt": "2026-11-22T12:00:00Z"
            },
            {
                "number": 43,
                "title": "Update README",
                "body": null,
                "labels": ["docs"],
                "assignees": ["@charlie"],
                "author": "dave",
                "milestone": null,
                "comments": 0,
                "url": "",
                "state": "OPEN"
            }
        ]"#;

        let issues = parse_github_issues_json("acme/api", json).unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].number, 42);
        assert_eq!(issues[0].labels, vec!["agent-ready", "bug"]);
        assert_eq!(issues[0].assignees, vec!["octocat", "alice"]);
        assert_eq!(issues[0].author.as_deref(), Some("bob"));
        assert_eq!(issues[0].milestone.as_deref(), Some("v2.0"));
        assert_eq!(issues[0].comments_count, 3);
        assert_eq!(
            issues[0].created_at.as_deref(),
            Some("2026-11-20T09:00:00Z")
        );
        assert_eq!(issues[1].assignees, vec!["charlie"]);
        assert_eq!(issues[1].author.as_deref(), Some("dave"));
        assert_eq!(issues[1].body, "");
        assert_eq!(issues[1].url, "https://github.com/acme/api/issues/43");
    }

    #[test]
    fn builds_github_search_queries_and_evaluates_filters_locally() {
        let clauses = vec![
            GitHubFilterClause::new(GitHubFilterQualifier::Label, "good first issue"),
            GitHubFilterClause::new(GitHubFilterQualifier::Assignee, "@octocat"),
            GitHubFilterClause::negated(GitHubFilterQualifier::Label, "blocked"),
            GitHubFilterClause::new(GitHubFilterQualifier::Milestone, "v2.0"),
            GitHubFilterClause::new(GitHubFilterQualifier::Comments, ">=2"),
            GitHubFilterClause::new(GitHubFilterQualifier::Created, ">2026-11-01"),
        ];
        assert_eq!(
            build_github_search_query(&clauses),
            r#"label:"good first issue" assignee:octocat -label:blocked milestone:v2.0 comments:>=2 created:>2026-11-01"#
        );

        let matching_issue = GitHubIssue {
            number: 1,
            title: "Retry timeout".to_owned(),
            body: "Needs jitter".to_owned(),
            labels: vec!["good first issue".to_owned(), "bug".to_owned()],
            assignees: vec!["octocat".to_owned()],
            author: Some("alice".to_owned()),
            milestone: Some("v2.0".to_owned()),
            comments_count: 3,
            created_at: Some("2026-11-15T10:00:00Z".to_owned()),
            url: "https://github.com/acme/api/issues/1".to_owned(),
            state: "OPEN".to_owned(),
            updated_at: Some("2026-11-22T10:00:00Z".to_owned()),
        };

        for clause in &clauses {
            assert_eq!(clause.matches_locally(&matching_issue), Some(true));
            assert!(!clause.requires_server_search());
        }

        // Negated assignee, `no:assignee`, and raw queries work cleanly.
        assert_eq!(
            GitHubFilterClause::negated(GitHubFilterQualifier::Assignee, "octocat")
                .matches_locally(&matching_issue),
            Some(false)
        );
        assert_eq!(
            GitHubFilterClause::new(GitHubFilterQualifier::No, "assignee")
                .matches_locally(&matching_issue),
            Some(false)
        );
        assert_eq!(
            GitHubFilterClause::new(
                GitHubFilterQualifier::RawQuery,
                "assignee:octocat -label:wontfix"
            )
            .matches_locally(&matching_issue),
            Some(true)
        );

        // Server-only qualifiers (`@me`, `linked:pr`, `reactions:>5`) trigger `--search`.
        let me_clause = GitHubFilterClause::new(GitHubFilterQualifier::Assignee, "@me");
        assert_eq!(me_clause.to_github_query_token(), "assignee:@me");
        assert!(me_clause.requires_server_search());
        assert_eq!(me_clause.matches_locally(&matching_issue), None);

        let raw_server =
            GitHubFilterClause::new(GitHubFilterQualifier::RawQuery, "linked:pr reactions:>5");
        assert!(raw_server.requires_server_search());
    }

    #[test]
    fn formats_relative_sync_timestamps() {
        assert_eq!(format_synced_relative(1000, 1010), "synced just now");
        assert_eq!(format_synced_relative(1000, 1065), "synced 1 min ago");
        assert_eq!(format_synced_relative(1000, 1300), "synced 5 min ago");
        assert_eq!(format_synced_relative(1000, 8200), "synced 2 hr ago");
        assert_eq!(format_synced_relative(1000, 90_000), "synced 1 day ago");
    }

    #[cfg(unix)]
    #[test]
    fn fetches_issues_and_passes_search_flag_for_server_side_filters() {
        use std::os::unix::fs::PermissionsExt;
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let log_file = temp.path().join("gh.log");
        let gh_path = bin_dir.join("gh");

        let script = format!(
            r#"#!/bin/sh
echo "TOKEN=${{GH_TOKEN:-}} ARGS=$*" >> "{}"
if [ "$1" = "issue" ] && [ "$2" = "list" ]; then
  if [ "${{GH_TOKEN:-}}" = "bad-token" ]; then
    echo "HTTP 401: Bad credentials" >&2
    exit 1
  fi
  printf '%s\n' '[{{"number":101,"title":"Handle 503 retries","body":"Add jitter","labels":[{{"name":"agent-ready"}},{{"name":"bug"}}],"assignees":[{{"login":"octocat"}}],"url":"https://github.com/acme/demo/issues/101","state":"OPEN"}}]'
  exit 0
fi
exit 0
"#,
            log_file.display()
        );
        fs::write(&gh_path, script).unwrap();
        fs::set_permissions(&gh_path, fs::Permissions::from_mode(0o755)).unwrap();

        let path_env = bin_dir.as_os_str();

        // 1. GitHub CLI auth with local-only filter omits `--search`.
        let snap = fetch_github_open_issues_with_filter(
            "acme/demo",
            &[GitHubFilterClause::new(
                GitHubFilterQualifier::Assignee,
                "octocat",
            )],
            &GitHubAuth::GitHubCli,
            Some(path_env),
        )
        .unwrap();
        assert_eq!(snap.repo, "acme/demo");
        assert!(!snap.server_filtered);
        assert_eq!(snap.open_issues[0].assignees, vec!["octocat"]);

        // 2. Server-side filter (`assignee:@me`) passes `--search assignee:@me`.
        let server_snap = fetch_github_open_issues_with_filter(
            "acme/demo",
            &[GitHubFilterClause::new(
                GitHubFilterQualifier::Assignee,
                "@me",
            )],
            &GitHubAuth::GitHubCli,
            Some(path_env),
        )
        .unwrap();
        assert!(server_snap.server_filtered);
        assert_eq!(server_snap.search_query, "assignee:@me");

        // 3. Personal token auth passes GH_TOKEN and surfaces 401 as NotAuthenticated.
        let err = fetch_github_open_issues(
            "acme/demo",
            &GitHubAuth::PersonalToken("bad-token".to_owned()),
            Some(path_env),
        )
        .unwrap_err();
        assert!(matches!(err, GitHubIntakeError::NotAuthenticated(_)));

        // 4. Label write-back passes expected arguments and GH_TOKEN.
        add_github_issue_label(
            "acme/demo",
            101,
            AGENT_WORKING_LABEL,
            &GitHubAuth::PersonalToken("ghp_good_1234".to_owned()),
            Some(path_env),
        )
        .unwrap();
        remove_github_issue_label(
            "acme/demo",
            101,
            AGENT_WORKING_LABEL,
            &GitHubAuth::GitHubCli,
            Some(path_env),
        )
        .unwrap();

        let log = fs::read_to_string(&log_file).unwrap();
        assert!(log.contains("--search assignee:@me"));
        assert!(log.contains(
            "TOKEN=ghp_good_1234 ARGS=issue edit 101 --repo acme/demo --add-label agent-working"
        ));
        assert!(log.contains("ARGS=issue edit 101 --repo acme/demo --remove-label agent-working"));
    }

    #[test]
    fn detects_github_origin_from_real_git_repo() {
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let init = Command::new("git")
            .arg("init")
            .arg(temp.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(init.success());

        let remote = Command::new("git")
            .arg("-C")
            .arg(temp.path())
            .args([
                "remote",
                "add",
                "origin",
                "git@github.com:octocat/hello-world.git",
            ])
            .status()
            .unwrap();
        assert!(remote.success());

        let detected = detect_git_remote_info(temp.path()).expect("should detect git origin");
        assert_eq!(
            detected.remote_display,
            "origin · git@github.com:octocat/hello-world.git"
        );
        assert_eq!(detected.github_repo.as_deref(), Some("octocat/hello-world"));
    }
}
