//! GitHub issue intake, `gh` CLI integration, and project work queue models.

pub mod github_intake;
pub mod work_queue;

pub use github_intake::{
    AGENT_WORKING_LABEL, DetectedGitRemote, GH_BINARY_NAME, GitHubAuth, GitHubFilterClause,
    GitHubFilterQualifier, GitHubIntakeError, GitHubIssue, GitHubSyncSnapshot,
    add_github_issue_label, build_github_search_query, comment_on_github_issue,
    current_unix_seconds, detect_git_remote_info, fetch_github_open_issues,
    fetch_github_open_issues_with_filter, format_synced_relative, normalize_assignee_handle,
    normalize_github_repo_slug, parse_github_issues_json, parse_github_repo_slug,
    remove_github_issue_label, resolve_gh_binary,
};
pub use work_queue::{
    GitHubIntakeDiff, IntakeRule, ProjectWorkQueue, ProjectWorkflowSummary, UnmatchedIssue,
    WorkQueueError, WorkQueueItem, WorkQueueStage, WorkQueueViewMode,
};
