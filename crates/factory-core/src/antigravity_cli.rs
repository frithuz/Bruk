//! Discovery, sign-in probing, model catalog, and context reads for the local Antigravity CLI
//! (`agy`).
//!
//! Antigravity runs with the user's existing system login (kept in the macOS Keychain, with
//! its data under `~/.gemini`). Bruk locates the `agy` executable from a user-configured custom
//! path or the default `PATH`, then:
//!
//! - `agy models` proves the login and lists the models it may use (no inference, and unlike an
//!   ACP `session/new` it leaves no conversation behind);
//! - `agy acp` (Agent Client Protocol over stdio) `session/load` reads how full a saved
//!   conversation's context window is, again without calling a model.
//!
//! Every `agy` Bruk spawns runs with `AGY_CLI_DISABLE_AUTO_UPDATE=true`, so a run never stops to
//! update the CLI.

use std::{
    collections::BTreeMap,
    env,
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;
use thiserror::Error;

use crate::claude_cli::{
    ClaudeModelOption, ClaudeUsageSummary, ClaudeUsageWindow, clamp_percent,
    default_severity_for_used, expand_tilde, is_executable_file, metadata_is_executable,
};

/// Default executable filename searched on `PATH`.
#[cfg(windows)]
pub const ANTIGRAVITY_BINARY_NAME: &str = "agy.exe";
/// Default executable filename searched on `PATH`.
#[cfg(not(windows))]
pub const ANTIGRAVITY_BINARY_NAME: &str = "agy";

/// Environment variable that keeps the Antigravity CLI from updating itself mid-run.
pub const ANTIGRAVITY_DISABLE_AUTO_UPDATE_ENV: &str = "AGY_CLI_DISABLE_AUTO_UPDATE";
/// Value Bruk always sets for [`ANTIGRAVITY_DISABLE_AUTO_UPDATE_ENV`].
pub const ANTIGRAVITY_DISABLE_AUTO_UPDATE_VALUE: &str = "true";

/// Where Antigravity keeps its data for the signed-in user.
pub const ANTIGRAVITY_DEFAULT_CONFIG_DIR: &str = "~/.gemini";

/// Model Bruk proposes when the account is added and the CLI hasn't listed its models yet.
pub const ANTIGRAVITY_DEFAULT_MODEL: &str = "gemini-4-neon-medium";

const MODELS_TIMEOUT: Duration = Duration::from_secs(20);
const ACP_CONTEXT_TIMEOUT: Duration = Duration::from_secs(20);
const RPC_INIT_ID: u64 = 1;
const RPC_LOAD_ID: u64 = 2;
/// How long to keep reading after `session/load` answers, for the trailing `usage_update`.
const USAGE_GRACE: Duration = Duration::from_millis(1500);

/// Errors returned when locating the `agy` executable.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AntigravityBinaryError {
    #[error("Antigravity CLI ('agy') was not found on the default PATH.")]
    NotFoundOnPath,
    #[error("Antigravity CLI was not found at '{}'.", .0.display())]
    CustomPathNotFound(PathBuf),
    #[error("'{}' is a directory, not an Antigravity CLI executable.", .0.display())]
    NotAFile(PathBuf),
    #[error("'{}' is not executable.", .0.display())]
    NotExecutable(PathBuf),
}

impl AntigravityBinaryError {
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

/// Result of probing the system's `agy` binary and login state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AntigravityCliProbe {
    /// Binary exists and `agy models` answered, which needs an active login.
    Authenticated {
        binary_path: PathBuf,
        /// The models `agy models` listed (the probe doubles as the catalog query).
        models: Vec<ClaudeModelOption>,
        config_dir: String,
    },
    /// Binary exists, but `agy models` asked the user to sign in.
    NotAuthenticated {
        binary_path: PathBuf,
        config_dir: String,
    },
    /// No executable `agy` binary was found on `PATH` or at the configured custom path.
    BinaryNotFound { error: AntigravityBinaryError },
    /// Binary was found, but `agy models` failed for another reason.
    ProbeFailed {
        binary_path: PathBuf,
        message: String,
    },
}

/// Live model catalog and plan usage queried from the Antigravity CLI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AntigravityCliDetails {
    pub models: Vec<ClaudeModelOption>,
    /// Remaining 5-hour and weekly quota per model group (`agy --print=/usage`).
    pub usage: Option<ClaudeUsageSummary>,
    pub warning: Option<String>,
}

