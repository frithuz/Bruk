//! Unified macOS system logging (`os_log`) bridge for Bruk under subsystem `dev.frithuz.bruk`.
//!
//! Routes all Rust `log::{error, warn, info, debug, trace}!` records from `factory-core`,
//! `factory-app`, and external crates into categorized Apple Unified Logging streams so live
//! sessions and `.app` launches can be inspected in Console.app or Terminal:
//!
//! ```sh
//! # Stream all Bruk logs (info, warn, error, fault)
//! log stream --predicate 'subsystem == "dev.frithuz.bruk"'
//!
//! # Include verbose debug/trace diagnostics
//! log stream --info --debug --predicate 'subsystem == "dev.frithuz.bruk"'
//!
//! # Filter by category (boot, app, storage, projects, providers, chat, workflow, mcp, secrets, github, work_queue, inbox, machines, preferences, runtime)
//! log stream --predicate 'subsystem == "dev.frithuz.bruk" && category IN {"workflow", "chat", "secrets"}'
//!
//! # Inspect recent logs after a crash or bug
//! log show --predicate 'subsystem == "dev.frithuz.bruk"' --last 10m
//! ```
//!
//! When `RUST_LOG` is set in the environment (for example during `cargo run`), matching records
//! are also echoed to `stderr` via `env_logger`. Headless tests never call [`init`], keeping
//! the system log quiet during `cargo test`.

use std::sync::OnceLock;

use log::{Level, LevelFilter, Log, Metadata, Record};

/// macOS Unified Logging subsystem matching `nsh.json`'s bundle identifier.
pub const SUBSYSTEM: &str = "dev.frithuz.bruk";

/// Maximum UTF-8 byte length sent in a single `os_log` message (macOS silently truncates near 1 KB).
const MAX_OS_LOG_BYTES: usize = 960;
const TRUNCATED_SUFFIX: &str = "… [truncated]";

/// Native macOS `os_log_type_t` levels matching `macos_os_log.c`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum OsLogLevel {
    Debug = 0,
    Info = 1,
    Default = 2,
    Error = 3,
    Fault = 4,
}

/// Structured macOS Unified Logging categories under [`SUBSYSTEM`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Boot,
    App,
    Storage,
    Projects,
    Providers,
    Chat,
    Workflow,
    Mcp,
    Secrets,
    GitHub,
    WorkQueue,
    Inbox,
    Machines,
    Preferences,
    Runtime,
}

impl Category {
    /// Returns the category name reported to macOS Unified Logging.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::App => "app",
            Self::Storage => "storage",
            Self::Projects => "projects",
            Self::Providers => "providers",
            Self::Chat => "chat",
            Self::Workflow => "workflow",
            Self::Mcp => "mcp",
            Self::Secrets => "secrets",
            Self::GitHub => "github",
            Self::WorkQueue => "work_queue",
            Self::Inbox => "inbox",
            Self::Machines => "machines",
            Self::Preferences => "preferences",
            Self::Runtime => "runtime",
        }
    }
}

struct CategoryHandles {
    #[cfg(target_os = "macos")]
    boot: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    app: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    storage: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    projects: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    providers: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    chat: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    workflow: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    mcp: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    secrets: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    github: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    work_queue: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    inbox: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    machines: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    preferences: macos::OsLogHandle,
    #[cfg(target_os = "macos")]
    runtime: macos::OsLogHandle,
}

impl CategoryHandles {
    fn create() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self {
                boot: macos::OsLogHandle::create(SUBSYSTEM, Category::Boot.as_str()),
                app: macos::OsLogHandle::create(SUBSYSTEM, Category::App.as_str()),
                storage: macos::OsLogHandle::create(SUBSYSTEM, Category::Storage.as_str()),
                projects: macos::OsLogHandle::create(SUBSYSTEM, Category::Projects.as_str()),
                providers: macos::OsLogHandle::create(SUBSYSTEM, Category::Providers.as_str()),
                chat: macos::OsLogHandle::create(SUBSYSTEM, Category::Chat.as_str()),
                workflow: macos::OsLogHandle::create(SUBSYSTEM, Category::Workflow.as_str()),
                mcp: macos::OsLogHandle::create(SUBSYSTEM, Category::Mcp.as_str()),
                secrets: macos::OsLogHandle::create(SUBSYSTEM, Category::Secrets.as_str()),
                github: macos::OsLogHandle::create(SUBSYSTEM, Category::GitHub.as_str()),
                work_queue: macos::OsLogHandle::create(SUBSYSTEM, Category::WorkQueue.as_str()),
                inbox: macos::OsLogHandle::create(SUBSYSTEM, Category::Inbox.as_str()),
                machines: macos::OsLogHandle::create(SUBSYSTEM, Category::Machines.as_str()),
                preferences: macos::OsLogHandle::create(SUBSYSTEM, Category::Preferences.as_str()),
                runtime: macos::OsLogHandle::create(SUBSYSTEM, Category::Runtime.as_str()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self {}
        }
    }

