//! Provider identity, CLI flag encoding, and live model/usage summary types.

use chrono::DateTime;
use serde::{Deserialize, Serialize};

/// Standard SSH agent socket environment variable name.
pub const SSH_AUTH_SOCK_ENV: &str = "SSH_AUTH_SOCK";

/// Supported AI provider families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Claude,
    Codex,
    Antigravity,
    OpenRouter,
}

impl ProviderKind {
    /// The agent CLIs Bruk can run, in the order Settings and pickers list them.
    pub const CLI_PROVIDERS: &'static [ProviderKind] =
        &[Self::Claude, Self::Codex, Self::Antigravity];

    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Antigravity => "Antigravity",
            Self::OpenRouter => "OpenRouter",
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            Self::Claude => "Cl",
            Self::Codex => "Cx",
            Self::Antigravity => "Ag",
            Self::OpenRouter => "OR",
        }
    }

    /// Lowercase id used for account ids and UI test ids (`"claude"`, `"antigravity"`).
    pub fn slug(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::OpenRouter => "openrouter",
        }
    }

    /// Whether this provider is an agent CLI (as opposed to an API key).
    pub fn is_cli(self) -> bool {
        !matches!(self, Self::OpenRouter)
    }

    pub fn group_hint(self) -> &'static str {
        match self {
            Self::Claude | Self::Codex | Self::Antigravity => "one account",
            Self::OpenRouter => "one or more keys",
        }
    }
}

/// How a CLI launch flag setting is serialized into `argv`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliFlagEncoding {
    /// Emits `flag` when `Bool(true)`; omitted when `Bool(false)` or unset.
    BoolFlag(&'static str),
    /// Emits `on` when `Bool(true)`, `off` when `Bool(false)`.
    BoolTogglePair { on: &'static str, off: &'static str },
    /// Emits `flag` followed by `"true"` or `"false"`.
    BoolValueFlag(&'static str),
    /// Emits `flag` followed by the scalar string/number value.
    ValueFlag(&'static str),
    /// Emits just `flag` when string is empty, or `flag <value>` when non-empty.
    OptionalValueFlag(&'static str),
    /// Emits `flag` followed by all list items as separate arguments.
    MultiValueFlag(&'static str),
    /// Emits `flag <item>` once per list item.
    RepeatableFlag(&'static str),
}

/// One model option reported by a provider CLI's model listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeModelOption {
    /// Model alias or identifier accepted by `claude --model <value>`.
    pub value: String,
    /// Concrete model ID resolved by the CLI (e.g. `"claude-opus-5-5"`).
    pub resolved_model: String,
    /// Human-readable display name (e.g. `"Opus 5.5"`).
    pub display_name: String,
    /// Short description of the model's intended use.
    pub description: String,
}

impl ClaudeModelOption {
    /// Derives the normalized model family name (e.g. `"Opus"`, `"Fable"`, `"Sonnet"`, `"Haiku"`).
    pub fn family(&self) -> String {
        if let Some(family) = model_family_from_display_name(&self.display_name) {
            return family;
        }
        if let Some(family) = model_family_from_id(&self.resolved_model) {
            return family;
        }
        if let Some(family) = model_family_from_id(&self.value) {
            return family;
        }
        "Other".to_owned()
    }
}

/// Extracts a normalized model family name from a model identifier or alias
/// (e.g. `"claude-fable-5-2"` -> `"Fable"`, `"opus"` -> `"Opus"`,
/// `"gpt-6.2-sol"` -> `"Sol"`, `"gpt-5.3-codex"` -> `"Codex"`, `"o4-mini"` -> `"o-series"`,
/// `"gemini-4-neon-high"` -> `"Neon"`).
pub fn model_family_from_id(id: &str) -> Option<String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(family) = codex_model_family_from_text(trimmed) {
        return Some(family);
    }
    let without_prefix = trimmed
        .strip_prefix("claude-")
        .or_else(|| trimmed.strip_prefix("Claude-"))
        .unwrap_or(trimmed);
    for segment in without_prefix.split(['-', '_', ' ', '/']) {
        if !segment.is_empty()
            && segment.chars().all(|c| c.is_ascii_alphabetic())
            && !segment.eq_ignore_ascii_case("claude")
            && !segment.eq_ignore_ascii_case("gemini")
            && !segment.eq_ignore_ascii_case("default")
        {
            return Some(title_case_word(segment));
        }
    }
    None
}

fn model_family_from_display_name(display_name: &str) -> Option<String> {
    let trimmed = display_name.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(family) = codex_model_family_from_text(trimmed) {
        return Some(family);
    }
    for word in trimmed.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| !c.is_ascii_alphabetic());
        if cleaned.is_empty()
            || cleaned.eq_ignore_ascii_case("claude")
            || cleaned.eq_ignore_ascii_case("gemini")
            || cleaned.eq_ignore_ascii_case("default")
        {
            continue;
        }
        return Some(title_case_word(cleaned));
    }
    None
}

fn codex_model_family_from_text(text: &str) -> Option<String> {
    let tokens: Vec<&str> = text
        .split(['-', '_', ' ', '/'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let first = *tokens.first()?;

    let after_provider: &[&str] = if first.eq_ignore_ascii_case("openai") {
        &tokens[1..]
    } else {
        &tokens[..]
    };
    let head = *after_provider.first()?;

    if head.eq_ignore_ascii_case("gpt") {
        for token in &after_provider[1..] {
            if token.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                continue;
            }
            if token.chars().all(|c| c.is_ascii_alphabetic()) {
                return Some(title_case_word(token));
            }
        }
        return Some("GPT".to_owned());
    }

    if head.eq_ignore_ascii_case("codex") {
        return Some("Codex".to_owned());
    }

    let mut head_chars = head.chars();
    if matches!(head_chars.next(), Some('o' | 'O')) {
        let rest = head_chars.as_str();
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some("o-series".to_owned());
        }
    }

    None
}

fn title_case_word(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => format!(
            "{}{}",
            first.to_uppercase(),
            chars.as_str().to_ascii_lowercase()
        ),
        None => String::new(),
    }
}

