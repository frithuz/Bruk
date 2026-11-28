//! Multi-turn agent chat sessions, attachments, worktrees, and git/PR actions.

pub mod chat;
pub mod chat_attachment;
pub mod chat_git;

pub use chat::{
    ChatContextCategory, ChatContextGroup, ChatContextItem, ChatContextKind, ChatContextUsage,
    ChatEntry, ChatEvent, ChatNoticeTone, ChatProviderTotals, ChatSession, ChatSessionStatus,
    ChatSessionSummary, ChatStreamItem, ChatTokenBreakdown, ChatToolStatus, ChatTurnControl,
    ChatTurnError, ChatTurnRequest, ChatUsageDelta, ChatWorkspace, ChatWorktree, ChatWorktreeSetup,
    TokenScope, UNTITLED_CHAT_TITLE, UsageCount, build_chat_turn_command, chat_title_from_message,
    chat_worktree_branch, execute_chat_turn_blocking, micros_to_cents, parse_antigravity_chat_line,
    parse_claude_chat_line, parse_claude_context_usage, parse_codex_chat_line,
    parse_codex_rollout_context, parse_codex_rollout_token_totals, query_chat_context,
    remove_chat_worktree, spawn_chat_turn,
};
pub use chat_attachment::{
    CHAT_ATTACHMENT_MAX_BYTES, CHAT_ATTACHMENTS_DIR, ChatAttachment, ChatAttachmentUpload,
    attachment_mime_type, format_bytes, is_model_image_type, plan_chat_attachments,
};
pub use chat_git::{
    BranchMergeEvidence, ChatApplyResult, ChatCommitResult, ChatCommitSummary, ChatGitError,
    ChatGitStatus, ChatPullRequestRequest, ChatPullRequestResult, GitHubPullRequestLookup,
    MergedBranchCleanup, RemoteBranchCleanup, apply_chat_worktree_to_checkout,
    chat_branch_merge_evidence, commit_chat_changes, delete_merged_branch, fetch_base_branch,
    inspect_chat_git_status, open_chat_pull_request, open_dir_in_terminal, open_url_in_browser,
    worktree_has_uncommitted_changes,
};
