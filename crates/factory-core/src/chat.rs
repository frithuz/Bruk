//! Chat sessions: open-ended, multi-turn conversations with a project's agents that sit
//! beside workflows.
//!
//! A workflow runs a fixed graph of steps; a chat is free-form. Each user message runs one
//! non-interactive turn of the provider CLI inside the project checkout and resumes the
//! provider's own session (`claude --resume <id>`, `codex exec … resume <id>`), so the agent
//! keeps its whole conversation between turns while Bruk streams and records each turn.
//!
//! Turns get the same user-context isolation as workflow steps (see [`crate::step_context`]):
//! only the chosen agent and the skills and subagents it declares enter the agent's context,
//! next to the repository's checked-in instructions.
//!
//! A chat works either in the project checkout or in its own git worktree (a new branch from
//! `HEAD`, created before the first turn and kept for the chat's life). After each turn Bruk
//! reads how the agent's context window is filled ([`query_chat_context`]).

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    chat_attachment::{
        ChatAttachment, ChatAttachmentUpload, claude_inline_images, claude_user_message_line,
        prompt_with_attachments, stage_chat_attachments,
    },
    claude_settings::ClaudeSpawnSettings,
    codex_settings::CodexSpawnSettings,
    definitions::{AgentTaskStepDef, BrukCatalog},
    mcp::{
        ANTIGRAVITY_LOG_FILE, McpFailureTracker, McpOrigin, McpProblem, McpSources,
        McpSourcesPolicy, ResolvedMcpSet, antigravity_log_failures, claude_init_failures,
        codex_stderr_failure,
    },
    model::ProjectId,
    preferences::ProviderKind,
    project_settings::ProjectSandboxMode,
    runner::terminate_pid,
    step_context::{
        ISOLATED_CODEX_HOME_DIR, StepContextError, StepMcpScope, TemplateContext,
        build_isolated_antigravity_spawn_plan, build_isolated_claude_spawn_plan,
        build_isolated_codex_spawn_plan, compile_step_context, materialize_step_context,
    },
};

/// Title shown for a chat before its first message names it.
pub const UNTITLED_CHAT_TITLE: &str = "New chat";

const TITLE_MAX_CHARS: usize = 60;
const PREVIEW_MAX_CHARS: usize = 90;
const TOOL_TARGET_MAX_CHARS: usize = 160;
const TOOL_DETAIL_MAX_CHARS: usize = 200;
/// The most of a tool call's full input or output a transcript keeps (characters). Longer
/// text keeps its start and its end, with a note saying how much was left out between them.
const TOOL_FULL_MAX_CHARS: usize = 256 * 1024;
const ERROR_MAX_CHARS: usize = 600;
const BRANCH_SLUG_MAX_CHARS: usize = 40;
const CONTEXT_GROUP_MAX_ITEMS: usize = 50;
/// The id Bruk gives its `get_context_usage` control request to Claude Code.
const CONTEXT_REQUEST_ID: &str = "bruk_context_usage";
const CONTEXT_QUERY_TIMEOUT: Duration = Duration::from_secs(45);

/// Where a chat session's latest turn stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatSessionStatus {
    /// Waiting for the user's next message.
    #[default]
    Idle,
    /// A turn is running.
    Running,
    /// The latest turn failed; the next message continues the same session.
    Failed,
    /// The user stopped the latest turn.
    Stopped,
}

impl ChatSessionStatus {
    pub fn is_running(self) -> bool {
        self == Self::Running
    }
}

/// Outcome of one tool call shown in a chat transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatToolStatus {
    #[default]
    Running,
    Succeeded,
    Failed,
}

/// How a transcript notice reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatNoticeTone {
    #[default]
    Info,
    Error,
}

/// One item of a chat transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatEntry {
    /// A message the user sent.
    User {
        text: String,
        #[serde(default)]
        sent_at: i64,
        /// Files sent with the message (the chat's own copies).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<ChatAttachment>,
    },
    /// A reply of the agent (Markdown). `streaming` while its text is still arriving.
    Assistant {
        text: String,
        #[serde(default)]
        streaming: bool,
    },
    /// A tool the agent called (a shell command, a file edit, a search).
    Tool {
        #[serde(default)]
        tool_id: Option<String>,
        verb: String,
        #[serde(default)]
        target: String,
        #[serde(default)]
        status: ChatToolStatus,
        #[serde(default)]
        detail: String,
        /// Everything the agent passed the tool (the whole command, the file and its edit).
        #[serde(default, skip_serializing_if = "String::is_empty")]
        input: String,
        /// Everything the tool gave back.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        output: String,
    },
    /// The agent's reasoning between replies and tool calls, as the provider reports it.
    Reasoning { text: String },
    /// Something Bruk reports about the turn (an error, a stop).
    Notice {
        text: String,
        #[serde(default)]
        tone: ChatNoticeTone,
    },
    /// An MCP server that didn't reach the agent, shown as a banner with a way to fix it.
    McpProblem { problem: McpProblem },
}

/// Where a chat's agent works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatWorkspace {
    /// The project's own checkout, shared with the user's edits.
    #[default]
    Checkout,
    /// A git worktree of its own on a new branch, so the checkout stays untouched.
    Worktree,
}

impl ChatWorkspace {
    pub fn label(self) -> &'static str {
        match self {
            Self::Checkout => "Project checkout",
            Self::Worktree => "New worktree",
        }
    }
}

/// The git worktree a chat works in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatWorktree {
    pub path: PathBuf,
    pub branch: String,
}

/// What the provider last reported as its session's running totals. Codex reports a thread's
/// tokens, and Claude Code (without per-turn figures) its cost, summed over every turn of the
/// session, including turns of earlier resumes; a turn's share is the difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChatProviderTotals {
    #[serde(default)]
    pub tokens: u64,
    #[serde(default)]
    pub cost_micros: u64,
    /// The session's running token totals by kind, for providers that report them so.
    #[serde(default)]
    pub breakdown: ChatTokenBreakdown,
}

/// A chat's tokens by kind. `input` is fresh (uncached) prompt input; cache reads and writes
/// are prompt input served from or added to the provider's prompt cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChatTokenBreakdown {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write: u64,
}

impl ChatTokenBreakdown {
    pub fn total(self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }

    pub fn is_zero(self) -> bool {
        self.total() == 0
    }

    pub fn plus(self, other: Self) -> Self {
        Self {
            input: self.input.saturating_add(other.input),
            output: self.output.saturating_add(other.output),
            cache_read: self.cache_read.saturating_add(other.cache_read),
            cache_write: self.cache_write.saturating_add(other.cache_write),
        }
    }

    /// What `self` (a running total) added since `baseline`; all of it when any kind counts
    /// less than before (the provider started counting afresh).
    fn since(self, baseline: Self) -> Self {
        let fresh = self.input < baseline.input
            || self.output < baseline.output
            || self.cache_read < baseline.cache_read
            || self.cache_write < baseline.cache_write;
        if fresh {
            return self;
        }
        Self {
            input: self.input - baseline.input,
            output: self.output - baseline.output,
            cache_read: self.cache_read - baseline.cache_read,
            cache_write: self.cache_write - baseline.cache_write,
        }
    }
}

/// A token or cost figure as a provider reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageCount {
    /// Used by this turn alone.
    Turn(u64),
    /// The provider session's running total, earlier turns included.
    SessionTotal(u64),
}

impl UsageCount {
    pub fn value(self) -> u64 {
        match self {
            Self::Turn(n) | Self::SessionTotal(n) => n,
        }
    }

    /// This turn's share, moving `baseline` (the last running total) along.
    fn turn_share(self, baseline: &mut u64) -> u64 {
        match self {
            Self::Turn(n) => n,
            Self::SessionTotal(total) => {
                // A total below the last one means the provider started counting afresh.
                let share = if total >= *baseline {
                    total - *baseline
                } else {
                    total
                };
                *baseline = total;
                share
            }
        }
    }
}

/// Tokens and cost one event added to a chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChatUsageDelta {
    pub tokens: u64,
    pub cost_micros: u64,
}

impl ChatUsageDelta {
    pub fn is_zero(self) -> bool {
        self.tokens == 0 && self.cost_micros == 0
    }
}

/// What a slice of the context window holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatContextKind {
    /// Content in the window (instructions, tools, messages).
    #[default]
    Used,
    /// Room the provider keeps free for compacting the conversation.
    Buffer,
    /// Room still free.
    Free,
    /// Tool definitions kept out of the window until the agent asks for them.
    Deferred,
}

/// One slice of the context window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatContextCategory {
    pub id: String,
    pub name: String,
    pub tokens: u64,
    #[serde(default)]
    pub kind: ChatContextKind,
}

/// One named thing in the window (a memory file, a skill, an MCP tool) and its size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatContextItem {
    pub name: String,
    pub tokens: u64,
}

/// A list of context items under a heading, largest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatContextGroup {
    pub title: String,
    pub items: Vec<ChatContextItem>,
    /// How many items there are in all (`items` keeps the largest).
    #[serde(default)]
    pub total_items: usize,
}

/// How a chat's context window was filled after its latest turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatContextUsage {
    #[serde(default)]
    pub model: Option<String>,
    /// Tokens in the window.
    pub used_tokens: u64,
    /// The window's size.
    pub window_tokens: u64,
    pub categories: Vec<ChatContextCategory>,
    #[serde(default)]
    pub groups: Vec<ChatContextGroup>,
    /// The size at which the provider compacts the conversation, when it says.
    #[serde(default)]
    pub autocompact_at: Option<u64>,
    /// False when the provider reports totals only (Codex).
    #[serde(default)]
    pub has_breakdown: bool,
    /// Updated from a running turn's token counts rather than measured: totals are current,
    /// the breakdown is the last measurement with the conversation grown or shrunk to match.
    #[serde(default)]
    pub live: bool,
    pub captured_at: i64,
}

impl ChatContextUsage {
    /// A reading from a running turn's token count alone, before any measurement (the first
    /// turn of a chat): the window's size isn't known yet.
    pub fn from_live_total(used_tokens: u64, now: i64) -> Self {
        Self {
            model: None,
            used_tokens,
            window_tokens: 0,
            categories: vec![ChatContextCategory {
                id: "messages".to_owned(),
                name: "Messages".to_owned(),
                tokens: used_tokens,
                kind: ChatContextKind::Used,
            }],
            groups: Vec::new(),
            autocompact_at: None,
            has_breakdown: false,
            live: true,
            captured_at: now,
        }
    }

    /// Takes a running turn's count of what is in the window. The conversation slice (what
    /// a turn adds to) absorbs the change and free space shrinks or grows to match; the other
    /// slices keep their last measurement until the turn ends and the window is read again.
    pub fn apply_live_total(&mut self, used_tokens: u64, now: i64) {
        let conversation = self
            .categories
            .iter()
            .position(|c| matches!(c.id.as_str(), "messages" | "conversation"))
            .or_else(|| {
                self.categories
                    .iter()
                    .rposition(|c| c.kind == ChatContextKind::Used)
            });
        let others: u64 = self
            .categories
            .iter()
            .enumerate()
            .filter(|(i, c)| c.kind == ChatContextKind::Used && Some(*i) != conversation)
            .map(|(_, c)| c.tokens)
            .sum();
        let conversation_tokens = used_tokens.saturating_sub(others);
        match conversation {
            Some(i) => self.categories[i].tokens = conversation_tokens,
            None => self.categories.insert(
                0,
                ChatContextCategory {
                    id: "messages".to_owned(),
                    name: "Messages".to_owned(),
                    tokens: conversation_tokens,
                    kind: ChatContextKind::Used,
                },
            ),
        }
        let reserved: u64 = self
            .categories
            .iter()
            .filter(|c| c.kind == ChatContextKind::Buffer)
            .map(|c| c.tokens)
            .sum();
        let free = self
            .window_tokens
            .saturating_sub(used_tokens.saturating_add(reserved));
        if let Some(category) = self
            .categories
            .iter_mut()
            .find(|c| c.kind == ChatContextKind::Free)
        {
            category.tokens = free;
        }
        self.used_tokens = used_tokens;
        self.live = true;
        self.captured_at = now;
    }

