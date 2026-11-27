//! External secret managers: built-in 1Password (`op`) and generic Command Managers.
//!
//! Manager definitions live strictly in `AppPreferences` — never in repository files.

use std::{
    collections::BTreeMap,
    io::{Read, Write},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;
use zeroize::Zeroize;

use super::{
    CommandManagerConfig, ManagerId, ONE_PASSWORD_MANAGER_ID, OnePasswordAuthMode,
    OnePasswordConfig, SecretFailure, SecretsSettings, keychain_account_key,
    store::{SecretError, SecretStore},
};

/// Availability status of an external secret manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagerStatus {
    Ready,
    Locked,
    NotInstalled,
    AuthFailed(String),
}

impl ManagerStatus {
    pub fn label(&self) -> String {
        match self {
            Self::Ready => "Ready".to_owned(),
            Self::Locked => "Locked".to_owned(),
            Self::NotInstalled => "Not installed".to_owned(),
            Self::AuthFailed(msg) => {
                if msg.is_empty() {
                    "Authentication failed".to_owned()
                } else {
                    format!("Auth failed: {msg}")
                }
            }
        }
    }
}

/// Cooperative cancellation token that immediately terminates registered child subprocesses.
#[derive(Clone, Default)]
pub struct CancellationToken {
    inner: Arc<CancellationInner>,
}

#[derive(Default)]
struct CancellationInner {
    cancelled: AtomicBool,
    pids: Mutex<Vec<u32>>,
}

impl std::fmt::Debug for CancellationToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        let pids = {
            let mut guard = self.inner.pids.lock().expect("cancel pids mutex poisoned");
            std::mem::take(&mut *guard)
        };
        for pid in pids {
            terminate_pid(pid);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    pub fn register_pid(&self, pid: u32) {
        if self.is_cancelled() {
            terminate_pid(pid);
            return;
        }
        let mut guard = self.inner.pids.lock().expect("cancel pids mutex poisoned");
        if self.is_cancelled() {
            drop(guard);
            terminate_pid(pid);
        } else {
            guard.push(pid);
        }
    }

    pub fn unregister_pid(&self, pid: u32) {
        let mut guard = self.inner.pids.lock().expect("cancel pids mutex poisoned");
        guard.retain(|&p| p != pid);
    }
}