/// How full a saved Antigravity conversation's context window is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AntigravityContextReading {
    pub used_tokens: u64,
    pub window_tokens: u64,
}

/// A `Command` for `agy` with the variables Bruk always sets.
pub fn new_antigravity_command(binary_path: impl AsRef<Path>) -> Command {
    let mut cmd = Command::new(binary_path.as_ref());
    apply_mandatory_antigravity_spawn_env(&mut cmd);
    cmd
}

/// Sets `AGY_CLI_DISABLE_AUTO_UPDATE=true` on a command that runs `agy`.
pub fn apply_mandatory_antigravity_spawn_env(cmd: &mut Command) {
    cmd.env(
        ANTIGRAVITY_DISABLE_AUTO_UPDATE_ENV,
        ANTIGRAVITY_DISABLE_AUTO_UPDATE_VALUE,
    );
}

/// Resolves the `agy` binary using an optional custom path override or the given `PATH`.
pub fn resolve_antigravity_binary(
    custom_path: Option<&str>,
    path_env: Option<&OsStr>,
) -> Result<PathBuf, AntigravityBinaryError> {
    if let Some(raw) = custom_path {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return validate_executable_file(expand_tilde(trimmed));
        }
    }
    if let Some(paths) = path_env {
        for dir in env::split_paths(paths) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let candidate = dir.join(ANTIGRAVITY_BINARY_NAME);
            if is_executable_file(&candidate) {
                return Ok(candidate);
            }
        }
    }
    Err(AntigravityBinaryError::NotFoundOnPath)
}

/// Resolves the `agy` binary using the current process's `PATH`.
pub fn resolve_antigravity_binary_from_env(
    custom_path: Option<&str>,
) -> Result<PathBuf, AntigravityBinaryError> {
    let path_env = env::var_os("PATH");
    resolve_antigravity_binary(custom_path, path_env.as_deref())
}

/// Probes the `agy` binary and checks whether the system already has an active login.
pub fn probe_antigravity_cli(
    custom_path: Option<&str>,
    path_env: Option<&OsStr>,
) -> AntigravityCliProbe {
    let binary_path = match resolve_antigravity_binary(custom_path, path_env) {
        Ok(path) => path,
        Err(error) => return AntigravityCliProbe::BinaryNotFound { error },
    };
    match run_models(&binary_path) {
        Ok((success, stdout, stderr)) => {
            parse_antigravity_models_output(binary_path, success, &stdout, &stderr)
        }
        Err(message) => AntigravityCliProbe::ProbeFailed {
            binary_path,
            message,
        },
    }
}

/// Probes `agy` using the current process's `PATH`.
pub fn probe_antigravity_cli_from_env(custom_path: Option<&str>) -> AntigravityCliProbe {
    let path_env = env::var_os("PATH");
    probe_antigravity_cli(custom_path, path_env.as_deref())
}

/// Lists the models the signed-in Antigravity account may use.
///
/// Also reads the remaining plan quota with `agy --print=/usage`, a slash command the CLI
/// answers itself: no agent turn starts and no quota is spent.
pub fn fetch_antigravity_details(binary_path: &Path) -> AntigravityCliDetails {
    let mut details = AntigravityCliDetails::default();
    match run_agy(binary_path, &["--output-format", "json", "--print=/usage"]) {
        Ok((_, stdout, stderr)) => match parse_antigravity_usage_output(&stdout) {
            Some(usage) => details.usage = Some(usage),
            None => {
                let message = strip_terminal_noise(&stderr);
                details.warning = Some(if message.trim().is_empty() {
                    "Antigravity CLI didn't report usage limits.".to_owned()
                } else {
                    format!("Could not fetch usage limits: {}", message.trim())
                });
            }
        },
        Err(message) => details.warning = Some(message),
    }
    match run_models(binary_path) {
        Ok((success, stdout, stderr)) => {
            match parse_antigravity_models_output(
                binary_path.to_path_buf(),
                success,
                &stdout,
                &stderr,
            ) {
                AntigravityCliProbe::Authenticated { models, .. } => details.models = models,
                AntigravityCliProbe::NotAuthenticated { .. } => {
                    details.warning = Some("Sign in to the Antigravity CLI first.".to_owned());
                }
                AntigravityCliProbe::ProbeFailed { message, .. } => details.warning = Some(message),
                AntigravityCliProbe::BinaryNotFound { error } => {
                    details.warning = Some(error.to_string());
                }
            }
        }
        Err(message) => details.warning = Some(message),
    }
    details
}

