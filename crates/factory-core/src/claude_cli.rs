//! Discovery and authentication probing for the local `claude` CLI.
//!
//! Claude runs with the user's existing system login rather than an isolated
//! Bruk-managed credential store. Bruk locates the `claude` executable either
//! from a user-configured custom path or from the default `PATH`, then invokes
//! `claude auth status --json` to verify system authentication and read
//! non-sensitive account metadata (email, plan, config directory).

use std::{
    env,
    ffi::OsStr,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Duration, FixedOffset, Local, Timelike};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default executable filename searched on `PATH`.
#[cfg(windows)]
pub const CLAUDE_BINARY_NAME: &str = "claude.exe";
/// Default executable filename searched on `PATH`.
#[cfg(not(windows))]
pub const CLAUDE_BINARY_NAME: &str = "claude";

/// Errors returned when locating the `claude` executable.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClaudeBinaryError {
    #[error("Claude CLI ('claude') was not found on the default PATH.")]
    NotFoundOnPath,
    #[error("Claude CLI was not found at '{}'.", .0.display())]
    CustomPathNotFound(PathBuf),
    #[error("'{}' is a directory, not a Claude CLI executable.", .0.display())]
    NotAFile(PathBuf),
    #[error("'{}' is not executable.", .0.display())]
    NotExecutable(PathBuf),
}

impl ClaudeBinaryError {
    /// Short summary suitable for the Preferences detail header status indicator.
    pub fn short_status(&self) -> String {
        match self {
            Self::NotFoundOnPath => "CLI not found on PATH".to_owned(),
            Self::CustomPathNotFound(_) => "Custom path not found".to_owned(),
            Self::NotAFile(_) => "Path is not a file".to_owned(),
            Self::NotExecutable(_) => "Binary is not executable".to_owned(),
        }
    }
}

/// Result of probing the system's `claude` CLI binary and login state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeCliProbe {
    /// Binary exists and `claude auth status --json` proved active authentication.
    Authenticated {
        binary_path: PathBuf,
        email: String,
        plan: String,
        auth_method: String,
        config_dir: String,
    },
    /// Binary exists, but `claude auth status --json` reported `loggedIn: false`.
    NotAuthenticated {
        binary_path: PathBuf,
        config_dir: String,
    },
    /// No executable `claude` binary was found on `PATH` or at the configured custom path.
    BinaryNotFound { error: ClaudeBinaryError },
    /// Binary was found, but probing `claude auth status --json` failed unexpectedly.
    ProbeFailed {
        binary_path: PathBuf,
        message: String,
    },
}

/// One model option reported by the Claude CLI's `initialize` control response.
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
/// `"gpt-6.2-sol"` -> `"Sol"`, `"gpt-5.3-codex"` -> `"Codex"`, `"o4-mini"` -> `"o-series"`).
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
            || cleaned.eq_ignore_ascii_case("default")
        {
            continue;
        }
        return Some(title_case_word(cleaned));
    }
    None
}

/// Extracts a normalized family name from OpenAI/Codex model identifiers or display names
/// (e.g. `"GPT-6.2-Sol"` -> `"Sol"`, `"GPT-6.1 Terra"` -> `"Terra"`,
/// `"gpt-5.3-codex"` -> `"Codex"`, `"GPT-5.5"` -> `"GPT"`, `"o4-mini"` -> `"o-series"`).
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

/// One plan usage/rate-limit window returned by the Claude CLI's `get_usage` control request.
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

/// One scheduled quota reset entry reported by the Claude CLI usage API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeQuotaReset {
    /// Window label (e.g. `"Current session (5h)"`, `"Weekly (all models)"`).
    pub label: String,
    /// Raw RFC-3339 reset timestamp from the CLI.
    pub resets_at: String,
}

/// Plan usage and remaining rate-limit summary fetched from the Claude CLI.
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

/// Live model catalog and usage details queried from the Claude CLI over `stream-json`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClaudeCliDetails {
    pub models: Vec<ClaudeModelOption>,
    pub usage: Option<ClaudeUsageSummary>,
    pub warning: Option<String>,
}

/// Subset of fields emitted by `claude auth status --json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeAuthStatusJson {
    #[serde(default)]
    logged_in: bool,
    #[serde(default)]
    auth_method: Option<String>,
    #[serde(default)]
    api_provider: Option<String>,
    #[serde(default)]
    config_directory: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    org_name: Option<String>,
    #[serde(default)]
    subscription_type: Option<String>,
    #[serde(default)]
    plan_tier: Option<String>,
    #[serde(default)]
    api_key_source: Option<String>,
}

/// Expands a leading `~` or `~/` using the current user's home directory.
pub fn expand_tilde(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home;
    }
    if let Some(rest) = trimmed.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(trimmed)
}