fn terminate_pid(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            .args(["-TERM", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = Command::new("/bin/kill")
            .args(["-KILL", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Trait implemented by external secret managers (1Password and generic Command Managers).
pub trait SecretManager: Send + Sync {
    fn id(&self) -> &ManagerId;
    fn status(&self, cancel: &CancellationToken) -> ManagerStatus;
    fn resolve(
        &self,
        refs: &[&str],
        cancel: &CancellationToken,
    ) -> BTreeMap<String, Result<SecretString, SecretFailure>>;
    fn validate_reference(&self, reference: &str) -> Result<(), String>;
}

// ---------------------------------------------------------------------------------------------
// 1Password (op CLI)
// ---------------------------------------------------------------------------------------------

const OP_SERVICE_ACCOUNT_TIMEOUT: Duration = Duration::from_secs(20);
const OP_DESKTOP_APP_TIMEOUT: Duration = Duration::from_secs(120);

/// Strictly validates a 1Password secret reference (`op://<vault>/<item>[/<section>]/<field>[?attribute=...]`).
///
/// Rejects `{{`, `}}`, `$`, `\r`, `\n`, `\0`, and empty path segments so `op inject` can never
/// expand `$OP_SERVICE_ACCOUNT_TOKEN` or host environment variables inside the template.
pub fn validate_op_reference(reference: &str) -> Result<(), String> {
    if reference.contains('\r') || reference.contains('\n') || reference.contains('\0') {
        return Err("1Password reference cannot contain control characters or newlines".to_owned());
    }
    if reference.contains("{{") || reference.contains("}}") {
        return Err("1Password reference cannot contain template braces '{{' or '}}'".to_owned());
    }
    if reference.contains('$') {
        return Err(
            "1Password reference cannot contain '$' (environment expansion is disallowed)"
                .to_owned(),
        );
    }
    let Some(rest) = reference.strip_prefix("op://") else {
        return Err("1Password reference must start with 'op://'".to_owned());
    };
    let (path, query) = match rest.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (rest, None),
    };
    let segments: Vec<&str> = path.split('/').collect();
    if !(segments.len() == 3 || segments.len() == 4) {
        return Err(
            "1Password reference must have format op://<vault>/<item>[/<section>]/<field>"
                .to_owned(),
        );
    }
    if segments.iter().any(|seg| seg.trim().is_empty()) {
        return Err("1Password reference cannot have empty path segments".to_owned());
    }
    if let Some(q) = query {
        if q.trim().is_empty() {
            return Err("1Password reference query after '?' cannot be empty".to_owned());
        }
        for pair in q.split('&') {
            let Some((k, v)) = pair.split_once('=') else {
                return Err(format!(
                    "invalid query parameter '{pair}' in 1Password reference"
                ));
            };
            if k.trim().is_empty() || v.trim().is_empty() {
                return Err(format!(
                    "invalid query parameter '{pair}' in 1Password reference"
                ));
            }
        }
    }
    Ok(())
}

/// Built-in 1Password secret manager backed by the official `op` CLI.
pub struct OnePasswordManager {
    id: ManagerId,
    config: OnePasswordConfig,
    store: Arc<dyn SecretStore>,
    db_instance_id: String,
}

impl OnePasswordManager {
    pub fn new(
        config: OnePasswordConfig,
        store: Arc<dyn SecretStore>,
        db_instance_id: String,
    ) -> Self {
        Self {
            id: ONE_PASSWORD_MANAGER_ID.to_owned(),
            config,
            store,
            db_instance_id,
        }
    }

    fn binary(&self) -> &str {
        let trimmed = self.config.binary_path.trim();
        if trimmed.is_empty() { "op" } else { trimmed }
    }

    fn timeout(&self) -> Duration {
        match self.config.auth_mode {
            OnePasswordAuthMode::ServiceAccount => OP_SERVICE_ACCOUNT_TIMEOUT,
            OnePasswordAuthMode::DesktopApp => OP_DESKTOP_APP_TIMEOUT,
        }
    }

    fn build_op_command(&self, args: &[&str]) -> Result<Command, SecretFailure> {
        let mut cmd = Command::new(self.binary());
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        match self.config.auth_mode {
            OnePasswordAuthMode::ServiceAccount => {
                // NEVER pass --account in Service Account mode.
                cmd.env_remove("OP_CONNECT_HOST")
                    .env_remove("OP_CONNECT_TOKEN")
                    .env_remove("OP_ACCOUNT");
                for (key, _) in std::env::vars() {
                    if key.starts_with("OP_SESSION_") {
                        cmd.env_remove(key);
                    }
                }
                let secret_id = self
                    .config
                    .service_account_secret_id
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| SecretFailure::ManagerAuthFailed {
                        manager: self.id.clone(),
                        detail: "1Password service-account token is not configured".to_owned(),
                    })?;
                let account_key = keychain_account_key(&self.db_instance_id, secret_id);
                let token = match self.store.get(&account_key) {
                    Ok(Some(tok)) => tok,
                    Ok(None) => {
                        return Err(SecretFailure::ManagerAuthFailed {
                            manager: self.id.clone(),
                            detail: "1Password service-account token is missing from Keychain"
                                .to_owned(),
                        });
                    }
                    Err(SecretError::Locked) => {
                        return Err(SecretFailure::ManagerLocked {
                            manager: self.id.clone(),
                            detail: "OS Keychain is locked".to_owned(),
                        });
                    }
                    Err(err) => {
                        return Err(SecretFailure::StoreUnavailable {
                            detail: err.to_string(),
                        });
                    }
                };
                cmd.env("OP_SERVICE_ACCOUNT_TOKEN", token.expose_secret());
                cmd.args(args);
            }
            OnePasswordAuthMode::DesktopApp => {
                cmd.env_remove("OP_SERVICE_ACCOUNT_TOKEN")
                    .env_remove("OP_CONNECT_HOST")
                    .env_remove("OP_CONNECT_TOKEN");
                let account = self.config.desktop_account.trim();
                if !account.is_empty() {
                    cmd.args(["--account", account]);
                }
                cmd.args(args);
            }
        }
        Ok(cmd)
    }

    /// Tests a single `op://` reference via `op read`, verifying it resolves to a non-empty value
    /// and immediately zeroizing the buffer without returning the secret.
    pub fn test_reference(
        &self,
        reference: &str,
        cancel: &CancellationToken,
    ) -> Result<(), SecretFailure> {
        validate_op_reference(reference).map_err(|detail| SecretFailure::ReferenceNotFound {
            secret_id: String::new(),
            name: reference.to_owned(),
            manager: self.id.clone(),
            detail,
        })?;
        let secret = self.read_single(reference, cancel)?;
        if secret.expose_secret().trim().is_empty() {
            return Err(SecretFailure::ReferenceNotFound {
                secret_id: String::new(),
                name: reference.to_owned(),
                manager: self.id.clone(),
                detail: "1Password field is empty".to_owned(),
            });
        }
        drop(secret);
        Ok(())
    }

    fn read_single(
        &self,
        reference: &str,
        cancel: &CancellationToken,
    ) -> Result<SecretString, SecretFailure> {
        let cmd = self.build_op_command(&["read", "--no-newline", reference])?;
        let out = run_subprocess(cmd, None, self.timeout(), cancel, &self.id, self.binary())?;
        if out.success {
            let mut text = out.stdout;
            let secret = SecretString::from(text.clone());
            text.zeroize();
            return Ok(secret);
        }
        Err(classify_op_stderr(&self.id, &out.stderr, Some(reference)))
    }
}

impl SecretManager for OnePasswordManager {
    fn id(&self) -> &ManagerId {
        &self.id
    }

    fn status(&self, cancel: &CancellationToken) -> ManagerStatus {
        let cmd = match self.build_op_command(&["whoami", "--format=json"]) {
            Ok(cmd) => cmd,
            Err(SecretFailure::ManagerAuthFailed { detail, .. }) => {
                return ManagerStatus::AuthFailed(detail);
            }
            Err(SecretFailure::ManagerLocked { .. }) => return ManagerStatus::Locked,
            Err(err) => return ManagerStatus::AuthFailed(err.short_message()),
        };
        match run_subprocess(cmd, None, self.timeout(), cancel, &self.id, self.binary()) {
            Ok(out) if out.success => {
                log::info!("secrets.manager.status manager={} status=ready", self.id);
                ManagerStatus::Ready
            }
            Ok(out) => {
                let status = match classify_op_stderr(&self.id, &out.stderr, None) {
                    SecretFailure::ManagerLocked { .. } => ManagerStatus::Locked,
                    SecretFailure::ManagerNotInstalled { .. } => ManagerStatus::NotInstalled,
                    SecretFailure::ManagerAuthFailed { detail, .. } => {
                        ManagerStatus::AuthFailed(detail)
                    }
                    other => ManagerStatus::AuthFailed(other.short_message()),
                };
                log::warn!(
                    "secrets.manager.status manager={} status={status:?}",
                    self.id
                );
                status
            }
            Err(SecretFailure::ManagerNotInstalled { .. }) => {
                log::warn!(
                    "secrets.manager.status manager={} status=NotInstalled",
                    self.id
                );
                ManagerStatus::NotInstalled
            }
            Err(SecretFailure::ManagerLocked { .. }) => {
                log::warn!("secrets.manager.status manager={} status=Locked", self.id);
                ManagerStatus::Locked
            }
            Err(other) => {
                let msg = other.short_message();
                log::warn!(
                    "secrets.manager.status manager={} status=AuthFailed err={msg}",
                    self.id
                );
                ManagerStatus::AuthFailed(msg)
            }
        }
    }

    fn resolve(
        &self,
        refs: &[&str],
        cancel: &CancellationToken,
    ) -> BTreeMap<String, Result<SecretString, SecretFailure>> {
        let mut results = BTreeMap::new();
        if refs.is_empty() {
            return results;
        }
        if cancel.is_cancelled() {
            for r in refs {
                results.insert((*r).to_owned(), Err(SecretFailure::Cancelled));
            }
            return results;
        }

        // Validate all references up front.
        let mut valid_refs: Vec<&str> = Vec::new();
        for &r in refs {
            match validate_op_reference(r) {
                Ok(()) => {
                    if !valid_refs.contains(&r) {
                        valid_refs.push(r);
                    }
                }
                Err(detail) => {
                    results.insert(
                        r.to_owned(),
                        Err(SecretFailure::ReferenceNotFound {
                            secret_id: String::new(),
                            name: r.to_owned(),
                            manager: self.id.clone(),
                            detail,
                        }),
                    );
                }
            }
        }
        if valid_refs.is_empty() {
            return results;
        }

        // Build inline random 128-bit hex sentinel framing (no structural newlines).
        let start_sentinel = format!("__BRUK_S_{}__", Uuid::new_v4().as_simple());
        let sep_sentinel = format!("__BRUK_P_{}__", Uuid::new_v4().as_simple());
        let end_sentinel = format!("__BRUK_E_{}__", Uuid::new_v4().as_simple());

        let mut template = String::new();
        template.push_str(&start_sentinel);
        for (idx, r) in valid_refs.iter().enumerate() {
            if idx > 0 {
                template.push_str(&sep_sentinel);
            }
            template.push_str("{{ ");
            template.push_str(r);
            template.push_str(" }}");
        }
        template.push_str(&end_sentinel);

        let cmd = match self.build_op_command(&["inject"]) {
            Ok(cmd) => cmd,
            Err(err) => {
                for r in valid_refs {
                    results.insert(r.to_owned(), Err(err.clone()));
                }
                return results;
            }
        };

        let out = match run_subprocess(
            cmd,
            Some(template.as_bytes()),
            self.timeout(),
            cancel,
            &self.id,
            self.binary(),
        ) {
            Ok(out) => out,
            Err(err) => {
                for r in valid_refs {
                    results.insert(r.to_owned(), Err(err.clone()));
                }
                return results;
            }
        };

        if out.success {
            let mut raw = out.stdout;
            match parse_inline_sentinels(
                &raw,
                &start_sentinel,
                &sep_sentinel,
                &end_sentinel,
                valid_refs.len(),
            ) {
                Ok(values) => {
                    for (r, val) in valid_refs.iter().zip(values) {
                        results.insert((*r).to_owned(), Ok(val));
                    }
                    raw.zeroize();
                    return results;
                }
                Err(detail) => {
                    raw.zeroize();
                    let err = SecretFailure::ManagerAuthFailed {
                        manager: self.id.clone(),
                        detail,
                    };
                    for r in valid_refs {
                        results.insert(r.to_owned(), Err(err.clone()));
                    }
                    return results;
                }
            }
        }

        // Smart error handling: inspect stderr FIRST to prevent prompt or rate-limit storms.
        let classified = classify_op_stderr(&self.id, &out.stderr, None);
        let is_not_found = matches!(classified, SecretFailure::ReferenceNotFound { .. });

        if !is_not_found || valid_refs.len() == 1 {
            let final_err = if is_not_found && valid_refs.len() == 1 {
                classify_op_stderr(&self.id, &out.stderr, Some(valid_refs[0]))
            } else {
                classified
            };
            for r in valid_refs {
                results.insert(r.to_owned(), Err(final_err.clone()));
            }
            return results;
        }

        // Only when stderr indicates a missing vault/item/field with multiple refs:
        // probe individually so valid refs succeed, stopping immediately on auth/lock/rate-limit/cancel.
        for (idx, r) in valid_refs.iter().enumerate() {
            if cancel.is_cancelled() {
                for remaining in &valid_refs[idx..] {
                    results.insert((*remaining).to_owned(), Err(SecretFailure::Cancelled));
                }
                break;
            }
            match self.read_single(r, cancel) {
                Ok(secret) => {
                    results.insert((*r).to_owned(), Ok(secret));
                }
                Err(err) => {
                    let abort_batch = matches!(
                        err,
                        SecretFailure::ManagerAuthFailed { .. }
                            | SecretFailure::ManagerLocked { .. }
                            | SecretFailure::ManagerNotInstalled { .. }
                            | SecretFailure::TimedOut { .. }
                            | SecretFailure::Cancelled
                    );
                    results.insert((*r).to_owned(), Err(err.clone()));
                    if abort_batch {
                        for remaining in &valid_refs[idx + 1..] {
                            results.insert((*remaining).to_owned(), Err(err.clone()));
                        }
                        break;
                    }
                }
            }
        }

        results
    }

    fn validate_reference(&self, reference: &str) -> Result<(), String> {
        validate_op_reference(reference)
    }
}

/// Extracts `expected_count` secret slices framed by inline sentinels (tolerating any trailing
/// newline `op inject` appends after `end_sentinel`).
pub fn parse_inline_sentinels(
    raw: &str,
    start_sentinel: &str,
    sep_sentinel: &str,
    end_sentinel: &str,
    expected_count: usize,
) -> Result<Vec<SecretString>, String> {
    let trimmed_end = raw.trim_end_matches(['\r', '\n']);
    let Some(after_start) = trimmed_end.strip_prefix(start_sentinel) else {
        return Err("unexpected op inject output: missing start sentinel".to_owned());
    };
    let Some(body) = after_start.strip_suffix(end_sentinel) else {
        return Err("unexpected op inject output: missing end sentinel".to_owned());
    };
    let parts: Vec<&str> = if expected_count == 1 {
        vec![body]
    } else {
        body.split(sep_sentinel).collect()
    };
    if parts.len() != expected_count {
        return Err(format!(
            "unexpected op inject output: expected {expected_count} values, got {}",
            parts.len()
        ));
    }
    Ok(parts
        .into_iter()
        .map(|slice| SecretString::from(slice.to_owned()))
        .collect())
}

fn classify_op_stderr(manager_id: &str, stderr: &str, single_ref: Option<&str>) -> SecretFailure {
    let first_line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("op exited with a non-zero status")
        .to_owned();
    let lower = stderr.to_ascii_lowercase();

    if lower.contains("429") || lower.contains("rate limit") || lower.contains("too many requests")
    {
        return SecretFailure::ManagerAuthFailed {
            manager: manager_id.to_owned(),
            detail: format!("1Password rate limit reached: {first_line}"),
        };
    }
    if lower.contains("401")
        || lower.contains("invalid token")
        || lower.contains("unauthorized")
        || lower.contains("service account")
        || lower.contains("authentication failed")
        || lower.contains("invalid credentials")
    {
        return SecretFailure::ManagerAuthFailed {
            manager: manager_id.to_owned(),
            detail: first_line,
        };
    }
    if lower.contains("not currently signed in")
        || lower.contains("locked")
        || lower.contains("canceled")
        || lower.contains("cancelled")
        || lower.contains("touch id")
        || lower.contains("app integration")
        || lower.contains("sign in")
        || lower.contains("authorization prompt")
    {
        return SecretFailure::ManagerLocked {
            manager: manager_id.to_owned(),
            detail: first_line,
        };
    }
    if lower.contains("isn't a vault")
        || lower.contains("isn't an item")
        || lower.contains("isn't a field")
        || lower.contains("does not have a field")
        || lower.contains("could not find")
        || lower.contains("not found")
        || lower.contains("no item")
        || lower.contains("404")
    {
        return SecretFailure::ReferenceNotFound {
            secret_id: String::new(),
            name: single_ref.unwrap_or("").to_owned(),
            manager: manager_id.to_owned(),
            detail: first_line,
        };
    }

    SecretFailure::ManagerAuthFailed {
        manager: manager_id.to_owned(),
        detail: first_line,
    }
}

// ---------------------------------------------------------------------------------------------
// Generic Command Manager
// ---------------------------------------------------------------------------------------------

const COMMAND_MANAGER_MAX_CONCURRENCY: usize = 4;

/// Built-in CLI command manager preset template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandManagerPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub suggested_auth_env: Option<&'static str>,
    pub reference_placeholder: &'static str,
    pub not_found_pattern: Option<&'static str>,
}

impl CommandManagerPreset {
    pub fn to_config(&self, id: ManagerId) -> CommandManagerConfig {
        CommandManagerConfig {
            id,
            name: self.name.to_owned(),
            program: self.program.to_owned(),
            args: self.args.iter().map(|s| (*s).to_owned()).collect(),
            timeout_secs: 30,
            auth_env: BTreeMap::new(),
            not_found_pattern: self.not_found_pattern.map(str::to_owned),
            enabled: true,
        }
    }
}

/// Built-in presets for popular CLI secret managers (§6).
pub const BUILT_IN_COMMAND_MANAGER_PRESETS: &[CommandManagerPreset] = &[
    CommandManagerPreset {
        id: "bws",
        name: "Bitwarden Secrets Manager",
        program: "bws",
        args: &["secret", "get", "--output", "raw", "--", "{ref}"],
        suggested_auth_env: Some("BWS_ACCESS_TOKEN"),
        reference_placeholder: "00000000-0000-0000-0000-000000000000",
        not_found_pattern: Some("not found"),
    },
    CommandManagerPreset {
        id: "vault",
        name: "HashiCorp Vault",
        program: "vault",
        args: &["kv", "get", "-field=value", "--", "{ref}"],
        suggested_auth_env: Some("VAULT_TOKEN"),
        reference_placeholder: "secret/app/linear",
        not_found_pattern: Some("No value found"),
    },
    CommandManagerPreset {
        id: "doppler",
        name: "Doppler",
        program: "doppler",
        args: &["secrets", "get", "--plain", "--", "{ref}"],
        suggested_auth_env: Some("DOPPLER_TOKEN"),
        reference_placeholder: "LINEAR_TOKEN",
        not_found_pattern: Some("Could not find"),
    },
    CommandManagerPreset {
        id: "pass",
        name: "pass (Unix password store)",
        program: "pass",
        args: &["show", "--", "{ref}"],
        suggested_auth_env: None,
        reference_placeholder: "work/linear-token",
        not_found_pattern: Some("is not in the password store"),
    },
    CommandManagerPreset {
        id: "aws",
        name: "AWS Secrets Manager",
        program: "aws",
        args: &[
            "secretsmanager",
            "get-secret-value",
            "--query",
            "SecretString",
            "--output",
            "text",
            "--secret-id",
            "{ref}",
        ],
        suggested_auth_env: None,
        reference_placeholder: "prod/linear/token",
        not_found_pattern: Some("ResourceNotFoundException"),
    },
    CommandManagerPreset {
        id: "gcp",
        name: "GCP Secret Manager",
        program: "gcloud",
        args: &[
            "secrets", "versions", "access", "latest", "--secret", "{ref}",
        ],
        suggested_auth_env: None,
        reference_placeholder: "linear-token",
        not_found_pattern: Some("NOT_FOUND"),
    },
];

/// Generic CLI secret manager executed directly via `Command::new` (never via shell).
pub struct CommandSecretManager {
    config: CommandManagerConfig,
    store: Arc<dyn SecretStore>,
    db_instance_id: String,
}

impl CommandSecretManager {
    pub fn new(
        config: CommandManagerConfig,
        store: Arc<dyn SecretStore>,
        db_instance_id: String,
    ) -> Self {
        Self {
            config,
            store,
            db_instance_id,
        }
    }

    fn resolve_auth_env(&self) -> Result<Vec<(String, SecretString)>, SecretFailure> {
        let mut resolved = Vec::with_capacity(self.config.auth_env.len());
        for (env_name, secret_id) in &self.config.auth_env {
            let account_key = keychain_account_key(&self.db_instance_id, secret_id);
            match self.store.get(&account_key) {
                Ok(Some(val)) => resolved.push((env_name.clone(), val)),
                Ok(None) => {
                    return Err(SecretFailure::ManagerAuthFailed {
                        manager: self.config.id.clone(),
                        detail: format!("auth secret for {env_name} is missing from Keychain"),
                    });
                }
                Err(SecretError::Locked) => {
                    return Err(SecretFailure::ManagerLocked {
                        manager: self.config.id.clone(),
                        detail: "OS Keychain is locked".to_owned(),
                    });
                }
                Err(err) => {
                    return Err(SecretFailure::StoreUnavailable {
                        detail: err.to_string(),
                    });
                }
            }
        }
        Ok(resolved)
    }

    fn resolve_one(
        &self,
        reference: &str,
        auth_env: &[(String, SecretString)],
        cancel: &CancellationToken,
    ) -> Result<SecretString, SecretFailure> {
        self.validate_reference(reference)
            .map_err(|detail| SecretFailure::ReferenceNotFound {
                secret_id: String::new(),
                name: reference.to_owned(),
                manager: self.config.id.clone(),
                detail,
            })?;

        let mut cmd = Command::new(self.config.program.trim());
        for arg in &self.config.args {
            cmd.arg(arg.replace("{ref}", reference));
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (env_name, secret_val) in auth_env {
            cmd.env(env_name, secret_val.expose_secret());
        }

        let timeout = Duration::from_secs(self.config.timeout_secs.max(1));
        let out = run_subprocess(
            cmd,
            None,
            timeout,
            cancel,
            &self.config.id,
            self.config.program.trim(),
        )?;

        if out.success {
            let mut raw = out.stdout;
            // Strip at most one trailing \r\n or \n.
            let stripped = raw
                .strip_suffix("\r\n")
                .or_else(|| raw.strip_suffix('\n'))
                .unwrap_or(&raw)
                .to_owned();
            raw.zeroize();
            return Ok(SecretString::from(stripped));
        }

        let first_line = out
            .stderr
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("command exited with a non-zero status")
            .to_owned();
        if let Some(pattern) = self
            .config
            .not_found_pattern
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            && out
                .stderr
                .to_ascii_lowercase()
                .contains(&pattern.to_ascii_lowercase())
        {
            return Err(SecretFailure::ReferenceNotFound {
                secret_id: String::new(),
                name: reference.to_owned(),
                manager: self.config.id.clone(),
                detail: first_line,
            });
        }
        Err(SecretFailure::ManagerAuthFailed {
            manager: self.config.id.clone(),
            detail: first_line,
        })
    }
}

impl SecretManager for CommandSecretManager {
    fn id(&self) -> &ManagerId {
        &self.config.id
    }

    fn status(&self, _cancel: &CancellationToken) -> ManagerStatus {
        let program = self.config.program.trim();
        if program.is_empty() {
            return ManagerStatus::NotInstalled;
        }
        if !program_exists(program) {
            return ManagerStatus::NotInstalled;
        }
        match self.resolve_auth_env() {
            Ok(_) => ManagerStatus::Ready,
            Err(SecretFailure::ManagerLocked { .. }) => ManagerStatus::Locked,
            Err(err) => ManagerStatus::AuthFailed(err.short_message()),
        }
    }

    fn resolve(
        &self,
        refs: &[&str],
        cancel: &CancellationToken,
    ) -> BTreeMap<String, Result<SecretString, SecretFailure>> {
        let mut results = BTreeMap::new();
        if refs.is_empty() {
            return results;
        }
        if cancel.is_cancelled() {
            for r in refs {
                results.insert((*r).to_owned(), Err(SecretFailure::Cancelled));
            }
            return results;
        }
        let auth_env = match self.resolve_auth_env() {
            Ok(env) => env,
            Err(err) => {
                for r in refs {
                    results.insert((*r).to_owned(), Err(err.clone()));
                }
                return results;
            }
        };

        let mut unique: Vec<&str> = Vec::new();
        for &r in refs {
            if !unique.contains(&r) {
                unique.push(r);
            }
        }

        if unique.len() == 1 {
            let r = unique[0];
            results.insert(r.to_owned(), self.resolve_one(r, &auth_env, cancel));
            return results;
        }

        // Bounded parallel execution (up to 4 concurrent subprocesses).
        for chunk in unique.chunks(COMMAND_MANAGER_MAX_CONCURRENCY) {
            if cancel.is_cancelled() {
                for &r in chunk {
                    results.insert(r.to_owned(), Err(SecretFailure::Cancelled));
                }
                continue;
            }
            std::thread::scope(|scope| {
                let mut handles = Vec::with_capacity(chunk.len());
                for &r in chunk {
                    let auth_ref = &auth_env;
                    handles.push((
                        r,
                        scope.spawn(move || self.resolve_one(r, auth_ref, cancel)),
                    ));
                }
                for (r, handle) in handles {
                    let res = handle.join().unwrap_or_else(|_| {
                        Err(SecretFailure::ManagerAuthFailed {
                            manager: self.config.id.clone(),
                            detail: "manager worker panicked".to_owned(),
                        })
                    });
                    results.insert(r.to_owned(), res);
                }
            });
        }

        results
    }

    fn validate_reference(&self, reference: &str) -> Result<(), String> {
        if reference.trim().is_empty() {
            return Err("secret reference cannot be empty".to_owned());
        }
        if reference.contains('\r') || reference.contains('\n') || reference.contains('\0') {
            return Err(
                "secret reference cannot contain control characters or newlines".to_owned(),
            );
        }
        // Flag-injection protection: reject references starting with '-' unless `{ref}` comes after `--`.
        if reference.starts_with('-') && !self.config.ref_is_after_double_dash() {
            return Err(
                "secret reference cannot start with '-' unless the manager args place '--' before '{ref}'"
                    .to_owned(),
            );
        }
        Ok(())
    }
}

fn program_exists(program: &str) -> bool {
    if program.contains('/') {
        return factory_types::expand_tilde(program).is_file();
    }
    let path = std::env::var("PATH").unwrap_or_default();
    path.split(':').filter(|d| !d.is_empty()).any(|dir| {
        let candidate = std::path::Path::new(dir).join(program);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            candidate
                .metadata()
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        }
        #[cfg(not(unix))]
        {
            candidate.is_file()
        }
    })
}

// ---------------------------------------------------------------------------------------------
// Subprocess helper with timeout + cooperative cancellation
// ---------------------------------------------------------------------------------------------

struct SubprocessOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

fn run_subprocess(
    mut cmd: Command,
    stdin_bytes: Option<&[u8]>,
    timeout: Duration,
    cancel: &CancellationToken,
    manager_id: &str,
    binary_name: &str,
) -> Result<SubprocessOutput, SecretFailure> {
    if cancel.is_cancelled() {
        return Err(SecretFailure::Cancelled);
    }

    let mut child: Child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(SecretFailure::ManagerNotInstalled {
                manager: manager_id.to_owned(),
                binary: binary_name.to_owned(),
            });
        }
        Err(err) => {
            return Err(SecretFailure::ManagerAuthFailed {
                manager: manager_id.to_owned(),
                detail: format!("failed to launch '{binary_name}': {err}"),
            });
        }
    };

    let pid = child.id();
    cancel.register_pid(pid);

    if let Some(bytes) = stdin_bytes {
        let payload = bytes.to_vec();
        if let Some(mut stdin) = child.stdin.take() {
            std::thread::spawn(move || {
                let _ = stdin.write_all(&payload);
            });
        }
    } else {
        drop(child.stdin.take());
    }

    let stdout_handle = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });
    let stderr_handle = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });

    let started = Instant::now();
    let status = loop {
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            cancel.unregister_pid(pid);
            return Err(SecretFailure::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    cancel.unregister_pid(pid);
                    return Err(SecretFailure::TimedOut {
                        manager: manager_id.to_owned(),
                        timeout_secs: timeout.as_secs(),
                    });
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                cancel.unregister_pid(pid);
                return Err(SecretFailure::ManagerAuthFailed {
                    manager: manager_id.to_owned(),
                    detail: format!("failed waiting on '{binary_name}': {err}"),
                });
            }
        }
    };
    cancel.unregister_pid(pid);

    let mut stdout_bytes = stdout_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let stderr_bytes = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    let stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();
    stdout_bytes.zeroize();
    let stderr = String::from_utf8_lossy(&stderr_bytes).into_owned();

    Ok(SubprocessOutput {
        success: status.success(),
        stdout,
        stderr,
    })
}