/// Reads the JSON `agy --output-format json --print=/usage` prints: per model group, a 5-hour
/// and a weekly bucket with the fraction left and when it resets.
pub fn parse_antigravity_usage_output(stdout: &str) -> Option<ClaudeUsageSummary> {
    let val: Value = serde_json::from_str::<Value>(stdout.trim())
        .ok()
        .or_else(|| {
            stdout
                .lines()
                .rev()
                .find_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        })?;
    let data = val.get("command")?.get("data")?;
    let mut windows = Vec::new();
    for group in data.get("groups")?.as_array()? {
        let group_name = group
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Models");
        for bucket in group
            .get("buckets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(remaining) = bucket.get("remaining_fraction").and_then(Value::as_f64) else {
                continue;
            };
            let remaining_percent = clamp_percent(remaining * 100.0);
            let used_percent = 100u8.saturating_sub(remaining_percent);
            let window = match bucket.get("window").and_then(Value::as_str) {
                Some("5h") => "5-hour".to_owned(),
                Some("weekly") => "Weekly".to_owned(),
                _ => bucket
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Limit")
                    .trim_end_matches(" Remaining")
                    .to_owned(),
            };
            windows.push(ClaudeUsageWindow {
                label: format!("{window} ({group_name})"),
                used_percent,
                remaining_percent,
                resets_at: bucket
                    .get("reset_time")
                    .and_then(Value::as_str)
                    .filter(|t| !t.trim().is_empty())
                    .map(str::to_owned),
                severity: default_severity_for_used(used_percent).to_owned(),
            });
        }
    }
    Some(ClaudeUsageSummary {
        rate_limits_available: !windows.is_empty(),
        windows,
        extra_usage_note: None,
    })
}