    /// How full the window is, 0–100.
    pub fn used_percent(&self) -> u8 {
        if self.window_tokens == 0 {
            return 0;
        }
        ((self.used_tokens as f64 / self.window_tokens as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8
    }
}

/// A conversation with one agent CLI in one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub project_id: ProjectId,
    pub project_name: String,
    #[serde(default)]
    pub title: String,
    pub provider: ProviderKind,
    /// Catalog reference of the agent the chat talks to; `None` uses the provider's own
    /// default behavior.
    #[serde(default)]
    pub agent: Option<String>,
    /// Display name of `agent` when the chat started.
    #[serde(default)]
    pub agent_name: Option<String>,
    /// Model override for the chat; `None` falls back to the agent, then the project, then the
    /// provider default.
    #[serde(default)]
    pub model: Option<String>,
    /// What the agent may do in the checkout; applies from the next turn when changed.
    #[serde(default)]
    pub sandbox: ProjectSandboxMode,
    /// The provider CLI's own session (Claude session id, Codex thread id) each turn resumes.
    #[serde(default)]
    pub provider_session_id: Option<String>,
    #[serde(default)]
    pub entries: Vec<ChatEntry>,
    #[serde(default)]
    pub status: ChatSessionStatus,
    /// Every token the chat's turns used (input, cache reads and writes, output).
    #[serde(default)]
    pub total_tokens: u64,
    /// What the chat's turns cost, in millionths of a dollar.
    #[serde(default)]
    pub total_cost_micros: u64,
    #[serde(default)]
    pub provider_totals: ChatProviderTotals,
    #[serde(default)]
    pub workspace: ChatWorkspace,
    /// The worktree of a [`ChatWorkspace::Worktree`] chat (planned until its first turn
    /// creates it).
    #[serde(default)]
    pub worktree: Option<ChatWorktree>,
    /// The context window after the latest turn, when known.
    #[serde(default)]
    pub context: Option<ChatContextUsage>,
    /// Tokens of the chat's finished turns, by kind (turns from before Bruk recorded kinds
    /// are left out).
    #[serde(default)]
    pub token_breakdown: ChatTokenBreakdown,
    /// The running turn's tokens so far, by kind.
    #[serde(skip)]
    pub turn_tokens: ChatTokenBreakdown,
    /// The provider session's running total behind `turn_tokens`, when it reports one.
    #[serde(skip)]
    turn_session_total: Option<ChatTokenBreakdown>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// What a chat list row shows, without the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSessionSummary {
    pub id: String,
    pub title: String,
    pub agent_label: String,
    pub provider: ProviderKind,
    pub status: ChatSessionStatus,
    pub preview: String,
    pub message_count: usize,
    pub updated_at: i64,
}

impl ChatSession {
    /// A chat with no messages yet.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        project_id: ProjectId,
        project_name: impl Into<String>,
        provider: ProviderKind,
        agent: Option<String>,
        agent_name: Option<String>,
        sandbox: ProjectSandboxMode,
        now: i64,
    ) -> Self {
        Self {
            id: id.into(),
            project_id,
            project_name: project_name.into(),
            title: String::new(),
            provider,
            agent: agent.filter(|a| !a.trim().is_empty()),
            agent_name: agent_name.filter(|a| !a.trim().is_empty()),
            model: None,
            sandbox,
            provider_session_id: None,
            entries: Vec::new(),
            status: ChatSessionStatus::Idle,
            total_tokens: 0,
            total_cost_micros: 0,
            provider_totals: ChatProviderTotals::default(),
            workspace: ChatWorkspace::Checkout,
            worktree: None,
            context: None,
            token_breakdown: ChatTokenBreakdown::default(),
            turn_tokens: ChatTokenBreakdown::default(),
            turn_session_total: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Every token the chat used by kind, the running turn's so far included.
    pub fn token_use(&self) -> ChatTokenBreakdown {
        self.token_breakdown.plus(self.turn_tokens)
    }

    /// Adds the running turn's tokens to the chat's.
    fn settle_turn_tokens(&mut self) {
        self.token_breakdown = self.token_breakdown.plus(self.turn_tokens);
        if let Some(total) = self.turn_session_total.take() {
            self.provider_totals.breakdown = total;
        }
        self.turn_tokens = ChatTokenBreakdown::default();
    }

    /// What the chat's turns cost, in cents.
    pub fn total_cost_cents(&self) -> u64 {
        micros_to_cents(self.total_cost_micros)
    }

    /// The title, or [`UNTITLED_CHAT_TITLE`] before the first message.
    pub fn display_title(&self) -> String {
        if self.title.trim().is_empty() {
            UNTITLED_CHAT_TITLE.to_owned()
        } else {
            self.title.clone()
        }
    }

    /// Who the user is talking to: the agent's name, or the provider's.
    pub fn agent_label(&self) -> String {
        self.agent_name
            .clone()
            .or_else(|| self.agent.clone())
            .unwrap_or_else(|| self.provider.label().to_owned())
    }

    pub fn is_running(&self) -> bool {
        self.status.is_running()
    }

    /// How many messages the user sent.
    pub fn message_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e, ChatEntry::User { .. }))
            .count()
    }

    /// The first line of the latest message, either side's.
    pub fn preview(&self) -> String {
        self.entries
            .iter()
            .rev()
            .find_map(|entry| match entry {
                ChatEntry::User { text, .. } | ChatEntry::Assistant { text, .. }
                    if !text.trim().is_empty() =>
                {
                    Some(single_line(text, PREVIEW_MAX_CHARS))
                }
                ChatEntry::User { attachments, .. } if !attachments.is_empty() => {
                    let names: Vec<&str> = attachments.iter().map(|a| a.name.as_str()).collect();
                    Some(single_line(&names.join(", "), PREVIEW_MAX_CHARS))
                }
                _ => None,
            })
            .unwrap_or_default()
    }

    pub fn summary(&self) -> ChatSessionSummary {
        ChatSessionSummary {
            id: self.id.clone(),
            title: self.display_title(),
            agent_label: self.agent_label(),
            provider: self.provider,
            status: self.status,
            preview: self.preview(),
            message_count: self.message_count(),
            updated_at: self.updated_at,
        }
    }

    /// Records a message the user sent; the first one names the chat.
    pub fn push_user_message(&mut self, text: &str, now: i64) {
        self.push_user_message_with_attachments(text, Vec::new(), now);
    }

    /// Records a message the user sent with files attached (either may be empty, not both);
    /// the first message names the chat, after its first attachment when it has no text.
    pub fn push_user_message_with_attachments(
        &mut self,
        text: &str,
        attachments: Vec<ChatAttachment>,
        now: i64,
    ) {
        let text = text.trim();
        if text.is_empty() && attachments.is_empty() {
            return;
        }
        if self.title.trim().is_empty() {
            self.title = match attachments.first() {
                Some(first) if text.is_empty() => chat_title_from_message(&first.name),
                _ => chat_title_from_message(text),
            };
        }
        self.entries.push(ChatEntry::User {
            text: text.to_owned(),
            sent_at: now,
            attachments,
        });
        self.updated_at = now;
    }

    /// Folds one event of a running turn into the transcript, returning the tokens and cost
    /// it added.
    pub fn apply_event(&mut self, event: &ChatEvent, now: i64) -> ChatUsageDelta {
        let mut delta = ChatUsageDelta::default();
        match event {
            ChatEvent::TurnStarted => self.status = ChatSessionStatus::Running,
            ChatEvent::ProviderSessionStarted { session_id } => {
                let session_id = session_id.trim();
                if !session_id.is_empty() {
                    if self.provider_session_id.as_deref() != Some(session_id) {
                        // A new provider session counts from zero.
                        self.provider_totals = ChatProviderTotals::default();
                    }
                    self.provider_session_id = Some(session_id.to_owned());
                }
            }
            ChatEvent::WorktreeReady { path, branch } => {
                self.worktree = Some(ChatWorktree {
                    path: path.clone(),
                    branch: branch.clone(),
                });
            }
            ChatEvent::McpProblem(problem) => {
                self.entries.push(ChatEntry::McpProblem {
                    problem: problem.clone(),
                });
            }
            ChatEvent::McpServersReady(_) => return delta,
            ChatEvent::ContextUsage(usage) => {
                // Not activity: the chat keeps its place in the list.
                self.context = Some(usage.clone());
                return delta;
            }
            ChatEvent::TokenUse { tokens, scope } => {
                match scope {
                    TokenScope::Turn => self.turn_tokens = *tokens,
                    TokenScope::SessionTotal => {
                        self.turn_tokens = tokens.since(self.provider_totals.breakdown);
                        self.turn_session_total = Some(*tokens);
                    }
                }
                return delta;
            }
            ChatEvent::ContextTokens { used_tokens } => {
                if *used_tokens > 0 {
                    match self.context.as_mut() {
                        Some(context) => context.apply_live_total(*used_tokens, now),
                        None => {
                            self.context =
                                Some(ChatContextUsage::from_live_total(*used_tokens, now));
                        }
                    }
                }
                return delta;
            }
            ChatEvent::AssistantDelta { text } => {
                if let Some(ChatEntry::Assistant {
                    text: body,
                    streaming: true,
                }) = self.entries.last_mut()
                {
                    body.push_str(text);
                } else if !text.is_empty() {
                    self.entries.push(ChatEntry::Assistant {
                        text: text.clone(),
                        streaming: true,
                    });
                }
            }
            ChatEvent::AssistantMessage { text } => {
                if let Some(ChatEntry::Assistant {
                    text: body,
                    streaming,
                }) = self.entries.last_mut()
                    && *streaming
                {
                    // The finished message replaces what streamed in for it.
                    *body = text.clone();
                    *streaming = false;
                } else if !text.trim().is_empty() {
                    self.entries.push(ChatEntry::Assistant {
                        text: text.clone(),
                        streaming: false,
                    });
                }
            }
            ChatEvent::ToolStarted {
                tool_id,
                verb,
                target,
                input,
            } => {
                self.finish_streaming();
                let known = tool_id.as_ref().and_then(|id| {
                    self.entries.iter_mut().find(|entry| {
                        matches!(entry, ChatEntry::Tool { tool_id: Some(existing), .. } if existing == id)
                    })
                });
                match known {
                    // A provider can repeat the call when it finishes, sometimes saying more.
                    Some(ChatEntry::Tool {
                        input: known_input, ..
                    }) => {
                        if !input.is_empty() {
                            *known_input = keep_full_text(input);
                        }
                    }
                    _ => self.entries.push(ChatEntry::Tool {
                        tool_id: tool_id.clone(),
                        verb: verb.clone(),
                        target: target.clone(),
                        status: ChatToolStatus::Running,
                        detail: String::new(),
                        input: keep_full_text(input),
                        output: String::new(),
                    }),
                }
            }
            ChatEvent::Reasoning { text } => {
                if text.trim().is_empty() {
                    return delta;
                }
                let entry = ChatEntry::Reasoning {
                    text: text.trim().to_owned(),
                };
                // Reasoning comes before the reply it leads to.
                match self.entries.last() {
                    Some(ChatEntry::Assistant {
                        streaming: true, ..
                    }) => {
                        let at = self.entries.len() - 1;
                        self.entries.insert(at, entry);
                    }
                    _ => self.entries.push(entry),
                }
            }
            ChatEvent::ToolFinished {
                tool_id,
                success,
                detail,
                output,
            } => {
                let found = self.entries.iter_mut().rev().find(|entry| match entry {
                    ChatEntry::Tool {
                        tool_id: Some(existing),
                        ..
                    } if tool_id.is_some() => Some(existing) == tool_id.as_ref(),
                    ChatEntry::Tool { status, .. } if tool_id.is_none() => {
                        *status == ChatToolStatus::Running
                    }
                    _ => false,
                });
                if let Some(ChatEntry::Tool {
                    status,
                    detail: entry_detail,
                    output: entry_output,
                    ..
                }) = found
                {
                    *status = if *success {
                        ChatToolStatus::Succeeded
                    } else {
                        ChatToolStatus::Failed
                    };
                    *entry_detail = single_line(detail, TOOL_DETAIL_MAX_CHARS);
                    *entry_output = keep_full_text(output);
                }
            }
            ChatEvent::Usage {
                tokens,
                cost_micros,
            } => {
                delta = ChatUsageDelta {
                    tokens: tokens.turn_share(&mut self.provider_totals.tokens),
                    cost_micros: cost_micros.turn_share(&mut self.provider_totals.cost_micros),
                };
                self.total_tokens = self.total_tokens.saturating_add(delta.tokens);
                self.total_cost_micros = self.total_cost_micros.saturating_add(delta.cost_micros);
            }
            ChatEvent::TurnFinished { status, error } => {
                self.finish_streaming();
                self.settle_turn_tokens();
                let settled = if *status == ChatSessionStatus::Idle {
                    ChatToolStatus::Succeeded
                } else {
                    ChatToolStatus::Failed
                };
                for entry in &mut self.entries {
                    if let ChatEntry::Tool { status, .. } = entry
                        && *status == ChatToolStatus::Running
                    {
                        *status = settled;
                    }
                }
                if let Some(message) = error.as_deref().filter(|m| !m.trim().is_empty()) {
                    self.entries.push(ChatEntry::Notice {
                        text: message.trim().to_owned(),
                        tone: ChatNoticeTone::Error,
                    });
                } else if *status == ChatSessionStatus::Stopped {
                    self.entries.push(ChatEntry::Notice {
                        text: "Stopped.".to_owned(),
                        tone: ChatNoticeTone::Info,
                    });
                }
                self.status = if status.is_running() {
                    ChatSessionStatus::Idle
                } else {
                    *status
                };
            }
        }
        self.updated_at = now;
        delta
    }

    /// Settles a turn the app quit in the middle of (it cannot still be running). True if the
    /// session changed.
    pub fn mark_interrupted(&mut self, now: i64) -> bool {
        if !self.is_running() {
            return false;
        }
        self.apply_event(
            &ChatEvent::TurnFinished {
                status: ChatSessionStatus::Stopped,
                error: Some("Bruk closed before this reply finished.".to_owned()),
            },
            now,
        );
        true
    }

    fn finish_streaming(&mut self) {
        if let Some(ChatEntry::Assistant { streaming, .. }) = self.entries.last_mut() {
            *streaming = false;
        }
    }
}

/// A chat title from its first message: the first line, whitespace collapsed, cut short.
pub fn chat_title_from_message(text: &str) -> String {
    let title = single_line(text, TITLE_MAX_CHARS);
    if title.is_empty() {
        UNTITLED_CHAT_TITLE.to_owned()
    } else {
        title
    }
}

/// The first non-empty line of `text` with runs of whitespace collapsed, at most `max_chars`
/// characters (an ellipsis marks a cut, including dropped lines).
fn single_line(text: &str, max_chars: usize) -> String {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let Some(first) = lines.next() else {
        return String::new();
    };
    let collapsed = first.split_whitespace().collect::<Vec<_>>().join(" ");
    let more_lines = lines.next().is_some();
    if collapsed.chars().count() > max_chars {
        let mut cut: String = collapsed.chars().take(max_chars).collect();
        cut = cut.trim_end().to_owned();
        cut.push('…');
        cut
    } else if more_lines {
        format!("{collapsed} …")
    } else {
        collapsed
    }
}

/// Text kept whole for the transcript, up to [`TOOL_FULL_MAX_CHARS`]: past that, its start and
/// its end with a note of how many characters were left out between them.
fn keep_full_text(text: &str) -> String {
    let text = text.trim_end();
    let total = text.chars().count();
    if total <= TOOL_FULL_MAX_CHARS {
        return text.to_owned();
    }
    let half = TOOL_FULL_MAX_CHARS / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = text.chars().skip(total - half).collect();
    format!(
        "{head}\n\n[… {} characters not kept …]\n\n{tail}",
        total - 2 * half
    )
}

/// Events of one running chat turn, in order, for the UI and persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    TurnStarted,
    /// The provider named its session; later turns resume it.
    ProviderSessionStarted {
        session_id: String,
    },
    /// More text of the reply being written.
    AssistantDelta {
        text: String,
    },
    /// A finished block of reply text (replaces what streamed in for it).
    AssistantMessage {
        text: String,
    },
    ToolStarted {
        tool_id: Option<String>,
        verb: String,
        target: String,
        /// Everything the agent passed the tool.
        input: String,
    },
    ToolFinished {
        tool_id: Option<String>,
        success: bool,
        detail: String,
        /// Everything the tool gave back.
        output: String,
    },
    /// The agent's reasoning, shown folded away in the transcript.
    Reasoning {
        text: String,
    },
    /// Tokens and cost (millionths of a dollar) as the provider reported them.
    Usage {
        tokens: UsageCount,
        cost_micros: UsageCount,
    },
    /// The chat's worktree exists (it was just created, maybe on another branch name).
    WorktreeReady {
        path: PathBuf,
        branch: String,
    },
    /// A fresh reading of the context window.
    ContextUsage(ChatContextUsage),
    /// How many tokens the window holds as of the running turn's latest model request.
    ContextTokens {
        used_tokens: u64,
    },
    /// The tokens the running turn has used so far by kind (each reading replaces the last).
    TokenUse {
        tokens: ChatTokenBreakdown,
        scope: TokenScope,
    },
    /// An MCP server didn't reach the agent for this turn.
    McpProblem(McpProblem),
    /// MCP servers that started for this turn, so earlier notifications about them can clear.
    McpServersReady(Vec<(String, McpOrigin)>),
    /// The turn ended: `Idle` when it succeeded.
    TurnFinished {
        status: ChatSessionStatus,
        error: Option<String>,
    },
}

/// What a [`ChatEvent::TokenUse`] reading covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenScope {
    /// The running turn alone.
    Turn,
    /// The provider session so far, earlier turns included.
    SessionTotal,
}

/// One thing a provider CLI's JSON stream said, as chat turns read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatStreamItem {
    SessionStarted(String),
    TextDelta(String),
    Text(String),
    ToolUse {
        id: Option<String>,
        verb: String,
        target: String,
        /// Everything the agent passed the tool, readable.
        input: String,
    },
    ToolResult {
        id: Option<String>,
        is_error: bool,
        detail: String,
        /// Everything the tool gave back.
        output: String,
    },
    /// The agent's reasoning (Claude's thinking, Codex's reasoning summaries).
    Reasoning(String),
    /// The window holds this many tokens as of the latest model request (prompt, cache and
    /// what the reply has written so far).
    ContextTokens(u64),
    /// A model request of the turn started, with its prompt's tokens (Claude).
    RequestStarted(ChatTokenBreakdown),
    /// The latest model request has written this many tokens (Claude).
    RequestOutput(u64),
    /// Tokens by kind as the provider totals them.
    TokenUse {
        tokens: ChatTokenBreakdown,
        scope: TokenScope,
    },
    /// The end of the turn as the provider reports it.
    Result {
        text: Option<String>,
        is_error: bool,
        tokens: UsageCount,
        /// Millionths of a dollar.
        cost_micros: UsageCount,
    },
    Error(String),
}

/// Reads one line of `claude --print --output-format stream-json --verbose
/// --include-partial-messages`. Subagent traffic (`parent_tool_use_id` set) belongs to the
/// tool call that started it and is left out.
pub fn parse_claude_chat_line(line: &str) -> Vec<ChatStreamItem> {
    let Ok(val) = serde_json::from_str::<Value>(line.trim()) else {
        return Vec::new();
    };
    let Some(kind) = val.get("type").and_then(Value::as_str) else {
        return Vec::new();
    };
    let from_subagent = val
        .get("parent_tool_use_id")
        .is_some_and(|parent| !parent.is_null());

    match kind {
        "system" => {
            let is_init = val.get("subtype").and_then(Value::as_str) == Some("init");
            match val.get("session_id").and_then(Value::as_str) {
                Some(id) if is_init && !id.trim().is_empty() => {
                    vec![ChatStreamItem::SessionStarted(id.to_owned())]
                }
                _ => Vec::new(),
            }
        }
        "stream_event" if !from_subagent => {
            let Some(event) = val.get("event") else {
                return Vec::new();
            };
            match event.get("type").and_then(Value::as_str) {
                Some("message_start") => {
                    let usage = event.get("message").and_then(|m| m.get("usage"));
                    let mut items = Vec::new();
                    if let Some(tokens) = claude_window_tokens(usage) {
                        items.push(ChatStreamItem::ContextTokens(tokens));
                        items.push(ChatStreamItem::RequestStarted(claude_token_breakdown(
                            usage,
                            CLAUDE_API_USAGE_KEYS,
                        )));
                    }
                    return items;
                }
                Some("message_delta") => {
                    return event
                        .get("usage")
                        .and_then(|u| u.get("output_tokens"))
                        .and_then(Value::as_u64)
                        .map(|output| vec![ChatStreamItem::RequestOutput(output)])
                        .unwrap_or_default();
                }
                _ => {}
            }
            let is_delta = event.get("type").and_then(Value::as_str) == Some("content_block_delta");
            let delta = event.get("delta");
            let is_text =
                delta.and_then(|d| d.get("type")).and_then(Value::as_str) == Some("text_delta");
            match delta.and_then(|d| d.get("text")).and_then(Value::as_str) {
                Some(text) if is_delta && is_text && !text.is_empty() => {
                    vec![ChatStreamItem::TextDelta(text.to_owned())]
                }
                _ => Vec::new(),
            }
        }
        "assistant" if !from_subagent => {
            let Some(blocks) = message_content(&val) else {
                return Vec::new();
            };
            let mut items: Vec<ChatStreamItem> =
                claude_window_tokens(val.get("message").and_then(|m| m.get("usage")))
                    .map(ChatStreamItem::ContextTokens)
                    .into_iter()
                    .collect();
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(Value::as_str)
                            && !text.trim().is_empty()
                        {
                            items.push(ChatStreamItem::Text(text.to_owned()));
                        }
                    }
                    Some("tool_use") => items.push(ChatStreamItem::ToolUse {
                        id: string_field(block, "id"),
                        verb: block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("Tool")
                            .to_owned(),
                        target: claude_tool_target(block.get("input")),
                        input: block
                            .get("input")
                            .map(format_tool_input)
                            .unwrap_or_default(),
                    }),
                    Some("thinking") => {
                        if let Some(text) = block.get("thinking").and_then(Value::as_str)
                            && !text.trim().is_empty()
                        {
                            items.push(ChatStreamItem::Reasoning(text.to_owned()));
                        }
                    }
                    _ => {}
                }
            }
            items
        }
        "user" if !from_subagent => {
            let Some(blocks) = message_content(&val) else {
                return Vec::new();
            };
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
                .map(|block| ChatStreamItem::ToolResult {
                    id: string_field(block, "tool_use_id"),
                    is_error: block
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    detail: tool_result_text(block.get("content")),
                    output: tool_result_full_text(block.get("content")),
                })
                .collect()
        }
        "result" => {
            let subtype = val.get("subtype").and_then(Value::as_str).unwrap_or("");
            let is_error = val
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || subtype.starts_with("error");
            let mut text = string_field(&val, "result").filter(|t| !t.trim().is_empty());
            if is_error && text.is_none() {
                text = val
                    .get("errors")
                    .and_then(Value::as_array)
                    .map(|errors| {
                        errors
                            .iter()
                            .filter_map(|e| {
                                e.as_str()
                                    .map(str::to_owned)
                                    .or_else(|| string_field(e, "message"))
                            })
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .filter(|t| !t.trim().is_empty())
                    .or_else(|| (!subtype.is_empty()).then(|| subtype.replace('_', " ")));
            }
            let (tokens, cost_micros) = claude_turn_usage(&val);
            let mut items = Vec::new();
            let breakdown = claude_turn_breakdown(&val);
            if !breakdown.is_zero() {
                items.push(ChatStreamItem::TokenUse {
                    tokens: breakdown,
                    scope: TokenScope::Turn,
                });
            }
            items.push(ChatStreamItem::Result {
                text,
                is_error,
                tokens,
                cost_micros,
            });
            items
        }
        _ => Vec::new(),
    }
}

