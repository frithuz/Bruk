//! Domain model for the Inbox window and macOS menu bar attention surface.
//!
//! Every waiting request (permission prompt, agent question, review gate, or
//! exhausted loop) lives in one shared [`InboxState`] so answering it in the
//! menu bar popover, the Inbox window, or a workflow run clears it everywhere.

use serde::{Deserialize, Serialize};

/// Filter tabs shown in the Inbox window toolbar (`All`, `Permissions`, `Questions`, `Reviews`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboxFilter {
    #[default]
    All,
    Permissions,
    Questions,
    Reviews,
}

impl InboxFilter {
    pub const ALL: &'static [Self] =
        &[Self::All, Self::Permissions, Self::Questions, Self::Reviews];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Permissions => "Permissions",
            Self::Questions => "Questions",
            Self::Reviews => "Reviews",
        }
    }

    pub fn matches(self, kind: AttentionRequestKind) -> bool {
        match self {
            Self::All => true,
            Self::Permissions => kind == AttentionRequestKind::Permission,
            Self::Questions => kind == AttentionRequestKind::Question,
            Self::Reviews => kind == AttentionRequestKind::Review,
        }
    }
}

/// Kind of blocking attention request surfaced in the Inbox and menu bar item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionRequestKind {
    Permission,
    Question,
    Review,
    LoopExhausted,
}

impl AttentionRequestKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Permission => "Permission",
            Self::Question => "Question",
            Self::Review => "Review",
            Self::LoopExhausted => "Loop",
        }
    }
}

/// Agent identity badge displayed in the Inbox detail view and menu bar rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentBadge {
    Codex,
    Claude,
}

impl AgentBadge {
    pub fn short_code(self) -> &'static str {
        match self {
            Self::Codex => "Cx",
            Self::Claude => "Cl",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
        }
    }
}

/// Risk/scope pill shown beneath a command in a permission request card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRiskTag {
    pub label: String,
    pub is_warning: bool,
    pub has_globe: bool,
}

/// Details for a shell command or tool permission request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionDetail {
    pub machine_name: String,
    pub working_dir: String,
    pub command: String,
    pub risk_tags: Vec<PermissionRiskTag>,
    /// Low-risk commands show quick inline `Allow` / `Deny` buttons in the menu bar popover.
    /// Risky commands (network access, package install, destructive ops) only show `Review...`.
    pub is_low_risk: bool,
    pub always_allow_workflow: String,
    pub always_allow_pattern: String,
    pub always_allow_note: String,
}

/// Suggested answer option for an agent question request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub recommended: bool,
}

/// Details for an open question asked by an agent mid-step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionDetail {
    pub prompt: String,
    pub options: Vec<QuestionOption>,
    pub default_if_unanswered: String,
}

/// File summary shown inside a review gate request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFileStat {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
}

/// Details for a human review gate before opening a PR or merging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewDetail {
    pub summary: String,
    pub branch: String,
    pub base_branch: String,
    pub tests_summary: String,
    pub files: Vec<ReviewFileStat>,
}

/// One attempt entry inside a loop-exhausted escalation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopAttemptSummary {
    pub attempt: u32,
    pub summary: String,
    pub failed: bool,
}

/// Details for a loop step that exhausted its configured attempt budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopExhaustedDetail {
    pub attempts_used: u32,
    pub max_attempts: u32,
    pub last_error: String,
    pub attempts: Vec<LoopAttemptSummary>,
}

/// Variant payload specific to an [`AttentionRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AttentionRequestBody {
    Permission(PermissionDetail),
    Question(QuestionDetail),
    Review(ReviewDetail),
    LoopExhausted(LoopExhaustedDetail),
}

/// Scope for saving a new `Always Allow` rule from the Inbox permission pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlwaysAllowScope {
    Workflow,
    Project,
    Global,
}

impl AlwaysAllowScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Workflow => "This workflow",
            Self::Project => "This project",
            Self::Global => "All projects",
        }
    }
}

