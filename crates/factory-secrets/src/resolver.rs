//! Async / off-UI-thread secret resolution, 4-level precedence, single-flight manager
//! coalescing, execution caches (`RunSecretsCache`, `ChatSecretsCache`), and workflow AST preflight.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Condvar, Mutex, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use secrecy::SecretString;

use factory_types::{ProjectId, current_unix_seconds};

use super::{
    EnvScope, Exposure, ManagerId, SecretFailure, SecretId, SecretKind, SecretRecord, SecretSource,
    SecretState, SecretsSettings, keychain_account_key,
    managers::{CancellationToken, ManagerRegistry},
    redactor::SecretRedactor,
    store::{SecretError, SecretStore},
};

/// Idle TTL for per-chat secret caching (§4: 10 minutes).
pub const CHAT_SECRETS_IDLE_TTL: Duration = Duration::from_secs(10 * 60);

/// Origin of a resolved environment value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueOrigin {
    Secret {
        secret_id: SecretId,
        is_app_scoped: bool,
    },
    PlainProjectVar,
    HostEnv,
}

/// One resolved environment value with its exposure level and origin.
#[derive(Debug, Clone)]
pub struct ResolvedValue {
    pub value: SecretString,
    pub exposure: Exposure,
    pub origin: ValueOrigin,
}

/// Result of resolving secrets for an execution (workflow step, shell step, or chat turn).
#[derive(Debug, Clone, Default)]
pub struct ResolvedSecrets {
    /// Successfully resolved values (Env secrets, plain project vars, or undeclared host vars).
    pub values: BTreeMap<String, ResolvedValue>,
    /// Existing secret records that failed to read (fail-stop; never masked by `${VAR:-default}`).
    pub failures: BTreeMap<String, SecretFailure>,
    /// Variable names that MUST be removed from spawned child environments (`env_remove`):
    /// excluded app secrets, unshared app secrets, unreferenced `McpOnly` secrets, `Internal`
    /// secret names, and Bruk tool tokens (`GH_TOKEN`, `OP_SERVICE_ACCOUNT_TOKEN`, `OP_CONNECT_*`).
    pub strip_from_child_env: BTreeSet<String>,
    /// Keychain records whose SQLite state was `Ready` but `store.get()` returned `None`,
    /// signalling they should be transitioned to `NeedsValue`.
    pub needs_value_downgrades: Vec<SecretId>,
}

impl ResolvedSecrets {
    /// Builds a [`SecretRedactor`] containing every resolved secret value (never plain vars or host env).
    pub fn build_redactor(&self) -> SecretRedactor {
        let pairs: Vec<(&str, &SecretString)> = self
            .values
            .iter()
            .filter(|(_, rv)| matches!(rv.origin, ValueOrigin::Secret { .. }))
            .map(|(name, rv)| (name.as_str(), &rv.value))
            .collect();
        SecretRedactor::from_secrets(pairs)
    }

    /// Builds a redactor combining resolved Env secrets with extra Internal secrets.
    pub fn build_redactor_with_internals<'a>(
        &'a self,
        internals: impl IntoIterator<Item = &'a SecretString>,
    ) -> SecretRedactor {
        let mut pairs: Vec<(&str, &SecretString)> = self
            .values
            .iter()
            .filter(|(_, rv)| matches!(rv.origin, ValueOrigin::Secret { .. }))
            .map(|(name, rv)| (name.as_str(), &rv.value))
            .collect();
        for secret in internals {
            pairs.push(("internal", secret));
        }
        SecretRedactor::from_secrets(pairs)
    }

    /// Plain + `AgentAndSteps` env map for shell steps or Antigravity (never includes `McpOnly`).
    pub fn agent_and_steps_env(&self) -> BTreeMap<String, String> {
        use secrecy::ExposeSecret;
        self.values
            .iter()
            .filter(|(_, rv)| rv.exposure == Exposure::AgentAndSteps)
            .map(|(k, rv)| (k.clone(), rv.value.expose_secret().to_owned()))
            .collect()
    }

    /// Plain + `AgentAndSteps` + referenced `McpOnly` env map for Claude / Codex.
    pub fn full_agent_env(&self) -> BTreeMap<String, String> {
        use secrecy::ExposeSecret;
        self.values
            .iter()
            .map(|(k, rv)| (k.clone(), rv.value.expose_secret().to_owned()))
            .collect()
    }

    /// Names of resolved `McpOnly` secrets (used for Claude `sandbox.credentials.envVars` and
    /// Codex `shell_environment_policy.exclude`).
    pub fn mcp_only_secret_names(&self) -> BTreeSet<String> {
        self.values
            .iter()
            .filter(|(_, rv)| {
                rv.exposure == Exposure::McpOnly && matches!(rv.origin, ValueOrigin::Secret { .. })
            })
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Names of resolved `AgentAndSteps` secrets.
    pub fn agent_and_steps_secret_names(&self) -> BTreeSet<String> {
        self.values
            .iter()
            .filter(|(_, rv)| {
                rv.exposure == Exposure::AgentAndSteps
                    && matches!(rv.origin, ValueOrigin::Secret { .. })
            })
            .map(|(k, _)| k.clone())
            .collect()
    }
}

/// Immutable snapshot of secret metadata and project scoping rules so background threads never
/// hold the SQLite database lock during Keychain/manager calls.
#[derive(Debug, Clone, Default)]
pub struct SecretsMetadataSnapshot {
    pub db_instance_id: String,
    pub records: Vec<SecretRecord>,
    /// Plain (non-secret) project env vars per project.
    pub project_plain_env: BTreeMap<ProjectId, BTreeMap<String, String>>,
    /// Per-project opted-out App secret IDs (`ProjectSettings.excluded_app_secrets`).
    pub project_excluded_app_secrets: BTreeMap<ProjectId, BTreeSet<SecretId>>,
    pub settings: SecretsSettings,
}