/// What a Claude model request put in the window: its prompt (fresh, cached and newly cached
/// input) and the reply's output so far. `None` without a usage block or when it counts nothing.
fn claude_window_tokens(usage: Option<&Value>) -> Option<u64> {
    let usage = usage?;
    let total = [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
        "output_tokens",
    ]
    .into_iter()
    .map(|key| usage.get(key).and_then(Value::as_u64).unwrap_or(0))
    .fold(0u64, u64::saturating_add);
    (total > 0).then_some(total)
}

/// Keys of a Claude usage block, in [`ChatTokenBreakdown`] order (input, output, cache reads,
/// cache writes): the API's in stream events and `usage`, Claude Code's in `modelUsage`.
const CLAUDE_API_USAGE_KEYS: [&str; 4] = [
    "input_tokens",
    "output_tokens",
    "cache_read_input_tokens",
    "cache_creation_input_tokens",
];
const CLAUDE_MODEL_USAGE_KEYS: [&str; 4] = [
    "inputTokens",
    "outputTokens",
    "cacheReadInputTokens",
    "cacheCreationInputTokens",
];

fn claude_token_breakdown(usage: Option<&Value>, keys: [&str; 4]) -> ChatTokenBreakdown {
    let count = |key: &str| {
        usage
            .and_then(|u| u.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    ChatTokenBreakdown {
        input: count(keys[0]),
        output: count(keys[1]),
        cache_read: count(keys[2]),
        cache_write: count(keys[3]),
    }
}

/// A Claude `result` event's tokens by kind for the turn (every model it used, subagents
/// included, when `model_usage_since_last_result` is there).
fn claude_turn_breakdown(val: &Value) -> ChatTokenBreakdown {
    match val
        .get("model_usage_since_last_result")
        .and_then(Value::as_object)
    {
        Some(models) => models
            .values()
            .map(|usage| claude_token_breakdown(Some(usage), CLAUDE_MODEL_USAGE_KEYS))
            .fold(ChatTokenBreakdown::default(), ChatTokenBreakdown::plus),
        None => claude_token_breakdown(val.get("usage"), CLAUDE_API_USAGE_KEYS),
    }
}

/// A Claude `result` event's tokens and cost. `model_usage_since_last_result` covers this turn
/// alone (every model it used, subagents included). Without it, `usage` still covers the turn,
/// but `total_cost_usd` covers the whole session: Claude Code restores a resumed session's cost.
fn claude_turn_usage(val: &Value) -> (UsageCount, UsageCount) {
    if let Some(models) = val
        .get("model_usage_since_last_result")
        .and_then(Value::as_object)
    {
        let mut tokens = 0u64;
        let mut cost_usd = 0.0f64;
        for usage in models.values() {
            for key in [
                "inputTokens",
                "outputTokens",
                "cacheReadInputTokens",
                "cacheCreationInputTokens",
            ] {
                tokens = tokens.saturating_add(usage.get(key).and_then(Value::as_u64).unwrap_or(0));
            }
            cost_usd += usage.get("costUSD").and_then(Value::as_f64).unwrap_or(0.0);
        }
        return (
            UsageCount::Turn(tokens),
            UsageCount::Turn(usd_to_micros(cost_usd)),
        );
    }
    let usage = val.get("usage");
    let tokens = [
        "input_tokens",
        "output_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
    ]
    .into_iter()
    .map(|key| {
        usage
            .and_then(|u| u.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    })
    .fold(0u64, u64::saturating_add);
    let cost_usd = val
        .get("total_cost_usd")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    (
        UsageCount::Turn(tokens),
        UsageCount::SessionTotal(usd_to_micros(cost_usd)),
    )
}

fn usd_to_micros(usd: f64) -> u64 {
    (usd * 1_000_000.0).round().max(0.0) as u64
}

/// Millionths of a dollar to cents, rounded.
pub fn micros_to_cents(micros: u64) -> u64 {
    micros.saturating_add(5_000) / 10_000
}

/// Reads one line of `codex exec --json`.
pub fn parse_codex_chat_line(line: &str) -> Vec<ChatStreamItem> {
    let Ok(val) = serde_json::from_str::<Value>(line.trim()) else {
        return Vec::new();
    };
    match val.get("type").and_then(Value::as_str) {
        Some("thread.started") => string_field(&val, "thread_id")
            .filter(|id| !id.trim().is_empty())
            .map(|id| vec![ChatStreamItem::SessionStarted(id)])
            .unwrap_or_default(),
        Some("item.started") => val
            .get("item")
            .and_then(codex_tool_use)
            .into_iter()
            .collect(),
        Some("item.completed") => {
            let Some(item) = val.get("item") else {
                return Vec::new();
            };
            match item.get("type").and_then(Value::as_str) {
                Some("agent_message") => string_field(item, "text")
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| vec![ChatStreamItem::Text(t)])
                    .unwrap_or_default(),
                Some("error") => string_field(item, "message")
                    .map(|m| vec![ChatStreamItem::Error(m)])
                    .unwrap_or_default(),
                Some("reasoning") => string_field(item, "text")
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| vec![ChatStreamItem::Reasoning(t)])
                    .unwrap_or_default(),
                _ => {
                    let Some(tool_use) = codex_tool_use(item) else {
                        return Vec::new();
                    };
                    let status = item.get("status").and_then(Value::as_str).unwrap_or("");
                    let exit_code = item.get("exit_code").and_then(Value::as_i64);
                    let failed = status == "failed"
                        || exit_code.is_some_and(|code| code != 0)
                        || item.get("error").is_some_and(|e| !e.is_null());
                    let detail = match exit_code {
                        Some(code) if code != 0 => format!("exit {code}"),
                        _ => tool_result_text(item.get("aggregated_output")),
                    };
                    vec![
                        tool_use,
                        ChatStreamItem::ToolResult {
                            id: string_field(item, "id"),
                            is_error: failed,
                            detail,
                            output: codex_tool_output(item),
                        },
                    ]
                }
            }
        }
        Some("turn.completed") => {
            let usage = val.get("usage");
            let count = |key: &str| {
                usage
                    .and_then(|u| u.get(key))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
            };
            // The thread's total so far (Codex restores it on `resume`); input includes the
            // cached part.
            let mut items = Vec::new();
            let breakdown = codex_token_breakdown(usage);
            if !breakdown.is_zero() {
                items.push(ChatStreamItem::TokenUse {
                    tokens: breakdown,
                    scope: TokenScope::SessionTotal,
                });
            }
            items.push(ChatStreamItem::Result {
                text: None,
                is_error: false,
                tokens: UsageCount::SessionTotal(count("input_tokens") + count("output_tokens")),
                cost_micros: UsageCount::Turn(0),
            });
            items
        }
        Some("turn.failed") => {
            let message = val
                .get("error")
                .and_then(|e| string_field(e, "message"))
                .unwrap_or_else(|| "Codex turn failed".to_owned());
            vec![ChatStreamItem::Error(message)]
        }
        Some("error") => string_field(&val, "message")
            .map(|m| vec![ChatStreamItem::Error(m)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// A Codex usage block by kind: its `input_tokens` include the cached part.
fn codex_token_breakdown(usage: Option<&Value>) -> ChatTokenBreakdown {
    let count = |key: &str| {
        usage
            .and_then(|u| u.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let cached = count("cached_input_tokens");
    ChatTokenBreakdown {
        input: count("input_tokens").saturating_sub(cached),
        output: count("output_tokens"),
        cache_read: cached,
        cache_write: 0,
    }
}

/// Reads one line of `agy --output-format stream-json`. Each `step_update` is one step of the
/// conversation (a reply, a tool call); text arrives as `text_delta`s, and the closing `result`
/// carries the turn's whole reply and its tokens.
pub fn parse_antigravity_chat_line(line: &str) -> Vec<ChatStreamItem> {
    let Ok(val) = serde_json::from_str::<Value>(line.trim()) else {
        return Vec::new();
    };
    match val.get("event").and_then(Value::as_str) {
        Some("init") => string_field(&val, "conversation_id")
            .filter(|id| !id.trim().is_empty())
            .map(|id| vec![ChatStreamItem::SessionStarted(id)])
            .unwrap_or_default(),
        Some("step_update") => {
            let Some(step) = val.get("step_update") else {
                return Vec::new();
            };
            let state = step.get("state").and_then(Value::as_str).unwrap_or("");
            match step.get("step_type").and_then(Value::as_str) {
                Some("agent_response") => string_field(step, "text_delta")
                    .filter(|t| !t.is_empty())
                    .map(|t| vec![ChatStreamItem::TextDelta(t)])
                    .unwrap_or_default(),
                Some("tool") => {
                    let Some(tool_use) = antigravity_tool_use(step) else {
                        return Vec::new();
                    };
                    if state == "ACTIVE" {
                        return vec![tool_use];
                    }
                    let info = step.get("tool_info");
                    let error = info.and_then(|i| i.get("error")).filter(|e| has_content(e));
                    let failed = !matches!(state, "DONE" | "") || error.is_some();
                    let result = error.or_else(|| info.and_then(|i| i.get("output")));
                    let detail = result
                        .map(|v| tool_result_text(Some(v)))
                        .unwrap_or_default();
                    let output = result.map(format_tool_output).unwrap_or_default();
                    let id = tool_use_id(&tool_use);
                    vec![
                        tool_use,
                        ChatStreamItem::ToolResult {
                            id,
                            is_error: failed,
                            detail,
                            output,
                        },
                    ]
                }
                _ => Vec::new(),
            }
        }
        Some("result") => {
            let Some(result) = val.get("result") else {
                return Vec::new();
            };
            let status = result.get("status").and_then(Value::as_str).unwrap_or("");
            let is_error = !status.eq_ignore_ascii_case("success");
            let text = if is_error {
                string_field(result, "error")
                    .filter(|t| !t.trim().is_empty())
                    .or_else(|| string_field(result, "response"))
            } else {
                string_field(result, "response")
            }
            .filter(|t| !t.trim().is_empty());
            let usage = result.get("usage");
            let count = |key: &str| {
                usage
                    .and_then(|u| u.get(key))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
            };
            let breakdown = ChatTokenBreakdown {
                input: count("input_tokens"),
                output: count("output_tokens").saturating_add(count("thinking_tokens")),
                cache_read: count("cache_read_tokens"),
                cache_write: 0,
            };
            let mut items = Vec::new();
            if !breakdown.is_zero() {
                items.push(ChatStreamItem::TokenUse {
                    tokens: breakdown,
                    scope: TokenScope::Turn,
                });
            }
            items.push(ChatStreamItem::Result {
                text,
                is_error,
                tokens: UsageCount::Turn(antigravity_usage_tokens(usage)),
                cost_micros: UsageCount::Turn(0),
            });
            items
        }
        Some("error") => string_field(&val, "message")
            .or_else(|| val.get("error").and_then(|e| string_field(e, "message")))
            .map(|m| vec![ChatStreamItem::Error(m)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Every token an Antigravity `usage` block counts: fresh input, cache reads, output and
/// thinking.
pub(crate) fn antigravity_usage_tokens(usage: Option<&Value>) -> u64 {
    [
        "input_tokens",
        "cache_read_tokens",
        "output_tokens",
        "thinking_tokens",
    ]
    .into_iter()
    .map(|key| {
        usage
            .and_then(|u| u.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    })
    .fold(0u64, u64::saturating_add)
}

fn has_content(val: &Value) -> bool {
    match val {
        Value::Null => false,
        Value::String(text) => !text.trim().is_empty(),
        _ => true,
    }
}

fn tool_use_id(item: &ChatStreamItem) -> Option<String> {
    match item {
        ChatStreamItem::ToolUse { id, .. } => id.clone(),
        _ => None,
    }
}

/// An Antigravity tool step as a tool call: a friendlier verb for the common tools and the
/// command, file, pattern or URL it works on.
fn antigravity_tool_use(step: &Value) -> Option<ChatStreamItem> {
    let info = step.get("tool_info");
    let name = step
        .get("tool_name")
        .and_then(Value::as_str)
        .or_else(|| info.and_then(|i| i.get("name")).and_then(Value::as_str))?;
    let parameters = info.and_then(|i| i.get("parameters"));
    let (verb, target) = antigravity_tool_label(name, parameters);
    let id = step
        .get("step_index")
        .and_then(Value::as_u64)
        .map(|index| format!("agy-step-{index}"));
    Some(ChatStreamItem::ToolUse {
        id,
        verb,
        target: single_line(&target, TOOL_TARGET_MAX_CHARS),
        input: parameters.map(format_tool_input).unwrap_or_default(),
    })
}

/// The verb and target Bruk shows for an Antigravity tool call.
pub(crate) fn antigravity_tool_label(name: &str, parameters: Option<&Value>) -> (String, String) {
    let verb = match name {
        "run_command" | "send_command_input" => "Shell",
        "view_file" | "read_file" => "Read",
        "write_to_file"
        | "write_file"
        | "edit_file"
        | "replace_file_content"
        | "multi_replace_file_content"
        | "sed_file" => "Edit",
        "grep_search" => "Search",
        "find_by_name" => "Find",
        "list_dir" => "List",
        "search_web" => "Web search",
        "read_url_content" => "Fetch",
        "invoke_subagent" | "browser_subagent" => "Subagent",
        other => other,
    };
    let mut target = String::new();
    if let Some(Value::Object(params)) = parameters {
        let preferred = [
            "commandline",
            "command",
            "absolutepath",
            "targetfile",
            "path",
            "file",
            "directorypath",
            "searchdirectory",
            "query",
            "pattern",
            "url",
            "task",
            "description",
        ];
        target = preferred
            .iter()
            .find_map(|key| {
                params
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(key))
                    .and_then(|(_, v)| v.as_str())
                    .filter(|v| !v.trim().is_empty())
            })
            .or_else(|| {
                params
                    .values()
                    .filter_map(Value::as_str)
                    .find(|v| !v.trim().is_empty())
            })
            .unwrap_or_default()
            .to_owned();
    }
    (verb.to_owned(), target)
}

fn codex_tool_use(item: &Value) -> Option<ChatStreamItem> {
    let (verb, target, input) = match item.get("type").and_then(Value::as_str)? {
        "command_execution" => {
            let command = string_field(item, "command").unwrap_or_default();
            ("Shell", command.clone(), command)
        }
        "file_change" => {
            let changes = item
                .get("changes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let paths = changes
                .iter()
                .filter_map(|c| string_field(c, "path"))
                .collect::<Vec<_>>()
                .join(", ");
            let listed = changes
                .iter()
                .filter_map(|c| {
                    let path = string_field(c, "path")?;
                    Some(match string_field(c, "kind") {
                        Some(kind) => format!("{kind} {path}"),
                        None => path,
                    })
                })
                .collect::<Vec<_>>()
                .join("\n");
            ("Edit", paths, listed)
        }
        "mcp_tool_call" => {
            let server = string_field(item, "server").unwrap_or_default();
            let tool = string_field(item, "tool").unwrap_or_default();
            let arguments = item
                .get("arguments")
                .filter(|a| has_content(a))
                .map(format_tool_input)
                .unwrap_or_default();
            ("MCP", [server, tool].join("."), arguments)
        }
        "web_search" => {
            let query = string_field(item, "query").unwrap_or_default();
            ("Web search", query.clone(), query)
        }
        _ => return None,
    };
    Some(ChatStreamItem::ToolUse {
        id: string_field(item, "id"),
        verb: verb.to_owned(),
        target: single_line(&target, TOOL_TARGET_MAX_CHARS),
        input,
    })
}

/// Everything a finished Codex item gave back: a command's output and exit code, an MCP
/// call's result or error.
fn codex_tool_output(item: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(output) = item
        .get("aggregated_output")
        .and_then(Value::as_str)
        .filter(|o| !o.trim().is_empty())
    {
        parts.push(output.trim_end().to_owned());
    }
    if let Some(result) = item.get("result").filter(|r| has_content(r)) {
        let content = result.get("content").filter(|c| has_content(c));
        parts.push(match content {
            Some(content) => tool_result_full_text(Some(content)),
            None => format_tool_output(result),
        });
    }
    if let Some(error) = item.get("error").filter(|e| has_content(e)) {
        parts.push(string_field(error, "message").unwrap_or_else(|| format_tool_output(error)));
    }
    if let Some(code) = item.get("exit_code").and_then(Value::as_i64) {
        parts.push(format!("[exit code {code}]"));
    }
    parts.retain(|p| !p.trim().is_empty());
    parts.join("\n")
}

fn message_content(val: &Value) -> Option<&Vec<Value>> {
    val.get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
}

fn string_field(val: &Value, key: &str) -> Option<String> {
    val.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// What a Claude tool call is about: its command, file, pattern, URL or query.
fn claude_tool_target(input: Option<&Value>) -> String {
    let Some(input) = input else {
        return String::new();
    };
    for key in [
        "command",
        "file_path",
        "notebook_path",
        "pattern",
        "path",
        "url",
        "query",
        "description",
        "skill",
    ] {
        if let Some(value) = input.get(key).and_then(Value::as_str)
            && !value.trim().is_empty()
        {
            return single_line(value, TOOL_TARGET_MAX_CHARS);
        }
    }
    String::new()
}

/// The first line of a tool result's content (a string, or text blocks).
fn tool_result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => single_line(text, TOOL_DETAIL_MAX_CHARS),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .map(|t| single_line(t, TOOL_DETAIL_MAX_CHARS))
            .find(|t| !t.is_empty())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Every text block of a tool result (a string, or blocks), whole; other blocks (an image)
/// are named.
fn tool_result_full_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.trim_end().to_owned(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .map(|block| match block.get("text").and_then(Value::as_str) {
                Some(text) => text.trim_end().to_owned(),
                None => match block.get("type").and_then(Value::as_str) {
                    Some(kind) => format!("[{kind}]"),
                    None => format_tool_output(block),
                },
            })
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Null) | None => String::new(),
        Some(other) => format_tool_output(other),
    }
}

/// A tool's output as text: strings as they are, anything else as indented JSON.
fn format_tool_output(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim_end().to_owned(),
        Value::Null => String::new(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// What the agent passed a tool, one `name: value` line per argument. Multi-line text (a
/// script, a file's new contents, an edit) goes under its name as it is, so it reads the way
/// the tool gets it.
fn format_tool_input(input: &Value) -> String {
    let Value::Object(fields) = input else {
        return format_tool_output(input);
    };
    let mut out = String::new();
    for (name, value) in fields {
        if !out.is_empty() {
            out.push('\n');
        }
        match value {
            Value::String(text) if text.contains('\n') => {
                out.push_str(&format!("{name}:\n{}", text.trim_end()));
            }
            Value::String(text) => out.push_str(&format!("{name}: {text}")),
            Value::Array(_) | Value::Object(_) => {
                let json = serde_json::to_string_pretty(value).unwrap_or_default();
                out.push_str(&format!("{name}: {json}"));
            }
            other => out.push_str(&format!("{name}: {other}")),
        }
    }
    out
}

/// Errors that keep a chat turn from starting.
#[derive(Debug, Error)]
pub enum ChatTurnError {
    #[error("Type a message to send")]
    EmptyMessage,
    #[error("The {0} CLI isn't set up. Add it in Settings › Providers.")]
    ProviderMissing(&'static str),
    #[error("{0} chats aren't supported yet. Pick Claude or Codex.")]
    UnsupportedProvider(&'static str),
    #[error("{0}")]
    StepContext(#[from] StepContextError),
    #[error("{0}")]
    Attachment(String),
}

/// Everything one chat turn needs, resolved by the caller from the session, the project,
/// and preferences.
#[derive(Debug, Clone)]
pub struct ChatTurnRequest {
    pub session_id: String,
    pub provider: ProviderKind,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub sandbox: ProjectSandboxMode,
    /// The provider session to resume; `None` starts the conversation.
    pub resume_session_id: Option<String>,
    pub prompt: String,
    /// Files sent with `prompt`, copied into the chat's folder before the provider starts.
    pub attachments: Vec<ChatAttachmentUpload>,
    /// Where the agent works: the project's root, or the chat's worktree.
    pub working_dir: PathBuf,
    /// The chat's worktree (at `working_dir`), created by the turn when missing.
    pub worktree: Option<ChatWorktreeSetup>,
    /// Out-of-repo folder for the session's instructions and (Codex) its isolated home, kept
    /// across turns so the provider can resume.
    pub runtime_dir: PathBuf,
    pub catalog: BrukCatalog,
    pub project_model_override: Option<String>,
    pub project_env: BTreeMap<String, String>,
    /// MCP servers the chat's agent chooses from, gathered when the turn starts.
    pub mcp: McpSources,
    pub claude_binary: Option<PathBuf>,
    pub claude_default_model: Option<String>,
    pub claude_settings: ClaudeSpawnSettings,
    pub codex_binary: Option<PathBuf>,
    pub codex_default_model: Option<String>,
    pub codex_settings: CodexSpawnSettings,
    pub source_codex_home: Option<PathBuf>,
    pub antigravity_binary: Option<PathBuf>,
    pub antigravity_default_model: Option<String>,
}

/// What a turn needs to create the chat's worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatWorktreeSetup {
    /// The repository the worktree belongs to.
    pub project_root: PathBuf,
    pub path: PathBuf,
    /// The new branch to ask for.
    pub branch: String,
}

/// Stops a running chat turn from another thread.
#[derive(Clone, Default)]
pub struct ChatTurnControl {
    shared: Arc<ChatTurnShared>,
}

#[derive(Default)]
struct ChatTurnShared {
    stopped: AtomicBool,
    child_pid: AtomicU32,
}

impl ChatTurnControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stops the turn, ending the provider process if it started.
    pub fn stop(&self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        let pid = self.shared.child_pid.swap(0, Ordering::SeqCst);
        if pid > 0 {
            terminate_pid(pid);
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.shared.stopped.load(Ordering::SeqCst)
    }

    fn set_child_pid(&self, pid: u32) {
        self.shared.child_pid.store(pid, Ordering::SeqCst);
        if self.is_stopped() {
            self.stop();
        }
    }

    fn clear_child_pid(&self) {
        self.shared.child_pid.store(0, Ordering::SeqCst);
    }
}

/// What a provider command is spawned for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatCommandPurpose {
    Turn,
    /// Claude only: answer a `get_context_usage` control request about the session.
    ContextQuery,
}

/// Builds the provider command for one chat turn. The message itself is written to the
/// command's stdin, so it is never read as a flag or a prompt template.
pub fn build_chat_turn_command(request: &ChatTurnRequest) -> Result<Command, ChatTurnError> {
    build_chat_command(request, ChatCommandPurpose::Turn).map(|(cmd, _, _)| cmd)
}

/// The command for one chat turn, what to write to its stdin, and the MCP servers it gets.
fn build_chat_command(
    request: &ChatTurnRequest,
    purpose: ChatCommandPurpose,
) -> Result<(Command, String, ResolvedMcpSet), ChatTurnError> {
    let attachments: Vec<ChatAttachment> = match purpose {
        ChatCommandPurpose::Turn => request
            .attachments
            .iter()
            .map(|upload| upload.attachment.clone())
            .collect(),
        ChatCommandPurpose::ContextQuery => Vec::new(),
    };
    let prompt = prompt_with_attachments(request.prompt.trim(), &attachments);
    // The agent reads attachments from the chat's folder, outside its working directory.
    let attachments_dir = attachments
        .first()
        .and_then(|a| a.path.parent())
        .map(|dir| dir.to_string_lossy().into_owned());
    let step = AgentTaskStepDef {
        id: "chat".to_owned(),
        name: "Chat".to_owned(),
        agent: request.agent.clone().filter(|a| !a.trim().is_empty()),
        provider: Some(request.provider),
        model: request.model.clone().filter(|m| !m.trim().is_empty()),
        skills: Vec::new(),
        prompt: String::new(),
        inputs: BTreeMap::new(),
        outputs: Vec::new(),
        mcp: Vec::new(),
    };
    let mut bundle = compile_step_context(
        &request.catalog,
        &step,
        &TemplateContext::default(),
        request.provider,
        request.sandbox,
        request.project_model_override.as_deref(),
        StepMcpScope {
            sources: &request.mcp,
            policy: McpSourcesPolicy::All,
            env: &request.project_env,
        },
    )?;
    // The access the user picked for this chat wins over the agent's default.
    bundle.provider = request.provider;
    bundle.sandbox = request.sandbox;
    let materialized = materialize_step_context(
        bundle,
        &request.runtime_dir,
        request.source_codex_home.as_deref(),
    )?;
    let resume = request
        .resume_session_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());

    match request.provider {
        ProviderKind::Claude => {
            let binary = request
                .claude_binary
                .as_ref()
                .ok_or(ChatTurnError::ProviderMissing(ProviderKind::Claude.label()))?;
            let mut plan = build_isolated_claude_spawn_plan(
                binary,
                request.claude_default_model.as_deref(),
                &request.claude_settings,
                &request.working_dir,
                &materialized,
                None,
                &request.project_env,
            )?;
            let mut stdin_text = prompt;
            match purpose {
                ChatCommandPurpose::Turn => {
                    if !plan.args.iter().any(|a| a == "--include-partial-messages") {
                        plan.args.push("--include-partial-messages".to_owned());
                    }
                    if let Some(dir) = attachments_dir.as_deref() {
                        plan.args.push(format!("--add-dir={dir}"));
                    }
                    // Images go in as image blocks of a stream-json message; plain text
                    // messages stay plain text.
                    let images = claude_inline_images(&attachments);
                    if !images.is_empty() {
                        stdin_text = claude_user_message_line(&stdin_text, &images)
                            .map_err(ChatTurnError::Attachment)?;
                        plan.args.push("--input-format".to_owned());
                        plan.args.push("stream-json".to_owned());
                    }
                    if let Some(id) = resume {
                        plan.args.push("--resume".to_owned());
                        plan.args.push(id.to_owned());
                    }
                }
                ChatCommandPurpose::ContextQuery => {
                    // Read the session through a fork that is never saved, so asking leaves no
                    // trace in the conversation.
                    plan.args.extend(
                        ["--input-format", "stream-json", "--resume"]
                            .into_iter()
                            .map(str::to_owned),
                    );
                    plan.args.push(resume.unwrap_or_default().to_owned());
                    plan.args.push("--fork-session".to_owned());
                    plan.args.push("--no-session-persistence".to_owned());
                }
            }
            Ok((
                plan.to_command(),
                stdin_text,
                materialized.bundle.mcp.clone(),
            ))
        }
        ProviderKind::Codex => {
            let binary = request
                .codex_binary
                .as_ref()
                .ok_or(ChatTurnError::ProviderMissing(ProviderKind::Codex.label()))?;
            let mut plan = build_isolated_codex_spawn_plan(
                binary,
                request.codex_default_model.as_deref(),
                &request.codex_settings,
                &request.working_dir,
                &materialized,
                &request.project_env,
            )?;
            if let Some(id) = resume {
                plan.args.push("resume".to_owned());
                plan.args.push(id.to_owned());
            }
            let images: Vec<&ChatAttachment> =
                attachments.iter().filter(|a| a.is_image()).collect();
            for image in &images {
                plan.args.push("--image".to_owned());
                plan.args.push(image.path.to_string_lossy().into_owned());
            }
            if !images.is_empty() {
                // `--image` takes several values: without `--` it would take `-` too.
                plan.args.push("--".to_owned());
            }
            // `-`: read the prompt from stdin.
            plan.args.push("-".to_owned());
            Ok((plan.to_command(), prompt, materialized.bundle.mcp.clone()))
        }
        ProviderKind::Antigravity => {
            let binary =
                request
                    .antigravity_binary
                    .as_ref()
                    .ok_or(ChatTurnError::ProviderMissing(
                        ProviderKind::Antigravity.label(),
                    ))?;
            let mut plan = build_isolated_antigravity_spawn_plan(
                binary,
                request.antigravity_default_model.as_deref(),
                &request.working_dir,
                &materialized,
                &request.project_env,
            );
            if let Some(dir) = attachments_dir.as_deref() {
                plan.args.push(format!("--add-dir={dir}"));
            }
            // The message goes in as one NDJSON line on stdin, so it's never read as a flag.
            plan.args.push("--input-format".to_owned());
            plan.args.push("stream-json".to_owned());
            let content = match resume {
                Some(id) => {
                    plan.args.push("--conversation".to_owned());
                    plan.args.push(id.to_owned());
                    prompt
                }
                // A new conversation starts with the agent's instructions.
                None => plan.first_message(&prompt),
            };
            let line = serde_json::json!({
                "event": "user",
                "message": { "role": "user", "content": content },
            });
            Ok((
                plan.to_command(),
                format!("{line}\n"),
                materialized.bundle.mcp.clone(),
            ))
        }
        ProviderKind::OpenRouter => Err(ChatTurnError::UnsupportedProvider(
            ProviderKind::OpenRouter.label(),
        )),
    }
}

/// Runs one chat turn on a background thread, returning its control and its events.
pub fn spawn_chat_turn(request: ChatTurnRequest) -> (ChatTurnControl, mpsc::Receiver<ChatEvent>) {
    let control = ChatTurnControl::new();
    let control_for_thread = control.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        execute_chat_turn_blocking(&request, &control_for_thread, &tx);
    });
    (control, rx)
}

/// Runs one chat turn on this thread, sending its events to `tx` from `TurnStarted` to
/// `TurnFinished`, and returns how it ended.
pub fn execute_chat_turn_blocking(
    request: &ChatTurnRequest,
    control: &ChatTurnControl,
    tx: &mpsc::Sender<ChatEvent>,
) -> ChatSessionStatus {
    let _ = tx.send(ChatEvent::TurnStarted);
    let (status, error) = match run_chat_turn(request, control, tx) {
        Ok(outcome) => outcome,
        Err(err) => (ChatSessionStatus::Failed, Some(err.to_string())),
    };
    let _ = tx.send(ChatEvent::TurnFinished { status, error });
    status
}

fn run_chat_turn(
    request: &ChatTurnRequest,
    control: &ChatTurnControl,
    tx: &mpsc::Sender<ChatEvent>,
) -> Result<(ChatSessionStatus, Option<String>), ChatTurnError> {
    if request.prompt.trim().is_empty() && request.attachments.is_empty() {
        return Err(ChatTurnError::EmptyMessage);
    }
    if control.is_stopped() {
        return Ok((ChatSessionStatus::Stopped, None));
    }
    if let Err(err) = stage_chat_attachments(&request.attachments) {
        return Ok((ChatSessionStatus::Failed, Some(err)));
    }
    if let Some(setup) = request.worktree.as_ref() {
        match ensure_chat_worktree(setup) {
            Ok(Some(branch)) => {
                let _ = tx.send(ChatEvent::WorktreeReady {
                    path: setup.path.clone(),
                    branch,
                });
            }
            Ok(None) => {}
            Err(err) => {
                return Ok((
                    ChatSessionStatus::Failed,
                    Some(format!("Couldn't create the chat's worktree: {err}")),
                ));
            }
        }
    }

    let (mut cmd, stdin_text, mcp) = build_chat_command(request, ChatCommandPurpose::Turn)?;
    for problem in &mcp.problems {
        let _ = tx.send(ChatEvent::McpProblem(problem.clone()));
    }
    if let Some(summary) = mcp.blocking_summary() {
        return Ok((
            ChatSessionStatus::Failed,
            Some(truncate_chars(
                &format!("This agent can't run without its MCP servers: {summary}"),
                ERROR_MAX_CHARS,
            )),
        ));
    }
    let mut mcp_tracker = McpFailureTracker::default();
    let mut mcp_blocking: Vec<String> = Vec::new();
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let provider_label = request.provider.label();
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            return Ok((
                ChatSessionStatus::Failed,
                Some(format!("Couldn't start the {provider_label} CLI: {err}")),
            ));
        }
    };
    control.set_child_pid(child.id());

    if let Some(mut stdin) = child.stdin.take() {
        std::thread::spawn(move || {
            let _ = stdin.write_all(stdin_text.as_bytes());
        });
    }
    let stderr_reader = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut raw = String::new();
            let _ = pipe.read_to_string(&mut raw);
            raw
        })
    });

    let mut saw_text = false;
    let mut result_error: Option<String> = None;
    let mut last_error: Option<String> = None;
    let mut stray_output: Vec<String> = Vec::new();
    // Codex streams no token counts; its rollout file records one after each model request.
    // Claude's requests of this turn: the finished ones' tokens and the latest one's.
    let mut finished_requests = ChatTokenBreakdown::default();
    let mut latest_request: Option<ChatTokenBreakdown> = None;
    let mut codex_live = (request.provider == ProviderKind::Codex).then(|| CodexLiveContext {
        thread_id: request.resume_session_id.clone(),
        rollout: None,
        last_read: None,
    });

    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if request.provider == ProviderKind::Claude
                && line.contains("\"mcp_servers\"")
                && let Ok(event) = serde_json::from_str::<Value>(&line)
            {
                for failure in claude_init_failures(&event) {
                    if let Some(problem) = mcp_tracker.record(&mcp, &failure) {
                        if problem.blocking {
                            mcp_blocking.push(problem.title());
                        }
                        let _ = tx.send(ChatEvent::McpProblem(problem));
                    }
                }
                if !mcp_blocking.is_empty() {
                    // A server the agent needs didn't start: stop before the model runs.
                    let _ = child.kill();
                    break;
                }
            }
            let items = match request.provider {
                ProviderKind::Claude => parse_claude_chat_line(&line),
                ProviderKind::Codex => parse_codex_chat_line(&line),
                ProviderKind::Antigravity => parse_antigravity_chat_line(&line),
                ProviderKind::OpenRouter => Vec::new(),
            };
            if items.is_empty() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('{') && stray_output.len() < 20 {
                    stray_output.push(trimmed.to_owned());
                }
                continue;
            }
            if let Some(live) = codex_live.as_mut() {
                if let Some(ChatStreamItem::SessionStarted(id)) = items
                    .iter()
                    .find(|item| matches!(item, ChatStreamItem::SessionStarted(_)))
                {
                    live.thread_id = Some(id.clone());
                }
                if let Some((usage, totals)) = live.read(&request.runtime_dir) {
                    let _ = tx.send(ChatEvent::ContextUsage(usage));
                    if let Some(tokens) = totals {
                        let _ = tx.send(ChatEvent::TokenUse {
                            tokens,
                            scope: TokenScope::SessionTotal,
                        });
                    }
                }
            }
            for item in items {
                let event = match item {
                    ChatStreamItem::SessionStarted(session_id) => {
                        ChatEvent::ProviderSessionStarted { session_id }
                    }
                    ChatStreamItem::ContextTokens(used_tokens) => {
                        ChatEvent::ContextTokens { used_tokens }
                    }
                    ChatStreamItem::RequestStarted(prompt) => {
                        if let Some(previous) = latest_request.replace(prompt) {
                            finished_requests = finished_requests.plus(previous);
                        }
                        ChatEvent::TokenUse {
                            tokens: finished_requests.plus(prompt),
                            scope: TokenScope::Turn,
                        }
                    }
                    ChatStreamItem::RequestOutput(output) => {
                        let Some(latest) = latest_request.as_mut() else {
                            continue;
                        };
                        latest.output = output;
                        ChatEvent::TokenUse {
                            tokens: finished_requests.plus(*latest),
                            scope: TokenScope::Turn,
                        }
                    }
                    ChatStreamItem::TokenUse { tokens, scope } => {
                        ChatEvent::TokenUse { tokens, scope }
                    }
                    ChatStreamItem::TextDelta(text) => {
                        saw_text = true;
                        ChatEvent::AssistantDelta { text }
                    }
                    ChatStreamItem::Text(text) => {
                        saw_text = true;
                        ChatEvent::AssistantMessage { text }
                    }
                    ChatStreamItem::ToolUse {
                        id,
                        verb,
                        target,
                        input,
                    } => ChatEvent::ToolStarted {
                        tool_id: id,
                        verb,
                        target,
                        input,
                    },
                    ChatStreamItem::ToolResult {
                        id,
                        is_error,
                        detail,
                        output,
                    } => ChatEvent::ToolFinished {
                        tool_id: id,
                        success: !is_error,
                        detail,
                        output,
                    },
                    ChatStreamItem::Reasoning(text) => ChatEvent::Reasoning { text },
                    ChatStreamItem::Result {
                        text,
                        is_error,
                        tokens,
                        cost_micros,
                    } => {
                        if tokens.value() > 0 || cost_micros.value() > 0 {
                            let _ = tx.send(ChatEvent::Usage {
                                tokens,
                                cost_micros,
                            });
                        }
                        let text = text.filter(|t| !t.trim().is_empty());
                        if is_error {
                            result_error = Some(
                                text.unwrap_or_else(|| format!("{provider_label} turn failed")),
                            );
                            continue;
                        }
                        match text {
                            Some(text) if !saw_text => {
                                saw_text = true;
                                ChatEvent::AssistantMessage { text }
                            }
                            _ => continue,
                        }
                    }
                    ChatStreamItem::Error(message) => {
                        last_error = Some(message);
                        continue;
                    }
                };
                let _ = tx.send(event);
            }
        }
    }

    let exit = child.wait();
    control.clear_child_pid();
    let raw_stderr = stderr_reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();

    // Codex reports servers that fail to start on stderr, Antigravity in its log.
    let mut late_failures = Vec::new();
    let mut stderr_lines = Vec::new();
    for line in raw_stderr.lines() {
        match codex_stderr_failure(line) {
            Some(failure) => late_failures.push(failure),
            None => stderr_lines.push(line),
        }
    }
    let stderr_text = stderr_lines.join("\n");
    if request.provider == ProviderKind::Antigravity
        && let Ok(log) = std::fs::read_to_string(request.runtime_dir.join(ANTIGRAVITY_LOG_FILE))
    {
        late_failures.extend(antigravity_log_failures(&log));
        // One log per turn, so the next turn doesn't report these again.
        let _ = std::fs::remove_file(request.runtime_dir.join(ANTIGRAVITY_LOG_FILE));
    }
    for failure in &late_failures {
        if let Some(problem) = mcp_tracker.record(&mcp, failure) {
            if problem.blocking {
                mcp_blocking.push(problem.title());
            }
            let _ = tx.send(ChatEvent::McpProblem(problem));
        }
    }

    if control.is_stopped() {
        return Ok((ChatSessionStatus::Stopped, None));
    }
    if mcp_blocking.is_empty() {
        let servers = mcp_tracker.healthy(&mcp);
        if !servers.is_empty() {
            let _ = tx.send(ChatEvent::McpServersReady(servers));
        }
    }
    if !mcp_blocking.is_empty() {
        return Ok((
            ChatSessionStatus::Failed,
            Some(truncate_chars(
                &format!(
                    "This agent lost MCP servers it needs: {}",
                    mcp_blocking.join("; ")
                ),
                ERROR_MAX_CHARS,
            )),
        ));
    }
    let exit_code = exit.ok().and_then(|status| status.code());
    if exit_code == Some(0) && result_error.is_none() {
        return Ok((ChatSessionStatus::Idle, None));
    }

    let message = result_error
        .or(last_error)
        .or_else(|| last_lines(&stderr_text, 6))
        .or_else(|| (!stray_output.is_empty()).then(|| stray_output.join("\n")))
        .unwrap_or_else(|| match exit_code {
            Some(code) => format!("{provider_label} exited with code {code}"),
            None => format!("{provider_label} was stopped by a signal"),
        });
    Ok((
        ChatSessionStatus::Failed,
        Some(truncate_chars(&message, ERROR_MAX_CHARS)),
    ))
}

/// How often a running Codex turn's rollout is read for its latest token count.
const CODEX_LIVE_CONTEXT_INTERVAL: Duration = Duration::from_millis(1500);

/// Follows a running Codex turn's context window through its rollout file.
struct CodexLiveContext {
    thread_id: Option<String>,
    rollout: Option<PathBuf>,
    last_read: Option<Instant>,
}

impl CodexLiveContext {
    /// The window as the rollout last recorded it, at most every
    /// [`CODEX_LIVE_CONTEXT_INTERVAL`].
    fn read(
        &mut self,
        runtime_dir: &Path,
    ) -> Option<(ChatContextUsage, Option<ChatTokenBreakdown>)> {
        if self
            .last_read
            .is_some_and(|at| at.elapsed() < CODEX_LIVE_CONTEXT_INTERVAL)
        {
            return None;
        }
        let thread_id = self.thread_id.as_deref()?;
        self.last_read = Some(Instant::now());
        if self.rollout.is_none() {
            let sessions = runtime_dir.join(ISOLATED_CODEX_HOME_DIR).join("sessions");
            self.rollout = find_codex_rollout(&sessions, thread_id);
        }
        let contents = std::fs::read_to_string(self.rollout.as_ref()?).ok()?;
        let mut usage = parse_codex_rollout_context(&contents, crate::current_unix_seconds())?;
        usage.live = true;
        Some((usage, parse_codex_rollout_token_totals(&contents)))
    }
}

/// Reads how the chat's context window is filled now. Claude Code answers a
/// `get_context_usage` request about a throwaway fork of the session (nothing is saved and no
/// model is called); Codex's own record of the thread holds its latest token count. Needs the
/// provider session (`resume_session_id`) of at least one finished turn.
pub fn query_chat_context(request: &ChatTurnRequest, now: i64) -> Result<ChatContextUsage, String> {
    let session_id = request
        .resume_session_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            "Send a message first: the context window fills as the chat goes.".to_owned()
        })?;
    match request.provider {
        ProviderKind::Claude => query_claude_context(request, now),
        ProviderKind::Codex => read_codex_context(&request.runtime_dir, session_id, now),
        ProviderKind::Antigravity => read_antigravity_chat_context(request, session_id, now),
        ProviderKind::OpenRouter => {
            Err(ChatTurnError::UnsupportedProvider(ProviderKind::OpenRouter.label()).to_string())
        }
    }
}

