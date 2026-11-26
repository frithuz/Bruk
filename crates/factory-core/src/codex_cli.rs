//! Discovery, authentication probing, model catalog, and usage probing for the local `codex` CLI.
//!
//! Codex runs with the user's existing system login (`~/.codex`) rather than an isolated
//! credential store. Bruk locates the `codex` executable either from a user-configured
//! custom path or from the default `PATH`, then talks to `codex app-server --stdio` over
//! JSON-RPC 2.0 (`initialize`, `account/read`, `account/rateLimits/read`, `model/list`)
//! without spending any inference tokens.

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use chrono::DateTime;
use serde::Deserialize;
use thiserror::Error;

use crate::{
    claude_cli::{
        ClaudeModelOption, ClaudeUsageSummary, ClaudeUsageWindow, clamp_percent,
        default_severity_for_used, expand_tilde, is_executable_file, metadata_is_executable,
    },
    codex_settings::new_codex_subcommand,
};

/// Default executable filename searched on `PATH`.
#[cfg(windows)]
pub const CODEX_BINARY_NAME: &str = "codex.exe";
/// Default executable filename searched on `PATH`.
#[cfg(not(windows))]
pub const CODEX_BINARY_NAME: &str = "codex";

const RPC_INIT_ID: u64 = 1;
const RPC_ACCOUNT_ID: u64 = 2;
const RPC_RATE_LIMITS_ID: u64 = 2;
const RPC_MODELS_ID: u64 = 3;

const APP_SERVER_PROBE_TIMEOUT: Duration = Duration::from_secs(12);
const APP_SERVER_DETAILS_TIMEOUT: Duration = Duration::from_secs(15);

/// Errors returned when locating the `codex` executable.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CodexBinaryError {
    #[error("Codex CLI ('codex') was not found on the default PATH.")]
    NotFoundOnPath,
    #[error("Codex CLI was not found at '{}'.", .0.display())]
    CustomPathNotFound(PathBuf),
    #[error("'{}' is a directory, not a Codex CLI executable.", .0.display())]
    NotAFile(PathBuf),
    #[error("'{}' is not executable.", .0.display())]
    NotExecutable(PathBuf),
}

impl CodexBinaryError {
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

/// Result of probing the system's `codex` CLI binary and login state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexCliProbe {
    /// Binary exists and `account/read` proved active authentication.
    Authenticated {
        binary_path: PathBuf,
        email: String,
        plan: String,
        auth_method: String,
        config_dir: String,
    },
    /// Binary exists, but `account/read` reported no active login (`account: null`).
    NotAuthenticated {
        binary_path: PathBuf,
        config_dir: String,
    },
    /// No executable `codex` binary was found on `PATH` or at the configured custom path.
    BinaryNotFound { error: CodexBinaryError },
    /// Binary was found, but probing `codex app-server --stdio` failed unexpectedly.
    ProbeFailed {
        binary_path: PathBuf,
        message: String,
    },
}

/// Live model catalog and usage details queried from the Codex CLI over `app-server --stdio`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodexCliDetails {
    pub models: Vec<ClaudeModelOption>,
    pub usage: Option<ClaudeUsageSummary>,
    pub warning: Option<String>,
}

/// Resolves the `codex` CLI binary using an optional custom path override or the given `PATH`.
pub fn resolve_codex_binary(
    custom_path: Option<&str>,
    path_env: Option<&OsStr>,
) -> Result<PathBuf, CodexBinaryError> {
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
            let candidate = dir.join(CODEX_BINARY_NAME);
            if is_executable_file(&candidate) {
                return Ok(candidate);
            }
        }
    }

    Err(CodexBinaryError::NotFoundOnPath)
}

/// Resolves the `codex` CLI binary using the current process's `PATH` environment variable.
pub fn resolve_codex_binary_from_env(
    custom_path: Option<&str>,
) -> Result<PathBuf, CodexBinaryError> {
    let path_env = env::var_os("PATH");
    resolve_codex_binary(custom_path, path_env.as_deref())
}