    #[cfg(target_os = "macos")]
    fn get(&self, category: Category) -> &macos::OsLogHandle {
        match category {
            Category::Boot => &self.boot,
            Category::App => &self.app,
            Category::Storage => &self.storage,
            Category::Projects => &self.projects,
            Category::Providers => &self.providers,
            Category::Chat => &self.chat,
            Category::Workflow => &self.workflow,
            Category::Mcp => &self.mcp,
            Category::Secrets => &self.secrets,
            Category::GitHub => &self.github,
            Category::WorkQueue => &self.work_queue,
            Category::Inbox => &self.inbox,
            Category::Machines => &self.machines,
            Category::Preferences => &self.preferences,
            Category::Runtime => &self.runtime,
        }
    }
}

struct SystemLogger {
    handles: CategoryHandles,
    stderr: env_logger::Logger,
    stderr_enabled: bool,
}

static LOGGER: OnceLock<SystemLogger> = OnceLock::new();
static PANIC_HOOK_INSTALLED: OnceLock<()> = OnceLock::new();

/// Initializes the global macOS Unified Logging bridge and panic fault hook once per process.
///
/// Safe to call from multiple entrypoints (`main.rs`, `native()`, `App()`); subsequent calls are
/// no-ops. Headless tests do not call `init`, keeping OS logs silent during `cargo test`.
pub fn init(entrypoint: &'static str) {
    init_with_metadata(entrypoint, env!("CARGO_PKG_VERSION"), 0);
}

/// Initializes the global macOS Unified Logging bridge with explicit application version and
/// data schema metadata.
pub fn init_with_metadata(entrypoint: &'static str, app_version: &str, data_schema: u32) {
    let mut initialized_here = false;
    let logger = LOGGER.get_or_init(|| {
        initialized_here = true;
        let stderr_enabled = std::env::var_os("RUST_LOG").is_some_and(|v| !v.is_empty());
        let stderr = env_logger::Builder::from_env(env_logger::Env::default()).build();
        SystemLogger {
            handles: CategoryHandles::create(),
            stderr,
            stderr_enabled,
        }
    });

    if initialized_here {
        if log::set_logger(logger).is_ok() {
            log::set_max_level(LevelFilter::Trace);
        }
        install_panic_hook();
        log::info!(
            target: "bruk::app",
            "system_log.init entrypoint={entrypoint} pid={} version={} schema={} stderr_mirror={}",
            std::process::id(),
            app_version,
            data_schema,
            logger.stderr_enabled,
        );
    }
}

/// Returns the shared `os_log_t` handle for `category`, if [`init`] has been called.
#[cfg(target_os = "macos")]
pub(crate) fn handle(category: Category) -> Option<&'static macos::OsLogHandle> {
    LOGGER.get().map(|logger| logger.handles.get(category))
}

/// Forwards a pre-formatted `boot_log` message to `stderr` when `RUST_LOG` is configured,
/// without re-emitting it to `os_log`.
pub(crate) fn mirror_boot_to_stderr(level: OsLogLevel, message: &str) {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    if !logger.stderr_enabled {
        return;
    }
    let log_level = match level {
        OsLogLevel::Fault | OsLogLevel::Error => Level::Error,
        OsLogLevel::Default => Level::Info,
        OsLogLevel::Info => Level::Debug,
        OsLogLevel::Debug => Level::Trace,
    };
    let emit = |args: std::fmt::Arguments<'_>| {
        let record = Record::builder()
            .args(args)
            .level(log_level)
            .target("bruk::boot")
            .build();
        if logger.stderr.matches(&record) {
            logger.stderr.log(&record);
        }
    };
    emit(format_args!("{message}"));
}