fn query_claude_context(request: &ChatTurnRequest, now: i64) -> Result<ChatContextUsage, String> {
    let (mut cmd, _, _) = build_chat_command(request, ChatCommandPurpose::ContextQuery)
        .map_err(|err| err.to_string())?;
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|err| format!("Couldn't start the Claude CLI: {err}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let request_line = serde_json::json!({
            "type": "control_request",
            "request_id": CONTEXT_REQUEST_ID,
            "request": { "subtype": "get_context_usage" },
        });
        // Dropping stdin ends the input: the CLI answers, then exits.
        let _ = writeln!(stdin, "{request_line}");
    }
    let (response_tx, response_rx) = mpsc::channel::<Value>();
    let stdout_reader = child.stdout.take().map(|stdout| {
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Ok(val) = serde_json::from_str::<Value>(line.trim()) else {
                    continue;
                };
                if val.get("type").and_then(Value::as_str) != Some("control_response") {
                    continue;
                }
                if let Some(response) = val.get("response").filter(|r| {
                    r.get("request_id").and_then(Value::as_str) == Some(CONTEXT_REQUEST_ID)
                }) {
                    let _ = response_tx.send(response.clone());
                    return;
                }
            }
        })
    });
    let stderr_reader = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut raw = String::new();
            let _ = pipe.read_to_string(&mut raw);
            raw
        })
    });

    // Wait for the answer, then give the CLI a moment to exit on its own.
    let deadline = Instant::now() + CONTEXT_QUERY_TIMEOUT;
    let mut response: Option<Value> = None;
    let mut exit_by = deadline;
    let mut timed_out = false;
    loop {
        if response.is_none()
            && let Ok(answer) = response_rx.try_recv()
        {
            response = Some(answer);
            exit_by = Instant::now() + Duration::from_secs(2);
        }
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => break,
            Ok(None) if Instant::now() >= exit_by => {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = response.is_none();
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
        }
    }
    if response.is_none() {
        response = response_rx.recv_timeout(Duration::from_millis(500)).ok();
    }
    if let Some(handle) = stdout_reader {
        let _ = handle.join();
    }
    let stderr_text = stderr_reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();

    match response {
        Some(answer) if answer.get("subtype").and_then(Value::as_str) == Some("success") => answer
            .get("response")
            .and_then(|report| parse_claude_context_usage(report, now))
            .ok_or_else(|| "Claude's context report was empty.".to_owned()),
        Some(answer) => Err(answer
            .get("error")
            .and_then(Value::as_str)
            .filter(|e| !e.trim().is_empty())
            .map(|e| truncate_chars(e, ERROR_MAX_CHARS))
            .unwrap_or_else(|| "Claude couldn't report its context window.".to_owned())),
        None if timed_out => Err("Claude took too long to report its context window.".to_owned()),
        None => Err(last_lines(&stderr_text, 4)
            .map(|text| truncate_chars(&text, ERROR_MAX_CHARS))
            .unwrap_or_else(|| {
                "This Claude CLI doesn't report its context window. Update Claude Code to see it."
                    .to_owned()
            })),
    }
}

