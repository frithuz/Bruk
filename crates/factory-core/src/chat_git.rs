//! Git inspection, diff capture, commit, pull request, and checkout-apply actions for Chats.
//!
//! Chats can work either in the project's own checkout or in an isolated git worktree under
//! `chats/<chat-id>/worktree`. This module inspects both working-tree changes (staged, unstaged,
//! and untracked) and branch commits ahead of the base branch, and provides safe git operations
//! to commit changes, push & open a GitHub pull request, or apply worktree changes back to the
//! main project checkout.

use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    github_intake::{GitHubAuth, resolve_gh_binary},
    preferences::SSH_AUTH_SOCK_ENV,
    runner::parse_unified_diff,
    secrets::SecretRedactor,
    workflow_run::{DiffFileChange, DiffLine, DiffLineKind},
};

/// Maximum number of untracked files expanded into diff cards.
const MAX_UNTRACKED_FILES: usize = 64;
/// Maximum bytes read from an untracked file to generate its diff preview.
const MAX_UNTRACKED_PREVIEW_BYTES: usize = 64 * 1024;
/// Maximum preview lines shown for an untracked file (matches `parse_unified_diff`).
const MAX_DIFF_PREVIEW_LINES: usize = 120;
/// Maximum commits listed in the worktree/branch summary.
const MAX_COMMITS_LISTED: usize = 50;

/// Errors returned by chat git operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChatGitError {
    #[error("'{0}' does not exist yet")]
    DirectoryMissing(PathBuf),
    #[error("this directory is not inside a git repository")]
    NotAGitRepository,
    #[error("commit message cannot be empty")]
    EmptyCommitMessage,
    #[error("pull request title cannot be empty")]
    EmptyPrTitle,
    #[error("there are no uncommitted changes to commit")]
    NothingToCommit,
    #[error("there are no changes in this worktree to apply")]
    NothingToApply,
    #[error("apply to checkout is only available for worktree chats")]
    NotAWorktree,
    #[error("{0}")]
    CommandFailed(String),
}

/// One commit on the chat's branch ahead of its base branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatCommitSummary {
    pub short_sha: String,
    pub subject: String,
}

/// Snapshot of git state and diffs for a chat's working directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatGitStatus {
    pub working_dir: PathBuf,
    pub is_worktree: bool,
    pub branch: String,
    pub base_branch: String,
    pub has_remote: bool,
    pub has_upstream: bool,
    pub commits_ahead: usize,
    pub commits: Vec<ChatCommitSummary>,
    /// Uncommitted working-tree + index + untracked file diffs.
    pub uncommitted_diffs: Vec<DiffFileChange>,
    /// File paths in `uncommitted_diffs` that are newly created untracked files.
    pub untracked_paths: BTreeSet<String>,
    /// Diff of branch commits since the merge-base with `base_branch`.
    pub committed_diffs: Vec<DiffFileChange>,
}

impl ChatGitStatus {
    pub fn uncommitted_file_count(&self) -> usize {
        self.uncommitted_diffs.len()
    }

    pub fn uncommitted_additions(&self) -> usize {
        self.uncommitted_diffs.iter().map(|f| f.additions).sum()
    }

    pub fn uncommitted_deletions(&self) -> usize {
        self.uncommitted_diffs.iter().map(|f| f.deletions).sum()
    }

    pub fn committed_file_count(&self) -> usize {
        self.committed_diffs.len()
    }

    pub fn committed_additions(&self) -> usize {
        self.committed_diffs.iter().map(|f| f.additions).sum()
    }

    pub fn committed_deletions(&self) -> usize {
        self.committed_diffs.iter().map(|f| f.deletions).sum()
    }

    pub fn has_uncommitted_changes(&self) -> bool {
        !self.uncommitted_diffs.is_empty()
    }

    pub fn has_any_changes(&self) -> bool {
        self.has_uncommitted_changes() || self.commits_ahead > 0
    }

    /// Whether a pull request can be opened from this branch right now.
    pub fn can_open_pr(&self) -> bool {
        self.commits_ahead > 0
            && !self.branch.is_empty()
            && self.branch != "HEAD"
            && self.branch != self.base_branch
    }

    /// Redacts any known secret values from diff lines and commit subjects.
    pub fn redact(&mut self, redactor: &SecretRedactor) {
        if redactor.is_empty() {
            return;
        }
        redactor.redact_diffs(&mut self.uncommitted_diffs);
        redactor.redact_diffs(&mut self.committed_diffs);
        for commit in &mut self.commits {
            commit.subject = redactor.redact(&commit.subject);
        }
    }
}

/// Result returned after committing changes in a chat's working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatCommitResult {
    pub short_sha: String,
    pub subject: String,
    pub files_changed: usize,
}