impl SecretsMetadataSnapshot {
    pub fn find_by_id(&self, id: &str) -> Option<&SecretRecord> {
        self.records.iter().find(|r| r.id == id)
    }

    /// Returns all active `Env` secrets visible to `project` (project secrets shadowing app secrets).
    pub fn visible_env_secrets_for_project(&self, project: ProjectId) -> Vec<&SecretRecord> {
        let excluded = self.project_excluded_app_secrets.get(&project);
        let mut by_lower: BTreeMap<String, &SecretRecord> = BTreeMap::new();
        for rec in &self.records {
            if let SecretKind::Env {
                scope: EnvScope::App { shared_with },
                ..
            } = &rec.kind
                && shared_with.includes_project(project)
                && !excluded.is_some_and(|ex| ex.contains(&rec.id))
            {
                by_lower.insert(rec.name.to_ascii_lowercase(), rec);
            }
        }
        // Project secrets shadow App secrets of the same name (case-insensitive).
        for rec in &self.records {
            if let SecretKind::Env {
                scope: EnvScope::Project(pid),
                ..
            } = &rec.kind
                && *pid == project
            {
                by_lower.insert(rec.name.to_ascii_lowercase(), rec);
            }
        }
        by_lower.into_values().collect()
    }
}

/// Cache interface shared by workflow runs and chat sessions.
pub trait SecretsCache: Send {
    fn get(&mut self, secret_id: &str, current_generation: u64) -> Option<SecretString>;
    fn insert(&mut self, secret_id: SecretId, value: SecretString, current_generation: u64);
    fn clear_and_zeroize(&mut self);
}

/// No-op cache used for one-shot lookups.
#[derive(Debug, Default)]
pub struct NoopSecretsCache;

impl SecretsCache for NoopSecretsCache {
    fn get(&mut self, _secret_id: &str, _current_generation: u64) -> Option<SecretString> {
        None
    }
    fn insert(&mut self, _secret_id: SecretId, _value: SecretString, _current_generation: u64) {}
    fn clear_and_zeroize(&mut self) {}
}

/// Per-workflow-run secret cache. Cleared and zeroized immediately when suspending at an
/// `ApprovalGate` so credentials do not sit in RAM while waiting for human review (§4).
#[derive(Debug, Default)]
pub struct RunSecretsCache {
    generation: u64,
    values: BTreeMap<SecretId, SecretString>,
}

impl RunSecretsCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl SecretsCache for RunSecretsCache {
    fn get(&mut self, secret_id: &str, current_generation: u64) -> Option<SecretString> {
        if self.generation != current_generation {
            self.clear_and_zeroize();
            self.generation = current_generation;
            return None;
        }
        self.values.get(secret_id).cloned()
    }

    fn insert(&mut self, secret_id: SecretId, value: SecretString, current_generation: u64) {
        if self.generation != current_generation {
            self.clear_and_zeroize();
            self.generation = current_generation;
        }
        self.values.insert(secret_id, value);
    }

    fn clear_and_zeroize(&mut self) {
        // Dropping SecretString runs ZeroizeOnDrop on each buffered secret.
        self.values.clear();
    }
}

/// Per-chat secret cache with a 10-minute idle TTL and instant invalidation on `secrets_generation` bump.
#[derive(Debug)]
pub struct ChatSecretsCache {
    generation: u64,
    last_access: Option<Instant>,
    ttl: Duration,
    values: BTreeMap<SecretId, SecretString>,
}

impl Default for ChatSecretsCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatSecretsCache {
    pub fn new() -> Self {
        Self::with_ttl(CHAT_SECRETS_IDLE_TTL)
    }

    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            generation: 0,
            last_access: None,
            ttl,
            values: BTreeMap::new(),
        }
    }

    fn check_freshness(&mut self, current_generation: u64) {
        let expired = self
            .last_access
            .is_some_and(|last| last.elapsed() >= self.ttl);
        if self.generation != current_generation || expired {
            self.clear_and_zeroize();
            self.generation = current_generation;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl SecretsCache for ChatSecretsCache {
    fn get(&mut self, secret_id: &str, current_generation: u64) -> Option<SecretString> {
        self.check_freshness(current_generation);
        let hit = self.values.get(secret_id).cloned();
        if hit.is_some() {
            self.last_access = Some(Instant::now());
        }
        hit
    }

    fn insert(&mut self, secret_id: SecretId, value: SecretString, current_generation: u64) {
        self.check_freshness(current_generation);
        self.values.insert(secret_id, value);
        self.last_access = Some(Instant::now());
    }

    fn clear_and_zeroize(&mut self) {
        self.values.clear();
        self.last_access = None;
    }
}

// ---------------------------------------------------------------------------------------------
// Single-flight manager coalescing
// ---------------------------------------------------------------------------------------------

type CoalescedBatchResults = (
    Instant,
    BTreeMap<String, Result<SecretString, SecretFailure>>,
);

struct SingleFlightEntry {
    in_flight: Mutex<bool>,
    cv: Condvar,
    /// Short-lived coalesced results shared among callers that arrived while a batch was running.
    recent: Mutex<Option<CoalescedBatchResults>>,
}

impl SingleFlightEntry {
    fn new() -> Self {
        Self {
            in_flight: Mutex::new(false),
            cv: Condvar::new(),
            recent: Mutex::new(None),
        }
    }
}

const SINGLE_FLIGHT_COALESCE_WINDOW: Duration = Duration::from_secs(2);

// ---------------------------------------------------------------------------------------------
// SecretResolver
// ---------------------------------------------------------------------------------------------

/// Off-UI-thread secret resolver shared across workflow runs, chat turns, and GitHub sync jobs.
pub struct SecretResolver {
    store: Arc<dyn SecretStore>,
    snapshot: RwLock<Arc<SecretsMetadataSnapshot>>,
    custom_managers: RwLock<BTreeMap<ManagerId, Arc<dyn super::managers::SecretManager>>>,
    generation: AtomicU64,
    single_flight: Mutex<BTreeMap<ManagerId, Arc<SingleFlightEntry>>>,
    pending_last_used: Mutex<BTreeMap<SecretId, i64>>,
    pending_needs_value: Mutex<BTreeSet<SecretId>>,
}

impl std::fmt::Debug for SecretResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretResolver")
            .field("generation", &self.generation())
            .finish()
    }
}