/// Probes the `codex` CLI binary and checks whether the system already has an active login.
pub fn probe_codex_cli(custom_path: Option<&str>, path_env: Option<&OsStr>) -> CodexCliProbe {
    let started = Instant::now();
    let binary_path = match resolve_codex_binary(custom_path, path_env) {
        Ok(path) => path,
        Err(error) => {
            log::debug!("codex.probe binary_not_found err={error}");
            return CodexCliProbe::BinaryNotFound { error };
        }
    };

    let account_req = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{RPC_ACCOUNT_ID},\"method\":\"account/read\",\"params\":{{\"refreshToken\":false}}}}"
    );
    let (stdout, stderr) = match run_app_server_session(
        &binary_path,
        &[account_req],
        &[RPC_INIT_ID, RPC_ACCOUNT_ID],
        APP_SERVER_PROBE_TIMEOUT,
    ) {
        Ok(out) => out,
        Err(message) => {
            log::warn!(
                "codex.probe outcome=probe_failed binary={} elapsed_ms={} err={message}",
                binary_path.display(),
                started.elapsed().as_millis()
            );
            return CodexCliProbe::ProbeFailed {
                binary_path,
                message,
            };
        }
    };

    let probe = parse_codex_account_stream(binary_path, &stdout, &stderr);
    let elapsed_ms = started.elapsed().as_millis();
    match &probe {
        CodexCliProbe::Authenticated {
            binary_path, plan, ..
        } => log::info!(
            "codex.probe outcome=authenticated binary={} plan={plan:?} elapsed_ms={elapsed_ms}",
            binary_path.display()
        ),
        CodexCliProbe::NotAuthenticated { binary_path, .. } => log::info!(
            "codex.probe outcome=not_authenticated binary={} elapsed_ms={elapsed_ms}",
            binary_path.display()
        ),
        CodexCliProbe::ProbeFailed {
            binary_path,
            message,
        } => log::warn!(
            "codex.probe outcome=probe_failed binary={} elapsed_ms={elapsed_ms} err={message}",
            binary_path.display()
        ),
        CodexCliProbe::BinaryNotFound { error } => {
            log::debug!("codex.probe outcome=binary_not_found elapsed_ms={elapsed_ms} err={error}")
        }
    }
    probe
}

/// Probes the `codex` CLI using the current process's `PATH` environment variable.
pub fn probe_codex_cli_from_env(custom_path: Option<&str>) -> CodexCliProbe {
    let path_env = env::var_os("PATH");
    probe_codex_cli(custom_path, path_env.as_deref())
}

/// Runs `codex login` so the user can authenticate their system Codex CLI account.
pub fn run_codex_login(binary_path: &Path) -> Result<(), String> {
    log::info!("codex.login.start binary={}", binary_path.display());
    let output = new_codex_subcommand(binary_path, &["login"])
        .stdin(Stdio::null())
        .output()
        .map_err(|err| {
            log::error!("codex.login.spawn_failed err={err}");
            format!("Failed to start Codex sign-in: {err}")
        })?;

    if output.status.success() {
        log::info!("codex.login.succeeded");
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let message = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("Codex sign-in exited with status {}.", output.status)
    };
    log::warn!("codex.login.failed err={message}");
    Err(message)
}

/// Queries the local `codex` CLI for its live available models and plan usage remaining
/// using the JSON-RPC `app-server --stdio` protocol (`initialize`, `account/rateLimits/read`,
/// and `model/list`).
///
/// This spends zero inference tokens and uses the CLI's own system authentication.
pub fn fetch_codex_details(binary_path: &Path) -> CodexCliDetails {
    let started = Instant::now();
    let rate_limits_req = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{RPC_RATE_LIMITS_ID},\"method\":\"account/rateLimits/read\",\"params\":null}}"
    );
    let models_req = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{RPC_MODELS_ID},\"method\":\"model/list\",\"params\":{{\"includeHidden\":false,\"limit\":100}}}}"
    );

    let (stdout, stderr) = match run_app_server_session(
        binary_path,
        &[rate_limits_req, models_req],
        &[RPC_INIT_ID, RPC_RATE_LIMITS_ID, RPC_MODELS_ID],
        APP_SERVER_DETAILS_TIMEOUT,
    ) {
        Ok(out) => out,
        Err(message) => {
            log::warn!(
                "codex.details.failed binary={} elapsed_ms={} err={message}",
                binary_path.display(),
                started.elapsed().as_millis()
            );
            return CodexCliDetails {
                models: Vec::new(),
                usage: None,
                warning: Some(message),
            };
        }
    };

    let details = parse_codex_details_stream(&stdout, &stderr);
    let elapsed_ms = started.elapsed().as_millis();
    if let Some(warn) = details.warning.as_deref() {
        log::warn!(
            "codex.details.done models={} has_usage={} elapsed_ms={elapsed_ms} warning={warn}",
            details.models.len(),
            details.usage.is_some()
        );
    } else {
        log::info!(
            "codex.details.done models={} has_usage={} elapsed_ms={elapsed_ms}",
            details.models.len(),
            details.usage.is_some()
        );
    }
    details
}