/// Returns milliseconds elapsed since kernel process start (`kp_proc.p_starttime`) on macOS.
pub(crate) fn process_age_ms() -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        macos::process_age_ms()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

fn install_panic_hook() {
    PANIC_HOOK_INSTALLED.get_or_init(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let thread = std::thread::current();
            let thread_name = thread.name().unwrap_or("unnamed");
            let location = info
                .location()
                .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
                .unwrap_or_else(|| "unknown".to_owned());
            let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_owned()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "non-string panic payload".to_owned()
            };
            let message = sanitize_and_truncate(&format!(
                "PANIC thread={thread_name} location={location} message={payload}"
            ));
            #[cfg(target_os = "macos")]
            if let Some(logger) = LOGGER.get() {
                logger
                    .handles
                    .get(Category::App)
                    .emit(OsLogLevel::Fault, &message);
            }
            let _ = &message;
            previous(info);
        }));
    });
}

impl Log for SystemLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        if self.stderr_enabled && self.stderr.enabled(metadata) {
            return true;
        }
        let (category, first_party) = classify_target(metadata.target());
        if !should_emit_to_os_log(first_party, metadata.level()) {
            return false;
        }
        #[cfg(target_os = "macos")]
        {
            self.handles
                .get(category)
                .is_enabled(map_rust_level(metadata.level()))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = category;
            false
        }
    }

    fn log(&self, record: &Record<'_>) {
        if self.stderr_enabled && self.stderr.matches(record) {
            self.stderr.log(record);
        }

        let (category, first_party) = classify_target(record.target());
        if !should_emit_to_os_log(first_party, record.level()) {
            return;
        }

        #[cfg(target_os = "macos")]
        {
            let os_level = map_rust_level(record.level());
            let handle = self.handles.get(category);
            if !handle.is_enabled(os_level) {
                return;
            }
            let formatted = format_record(record, category, first_party);
            handle.emit(os_level, &formatted);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (category, first_party);
        }
    }

    fn flush(&self) {
        self.stderr.flush();
    }
}

/// Maps Rust `log::Level` onto Apple Unified Logging levels.
///
/// `Info` and `Warn` map to `OS_LOG_TYPE_DEFAULT` so standard operational events and warnings are
/// persisted and visible in `log stream --predicate 'subsystem == "dev.frithuz.bruk"'` without
/// extra flags. `Debug` maps to `OS_LOG_TYPE_INFO` (`--info`) and `Trace` maps to
/// `OS_LOG_TYPE_DEBUG` (`--debug`).
pub(crate) fn map_rust_level(level: Level) -> OsLogLevel {
    match level {
        Level::Error => OsLogLevel::Error,
        Level::Warn | Level::Info => OsLogLevel::Default,
        Level::Debug => OsLogLevel::Info,
        Level::Trace => OsLogLevel::Debug,
    }
}

/// Returns whether a record with `first_party` ownership and `level` should be sent to `os_log`.
///
/// Third-party crates only emit `Warn` and `Error` to `os_log` to keep Console.app focused on
/// Bruk's own events while still capturing framework failures.
pub(crate) fn should_emit_to_os_log(first_party: bool, level: Level) -> bool {
    first_party || level <= Level::Warn
}