impl SecretResolver {
    pub fn new(store: Arc<dyn SecretStore>, snapshot: SecretsMetadataSnapshot) -> Self {
        Self {
            store,
            snapshot: RwLock::new(Arc::new(snapshot)),
            custom_managers: RwLock::new(BTreeMap::new()),
            generation: AtomicU64::new(1),
            single_flight: Mutex::new(BTreeMap::new()),
            pending_last_used: Mutex::new(BTreeMap::new()),
            pending_needs_value: Mutex::new(BTreeSet::new()),
        }
    }

    pub fn store(&self) -> Arc<dyn SecretStore> {
        Arc::clone(&self.store)
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Bumps the global monotonic generation counter, invalidating cached secrets on next turn/step.
    pub fn bump_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Replaces the metadata snapshot and bumps `secrets_generation` for instant cache invalidation.
    pub fn update_snapshot(&self, snapshot: SecretsMetadataSnapshot) {
        *self
            .snapshot
            .write()
            .expect("resolver snapshot lock poisoned") = Arc::new(snapshot);
        self.bump_generation();
    }

    pub fn snapshot(&self) -> Arc<SecretsMetadataSnapshot> {
        Arc::clone(
            &self
                .snapshot
                .read()
                .expect("resolver snapshot lock poisoned"),
        )
    }

    /// Registers a custom/mock [`SecretManager`] (primarily for tests).
    pub fn register_custom_manager(&self, manager: Arc<dyn super::managers::SecretManager>) {
        self.custom_managers
            .write()
            .expect("custom managers lock poisoned")
            .insert(manager.id().clone(), manager);
    }

    /// Drains queued `last_used_at` timestamp updates for batched persistence to SQLite.
    pub fn drain_last_used_updates(&self) -> BTreeMap<SecretId, i64> {
        let mut guard = self
            .pending_last_used
            .lock()
            .expect("last_used lock poisoned");
        std::mem::take(&mut *guard)
    }

    /// Drains secret IDs that were `Ready` in SQLite but missing in Keychain, to downgrade to `NeedsValue`.
    pub fn drain_needs_value_downgrades(&self) -> Vec<SecretId> {
        let mut guard = self
            .pending_needs_value
            .lock()
            .expect("needs_value lock poisoned");
        std::mem::take(&mut *guard).into_iter().collect()
    }

    fn record_last_used(&self, id: &str) {
        let now = current_unix_seconds();
        self.pending_last_used
            .lock()
            .expect("last_used lock poisoned")
            .insert(id.to_owned(), now);
    }

    fn record_needs_value_downgrade(&self, id: &str) {
        self.pending_needs_value
            .lock()
            .expect("needs_value lock poisoned")
            .insert(id.to_owned());
    }

    fn build_manager_registry(&self, snap: &SecretsMetadataSnapshot) -> ManagerRegistry {
        let mut reg = ManagerRegistry::from_settings(
            &snap.settings,
            Arc::clone(&self.store),
            &snap.db_instance_id,
        );
        for mgr in self
            .custom_managers
            .read()
            .expect("custom managers lock poisoned")
            .values()
        {
            reg.insert(Arc::clone(mgr));
        }
        reg
    }

    /// Resolves `referenced_names` + (when `include_agent_and_steps`) all active `AgentAndSteps`
    /// secrets and plain vars for `project` in one Keychain pass and one single-flight call per
    /// manager. MUST be called on a background worker thread, never the UI thread.
    pub fn resolve_for_execution(
        &self,
        project: ProjectId,
        referenced_names: &BTreeSet<String>,
        include_agent_and_steps: bool,
        cache: &mut dyn SecretsCache,
        cancel: &CancellationToken,
    ) -> ResolvedSecrets {
        let snap = self.snapshot();
        let current_gen = self.generation();
        let mut out = ResolvedSecrets::default();

        // Always strip Bruk's own tool tokens from child environments.
        for tool_env in [
            "GH_TOKEN",
            "OP_SERVICE_ACCOUNT_TOKEN",
            "OP_CONNECT_HOST",
            "OP_CONNECT_TOKEN",
        ] {
            out.strip_from_child_env.insert(tool_env.to_owned());
        }

        // Index records by lowercase name for fast case-insensitive lookup.
        let mut project_secrets_by_lower: BTreeMap<String, &SecretRecord> = BTreeMap::new();
        let mut app_secrets_by_lower: BTreeMap<String, &SecretRecord> = BTreeMap::new();
        for rec in &snap.records {
            match &rec.kind {
                SecretKind::Env {
                    scope: EnvScope::Project(pid),
                    ..
                } if *pid == project => {
                    project_secrets_by_lower.insert(rec.name.to_ascii_lowercase(), rec);
                }
                SecretKind::Env {
                    scope: EnvScope::App { .. },
                    ..
                } => {
                    app_secrets_by_lower.insert(rec.name.to_ascii_lowercase(), rec);
                }
                SecretKind::Internal { .. } if super::validate_env_name(&rec.name).is_ok() => {
                    out.strip_from_child_env.insert(rec.name.clone());
                }
                _ => {}
            }
        }

        let plain_vars = snap.project_plain_env.get(&project);
        let mut plain_by_lower: BTreeMap<String, (&str, &str)> = BTreeMap::new();
        if let Some(map) = plain_vars {
            for (k, v) in map {
                plain_by_lower.insert(k.to_ascii_lowercase(), (k.as_str(), v.as_str()));
            }
        }
        let excluded_app = snap.project_excluded_app_secrets.get(&project);

        // Build case-insensitive referenced set.
        let referenced_lower: BTreeSet<String> = referenced_names
            .iter()
            .map(|n| n.to_ascii_lowercase())
            .collect();

        // Always populate strip_from_child_env for excluded, unshared, and unreferenced McpOnly secrets.
        for rec in &snap.records {
            match &rec.kind {
                SecretKind::Env {
                    scope: EnvScope::App { shared_with },
                    exposure,
                } => {
                    let lower = rec.name.to_ascii_lowercase();
                    let shadowed = project_secrets_by_lower.contains_key(&lower)
                        || plain_by_lower.contains_key(&lower);
                    let shared = shared_with.includes_project(project)
                        && !excluded_app.is_some_and(|ex| ex.contains(&rec.id));
                    if !shadowed
                        && (!shared
                            || (*exposure == Exposure::McpOnly
                                && !referenced_lower.contains(&lower)))
                    {
                        out.strip_from_child_env.insert(rec.name.clone());
                    }
                }
                SecretKind::Env {
                    scope: EnvScope::Project(pid),
                    exposure,
                } if *pid == project => {
                    let lower = rec.name.to_ascii_lowercase();
                    if *exposure == Exposure::McpOnly && !referenced_lower.contains(&lower) {
                        out.strip_from_child_env.insert(rec.name.clone());
                    }
                }
                _ => {}
            }
        }

        // Collect all names to resolve:
        // 1. All plain project vars (when include_agent_and_steps)
        // 2. All active AgentAndSteps secrets (when include_agent_and_steps)
        // 3. All referenced_names
        let mut target_names: BTreeSet<String> = referenced_names.clone();
        if include_agent_and_steps {
            if let Some(map) = plain_vars {
                for k in map.keys() {
                    target_names.insert(k.clone());
                }
            }
            for rec in snap.visible_env_secrets_for_project(project) {
                if let SecretKind::Env {
                    exposure: Exposure::AgentAndSteps,
                    ..
                } = rec.kind
                {
                    target_names.insert(rec.name.clone());
                }
            }
        }

        // Classify each target name according to 4-level precedence (§4).
        let mut records_to_resolve: Vec<(String, &SecretRecord, Exposure, bool)> = Vec::new();
        for name in target_names {
            let lower = name.to_ascii_lowercase();

            // Level 1: Project P's own scope (SecretRecord wins on tie over plain var).
            if let Some(rec) = project_secrets_by_lower.get(&lower) {
                if plain_by_lower.contains_key(&lower) {
                    log::warn!(
                        "secret.shadowed project_id={} name={:?} winner=SecretRecord",
                        project,
                        rec.name
                    );
                }
                let exposure = match rec.kind {
                    SecretKind::Env { exposure, .. } => exposure,
                    _ => Exposure::McpOnly,
                };
                if exposure == Exposure::McpOnly && !referenced_lower.contains(&lower) {
                    out.strip_from_child_env.insert(rec.name.clone());
                    continue;
                }
                records_to_resolve.push((rec.name.clone(), *rec, exposure, false));
                continue;
            }
            if let Some((canonical, plain_val)) = plain_by_lower.get(&lower) {
                out.values.insert(
                    (*canonical).to_owned(),
                    ResolvedValue {
                        value: SecretString::from((*plain_val).to_owned()),
                        exposure: Exposure::AgentAndSteps,
                        origin: ValueOrigin::PlainProjectVar,
                    },
                );
                continue;
            }

            // Level 2: App Env secrets (with Exclusion barrier blocking Level 3 fallback!).
            if let Some(rec) = app_secrets_by_lower.get(&lower) {
                let (shared_with, exposure) = match &rec.kind {
                    SecretKind::Env {
                        scope: EnvScope::App { shared_with },
                        exposure,
                    } => (shared_with, *exposure),
                    _ => continue,
                };
                let is_excluded = !shared_with.includes_project(project)
                    || excluded_app.is_some_and(|ex| ex.contains(&rec.id));
                if is_excluded {
                    // Exclusion barrier: STOP here, do NOT fall through to Level 3 host env.
                    out.strip_from_child_env.insert(rec.name.clone());
                    out.strip_from_child_env.insert(name);
                    continue;
                }
                if exposure == Exposure::McpOnly && !referenced_lower.contains(&lower) {
                    out.strip_from_child_env.insert(rec.name.clone());
                    continue;
                }
                records_to_resolve.push((rec.name.clone(), *rec, exposure, true));
                continue;
            }

            // Level 3: Host process env, ONLY for undeclared names (never if stripped).
            if !out.strip_from_child_env.contains(&name)
                && let Ok(host_val) = std::env::var(&name)
            {
                out.values.insert(
                    name,
                    ResolvedValue {
                        value: SecretString::from(host_val),
                        exposure: Exposure::AgentAndSteps,
                        origin: ValueOrigin::HostEnv,
                    },
                );
            }
        }

        // Resolve matched SecretRecords in one Keychain pass + one single-flight batch per manager.
        self.resolve_record_batch(
            &snap,
            current_gen,
            &records_to_resolve,
            cache,
            cancel,
            &mut out,
        );

        // Any name that resolved successfully should not also be in strip_from_child_env.
        for resolved_name in out.values.keys() {
            out.strip_from_child_env.remove(resolved_name);
        }

        out
    }

    fn resolve_record_batch(
        &self,
        snap: &SecretsMetadataSnapshot,
        current_gen: u64,
        records: &[(String, &SecretRecord, Exposure, bool)],
        cache: &mut dyn SecretsCache,
        cancel: &CancellationToken,
        out: &mut ResolvedSecrets,
    ) {
        type ManagerBatchItem<'a> = (String, &'a SecretRecord, Exposure, bool, String);
        let mut manager_batches: BTreeMap<ManagerId, Vec<ManagerBatchItem<'_>>> = BTreeMap::new();

        for (canonical_name, rec, exposure, is_app) in records {
            if cancel.is_cancelled() {
                out.failures
                    .insert(canonical_name.clone(), SecretFailure::Cancelled);
                continue;
            }

            // Check execution cache first.
            if let Some(cached) = cache.get(&rec.id, current_gen) {
                self.record_last_used(&rec.id);
                out.values.insert(
                    canonical_name.clone(),
                    ResolvedValue {
                        value: cached,
                        exposure: *exposure,
                        origin: ValueOrigin::Secret {
                            secret_id: rec.id.clone(),
                            is_app_scoped: *is_app,
                        },
                    },
                );
                continue;
            }

            if rec.state == SecretState::NeedsValue {
                out.failures.insert(
                    canonical_name.clone(),
                    SecretFailure::NeedsValue {
                        secret_id: rec.id.clone(),
                        name: rec.name.clone(),
                    },
                );
                continue;
            }

            match &rec.source {
                SecretSource::Keychain => {
                    let account_key = keychain_account_key(&snap.db_instance_id, &rec.id);
                    match self.store.get(&account_key) {
                        Ok(Some(val)) => {
                            cache.insert(rec.id.clone(), val.clone(), current_gen);
                            self.record_last_used(&rec.id);
                            out.values.insert(
                                canonical_name.clone(),
                                ResolvedValue {
                                    value: val,
                                    exposure: *exposure,
                                    origin: ValueOrigin::Secret {
                                        secret_id: rec.id.clone(),
                                        is_app_scoped: *is_app,
                                    },
                                },
                            );
                        }
                        Ok(None) => {
                            // SQLite says Ready but Keychain item is missing -> downgrade to NeedsValue.
                            out.needs_value_downgrades.push(rec.id.clone());
                            self.record_needs_value_downgrade(&rec.id);
                            out.failures.insert(
                                canonical_name.clone(),
                                SecretFailure::NeedsValue {
                                    secret_id: rec.id.clone(),
                                    name: rec.name.clone(),
                                },
                            );
                        }
                        Err(SecretError::Locked) => {
                            out.failures.insert(
                                canonical_name.clone(),
                                SecretFailure::StoreUnavailable {
                                    detail: "OS Keychain is locked".to_owned(),
                                },
                            );
                        }
                        Err(err) => {
                            out.failures.insert(
                                canonical_name.clone(),
                                SecretFailure::StoreUnavailable {
                                    detail: err.to_string(),
                                },
                            );
                        }
                    }
                }
                SecretSource::Manager { manager, reference } => {
                    manager_batches.entry(manager.clone()).or_default().push((
                        canonical_name.clone(),
                        *rec,
                        *exposure,
                        *is_app,
                        reference.clone(),
                    ));
                }
            }
        }

        if manager_batches.is_empty() {
            return;
        }

        let registry = self.build_manager_registry(snap);
        for (manager_id, items) in manager_batches {
            let Some(manager) = registry.get(&manager_id) else {
                let err = SecretFailure::ManagerNotInstalled {
                    manager: manager_id.clone(),
                    binary: snap.settings.manager_display_name(&manager_id),
                };
                for (name, _, _, _, _) in items {
                    out.failures.insert(name, err.clone());
                }
                continue;
            };

            let refs: Vec<String> = items.iter().map(|(_, _, _, _, r)| r.clone()).collect();
            let batch_res =
                self.resolve_manager_single_flight(&manager_id, &manager, &refs, cancel);

            for (canonical_name, rec, exposure, is_app, reference) in items {
                match batch_res.get(&reference) {
                    Some(Ok(val)) => {
                        cache.insert(rec.id.clone(), val.clone(), current_gen);
                        self.record_last_used(&rec.id);
                        out.values.insert(
                            canonical_name,
                            ResolvedValue {
                                value: val.clone(),
                                exposure,
                                origin: ValueOrigin::Secret {
                                    secret_id: rec.id.clone(),
                                    is_app_scoped: is_app,
                                },
                            },
                        );
                    }
                    Some(Err(err)) => {
                        let enriched = enrich_failure_with_record(err.clone(), rec);
                        out.failures.insert(canonical_name, enriched);
                    }
                    None => {
                        out.failures.insert(
                            canonical_name,
                            SecretFailure::ReferenceNotFound {
                                secret_id: rec.id.clone(),
                                name: rec.name.clone(),
                                manager: manager_id.clone(),
                                detail: "manager did not return a value for reference".to_owned(),
                            },
                        );
                    }
                }
            }
        }
    }