/// Runs a short-lived `codex app-server --stdio` session, sending `initialize`, waiting for its
/// response, then sending `initialized` and `post_init_requests`, collecting JSON-RPC responses
/// until all `expected_ids` arrive or `timeout` elapses, and cleanly closing stdin.
fn run_app_server_session(
    binary_path: &Path,
    post_init_requests: &[String],
    expected_ids: &[u64],
    timeout: Duration,
) -> Result<(String, String), String> {
    let mut child = new_codex_subcommand(binary_path, &["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Failed to run Codex CLI: {err}"))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Failed to open Codex CLI stdin pipe.".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to open Codex CLI stdout pipe.".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to open Codex CLI stderr pipe.".to_owned())?;

    let (line_tx, line_rx) = mpsc::channel::<String>();
    let stdout_handle = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(text) => {
                    if line_tx.send(text).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let stderr_handle = thread::spawn(move || {
        let mut buf = String::new();
        let mut reader = BufReader::new(stderr);
        let _ = reader.read_to_string(&mut buf);
        buf
    });

    let init_req = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{RPC_INIT_ID},\"method\":\"initialize\",\"params\":{{\"clientInfo\":{{\"name\":\"bruk\",\"version\":\"0.1.0\"}}}}}}\n"
    );
    let _ = stdin.write_all(init_req.as_bytes());
    let _ = stdin.flush();

    let deadline = Instant::now() + timeout;
    let mut collected_lines = Vec::new();
    let mut received_ids = BTreeSet::new();
    let expected_set: BTreeSet<u64> = expected_ids.iter().copied().collect();

    // Wait for the `initialize` response first so real `codex app-server` completes its handshake.
    let mut stdout_closed = false;
    while !received_ids.contains(&RPC_INIT_ID) {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        match line_rx.recv_timeout(remaining) {
            Ok(line) => {
                if let Some(id) = extract_jsonrpc_id(&line) {
                    received_ids.insert(id);
                }
                collected_lines.push(line);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                stdout_closed = true;
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => break,
        }
    }

    if !stdout_closed && !expected_set.is_subset(&received_ids) {
        let mut batch =
            String::from("{\"jsonrpc\":\"2.0\",\"method\":\"initialized\",\"params\":{}}\n");
        for req in post_init_requests {
            batch.push_str(req);
            batch.push('\n');
        }
        let _ = stdin.write_all(batch.as_bytes());
        let _ = stdin.flush();

        while !expected_set.is_subset(&received_ids) {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            match line_rx.recv_timeout(remaining) {
                Ok(line) => {
                    if let Some(id) = extract_jsonrpc_id(&line) {
                        received_ids.insert(id);
                    }
                    collected_lines.push(line);
                }
                Err(mpsc::RecvTimeoutError::Disconnected | mpsc::RecvTimeoutError::Timeout) => {
                    break;
                }
            }
        }
    }

    // Drop stdin so `codex app-server --stdio` observes EOF and exits cleanly.
    drop(stdin);

    let exit_deadline = Instant::now() + Duration::from_millis(600);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= exit_deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                thread::sleep(Duration::from_millis(15));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }

    // Drain any buffered lines emitted right as the child exited.
    while let Ok(line) = line_rx.try_recv() {
        collected_lines.push(line);
    }

    let _ = stdout_handle.join();
    let stderr_text = stderr_handle.join().unwrap_or_default();

    Ok((collected_lines.join("\n"), stderr_text))
}

fn extract_jsonrpc_id(line: &str) -> Option<u64> {
    let trimmed = line.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let envelope: JsonRpcEnvelope = serde_json::from_str(trimmed).ok()?;
    envelope.id_u64()
}

/// Parses NDJSON JSON-RPC responses from `codex app-server --stdio` (`initialize` + `account/read`)
/// into a typed [`CodexCliProbe`].
pub fn parse_codex_account_stream(
    binary_path: PathBuf,
    stdout: &str,
    stderr: &str,
) -> CodexCliProbe {
    let mut config_dir: Option<String> = None;
    let mut account_response: Option<GetAccountResult> = None;
    let mut rpc_error: Option<String> = None;

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        let Ok(envelope) = serde_json::from_str::<JsonRpcEnvelope>(trimmed) else {
            continue;
        };
        let Some(id) = envelope.id_u64() else {
            continue;
        };

        if id == RPC_INIT_ID {
            if let Some(result) = envelope.result
                && let Ok(init) = serde_json::from_value::<InitializeResult>(result)
                && let Some(home) = init
                    .codex_home
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            {
                config_dir = Some(home.to_owned());
            } else if let Some(err) = envelope.error {
                rpc_error = Some(err.message);
            }
        } else if id == RPC_ACCOUNT_ID {
            if let Some(result) = envelope.result
                && let Ok(parsed) = serde_json::from_value::<GetAccountResult>(result)
            {
                account_response = Some(parsed);
            } else if let Some(err) = envelope.error {
                rpc_error = Some(err.message);
            }
        }
    }

    let resolved_config_dir = config_dir.unwrap_or_else(|| "~/.codex".to_owned());

    let Some(account_res) = account_response else {
        let message = if let Some(err) = rpc_error {
            err
        } else if !stderr.trim().is_empty() {
            stderr.trim().to_owned()
        } else {
            "Codex CLI did not return account status metadata.".to_owned()
        };
        return CodexCliProbe::ProbeFailed {
            binary_path,
            message,
        };
    };

    let Some(account) = account_res.account else {
        return CodexCliProbe::NotAuthenticated {
            binary_path,
            config_dir: resolved_config_dir,
        };
    };

    let auth_method = account
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("system")
        .to_owned();

    let (email, plan) = format_codex_account_identity(&account, &auth_method);

    CodexCliProbe::Authenticated {
        binary_path,
        email,
        plan,
        auth_method,
        config_dir: resolved_config_dir,
    }
}