/// Signs the user in to Antigravity (`agy acp --login`, which opens Google sign-in in the
/// browser and exits when it finishes).
pub fn run_antigravity_login(binary_path: &Path) -> Result<(), String> {
    let output = new_antigravity_command(binary_path)
        .args(["acp", "--login"])
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("Failed to start Antigravity sign-in: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = strip_terminal_noise(&String::from_utf8_lossy(&output.stderr));
    let stdout = strip_terminal_noise(&String::from_utf8_lossy(&output.stdout));
    Err(first_non_empty(&[&stderr, &stdout])
        .unwrap_or_else(|| format!("Antigravity sign-in exited with status {}.", output.status)))
}

/// Reads `agy models` output: one `<id>\t<display name>` line per model.
pub fn parse_antigravity_models_output(
    binary_path: PathBuf,
    success: bool,
    stdout: &str,
    stderr: &str,
) -> AntigravityCliProbe {
    let config_dir = ANTIGRAVITY_DEFAULT_CONFIG_DIR.to_owned();
    let stderr_clean = strip_terminal_noise(stderr);
    let stdout_clean = strip_terminal_noise(stdout);
    let combined = format!("{stderr_clean}\n{stdout_clean}").to_ascii_lowercase();
    if combined.contains("sign in")
        || combined.contains("not logged in")
        || combined.contains("not authenticated")
        || combined.contains("authentication required")
    {
        return AntigravityCliProbe::NotAuthenticated {
            binary_path,
            config_dir,
        };
    }
    let models = parse_model_lines(stdout);
    if success {
        return AntigravityCliProbe::Authenticated {
            binary_path,
            models,
            config_dir,
        };
    }
    let message = first_non_empty(&[&stderr_clean, &stdout_clean])
        .map(|m| m.trim_start_matches("Error:").trim().to_owned())
        .unwrap_or_else(|| "Antigravity CLI couldn't list its models.".to_owned());
    AntigravityCliProbe::ProbeFailed {
        binary_path,
        message,
    }
}

fn parse_model_lines(stdout: &str) -> Vec<ClaudeModelOption> {
    let mut models: Vec<ClaudeModelOption> = Vec::new();
    for line in stdout.lines() {
        let Some((id, name)) = line.split_once('\t') else {
            continue;
        };
        let id = id.trim();
        if id.is_empty() || id.contains(char::is_whitespace) || models.iter().any(|m| m.value == id)
        {
            continue;
        }
        let name = name.trim();
        models.push(ClaudeModelOption {
            value: id.to_owned(),
            resolved_model: id.to_owned(),
            display_name: if name.is_empty() { id } else { name }.to_owned(),
            description: String::new(),
        });
    }
    models
}

/// Reads a saved conversation's context window through `agy acp`: `session/load` replays the
/// conversation and then reports `usage_update { used, size }`. No model is called.
///
/// `extra_args` carry the flags turns run with (so ACP sees the same data folders), and `env`
/// their environment.
pub fn read_antigravity_context(
    binary_path: &Path,
    working_dir: &Path,
    conversation_id: &str,
    extra_args: &[String],
    env: &BTreeMap<String, String>,
) -> Result<AntigravityContextReading, String> {
    let mut cmd = new_antigravity_command(binary_path);
    cmd.arg("acp")
        .args(extra_args)
        .envs(env)
        .current_dir(working_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|err| format!("Couldn't start the Antigravity CLI: {err}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Couldn't talk to the Antigravity CLI.".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Couldn't read the Antigravity CLI.".to_owned())?;
    let stderr_reader = child.stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut raw = String::new();
            let _ = pipe.read_to_string(&mut raw);
            raw
        })
    });
    let (line_tx, line_rx) = mpsc::channel::<String>();
    let stdout_reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line_tx.send(line).is_err() {
                break;
            }
        }
    });

    let requests = [
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": RPC_INIT_ID,
            "method": "initialize",
            "params": { "protocolVersion": 1, "clientCapabilities": {} },
        }),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": RPC_LOAD_ID,
            "method": "session/load",
            "params": {
                "sessionId": conversation_id,
                "cwd": working_dir.to_string_lossy(),
                "mcpServers": [],
            },
        }),
    ];
    for request in requests {
        let _ = writeln!(stdin, "{request}");
    }
    let _ = stdin.flush();

    let deadline = Instant::now() + ACP_CONTEXT_TIMEOUT;
    let mut reading: Option<AntigravityContextReading> = None;
    let mut load_answered_at: Option<Instant> = None;
    let mut error: Option<String> = None;
    loop {
        let now = Instant::now();
        let mut until = deadline;
        if let Some(at) = load_answered_at {
            until = until.min(at + USAGE_GRACE);
        }
        let Some(remaining) = until.checked_duration_since(now) else {
            break;
        };
        match line_rx.recv_timeout(remaining) {
            Ok(line) => match parse_acp_context_line(&line) {
                AcpLine::Usage(found) => {
                    reading = Some(found);
                    if load_answered_at.is_some() {
                        break;
                    }
                }
                AcpLine::LoadAnswered => {
                    load_answered_at = Some(Instant::now());
                    if reading.is_some() {
                        break;
                    }
                }
                AcpLine::LoadFailed(message) => {
                    error = Some(message);
                    break;
                }
                AcpLine::Other => {}
            },
            Err(_) => break,
        }
    }

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let _ = stdout_reader.join();
    let stderr_text = stderr_reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();

    if let Some(reading) = reading {
        return Ok(reading);
    }
    Err(error
        .or_else(|| first_non_empty(&[&strip_terminal_noise(&stderr_text)]))
        .unwrap_or_else(|| {
            "Antigravity hasn't reported a token count for this chat yet.".to_owned()
        }))
}

enum AcpLine {
    Usage(AntigravityContextReading),
    LoadAnswered,
    LoadFailed(String),
    Other,
}

fn parse_acp_context_line(line: &str) -> AcpLine {
    let Ok(val) = serde_json::from_str::<Value>(line.trim()) else {
        return AcpLine::Other;
    };
    if let Some(update) = val.get("params").and_then(|p| p.get("update"))
        && update.get("sessionUpdate").and_then(Value::as_str) == Some("usage_update")
    {
        let used = update.get("used").and_then(Value::as_u64).unwrap_or(0);
        let size = update.get("size").and_then(Value::as_u64).unwrap_or(0);
        if size > 0 {
            return AcpLine::Usage(AntigravityContextReading {
                used_tokens: used,
                window_tokens: size,
            });
        }
        return AcpLine::Other;
    }
    if val.get("id").and_then(Value::as_u64) == Some(RPC_LOAD_ID) {
        if let Some(err) = val.get("error") {
            let message = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Antigravity couldn't open this conversation.")
                .to_owned();
            return AcpLine::LoadFailed(message);
        }
        return AcpLine::LoadAnswered;
    }
    AcpLine::Other
}