/// A single blocking agent request waiting on the operator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionRequest {
    pub id: String,
    pub kind: AttentionRequestKind,
    pub agent: AgentBadge,
    pub project_name: String,
    pub workflow_name: String,
    /// Human context segment shown in breadcrumbs (e.g. `Dependency upgrade`, `Fix issue #318`).
    pub context_label: String,
    pub run_number: u32,
    pub machine_name: String,
    pub inbox_title: String,
    pub inbox_subtitle: String,
    pub menubar_title: String,
    pub menubar_subtitle: String,
    /// Minutes this request has been waiting (`0` formats as `now`).
    pub waiting_minutes: u32,
    /// Once past its reminder time, the menu bar count turns orange and the row dot turns amber.
    pub past_reminder_threshold: bool,
    pub status_pill: String,
    pub why_text: String,
    pub deadline_badge: Option<String>,
    pub timeout_footer: String,
    pub note_placeholder: String,
    pub body: AttentionRequestBody,
}

impl AttentionRequest {
    /// Breadcrumb string shown in Inbox rows and detail header:
    /// `api-gateway › Dependency upgrade › run #44 · build-box-01`.
    pub fn breadcrumb(&self) -> String {
        format!(
            "{} › {} › run #{} · {}",
            self.project_name, self.context_label, self.run_number, self.machine_name
        )
    }

    /// Compact waiting age shown on the right edge of Inbox list rows (`25m`, `3m`, `now`).
    pub fn compact_age(&self) -> String {
        if self.waiting_minutes == 0 {
            "now".to_owned()
        } else if self.waiting_minutes < 60 {
            format!("{}m", self.waiting_minutes)
        } else {
            let h = self.waiting_minutes / 60;
            let m = self.waiting_minutes % 60;
            if m == 0 {
                format!("{h}h")
            } else {
                format!("{h}h {m}m")
            }
        }
    }

    /// Waiting age formatted for the menu bar popover subtitle (`now`, `3 min`, `18 min`).
    pub fn menubar_age(&self) -> String {
        if self.waiting_minutes == 0 {
            "now".to_owned()
        } else {
            format!("{} min", self.waiting_minutes)
        }
    }

    /// Full subtitle line inside the menu bar popover row.
    pub fn menubar_meta_line(&self) -> String {
        format!("{} · {}", self.menubar_subtitle, self.menubar_age())
    }

    /// Whether the menu bar popover should show inline `Allow` / `Deny` buttons.
    pub fn allows_menubar_quick_answer(&self) -> bool {
        matches!(
            &self.body,
            AttentionRequestBody::Permission(detail) if detail.is_low_risk
        )
    }

    /// Label for the single action button in the menu bar popover when quick answer is unavailable.
    pub fn menubar_action_label(&self) -> &'static str {
        match self.kind {
            AttentionRequestKind::Permission => "Review...",
            AttentionRequestKind::Question => "Answer...",
            AttentionRequestKind::Review => "Review...",
            AttentionRequestKind::LoopExhausted => "Open...",
        }
    }

    /// Label for the primary answer button in the Inbox window.
    pub fn primary_action_label(&self) -> &'static str {
        match self.kind {
            AttentionRequestKind::Permission => "Allow Once",
            AttentionRequestKind::Question => "Send Answer",
            AttentionRequestKind::Review => "Approve & Open PR",
            AttentionRequestKind::LoopExhausted => "Grant 3 More Attempts",
        }
    }
}

/// Completed request shown in the collapsible `Answered today` section of the Inbox window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnsweredInboxItem {
    pub id: String,
    pub kind: AttentionRequestKind,
    pub agent: AgentBadge,
    pub project_name: String,
    pub workflow_name: String,
    pub context_label: String,
    pub run_number: u32,
    pub machine_name: String,
    pub title: String,
    pub subtitle: String,
    pub outcome: String,
    pub note: Option<String>,
    pub answered_ago: String,
}

impl AnsweredInboxItem {
    pub fn breadcrumb(&self) -> String {
        format!(
            "{} › {} › run #{} · {}",
            self.project_name, self.context_label, self.run_number, self.machine_name
        )
    }
}

/// Waiting timeout and reminder rules displayed from the Inbox footer's `Waiting rules` link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitingRulesConfig {
    pub reminder_after_minutes: u32,
    pub permission_timeout_minutes: u32,
    pub question_timeout_minutes: u32,
    pub review_waits_indefinitely: bool,
}

impl Default for WaitingRulesConfig {
    fn default() -> Self {
        Self {
            reminder_after_minutes: 15,
            permission_timeout_minutes: 120,
            question_timeout_minutes: 120,
            review_waits_indefinitely: true,
        }
    }
}

/// Complete Inbox + menu bar attention state shared across all windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxState {
    pub pending: Vec<AttentionRequest>,
    pub answered_today: Vec<AnsweredInboxItem>,
    pub saved_allow_rules: Vec<String>,
    pub active_agents_count: u32,
    pub all_agents_paused: bool,
    pub notifications_snoozed_minutes: Option<u32>,
    pub waiting_rules: WaitingRulesConfig,
}