/// Routes a Rust `log` target string to `(Category, is_first_party)`.
pub(crate) fn classify_target(target: &str) -> (Category, bool) {
    if let Some(rest) = target.strip_prefix("bruk::") {
        let head = rest.split("::").next().unwrap_or("");
        let cat = match head {
            "boot" => Category::Boot,
            "app" => Category::App,
            "storage" => Category::Storage,
            "projects" => Category::Projects,
            "providers" => Category::Providers,
            "chat" => Category::Chat,
            "workflow" => Category::Workflow,
            "mcp" => Category::Mcp,
            "secrets" => Category::Secrets,
            "github" => Category::GitHub,
            "work_queue" => Category::WorkQueue,
            "inbox" => Category::Inbox,
            "machines" => Category::Machines,
            "preferences" => Category::Preferences,
            _ => Category::App,
        };
        return (cat, true);
    }

    if let Some((crate_name, module)) = split_factory_target(target) {
        let cat = match crate_name {
            "factory_core" => match module {
                "storage" | "bruk_home" => Category::Storage,
                "definitions" | "project_settings" => Category::Projects,
                "claude_cli" | "codex_cli" | "antigravity_cli" | "claude_settings"
                | "codex_settings" => Category::Providers,
                "chat" | "chat_attachment" | "chat_git" => Category::Chat,
                "runner" | "engine" | "step_context" | "workflow_editor" | "workflow_run"
                | "workflows_overview" => Category::Workflow,
                "mcp" => Category::Mcp,
                "secrets" => Category::Secrets,
                "github_intake" => Category::GitHub,
                "work_queue" => Category::WorkQueue,
                "inbox" => Category::Inbox,
                "machines" => Category::Machines,
                "preferences" => Category::Preferences,
                _ => Category::App,
            },
            "factory_storage" => Category::Storage,
            "factory_providers" => Category::Providers,
            "factory_chat" => Category::Chat,
            "factory_workflows" => match module {
                "definitions" => Category::Projects,
                _ => Category::Workflow,
            },
            "factory_runner" => Category::Workflow,
            "factory_secrets" => Category::Secrets,
            "factory_github" => match module {
                "work_queue" => Category::WorkQueue,
                _ => Category::GitHub,
            },
            "factory_machines" => Category::Machines,
            "factory_config" => match module {
                "mcp" => Category::Mcp,
                "inbox" => Category::Inbox,
                "preferences" => Category::Preferences,
                "project_settings" => Category::Projects,
                _ => Category::App,
            },
            "factory_logging" => match module {
                "boot_log" => Category::Boot,
                _ => Category::App,
            },
            "factory_app" => match module {
                "boot_log" => Category::Boot,
                _ => Category::App,
            },
            _ => Category::App,
        };
        return (cat, true);
    }

    (Category::Runtime, false)
}

fn split_factory_target(target: &str) -> Option<(&str, &str)> {
    let mut parts = target.split("::");
    let crate_name = parts.next()?;
    let is_first_party = matches!(
        crate_name,
        "factory_core"
            | "factory_types"
            | "factory_machines"
            | "factory_github"
            | "factory_secrets"
            | "factory_providers"
            | "factory_workflows"
            | "factory_config"
            | "factory_storage"
            | "factory_chat"
            | "factory_runner"
            | "factory_logging"
            | "factory_updater"
            | "factory_fonts"
            | "factory_ui_kit"
            | "factory_store"
            | "factory_ui_preferences"
            | "factory_ui_workflows"
            | "factory_ui_chat"
            | "factory_ui_ops"
            | "factory_app"
            | "factory_desktop"
    );
    if !is_first_party {
        return None;
    }
    let module = parts.next().unwrap_or("");
    Some((crate_name, module))
}

fn format_record(record: &Record<'_>, category: Category, first_party: bool) -> String {
    let raw_body = format!("{}", record.args());
    let mut prefix = String::new();

    if record.level() == Level::Warn {
        prefix.push_str("WARN ");
    }

    if !first_party {
        prefix.push('[');
        prefix.push_str(record.target());
        prefix.push_str("] ");
    } else if let Some(sub) = concise_submodule(record.target(), category) {
        prefix.push('[');
        prefix.push_str(sub);
        prefix.push_str("] ");
    }

    if prefix.is_empty() {
        sanitize_and_truncate(&raw_body)
    } else {
        prefix.push_str(&raw_body);
        sanitize_and_truncate(&prefix)
    }
}

/// Extracts a concise submodule label when the target adds specificity beyond `category`.
fn concise_submodule(target: &str, category: Category) -> Option<&str> {
    let (crate_name, _) = split_factory_target(target)?;
    let rest = target.strip_prefix(crate_name)?.strip_prefix("::")?;
    let tail = rest.rsplit("::").next().unwrap_or(rest);
    if tail != category.as_str() && !(crate_name == "factory_app" && tail == "store") {
        return Some(tail);
    }
    None
}

/// Collapses control characters / newlines into single-line text and truncates safely at a UTF-8
/// boundary so messages fit cleanly inside macOS `os_log` buffers.
pub(crate) fn sanitize_and_truncate(input: &str) -> String {
    let mut cleaned = String::with_capacity(input.len().min(MAX_OS_LOG_BYTES + 32));
    let mut last_was_space = false;
    for ch in input.chars() {
        let normalized = match ch {
            '\0' | '\r' | '\n' | '\t' => ' ',
            c if c.is_control() => ' ',
            c => c,
        };
        if normalized == ' ' {
            if !last_was_space {
                cleaned.push(' ');
                last_was_space = true;
            }
        } else {
            cleaned.push(normalized);
            last_was_space = false;
        }
    }
    let trimmed = cleaned.trim();
    if trimmed.len() <= MAX_OS_LOG_BYTES {
        return trimmed.to_owned();
    }
    let budget = MAX_OS_LOG_BYTES.saturating_sub(TRUNCATED_SUFFIX.len());
    let mut cut = budget.min(trimmed.len());
    while cut > 0 && !trimmed.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + TRUNCATED_SUFFIX.len());
    out.push_str(&trimmed[..cut]);
    out.push_str(TRUNCATED_SUFFIX);
    out
}