/// Runs `agy models` with a time limit: `(exited successfully, stdout, stderr)`.
fn run_models(binary_path: &Path) -> Result<(bool, String, String), String> {
    run_agy(binary_path, &["models"])
}

/// Runs `agy <args>` in the temp folder (so no project is involved) with a time limit.
fn run_agy(binary_path: &Path, args: &[&str]) -> Result<(bool, String, String), String> {
    let mut child = new_antigravity_command(binary_path)
        .args(args)
        .current_dir(env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Failed to run Antigravity CLI: {err}"))?;
    let stdout_reader = child.stdout.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut raw = String::new();
            let _ = pipe.read_to_string(&mut raw);
            raw
        })
    });
    let stderr_reader = child.stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut raw = String::new();
            let _ = pipe.read_to_string(&mut raw);
            raw
        })
    });
    let deadline = Instant::now() + MODELS_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let stdout = stdout_reader
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let stderr = stderr_reader
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    match status {
        Some(status) => Ok((status.success(), stdout, stderr)),
        None => Err("Antigravity CLI didn't answer in time.".to_owned()),
    }
}

/// Drops spinner frames and ANSI escapes the CLI writes to stderr, keeping the last line of
/// each carriage-return-separated redraw.
fn strip_terminal_noise(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // CSI: ESC [ ... final byte in @..~
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out.lines()
        .map(|line| line.rsplit('\r').next().unwrap_or(line))
        .map(|line| {
            line.trim_start_matches(|c: char| ('\u{2800}'..='\u{28ff}').contains(&c) || c == ' ')
        })
        .filter(|line| !line.trim().is_empty() && !line.contains("Fetching available models"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_non_empty(candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .map(|c| c.trim())
        .find(|c| !c.is_empty())
        .map(str::to_owned)
}

fn validate_executable_file(path: PathBuf) -> Result<PathBuf, AntigravityBinaryError> {
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(_) => return Err(AntigravityBinaryError::CustomPathNotFound(path)),
    };
    if metadata.is_dir() {
        return Err(AntigravityBinaryError::NotAFile(path));
    }
    if !metadata_is_executable(&metadata) {
        return Err(AntigravityBinaryError::NotExecutable(path));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use tempfile::TempDir;

    use super::*;

    #[cfg(unix)]
    fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn models_output_proves_login_and_lists_models() {
        let probe = parse_antigravity_models_output(
            PathBuf::from("/opt/homebrew/bin/agy"),
            true,
            "gemini-4-neon-high\tGemini 4 Neon (High)\nclaude-opus-5-5-low\tClaude Opus 5.5 (Low)\n",
            "\u{1b}[K⠋ Fetching available models...\rFetching available models...\n",
        );
        let AntigravityCliProbe::Authenticated { models, .. } = probe else {
            panic!("expected authenticated, got {probe:?}");
        };
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].value, "gemini-4-neon-high");
        assert_eq!(models[0].display_name, "Gemini 4 Neon (High)");
        assert_eq!(models[0].family(), "Neon");
        assert_eq!(models[1].family(), "Opus");
    }

    #[test]
    fn sign_in_prompt_means_not_authenticated() {
        let probe = parse_antigravity_models_output(
            PathBuf::from("/opt/homebrew/bin/agy"),
            false,
            "",
            "⠋ Fetching available models...\u{1b}[KFetching available models...\nError: Please sign in to view available models. Launch the CLI without arguments to sign in.\n",
        );
        assert!(matches!(
            probe,
            AntigravityCliProbe::NotAuthenticated { .. }
        ));
    }

    #[test]
    fn other_failures_keep_the_cli_message() {
        let probe = parse_antigravity_models_output(
            PathBuf::from("/opt/homebrew/bin/agy"),
            false,
            "",
            "Error: model api cannot be reached\n",
        );
        assert_eq!(
            probe,
            AntigravityCliProbe::ProbeFailed {
                binary_path: PathBuf::from("/opt/homebrew/bin/agy"),
                message: "model api cannot be reached".to_owned(),
            }
        );
    }

    #[test]
    fn reads_usage_buckets_from_the_usage_command() {
        let stdout = r#"{"conversation_id":"","status":"SUCCESS","response":"...","command":{"name":"usage","data":{"groups":[{"name":"Gemini Models","buckets":[{"id":"gemini-weekly","name":"Weekly Limit Remaining","window":"weekly","remaining_fraction":0.995,"reset_time":"2026-12-01T12:15:19Z"},{"id":"gemini-5h","name":"Five Hour Limit Remaining","window":"5h","remaining_fraction":0.2,"reset_time":"2026-11-24T17:15:19Z"}]},{"name":"Claude and GPT models","buckets":[{"id":"3p-weekly","name":"Weekly Limit Remaining","window":"weekly","remaining_fraction":1}]}]}}}"#;
        let usage = parse_antigravity_usage_output(stdout).unwrap();
        assert!(usage.rate_limits_available);
        let labels: Vec<_> = usage.windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Weekly (Gemini Models)",
                "5-hour (Gemini Models)",
                "Weekly (Claude and GPT models)"
            ]
        );
        assert_eq!(usage.windows[0].remaining_percent, 100);
        assert_eq!(usage.windows[1].used_percent, 80);
        assert_eq!(usage.windows[1].severity, "warning");
        assert_eq!(
            usage.windows[1].resets_at.as_deref(),
            Some("2026-11-24T17:15:19Z")
        );
        assert!(usage.windows[2].resets_at.is_none());
        assert!(parse_antigravity_usage_output("Error: not logged in").is_none());
    }

    #[test]
    fn reads_acp_usage_updates() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"usage_update","used":9838,"size":1048576}}}"#;
        assert!(matches!(
            parse_acp_context_line(line),
            AcpLine::Usage(AntigravityContextReading {
                used_tokens: 9838,
                window_tokens: 1_048_576
            })
        ));
        assert!(matches!(
            parse_acp_context_line(r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s"}}"#),
            AcpLine::LoadAnswered
        ));
        assert!(matches!(
            parse_acp_context_line(
                r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32603,"message":"conversation not found"}}"#
            ),
            AcpLine::LoadFailed(m) if m == "conversation not found"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn resolves_and_probes_a_fake_agy_on_path() {
        let temp = TempDir::new().unwrap();
        write_script(
            temp.path(),
            ANTIGRAVITY_BINARY_NAME,
            "#!/bin/sh\n[ \"$AGY_CLI_DISABLE_AUTO_UPDATE\" = true ] || exit 3\nprintf 'gemini-3.8-flash-low\\tGemini 3.8 Flash (Low)\\n'\n",
        );
        let path_env = OsString::from(temp.path());
        let probe = probe_antigravity_cli(None, Some(&path_env));
        let AntigravityCliProbe::Authenticated { models, .. } = probe else {
            panic!("expected authenticated, got {probe:?}");
        };
        assert_eq!(models[0].family(), "Flash");

        assert_eq!(
            resolve_antigravity_binary(Some("/nope/agy"), None),
            Err(AntigravityBinaryError::CustomPathNotFound(PathBuf::from(
                "/nope/agy"
            )))
        );
        assert_eq!(
            resolve_antigravity_binary(None, Some(&OsString::from("/nonexistent"))),
            Err(AntigravityBinaryError::NotFoundOnPath)
        );
    }

    #[cfg(unix)]
    #[test]
    fn reads_context_from_a_fake_acp_server() {
        let temp = TempDir::new().unwrap();
        let agy = write_script(
            temp.path(),
            "agy",
            r#"#!/bin/sh
read init
echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}'
read load
echo '{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk"}}}'
echo '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"conv-1"}}'
echo '{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"usage_update","used":1200,"size":1048576}}}'
sleep 5
"#,
        );
        let reading =
            read_antigravity_context(&agy, temp.path(), "conv-1", &[], &BTreeMap::new()).unwrap();
        assert_eq!(reading.used_tokens, 1200);
        assert_eq!(reading.window_tokens, 1_048_576);
    }
}