impl InboxState {
    /// Mock dataset matching both `08 Inbox.png` and `10 AttentionSurfaces.png`.
    ///
    /// Includes the four blocking requests shown in `08 Inbox.png` plus the
    /// low-risk `npm run lint -- --fix` request shown at the top of the menu bar
    /// popover in `10 AttentionSurfaces.png`, along with six already-answered items.
    pub fn sample() -> Self {
        Self {
            pending: vec![
                AttentionRequest {
                    id: "req-loop7".to_owned(),
                    kind: AttentionRequestKind::LoopExhausted,
                    agent: AgentBadge::Codex,
                    project_name: "mobile-app".to_owned(),
                    workflow_name: "Dependency upgrade".to_owned(),
                    context_label: "Dependency upgrade".to_owned(),
                    run_number: 7,
                    machine_name: "build-box-01".to_owned(),
                    inbox_title: "Loop ran out of attempts".to_owned(),
                    inbox_subtitle: "Codex · build still fails after 5 attempts".to_owned(),
                    menubar_title: "Loop ran out of attempts".to_owned(),
                    menubar_subtitle: "mobile-app › Dependency upgrade".to_owned(),
                    waiting_minutes: 25,
                    past_reminder_threshold: true,
                    status_pill: "Loop paused · 5 of 5 attempts used".to_owned(),
                    why_text: "TypeScript compilation still fails in RootStack.tsx after react-native-screens 4.2.0 changed ScreenProps generics.".to_owned(),
                    deadline_badge: None,
                    timeout_footer: "Paused until you grant more attempts, send guidance, or stop the run.".to_owned(),
                    note_placeholder: "e.g. Pin react-native-screens to 4.1.x or update the ScreenProps type alias".to_owned(),
                    body: AttentionRequestBody::LoopExhausted(LoopExhaustedDetail {
                        attempts_used: 5,
                        max_attempts: 5,
                        last_error: "TS2344: Type 'NativeStackNavigationOptions' does not satisfy the constraint 'ScreenOptions' in src/navigation/RootStack.tsx:42".to_owned(),
                        attempts: vec![
                            LoopAttemptSummary {
                                attempt: 1,
                                summary: "Bumped react-native-screens to 4.2.0; pod install succeeded, 4 TS errors.".to_owned(),
                                failed: true,
                            },
                            LoopAttemptSummary {
                                attempt: 2,
                                summary: "Updated StackNavigator imports; 2 TS errors remaining in RootStack.tsx.".to_owned(),
                                failed: true,
                            },
                            LoopAttemptSummary {
                                attempt: 3,
                                summary: "Adjusted headerBackTitle prop; 1 generic constraint error remaining.".to_owned(),
                                failed: true,
                            },
                            LoopAttemptSummary {
                                attempt: 4,
                                summary: "Tried widening ScreenOptions wrapper; broke iOS modal presentation types.".to_owned(),
                                failed: true,
                            },
                            LoopAttemptSummary {
                                attempt: 5,
                                summary: "Reverted wrapper change; build still fails on RootStack.tsx:42.".to_owned(),
                                failed: true,
                            },
                        ],
                    }),
                },
                AttentionRequest {
                    id: "req-71d0".to_owned(),
                    kind: AttentionRequestKind::Review,
                    agent: AgentBadge::Codex,
                    project_name: "api-gateway".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    context_label: "Fix issue #318".to_owned(),
                    run_number: 41,
                    machine_name: "This Mac".to_owned(),
                    inbox_title: "Review before the pull request".to_owned(),
                    inbox_subtitle: "Review gate · 2 files, tests passing".to_owned(),
                    menubar_title: "Review before pull request".to_owned(),
                    menubar_subtitle: "api-gateway › Fix issue #318".to_owned(),
                    waiting_minutes: 18,
                    past_reminder_threshold: true,
                    status_pill: "Review gate · step 4 of 5".to_owned(),
                    why_text: "Implemented jittered exponential backoff for transient 503 responses from the auth upstream and verified all 48 unit tests pass.".to_owned(),
                    deadline_badge: None,
                    timeout_footer: "Review gates wait indefinitely until you approve or request changes.".to_owned(),
                    note_placeholder: "e.g. Cap max backoff jitter at 250ms before opening the PR".to_owned(),
                    body: AttentionRequestBody::Review(ReviewDetail {
                        summary: "Adds bounded retry with full jitter on HTTP 503 auth service responses and emits structured retry-attempt metrics without delaying non-idempotent requests.".to_owned(),
                        branch: "fix/issue-318-auth-503-retry".to_owned(),
                        base_branch: "main".to_owned(),
                        tests_summary: "48 passed · 0 failed · 0 skipped (1.8s)".to_owned(),
                        files: vec![
                            ReviewFileStat {
                                path: "src/auth/retry.ts".to_owned(),
                                additions: 38,
                                deletions: 6,
                            },
                            ReviewFileStat {
                                path: "src/auth/retry.test.ts".to_owned(),
                                additions: 54,
                                deletions: 0,
                            },
                        ],
                    }),
                },
                AttentionRequest {
                    id: "req-5a19".to_owned(),
                    kind: AttentionRequestKind::Question,
                    agent: AgentBadge::Claude,
                    project_name: "marketing-site".to_owned(),
                    workflow_name: "Docs refresh".to_owned(),
                    context_label: "Docs refresh".to_owned(),
                    run_number: 12,
                    machine_name: "build-box-01".to_owned(),
                    inbox_title: "Claude has a question".to_owned(),
                    inbox_subtitle: "Which pricing page should the table link to?".to_owned(),
                    menubar_title: "Which pricing page to link?".to_owned(),
                    menubar_subtitle: "Claude · marketing-site".to_owned(),
                    waiting_minutes: 11,
                    past_reminder_threshold: false,
                    status_pill: "Step 3 paused · Docs refresh".to_owned(),
                    why_text: "Both /pricing and /plans/enterprise exist in the sitemap, and the tier comparison table only has a single primary CTA link.".to_owned(),
                    deadline_badge: Some("Agent decides in 1h 49m".to_owned()),
                    timeout_footer: "If unanswered by 7:55 PM, Claude decides and continues the run.".to_owned(),
                    note_placeholder: "Or type a custom answer for Claude...".to_owned(),
                    body: AttentionRequestBody::Question(QuestionDetail {
                        prompt: "Which pricing page should the comparison table link to?".to_owned(),
                        options: vec![
                            QuestionOption {
                                id: "pricing".to_owned(),
                                label: "Link to /pricing".to_owned(),
                                detail: "Matches existing self-serve docs and keeps all tiers on one page.".to_owned(),
                                recommended: true,
                            },
                            QuestionOption {
                                id: "enterprise".to_owned(),
                                label: "Link to /plans/enterprise".to_owned(),
                                detail: "Highlights dedicated clusters and SSO for enterprise evaluators.".to_owned(),
                                recommended: false,
                            },
                            QuestionOption {
                                id: "both".to_owned(),
                                label: "Keep /pricing as primary and add an Enterprise footnote".to_owned(),
                                detail: "Covers both audiences without changing table column widths.".to_owned(),
                                recommended: false,
                            },
                        ],
                        default_if_unanswered: "Link to /pricing".to_owned(),
                    }),
                },
                AttentionRequest {
                    id: "req-vitest".to_owned(),
                    kind: AttentionRequestKind::Permission,
                    agent: AgentBadge::Codex,
                    project_name: "api-gateway".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    context_label: "Dependency upgrade".to_owned(),
                    run_number: 44,
                    machine_name: "build-box-01".to_owned(),
                    inbox_title: "Codex wants to run a command".to_owned(),
                    inbox_subtitle: "npm install --save-dev @vitest/coverage-v8".to_owned(),
                    menubar_title: "Install @vitest/coverage-v8".to_owned(),
                    menubar_subtitle: "Codex · needs network".to_owned(),
                    waiting_minutes: 3,
                    past_reminder_threshold: true,
                    status_pill: "Step 2 paused · attempt 1 of 3".to_owned(),
                    why_text: "Coverage reporting needs the v8 provider, and it isn't installed in this project yet.".to_owned(),
                    deadline_badge: Some("Denied in 1h 57m".to_owned()),
                    timeout_footer: "If unanswered by 8:05 PM, this is denied and Codex is told no one answered.".to_owned(),
                    note_placeholder: "e.g. Use the istanbul provider that's already installed".to_owned(),
                    body: AttentionRequestBody::Permission(PermissionDetail {
                        machine_name: "build-box-01".to_owned(),
                        working_dir: "/home/bruk/work/api-gateway".to_owned(),
                        command: "npm install --save-dev @vitest/coverage-v8".to_owned(),
                        risk_tags: vec![
                            PermissionRiskTag {
                                label: "Network access".to_owned(),
                                is_warning: true,
                                has_globe: true,
                            },
                            PermissionRiskTag {
                                label: "Changes package.json, package-lock.json".to_owned(),
                                is_warning: false,
                                has_globe: false,
                            },
                            PermissionRiskTag {
                                label: "Not in this step's allowed commands".to_owned(),
                                is_warning: false,
                                has_globe: false,
                            },
                        ],
                        is_low_risk: false,
                        always_allow_workflow: "Fix issue".to_owned(),
                        always_allow_pattern: "npm install --save-dev @vitest/*".to_owned(),
                        always_allow_note: "You can narrow it before saving. Rules never cover network access to new hosts.".to_owned(),
                    }),
                },
                AttentionRequest {
                    id: "req-8f2c".to_owned(),
                    kind: AttentionRequestKind::Permission,
                    agent: AgentBadge::Codex,
                    project_name: "api-gateway".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    context_label: "Fix issue #331".to_owned(),
                    run_number: 45,
                    machine_name: "This Mac".to_owned(),
                    inbox_title: "Codex wants to run a command".to_owned(),
                    inbox_subtitle: "npm run lint -- --fix".to_owned(),
                    menubar_title: "Run npm run lint -- --fix".to_owned(),
                    menubar_subtitle: "Codex · api-gateway › Fix issue #331".to_owned(),
                    waiting_minutes: 0,
                    past_reminder_threshold: false,
                    status_pill: "Step 3 paused · attempt 1 of 3".to_owned(),
                    why_text: "Auto-format the new request-ID middleware files so ESLint passes before running the test suite.".to_owned(),
                    deadline_badge: Some("Denied in 2h 0m".to_owned()),
                    timeout_footer: "If unanswered by 8:08 PM, this is denied and Codex is told no one answered.".to_owned(),
                    note_placeholder: "e.g. Run eslint without --fix first so we can inspect warnings".to_owned(),
                    body: AttentionRequestBody::Permission(PermissionDetail {
                        machine_name: "This Mac".to_owned(),
                        working_dir: "/Users/jonathan/src/api-gateway".to_owned(),
                        command: "npm run lint -- --fix".to_owned(),
                        risk_tags: vec![
                            PermissionRiskTag {
                                label: "Low risk · workspace only".to_owned(),
                                is_warning: false,
                                has_globe: false,
                            },
                            PermissionRiskTag {
                                label: "No network access".to_owned(),
                                is_warning: false,
                                has_globe: false,
                            },
                        ],
                        is_low_risk: true,
                        always_allow_workflow: "Fix issue".to_owned(),
                        always_allow_pattern: "npm run lint*".to_owned(),
                        always_allow_note: "Safe workspace formatter rule for this workflow. Rules never cover network access.".to_owned(),
                    }),
                },
            ],
            answered_today: vec![
                AnsweredInboxItem {
                    id: "ans-1".to_owned(),
                    kind: AttentionRequestKind::Permission,
                    agent: AgentBadge::Codex,
                    project_name: "api-gateway".to_owned(),
                    workflow_name: "Release prep".to_owned(),
                    context_label: "Release prep".to_owned(),
                    run_number: 42,
                    machine_name: "This Mac".to_owned(),
                    title: "Codex wants to run a command".to_owned(),
                    subtitle: "npm version 2.8.0 --no-git-tag-version".to_owned(),
                    outcome: "Allowed once".to_owned(),
                    note: None,
                    answered_ago: "14m ago".to_owned(),
                },
                AnsweredInboxItem {
                    id: "ans-2".to_owned(),
                    kind: AttentionRequestKind::Review,
                    agent: AgentBadge::Claude,
                    project_name: "api-gateway".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    context_label: "Fix issue #329".to_owned(),
                    run_number: 39,
                    machine_name: "This Mac".to_owned(),
                    title: "Review before the pull request".to_owned(),
                    subtitle: "Return 429 instead of 500 on burst · 3 files".to_owned(),
                    outcome: "Approved & opened PR #342".to_owned(),
                    note: None,
                    answered_ago: "38m ago".to_owned(),
                },
                AnsweredInboxItem {
                    id: "ans-3".to_owned(),
                    kind: AttentionRequestKind::Question,
                    agent: AgentBadge::Claude,
                    project_name: "marketing-site".to_owned(),
                    workflow_name: "Docs refresh".to_owned(),
                    context_label: "Docs refresh".to_owned(),
                    run_number: 11,
                    machine_name: "build-box-01".to_owned(),
                    title: "Claude has a question".to_owned(),
                    subtitle: "Should webhook payload examples use RFC 3339 timestamps?".to_owned(),
                    outcome: "Answered: Yes, UTC with Z suffix".to_owned(),
                    note: None,
                    answered_ago: "1h ago".to_owned(),
                },
                AnsweredInboxItem {
                    id: "ans-4".to_owned(),
                    kind: AttentionRequestKind::Permission,
                    agent: AgentBadge::Codex,
                    project_name: "mobile-app".to_owned(),
                    workflow_name: "Dependency upgrade".to_owned(),
                    context_label: "Dependency upgrade".to_owned(),
                    run_number: 6,
                    machine_name: "build-box-01".to_owned(),
                    title: "Codex wants to run a command".to_owned(),
                    subtitle: "npx pod-install --non-interactive".to_owned(),
                    outcome: "Allowed for this run".to_owned(),
                    note: None,
                    answered_ago: "2h ago".to_owned(),
                },
                AnsweredInboxItem {
                    id: "ans-5".to_owned(),
                    kind: AttentionRequestKind::Permission,
                    agent: AgentBadge::Codex,
                    project_name: "api-gateway".to_owned(),
                    workflow_name: "Security audit".to_owned(),
                    context_label: "Security audit".to_owned(),
                    run_number: 9,
                    machine_name: "build-box-01".to_owned(),
                    title: "Codex wants to run a command".to_owned(),
                    subtitle: "curl -fsSL https://sh.example.dev/scanner | sh".to_owned(),
                    outcome: "Denied".to_owned(),
                    note: Some("Use the pinned local osv-scanner binary instead.".to_owned()),
                    answered_ago: "3h ago".to_owned(),
                },
                AnsweredInboxItem {
                    id: "ans-6".to_owned(),
                    kind: AttentionRequestKind::Review,
                    agent: AgentBadge::Claude,
                    project_name: "api-gateway".to_owned(),
                    workflow_name: "Fix issue".to_owned(),
                    context_label: "Fix issue #324".to_owned(),
                    run_number: 36,
                    machine_name: "This Mac".to_owned(),
                    title: "Review before the pull request".to_owned(),
                    subtitle: "Typo in auth error message · 1 file".to_owned(),
                    outcome: "Approved & opened PR #339".to_owned(),
                    note: None,
                    answered_ago: "4h ago".to_owned(),
                },
            ],
            saved_allow_rules: Vec::new(),
            active_agents_count: 3,
            all_agents_paused: false,
            notifications_snoozed_minutes: None,
            waiting_rules: WaitingRulesConfig::default(),
        }
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn count_for_filter(&self, filter: InboxFilter) -> usize {
        self.pending
            .iter()
            .filter(|req| filter.matches(req.kind))
            .count()
    }

    /// Pending requests ordered oldest first (for the Inbox window list).
    pub fn pending_oldest_first(&self, filter: InboxFilter) -> Vec<&AttentionRequest> {
        let mut items: Vec<&AttentionRequest> = self
            .pending
            .iter()
            .filter(|req| filter.matches(req.kind))
            .collect();
        items.sort_by_key(|a| std::cmp::Reverse(a.waiting_minutes));
        items
    }

    /// Pending requests ordered newest first (for the menu bar popover, matching `10 AttentionSurfaces.png`).
    pub fn pending_newest_first(&self) -> Vec<&AttentionRequest> {
        let mut items: Vec<&AttentionRequest> = self.pending.iter().collect();
        items.sort_by_key(|a| a.waiting_minutes);
        items
    }

    /// Default selection when opening Inbox: highlights the `@vitest/coverage-v8` permission request
    /// from `08 Inbox.png` when present, else the newest pending item.
    pub fn default_selected_id(&self) -> Option<String> {
        if self.pending.iter().any(|r| r.id == "req-vitest") {
            Some("req-vitest".to_owned())
        } else {
            self.pending_oldest_first(InboxFilter::All)
                .last()
                .map(|r| r.id.clone())
        }
    }

    pub fn pending_by_id(&self, id: &str) -> Option<&AttentionRequest> {
        self.pending.iter().find(|r| r.id == id)
    }

    pub fn answered_by_id(&self, id: &str) -> Option<&AnsweredInboxItem> {
        self.answered_today.iter().find(|r| r.id == id)
    }

    /// True once any waiting request has passed its reminder threshold (turns the menu bar count orange).
    pub fn any_overdue(&self) -> bool {
        self.pending.iter().any(|r| r.past_reminder_threshold)
    }

    pub fn oldest_waiting_minutes(&self) -> Option<u32> {
        self.pending.iter().map(|r| r.waiting_minutes).max()
    }

    /// Subtitle shown below `Inbox` in the Inbox window toolbar.
    pub fn header_subtitle(&self) -> String {
        let count = self.pending.len();
        let agents_part = if self.all_agents_paused {
            "agents paused".to_owned()
        } else {
            format!("{} still working", self.active_agents_count)
        };
        if count == 0 {
            return format!("All caught up · {agents_part}");
        }
        let agent_word = if count == 1 { "agent" } else { "agents" };
        let oldest = self.oldest_waiting_minutes().unwrap_or(0);
        let oldest_part = if oldest == 0 {
            "oldest just now".to_owned()
        } else {
            format!("oldest {oldest} min")
        };
        format!("{count} {agent_word} waiting · {oldest_part} · {agents_part}")
    }

    /// Right-hand status shown in the menu bar popover header.
    pub fn menubar_agents_status(&self) -> String {
        if self.all_agents_paused {
            "Agents paused".to_owned()
        } else {
            format!("{} agents working", self.active_agents_count)
        }
    }

    pub fn allow_once(
        &mut self,
        id: &str,
        edited_command: Option<&str>,
        note: Option<&str>,
    ) -> Option<AnsweredInboxItem> {
        let cmd_note = edited_command
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_owned);
        let outcome = match cmd_note.as_deref() {
            Some(cmd) => format!("Allowed once (`{cmd}`)"),
            None => "Allowed once".to_owned(),
        };
        self.resolve_request(id, outcome, note)
    }

