//! Application boot instrumentation routed to macOS Unified Logging (`os_log`)
//! and Instruments (`os_signpost`) under subsystem `dev.frithuz.bruk`, category `boot`.
//!
//! View live boot timings in Terminal with:
//! ```sh
//! log stream --predicate 'subsystem == "dev.frithuz.bruk" && category == "boot"'
//! ```
//! Or inspect the most recent launch with:
//! ```sh
//! log show --predicate 'subsystem == "dev.frithuz.bruk" && category == "boot"' --last 5m
//! ```

use std::{
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crate::system_log::{self, Category, OsLogLevel};

/// macOS Unified Logging subsystem matching `nsh.json`'s bundle identifier.
pub const SUBSYSTEM: &str = system_log::SUBSYSTEM;
/// macOS Unified Logging category for application boot instrumentation.
pub const CATEGORY: &str = Category::Boot.as_str();

struct BootSession {
    origin: Instant,
}

static SESSION: OnceLock<BootSession> = OnceLock::new();
static SYNC_BOOT_ACTIVE: AtomicBool = AtomicBool::new(false);
static MAIN_THREAD_READY_SCHEDULED: AtomicBool = AtomicBool::new(false);

/// Initializes system logging and the boot logger once for the GUI process and emits the
/// `boot.start` event.
///
/// Subsequent calls in the same process are no-ops, so both `main.rs` (`cargo run`),
/// `native()` (`nsh run` / `Bruk.app`), and `App()` can call this safely without duplicating
/// the boot origin. Headless UI tests that mount `AppWithDb` / `AppWithHome` never call
/// `init`, keeping OS logs quiet during `cargo test`.
pub fn init(entrypoint: &'static str) {
    init_with_metadata(entrypoint, env!("CARGO_PKG_VERSION"), 0);
}

/// Initializes system logging and boot instrumentation with explicit version/schema metadata.
pub fn init_with_metadata(entrypoint: &'static str, app_version: &str, data_schema: u32) {
    system_log::init_with_metadata(entrypoint, app_version, data_schema);

    let mut initialized_here = false;
    let session = SESSION.get_or_init(|| {
        initialized_here = true;
        SYNC_BOOT_ACTIVE.store(true, Ordering::Release);
        BootSession {
            origin: Instant::now(),
        }
    });

    if initialized_here {
        let pid = std::process::id();
        let detail = match process_age_ms() {
            Some(age_ms) => format!("entrypoint={entrypoint} pid={pid} proc_age_ms={age_ms:.1}"),
            None => format!("entrypoint={entrypoint} pid={pid}"),
        };
        emit_event(session, OsLogLevel::Default, "boot.start", &detail);
    }
}

/// Returns whether boot instrumentation has been initialized for this process.
pub fn is_initialized() -> bool {
    SESSION.get().is_some()
}

/// Returns whether the synchronous application boot phase is currently active.
pub fn is_booting() -> bool {
    SESSION.get().is_some() && SYNC_BOOT_ACTIVE.load(Ordering::Acquire)
}

/// Ends the synchronous boot phase so later runtime actions (such as toggling Demo Mode or
/// manually refreshing providers in Preferences) do not log into the `boot` category.
pub fn finish_sync_boot(detail: &str) {
    if !SYNC_BOOT_ACTIVE.swap(false, Ordering::AcqRel) {
        return;
    }
    let Some(session) = SESSION.get() else {
        return;
    };
    let combined = match (process_age_ms(), detail.is_empty()) {
        (Some(age_ms), true) => format!("proc_age_ms={age_ms:.1}"),
        (Some(age_ms), false) => format!("proc_age_ms={age_ms:.1} {detail}"),
        (None, _) => detail.to_owned(),
    };
    emit_event(
        session,
        OsLogLevel::Default,
        "boot.sync_complete",
        &combined,
    );
}

/// Schedules a one-shot callback on the main AppKit thread to record when the main run loop
/// becomes responsive after initial mount.
pub fn schedule_main_thread_ready() {
    if !is_initialized() || MAIN_THREAD_READY_SCHEDULED.swap(true, Ordering::AcqRel) {
        return;
    }
    drop(nsh_core::prelude::run_on_main_thread(|| {
        if let Some(session) = SESSION.get() {
            let detail = match process_age_ms() {
                Some(age_ms) => format!("proc_age_ms={age_ms:.1}"),
                None => String::new(),
            };
            emit_event(
                session,
                OsLogLevel::Default,
                "boot.main_thread_ready",
                &detail,
            );
        }
    }));
}

/// Starts a timed boot span if the synchronous boot phase is active.
///
/// The span automatically logs its duration when dropped, or when [`BootSpan::finish`] is called
/// with structured metadata.
pub fn span(phase: &'static str) -> BootSpan {
    if is_booting() {
        start_span(phase)
    } else {
        BootSpan::inert()
    }
}

/// Starts a timed span for an asynchronous follow-up task belonging to the initial boot
/// (even if the synchronous boot phase has already completed).
pub fn async_span(phase: &'static str, from_boot: bool) -> BootSpan {
    if from_boot && is_initialized() {
        start_span(phase)
    } else {
        BootSpan::inert()
    }
}

/// Starts a one-shot timed boot span guarded by `gate`, active whenever [`init`] was called
/// (even for lazy initial UI builds that run right after `App()` returns).
pub fn once_span(phase: &'static str, gate: &'static AtomicBool) -> BootSpan {
    if is_initialized() && !gate.swap(true, Ordering::AcqRel) {
        start_span(phase)
    } else {
        BootSpan::inert()
    }
}

/// Emits a point-in-time boot milestone event while the synchronous boot phase is active.
pub fn event(name: &'static str, detail: &str) {
    if !is_booting() {
        return;
    }
    if let Some(session) = SESSION.get() {
        emit_event(session, OsLogLevel::Default, name, detail);
    }
}

/// Emits a boot error event while the synchronous boot phase is active.
pub fn error(name: &'static str, detail: &str) {
    if !is_booting() {
        return;
    }
    if let Some(session) = SESSION.get() {
        emit_event(session, OsLogLevel::Error, name, detail);
    }
}

/// Returns milliseconds elapsed since kernel process start (`kp_proc.p_starttime`) on macOS.
pub fn process_age_ms() -> Option<f64> {
    system_log::process_age_ms()
}

fn start_span(phase: &'static str) -> BootSpan {
    if SESSION.get().is_none() {
        return BootSpan::inert();
    }
    #[cfg(target_os = "macos")]
    let signpost_id = system_log::handle(Category::Boot)
        .map(|h| h.signpost_begin(phase))
        .unwrap_or(0);
    #[cfg(not(target_os = "macos"))]
    let signpost_id = 0;

    BootSpan {
        active: Some(ActiveSpan {
            phase,
            started: Instant::now(),
            signpost_id,
        }),
    }
}

struct ActiveSpan {
    phase: &'static str,
    started: Instant,
    signpost_id: u64,
}

/// RAII timer for a single application boot phase.
///
/// Safe to move across threads (e.g. into initial background provider refresh tasks).
pub struct BootSpan {
    active: Option<ActiveSpan>,
}

impl BootSpan {
    fn inert() -> Self {
        Self { active: None }
    }

    /// Finishes the span and logs its elapsed duration along with structured `detail`.
    pub fn finish(mut self, detail: impl AsRef<str>) {
        if let Some(active) = self.active.take() {
            complete_span(active, OsLogLevel::Default, detail.as_ref());
        }
    }

    /// Finishes the span with an error log level and structured `detail`.
    pub fn finish_error(mut self, detail: impl AsRef<str>) {
        if let Some(active) = self.active.take() {
            complete_span(active, OsLogLevel::Error, detail.as_ref());
        }
    }
}

impl Drop for BootSpan {
    fn drop(&mut self) {
        if let Some(active) = self.active.take() {
            complete_span(active, OsLogLevel::Default, "");
        }
    }
}

fn complete_span(active: ActiveSpan, level: OsLogLevel, detail: &str) {
    let Some(session) = SESSION.get() else {
        return;
    };
    let duration = active.started.elapsed();
    let since_boot = session.origin.elapsed();
    let message = format_span_message(since_boot, active.phase, duration, level, detail);

    #[cfg(target_os = "macos")]
    if let Some(handle) = system_log::handle(Category::Boot) {
        handle.signpost_end(active.signpost_id, active.phase, detail);
        handle.emit(level, &message);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = active.signpost_id;
    }

    system_log::mirror_boot_to_stderr(level, &message);
}

fn emit_event(session: &BootSession, level: OsLogLevel, name: &str, detail: &str) {
    let since_boot = session.origin.elapsed();
    let message = format_event_message(since_boot, name, level, detail);

    #[cfg(target_os = "macos")]
    if let Some(handle) = system_log::handle(Category::Boot) {
        handle.signpost_event(name, detail);
        handle.emit(level, &message);
    }

    system_log::mirror_boot_to_stderr(level, &message);
}

fn format_span_message(
    since_boot: Duration,
    phase: &str,
    duration: Duration,
    level: OsLogLevel,
    detail: &str,
) -> String {
    let since_ms = since_boot.as_secs_f64() * 1000.0;
    let dur_ms = duration.as_secs_f64() * 1000.0;
    let error_tag = matches!(level, OsLogLevel::Error | OsLogLevel::Fault);
    match (error_tag, detail.is_empty()) {
        (false, true) => format!("+{since_ms:.1}ms [{phase}] {dur_ms:.1}ms"),
        (false, false) => format!("+{since_ms:.1}ms [{phase}] {dur_ms:.1}ms {detail}"),
        (true, true) => format!("+{since_ms:.1}ms [{phase}] ERROR {dur_ms:.1}ms"),
        (true, false) => format!("+{since_ms:.1}ms [{phase}] ERROR {dur_ms:.1}ms {detail}"),
    }
}

fn format_event_message(
    since_boot: Duration,
    name: &str,
    level: OsLogLevel,
    detail: &str,
) -> String {
    let since_ms = since_boot.as_secs_f64() * 1000.0;
    let error_tag = matches!(level, OsLogLevel::Error | OsLogLevel::Fault);
    match (error_tag, detail.is_empty()) {
        (false, true) => format!("+{since_ms:.1}ms [{name}]"),
        (false, false) => format!("+{since_ms:.1}ms [{name}] {detail}"),
        (true, true) => format!("+{since_ms:.1}ms [{name}] ERROR"),
        (true, false) => format!("+{since_ms:.1}ms [{name}] ERROR {detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_span_and_event_messages_consistently() {
        assert_eq!(
            format_span_message(
                Duration::from_millis(125),
                "boot.store.db_open",
                Duration::from_micros(4_250),
                OsLogLevel::Default,
                "ok=true"
            ),
            "+125.0ms [boot.store.db_open] 4.2ms ok=true"
        );
        assert_eq!(
            format_span_message(
                Duration::from_millis(50),
                "boot.store.db_open",
                Duration::from_millis(2),
                OsLogLevel::Error,
                "err=locked"
            ),
            "+50.0ms [boot.store.db_open] ERROR 2.0ms err=locked"
        );
        assert_eq!(
            format_event_message(
                Duration::from_millis(12),
                "boot.start",
                OsLogLevel::Default,
                "entrypoint=cargo_main"
            ),
            "+12.0ms [boot.start] entrypoint=cargo_main"
        );
        assert_eq!(
            format_event_message(
                Duration::from_millis(12),
                "boot.main_thread_ready",
                OsLogLevel::Default,
                ""
            ),
            "+12.0ms [boot.main_thread_ready]"
        );
    }
}