#[cfg(target_os = "macos")]
pub(crate) mod macos {
    use std::ffi::{CString, c_char, c_void};

    use super::OsLogLevel;

    unsafe extern "C" {
        fn bruk_os_log_create(subsystem: *const c_char, category: *const c_char) -> *mut c_void;
        fn bruk_os_log_is_enabled(log: *mut c_void, level: u8) -> bool;
        fn bruk_os_log_emit(log: *mut c_void, level: u8, message: *const c_char);
        fn bruk_os_signpost_begin(log: *mut c_void, phase: *const c_char) -> u64;
        fn bruk_os_signpost_end(
            log: *mut c_void,
            spid: u64,
            phase: *const c_char,
            detail: *const c_char,
        );
        fn bruk_os_signpost_event(log: *mut c_void, name: *const c_char, detail: *const c_char);
        fn bruk_process_age_ms() -> f64;
    }

    /// Process-lifetime handle to an Apple `os_log_t` object.
    pub(crate) struct OsLogHandle(*mut c_void);

    // SAFETY: `os_log_t` handles created by `os_log_create` are thread-safe according to Apple's
    // Unified Logging documentation and remain valid for the lifetime of the process.
    unsafe impl Send for OsLogHandle {}
    unsafe impl Sync for OsLogHandle {}

    impl OsLogHandle {
        pub(crate) fn create(subsystem: &str, category: &str) -> Self {
            let sub = sanitize_c_string(subsystem);
            let cat = sanitize_c_string(category);
            // SAFETY: Both pointers are valid NUL-terminated C strings.
            let raw = unsafe { bruk_os_log_create(sub.as_ptr(), cat.as_ptr()) };
            Self(raw)
        }

        pub(crate) fn is_enabled(&self, level: OsLogLevel) -> bool {
            if self.0.is_null() {
                return false;
            }
            // SAFETY: `self.0` is a valid `os_log_t` handle.
            unsafe { bruk_os_log_is_enabled(self.0, level as u8) }
        }

        pub(crate) fn emit(&self, level: OsLogLevel, message: &str) {
            if self.0.is_null() {
                return;
            }
            let c_msg = sanitize_c_string(message);
            // SAFETY: `self.0` is a valid `os_log_t` handle and `c_msg` is NUL-terminated.
            unsafe { bruk_os_log_emit(self.0, level as u8, c_msg.as_ptr()) };
        }

        pub(crate) fn signpost_begin(&self, phase: &str) -> u64 {
            if self.0.is_null() {
                return 0;
            }
            let c_phase = sanitize_c_string(phase);
            // SAFETY: `self.0` is a valid `os_log_t` handle and `c_phase` is NUL-terminated.
            unsafe { bruk_os_signpost_begin(self.0, c_phase.as_ptr()) }
        }

        pub(crate) fn signpost_end(&self, spid: u64, phase: &str, detail: &str) {
            if self.0.is_null() || spid == 0 {
                return;
            }
            let c_phase = sanitize_c_string(phase);
            let c_detail = sanitize_c_string(detail);
            // SAFETY: `self.0` is a valid `os_log_t` handle and both strings are NUL-terminated.
            unsafe { bruk_os_signpost_end(self.0, spid, c_phase.as_ptr(), c_detail.as_ptr()) };
        }

        pub(crate) fn signpost_event(&self, name: &str, detail: &str) {
            if self.0.is_null() {
                return;
            }
            let c_name = sanitize_c_string(name);
            let c_detail = sanitize_c_string(detail);
            // SAFETY: `self.0` is a valid `os_log_t` handle and both strings are NUL-terminated.
            unsafe { bruk_os_signpost_event(self.0, c_name.as_ptr(), c_detail.as_ptr()) };
        }
    }