/// Parameters for pushing a chat branch and opening a GitHub pull request.
#[derive(Debug, Clone)]
pub struct ChatPullRequestRequest {
    pub working_dir: PathBuf,
    pub branch: String,
    pub base_branch: Option<String>,
    pub title: String,
    pub body: String,
    pub draft: bool,
    pub github_auth: Option<GitHubAuth>,
    pub gh_binary: Option<PathBuf>,
    pub ssh_auth_sock: Option<String>,
    pub path_env: Option<OsString>,
}

/// Result returned after pushing a branch and opening a GitHub pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatPullRequestResult {
    pub url: String,
    pub branch: String,
    pub draft: bool,
}

/// Result returned after applying a chat worktree's changes to the main project checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatApplyResult {
    pub summary: String,
    pub files_applied: usize,
    pub commits_applied: usize,
}

/// Inspects git state, uncommitted changes (staged, unstaged, untracked), and branch commits
/// ahead of `base_branch` for `working_dir`.
pub fn inspect_chat_git_status(
    working_dir: &Path,
    base_branch: Option<&str>,
    is_worktree: bool,
) -> Result<ChatGitStatus, ChatGitError> {
    if !working_dir.is_dir() {
        return Err(ChatGitError::DirectoryMissing(working_dir.to_path_buf()));
    }
    if git_output(working_dir, &["rev-parse", "--is-inside-work-tree"]).is_err() {
        return Err(ChatGitError::NotAGitRepository);
    }

    let has_head = git_output(working_dir, &["rev-parse", "--verify", "HEAD"]).is_ok();
    let branch = git_output(working_dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "HEAD".to_owned());

    let resolved_base = resolve_base_branch(working_dir, base_branch, &branch);
    let has_remote = git_output(working_dir, &["remote"])
        .map(|out| !out.trim().is_empty())
        .unwrap_or(false);
    let has_upstream = git_output(
        working_dir,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .is_ok();

    let (uncommitted_diffs, untracked_paths) = collect_uncommitted_diffs(working_dir, has_head);

    let (commits_ahead, commits, committed_diffs) = if has_head
        && !resolved_base.is_empty()
        && resolved_base != branch
        && let Some(merge_base) = resolve_merge_base(working_dir, &resolved_base)
    {
        collect_branch_commits_and_diffs(working_dir, &merge_base)
    } else {
        (0, Vec::new(), Vec::new())
    };

    Ok(ChatGitStatus {
        working_dir: working_dir.to_path_buf(),
        is_worktree,
        branch,
        base_branch: resolved_base,
        has_remote,
        has_upstream,
        commits_ahead,
        commits,
        uncommitted_diffs,
        untracked_paths,
        committed_diffs,
    })
}

/// Stages all changes in `working_dir` and creates a git commit with `message`.
pub fn commit_chat_changes(
    working_dir: &Path,
    message: &str,
    ssh_auth_sock: Option<&str>,
) -> Result<ChatCommitResult, ChatGitError> {
    if !working_dir.is_dir() {
        return Err(ChatGitError::DirectoryMissing(working_dir.to_path_buf()));
    }
    if git_output(working_dir, &["rev-parse", "--is-inside-work-tree"]).is_err() {
        return Err(ChatGitError::NotAGitRepository);
    }
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err(ChatGitError::EmptyCommitMessage);
    }

    let status_before =
        git_output(working_dir, &["status", "--porcelain"]).map_err(ChatGitError::CommandFailed)?;
    if status_before.trim().is_empty() {
        return Err(ChatGitError::NothingToCommit);
    }

    git_output(working_dir, &["add", "-A"]).map_err(ChatGitError::CommandFailed)?;

    let staged_check = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(working_dir)
        .status()
        .map_err(|err| ChatGitError::CommandFailed(format!("couldn't run git: {err}")))?;
    if staged_check.success() {
        return Err(ChatGitError::NothingToCommit);
    }

    git_output_with_ssh(working_dir, &["commit", "-m", trimmed], ssh_auth_sock)
        .map_err(ChatGitError::CommandFailed)?;

    let short_sha = git_output(working_dir, &["rev-parse", "--short", "HEAD"])
        .unwrap_or_else(|_| "HEAD".to_owned());
    let files_changed = git_output(
        working_dir,
        &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
    )
    .map(|out| out.lines().filter(|l| !l.trim().is_empty()).count())
    .unwrap_or(0);
    let subject = trimmed.lines().next().unwrap_or(trimmed).trim().to_owned();

    log::info!(
        target: "bruk::chat",
        "chat.git.committed sha={short_sha} files={files_changed} dir={}",
        working_dir.display()
    );

    Ok(ChatCommitResult {
        short_sha,
        subject,
        files_changed,
    })
}