/// Reads a Claude Code context report (the answer to `get_context_usage`, the data behind its
/// `/context` command).
pub fn parse_claude_context_usage(report: &Value, now: i64) -> Option<ChatContextUsage> {
    let categories: Vec<ChatContextCategory> = report
        .get("categories")?
        .as_array()?
        .iter()
        .filter_map(|category| {
            let name = string_field(category, "name").filter(|n| !n.trim().is_empty())?;
            let tokens = category.get("tokens").and_then(Value::as_u64).unwrap_or(0);
            let deferred = category.get("isDeferred").and_then(Value::as_bool) == Some(true);
            let kind = match category.get("kind").and_then(Value::as_str) {
                Some("used") => ChatContextKind::Used,
                Some("free") => ChatContextKind::Free,
                Some("buffer") => ChatContextKind::Buffer,
                Some("deferred") => ChatContextKind::Deferred,
                _ if deferred => ChatContextKind::Deferred,
                _ => match name.as_str() {
                    "Free space" => ChatContextKind::Free,
                    "Autocompact buffer" => ChatContextKind::Buffer,
                    _ => ChatContextKind::Used,
                },
            };
            let id = string_field(category, "id")
                .filter(|id| !id.trim().is_empty())
                .unwrap_or_else(|| name.to_lowercase().replace(' ', "_"));
            Some(ChatContextCategory {
                id,
                name,
                tokens,
                kind,
            })
        })
        .collect();
    let window_tokens = ["maxTokens", "rawMaxTokens"]
        .into_iter()
        .find_map(|key| report.get(key).and_then(Value::as_u64).filter(|n| *n > 0))
        .unwrap_or(0);
    let used_tokens = report
        .get("totalTokens")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            categories
                .iter()
                .filter(|c| c.kind == ChatContextKind::Used)
                .map(|c| c.tokens)
                .sum()
        });

    let mut groups = Vec::new();
    push_context_group(
        &mut groups,
        "Memory files",
        report.get("memoryFiles"),
        |item| string_field(item, "path").map(|path| home_relative(&path)),
    );
    push_context_group(&mut groups, "Agents", report.get("agents"), |item| {
        string_field(item, "agentType")
    });
    push_context_group(
        &mut groups,
        "Skills",
        report.get("skills").and_then(|s| s.get("skillFrontmatter")),
        |item| string_field(item, "name"),
    );
    push_context_group(&mut groups, "MCP tools", report.get("mcpTools"), |item| {
        let name = string_field(item, "name")?;
        Some(match string_field(item, "serverName") {
            Some(server) if !server.trim().is_empty() => format!("{server} · {name}"),
            _ => name,
        })
    });

    let autocompact_at =
        if report.get("isAutoCompactEnabled").and_then(Value::as_bool) == Some(false) {
            None
        } else {
            report
                .get("autoCompactThreshold")
                .and_then(Value::as_u64)
                .filter(|n| *n > 0)
        };
    Some(ChatContextUsage {
        model: string_field(report, "model").filter(|m| !m.trim().is_empty()),
        used_tokens,
        window_tokens,
        categories,
        groups,
        autocompact_at,
        has_breakdown: true,
        live: false,
        captured_at: now,
    })
}

fn push_context_group(
    groups: &mut Vec<ChatContextGroup>,
    title: &str,
    list: Option<&Value>,
    name_of: impl Fn(&Value) -> Option<String>,
) {
    let Some(list) = list.and_then(Value::as_array) else {
        return;
    };
    let mut items: Vec<ChatContextItem> = list
        .iter()
        .filter_map(|item| {
            Some(ChatContextItem {
                name: name_of(item).filter(|n| !n.trim().is_empty())?,
                tokens: item.get("tokens").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .collect();
    if items.is_empty() {
        return;
    }
    items.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.name.cmp(&b.name)));
    let total_items = items.len();
    items.truncate(CONTEXT_GROUP_MAX_ITEMS);
    groups.push(ChatContextGroup {
        title: title.to_owned(),
        items,
        total_items,
    });
}

/// `path` with the home folder written as `~`.
fn home_relative(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path.starts_with(&home) => {
            format!("~{}", &path[home.len()..])
        }
        _ => path.to_owned(),
    }
}

/// Antigravity reports a conversation's tokens and window size when ACP loads it.
fn read_antigravity_chat_context(
    request: &ChatTurnRequest,
    conversation_id: &str,
    now: i64,
) -> Result<ChatContextUsage, String> {
    let binary = request
        .antigravity_binary
        .as_ref()
        .ok_or_else(|| ChatTurnError::ProviderMissing(ProviderKind::Antigravity.label()))
        .map_err(|err| err.to_string())?;
    let reading = crate::antigravity_cli::read_antigravity_context(
        binary,
        &request.working_dir,
        conversation_id,
        &[],
        &request.project_env,
    )?;
    Ok(ChatContextUsage {
        model: request
            .model
            .clone()
            .or_else(|| request.antigravity_default_model.clone())
            .filter(|m| !m.trim().is_empty()),
        used_tokens: reading.used_tokens,
        window_tokens: reading.window_tokens,
        categories: vec![
            ChatContextCategory {
                id: "conversation".to_owned(),
                name: "Instructions, tools and history".to_owned(),
                tokens: reading.used_tokens,
                kind: ChatContextKind::Used,
            },
            ChatContextCategory {
                id: "free_space".to_owned(),
                name: "Free space".to_owned(),
                tokens: reading.window_tokens.saturating_sub(reading.used_tokens),
                kind: ChatContextKind::Free,
            },
        ],
        groups: Vec::new(),
        autocompact_at: None,
        has_breakdown: false,
        live: false,
        captured_at: now,
    })
}

fn read_codex_context(
    runtime_dir: &Path,
    thread_id: &str,
    now: i64,
) -> Result<ChatContextUsage, String> {
    let sessions = runtime_dir.join(ISOLATED_CODEX_HOME_DIR).join("sessions");
    let rollout = find_codex_rollout(&sessions, thread_id)
        .ok_or_else(|| "Codex hasn't recorded this conversation yet.".to_owned())?;
    let contents = std::fs::read_to_string(&rollout)
        .map_err(|err| format!("Couldn't read Codex's record of this chat: {err}"))?;
    parse_codex_rollout_context(&contents, now)
        .ok_or_else(|| "Codex hasn't reported a token count for this chat yet.".to_owned())
}

/// The newest `rollout-*.jsonl` of `thread_id` under Codex's `sessions/YYYY/MM/DD/`.
fn find_codex_rollout(sessions_dir: &Path, thread_id: &str) -> Option<PathBuf> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    let mut pending = vec![(sessions_dir.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                if depth < 4 {
                    pending.push((entry.path(), depth + 1));
                }
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("rollout-") && name.ends_with(".jsonl") && name.contains(thread_id)
            {
                let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                if newest.as_ref().is_none_or(|(seen, _)| modified > *seen) {
                    newest = Some((modified, entry.path()));
                }
            }
        }
    }
    newest.map(|(_, path)| path)
}

/// The thread's running token totals by kind, from the latest token count in a Codex rollout
/// file.
pub fn parse_codex_rollout_token_totals(contents: &str) -> Option<ChatTokenBreakdown> {
    contents.lines().rev().find_map(|line| {
        let val = serde_json::from_str::<Value>(line.trim()).ok()?;
        let payload = val.get("payload")?;
        if val.get("type").and_then(Value::as_str) != Some("event_msg")
            || payload.get("type").and_then(Value::as_str) != Some("token_count")
        {
            return None;
        }
        let totals = payload.get("info")?.get("total_token_usage")?;
        Some(codex_token_breakdown(Some(totals))).filter(|b| !b.is_zero())
    })
}

/// Reads the latest token count in a Codex rollout file. Codex reports how much of the window
/// the last request filled, not what filled it.
pub fn parse_codex_rollout_context(contents: &str, now: i64) -> Option<ChatContextUsage> {
    let mut info: Option<Value> = None;
    let mut model: Option<String> = None;
    for line in contents.lines() {
        let Ok(val) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let Some(payload) = val.get("payload") else {
            continue;
        };
        match val.get("type").and_then(Value::as_str) {
            Some("event_msg")
                if payload.get("type").and_then(Value::as_str) == Some("token_count") =>
            {
                if let Some(found) = payload.get("info").filter(|i| !i.is_null()) {
                    info = Some(found.clone());
                }
            }
            Some("turn_context") => {
                if let Some(name) = string_field(payload, "model").filter(|m| !m.is_empty()) {
                    model = Some(name);
                }
            }
            _ => {}
        }
    }
    let info = info?;
    let last = info.get("last_token_usage")?;
    let count = |key: &str| last.get(key).and_then(Value::as_u64).unwrap_or(0);
    let input = count("input_tokens");
    let used_tokens = Some(count("total_tokens"))
        .filter(|total| *total > 0)
        .unwrap_or_else(|| input + count("output_tokens"));
    let window_tokens = info
        .get("model_context_window")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut categories = vec![
        ChatContextCategory {
            id: "conversation".to_owned(),
            name: "Instructions, tools and history".to_owned(),
            tokens: input.min(used_tokens),
            kind: ChatContextKind::Used,
        },
        ChatContextCategory {
            id: "latest_reply".to_owned(),
            name: "Latest reply".to_owned(),
            tokens: used_tokens.saturating_sub(input),
            kind: ChatContextKind::Used,
        },
    ];
    if window_tokens > used_tokens {
        categories.push(ChatContextCategory {
            id: "free_space".to_owned(),
            name: "Free space".to_owned(),
            tokens: window_tokens - used_tokens,
            kind: ChatContextKind::Free,
        });
    }
    Some(ChatContextUsage {
        model,
        used_tokens,
        window_tokens,
        categories,
        groups: Vec::new(),
        autocompact_at: None,
        has_breakdown: false,
        live: false,
        captured_at: now,
    })
}