// ---------------------------------------------------------------------------------------------
// ManagerRegistry
// ---------------------------------------------------------------------------------------------

/// Registry of active external secret managers constructed from local `SecretsSettings`.
pub struct ManagerRegistry {
    managers: BTreeMap<ManagerId, Arc<dyn SecretManager>>,
}

impl ManagerRegistry {
    pub fn from_settings(
        settings: &SecretsSettings,
        store: Arc<dyn SecretStore>,
        db_instance_id: &str,
    ) -> Self {
        let mut managers: BTreeMap<ManagerId, Arc<dyn SecretManager>> = BTreeMap::new();
        if settings.one_password.enabled {
            managers.insert(
                ONE_PASSWORD_MANAGER_ID.to_owned(),
                Arc::new(OnePasswordManager::new(
                    settings.one_password.clone(),
                    Arc::clone(&store),
                    db_instance_id.to_owned(),
                )),
            );
        }
        for cmd_cfg in &settings.command_managers {
            if cmd_cfg.enabled {
                managers.insert(
                    cmd_cfg.id.clone(),
                    Arc::new(CommandSecretManager::new(
                        cmd_cfg.clone(),
                        Arc::clone(&store),
                        db_instance_id.to_owned(),
                    )),
                );
            }
        }
        Self { managers }
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn SecretManager>> {
        self.managers.get(id).cloned()
    }

    pub fn insert(&mut self, manager: Arc<dyn SecretManager>) {
        self.managers.insert(manager.id().clone(), manager);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;
    use crate::store::MemoryStore;

    #[test]
    fn validates_op_references_strictly() {
        assert!(validate_op_reference("op://Eng/Linear/token").is_ok());
        assert!(validate_op_reference("op://Eng/Linear/API/token").is_ok());
        assert!(validate_op_reference("op://Eng/Linear/token?attribute=otp").is_ok());

        // Rejects $ expansion, template braces, control chars, bad segment counts
        assert!(validate_op_reference("op://Eng/Linear/$OP_SERVICE_ACCOUNT_TOKEN").is_err());
        assert!(validate_op_reference("op://Eng/Linear/tok}}en").is_err());
        assert!(validate_op_reference("op://Eng/Linear/tok{{en").is_err());
        assert!(validate_op_reference("op://Eng/Linear/tok\nen").is_err());
        assert!(validate_op_reference("op://Eng/Linear").is_err());
        assert!(validate_op_reference("op://Eng//token").is_err());
        assert!(validate_op_reference("https://example.com").is_err());
    }

    #[test]
    fn inline_sentinel_parser_handles_multiline_and_trailing_newline() {
        let raw = "__S__line1\nline2__P__second-value__E__\n";
        let out = parse_inline_sentinels(raw, "__S__", "__P__", "__E__", 2).unwrap();
        assert_eq!(out[0].expose_secret(), "line1\nline2");
        assert_eq!(out[1].expose_secret(), "second-value");
    }

    #[cfg(unix)]
    fn write_script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn one_password_service_account_sanitizes_env_and_omits_account_flag() {
        let temp = TempDir::new().unwrap();
        let log_file = temp.path().join("op.log");
        let fake_op = write_script(
            temp.path(),
            "op",
            &format!(
                r#"echo "ARGS:$*" >> "{log}"
echo "TOKEN:$OP_SERVICE_ACCOUNT_TOKEN" >> "{log}"
echo "CONNECT:$OP_CONNECT_HOST:$OP_CONNECT_TOKEN" >> "{log}"
echo "ACCT:$OP_ACCOUNT" >> "{log}"
if [ "$1" = "inject" ]; then
  sed 's/{{{{ op:\/\/Eng\/Linear\/token }}}}/lin_tok_1234567890/g; s/{{{{ op:\/\/Eng\/Stripe\/key }}}}/sk_live_9876543210/g'
  printf '\n'
fi"#,
                log = log_file.display()
            ),
        );

        let store = Arc::new(MemoryStore::new());
        store
            .set(
                "db1:sa-id",
                "1Password SA",
                &SecretString::from("ops_secret_token_abc".to_owned()),
            )
            .unwrap();

        let mgr = OnePasswordManager::new(
            OnePasswordConfig {
                enabled: true,
                binary_path: fake_op.display().to_string(),
                auth_mode: OnePasswordAuthMode::ServiceAccount,
                desktop_account: "should-never-be-passed".to_owned(),
                service_account_secret_id: Some("sa-id".to_owned()),
            },
            store,
            "db1".to_owned(),
        );

        let res = mgr.resolve(
            &["op://Eng/Linear/token", "op://Eng/Stripe/key"],
            &CancellationToken::new(),
        );
        assert_eq!(
            res.get("op://Eng/Linear/token")
                .unwrap()
                .as_ref()
                .unwrap()
                .expose_secret(),
            "lin_tok_1234567890"
        );
        assert_eq!(
            res.get("op://Eng/Stripe/key")
                .unwrap()
                .as_ref()
                .unwrap()
                .expose_secret(),
            "sk_live_9876543210"
        );

        let log = fs::read_to_string(&log_file).unwrap();
        assert!(log.contains("ARGS:inject"), "log: {log}");
        assert!(
            !log.contains("--account"),
            "must never pass --account: {log}"
        );
        assert!(log.contains("TOKEN:ops_secret_token_abc"));
        assert!(log.contains("CONNECT::"));
        assert!(log.contains("ACCT:\n"));
    }

    #[cfg(unix)]
    #[test]
    fn one_password_does_not_storm_op_read_on_401_429_or_locked() {
        let temp = TempDir::new().unwrap();
        let count_file = temp.path().join("calls.txt");
        let fake_op = write_script(
            temp.path(),
            "op",
            &format!(
                r#"echo "call:$1" >> "{count}"
echo "[ERROR] 429 Too Many Requests: rate limit exceeded" >&2
exit 1"#,
                count = count_file.display()
            ),
        );

        let store = Arc::new(MemoryStore::new());
        store
            .set(
                "db1:sa-id",
                "1Password SA",
                &SecretString::from("ops_tok".to_owned()),
            )
            .unwrap();

        let mgr = OnePasswordManager::new(
            OnePasswordConfig {
                enabled: true,
                binary_path: fake_op.display().to_string(),
                auth_mode: OnePasswordAuthMode::ServiceAccount,
                desktop_account: String::new(),
                service_account_secret_id: Some("sa-id".to_owned()),
            },
            store,
            "db1".to_owned(),
        );

        let res = mgr.resolve(
            &["op://Eng/A/tok", "op://Eng/B/tok", "op://Eng/C/tok"],
            &CancellationToken::new(),
        );
        assert_eq!(res.len(), 3);
        for v in res.values() {
            assert!(matches!(v, Err(SecretFailure::ManagerAuthFailed { .. })));
        }
        let calls = fs::read_to_string(&count_file).unwrap();
        assert_eq!(
            calls.lines().count(),
            1,
            "expected exactly 1 op inject call, got:\n{calls}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn command_manager_rejects_flag_injection_and_respects_cancellation() {
        let temp = TempDir::new().unwrap();
        let slow_script = write_script(temp.path(), "slow-mgr", "sleep 30");

        let cfg = CommandManagerConfig {
            id: "custom".to_owned(),
            name: "Custom".to_owned(),
            program: slow_script.display().to_string(),
            args: vec!["get".to_owned(), "{ref}".to_owned()],
            timeout_secs: 30,
            auth_env: BTreeMap::new(),
            not_found_pattern: None,
            enabled: true,
        };
        let mgr = CommandSecretManager::new(cfg, Arc::new(MemoryStore::new()), "db1".to_owned());

        // Rejects leading '-' without '--'
        assert!(mgr.validate_reference("-evil-flag").is_err());

        // Cancellation kills hung subprocess immediately
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            cancel_clone.cancel();
        });
        let started = Instant::now();
        let res = mgr.resolve(&["valid-ref"], &cancel);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(
            res.get("valid-ref").unwrap().as_ref().unwrap_err(),
            &SecretFailure::Cancelled
        );
    }
}