    /// Single-flight coalescing per manager so parallel runs starting concurrently do not trigger
    /// duplicate Touch ID prompts or colliding CLI calls.
    fn resolve_manager_single_flight(
        &self,
        manager_id: &str,
        manager: &Arc<dyn super::managers::SecretManager>,
        refs: &[String],
        cancel: &CancellationToken,
    ) -> BTreeMap<String, Result<SecretString, SecretFailure>> {
        let entry = {
            let mut map = self
                .single_flight
                .lock()
                .expect("single_flight lock poisoned");
            Arc::clone(
                map.entry(manager_id.to_owned())
                    .or_insert_with(|| Arc::new(SingleFlightEntry::new())),
            )
        };

        // Wait if another thread is currently invoking this manager.
        let mut guard = entry.in_flight.lock().expect("in_flight lock poisoned");
        while *guard {
            if cancel.is_cancelled() {
                let mut out = BTreeMap::new();
                for r in refs {
                    out.insert(r.clone(), Err(SecretFailure::Cancelled));
                }
                return out;
            }
            let (new_guard, _) = entry
                .cv
                .wait_timeout(guard, Duration::from_millis(50))
                .expect("in_flight condvar poisoned");
            guard = new_guard;
        }

        // Check if the just-completed leader already resolved all the refs we need.
        {
            let recent = entry.recent.lock().expect("recent lock poisoned");
            if let Some((at, map)) = recent.as_ref()
                && at.elapsed() <= SINGLE_FLIGHT_COALESCE_WINDOW
                && refs.iter().all(|r| map.contains_key(r))
            {
                let mut out = BTreeMap::new();
                for r in refs {
                    if let Some(val) = map.get(r) {
                        out.insert(r.clone(), val.clone());
                    }
                }
                return out;
            }
        }

        // Become the leader for this manager.
        *guard = true;
        drop(guard);

        let ref_slices: Vec<&str> = refs.iter().map(String::as_str).collect();
        let resolved = manager.resolve(&ref_slices, cancel);

        // Publish results briefly for any follower waiting on the same refs (e.g. concurrent runs).
        {
            let mut recent = entry.recent.lock().expect("recent lock poisoned");
            *recent = Some((Instant::now(), resolved.clone()));
        }

        // Release leadership and wake waiting threads.
        {
            let mut guard = entry.in_flight.lock().expect("in_flight lock poisoned");
            *guard = false;
            entry.cv.notify_all();
        }

        resolved
    }