/// The branch a new worktree chat asks for: the project's branch prefix, `chat-`, and the
/// chat's title as a slug (its id when the title has no letters or digits).
pub fn chat_worktree_branch(prefix: &str, title: &str, chat_id: &str) -> String {
    let prefix = match prefix.trim() {
        "" => "agent/",
        prefix => prefix,
    };
    let mut slug = String::new();
    for ch in title.chars() {
        if slug.len() >= BRANCH_SLUG_MAX_CHARS {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() {
        chat_id.trim_start_matches("chat-")
    } else {
        slug
    };
    format!("{prefix}chat-{slug}")
}

/// Creates the chat's worktree on a new branch from `HEAD` unless it exists. Returns the branch
/// when it created one: a name already taken gets a number (`-2`, `-3`, …).
fn ensure_chat_worktree(setup: &ChatWorktreeSetup) -> Result<Option<String>, String> {
    if setup.path.join(".git").exists() {
        return Ok(None);
    }
    if std::fs::read_dir(&setup.path).is_ok_and(|mut entries| entries.next().is_some()) {
        return Err(format!(
            "{} already exists and isn't a worktree",
            setup.path.display()
        ));
    }
    if git_in(&setup.project_root, &["rev-parse", "--verify", "HEAD"]).is_err() {
        return Err(
            "the project isn't a git repository with a commit yet, so it can't have worktrees"
                .to_owned(),
        );
    }
    if let Some(parent) = setup.path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("couldn't create {}: {err}", parent.display()))?;
    }
    let path = setup.path.to_string_lossy().into_owned();
    let mut last_error = String::new();
    for attempt in 1..=5 {
        let branch = if attempt == 1 {
            setup.branch.clone()
        } else {
            format!("{}-{attempt}", setup.branch)
        };
        match git_in(
            &setup.project_root,
            &["worktree", "add", "-b", &branch, &path, "HEAD"],
        ) {
            Ok(_) => return Ok(Some(branch)),
            Err(err) if err.contains("a branch named") => last_error = err,
            Err(err) => return Err(err),
        }
    }
    Err(last_error)
}

/// Removes a chat's worktree folder and git's record of it. The branch, and any commits on it,
/// stay.
pub fn remove_chat_worktree(project_root: &Path, path: &Path) -> Result<(), String> {
    if path.exists() {
        let path_arg = path.to_string_lossy().into_owned();
        if git_in(project_root, &["worktree", "remove", "--force", &path_arg]).is_ok() {
            return Ok(());
        }
        std::fs::remove_dir_all(path)
            .map_err(|err| format!("Couldn't remove {}: {err}", path.display()))?;
    }
    let _ = git_in(project_root, &["worktree", "prune"]);
    Ok(())
}