/// Parses NDJSON JSON-RPC responses from `codex app-server --stdio`
/// (`account/rateLimits/read` + `model/list`) into [`CodexCliDetails`].
pub fn parse_codex_details_stream(stdout: &str, stderr: &str) -> CodexCliDetails {
    let mut details = CodexCliDetails::default();
    let mut saw_any_response = false;

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        let Ok(envelope) = serde_json::from_str::<JsonRpcEnvelope>(trimmed) else {
            continue;
        };
        let Some(id) = envelope.id_u64() else {
            continue;
        };
        saw_any_response = true;

        if id == RPC_RATE_LIMITS_ID {
            if let Some(result) = envelope.result
                && let Ok(rate_resp) = serde_json::from_value::<GetAccountRateLimitsResult>(result)
            {
                details.usage = Some(build_codex_usage_summary(rate_resp));
            } else if let Some(err) = envelope.error {
                let lower = err.message.to_ascii_lowercase();
                if lower.contains("not available")
                    || lower.contains("unsupported")
                    || lower.contains("api key")
                {
                    details.usage = Some(ClaudeUsageSummary {
                        rate_limits_available: false,
                        windows: Vec::new(),
                        extra_usage_note: None,
                    });
                } else {
                    details.warning =
                        Some(format!("Could not fetch usage limits: {}", err.message));
                }
            }
        } else if id == RPC_MODELS_ID {
            if let Some(result) = envelope.result
                && let Ok(list_resp) = serde_json::from_value::<ModelListResult>(result)
            {
                details.models = normalize_codex_models(list_resp.data);
            } else if let Some(err) = envelope.error
                && details.warning.is_none()
            {
                details.warning = Some(err.message);
            }
        }
    }

    if !saw_any_response && details.warning.is_none() {
        let err_detail = stderr.trim();
        if !err_detail.is_empty() {
            details.warning = Some(err_detail.to_owned());
        } else {
            details.warning = Some(
                "Codex CLI did not return model or usage metadata (an older CLI version may be installed)."
                    .to_owned(),
            );
        }
    }

    details
}