    pub fn allow_for_run(
        &mut self,
        id: &str,
        edited_command: Option<&str>,
        note: Option<&str>,
    ) -> Option<AnsweredInboxItem> {
        let cmd_note = edited_command
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_owned);
        let outcome = match cmd_note.as_deref() {
            Some(cmd) => format!("Allowed for this run (`{cmd}`)"),
            None => "Allowed for this run".to_owned(),
        };
        self.resolve_request(id, outcome, note)
    }

    pub fn always_allow(
        &mut self,
        id: &str,
        pattern: &str,
        scope: AlwaysAllowScope,
    ) -> Option<AnsweredInboxItem> {
        let trimmed = pattern.trim();
        let rule_label = if trimmed.is_empty() {
            format!("Saved rule ({})", scope.label())
        } else {
            self.saved_allow_rules
                .push(format!("{trimmed} ({})", scope.label()));
            format!("Always allowed `{trimmed}` ({})", scope.label())
        };
        self.resolve_request(id, rule_label, None)
    }

    pub fn deny(&mut self, id: &str, note: Option<&str>) -> Option<AnsweredInboxItem> {
        self.resolve_request(id, "Denied".to_owned(), note)
    }

    pub fn answer_question(&mut self, id: &str, answer: &str) -> Option<AnsweredInboxItem> {
        let trimmed = answer.trim();
        let outcome = if trimmed.is_empty() {
            "Answered".to_owned()
        } else {
            format!("Answered: {trimmed}")
        };
        self.resolve_request(id, outcome, None)
    }

    pub fn let_agent_decide(&mut self, id: &str) -> Option<AnsweredInboxItem> {
        self.resolve_request(id, "Let agent decide".to_owned(), None)
    }

    pub fn approve_review(&mut self, id: &str, note: Option<&str>) -> Option<AnsweredInboxItem> {
        self.resolve_request(id, "Approved & opened PR".to_owned(), note)
    }

    pub fn request_changes(&mut self, id: &str, note: Option<&str>) -> Option<AnsweredInboxItem> {
        self.resolve_request(id, "Requested changes".to_owned(), note)
    }

    pub fn grant_more_attempts(
        &mut self,
        id: &str,
        extra_attempts: u32,
        note: Option<&str>,
    ) -> Option<AnsweredInboxItem> {
        self.resolve_request(id, format!("Granted +{extra_attempts} attempts"), note)
    }

    pub fn stop_exhausted_run(
        &mut self,
        id: &str,
        note: Option<&str>,
    ) -> Option<AnsweredInboxItem> {
        self.resolve_request(id, "Stopped run".to_owned(), note)
    }

    pub fn toggle_pause_all_agents(&mut self) -> bool {
        self.all_agents_paused = !self.all_agents_paused;
        self.all_agents_paused
    }

    pub fn toggle_snooze_notifications(&mut self) -> Option<u32> {
        if self.notifications_snoozed_minutes.is_some() {
            self.notifications_snoozed_minutes = None;
        } else {
            self.notifications_snoozed_minutes = Some(60);
        }
        self.notifications_snoozed_minutes
    }

    fn resolve_request(
        &mut self,
        id: &str,
        outcome: String,
        note: Option<&str>,
    ) -> Option<AnsweredInboxItem> {
        let idx = self.pending.iter().position(|r| r.id == id)?;
        let req = self.pending.remove(idx);
        let cleaned_note = note
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_owned);
        let answered = AnsweredInboxItem {
            id: req.id,
            kind: req.kind,
            agent: req.agent,
            project_name: req.project_name,
            workflow_name: req.workflow_name,
            context_label: req.context_label,
            run_number: req.run_number,
            machine_name: req.machine_name,
            title: req.inbox_title,
            subtitle: req.inbox_subtitle,
            outcome,
            note: cleaned_note,
            answered_ago: "Just now".to_owned(),
        };
        self.answered_today.insert(0, answered.clone());
        Some(answered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_inbox_orders_oldest_first_in_inbox_and_newest_first_in_menubar() {
        let state = InboxState::sample();
        assert_eq!(state.pending_count(), 5);
        assert_eq!(state.answered_today.len(), 6);
        assert!(state.any_overdue());
        assert_eq!(state.default_selected_id().as_deref(), Some("req-vitest"));

        let oldest: Vec<&str> = state
            .pending_oldest_first(InboxFilter::All)
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(
            oldest,
            vec![
                "req-loop7",
                "req-71d0",
                "req-5a19",
                "req-vitest",
                "req-8f2c"
            ]
        );

        let newest: Vec<&str> = state
            .pending_newest_first()
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(
            newest,
            vec![
                "req-8f2c",
                "req-vitest",
                "req-5a19",
                "req-71d0",
                "req-loop7"
            ]
        );
    }

    #[test]
    fn filters_and_quick_answer_eligibility_match_risk_profile() {
        let state = InboxState::sample();
        assert_eq!(state.count_for_filter(InboxFilter::All), 5);
        assert_eq!(state.count_for_filter(InboxFilter::Permissions), 2);
        assert_eq!(state.count_for_filter(InboxFilter::Questions), 1);
        assert_eq!(state.count_for_filter(InboxFilter::Reviews), 1);

        let lint = state.pending_by_id("req-8f2c").unwrap();
        assert!(lint.allows_menubar_quick_answer());

        let vitest = state.pending_by_id("req-vitest").unwrap();
        assert!(!vitest.allows_menubar_quick_answer());
        assert_eq!(vitest.menubar_action_label(), "Review...");
    }

    #[test]
    fn answering_request_moves_it_to_answered_today_and_updates_header() {
        let mut state = InboxState::sample();
        let answered = state
            .allow_once("req-8f2c", None, None)
            .expect("should resolve low-risk lint command");
        assert_eq!(answered.outcome, "Allowed once");
        assert_eq!(state.pending_count(), 4);
        assert_eq!(state.answered_today.len(), 7);
        assert_eq!(
            state.header_subtitle(),
            "4 agents waiting · oldest 25 min · 3 still working"
        );

        let saved = state
            .always_allow(
                "req-vitest",
                "npm install --save-dev @vitest/*",
                AlwaysAllowScope::Workflow,
            )
            .expect("should save always allow rule");
        assert!(saved.outcome.contains("@vitest/*"));
        assert_eq!(state.saved_allow_rules.len(), 1);
        assert_eq!(state.pending_count(), 3);
    }

    #[test]
    fn pause_and_snooze_toggle_cleanly() {
        let mut state = InboxState::sample();
        assert!(!state.all_agents_paused);
        assert!(state.toggle_pause_all_agents());
        assert!(state.header_subtitle().contains("agents paused"));
        assert_eq!(state.menubar_agents_status(), "Agents paused");

        assert_eq!(state.notifications_snoozed_minutes, None);
        assert_eq!(state.toggle_snooze_notifications(), Some(60));
        assert_eq!(state.toggle_snooze_notifications(), None);
    }
}