/// Pushes the chat's branch to `origin` and opens a GitHub pull request via `gh pr create`.
pub fn open_chat_pull_request(
    request: &ChatPullRequestRequest,
) -> Result<ChatPullRequestResult, ChatGitError> {
    if !request.working_dir.is_dir() {
        return Err(ChatGitError::DirectoryMissing(request.working_dir.clone()));
    }
    let title = request.title.trim();
    if title.is_empty() {
        return Err(ChatGitError::EmptyPrTitle);
    }

    let status =
        inspect_chat_git_status(&request.working_dir, request.base_branch.as_deref(), true)?;
    if status.commits_ahead == 0 {
        if status.has_uncommitted_changes() {
            return Err(ChatGitError::CommandFailed(
                "Commit your changes first before opening a pull request.".to_owned(),
            ));
        }
        return Err(ChatGitError::CommandFailed(format!(
            "Branch '{}' has no commits ahead of '{}'.",
            status.branch, status.base_branch
        )));
    }
    if status.branch == status.base_branch {
        return Err(ChatGitError::CommandFailed(format!(
            "Cannot open a pull request from base branch '{}'.",
            status.branch
        )));
    }

    let branch = if request.branch.trim().is_empty() {
        status.branch.clone()
    } else {
        request.branch.trim().to_owned()
    };

    // Push branch to origin first so `gh pr create` has a remote ref.
    let mut push_cmd = Command::new("git");
    push_cmd
        .args(["push", "-u", "origin", &branch])
        .current_dir(&request.working_dir);
    if let Some(sock) = request
        .ssh_auth_sock
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        push_cmd.env(SSH_AUTH_SOCK_ENV, sock);
    }
    let push_out = push_cmd
        .output()
        .map_err(|err| ChatGitError::CommandFailed(format!("couldn't run git push: {err}")))?;
    if !push_out.status.success() {
        let stderr = String::from_utf8_lossy(&push_out.stderr);
        return Err(ChatGitError::CommandFailed(
            last_lines(&stderr, 3).unwrap_or_else(|| format!("git push -u origin {branch} failed")),
        ));
    }

    let gh_bin = match request.gh_binary.as_ref() {
        Some(bin) => bin.clone(),
        None => resolve_gh_binary(request.path_env.as_deref())
            .map_err(|err| ChatGitError::CommandFailed(err.to_string()))?,
    };

    let mut pr_cmd = Command::new(gh_bin);
    pr_cmd.current_dir(&request.working_dir).args([
        "pr",
        "create",
        "--title",
        title,
        "--body",
        request.body.trim(),
    ]);
    if let Some(base) = request
        .base_branch
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty() && *b != branch)
    {
        pr_cmd.args(["--base", base]);
    }
    if request.draft {
        pr_cmd.arg("--draft");
    }
    if let Some(path_env) = request.path_env.as_ref() {
        pr_cmd.env("PATH", path_env);
    }
    if let Some(GitHubAuth::PersonalToken(token)) = request.github_auth.as_ref() {
        pr_cmd.env("GH_TOKEN", token);
    }

    let pr_out = pr_cmd
        .output()
        .map_err(|err| ChatGitError::CommandFailed(format!("couldn't run gh pr create: {err}")))?;
    if !pr_out.status.success() {
        let stderr = String::from_utf8_lossy(&pr_out.stderr);
        return Err(ChatGitError::CommandFailed(
            last_lines(&stderr, 4).unwrap_or_else(|| "gh pr create failed".to_owned()),
        ));
    }

    let stdout = String::from_utf8_lossy(&pr_out.stdout);
    let url = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with("https://") || line.starts_with("http://"))
        .unwrap_or_else(|| stdout.trim())
        .to_owned();

    log::info!(
        target: "bruk::chat",
        "chat.git.pr_opened branch={branch} draft={} url={url}",
        request.draft
    );

    Ok(ChatPullRequestResult {
        url,
        branch,
        draft: request.draft,
    })
}