/// Resolves the `claude` CLI binary using an optional custom path override or the given `PATH`.
pub fn resolve_claude_binary(
    custom_path: Option<&str>,
    path_env: Option<&OsStr>,
) -> Result<PathBuf, ClaudeBinaryError> {
    if let Some(raw) = custom_path {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let candidate = expand_tilde(trimmed);
            return validate_executable_file(candidate);
        }
    }

    if let Some(paths) = path_env {
        for dir in env::split_paths(paths) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let candidate = dir.join(CLAUDE_BINARY_NAME);
            if is_executable_file(&candidate) {
                return Ok(candidate);
            }
        }
    }

    Err(ClaudeBinaryError::NotFoundOnPath)
}

/// Resolves the `claude` CLI binary using the current process's `PATH` environment variable.
pub fn resolve_claude_binary_from_env(
    custom_path: Option<&str>,
) -> Result<PathBuf, ClaudeBinaryError> {
    let path_env = env::var_os("PATH");
    resolve_claude_binary(custom_path, path_env.as_deref())
}

/// Probes the `claude` CLI binary and checks whether the system already has an active login.
pub fn probe_claude_cli(custom_path: Option<&str>, path_env: Option<&OsStr>) -> ClaudeCliProbe {
    let binary_path = match resolve_claude_binary(custom_path, path_env) {
        Ok(path) => path,
        Err(error) => return ClaudeCliProbe::BinaryNotFound { error },
    };

    let output = match Command::new(&binary_path)
        .args(["auth", "status", "--json"])
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            return ClaudeCliProbe::ProbeFailed {
                binary_path,
                message: format!("Failed to run Claude CLI: {err}"),
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_claude_auth_status(binary_path, &stdout, &stderr, output.status.success())
}

/// Probes the `claude` CLI using the current process's `PATH` environment variable.
pub fn probe_claude_cli_from_env(custom_path: Option<&str>) -> ClaudeCliProbe {
    let path_env = env::var_os("PATH");
    probe_claude_cli(custom_path, path_env.as_deref())
}

/// Runs `claude auth login` so the user can authenticate their system Claude CLI account.
pub fn run_claude_auth_login(binary_path: &Path) -> Result<(), String> {
    let output = Command::new(binary_path)
        .args(["auth", "login"])
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("Failed to start Claude sign-in: {err}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !stderr.is_empty() {
        Err(stderr)
    } else if !stdout.is_empty() {
        Err(stdout)
    } else {
        Err(format!(
            "Claude sign-in exited with status {}.",
            output.status
        ))
    }
}

const STREAM_INIT_REQUEST_ID: &str = "bruk_init";
const STREAM_USAGE_REQUEST_ID: &str = "bruk_usage";

/// Queries the local `claude` CLI for its live available models and plan usage remaining
/// using the bidirectional `stream-json` control protocol (`initialize` + `get_usage`).
///
/// This spends zero inference tokens and uses the CLI's own system authentication.
pub fn fetch_claude_details(binary_path: &Path) -> ClaudeCliDetails {
    let mut child = match Command::new(binary_path)
        .args([
            "--print",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
            "--safe-mode",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            return ClaudeCliDetails {
                models: Vec::new(),
                usage: None,
                warning: Some(format!("Failed to query Claude CLI details: {err}")),
            };
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let requests = format!(
            "{{\"type\":\"control_request\",\"request_id\":\"{STREAM_INIT_REQUEST_ID}\",\"request\":{{\"subtype\":\"initialize\"}}}}\n\
             {{\"type\":\"control_request\",\"request_id\":\"{STREAM_USAGE_REQUEST_ID}\",\"request\":{{\"subtype\":\"get_usage\"}}}}\n"
        );
        let _ = stdin.write_all(requests.as_bytes());
        let _ = stdin.flush();
    }

    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(err) => {
            return ClaudeCliDetails {
                models: Vec::new(),
                usage: None,
                warning: Some(format!(
                    "Failed while waiting for Claude CLI details: {err}"
                )),
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_claude_details_stream(&stdout, &stderr)
}

/// Parses NDJSON control responses from `claude --print --input-format stream-json --output-format stream-json`.
pub fn parse_claude_details_stream(stdout: &str, stderr: &str) -> ClaudeCliDetails {
    let mut details = ClaudeCliDetails::default();
    let mut saw_any_control_response = false;

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        let Ok(envelope) = serde_json::from_str::<ControlEnvelope>(trimmed) else {
            continue;
        };
        if envelope.kind.as_deref() != Some("control_response") {
            continue;
        }
        let Some(resp) = envelope.response else {
            continue;
        };
        saw_any_control_response = true;
        let req_id = resp.request_id.as_deref().unwrap_or("");
        let subtype = resp.subtype.as_deref().unwrap_or("");

        if req_id == STREAM_INIT_REQUEST_ID {
            if subtype == "success"
                && let Some(payload) = resp.response
                && let Ok(init) = serde_json::from_value::<InitControlResponse>(payload)
            {
                details.models = normalize_model_options(init.models);
            } else if let Some(err_msg) = resp.error {
                details.warning = Some(err_msg);
            }
        } else if req_id == STREAM_USAGE_REQUEST_ID {
            if subtype == "success"
                && let Some(payload) = resp.response
                && let Ok(usage_resp) = serde_json::from_value::<UsageControlResponse>(payload)
            {
                details.usage = Some(build_usage_summary(usage_resp));
            } else if let Some(err_msg) = resp.error {
                details.warning = Some(format!("Could not fetch usage limits: {err_msg}"));
            }
        }
    }

    if !saw_any_control_response && details.warning.is_none() {
        let err_detail = stderr.trim();
        if !err_detail.is_empty() {
            details.warning = Some(err_detail.to_owned());
        } else {
            details.warning = Some(
                "Claude CLI did not return model or usage metadata (an older CLI version may be installed)."
                    .to_owned(),
            );
        }
    }

    details
}

/// Formats an ISO-8601 reset timestamp into a local date/time and countdown relative to now
/// (e.g. `"Today at 7:00 PM · in 2h 35m"` or `"Sun, Nov 29 at 3:00 PM · in 6d 22h"`).
pub fn format_reset_schedule_now(resets_at: &str) -> Option<String> {
    let now_local = Local::now().fixed_offset();
    format_reset_schedule(resets_at, now_local)
}

/// Formats an ISO-8601 reset timestamp in `now_local`'s timezone along with a relative countdown.
pub fn format_reset_schedule(resets_at: &str, now_local: DateTime<FixedOffset>) -> Option<String> {
    let parsed = DateTime::parse_from_rfc3339(resets_at.trim()).ok()?;
    let target_local = parsed.with_timezone(now_local.offset());

    // Round sub-minute timestamps (e.g. `23:59:59.838`) to the nearest minute for clean display.
    let rounded_local = if target_local.second() >= 30 {
        target_local + Duration::seconds(60 - i64::from(target_local.second()))
    } else {
        target_local
    };

    let today = now_local.date_naive();
    let target_date = rounded_local.date_naive();
    let time_str = rounded_local.format("%-I:%M %p").to_string();

    let day_str = if target_date == today {
        format!("Today at {time_str}")
    } else if target_date == today + Duration::days(1) {
        format!("Tomorrow at {time_str}")
    } else {
        format!("{} at {time_str}", rounded_local.format("%a, %b %-d"))
    };

    let diff = parsed.timestamp() - now_local.timestamp();
    let countdown = if diff <= 60 {
        "soon".to_owned()
    } else {
        let total_mins = (diff + 59) / 60;
        let days = total_mins / (24 * 60);
        let hours = (total_mins % (24 * 60)) / 60;
        let mins = total_mins % 60;
        if days > 0 {
            if hours > 0 {
                format!("in {days}d {hours}h")
            } else {
                format!("in {days}d")
            }
        } else if hours > 0 {
            if mins > 0 {
                format!("in {hours}h {mins}m")
            } else {
                format!("in {hours}h")
            }
        } else {
            format!("in {mins}m")
        }
    };

    Some(format!("{day_str} · {countdown}"))
}

/// Formats an ISO-8601 UTC/offset reset timestamp as a compact human-readable countdown
/// relative to the current system clock (e.g. `"Resets in 3h 15m"`, `"Resets in 6d 4h"`).
pub fn format_reset_countdown_now(resets_at: &str) -> Option<String> {
    let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    format_reset_countdown(resets_at, now_secs)
}

/// Formats an ISO-8601 reset timestamp relative to `now_unix_secs`.
pub fn format_reset_countdown(resets_at: &str, now_unix_secs: i64) -> Option<String> {
    let target_secs = parse_rfc3339_unix_secs(resets_at)?;
    let diff = target_secs - now_unix_secs;
    if diff <= 60 {
        return Some("Resets soon".to_owned());
    }
    let total_mins = (diff + 59) / 60;
    let days = total_mins / (24 * 60);
    let hours = (total_mins % (24 * 60)) / 60;
    let mins = total_mins % 60;
    if days > 0 {
        if hours > 0 {
            Some(format!("Resets in {days}d {hours}h"))
        } else {
            Some(format!("Resets in {days}d"))
        }
    } else if hours > 0 {
        if mins > 0 {
            Some(format!("Resets in {hours}h {mins}m"))
        } else {
            Some(format!("Resets in {hours}h"))
        }
    } else {
        Some(format!("Resets in {mins}m"))
    }
}

#[derive(Debug, Deserialize)]
struct ControlEnvelope {
    #[serde(rename = "type")]
    kind: Option<String>,
    response: Option<ControlResponseBody>,
}

#[derive(Debug, Deserialize)]
struct ControlResponseBody {
    subtype: Option<String>,
    request_id: Option<String>,
    response: Option<serde_json::Value>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InitControlResponse {
    #[serde(default)]
    models: Vec<RawClaudeModel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawClaudeModel {
    #[serde(default)]
    value: String,
    #[serde(default)]
    resolved_model: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Deserialize)]
struct UsageControlResponse {
    #[serde(default)]
    rate_limits_available: bool,
    #[serde(default)]
    rate_limits: Option<RawRateLimits>,
}

#[derive(Debug, Deserialize)]
struct RawRateLimits {
    #[serde(default)]
    five_hour: Option<RawLegacyWindow>,
    #[serde(default)]
    seven_day: Option<RawLegacyWindow>,
    #[serde(default)]
    limits: Vec<RawLimitEntry>,
    #[serde(default)]
    model_scoped: Vec<RawModelScopedWindow>,
    #[serde(default)]
    extra_usage: Option<RawExtraUsage>,
}

#[derive(Debug, Deserialize)]
struct RawLegacyWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawModelScopedWindow {
    display_name: Option<String>,
    utilization: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawLimitEntry {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    group: String,
    percent: Option<f64>,
    resets_at: Option<String>,
    #[serde(default)]
    severity: String,
    #[serde(default)]
    scope: Option<RawLimitScope>,
}

#[derive(Debug, Deserialize)]
struct RawLimitScope {
    model: Option<RawLimitScopeModel>,
}

#[derive(Debug, Deserialize)]
struct RawLimitScopeModel {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawExtraUsage {
    #[serde(default)]
    is_enabled: bool,
    monthly_limit: Option<f64>,
    used_credits: Option<f64>,
    currency: Option<String>,
}

fn normalize_model_options(raw_models: Vec<RawClaudeModel>) -> Vec<ClaudeModelOption> {
    let has_non_default_match = |resolved: &str| {
        !resolved.is_empty()
            && raw_models
                .iter()
                .any(|m| m.value != "default" && m.resolved_model == resolved)
    };

    let mut out = Vec::new();
    let mut seen_values = Vec::new();
    for raw in &raw_models {
        let value = raw.value.trim();
        let resolved = raw.resolved_model.trim();
        if value.is_empty() && resolved.is_empty() {
            continue;
        }
        // Skip the synthetic `"default"` row when its resolved model is already listed explicitly
        // (e.g. `"opus"` -> `"claude-opus-5-5"`).
        if value == "default" && has_non_default_match(resolved) {
            continue;
        }
        let chosen_value = if value == "default" && !resolved.is_empty() {
            resolved.to_owned()
        } else if !value.is_empty() {
            value.to_owned()
        } else {
            resolved.to_owned()
        };
        if seen_values.contains(&chosen_value) {
            continue;
        }
        seen_values.push(chosen_value.clone());

        let resolved_model = if resolved.is_empty() {
            chosen_value.clone()
        } else {
            resolved.to_owned()
        };
        let display_name = if raw.display_name.trim().is_empty() {
            chosen_value.clone()
        } else {
            raw.display_name.trim().to_owned()
        };
        out.push(ClaudeModelOption {
            value: chosen_value,
            resolved_model,
            display_name,
            description: raw.description.trim().to_owned(),
        });
    }
    out
}

pub(crate) fn clamp_percent(raw: f64) -> u8 {
    if !raw.is_finite() {
        return 0;
    }
    raw.round().clamp(0.0, 100.0) as u8
}

fn build_usage_summary(resp: UsageControlResponse) -> ClaudeUsageSummary {
    let Some(limits) = resp.rate_limits else {
        return ClaudeUsageSummary {
            rate_limits_available: resp.rate_limits_available,
            windows: Vec::new(),
            extra_usage_note: None,
        };
    };

    let mut windows = Vec::new();
    if !limits.limits.is_empty() {
        for entry in &limits.limits {
            let Some(pct) = entry.percent else {
                continue;
            };
            let used = clamp_percent(pct);
            let label = format_limit_entry_label(entry);
            let severity = if entry.severity.trim().is_empty() {
                default_severity_for_used(used).to_owned()
            } else {
                entry.severity.trim().to_owned()
            };
            windows.push(ClaudeUsageWindow {
                label,
                used_percent: used,
                remaining_percent: 100u8.saturating_sub(used),
                resets_at: entry.resets_at.clone(),
                severity,
            });
        }
    } else {
        if let Some(five_h) = &limits.five_hour
            && let Some(util) = five_h.utilization
        {
            let used = clamp_percent(util);
            windows.push(ClaudeUsageWindow {
                label: "Current session (5h)".to_owned(),
                used_percent: used,
                remaining_percent: 100u8.saturating_sub(used),
                resets_at: five_h.resets_at.clone(),
                severity: default_severity_for_used(used).to_owned(),
            });
        }
        if let Some(seven_d) = &limits.seven_day
            && let Some(util) = seven_d.utilization
        {
            let used = clamp_percent(util);
            windows.push(ClaudeUsageWindow {
                label: "Weekly (all models)".to_owned(),
                used_percent: used,
                remaining_percent: 100u8.saturating_sub(used),
                resets_at: seven_d.resets_at.clone(),
                severity: default_severity_for_used(used).to_owned(),
            });
        }
        for scoped in &limits.model_scoped {
            if let Some(util) = scoped.utilization {
                let used = clamp_percent(util);
                let model = scoped
                    .display_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Model");
                windows.push(ClaudeUsageWindow {
                    label: format!("Weekly ({model})"),
                    used_percent: used,
                    remaining_percent: 100u8.saturating_sub(used),
                    resets_at: scoped.resets_at.clone(),
                    severity: default_severity_for_used(used).to_owned(),
                });
            }
        }
    }

    let extra_usage_note = limits.extra_usage.and_then(|extra| {
        if !extra.is_enabled {
            return None;
        }
        let currency = extra.currency.as_deref().unwrap_or("USD");
        match (extra.used_credits, extra.monthly_limit) {
            (Some(used), Some(limit)) => Some(format!(
                "Extra usage enabled: {used:.2} / {limit:.2} {currency} used"
            )),
            (Some(used), None) => Some(format!("Extra usage enabled: {used:.2} {currency} used")),
            _ => Some("Extra usage credits enabled.".to_owned()),
        }
    });

    ClaudeUsageSummary {
        rate_limits_available: resp.rate_limits_available || !windows.is_empty(),
        windows,
        extra_usage_note,
    }
}

fn format_limit_entry_label(entry: &RawLimitEntry) -> String {
    let model_name = entry
        .scope
        .as_ref()
        .and_then(|s| s.model.as_ref())
        .and_then(|m| m.display_name.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    match entry.kind.as_str() {
        "session" => "Current session (5h)".to_owned(),
        "weekly_all" => "Weekly (all models)".to_owned(),
        "weekly_scoped" => match model_name {
            Some(model) => format!("Weekly ({model})"),
            None => "Weekly (model limit)".to_owned(),
        },
        other => match (entry.group.as_str(), model_name) {
            ("weekly", Some(model)) => format!("Weekly ({model})"),
            ("session", _) => "Current session (5h)".to_owned(),
            _ if !other.is_empty() => humanize_plan_name(other),
            _ => "Usage limit".to_owned(),
        },
    }
}

pub(crate) fn default_severity_for_used(used: u8) -> &'static str {
    if used >= 90 {
        "critical"
    } else if used >= 75 {
        "warning"
    } else {
        "normal"
    }
}

fn parse_rfc3339_unix_secs(input: &str) -> Option<i64> {
    let s = input.trim();
    let (date_part, time_and_tz) = s.split_once('T').or_else(|| s.split_once('t'))?;
    let mut date_iter = date_part.split('-');
    let year: i64 = date_iter.next()?.parse().ok()?;
    let month: u32 = date_iter.next()?.parse().ok()?;
    let day: u32 = date_iter.next()?.parse().ok()?;
    if date_iter.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let (time_part, offset_secs): (&str, i64) =
        if let Some(stripped) = time_and_tz.strip_suffix(['Z', 'z']) {
            (stripped, 0)
        } else {
            let idx = time_and_tz.rfind(['+', '-'])?;
            let (t, off) = time_and_tz.split_at(idx);
            let sign: i64 = if off.starts_with('-') { -1 } else { 1 };
            let (oh, om) = off[1..].split_once(':')?;
            let oh: i64 = oh.parse().ok()?;
            let om: i64 = om.parse().ok()?;
            (t, sign * (oh * 3600 + om * 60))
        };

    let main_time = time_part.split('.').next()?;
    let mut time_iter = main_time.split(':');
    let hour: i64 = time_iter.next()?.parse().ok()?;
    let minute: i64 = time_iter.next()?.parse().ok()?;
    let second: i64 = time_iter.next()?.parse().ok()?;
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }

    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset_secs)
}

/// Howard Hinnant's civil-to-days algorithm (days since 1970-01-01).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(month);
    let d = i64::from(day);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parses the output of `claude auth status --json` into a typed [`ClaudeCliProbe`].
pub fn parse_claude_auth_status(
    binary_path: PathBuf,
    stdout: &str,
    stderr: &str,
    success: bool,
) -> ClaudeCliProbe {
    let parsed = parse_json_object(stdout).or_else(|| parse_json_object(stderr));

    let Some(status) = parsed else {
        let detail = if !stderr.trim().is_empty() {
            stderr.trim().to_owned()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_owned()
        } else if success {
            "Claude CLI returned empty auth status output.".to_owned()
        } else {
            "Claude CLI auth status check failed.".to_owned()
        };
        return ClaudeCliProbe::ProbeFailed {
            binary_path,
            message: detail,
        };
    };

    let config_dir = status
        .config_directory
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| "~/.claude".to_owned());

    if !status.logged_in {
        return ClaudeCliProbe::NotAuthenticated {
            binary_path,
            config_dir,
        };
    }

    let auth_method = status
        .auth_method
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("system")
        .to_owned();

    let email = resolve_account_label(&status, &auth_method);
    let plan = format_plan_label(&status, &auth_method);

    ClaudeCliProbe::Authenticated {
        binary_path,
        email,
        plan,
        auth_method,
        config_dir,
    }
}

fn parse_json_object(raw: &str) -> Option<ClaudeAuthStatusJson> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(parsed) = serde_json::from_str::<ClaudeAuthStatusJson>(trimmed) {
        return Some(parsed);
    }
    // Fallback if a wrapper script wrote banner text before/after the JSON payload.
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end < start {
        return None;
    }
    serde_json::from_str::<ClaudeAuthStatusJson>(&trimmed[start..=end]).ok()
}

fn resolve_account_label(status: &ClaudeAuthStatusJson, auth_method: &str) -> String {
    if let Some(email) = status
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return email.to_owned();
    }
    if auth_method.eq_ignore_ascii_case("api_key") {
        if let Some(source) = status
            .api_key_source
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return format!("API key ({source})");
        }
        return "API key".to_owned();
    }
    if let Some(org) = status
        .org_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return org.to_owned();
    }
    if !auth_method.is_empty() && !auth_method.eq_ignore_ascii_case("none") {
        return format!("System login ({auth_method})");
    }
    "System account".to_owned()
}