/// Runs git in `dir`, returning its output, or its last error lines.
fn git_in(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
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

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_owned()
    } else {
        let mut cut: String = trimmed.chars().take(max_chars).collect();
        cut.push('…');
        cut
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    use tempfile::TempDir;

    use super::*;

    fn session() -> ChatSession {
        ChatSession::new(
            "chat-1",
            1,
            "demo",
            ProviderKind::Claude,
            Some("implementer".to_owned()),
            Some("Implementer".to_owned()),
            ProjectSandboxMode::WorkspaceWrite,
            100,
        )
    }

    #[test]
    fn tool_steps_keep_their_whole_input_and_output() {
        assert_eq!(keep_full_text("line 1\nline 2\n"), "line 1\nline 2");
        let long: String = "a".repeat(TOOL_FULL_MAX_CHARS) + &"b".repeat(10);
        let kept = keep_full_text(&long);
        assert!(kept.starts_with(&"a".repeat(TOOL_FULL_MAX_CHARS / 2)));
        assert!(kept.ends_with(&"b".repeat(10)));
        assert!(kept.contains("[… 10 characters not kept …]"));

        assert_eq!(
            format_tool_input(&serde_json::json!({
                "file_path": "src/a.rs",
                "old_string": "fn a() {\n}",
                "replace_all": false,
            })),
            "file_path: src/a.rs\nold_string:\nfn a() {\n}\nreplace_all: false"
        );

        // Reasoning that lands while a reply streams goes before the reply.
        let mut chat = session();
        chat.apply_event(
            &ChatEvent::AssistantDelta {
                text: "Hi".to_owned(),
            },
            101,
        );
        chat.apply_event(
            &ChatEvent::Reasoning {
                text: "  think  ".to_owned(),
            },
            101,
        );
        assert_eq!(
            chat.entries,
            vec![
                ChatEntry::Reasoning {
                    text: "think".to_owned()
                },
                ChatEntry::Assistant {
                    text: "Hi".to_owned(),
                    streaming: true,
                },
            ]
        );
    }

    #[test]
    fn parses_claude_stream_json_for_chat_turns() {
        assert_eq!(
            parse_claude_chat_line(
                r#"{"type":"system","subtype":"init","session_id":"abc-123","tools":[]}"#
            ),
            vec![ChatStreamItem::SessionStarted("abc-123".to_owned())]
        );
        assert_eq!(
            parse_claude_chat_line(
                r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}}"#
            ),
            vec![ChatStreamItem::TextDelta("Hel".to_owned())]
        );
        assert_eq!(
            parse_claude_chat_line(
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Looking."},{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls -la\nwc -l","description":"List"}}]}}"#
            ),
            vec![
                ChatStreamItem::Text("Looking.".to_owned()),
                ChatStreamItem::ToolUse {
                    id: Some("toolu_1".to_owned()),
                    verb: "Bash".to_owned(),
                    target: "ls -la …".to_owned(),
                    input: "command:\nls -la\nwc -l\ndescription: List".to_owned(),
                },
            ]
        );
        assert_eq!(
            parse_claude_chat_line(
                r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","is_error":true,"content":[{"type":"text","text":"No such file\nmore"}]}]}}"#
            ),
            vec![ChatStreamItem::ToolResult {
                id: Some("toolu_1".to_owned()),
                is_error: true,
                detail: "No such file …".to_owned(),
                output: "No such file\nmore".to_owned(),
            }]
        );
        assert_eq!(
            parse_claude_chat_line(
                r#"{"type":"result","subtype":"success","is_error":false,"result":"Done","total_cost_usd":0.042,"usage":{"input_tokens":300,"output_tokens":50}}"#
            ),
            vec![
                ChatStreamItem::TokenUse {
                    tokens: ChatTokenBreakdown {
                        input: 300,
                        output: 50,
                        ..ChatTokenBreakdown::default()
                    },
                    scope: TokenScope::Turn,
                },
                ChatStreamItem::Result {
                    text: Some("Done".to_owned()),
                    is_error: false,
                    tokens: UsageCount::Turn(350),
                    cost_micros: UsageCount::SessionTotal(42_000),
                }
            ]
        );
        // Per-turn figures, when Claude Code gives them, beat its session-wide cost.
        assert_eq!(
            parse_claude_chat_line(
                r#"{"type":"result","subtype":"success","is_error":false,"result":"two","total_cost_usd":0.0724,"usage":{"input_tokens":4,"cache_creation_input_tokens":14,"cache_read_input_tokens":17631,"output_tokens":3},"model_usage_since_last_result":{"claude-sonnet-5-5":{"inputTokens":4,"outputTokens":3,"cacheReadInputTokens":17631,"cacheCreationInputTokens":14,"costUSD":0.0018571},"claude-haiku-5-5":{"inputTokens":100,"outputTokens":20,"costUSD":0.0001}}}"#
            ),
            vec![
                ChatStreamItem::TokenUse {
                    tokens: ChatTokenBreakdown {
                        input: 104,
                        output: 23,
                        cache_read: 17_631,
                        cache_write: 14,
                    },
                    scope: TokenScope::Turn,
                },
                ChatStreamItem::Result {
                    text: Some("two".to_owned()),
                    is_error: false,
                    tokens: UsageCount::Turn(17_772),
                    cost_micros: UsageCount::Turn(1_957),
                }
            ]
        );
        assert_eq!(
            parse_claude_chat_line(
                r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#
            ),
            vec![ChatStreamItem::Result {
                text: Some("error max turns".to_owned()),
                is_error: true,
                tokens: UsageCount::Turn(0),
                cost_micros: UsageCount::SessionTotal(0),
            }]
        );
        assert_eq!(
            parse_claude_chat_line(
                r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"Check the config first.","signature":"x"},{"type":"thinking","thinking":""}]}}"#
            ),
            vec![ChatStreamItem::Reasoning(
                "Check the config first.".to_owned()
            )]
        );
        // A subagent's messages belong to its Task tool call, not the chat.
        assert!(
            parse_claude_chat_line(
                r#"{"type":"assistant","parent_tool_use_id":"toolu_9","message":{"content":[{"type":"text","text":"sub"}]}}"#
            )
            .is_empty()
        );
        assert!(parse_claude_chat_line("not json").is_empty());
    }

    #[test]
    fn parses_codex_exec_json_for_chat_turns() {
        assert_eq!(
            parse_codex_chat_line(r#"{"type":"thread.started","thread_id":"0199-thread"}"#),
            vec![ChatStreamItem::SessionStarted("0199-thread".to_owned())]
        );
        let started = ChatStreamItem::ToolUse {
            id: Some("item_1".to_owned()),
            verb: "Shell".to_owned(),
            target: "bash -lc 'cargo test'".to_owned(),
            input: "bash -lc 'cargo test'".to_owned(),
        };
        assert_eq!(
            parse_codex_chat_line(
                r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"bash -lc 'cargo test'","status":"in_progress"}}"#
            ),
            vec![started.clone()]
        );
        assert_eq!(
            parse_codex_chat_line(
                r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"bash -lc 'cargo test'","aggregated_output":"boom","exit_code":101,"status":"failed"}}"#
            ),
            vec![
                started,
                ChatStreamItem::ToolResult {
                    id: Some("item_1".to_owned()),
                    is_error: true,
                    detail: "exit 101".to_owned(),
                    output: "boom\n[exit code 101]".to_owned(),
                },
            ]
        );
        assert_eq!(
            parse_codex_chat_line(
                r#"{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"All green."}}"#
            ),
            vec![ChatStreamItem::Text("All green.".to_owned())]
        );
        assert_eq!(
            parse_codex_chat_line(
                r#"{"type":"turn.completed","usage":{"input_tokens":1000,"cached_input_tokens":600,"output_tokens":20}}"#
            ),
            vec![
                ChatStreamItem::TokenUse {
                    tokens: ChatTokenBreakdown {
                        input: 400,
                        output: 20,
                        cache_read: 600,
                        cache_write: 0,
                    },
                    scope: TokenScope::SessionTotal,
                },
                ChatStreamItem::Result {
                    text: None,
                    is_error: false,
                    tokens: UsageCount::SessionTotal(1020),
                    cost_micros: UsageCount::Turn(0),
                }
            ]
        );
        assert_eq!(
            parse_codex_chat_line(r#"{"type":"turn.failed","error":{"message":"quota"}}"#),
            vec![ChatStreamItem::Error("quota".to_owned())]
        );
    }

    #[test]
    fn session_folds_streamed_text_tools_and_turn_outcomes() {
        let mut chat = session();
        assert_eq!(chat.display_title(), UNTITLED_CHAT_TITLE);
        chat.push_user_message("  Fix the flaky   retry test\nin billing  ", 101);
        assert_eq!(chat.title, "Fix the flaky retry test …");
        assert_eq!(chat.message_count(), 1);

        chat.apply_event(&ChatEvent::TurnStarted, 102);
        assert!(chat.is_running());
        chat.apply_event(
            &ChatEvent::ProviderSessionStarted {
                session_id: "sess-1".to_owned(),
            },
            102,
        );
        for piece in ["Let me ", "look"] {
            chat.apply_event(
                &ChatEvent::AssistantDelta {
                    text: piece.to_owned(),
                },
                103,
            );
        }
        assert_eq!(
            chat.entries.last(),
            Some(&ChatEntry::Assistant {
                text: "Let me look".to_owned(),
                streaming: true,
            })
        );
        chat.apply_event(
            &ChatEvent::AssistantMessage {
                text: "Let me look.".to_owned(),
            },
            104,
        );
        // Codex reports a tool when it starts and again when it completes.
        for _ in 0..2 {
            chat.apply_event(
                &ChatEvent::ToolStarted {
                    tool_id: Some("t1".to_owned()),
                    verb: "Bash".to_owned(),
                    target: "cargo test".to_owned(),
                    input: "cargo test --workspace".to_owned(),
                },
                105,
            );
        }
        chat.apply_event(
            &ChatEvent::ToolStarted {
                tool_id: None,
                verb: "Read".to_owned(),
                target: "src/lib.rs".to_owned(),
                input: String::new(),
            },
            105,
        );
        chat.apply_event(
            &ChatEvent::ToolFinished {
                tool_id: Some("t1".to_owned()),
                success: false,
                detail: "1 failed".to_owned(),
                output: "running 2 tests\ntest a ... ok\ntest b ... FAILED\n".to_owned(),
            },
            106,
        );
        let added = chat.apply_event(
            &ChatEvent::Usage {
                tokens: UsageCount::Turn(500),
                cost_micros: UsageCount::Turn(30_000),
            },
            107,
        );
        assert_eq!(
            added,
            ChatUsageDelta {
                tokens: 500,
                cost_micros: 30_000,
            }
        );
        chat.apply_event(
            &ChatEvent::TurnFinished {
                status: ChatSessionStatus::Idle,
                error: None,
            },
            108,
        );

        assert_eq!(chat.status, ChatSessionStatus::Idle);
        assert_eq!(chat.provider_session_id.as_deref(), Some("sess-1"));
        assert_eq!((chat.total_tokens, chat.total_cost_cents()), (500, 3));
        assert_eq!(chat.updated_at, 108);
        assert_eq!(
            chat.entries,
            vec![
                ChatEntry::User {
                    text: "Fix the flaky   retry test\nin billing".to_owned(),
                    sent_at: 101,
                    attachments: Vec::new(),
                },
                ChatEntry::Assistant {
                    text: "Let me look.".to_owned(),
                    streaming: false,
                },
                ChatEntry::Tool {
                    tool_id: Some("t1".to_owned()),
                    verb: "Bash".to_owned(),
                    target: "cargo test".to_owned(),
                    status: ChatToolStatus::Failed,
                    detail: "1 failed".to_owned(),
                    input: "cargo test --workspace".to_owned(),
                    output: "running 2 tests\ntest a ... ok\ntest b ... FAILED".to_owned(),
                },
                ChatEntry::Tool {
                    tool_id: None,
                    verb: "Read".to_owned(),
                    target: "src/lib.rs".to_owned(),
                    status: ChatToolStatus::Succeeded,
                    detail: String::new(),
                    input: String::new(),
                    output: String::new(),
                },
            ]
        );
        assert_eq!(chat.preview(), "Let me look.");

        // A failed turn leaves an error notice and settles tools still running as failed.
        chat.push_user_message("Try again", 110);
        chat.apply_event(&ChatEvent::TurnStarted, 110);
        chat.apply_event(
            &ChatEvent::ToolStarted {
                tool_id: Some("t2".to_owned()),
                verb: "Bash".to_owned(),
                target: "make".to_owned(),
                input: "make".to_owned(),
            },
            111,
        );
        chat.apply_event(
            &ChatEvent::TurnFinished {
                status: ChatSessionStatus::Failed,
                error: Some("rate limited".to_owned()),
            },
            112,
        );
        assert_eq!(chat.status, ChatSessionStatus::Failed);
        assert!(matches!(
            chat.entries[chat.entries.len() - 2],
            ChatEntry::Tool {
                status: ChatToolStatus::Failed,
                ..
            }
        ));
        assert_eq!(
            chat.entries.last(),
            Some(&ChatEntry::Notice {
                text: "rate limited".to_owned(),
                tone: ChatNoticeTone::Error,
            })
        );
        assert_eq!(
            chat.title, "Fix the flaky retry test …",
            "the first message names the chat"
        );
    }

    #[test]
    fn interrupted_turns_settle_when_reloaded() {
        let mut chat = session();
        chat.push_user_message("Hello", 1);
        assert!(!chat.mark_interrupted(2));
        chat.apply_event(&ChatEvent::TurnStarted, 2);
        assert!(chat.mark_interrupted(3));
        assert_eq!(chat.status, ChatSessionStatus::Stopped);
        assert!(matches!(
            chat.entries.last(),
            Some(ChatEntry::Notice {
                tone: ChatNoticeTone::Error,
                ..
            })
        ));

        let json = serde_json::to_string(&chat).unwrap();
        let back: ChatSession = serde_json::from_str(&json).unwrap();
        assert_eq!(back, chat);
    }

    #[test]
    fn session_totals_count_each_turn_once() {
        let mut chat = session();
        let usage = |tokens: u64, cost_micros: u64| ChatEvent::Usage {
            tokens: UsageCount::SessionTotal(tokens),
            cost_micros: UsageCount::SessionTotal(cost_micros),
        };
        chat.apply_event(
            &ChatEvent::ProviderSessionStarted {
                session_id: "thread-1".to_owned(),
            },
            1,
        );
        assert_eq!(chat.apply_event(&usage(1_000, 70_000), 2).tokens, 1_000);
        // The next turn resumes the session: its totals include the first turn.
        let second = chat.apply_event(&usage(2_500, 72_000), 3);
        assert_eq!(
            second,
            ChatUsageDelta {
                tokens: 1_500,
                cost_micros: 2_000,
            }
        );
        // Resuming the same session keeps counting from its totals.
        chat.apply_event(
            &ChatEvent::ProviderSessionStarted {
                session_id: "thread-1".to_owned(),
            },
            4,
        );
        assert_eq!(chat.apply_event(&usage(2_600, 72_000), 5).tokens, 100);
        // A new provider session counts from zero.
        chat.apply_event(
            &ChatEvent::ProviderSessionStarted {
                session_id: "thread-2".to_owned(),
            },
            6,
        );
        assert_eq!(chat.apply_event(&usage(300, 1_000), 7).tokens, 300);
        assert_eq!((chat.total_tokens, chat.total_cost_micros), (2_900, 73_000));
        assert_eq!(chat.total_cost_cents(), 7);

        // A context reading doesn't move the chat up the list.
        let before = chat.updated_at;
        chat.apply_event(
            &ChatEvent::ContextUsage(ChatContextUsage {
                model: None,
                used_tokens: 1,
                window_tokens: 10,
                categories: Vec::new(),
                groups: Vec::new(),
                autocompact_at: None,
                has_breakdown: true,
                live: false,
                captured_at: 99,
            }),
            99,
        );
        assert_eq!(chat.updated_at, before);
        assert_eq!(
            chat.context.as_ref().map(ChatContextUsage::used_percent),
            Some(10)
        );
    }

    #[test]
    fn claude_streams_how_full_the_window_is() {
        assert_eq!(
            parse_claude_chat_line(
                r#"{"type":"stream_event","event":{"type":"message_start","message":{"usage":{"input_tokens":5,"cache_creation_input_tokens":100,"cache_read_input_tokens":4000,"output_tokens":1}}}}"#
            ),
            vec![
                ChatStreamItem::ContextTokens(4_106),
                ChatStreamItem::RequestStarted(ChatTokenBreakdown {
                    input: 5,
                    output: 1,
                    cache_read: 4_000,
                    cache_write: 100,
                })
            ]
        );
        assert_eq!(
            parse_claude_chat_line(
                r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42}}}"#
            ),
            vec![ChatStreamItem::RequestOutput(42)]
        );
        assert_eq!(
            parse_claude_chat_line(
                r#"{"type":"assistant","message":{"usage":{"input_tokens":5,"cache_read_input_tokens":4100,"output_tokens":60},"content":[{"type":"text","text":"Hi"}]}}"#
            ),
            vec![
                ChatStreamItem::ContextTokens(4_165),
                ChatStreamItem::Text("Hi".to_owned())
            ]
        );
        // A subagent's requests fill its own window, not the chat's.
        assert!(
            parse_claude_chat_line(
                r#"{"type":"stream_event","parent_tool_use_id":"toolu_1","event":{"type":"message_start","message":{"usage":{"input_tokens":900}}}}"#
            )
            .is_empty()
        );
    }

    #[test]
    fn token_use_follows_the_running_turn_and_settles_when_it_ends() {
        let mut chat = ChatSession::new(
            "c1",
            1,
            "repo",
            ProviderKind::Codex,
            None,
            None,
            ProjectSandboxMode::default(),
            10,
        );
        let tokens = |input, output, cache_read| ChatTokenBreakdown {
            input,
            output,
            cache_read,
            cache_write: 0,
        };
        let finish = ChatEvent::TurnFinished {
            status: ChatSessionStatus::Idle,
            error: None,
        };
        chat.apply_event(&ChatEvent::TurnStarted, 11);
        chat.apply_event(
            &ChatEvent::TokenUse {
                tokens: tokens(100, 10, 0),
                scope: TokenScope::SessionTotal,
            },
            11,
        );
        assert_eq!(chat.token_use(), tokens(100, 10, 0));
        chat.apply_event(&finish, 12);
        assert_eq!(chat.token_breakdown, tokens(100, 10, 0));
        assert!(chat.turn_tokens.is_zero());

        // A session's running totals count only what the new turn added, as it goes.
        chat.apply_event(&ChatEvent::TurnStarted, 13);
        for (seen, total) in [(tokens(150, 20, 80), 250), (tokens(200, 30, 300), 530)] {
            chat.apply_event(
                &ChatEvent::TokenUse {
                    tokens: seen,
                    scope: TokenScope::SessionTotal,
                },
                13,
            );
            assert_eq!(chat.token_use().total(), total);
        }
        chat.apply_event(&finish, 14);
        assert_eq!(chat.token_breakdown, tokens(200, 30, 300));
    }

    #[test]
    fn live_token_counts_grow_the_conversation_and_shrink_free_space() {
        let mut chat = ChatSession::new(
            "c1".to_owned(),
            1,
            "repo".to_owned(),
            ProviderKind::Claude,
            None,
            None,
            ProjectSandboxMode::default(),
            10,
        );
        // Before any measurement the total is all there is.
        chat.apply_event(&ChatEvent::ContextTokens { used_tokens: 900 }, 11);
        let first = chat.context.clone().unwrap();
        assert_eq!((first.used_tokens, first.window_tokens), (900, 0));
        assert!(first.live);

        chat.apply_event(
            &ChatEvent::ContextUsage(ChatContextUsage {
                model: None,
                used_tokens: 4_000,
                window_tokens: 200_000,
                categories: vec![
                    ChatContextCategory {
                        id: "system_prompt".to_owned(),
                        name: "System prompt".to_owned(),
                        tokens: 3_000,
                        kind: ChatContextKind::Used,
                    },
                    ChatContextCategory {
                        id: "messages".to_owned(),
                        name: "Messages".to_owned(),
                        tokens: 1_000,
                        kind: ChatContextKind::Used,
                    },
                    ChatContextCategory {
                        id: "autocompact_buffer".to_owned(),
                        name: "Autocompact buffer".to_owned(),
                        tokens: 33_000,
                        kind: ChatContextKind::Buffer,
                    },
                    ChatContextCategory {
                        id: "free_space".to_owned(),
                        name: "Free space".to_owned(),
                        tokens: 163_000,
                        kind: ChatContextKind::Free,
                    },
                ],
                groups: Vec::new(),
                autocompact_at: None,
                has_breakdown: true,
                live: false,
                captured_at: 12,
            }),
            12,
        );
        chat.apply_event(&ChatEvent::ContextTokens { used_tokens: 6_500 }, 13);
        let live = chat.context.clone().unwrap();
        let tokens: Vec<(&str, u64)> = live
            .categories
            .iter()
            .map(|c| (c.id.as_str(), c.tokens))
            .collect();
        assert_eq!(
            tokens,
            vec![
                ("system_prompt", 3_000),
                ("messages", 3_500),
                ("autocompact_buffer", 33_000),
                ("free_space", 160_500),
            ]
        );
        assert_eq!((live.used_tokens, live.captured_at), (6_500, 13));
        assert!(live.live && live.has_breakdown);
    }

    #[test]
    fn parses_claude_context_reports() {
        let report: Value = serde_json::from_str(
            r#"{"categories":[
                {"id":"system_prompt","name":"System prompt","tokens":1854,"color":"promptBorder","kind":"used"},
                {"id":"system_tools","name":"System tools","tokens":363,"kind":"used"},
                {"id":"system_tools_deferred","name":"System tools (deferred)","tokens":5702,"isDeferred":true,"kind":"deferred"},
                {"name":"Memory files","tokens":420},
                {"id":"messages","name":"Messages","tokens":9475,"kind":"used"},
                {"id":"autocompact_buffer","name":"Autocompact buffer","tokens":33000,"kind":"buffer"},
                {"id":"free_space","name":"Free space","tokens":949351,"kind":"free"}],
              "totalTokens":12112,"maxTokens":1000000,"rawMaxTokens":1000000,"percentage":1,
              "model":"claude-sonnet-5-5",
              "memoryFiles":[{"path":"/repo/CLAUDE.md","type":"Project","tokens":420}],
              "mcpTools":[{"name":"search","serverName":"docs","tokens":90}],
              "agents":[],
              "skills":{"totalSkills":2,"includedSkills":2,"tokens":30,"skillFrontmatter":[{"name":"small","tokens":10},{"name":"big","tokens":20}]},
              "autoCompactThreshold":967000,"isAutoCompactEnabled":true}"#,
        )
        .unwrap();
        let usage = parse_claude_context_usage(&report, 50).unwrap();
        assert_eq!(usage.model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(
            (usage.used_tokens, usage.window_tokens),
            (12_112, 1_000_000)
        );
        assert_eq!(usage.used_percent(), 1);
        assert_eq!(usage.autocompact_at, Some(967_000));
        assert!(usage.has_breakdown);
        let kinds: Vec<(&str, ChatContextKind)> = usage
            .categories
            .iter()
            .map(|c| (c.id.as_str(), c.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("system_prompt", ChatContextKind::Used),
                ("system_tools", ChatContextKind::Used),
                ("system_tools_deferred", ChatContextKind::Deferred),
                ("memory_files", ChatContextKind::Used),
                ("messages", ChatContextKind::Used),
                ("autocompact_buffer", ChatContextKind::Buffer),
                ("free_space", ChatContextKind::Free),
            ]
        );
        let groups: Vec<(&str, Vec<&str>)> = usage
            .groups
            .iter()
            .map(|g| {
                (
                    g.title.as_str(),
                    g.items.iter().map(|i| i.name.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            groups,
            vec![
                ("Memory files", vec!["/repo/CLAUDE.md"]),
                ("Skills", vec!["big", "small"]),
                ("MCP tools", vec!["docs · search"]),
            ]
        );
        assert!(parse_claude_context_usage(&serde_json::json!({}), 0).is_none());
    }

    #[test]
    fn parses_codex_rollout_token_counts() {
        let rollout = r#"{"timestamp":"t","type":"session_meta","payload":{"id":"thread-7"}}
{"timestamp":"t","type":"turn_context","payload":{"cwd":"/repo","model":"gpt-5.5"}}
{"timestamp":"t","type":"event_msg","payload":{"type":"token_count","info":null}}
{"timestamp":"t","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":9000,"cached_input_tokens":4000,"output_tokens":300,"total_tokens":9300},"last_token_usage":{"input_tokens":5000,"cached_input_tokens":4000,"output_tokens":200,"reasoning_output_tokens":50,"total_tokens":5200},"model_context_window":272000}}}
{"timestamp":"t","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{}}}"#;
        let usage = parse_codex_rollout_context(rollout, 9).unwrap();
        assert_eq!(usage.model.as_deref(), Some("gpt-5.5"));
        assert_eq!((usage.used_tokens, usage.window_tokens), (5_200, 272_000));
        assert!(!usage.has_breakdown);
        let slices: Vec<(&str, u64, ChatContextKind)> = usage
            .categories
            .iter()
            .map(|c| (c.id.as_str(), c.tokens, c.kind))
            .collect();
        assert_eq!(
            slices,
            vec![
                ("conversation", 5_000, ChatContextKind::Used),
                ("latest_reply", 200, ChatContextKind::Used),
                ("free_space", 266_800, ChatContextKind::Free),
            ]
        );
        assert!(parse_codex_rollout_context("{}\n", 0).is_none());
    }

    #[test]
    fn worktree_branches_come_from_the_title() {
        assert_eq!(
            chat_worktree_branch("agent/", "Fix the flaky retry test …", "chat-9-1"),
            "agent/chat-fix-the-flaky-retry-test"
        );
        assert_eq!(
            chat_worktree_branch("", "Übersicht?!", "chat-9-1"),
            "agent/chat-bersicht"
        );
        assert_eq!(
            chat_worktree_branch("bruk-", "???", "chat-9-1"),
            "bruk-chat-9-1"
        );
        assert!(
            chat_worktree_branch("agent/", &"word ".repeat(40), "chat-1").len()
                <= "agent/chat-".len() + BRANCH_SLUG_MAX_CHARS
        );
    }

    fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn request(temp: &TempDir, provider: ProviderKind, binary: &Path) -> ChatTurnRequest {
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        ChatTurnRequest {
            session_id: "chat-1".to_owned(),
            provider,
            agent: Some("implementer".to_owned()),
            model: None,
            sandbox: ProjectSandboxMode::ReadOnly,
            resume_session_id: None,
            prompt: "Explain {{ inputs.x }} -- please".to_owned(),
            attachments: Vec::new(),
            working_dir: repo.clone(),
            worktree: None,
            runtime_dir: temp.path().join("chats").join("chat-1"),
            catalog: BrukCatalog::load_for_project(&repo),
            project_model_override: None,
            project_env: BTreeMap::new(),
            mcp: Default::default(),
            claude_binary: (provider == ProviderKind::Claude).then(|| binary.to_path_buf()),
            claude_default_model: None,
            claude_settings: ClaudeSpawnSettings::default(),
            codex_binary: (provider == ProviderKind::Codex).then(|| binary.to_path_buf()),
            codex_default_model: None,
            codex_settings: CodexSpawnSettings::default(),
            source_codex_home: None,
            antigravity_binary: (provider == ProviderKind::Antigravity)
                .then(|| binary.to_path_buf()),
            antigravity_default_model: None,
        }
    }

    fn run_turn(request: &ChatTurnRequest, session: &mut ChatSession) -> ChatSessionStatus {
        let (tx, rx) = mpsc::channel();
        let status = execute_chat_turn_blocking(request, &ChatTurnControl::new(), &tx);
        drop(tx);
        for event in rx {
            session.apply_event(&event, 0);
        }
        status
    }

    #[test]
    fn claude_turns_pipe_the_message_stream_replies_and_resume_the_session() {
        let temp = TempDir::new().unwrap();
        let log = temp.path().join("calls.log");
        let script = format!(
            r#"#!/bin/sh
echo "ARGS: $*" >> '{log}'
echo "STDIN: $(cat)" >> '{log}'
echo '{{"type":"system","subtype":"init","session_id":"claude-sess-1"}}'
echo '{{"type":"stream_event","event":{{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"Hi "}}}}}}'
echo '{{"type":"assistant","message":{{"content":[{{"type":"text","text":"Hi there."}}]}}}}'
echo '{{"type":"result","subtype":"success","is_error":false,"result":"Hi there.","total_cost_usd":0.01,"usage":{{"input_tokens":10,"output_tokens":5}}}}'
"#,
            log = log.display()
        );
        let claude = write_script(temp.path(), "claude", &script);
        let mut req = request(&temp, ProviderKind::Claude, &claude);
        let mut chat = session();
        chat.push_user_message(&req.prompt, 0);

        assert_eq!(run_turn(&req, &mut chat), ChatSessionStatus::Idle);
        assert_eq!(chat.provider_session_id.as_deref(), Some("claude-sess-1"));
        assert_eq!(
            chat.entries.last(),
            Some(&ChatEntry::Assistant {
                text: "Hi there.".to_owned(),
                streaming: false,
            })
        );
        assert_eq!((chat.total_tokens, chat.total_cost_cents()), (15, 1));

        req.resume_session_id = chat.provider_session_id.clone();
        req.prompt = "And then?".to_owned();
        assert_eq!(run_turn(&req, &mut chat), ChatSessionStatus::Idle);
        // The fake reports the same session-wide cost again: the second turn was free.
        assert_eq!((chat.total_tokens, chat.total_cost_micros), (30, 10_000));

        let calls = fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = calls.lines().collect();
        assert_eq!(lines.len(), 4, "{calls}");
        for args in [lines[0], lines[2]] {
            for flag in [
                "--print",
                "--output-format stream-json",
                "--include-partial-messages",
                "--setting-sources project",
                "--strict-mcp-config",
                "--permission-mode plan",
                "--append-system-prompt-file",
            ] {
                assert!(args.contains(flag), "missing {flag} in {args}");
            }
            assert!(
                !args.contains("Explain"),
                "the message goes on stdin: {args}"
            );
        }
        assert!(!lines[0].contains("--resume"));
        assert!(lines[2].ends_with("--resume claude-sess-1"), "{}", lines[2]);
        // Messages reach the agent verbatim: no template rendering, no flag parsing.
        assert_eq!(lines[1], "STDIN: Explain {{ inputs.x }} -- please");
        assert_eq!(lines[3], "STDIN: And then?");
        assert!(
            temp.path()
                .join("chats/chat-1/step-instructions.md")
                .is_file()
        );
    }

    #[test]
    fn codex_turns_resume_the_thread_in_an_isolated_home() {
        let temp = TempDir::new().unwrap();
        let log = temp.path().join("calls.log");
        let script = format!(
            r#"#!/bin/sh
echo "ARGS: $* HOME=$CODEX_HOME STDIN=$(cat)" >> '{log}'
turns=$(grep -c ARGS '{log}')
echo '{{"type":"thread.started","thread_id":"codex-thread-7"}}'
echo '{{"type":"item.completed","item":{{"id":"item_0","type":"agent_message","text":"Done."}}}}'
echo "{{\"type\":\"turn.completed\",\"usage\":{{\"input_tokens\":$((40 * turns)),\"output_tokens\":$((2 * turns))}}}}"
"#,
            log = log.display()
        );
        let codex = write_script(temp.path(), "codex", &script);
        let mut req = request(&temp, ProviderKind::Codex, &codex);
        // No agent: its multi-line developer instructions would split the one-line call log.
        req.agent = None;
        req.prompt = "Hello codex".to_owned();
        let mut chat = session();
        chat.provider = ProviderKind::Codex;

        assert_eq!(run_turn(&req, &mut chat), ChatSessionStatus::Idle);
        req.resume_session_id = chat.provider_session_id.clone();
        assert_eq!(run_turn(&req, &mut chat), ChatSessionStatus::Idle);

        let calls = fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = calls.lines().collect();
        assert_eq!(lines.len(), 2, "{calls}");
        let home = temp.path().join("chats/chat-1/codex-home");
        for line in &lines {
            assert!(line.starts_with("ARGS: exec "), "{line}");
            assert!(line.contains("sandbox_mode=\"read-only\""), "{line}");
            assert!(line.contains(&format!("HOME={}", home.display())), "{line}");
            assert!(line.ends_with("STDIN=Hello codex"), "{line}");
        }
        assert!(lines[0].contains(" --json ") && lines[0].contains(" - HOME="));
        assert!(
            lines[1].contains(" resume codex-thread-7 - HOME="),
            "{}",
            lines[1]
        );
        // Codex reports the thread's running total (42, then 84): each turn counts once.
        assert_eq!(chat.total_tokens, 84);
    }

    fn attach(temp: &TempDir, req: &mut ChatTurnRequest, files: &[(&str, &[u8])]) {
        let sources: Vec<PathBuf> = files
            .iter()
            .map(|(name, bytes)| {
                let path = temp.path().join("drops").join(name);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, bytes).unwrap();
                path
            })
            .collect();
        req.attachments =
            crate::chat_attachment::plan_chat_attachments(&req.runtime_dir, &sources).unwrap();
    }

    #[test]
    fn claude_turns_send_attached_images_as_blocks_and_list_every_file() {
        let temp = TempDir::new().unwrap();
        let log = temp.path().join("calls.log");
        let script = format!(
            r#"#!/bin/sh
echo "ARGS: $*" >> '{log}'
printf 'STDIN: %s\n' "$(cat)" >> '{log}'
echo '{{"type":"result","subtype":"success","is_error":false,"result":"A cat."}}'
"#,
            log = log.display()
        );
        let claude = write_script(temp.path(), "claude", &script);
        let mut req = request(&temp, ProviderKind::Claude, &claude);
        req.prompt = "What is this?".to_owned();
        attach(
            &temp,
            &mut req,
            &[("cat.png", &[1, 2, 3]), ("notes.pdf", b"%PDF")],
        );
        let mut chat = session();

        assert_eq!(run_turn(&req, &mut chat), ChatSessionStatus::Idle);
        let dir = temp.path().join("chats/chat-1/attachments");
        assert_eq!(fs::read(dir.join("cat.png")).unwrap(), [1, 2, 3]);
        assert!(dir.join("notes.pdf").is_file());

        let calls = fs::read_to_string(&log).unwrap();
        let args = calls.lines().next().unwrap();
        assert!(args.contains("--input-format stream-json"), "{args}");
        assert!(
            args.contains(&format!("--add-dir={}", dir.display())),
            "{args}"
        );
        let stdin = calls
            .lines()
            .nth(1)
            .unwrap()
            .strip_prefix("STDIN: ")
            .unwrap();
        let message: Value = serde_json::from_str(stdin).unwrap();
        let content = &message["message"]["content"];
        let text = content[0]["text"].as_str().unwrap();
        assert!(
            text.starts_with("What is this?\n\nI attached files."),
            "{text}"
        );
        assert!(text.contains(&dir.join("notes.pdf").display().to_string()));
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["data"], "AQID");
        assert_eq!(
            content.as_array().unwrap().len(),
            2,
            "PDFs are read by path"
        );
    }

    #[test]
    fn codex_turns_pass_attached_images_with_image_flags() {
        let temp = TempDir::new().unwrap();
        let log = temp.path().join("calls.log");
        let script = format!(
            r#"#!/bin/sh
echo "ARGS: $* STDIN=$(cat | tr '\n' ' ')" >> '{log}'
echo '{{"type":"thread.started","thread_id":"t-1"}}'
echo '{{"type":"item.completed","item":{{"id":"i","type":"agent_message","text":"Ok."}}}}'
"#,
            log = log.display()
        );
        let codex = write_script(temp.path(), "codex", &script);
        let mut req = request(&temp, ProviderKind::Codex, &codex);
        req.agent = None;
        req.prompt = String::new();
        req.resume_session_id = Some("t-0".to_owned());
        attach(
            &temp,
            &mut req,
            &[("ui.jpg", b"jpg"), ("build.log", b"oops")],
        );
        let mut chat = session();
        chat.provider = ProviderKind::Codex;

        assert_eq!(run_turn(&req, &mut chat), ChatSessionStatus::Idle);
        let dir = temp.path().join("chats/chat-1/attachments");
        let line = fs::read_to_string(&log).unwrap();
        let image = dir.join("ui.jpg");
        assert!(
            line.contains(&format!(
                " resume t-0 --image {} -- - STDIN=",
                image.display()
            )),
            "{line}"
        );
        assert_eq!(
            line.matches("--image").count(),
            1,
            "only images get --image: {line}"
        );
        assert!(
            line.contains(&dir.join("build.log").display().to_string()),
            "{line}"
        );
    }

    #[test]
    fn antigravity_turns_send_ndjson_and_resume_the_conversation() {
        let temp = TempDir::new().unwrap();
        let log = temp.path().join("calls.log");
        let script = format!(
            r#"#!/bin/sh
echo "ARGS: $* UPDATE=$AGY_CLI_DISABLE_AUTO_UPDATE STDIN=$(cat)" >> '{log}'
echo '{{"event":"init","conversation_id":"agy-conv-3","init":{{"model":"gemini-3.8-flash-low"}}}}'
echo '{{"event":"step_update","step_update":{{"step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"Do"}}}}'
echo '{{"event":"step_update","step_update":{{"step_index":1,"state":"DONE","step_type":"agent_response","text_delta":"ne."}}}}'
echo '{{"event":"result","result":{{"conversation_id":"agy-conv-3","status":"SUCCESS","response":"Done.","usage":{{"input_tokens":30,"output_tokens":2,"thinking_tokens":1,"cache_read_tokens":10}}}}}}'
"#,
            log = log.display()
        );
        let agy = write_script(temp.path(), "agy", &script);
        let mut req = request(&temp, ProviderKind::Antigravity, &agy);
        req.agent = None;
        req.prompt = "-- hello agy".to_owned();
        let mut chat = session();
        chat.provider = ProviderKind::Antigravity;

        assert_eq!(run_turn(&req, &mut chat), ChatSessionStatus::Idle);
        assert_eq!(chat.provider_session_id.as_deref(), Some("agy-conv-3"));
        req.resume_session_id = chat.provider_session_id.clone();
        assert_eq!(run_turn(&req, &mut chat), ChatSessionStatus::Idle);

        let calls = fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = calls.lines().collect();
        assert_eq!(lines.len(), 2, "{calls}");
        for line in &lines {
            assert!(line.contains("--output-format stream-json"), "{line}");
            assert!(line.contains("--input-format stream-json"), "{line}");
            assert!(
                line.contains(
                    r#"--settings {"permissions":{"deny":["write_file(*)","command(*)","mcp(*)"]}}"#
                ),
                "{line}"
            );
            assert!(line.contains("--sandbox"), "{line}");
            assert!(line.contains("--disable-slash-commands"), "{line}");
            assert!(line.contains("UPDATE=true"), "{line}");
            assert!(
                line.ends_with(
                    r#"STDIN={"event":"user","message":{"content":"-- hello agy","role":"user"}}"#
                ),
                "{line}"
            );
        }
        assert!(!lines[0].contains("--conversation"), "{}", lines[0]);
        assert!(
            lines[1].contains("--conversation agy-conv-3"),
            "{}",
            lines[1]
        );
        // Each result counts that turn alone: (30 + 10 + 2 + 1) twice.
        assert_eq!(chat.total_tokens, 86);
        assert!(chat.entries.iter().any(|e| matches!(
            e,
            ChatEntry::Assistant { text, .. } if text == "Done."
        )));
    }

    #[test]
    fn parses_antigravity_stream_json_for_chat_turns() {
        assert_eq!(
            parse_antigravity_chat_line(
                r#"{"event":"step_update","step_update":{"step_index":5,"state":"ACTIVE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"cargo test","Cwd":"/repo"}}}}"#
            ),
            vec![ChatStreamItem::ToolUse {
                id: Some("agy-step-5".to_owned()),
                verb: "Shell".to_owned(),
                target: "cargo test".to_owned(),
                input: "CommandLine: cargo test\nCwd: /repo".to_owned(),
            }]
        );
        assert_eq!(
            parse_antigravity_chat_line(
                r#"{"event":"step_update","step_update":{"step_index":6,"state":"DONE","step_type":"tool","tool_name":"read_file","tool_info":{"name":"read_file","parameters":{"Path":"/repo/f.txt"},"output":"1 lines, 6 bytes"}}}"#
            ),
            vec![
                ChatStreamItem::ToolUse {
                    id: Some("agy-step-6".to_owned()),
                    verb: "Read".to_owned(),
                    target: "/repo/f.txt".to_owned(),
                    input: "Path: /repo/f.txt".to_owned(),
                },
                ChatStreamItem::ToolResult {
                    id: Some("agy-step-6".to_owned()),
                    is_error: false,
                    detail: "1 lines, 6 bytes".to_owned(),
                    output: "1 lines, 6 bytes".to_owned(),
                },
            ]
        );
        assert_eq!(
            parse_antigravity_chat_line(
                r#"{"event":"result","result":{"status":"ERROR","response":"","error":"quota exhausted","usage":{"input_tokens":0,"output_tokens":0}}}"#
            ),
            vec![ChatStreamItem::Result {
                text: Some("quota exhausted".to_owned()),
                is_error: true,
                tokens: UsageCount::Turn(0),
                cost_micros: UsageCount::Turn(0),
            }]
        );
        assert!(parse_antigravity_chat_line("not json").is_empty());
    }

    fn git_repo(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("README.md"), "demo\n").unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["add", "."],
            vec!["commit", "-m", "initial"],
        ] {
            let status = Command::new("git")
                .args(&args)
                .current_dir(dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        }
    }

    #[test]
    fn worktree_chats_get_their_own_branch_and_folder() {
        let temp = TempDir::new().unwrap();
        let log = temp.path().join("calls.log");
        let claude = write_script(
            temp.path(),
            "claude",
            &format!(
                "#!/bin/sh\ncat > /dev/null\necho \"CWD: $(pwd -P)\" >> '{}'\necho '{{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s-1\"}}'\n",
                log.display()
            ),
        );
        let mut req = request(&temp, ProviderKind::Claude, &claude);
        let repo = req.working_dir.clone();
        git_repo(&repo);
        // Someone already has the branch the title asks for.
        let status = Command::new("git")
            .args(["branch", "agent/chat-explain"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());

        let worktree = req.runtime_dir.join("worktree");
        req.working_dir = worktree.clone();
        req.worktree = Some(ChatWorktreeSetup {
            project_root: repo.clone(),
            path: worktree.clone(),
            branch: "agent/chat-explain".to_owned(),
        });
        let mut chat = session();
        assert_eq!(run_turn(&req, &mut chat), ChatSessionStatus::Idle);
        assert_eq!(
            chat.worktree,
            Some(ChatWorktree {
                path: worktree.clone(),
                branch: "agent/chat-explain-2".to_owned(),
            })
        );
        assert!(worktree.join("README.md").is_file());
        // Later turns reuse it.
        assert_eq!(run_turn(&req, &mut chat), ChatSessionStatus::Idle);
        let real = fs::canonicalize(&worktree).unwrap();
        let calls = fs::read_to_string(&log).unwrap();
        assert_eq!(
            calls.lines().collect::<Vec<_>>(),
            vec![format!("CWD: {}", real.display()); 2]
        );

        // Removing the worktree keeps its branch.
        remove_chat_worktree(&repo, &worktree).unwrap();
        assert!(!worktree.exists());
        let branches = git_in(&repo, &["branch", "--list", "agent/chat-explain-2"]).unwrap();
        assert!(branches.contains("agent/chat-explain-2"), "{branches}");
        let listed = git_in(&repo, &["worktree", "list"]).unwrap();
        assert_eq!(listed.lines().count(), 1, "{listed}");

        // A project that isn't a git repository can't have one.
        let plain = temp.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        req.worktree = Some(ChatWorktreeSetup {
            project_root: plain.clone(),
            path: temp.path().join("other-worktree"),
            branch: "agent/chat-x".to_owned(),
        });
        req.working_dir = plain;
        let mut chat = session();
        assert_eq!(run_turn(&req, &mut chat), ChatSessionStatus::Failed);
        assert!(matches!(
            chat.entries.last(),
            Some(ChatEntry::Notice { text, .. }) if text.contains("isn't a git repository")
        ));
    }

    #[test]
    fn claude_context_comes_from_an_unsaved_fork_of_the_session() {
        let temp = TempDir::new().unwrap();
        let log = temp.path().join("calls.log");
        let script = format!(
            r#"#!/bin/sh
echo "ARGS: $*" >> '{log}'
input=$(cat)
echo "STDIN: $input" >> '{log}'
case "$input" in
  *get_context_usage*)
    echo '{{"type":"control_response","response":{{"subtype":"success","request_id":"bruk_context_usage","response":{{"categories":[{{"id":"messages","name":"Messages","tokens":700,"kind":"used"}},{{"id":"free_space","name":"Free space","tokens":199300,"kind":"free"}}],"totalTokens":700,"maxTokens":200000}}}}}}'
    ;;
esac
"#,
            log = log.display()
        );
        let claude = write_script(temp.path(), "claude", &script);
        let mut req = request(&temp, ProviderKind::Claude, &claude);
        assert!(query_chat_context(&req, 5).is_err(), "no session yet");

        req.resume_session_id = Some("sess-42".to_owned());
        let usage = query_chat_context(&req, 5).unwrap();
        assert_eq!((usage.used_tokens, usage.window_tokens), (700, 200_000));
        assert_eq!(usage.captured_at, 5);

        let calls = fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = calls.lines().collect();
        assert_eq!(lines.len(), 2, "{calls}");
        for flag in [
            "--input-format stream-json",
            "--resume sess-42",
            "--fork-session",
            "--no-session-persistence",
            "--setting-sources project",
            "--append-system-prompt-file",
        ] {
            assert!(lines[0].contains(flag), "missing {flag} in {}", lines[0]);
        }
        assert!(!lines[0].contains("--include-partial-messages"));
        assert!(
            lines[1].contains(r#""subtype":"get_context_usage""#),
            "{}",
            lines[1]
        );

        // A CLI that doesn't know the request says nothing back.
        let silent = write_script(temp.path(), "old-claude", "#!/bin/sh\ncat > /dev/null\n");
        req.claude_binary = Some(silent);
        let err = query_chat_context(&req, 5).unwrap_err();
        assert!(err.contains("Update Claude Code"), "{err}");
    }

    #[test]
    fn codex_context_comes_from_the_threads_rollout() {
        let temp = TempDir::new().unwrap();
        let codex = write_script(temp.path(), "codex", "#!/bin/sh\n");
        let mut req = request(&temp, ProviderKind::Codex, &codex);
        req.resume_session_id = Some("019a-thread".to_owned());
        assert!(query_chat_context(&req, 1).is_err(), "nothing recorded yet");

        let day = req.runtime_dir.join("codex-home/sessions/2026/11/23");
        fs::create_dir_all(&day).unwrap();
        fs::write(
            day.join("rollout-2026-11-23T10-00-00-019a-thread.jsonl"),
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":900,"output_tokens":100,"total_tokens":1000},"model_context_window":272000}}}"#,
        )
        .unwrap();
        fs::write(
            day.join("rollout-2026-11-23T09-00-00-other-thread.jsonl"),
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"total_tokens":5},"model_context_window":5}}}"#,
        )
        .unwrap();
        let usage = query_chat_context(&req, 1).unwrap();
        assert_eq!((usage.used_tokens, usage.window_tokens), (1_000, 272_000));
    }

    #[test]
    fn failed_turns_report_the_provider_error() {
        let temp = TempDir::new().unwrap();
        let claude = write_script(
            temp.path(),
            "claude",
            "#!/bin/sh\ncat > /dev/null\necho 'Invalid API key' >&2\nexit 1\n",
        );
        let req = request(&temp, ProviderKind::Claude, &claude);
        let mut chat = session();
        assert_eq!(run_turn(&req, &mut chat), ChatSessionStatus::Failed);
        assert_eq!(
            chat.entries.last(),
            Some(&ChatEntry::Notice {
                text: "Invalid API key".to_owned(),
                tone: ChatNoticeTone::Error,
            })
        );

        let mut missing = request(&temp, ProviderKind::Claude, &claude);
        missing.claude_binary = None;
        let mut chat = session();
        assert_eq!(run_turn(&missing, &mut chat), ChatSessionStatus::Failed);
        assert!(matches!(
            chat.entries.last(),
            Some(ChatEntry::Notice { text, .. }) if text.contains("Claude CLI isn't set up")
        ));
    }

    #[test]
    fn stopping_a_turn_ends_the_provider_process() {
        let temp = TempDir::new().unwrap();
        let claude = write_script(
            temp.path(),
            "claude",
            "#!/bin/sh\ncat > /dev/null\necho '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s\"}'\nexec sleep 30\n",
        );
        let req = request(&temp, ProviderKind::Claude, &claude);
        let control = ChatTurnControl::new();
        let (tx, rx) = mpsc::channel();
        let thread_control = control.clone();
        let handle =
            std::thread::spawn(move || execute_chat_turn_blocking(&req, &thread_control, &tx));
        // Wait until the provider is running, then stop it.
        assert_eq!(rx.recv().unwrap(), ChatEvent::TurnStarted);
        assert!(matches!(
            rx.recv().unwrap(),
            ChatEvent::ProviderSessionStarted { .. }
        ));
        let started = std::time::Instant::now();
        control.stop();
        assert_eq!(handle.join().unwrap(), ChatSessionStatus::Stopped);
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        assert_eq!(
            rx.recv().unwrap(),
            ChatEvent::TurnFinished {
                status: ChatSessionStatus::Stopped,
                error: None,
            }
        );
    }
}