    /// Resolves a sealed `Internal` credential by [`SecretId`] (for `gh`, OpenRouter, manager auth,
    /// or Claude/Codex spawn-settings secret env vars).
    pub fn resolve_internal(
        &self,
        id: &SecretId,
        cancel: &CancellationToken,
    ) -> Result<SecretString, SecretFailure> {
        if cancel.is_cancelled() {
            return Err(SecretFailure::Cancelled);
        }
        let snap = self.snapshot();
        let Some(rec) = snap.find_by_id(id) else {
            return Err(SecretFailure::NeedsValue {
                secret_id: id.clone(),
                name: id.clone(),
            });
        };
        if !rec.is_internal() {
            return Err(SecretFailure::StoreUnavailable {
                detail: format!("secret '{}' is not an internal credential", rec.name),
            });
        }
        if rec.state == SecretState::NeedsValue {
            return Err(SecretFailure::NeedsValue {
                secret_id: rec.id.clone(),
                name: rec.name.clone(),
            });
        }

        match &rec.source {
            SecretSource::Keychain => {
                let account_key = keychain_account_key(&snap.db_instance_id, &rec.id);
                match self.store.get(&account_key) {
                    Ok(Some(val)) => {
                        self.record_last_used(&rec.id);
                        Ok(val)
                    }
                    Ok(None) => {
                        self.record_needs_value_downgrade(&rec.id);
                        Err(SecretFailure::NeedsValue {
                            secret_id: rec.id.clone(),
                            name: rec.name.clone(),
                        })
                    }
                    Err(SecretError::Locked) => Err(SecretFailure::StoreUnavailable {
                        detail: "OS Keychain is locked".to_owned(),
                    }),
                    Err(err) => Err(SecretFailure::StoreUnavailable {
                        detail: err.to_string(),
                    }),
                }
            }
            SecretSource::Manager { manager, reference } => {
                let registry = self.build_manager_registry(&snap);
                let Some(mgr) = registry.get(manager) else {
                    return Err(SecretFailure::ManagerNotInstalled {
                        manager: manager.clone(),
                        binary: snap.settings.manager_display_name(manager),
                    });
                };
                let batch = self.resolve_manager_single_flight(
                    manager,
                    &mgr,
                    std::slice::from_ref(reference),
                    cancel,
                );
                match batch.get(reference) {
                    Some(Ok(val)) => {
                        self.record_last_used(&rec.id);
                        Ok(val.clone())
                    }
                    Some(Err(err)) => Err(enrich_failure_with_record(err.clone(), rec)),
                    None => Err(SecretFailure::ReferenceNotFound {
                        secret_id: rec.id.clone(),
                        name: rec.name.clone(),
                        manager: manager.clone(),
                        detail: "manager did not return a value".to_owned(),
                    }),
                }
            }
        }
    }
}