/// One plan usage/rate-limit window returned by a provider CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeUsageWindow {
    /// Human-readable label (e.g. `"Current session (5h)"`, `"Weekly (all models)"`).
    pub label: String,
    /// Percentage of the quota window already consumed (`0..=100`).
    pub used_percent: u8,
    /// Percentage of the quota window remaining (`0..=100`).
    pub remaining_percent: u8,
    /// Optional ISO-8601 reset timestamp reported by the CLI.
    pub resets_at: Option<String>,
    /// Severity string reported by the CLI (`"normal"`, `"warning"`, `"critical"`).
    pub severity: String,
}

/// One scheduled quota reset entry reported by a provider CLI usage API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeQuotaReset {
    /// Window label (e.g. `"Current session (5h)"`, `"Weekly (all models)"`).
    pub label: String,
    /// Raw RFC-3339 reset timestamp from the CLI.
    pub resets_at: String,
}

/// Plan usage and remaining rate-limit summary fetched from a provider CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeUsageSummary {
    /// Whether subscription rate-limit windows are available for this account.
    pub rate_limits_available: bool,
    /// Active quota windows (5-hour session, weekly all models, model-scoped weekly, etc.).
    pub windows: Vec<ClaudeUsageWindow>,
    /// Optional note describing extra usage credits or billing status.
    pub extra_usage_note: Option<String>,
}

impl ClaudeUsageSummary {
    /// Returns all available scheduled quota resets reported by the usage API, sorted soonest-first.
    pub fn upcoming_resets(&self) -> Vec<ClaudeQuotaReset> {
        let mut entries: Vec<(Option<i64>, usize, ClaudeQuotaReset)> = self
            .windows
            .iter()
            .enumerate()
            .filter_map(|(idx, window)| {
                let raw = window.resets_at.as_deref()?.trim();
                if raw.is_empty() {
                    return None;
                }
                let sort_key = DateTime::parse_from_rfc3339(raw)
                    .ok()
                    .map(|dt| dt.timestamp());
                Some((
                    sort_key,
                    idx,
                    ClaudeQuotaReset {
                        label: window.label.clone(),
                        resets_at: raw.to_owned(),
                    },
                ))
            })
            .collect();

        entries.sort_by_key(|(ts, idx, _)| (ts.unwrap_or(i64::MAX), *idx));
        entries.into_iter().map(|(_, _, item)| item).collect()
    }
}