#[derive(Debug, Deserialize)]
struct JsonRpcEnvelope {
    id: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

impl JsonRpcEnvelope {
    fn id_u64(&self) -> Option<u64> {
        match self.id.as_ref()? {
            serde_json::Value::Number(num) => num.as_u64(),
            serde_json::Value::String(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeResult {
    #[serde(default)]
    codex_home: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetAccountResult {
    #[serde(default)]
    account: Option<RawCodexAccount>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCodexAccount {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    plan_type: Option<String>,
    #[serde(default)]
    workspace_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelListResult {
    #[serde(default)]
    data: Vec<RawCodexModel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCodexModel {
    #[serde(default)]
    id: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    hidden: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetAccountRateLimitsResult {
    #[serde(default)]
    ordinary_usage_allowed: Option<bool>,
    #[serde(default)]
    rate_limits: Option<RawCodexRateLimitSnapshot>,
    #[serde(default)]
    rate_limits_by_limit_id: Option<BTreeMap<String, RawCodexRateLimitSnapshot>>,
    #[serde(default)]
    rate_limit_reset_credits: Option<RawCodexResetCreditsSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCodexRateLimitSnapshot {
    #[serde(default)]
    limit_id: Option<String>,
    #[serde(default)]
    limit_name: Option<String>,
    #[serde(default)]
    normal_model_slug: Option<String>,
    #[serde(default)]
    primary: Option<RawCodexRateLimitWindow>,
    #[serde(default)]
    secondary: Option<RawCodexRateLimitWindow>,
    #[serde(default)]
    credits: Option<RawCodexCreditsSnapshot>,
    #[serde(default)]
    individual_limit: Option<RawCodexSpendControlLimit>,
    #[serde(default)]
    spend_control_reached: Option<bool>,
    #[serde(default)]
    rate_limit_reached_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCodexRateLimitWindow {
    #[serde(default)]
    used_percent: f64,
    #[serde(default)]
    window_duration_mins: Option<i64>,
    #[serde(default)]
    resets_at: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCodexCreditsSnapshot {
    #[serde(default)]
    has_credits: bool,
    #[serde(default)]
    unlimited: bool,
    #[serde(default)]
    balance: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCodexSpendControlLimit {
    #[serde(default)]
    limit: String,
    #[serde(default)]
    used: String,
    #[serde(default)]
    remaining_percent: f64,
    #[serde(default)]
    resets_at: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCodexResetCreditsSummary {
    #[serde(default)]
    available_count: i64,
}

fn format_codex_account_identity(account: &RawCodexAccount, auth_method: &str) -> (String, String) {
    match auth_method.to_ascii_lowercase().as_str() {
        "apikey" | "api_key" => ("API key".to_owned(), "API billing".to_owned()),
        "amazonbedrock" | "bedrock" => ("AWS Bedrock".to_owned(), "AWS Bedrock".to_owned()),
        "gcp" => ("Google Vertex".to_owned(), "Google Vertex".to_owned()),
        _ => {
            let email = account
                .email
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    account
                        .workspace_name
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "ChatGPT account".to_owned());

            let plan = account
                .plan_type
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("unknown"))
                .map(humanize_codex_plan)
                .unwrap_or_default();

            (email, plan)
        }
    }
}

fn humanize_codex_plan(raw: &str) -> String {
    match raw.to_ascii_lowercase().as_str() {
        "free" => "Free".to_owned(),
        "go" => "Go".to_owned(),
        "plus" => "Plus".to_owned(),
        "pro" => "Pro".to_owned(),
        "prolite" => "Pro Lite".to_owned(),
        "promax" => "Pro Max".to_owned(),
        "team" => "Team".to_owned(),
        "business" | "self_serve_business_usage_based" => "Business".to_owned(),
        "self_serve_business_prolite" => "Business Pro Lite".to_owned(),
        "enterprise" | "ent26" | "enterprise_cbp_automation" | "enterprise_cbp_usage_based" => {
            "Enterprise".to_owned()
        }
        "edu" => "Edu".to_owned(),
        "edu_plus" => "Edu Plus".to_owned(),
        "edu_pro" => "Edu Pro".to_owned(),
        "sci" => "Sci".to_owned(),
        "sci_plus" => "Sci Plus".to_owned(),
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

fn normalize_codex_models(raw_models: Vec<RawCodexModel>) -> Vec<ClaudeModelOption> {
    let mut out = Vec::new();
    let mut seen_values = Vec::new();
    for raw in raw_models {
        if raw.hidden {
            continue;
        }
        let model = raw.model.trim();
        let id = raw.id.trim();
        if model.is_empty() && id.is_empty() {
            continue;
        }
        let value = if !model.is_empty() {
            model.to_owned()
        } else {
            id.to_owned()
        };
        if seen_values.contains(&value) {
            continue;
        }
        seen_values.push(value.clone());

        let resolved_model = if !id.is_empty() {
            id.to_owned()
        } else {
            value.clone()
        };
        let display_name = if raw.display_name.trim().is_empty() {
            value.clone()
        } else {
            raw.display_name.trim().to_owned()
        };
        out.push(ClaudeModelOption {
            value,
            resolved_model,
            display_name,
            description: raw.description.trim().to_owned(),
        });
    }
    out
}

fn build_codex_usage_summary(resp: GetAccountRateLimitsResult) -> ClaudeUsageSummary {
    let mut ordered_snapshots: Vec<(Option<String>, &RawCodexRateLimitSnapshot)> = Vec::new();

    if let Some(by_id) = &resp.rate_limits_by_limit_id
        && !by_id.is_empty()
    {
        if let Some(primary_codex) = by_id.get("codex") {
            ordered_snapshots.push((Some("codex".to_owned()), primary_codex));
        }
        for (key, snapshot) in by_id {
            if key == "codex" {
                continue;
            }
            ordered_snapshots.push((Some(key.clone()), snapshot));
        }
    } else if let Some(snapshot) = &resp.rate_limits {
        ordered_snapshots.push((snapshot.limit_id.clone(), snapshot));
    }

    let mut windows = Vec::new();
    let mut notes = Vec::new();

    for (map_key, snapshot) in &ordered_snapshots {
        let scope_label = snapshot_scope_label(map_key.as_deref(), snapshot);

        if let Some(primary) = &snapshot.primary {
            let used = clamp_percent(primary.used_percent);
            windows.push(ClaudeUsageWindow {
                label: format_codex_window_label(
                    primary.window_duration_mins,
                    scope_label.as_deref(),
                    true,
                ),
                used_percent: used,
                remaining_percent: 100u8.saturating_sub(used),
                resets_at: primary
                    .resets_at
                    .as_ref()
                    .and_then(parse_codex_reset_timestamp),
                severity: codex_window_severity(used, snapshot, resp.ordinary_usage_allowed)
                    .to_owned(),
            });
        }

        if let Some(secondary) = &snapshot.secondary {
            let used = clamp_percent(secondary.used_percent);
            windows.push(ClaudeUsageWindow {
                label: format_codex_window_label(
                    secondary.window_duration_mins,
                    scope_label.as_deref(),
                    false,
                ),
                used_percent: used,
                remaining_percent: 100u8.saturating_sub(used),
                resets_at: secondary
                    .resets_at
                    .as_ref()
                    .and_then(parse_codex_reset_timestamp),
                severity: codex_window_severity(used, snapshot, resp.ordinary_usage_allowed)
                    .to_owned(),
            });
        }

        if let Some(ind) = &snapshot.individual_limit {
            let remaining = clamp_percent(ind.remaining_percent);
            let used = 100u8.saturating_sub(remaining);
            let base = if !ind.used.trim().is_empty() && !ind.limit.trim().is_empty() {
                format!("Spend limit ({} / {})", ind.used.trim(), ind.limit.trim())
            } else {
                "Spend limit".to_owned()
            };
            let label = match scope_label.as_deref() {
                Some(scope) => format!("{base} · {scope}"),
                None => base,
            };
            windows.push(ClaudeUsageWindow {
                label,
                used_percent: used,
                remaining_percent: remaining,
                resets_at: ind.resets_at.as_ref().and_then(parse_codex_reset_timestamp),
                severity: if snapshot.spend_control_reached == Some(true) {
                    "critical".to_owned()
                } else {
                    default_severity_for_used(used).to_owned()
                },
            });
        }
    }

    if let Some((_, first_snapshot)) = ordered_snapshots.first()
        && let Some(credits) = &first_snapshot.credits
    {
        if credits.unlimited {
            notes.push("Unlimited credits enabled.".to_owned());
        } else if credits.has_credits
            && let Some(balance) = credits
                .balance
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != "0" && *s != "0.0" && *s != "0.00")
        {
            notes.push(format!("Credits balance: {balance}"));
        }
    }

    if let Some(reset_credits) = &resp.rate_limit_reset_credits {
        if reset_credits.available_count == 1 {
            notes.push("1 rate limit reset credit available.".to_owned());
        } else if reset_credits.available_count > 1 {
            notes.push(format!(
                "{} rate limit reset credits available.",
                reset_credits.available_count
            ));
        }
    }

    let extra_usage_note = if notes.is_empty() {
        None
    } else {
        Some(notes.join(" · "))
    };

    ClaudeUsageSummary {
        rate_limits_available: !windows.is_empty(),
        windows,
        extra_usage_note,
    }
}

fn snapshot_scope_label(
    map_key: Option<&str>,
    snapshot: &RawCodexRateLimitSnapshot,
) -> Option<String> {
    if let Some(name) = snapshot
        .limit_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("codex"))
    {
        return Some(name.to_owned());
    }
    let effective_id = snapshot
        .limit_id
        .as_deref()
        .or(map_key)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if effective_id.is_none_or(|id| id.eq_ignore_ascii_case("codex")) {
        return None;
    }
    if let Some(slug) = snapshot
        .normal_model_slug
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(slug.to_owned());
    }
    effective_id.map(humanize_codex_plan)
}

fn format_codex_window_label(
    duration_mins: Option<i64>,
    scope: Option<&str>,
    is_primary: bool,
) -> String {
    match (duration_mins, scope) {
        (Some(300), None) => "Current session (5h)".to_owned(),
        (Some(300), Some(s)) => format!("5h limit ({s})"),
        (Some(1440), None) => "Daily (24h)".to_owned(),
        (Some(1440), Some(s)) => format!("Daily ({s})"),
        (Some(10080), None) => "Weekly (all models)".to_owned(),
        (Some(10080), Some(s)) => format!("Weekly ({s})"),
        (Some(mins), scope_opt) if mins > 0 => {
            let base = if mins % 1440 == 0 {
                format!("{}d limit", mins / 1440)
            } else if mins % 60 == 0 {
                format!("{}h limit", mins / 60)
            } else {
                format!("{mins}m limit")
            };
            match scope_opt {
                Some(s) => format!("{base} ({s})"),
                None => base,
            }
        }
        (_, Some(s)) => {
            if is_primary {
                format!("Primary limit ({s})")
            } else {
                format!("Secondary limit ({s})")
            }
        }
        (_, None) => {
            if is_primary {
                "Primary limit".to_owned()
            } else {
                "Secondary limit".to_owned()
            }
        }
    }
}

fn codex_window_severity(
    used: u8,
    snapshot: &RawCodexRateLimitSnapshot,
    ordinary_usage_allowed: Option<bool>,
) -> &'static str {
    if used >= 100
        && (ordinary_usage_allowed == Some(false)
            || snapshot.spend_control_reached == Some(true)
            || snapshot.rate_limit_reached_type.is_some())
    {
        return "critical";
    }
    default_severity_for_used(used)
}

fn parse_codex_reset_timestamp(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Number(num) => num.as_i64().and_then(unix_to_rfc3339),
        serde_json::Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Ok(secs) = trimmed.parse::<i64>() {
                return unix_to_rfc3339(secs);
            }
            DateTime::parse_from_rfc3339(trimmed)
                .ok()
                .map(|_| trimmed.to_owned())
        }
        _ => None,
    }
}

fn unix_to_rfc3339(raw: i64) -> Option<String> {
    // Support both Unix seconds and Unix milliseconds cleanly.
    let secs = if raw.abs() > 1_000_000_000_000 {
        raw / 1000
    } else {
        raw
    };
    DateTime::from_timestamp(secs, 0).map(|dt| dt.to_rfc3339())
}

fn validate_executable_file(path: PathBuf) -> Result<PathBuf, CodexBinaryError> {
    let metadata = match fs::metadata(&path) {
        Ok(meta) => meta,
        Err(_) => return Err(CodexBinaryError::CustomPathNotFound(path)),
    };
    if !metadata.is_file() {
        return Err(CodexBinaryError::NotAFile(path));
    }
    if !metadata_is_executable(&metadata) {
        return Err(CodexBinaryError::NotExecutable(path));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use tempfile::TempDir;

    use super::*;

    #[cfg(unix)]
    fn write_fake_codex_script(dir: &Path, name: &str, body: &str, executable: bool) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        let mode = if executable { 0o755 } else { 0o644 };
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn parses_authenticated_chatgpt_account_stream() {
        let stdout = r#"{"id":1,"result":{"userAgent":"bruk/0.171.0","codexHome":"/Users/dev/.codex"}}
{"id":2,"result":{"account":{"type":"chatgpt","email":"dev@example.com","planType":"prolite","workspaceName":null},"requiresOpenaiAuth":true}}"#;
        let probe = parse_codex_account_stream(PathBuf::from("/usr/local/bin/codex"), stdout, "");
        assert_eq!(
            probe,
            CodexCliProbe::Authenticated {
                binary_path: PathBuf::from("/usr/local/bin/codex"),
                email: "dev@example.com".to_owned(),
                plan: "Pro Lite".to_owned(),
                auth_method: "chatgpt".to_owned(),
                config_dir: "/Users/dev/.codex".to_owned(),
            }
        );
    }

    #[test]
    fn parses_authenticated_api_key_account_stream() {
        let stdout = r#"{"id":1,"result":{"codexHome":"/Users/dev/.codex"}}
{"id":2,"result":{"account":{"type":"apiKey"},"requiresOpenaiAuth":true}}"#;
        let probe = parse_codex_account_stream(PathBuf::from("/opt/bin/codex"), stdout, "");
        assert_eq!(
            probe,
            CodexCliProbe::Authenticated {
                binary_path: PathBuf::from("/opt/bin/codex"),
                email: "API key".to_owned(),
                plan: "API billing".to_owned(),
                auth_method: "apiKey".to_owned(),
                config_dir: "/Users/dev/.codex".to_owned(),
            }
        );
    }

    #[test]
    fn parses_logged_out_account_stream() {
        let stdout = r#"{"id":1,"result":{"codexHome":"/Users/dev/.codex"}}
{"id":2,"result":{"account":null,"requiresOpenaiAuth":true}}"#;
        let probe = parse_codex_account_stream(PathBuf::from("/usr/local/bin/codex"), stdout, "");
        assert_eq!(
            probe,
            CodexCliProbe::NotAuthenticated {
                binary_path: PathBuf::from("/usr/local/bin/codex"),
                config_dir: "/Users/dev/.codex".to_owned(),
            }
        );
    }

    #[test]
    fn parses_codex_models_and_rate_limits_with_unix_resets() {
        let stdout = r#"{"id":1,"result":{"codexHome":"/Users/dev/.codex"}}
{"id":2,"result":{"ordinaryUsageAllowed":true,"rateLimits":{"limitId":"codex","limitName":null,"primary":{"usedPercent":14,"windowDurationMins":300,"resetsAt":1795384800},"secondary":{"usedPercent":25,"windowDurationMins":10080,"resetsAt":1795914242},"credits":{"hasCredits":false,"unlimited":false,"balance":"0"}},"rateLimitsByLimitId":{"codex":{"limitId":"codex","limitName":null,"primary":{"usedPercent":14,"windowDurationMins":300,"resetsAt":1795384800},"secondary":{"usedPercent":25,"windowDurationMins":10080,"resetsAt":1795914242},"credits":{"hasCredits":false,"unlimited":false,"balance":"0"}},"codex_spark":{"limitId":"codex_spark","limitName":"GPT-5.3-Codex-Spark","primary":{"usedPercent":5,"windowDurationMins":10080,"resetsAt":1795914242}}},"rateLimitResetCredits":{"availableCount":2}}}
{"id":3,"result":{"data":[{"id":"gpt-6.2-sol","model":"gpt-6.2-sol","displayName":"GPT-6.2-Sol","description":"Fast go-to model","hidden":false},{"id":"gpt-6.2-astra","model":"gpt-6.2-astra","displayName":"GPT-6.2-Astra","description":"Most capable model","hidden":false},{"id":"gpt-6.1-terra","model":"gpt-6.1-terra","displayName":"GPT-6.1 Terra","description":"Visual model","hidden":false},{"id":"gpt-6-luna","model":"gpt-6-luna","displayName":"GPT-6-Luna","description":"Fast and affordable","hidden":false},{"id":"hidden-internal","model":"hidden-internal","displayName":"Hidden","description":"Should be skipped","hidden":true}]}}"#;

        let details = parse_codex_details_stream(stdout, "");
        assert_eq!(details.warning, None);
        assert_eq!(details.models.len(), 4);
        assert_eq!(details.models[0].value, "gpt-6.2-sol");
        assert_eq!(details.models[0].family(), "Sol");
        assert_eq!(details.models[1].family(), "Astra");
        assert_eq!(details.models[2].family(), "Terra");
        assert_eq!(details.models[3].family(), "Luna");

        let usage = details.usage.expect("usage summary parsed");
        assert!(usage.rate_limits_available);
        assert_eq!(usage.windows.len(), 3);
        assert_eq!(usage.windows[0].label, "Current session (5h)");
        assert_eq!(usage.windows[0].used_percent, 14);
        assert_eq!(usage.windows[0].remaining_percent, 86);
        assert!(usage.windows[0].resets_at.is_some());
        assert_eq!(usage.windows[1].label, "Weekly (all models)");
        assert_eq!(usage.windows[1].remaining_percent, 75);
        assert_eq!(usage.windows[2].label, "Weekly (GPT-5.3-Codex-Spark)");
        assert_eq!(usage.windows[2].remaining_percent, 95);
        assert_eq!(
            usage.extra_usage_note.as_deref(),
            Some("2 rate limit reset credits available.")
        );

        let resets = usage.upcoming_resets();
        assert_eq!(resets.len(), 3);
        assert_eq!(resets[0].label, "Current session (5h)");
    }

    #[cfg(unix)]
    #[test]
    fn resolves_and_probes_codex_binary_from_path_and_custom_path() {
        let temp = TempDir::new().unwrap();
        let path_dir = temp.path().join("path-bin");
        let custom_dir = temp.path().join("custom-bin");
        let empty_dir = temp.path().join("empty-bin");
        fs::create_dir_all(&path_dir).unwrap();
        fs::create_dir_all(&custom_dir).unwrap();
        fs::create_dir_all(&empty_dir).unwrap();

        let path_codex = write_fake_codex_script(
            &path_dir,
            "codex",
            r#"#!/bin/sh
echo '{"id":1,"result":{"codexHome":"/tmp/.codex-path"}}'
echo '{"id":2,"result":{"account":{"type":"chatgpt","email":"codex@example.com","planType":"pro"},"requiresOpenaiAuth":true}}'
"#,
            true,
        );
        let custom_codex = write_fake_codex_script(
            &custom_dir,
            "codex-custom",
            r#"#!/bin/sh
echo '{"id":1,"result":{"codexHome":"/tmp/.codex-custom"}}'
echo '{"id":2,"result":{"account":null,"requiresOpenaiAuth":true}}'
"#,
            true,
        );
        let non_exec = write_fake_codex_script(&custom_dir, "codex-noexec", "#!/bin/sh\n", false);

        let probe = probe_codex_cli(None, Some(path_dir.as_os_str()));
        assert_eq!(
            probe,
            CodexCliProbe::Authenticated {
                binary_path: path_codex,
                email: "codex@example.com".to_owned(),
                plan: "Pro".to_owned(),
                auth_method: "chatgpt".to_owned(),
                config_dir: "/tmp/.codex-path".to_owned(),
            }
        );

        let missing = probe_codex_cli(None, Some(empty_dir.as_os_str()));
        assert_eq!(
            missing,
            CodexCliProbe::BinaryNotFound {
                error: CodexBinaryError::NotFoundOnPath,
            }
        );

        let custom_str = custom_codex.to_string_lossy().into_owned();
        let custom_probe = probe_codex_cli(Some(&custom_str), Some(OsString::new().as_os_str()));
        assert_eq!(
            custom_probe,
            CodexCliProbe::NotAuthenticated {
                binary_path: custom_codex,
                config_dir: "/tmp/.codex-custom".to_owned(),
            }
        );

        assert!(matches!(
            resolve_codex_binary(Some(custom_dir.to_str().unwrap()), None),
            Err(CodexBinaryError::NotAFile(_))
        ));
        assert!(matches!(
            resolve_codex_binary(Some(non_exec.to_str().unwrap()), None),
            Err(CodexBinaryError::NotExecutable(_))
        ));
        assert!(matches!(
            resolve_codex_binary(Some("/nonexistent/path/to/codex"), None),
            Err(CodexBinaryError::CustomPathNotFound(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn all_codex_cli_invocations_disable_startup_update_check() {
        let temp = TempDir::new().unwrap();
        let log_file = temp.path().join("args.log");
        let script = format!(
            r#"#!/bin/sh
echo "ARGS=$*" >> "{log}"
if [ "$1" = "login" ]; then
  exit 0
fi
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      echo '{{"id":1,"result":{{"codexHome":"/tmp/.codex"}}}}'
      ;;
    *'"method":"account/read"'*)
      echo '{{"id":2,"result":{{"account":{{"type":"chatgpt","email":"env@example.com","planType":"pro"}},"requiresOpenaiAuth":true}}}}'
      ;;
    *'"method":"account/rateLimits/read"'*)
      echo '{{"id":2,"result":{{"ordinaryUsageAllowed":true,"rateLimits":null}}}}'
      ;;
    *'"method":"model/list"'*)
      echo '{{"id":3,"result":{{"data":[]}}}}'
      ;;
  esac
done
"#,
            log = log_file.display()
        );
        let codex_bin = write_fake_codex_script(temp.path(), "codex", &script, true);

        let probe = probe_codex_cli(Some(codex_bin.to_str().unwrap()), None);
        assert!(matches!(probe, CodexCliProbe::Authenticated { .. }));
        run_codex_login(&codex_bin).unwrap();
        let details = fetch_codex_details(&codex_bin);
        assert_eq!(details.warning, None);

        let logged = fs::read_to_string(&log_file).unwrap();
        let lines: Vec<&str> = logged.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(
            lines[0],
            "ARGS=app-server --stdio -c check_for_update_on_startup=false"
        );
        assert_eq!(lines[1], "ARGS=login -c check_for_update_on_startup=false");
        assert_eq!(
            lines[2],
            "ARGS=app-server --stdio -c check_for_update_on_startup=false"
        );
    }
}