fn enrich_failure_with_record(err: SecretFailure, rec: &SecretRecord) -> SecretFailure {
    match err {
        SecretFailure::ReferenceNotFound {
            manager, detail, ..
        } => SecretFailure::ReferenceNotFound {
            secret_id: rec.id.clone(),
            name: rec.name.clone(),
            manager,
            detail,
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use secrecy::ExposeSecret;

    use super::*;
    use crate::{
        InternalPurpose, Sharing,
        managers::{ManagerStatus, SecretManager},
        store::MemoryStore,
    };

    fn make_env_record(
        id: &str,
        name: &str,
        scope: EnvScope,
        exposure: Exposure,
        state: SecretState,
    ) -> SecretRecord {
        SecretRecord {
            id: id.to_owned(),
            name: name.to_owned(),
            kind: SecretKind::Env { scope, exposure },
            source: SecretSource::Keychain,
            state,
            description: String::new(),
            preview: None,
            created_at: 0,
            updated_at: 0,
            last_used_at: None,
        }
    }

    #[test]
    fn precedence_exclusion_barrier_and_fail_stop() {
        let store = Arc::new(MemoryStore::new());
        store
            .set(
                "db1:app-shared",
                "app",
                &SecretString::from("app_val_123456".to_owned()),
            )
            .unwrap();
        store
            .set(
                "db1:proj-override",
                "proj",
                &SecretString::from("proj_val_123456".to_owned()),
            )
            .unwrap();
        store
            .set(
                "db1:app-excluded",
                "excluded",
                &SecretString::from("excluded_val_1234".to_owned()),
            )
            .unwrap();
        store
            .set(
                "db1:mcp-only-unreferenced",
                "unreferenced",
                &SecretString::from("unreferenced_1234".to_owned()),
            )
            .unwrap();

        let mut excluded = BTreeSet::new();
        excluded.insert("app-excluded".to_owned());

        let mut snap = SecretsMetadataSnapshot {
            db_instance_id: "db1".to_owned(),
            records: vec![
                make_env_record(
                    "app-shared",
                    "SHARED_TOK",
                    EnvScope::App {
                        shared_with: Sharing::AllProjects,
                    },
                    Exposure::McpOnly,
                    SecretState::Ready,
                ),
                make_env_record(
                    "proj-override",
                    "SHARED_TOK",
                    EnvScope::Project(1),
                    Exposure::McpOnly,
                    SecretState::Ready,
                ),
                make_env_record(
                    "app-excluded",
                    "EXCLUDED_TOK",
                    EnvScope::App {
                        shared_with: Sharing::AllProjects,
                    },
                    Exposure::AgentAndSteps,
                    SecretState::Ready,
                ),
                make_env_record(
                    "mcp-only-unreferenced",
                    "UNREF_MCP",
                    EnvScope::Project(1),
                    Exposure::McpOnly,
                    SecretState::Ready,
                ),
                make_env_record(
                    "broken-rec",
                    "BROKEN_TOK",
                    EnvScope::Project(1),
                    Exposure::McpOnly,
                    SecretState::NeedsValue,
                ),
                SecretRecord {
                    id: "int-pat".to_owned(),
                    name: "INTERNAL_SECRET".to_owned(),
                    kind: SecretKind::Internal {
                        purpose: InternalPurpose::GitHubPat,
                    },
                    source: SecretSource::Keychain,
                    state: SecretState::Ready,
                    description: String::new(),
                    preview: None,
                    created_at: 0,
                    updated_at: 0,
                    last_used_at: None,
                },
            ],
            project_plain_env: BTreeMap::new(),
            project_excluded_app_secrets: BTreeMap::from([(1, excluded)]),
            settings: SecretsSettings::default(),
        };
        snap.project_plain_env.insert(
            1,
            BTreeMap::from([("PLAIN_VAR".to_owned(), "hello".to_owned())]),
        );

        let resolver = SecretResolver::new(store, snap);
        let mut cache = RunSecretsCache::new();
        let referenced = BTreeSet::from([
            "SHARED_TOK".to_owned(),
            "EXCLUDED_TOK".to_owned(),
            "BROKEN_TOK".to_owned(),
            "INTERNAL_SECRET".to_owned(),
        ]);

        // Project 1: project secret shadows app secret; excluded is stripped; broken fails-stop;
        // internal is never resolved via Env lookup and is stripped; unreferenced McpOnly is stripped.
        let res1 = resolver.resolve_for_execution(
            1,
            &referenced,
            true,
            &mut cache,
            &CancellationToken::new(),
        );
        assert_eq!(
            res1.values["SHARED_TOK"].value.expose_secret(),
            "proj_val_123456"
        );
        assert_eq!(res1.values["PLAIN_VAR"].value.expose_secret(), "hello");
        assert!(!res1.values.contains_key("EXCLUDED_TOK"));
        assert!(res1.strip_from_child_env.contains("EXCLUDED_TOK"));
        assert!(res1.strip_from_child_env.contains("UNREF_MCP"));
        assert!(res1.strip_from_child_env.contains("INTERNAL_SECRET"));
        assert!(!res1.values.contains_key("INTERNAL_SECRET"));
        assert!(matches!(
            res1.failures.get("BROKEN_TOK"),
            Some(SecretFailure::NeedsValue { .. })
        ));

        // Project 2: receives App SHARED_TOK and App EXCLUDED_TOK (not excluded on project 2).
        let res2 = resolver.resolve_for_execution(
            2,
            &referenced,
            true,
            &mut cache,
            &CancellationToken::new(),
        );
        assert_eq!(
            res2.values["SHARED_TOK"].value.expose_secret(),
            "app_val_123456"
        );
        assert_eq!(
            res2.values["EXCLUDED_TOK"].value.expose_secret(),
            "excluded_val_1234"
        );
    }

    #[test]
    fn approval_gate_cache_clear_and_generation_invalidation() {
        let store = Arc::new(MemoryStore::new());
        store
            .set(
                "db1:s1",
                "s1",
                &SecretString::from("v1_secret_value".to_owned()),
            )
            .unwrap();
        let snap = SecretsMetadataSnapshot {
            db_instance_id: "db1".to_owned(),
            records: vec![make_env_record(
                "s1",
                "API_KEY",
                EnvScope::Project(1),
                Exposure::AgentAndSteps,
                SecretState::Ready,
            )],
            ..Default::default()
        };
        let resolver = SecretResolver::new(Arc::clone(&store) as Arc<dyn SecretStore>, snap);
        let mut run_cache = RunSecretsCache::new();
        let mut chat_cache = ChatSecretsCache::new();

        let res = resolver.resolve_for_execution(
            1,
            &BTreeSet::new(),
            true,
            &mut run_cache,
            &CancellationToken::new(),
        );
        assert_eq!(
            res.values["API_KEY"].value.expose_secret(),
            "v1_secret_value"
        );
        assert!(!run_cache.is_empty());

        // Rotate in store
        store
            .set(
                "db1:s1",
                "s1",
                &SecretString::from("v2_rotated_value".to_owned()),
            )
            .unwrap();

        // Run cache returns cached v1 until cleared on ApprovalGate
        let res_cached = resolver.resolve_for_execution(
            1,
            &BTreeSet::new(),
            true,
            &mut run_cache,
            &CancellationToken::new(),
        );
        assert_eq!(
            res_cached.values["API_KEY"].value.expose_secret(),
            "v1_secret_value"
        );

        run_cache.clear_and_zeroize();
        assert!(run_cache.is_empty());
        let res_fresh = resolver.resolve_for_execution(
            1,
            &BTreeSet::new(),
            true,
            &mut run_cache,
            &CancellationToken::new(),
        );
        assert_eq!(
            res_fresh.values["API_KEY"].value.expose_secret(),
            "v2_rotated_value"
        );

        // Chat cache invalidates immediately on generation bump
        let _ = resolver.resolve_for_execution(
            1,
            &BTreeSet::new(),
            true,
            &mut chat_cache,
            &CancellationToken::new(),
        );
        store
            .set(
                "db1:s1",
                "s1",
                &SecretString::from("v3_chat_rotated".to_owned()),
            )
            .unwrap();
        resolver.bump_generation();
        let res_chat = resolver.resolve_for_execution(
            1,
            &BTreeSet::new(),
            true,
            &mut chat_cache,
            &CancellationToken::new(),
        );
        assert_eq!(
            res_chat.values["API_KEY"].value.expose_secret(),
            "v3_chat_rotated"
        );
    }

    struct CountingManager {
        id: ManagerId,
        calls: Arc<AtomicUsize>,
    }

    impl SecretManager for CountingManager {
        fn id(&self) -> &ManagerId {
            &self.id
        }
        fn status(&self, _: &CancellationToken) -> ManagerStatus {
            ManagerStatus::Ready
        }
        fn resolve(
            &self,
            refs: &[&str],
            _: &CancellationToken,
        ) -> BTreeMap<String, Result<SecretString, SecretFailure>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(80));
            refs.iter()
                .map(|r| {
                    (
                        (*r).to_owned(),
                        Ok(SecretString::from("coalesced_secret_val".to_owned())),
                    )
                })
                .collect()
        }
        fn validate_reference(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn single_flight_coalesces_concurrent_manager_calls() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mgr = Arc::new(CountingManager {
            id: "1password".to_owned(),
            calls: Arc::clone(&calls),
        });
        let snap = SecretsMetadataSnapshot {
            db_instance_id: "db1".to_owned(),
            records: vec![SecretRecord {
                id: "op-1".to_owned(),
                name: "LINEAR_TOKEN".to_owned(),
                kind: SecretKind::Env {
                    scope: EnvScope::Project(1),
                    exposure: Exposure::AgentAndSteps,
                },
                source: SecretSource::Manager {
                    manager: "1password".to_owned(),
                    reference: "op://Eng/Linear/token".to_owned(),
                },
                state: SecretState::Ready,
                description: String::new(),
                preview: None,
                created_at: 0,
                updated_at: 0,
                last_used_at: None,
            }],
            ..Default::default()
        };
        let resolver = Arc::new(SecretResolver::new(Arc::new(MemoryStore::new()), snap));
        resolver.register_custom_manager(mgr);

        let r1 = Arc::clone(&resolver);
        let r2 = Arc::clone(&resolver);
        let h1 = std::thread::spawn(move || {
            let mut c = NoopSecretsCache;
            r1.resolve_for_execution(1, &BTreeSet::new(), true, &mut c, &CancellationToken::new())
        });
        std::thread::sleep(Duration::from_millis(15));
        let h2 = std::thread::spawn(move || {
            let mut c = NoopSecretsCache;
            r2.resolve_for_execution(1, &BTreeSet::new(), true, &mut c, &CancellationToken::new())
        });

        let out1 = h1.join().unwrap();
        let out2 = h2.join().unwrap();
        assert_eq!(
            out1.values["LINEAR_TOKEN"].value.expose_secret(),
            "coalesced_secret_val"
        );
        assert_eq!(
            out2.values["LINEAR_TOKEN"].value.expose_secret(),
            "coalesced_secret_val"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "concurrent runs should coalesce into 1 manager call"
        );
    }
}