    pub(crate) fn process_age_ms() -> Option<f64> {
        // SAFETY: `bruk_process_age_ms` only queries the current process's `kinfo_proc` via sysctl.
        let ms = unsafe { bruk_process_age_ms() };
        (ms >= 0.0 && ms.is_finite()).then_some(ms)
    }

    pub(crate) fn sanitize_c_string(input: &str) -> CString {
        match CString::new(input) {
            Ok(c) => c,
            Err(_) => {
                let sanitized: Vec<u8> = input
                    .bytes()
                    .map(|b| if b == 0 { b' ' } else { b })
                    .collect();
                CString::new(sanitized).expect("NUL bytes already replaced")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_first_party_and_external_targets() {
        assert_eq!(classify_target("bruk::boot"), (Category::Boot, true));
        assert_eq!(
            classify_target("bruk::workflow"),
            (Category::Workflow, true)
        );
        assert_eq!(
            classify_target("factory_core::secrets::resolver"),
            (Category::Secrets, true)
        );
        assert_eq!(
            classify_target("factory_core::claude_cli"),
            (Category::Providers, true)
        );
        assert_eq!(
            classify_target("factory_core::github_intake"),
            (Category::GitHub, true)
        );
        assert_eq!(
            classify_target("factory_core::storage"),
            (Category::Storage, true)
        );
        assert_eq!(
            classify_target("factory_core::bruk_home"),
            (Category::Storage, true)
        );
        assert_eq!(
            classify_target("factory_core::chat"),
            (Category::Chat, true)
        );
        assert_eq!(
            classify_target("factory_core::runner"),
            (Category::Workflow, true)
        );
        assert_eq!(classify_target("factory_app::store"), (Category::App, true));
        assert_eq!(
            classify_target("nsh_platform_macos::window"),
            (Category::Runtime, false)
        );
    }

    #[test]
    fn maps_rust_levels_and_filters_external_noise() {
        assert_eq!(map_rust_level(Level::Error), OsLogLevel::Error);
        assert_eq!(map_rust_level(Level::Warn), OsLogLevel::Default);
        assert_eq!(map_rust_level(Level::Info), OsLogLevel::Default);
        assert_eq!(map_rust_level(Level::Debug), OsLogLevel::Info);
        assert_eq!(map_rust_level(Level::Trace), OsLogLevel::Debug);

        assert!(should_emit_to_os_log(true, Level::Trace));
        assert!(should_emit_to_os_log(false, Level::Error));
        assert!(should_emit_to_os_log(false, Level::Warn));
        assert!(!should_emit_to_os_log(false, Level::Info));
        assert!(!should_emit_to_os_log(false, Level::Debug));
    }

    #[test]
    fn sanitizes_newlines_and_truncates_utf8_safely() {
        assert_eq!(
            sanitize_and_truncate("line 1\r\n  line 2\0\tline 3"),
            "line 1 line 2 line 3"
        );

        let long = "é".repeat(600); // 1200 bytes > 960
        let truncated = sanitize_and_truncate(&long);
        assert!(truncated.len() <= MAX_OS_LOG_BYTES);
        assert!(truncated.ends_with(TRUNCATED_SUFFIX));
    }

    #[test]
    fn formats_record_prefixes_concisely() {
        let warn_rec = Record::builder()
            .args(format_args!("disk nearly full"))
            .level(Level::Warn)
            .target("bruk::storage")
            .build();
        assert_eq!(
            format_record(&warn_rec, Category::Storage, true),
            "WARN disk nearly full"
        );

        let sub_rec = Record::builder()
            .args(format_args!("keychain locked"))
            .level(Level::Error)
            .target("factory_core::secrets::store")
            .build();
        assert_eq!(
            format_record(&sub_rec, Category::Secrets, true),
            "[store] keychain locked"
        );

        let ext_rec = Record::builder()
            .args(format_args!("surface resized"))
            .level(Level::Warn)
            .target("nsh_platform_macos::window")
            .build();
        assert_eq!(
            format_record(&ext_rec, Category::Runtime, false),
            "WARN [nsh_platform_macos::window] surface resized"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_c_string_sanitization_and_process_age_work() {
        let age = process_age_ms();
        assert!(age.is_some_and(|ms| ms >= 0.0));
        let c = macos::sanitize_c_string("hello\0world");
        assert_eq!(c.to_str().unwrap(), "hello world");
    }
}