/// Applies changes from a chat's isolated worktree back to `project_root`.
///
/// - If the worktree has **uncommitted changes only**, checks for conflicts and applies the
///   tracked patch + untracked files into `project_root` without modifying the worktree.
/// - If the worktree has **commits ahead only**, verifies `project_root` is clean and fast-forward
///   merges (or cherry-picks) those commits onto the checkout's current branch.
/// - If the worktree has **both** commits and uncommitted edits, asks the user to commit first so
///   the full change set applies atomically.
pub fn apply_chat_worktree_to_checkout(
    project_root: &Path,
    worktree_dir: &Path,
    base_branch: Option<&str>,
    ssh_auth_sock: Option<&str>,
) -> Result<ChatApplyResult, ChatGitError> {
    if !worktree_dir.is_dir() {
        return Err(ChatGitError::DirectoryMissing(worktree_dir.to_path_buf()));
    }
    if !project_root.is_dir() {
        return Err(ChatGitError::DirectoryMissing(project_root.to_path_buf()));
    }
    if canonical_or_raw(project_root) == canonical_or_raw(worktree_dir) {
        return Err(ChatGitError::NotAWorktree);
    }

    let status = inspect_chat_git_status(worktree_dir, base_branch, true)?;
    if !status.has_any_changes() {
        return Err(ChatGitError::NothingToApply);
    }

    if status.has_uncommitted_changes() && status.commits_ahead > 0 {
        return Err(ChatGitError::CommandFailed(
            "This worktree has both commits and uncommitted edits. Commit the remaining changes first so all commits can be applied cleanly together.".to_owned(),
        ));
    }

    if status.has_uncommitted_changes() {
        apply_uncommitted_worktree_changes(project_root, worktree_dir, &status)
    } else {
        apply_committed_worktree_branch(project_root, worktree_dir, &status, ssh_auth_sock)
    }
}

/// Opens `dir` in Terminal.app on macOS.
pub fn open_dir_in_terminal(dir: &Path) -> Result<(), ChatGitError> {
    if !dir.is_dir() {
        return Err(ChatGitError::DirectoryMissing(dir.to_path_buf()));
    }
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("/usr/bin/open")
            .args(["-a", "Terminal"])
            .arg(dir)
            .status()
            .map_err(|err| ChatGitError::CommandFailed(format!("couldn't open Terminal: {err}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(ChatGitError::CommandFailed(
                "Terminal could not open this folder.".to_owned(),
            ))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = dir;
        Err(ChatGitError::CommandFailed(
            "Opening Terminal is only supported on macOS.".to_owned(),
        ))
    }
}

/// Opens an `http://` or `https://` URL in the default macOS browser.
pub fn open_url_in_browser(url: &str) -> Result<(), ChatGitError> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err(ChatGitError::CommandFailed(
            "Only http:// and https:// links can be opened.".to_owned(),
        ));
    }
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("/usr/bin/open")
            .arg(trimmed)
            .status()
            .map_err(|err| ChatGitError::CommandFailed(format!("couldn't open link: {err}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(ChatGitError::CommandFailed(
                "Could not open link in browser.".to_owned(),
            ))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = trimmed;
        Err(ChatGitError::CommandFailed(
            "Opening browser links is only supported on macOS.".to_owned(),
        ))
    }
}

// --- Internals --------------------------------------------------------------------------