fn format_plan_label(status: &ClaudeAuthStatusJson, auth_method: &str) -> String {
    if let Some(tier) = status
        .plan_tier
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return humanize_plan_name(tier);
    }
    if let Some(sub) = status
        .subscription_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return humanize_plan_name(sub);
    }
    if auth_method.eq_ignore_ascii_case("api_key") {
        return "API billing".to_owned();
    }
    if let Some(provider) = status
        .api_provider
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("firstParty"))
    {
        return humanize_plan_name(provider);
    }
    if auth_method.eq_ignore_ascii_case("claude.ai") {
        return "Claude.ai".to_owned();
    }
    String::new()
}

fn humanize_plan_name(raw: &str) -> String {
    match raw.to_ascii_lowercase().as_str() {
        "max" => "Max".to_owned(),
        "max_5x" => "Max 5x".to_owned(),
        "max_20x" => "Max 20x".to_owned(),
        "pro" => "Pro".to_owned(),
        "team" => "Team".to_owned(),
        "enterprise" => "Enterprise".to_owned(),
        "free" => "Free".to_owned(),
        "bedrock" => "AWS Bedrock".to_owned(),
        "vertex" => "Google Vertex".to_owned(),
        "foundry" => "Azure Foundry".to_owned(),
        other => other
            .split(['_', '-'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn validate_executable_file(path: PathBuf) -> Result<PathBuf, ClaudeBinaryError> {
    let metadata = match fs::metadata(&path) {
        Ok(meta) => meta,
        Err(_) => return Err(ClaudeBinaryError::CustomPathNotFound(path)),
    };
    if !metadata.is_file() {
        return Err(ClaudeBinaryError::NotAFile(path));
    }
    if !metadata_is_executable(&metadata) {
        return Err(ClaudeBinaryError::NotExecutable(path));
    }
    Ok(path)
}

pub(crate) fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && metadata_is_executable(&metadata)
}

#[cfg(unix)]
pub(crate) fn metadata_is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
pub(crate) fn metadata_is_executable(_metadata: &fs::Metadata) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use tempfile::TempDir;

    use super::*;

    #[cfg(unix)]
    fn write_fake_claude_script(dir: &Path, name: &str, body: &str, executable: bool) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        let mode = if executable { 0o755 } else { 0o644 };
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn parses_authenticated_claude_ai_status_json() {
        let stdout = r#"{
            "loggedIn": true,
            "authMethod": "claude.ai",
            "apiProvider": "firstParty",
            "configDirectory": "/Users/dev/.claude",
            "email": "dev@example.com",
            "subscriptionType": "max",
            "planTier": "max_5x"
        }"#;
        let probe =
            parse_claude_auth_status(PathBuf::from("/usr/local/bin/claude"), stdout, "", true);
        assert_eq!(
            probe,
            ClaudeCliProbe::Authenticated {
                binary_path: PathBuf::from("/usr/local/bin/claude"),
                email: "dev@example.com".to_owned(),
                plan: "Max 5x".to_owned(),
                auth_method: "claude.ai".to_owned(),
                config_dir: "/Users/dev/.claude".to_owned(),
            }
        );
    }

    #[test]
    fn parses_authenticated_api_key_status_json() {
        let stdout = r#"{
            "loggedIn": true,
            "authMethod": "api_key",
            "apiProvider": "firstParty",
            "configDirectory": "/Users/dev/.claude",
            "apiKeySource": "ANTHROPIC_API_KEY"
        }"#;
        let probe = parse_claude_auth_status(PathBuf::from("/opt/bin/claude"), stdout, "", true);
        assert_eq!(
            probe,
            ClaudeCliProbe::Authenticated {
                binary_path: PathBuf::from("/opt/bin/claude"),
                email: "API key (ANTHROPIC_API_KEY)".to_owned(),
                plan: "API billing".to_owned(),
                auth_method: "api_key".to_owned(),
                config_dir: "/Users/dev/.claude".to_owned(),
            }
        );
    }

    #[test]
    fn parses_logged_out_status_json_even_with_nonzero_exit() {
        let stdout = r#"{
            "loggedIn": false,
            "authMethod": "none",
            "configDirectory": "/Users/dev/.claude"
        }"#;
        let probe =
            parse_claude_auth_status(PathBuf::from("/usr/local/bin/claude"), stdout, "", false);
        assert_eq!(
            probe,
            ClaudeCliProbe::NotAuthenticated {
                binary_path: PathBuf::from("/usr/local/bin/claude"),
                config_dir: "/Users/dev/.claude".to_owned(),
            }
        );
    }

    #[test]
    fn handles_invalid_status_output_as_probe_failed() {
        let probe = parse_claude_auth_status(
            PathBuf::from("/usr/local/bin/claude"),
            "not json",
            "unexpected flag --json",
            false,
        );
        assert_eq!(
            probe,
            ClaudeCliProbe::ProbeFailed {
                binary_path: PathBuf::from("/usr/local/bin/claude"),
                message: "unexpected flag --json".to_owned(),
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolves_and_probes_claude_binary_from_path_and_custom_path() {
        let temp = TempDir::new().unwrap();
        let path_dir = temp.path().join("path-bin");
        let custom_dir = temp.path().join("custom-bin");
        let empty_dir = temp.path().join("empty-bin");
        fs::create_dir_all(&path_dir).unwrap();
        fs::create_dir_all(&custom_dir).unwrap();
        fs::create_dir_all(&empty_dir).unwrap();

        let path_claude = write_fake_claude_script(
            &path_dir,
            "claude",
            r#"#!/bin/sh
echo '{"loggedIn":true,"authMethod":"claude.ai","email":"path@example.com","subscriptionType":"pro","configDirectory":"/tmp/.claude-path"}'
"#,
            true,
        );
        let custom_claude = write_fake_claude_script(
            &custom_dir,
            "claude-custom",
            r#"#!/bin/sh
echo '{"loggedIn":false,"authMethod":"none","configDirectory":"/tmp/.claude-custom"}'
exit 1
"#,
            true,
        );
        let non_exec = write_fake_claude_script(&custom_dir, "claude-noexec", "#!/bin/sh\n", false);

        // Found on PATH and authenticated.
        let probe = probe_claude_cli(None, Some(path_dir.as_os_str()));
        assert_eq!(
            probe,
            ClaudeCliProbe::Authenticated {
                binary_path: path_claude,
                email: "path@example.com".to_owned(),
                plan: "Pro".to_owned(),
                auth_method: "claude.ai".to_owned(),
                config_dir: "/tmp/.claude-path".to_owned(),
            }
        );

        // Missing on empty PATH.
        let missing = probe_claude_cli(None, Some(empty_dir.as_os_str()));
        assert_eq!(
            missing,
            ClaudeCliProbe::BinaryNotFound {
                error: ClaudeBinaryError::NotFoundOnPath,
            }
        );

        // Custom path overrides empty PATH and reports NotAuthenticated.
        let custom_str = custom_claude.to_string_lossy().into_owned();
        let custom_probe = probe_claude_cli(Some(&custom_str), Some(OsString::new().as_os_str()));
        assert_eq!(
            custom_probe,
            ClaudeCliProbe::NotAuthenticated {
                binary_path: custom_claude,
                config_dir: "/tmp/.claude-custom".to_owned(),
            }
        );

        // Invalid custom paths return specific errors.
        assert!(matches!(
            resolve_claude_binary(Some(custom_dir.to_str().unwrap()), None),
            Err(ClaudeBinaryError::NotAFile(_))
        ));
        assert!(matches!(
            resolve_claude_binary(Some(non_exec.to_str().unwrap()), None),
            Err(ClaudeBinaryError::NotExecutable(_))
        ));
        assert!(matches!(
            resolve_claude_binary(Some("/nonexistent/path/to/claude"), None),
            Err(ClaudeBinaryError::CustomPathNotFound(_))
        ));
    }

    #[test]
    fn parses_stream_json_models_and_usage_control_responses() {
        let stdout = r#"{"type":"control_response","response":{"subtype":"success","request_id":"bruk_init","response":{"models":[{"value":"default","resolvedModel":"claude-opus-5-5","displayName":"Default (recommended)","description":"Opus 5.5"},{"value":"opus","resolvedModel":"claude-opus-5-5","displayName":"Opus 5.5","description":"For complex work"},{"value":"sonnet","resolvedModel":"claude-sonnet-5-5","displayName":"Sonnet 5.5","description":"Most efficient"},{"value":"claude-sonnet-4-6","resolvedModel":"claude-sonnet-4-6","displayName":"Sonnet 4.6","description":"Routine tasks"}]}}}
{"type":"control_response","response":{"subtype":"success","request_id":"bruk_usage","response":{"rate_limits_available":true,"rate_limits":{"limits":[{"kind":"session","group":"session","percent":15,"resets_at":"2026-11-22T23:59:59+00:00","severity":"normal","scope":null},{"kind":"weekly_all","group":"weekly","percent":20,"resets_at":"2026-11-29T20:00:00+00:00","severity":"normal","scope":null},{"kind":"weekly_scoped","group":"weekly","percent":5,"resets_at":"2026-11-29T20:00:00+00:00","severity":"normal","scope":{"model":{"display_name":"Fable"}}}]}}}}"#;

        let details = parse_claude_details_stream(stdout, "");
        assert_eq!(details.warning, None);
        assert_eq!(details.models.len(), 3);
        assert_eq!(details.models[0].value, "opus");
        assert_eq!(details.models[0].display_name, "Opus 5.5");
        assert_eq!(details.models[1].value, "sonnet");
        assert_eq!(details.models[2].value, "claude-sonnet-4-6");

        let usage = details.usage.expect("usage summary parsed");
        assert!(usage.rate_limits_available);
        assert_eq!(usage.windows.len(), 3);
        assert_eq!(usage.windows[0].label, "Current session (5h)");
        assert_eq!(usage.windows[0].used_percent, 15);
        assert_eq!(usage.windows[0].remaining_percent, 85);
        assert_eq!(usage.windows[1].label, "Weekly (all models)");
        assert_eq!(usage.windows[1].remaining_percent, 80);
        assert_eq!(usage.windows[2].label, "Weekly (Fable)");
        assert_eq!(usage.windows[2].remaining_percent, 95);
    }

    #[test]
    fn formats_rfc3339_reset_countdowns() {
        let now = parse_rfc3339_unix_secs("2026-11-22T20:00:00+00:00").unwrap();
        assert_eq!(
            format_reset_countdown("2026-11-22T20:00:30+00:00", now).as_deref(),
            Some("Resets soon")
        );
        assert_eq!(
            format_reset_countdown("2026-11-22T20:42:00Z", now).as_deref(),
            Some("Resets in 42m")
        );
        assert_eq!(
            format_reset_countdown("2026-11-22T23:15:00+00:00", now).as_deref(),
            Some("Resets in 3h 15m")
        );
        assert_eq!(
            format_reset_countdown("2026-11-29T20:00:00+00:00", now).as_deref(),
            Some("Resets in 7d")
        );
    }

    #[test]
    fn derives_model_family_from_display_name_and_model_ids() {
        let fable = ClaudeModelOption {
            value: "fable".to_owned(),
            resolved_model: "claude-fable-5-2".to_owned(),
            display_name: "Fable 5.2".to_owned(),
            description: String::new(),
        };
        assert_eq!(fable.family(), "Fable");

        let sonnet = ClaudeModelOption {
            value: "claude-sonnet-4-6".to_owned(),
            resolved_model: "claude-sonnet-4-6".to_owned(),
            display_name: "Claude Sonnet 4.6".to_owned(),
            description: String::new(),
        };
        assert_eq!(sonnet.family(), "Sonnet");

        let id_only = ClaudeModelOption {
            value: "claude-haiku-4-5-20251001".to_owned(),
            resolved_model: "claude-haiku-4-5-20251001".to_owned(),
            display_name: String::new(),
            description: String::new(),
        };
        assert_eq!(id_only.family(), "Haiku");

        assert_eq!(
            model_family_from_id("claude-opus-4-6").as_deref(),
            Some("Opus")
        );
        assert_eq!(model_family_from_id("fable").as_deref(), Some("Fable"));
    }

    #[test]
    fn formats_local_reset_schedules_and_sorts_upcoming_resets() {
        let now = DateTime::parse_from_rfc3339("2026-11-22T16:25:00-05:00").unwrap();
        assert_eq!(
            format_reset_schedule("2026-11-22T23:59:59.838563+00:00", now).as_deref(),
            Some("Today at 7:00 PM · in 2h 35m")
        );
        assert_eq!(
            format_reset_schedule("2026-11-23T14:00:00-05:00", now).as_deref(),
            Some("Tomorrow at 2:00 PM · in 21h 35m")
        );
        assert_eq!(
            format_reset_schedule("2026-11-29T19:59:59.838580+00:00", now).as_deref(),
            Some("Sun, Nov 29 at 3:00 PM · in 6d 22h")
        );

        let summary = ClaudeUsageSummary {
            rate_limits_available: true,
            windows: vec![
                ClaudeUsageWindow {
                    label: "Weekly (all models)".to_owned(),
                    used_percent: 10,
                    remaining_percent: 90,
                    resets_at: Some("2026-11-29T20:00:00+00:00".to_owned()),
                    severity: "normal".to_owned(),
                },
                ClaudeUsageWindow {
                    label: "Current session (5h)".to_owned(),
                    used_percent: 5,
                    remaining_percent: 95,
                    resets_at: Some("2026-11-22T23:59:59+00:00".to_owned()),
                    severity: "normal".to_owned(),
                },
                ClaudeUsageWindow {
                    label: "Window without reset".to_owned(),
                    used_percent: 0,
                    remaining_percent: 100,
                    resets_at: None,
                    severity: "normal".to_owned(),
                },
            ],
            extra_usage_note: None,
        };
        let resets = summary.upcoming_resets();
        assert_eq!(resets.len(), 2);
        assert_eq!(resets[0].label, "Current session (5h)");
        assert_eq!(resets[1].label, "Weekly (all models)");
    }
}