fn canonical_or_raw(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn resolve_base_branch(
    working_dir: &Path,
    requested: Option<&str>,
    current_branch: &str,
) -> String {
    if let Some(base) = requested.map(str::trim).filter(|s| !s.is_empty())
        && (git_ref_exists(working_dir, base)
            || git_ref_exists(working_dir, &format!("origin/{base}")))
    {
        return base.to_owned();
    }
    for candidate in ["main", "master"] {
        if git_ref_exists(working_dir, candidate)
            || git_ref_exists(working_dir, &format!("origin/{candidate}"))
        {
            return candidate.to_owned();
        }
    }
    current_branch.to_owned()
}

fn git_ref_exists(working_dir: &Path, reference: &str) -> bool {
    git_output(
        working_dir,
        &["rev-parse", "--verify", "--quiet", reference],
    )
    .is_ok()
}

fn resolve_merge_base(working_dir: &Path, base_branch: &str) -> Option<String> {
    if git_ref_exists(working_dir, base_branch)
        && let Ok(sha) = git_output(working_dir, &["merge-base", base_branch, "HEAD"])
        && !sha.is_empty()
    {
        return Some(sha);
    }
    let remote_ref = format!("origin/{base_branch}");
    if git_ref_exists(working_dir, &remote_ref)
        && let Ok(sha) = git_output(working_dir, &["merge-base", &remote_ref, "HEAD"])
        && !sha.is_empty()
    {
        return Some(sha);
    }
    None
}

fn collect_uncommitted_diffs(
    working_dir: &Path,
    has_head: bool,
) -> (Vec<DiffFileChange>, BTreeSet<String>) {
    let mut diffs = if has_head {
        run_git_diff(working_dir, &["diff", "HEAD"])
    } else {
        let mut combined = run_git_diff(working_dir, &["diff", "--cached"]);
        let unstaged = run_git_diff(working_dir, &["diff"]);
        let seen: BTreeSet<String> = combined.iter().map(|f| f.path.clone()).collect();
        for file in unstaged {
            if !seen.contains(&file.path) {
                combined.push(file);
            }
        }
        combined
    };

    let existing_paths: BTreeSet<String> = diffs.iter().map(|f| f.path.clone()).collect();
    let mut untracked_paths = BTreeSet::new();

    if let Ok(raw_list) = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .current_dir(working_dir)
        .output()
        && raw_list.status.success()
    {
        for entry in raw_list.stdout.split(|&b| b == 0) {
            if entry.is_empty() || untracked_paths.len() >= MAX_UNTRACKED_FILES {
                continue;
            }
            let rel = String::from_utf8_lossy(entry).into_owned();
            if existing_paths.contains(&rel) {
                continue;
            }
            let full_path = working_dir.join(&rel);
            if !full_path.is_file() {
                continue;
            }
            let diff = build_untracked_file_diff(&rel, &full_path);
            untracked_paths.insert(rel);
            diffs.push(diff);
        }
    }

    (diffs, untracked_paths)
}

fn build_untracked_file_diff(rel_path: &str, full_path: &Path) -> DiffFileChange {
    let Ok(bytes) = fs::read(full_path) else {
        return DiffFileChange {
            path: rel_path.to_owned(),
            additions: 0,
            deletions: 0,
            hunk_header: "@@ new file @@".to_owned(),
            lines: Vec::new(),
        };
    };

    let sample = &bytes[..bytes.len().min(8192)];
    if sample.contains(&0) {
        return DiffFileChange {
            path: rel_path.to_owned(),
            additions: 0,
            deletions: 0,
            hunk_header: "Binary file (new)".to_owned(),
            lines: Vec::new(),
        };
    }

    let truncated = bytes.len() > MAX_UNTRACKED_PREVIEW_BYTES;
    let slice = &bytes[..bytes.len().min(MAX_UNTRACKED_PREVIEW_BYTES)];
    let text = String::from_utf8_lossy(slice);
    let total_lines = text.lines().count();
    let lines: Vec<DiffLine> = text
        .lines()
        .take(MAX_DIFF_PREVIEW_LINES)
        .map(|line| DiffLine {
            kind: DiffLineKind::Added,
            text: line.to_owned(),
        })
        .collect();
    let hunk_header = if truncated {
        format!("@@ -0,0 +1,{total_lines}+ @@ (new file, truncated)")
    } else if total_lines == 0 {
        "@@ -0,0 +0,0 @@ (empty new file)".to_owned()
    } else {
        format!("@@ -0,0 +1,{total_lines} @@ (new file)")
    };

    DiffFileChange {
        path: rel_path.to_owned(),
        additions: total_lines,
        deletions: 0,
        hunk_header,
        lines,
    }
}

fn collect_branch_commits_and_diffs(
    working_dir: &Path,
    merge_base: &str,
) -> (usize, Vec<ChatCommitSummary>, Vec<DiffFileChange>) {
    let range = format!("{merge_base}..HEAD");
    let count = git_output(working_dir, &["rev-list", "--count", &range])
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if count == 0 {
        return (0, Vec::new(), Vec::new());
    }

    let max_arg = format!("-n{MAX_COMMITS_LISTED}");
    let commits = git_output(working_dir, &["log", &max_arg, "--format=%h%x09%s", &range])
        .map(|out| {
            out.lines()
                .filter_map(|line| {
                    let (sha, subject) = line.split_once('\t')?;
                    Some(ChatCommitSummary {
                        short_sha: sha.trim().to_owned(),
                        subject: subject.trim().to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let committed_diffs = run_git_diff(working_dir, &["diff", &range]);
    (count, commits, committed_diffs)
}

fn run_git_diff(working_dir: &Path, diff_args: &[&str]) -> Vec<DiffFileChange> {
    let mut args = vec![
        "-c",
        "diff.noprefix=false",
        "-c",
        "diff.mnemonicPrefix=false",
    ];
    args.extend_from_slice(diff_args);
    args.extend_from_slice(&["--no-ext-diff", "--no-color", "--unified=3"]);
    let Ok(output) = Command::new("git")
        .args(&args)
        .current_dir(working_dir)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_unified_diff(&String::from_utf8_lossy(&output.stdout))
}

fn apply_uncommitted_worktree_changes(
    project_root: &Path,
    worktree_dir: &Path,
    status: &ChatGitStatus,
) -> Result<ChatApplyResult, ChatGitError> {
    // Capture tracked diff (staged + unstaged against HEAD) as raw binary-safe patch.
    let patch_out = Command::new("git")
        .args([
            "-c",
            "diff.noprefix=false",
            "-c",
            "diff.mnemonicPrefix=false",
            "diff",
            "HEAD",
            "--binary",
            "--no-ext-diff",
            "--no-color",
        ])
        .current_dir(worktree_dir)
        .output()
        .map_err(|err| ChatGitError::CommandFailed(format!("couldn't generate patch: {err}")))?;
    if !patch_out.status.success() {
        let stderr = String::from_utf8_lossy(&patch_out.stderr);
        return Err(ChatGitError::CommandFailed(
            last_lines(&stderr, 3).unwrap_or_else(|| "couldn't generate diff patch".to_owned()),
        ));
    }

    // Verify untracked files won't clobber different files already in the main checkout.
    for rel in &status.untracked_paths {
        let src = worktree_dir.join(rel);
        let dst = project_root.join(rel);
        if dst.exists() {
            let same = fs::read(&src).ok() == fs::read(&dst).ok();
            if !same {
                return Err(ChatGitError::CommandFailed(format!(
                    "'{rel}' already exists with different contents in the project checkout."
                )));
            }
        }
    }

    // Dry-run check the tracked patch before mutating `project_root`.
    if !patch_out.stdout.is_empty() {
        run_git_apply(project_root, &["apply", "--check"], &patch_out.stdout).map_err(|err| {
            ChatGitError::CommandFailed(format!(
                "Patch does not apply cleanly to the project checkout: {err}"
            ))
        })?;
        run_git_apply(project_root, &["apply"], &patch_out.stdout)
            .map_err(ChatGitError::CommandFailed)?;
    }

    // Copy untracked files into the project checkout.
    for rel in &status.untracked_paths {
        let src = worktree_dir.join(rel);
        let dst = project_root.join(rel);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                ChatGitError::CommandFailed(format!(
                    "couldn't create folder '{}': {err}",
                    parent.display()
                ))
            })?;
        }
        fs::copy(&src, &dst).map_err(|err| {
            ChatGitError::CommandFailed(format!("couldn't copy '{rel}' to checkout: {err}"))
        })?;
    }

    let files_applied = status.uncommitted_file_count();
    let noun = if files_applied == 1 { "file" } else { "files" };
    let summary = format!("Applied {files_applied} {noun} to the project checkout.");

    log::info!(
        target: "bruk::chat",
        "chat.git.applied_patch files={files_applied} checkout={}",
        project_root.display()
    );

    Ok(ChatApplyResult {
        summary,
        files_applied,
        commits_applied: 0,
    })
}

fn apply_committed_worktree_branch(
    project_root: &Path,
    worktree_dir: &Path,
    status: &ChatGitStatus,
    ssh_auth_sock: Option<&str>,
) -> Result<ChatApplyResult, ChatGitError> {
    let checkout_dirty = git_output(project_root, &["status", "--porcelain"])
        .map_err(ChatGitError::CommandFailed)?;
    if !checkout_dirty.trim().is_empty() {
        return Err(ChatGitError::CommandFailed(
            "Your project checkout has uncommitted changes. Commit or stash them before applying commits from this worktree.".to_owned(),
        ));
    }

    let files_applied = status.committed_file_count();
    let commits_applied = status.commits_ahead;

    // Prefer fast-forward merge when the checkout hasn't diverged.
    if git_output(project_root, &["merge", "--ff-only", &status.branch]).is_ok() {
        let noun = if commits_applied == 1 {
            "commit"
        } else {
            "commits"
        };
        let summary = format!(
            "Fast-forwarded project checkout with {commits_applied} {noun} from '{}'.",
            status.branch
        );
        log::info!(
            target: "bruk::chat",
            "chat.git.applied_ff branch={} commits={commits_applied}",
            status.branch
        );
        return Ok(ChatApplyResult {
            summary,
            files_applied,
            commits_applied,
        });
    }

    // Otherwise cherry-pick the worktree's commits onto the checkout's current HEAD.
    let merge_base = resolve_merge_base(worktree_dir, &status.base_branch).ok_or_else(|| {
        ChatGitError::CommandFailed(format!(
            "Couldn't find merge-base between '{}' and '{}'.",
            status.base_branch, status.branch
        ))
    })?;
    let range = format!("{merge_base}..{}", status.branch);
    if let Err(err) = git_output_with_ssh(project_root, &["cherry-pick", &range], ssh_auth_sock) {
        let _ = git_output(project_root, &["cherry-pick", "--abort"]);
        return Err(ChatGitError::CommandFailed(format!(
            "Cherry-pick conflicted and was aborted cleanly: {err}"
        )));
    }

    let noun = if commits_applied == 1 {
        "commit"
    } else {
        "commits"
    };
    let summary = format!(
        "Cherry-picked {commits_applied} {noun} from '{}' into the project checkout.",
        status.branch
    );
    log::info!(
        target: "bruk::chat",
        "chat.git.applied_cherry_pick branch={} commits={commits_applied}",
        status.branch
    );
    Ok(ChatApplyResult {
        summary,
        files_applied,
        commits_applied,
    })
}

fn run_git_apply(dir: &Path, args: &[&str], patch: &[u8]) -> Result<(), String> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("couldn't run git {}: {err}", args.join(" ")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(patch)
            .map_err(|err| format!("couldn't pipe patch to git apply: {err}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|err| format!("couldn't wait for git apply: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(last_lines(&stderr, 3).unwrap_or_else(|| "git apply failed".to_owned()))
    }
}

fn git_output(dir: &Path, args: &[&str]) -> Result<String, String> {
    git_output_with_ssh(dir, args, None)
}

fn git_output_with_ssh(
    dir: &Path,
    args: &[&str],
    ssh_auth_sock: Option<&str>,
) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir);
    if let Some(sock) = ssh_auth_sock.map(str::trim).filter(|s| !s.is_empty()) {
        cmd.env(SSH_AUTH_SOCK_ENV, sock);
    }
    let output = cmd
        .output()
        .map_err(|err| format!("couldn't run git: {err}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(last_lines(&stderr, 3)
            .map(|text| text.trim_start_matches("fatal: ").to_owned())
            .unwrap_or_else(|| format!("git {} failed", args.join(" "))))
    }
}

fn last_lines(text: &str, count: usize) -> Option<String> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    let start = lines.len().saturating_sub(count);
    Some(lines[start..].join("\n"))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;

    fn init_git_repo(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        git_output(dir, &["init", "-b", "main"]).unwrap();
        git_output(dir, &["config", "user.name", "Test User"]).unwrap();
        git_output(dir, &["config", "user.email", "test@example.com"]).unwrap();
        git_output(dir, &["config", "commit.gpgsign", "false"]).unwrap();
        git_output(dir, &["config", "tag.gpgsign", "false"]).unwrap();
        fs::write(dir.join("README.md"), "hello\n").unwrap();
        git_output(dir, &["add", "README.md"]).unwrap();
        git_output(dir, &["commit", "-m", "initial commit"]).unwrap();
    }

    fn add_worktree(repo: &Path, worktree: &Path, branch: &str) {
        let path = worktree.to_string_lossy().into_owned();
        git_output(repo, &["worktree", "add", "-b", branch, &path, "HEAD"]).unwrap();
    }

    #[test]
    fn inspects_modified_staged_and_untracked_files() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        init_git_repo(&repo);

        fs::write(repo.join("README.md"), "hello\nworld\n").unwrap();
        fs::write(repo.join("new_file.rs"), "fn main() {}\n").unwrap();

        let status = inspect_chat_git_status(&repo, Some("main"), false).unwrap();
        assert_eq!(status.branch, "main");
        assert_eq!(status.uncommitted_file_count(), 2);
        assert!(status.untracked_paths.contains("new_file.rs"));
        assert!(!status.untracked_paths.contains("README.md"));
        assert_eq!(status.uncommitted_additions(), 2);
        assert_eq!(status.uncommitted_deletions(), 0);
        assert_eq!(status.commits_ahead, 0);
        assert!(!status.can_open_pr());
    }

    #[test]
    fn commits_changes_and_reports_commits_ahead_on_worktree_branch() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let worktree = temp.path().join("wt");
        init_git_repo(&repo);
        add_worktree(&repo, &worktree, "agent/chat-add-greeting");

        fs::write(worktree.join("README.md"), "hello\ngreeting\n").unwrap();
        fs::write(worktree.join("notes.txt"), "line 1\nline 2\n").unwrap();

        let commit = commit_chat_changes(&worktree, "Add greeting and notes", None).unwrap();
        assert!(!commit.short_sha.is_empty());
        assert_eq!(commit.subject, "Add greeting and notes");
        assert_eq!(commit.files_changed, 2);

        let status = inspect_chat_git_status(&worktree, Some("main"), true).unwrap();
        assert!(!status.has_uncommitted_changes());
        assert_eq!(status.commits_ahead, 1);
        assert_eq!(status.commits.len(), 1);
        assert_eq!(status.commits[0].subject, "Add greeting and notes");
        assert_eq!(status.committed_file_count(), 2);
        assert!(status.can_open_pr());
    }

    #[test]
    fn applies_uncommitted_worktree_changes_including_untracked_files_to_checkout() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let worktree = temp.path().join("wt");
        init_git_repo(&repo);
        add_worktree(&repo, &worktree, "agent/chat-patch");

        fs::write(worktree.join("README.md"), "hello\nfrom worktree\n").unwrap();
        fs::create_dir_all(worktree.join("src")).unwrap();
        fs::write(
            worktree.join("src/helper.rs"),
            "pub fn ok() -> bool { true }\n",
        )
        .unwrap();

        let applied =
            apply_chat_worktree_to_checkout(&repo, &worktree, Some("main"), None).unwrap();
        assert_eq!(applied.files_applied, 2);
        assert_eq!(applied.commits_applied, 0);
        assert_eq!(
            fs::read_to_string(repo.join("README.md")).unwrap(),
            "hello\nfrom worktree\n"
        );
        assert_eq!(
            fs::read_to_string(repo.join("src/helper.rs")).unwrap(),
            "pub fn ok() -> bool { true }\n"
        );
    }

    #[test]
    fn applies_committed_worktree_branch_via_ff_and_cherry_pick() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let worktree = temp.path().join("wt");
        init_git_repo(&repo);
        add_worktree(&repo, &worktree, "agent/chat-commit");

        fs::write(worktree.join("feature.txt"), "shipped\n").unwrap();
        commit_chat_changes(&worktree, "Add feature", None).unwrap();

        // Advance main on a different file so cherry-pick path is exercised too.
        fs::write(repo.join("other.txt"), "main moved\n").unwrap();
        git_output(&repo, &["add", "other.txt"]).unwrap();
        git_output(&repo, &["commit", "-m", "main commit"]).unwrap();

        let applied =
            apply_chat_worktree_to_checkout(&repo, &worktree, Some("main"), None).unwrap();
        assert_eq!(applied.commits_applied, 1);
        assert_eq!(
            fs::read_to_string(repo.join("feature.txt")).unwrap(),
            "shipped\n"
        );
        assert_eq!(
            fs::read_to_string(repo.join("other.txt")).unwrap(),
            "main moved\n"
        );
    }

    #[test]
    fn refuses_apply_when_worktree_has_both_commits_and_uncommitted_edits() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let worktree = temp.path().join("wt");
        init_git_repo(&repo);
        add_worktree(&repo, &worktree, "agent/chat-mixed");

        fs::write(worktree.join("a.txt"), "one\n").unwrap();
        commit_chat_changes(&worktree, "Commit one", None).unwrap();
        fs::write(worktree.join("b.txt"), "uncommitted\n").unwrap();

        let err =
            apply_chat_worktree_to_checkout(&repo, &worktree, Some("main"), None).unwrap_err();
        assert!(
            err.to_string()
                .contains("both commits and uncommitted edits")
        );
    }

    #[cfg(unix)]
    #[test]
    fn pushes_branch_and_opens_pull_request_via_gh() {
        let temp = TempDir::new().unwrap();
        let bare = temp.path().join("origin.git");
        fs::create_dir_all(&bare).unwrap();
        git_output(&bare, &["init", "--bare"]).unwrap();

        let repo = temp.path().join("repo");
        init_git_repo(&repo);
        git_output(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]).unwrap();
        git_output(&repo, &["push", "-u", "origin", "main"]).unwrap();

        let worktree = temp.path().join("wt");
        add_worktree(&repo, &worktree, "agent/chat-pr");
        fs::write(worktree.join("README.md"), "hello\npr\n").unwrap();
        commit_chat_changes(&worktree, "Add PR line", None).unwrap();

        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let gh_log = temp.path().join("gh.log");
        let gh_bin = bin_dir.join("gh");
        fs::write(
            &gh_bin,
            format!(
                "#!/bin/sh\necho \"$*\" > '{}'\necho 'https://github.com/acme/repo/pull/42'\n",
                gh_log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&gh_bin, fs::Permissions::from_mode(0o755)).unwrap();

        let result = open_chat_pull_request(&ChatPullRequestRequest {
            working_dir: worktree.clone(),
            branch: "agent/chat-pr".to_owned(),
            base_branch: Some("main".to_owned()),
            title: "Add PR line".to_owned(),
            body: "Summary from chat".to_owned(),
            draft: true,
            github_auth: Some(GitHubAuth::GitHubCli),
            gh_binary: Some(gh_bin),
            ssh_auth_sock: None,
            path_env: None,
        })
        .unwrap();

        assert_eq!(result.url, "https://github.com/acme/repo/pull/42");
        assert!(result.draft);
        let args = fs::read_to_string(&gh_log).unwrap();
        assert!(args.contains("pr create"));
        assert!(args.contains("--draft"));
        assert!(args.contains("--base main"));

        // Confirm branch was pushed to bare origin.
        let remote_heads = git_output(&bare, &["branch", "--list"]).unwrap();
        assert!(remote_heads.contains("agent/chat-pr"));
    }
}
