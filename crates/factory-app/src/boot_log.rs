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

/// macOS Unified Logging subsystem matching `nsh.json`'s bundle identifier.
pub const SUBSYSTEM: &str = "dev.frithuz.bruk";
/// macOS Unified Logging category for application boot instrumentation.
pub const CATEGORY: &str = "boot";

#[derive(Clone, Copy)]
enum LogLevel {
    Default = 0,
    #[allow(dead_code)]
    Info = 1,
    Error = 2,
}

struct BootSession {
    origin: Instant,
    #[cfg(target_os = "macos")]
    handle: macos::OsLogHandle,
}

static SESSION: OnceLock<BootSession> = OnceLock::new();
static SYNC_BOOT_ACTIVE: AtomicBool = AtomicBool::new(false);
static MAIN_THREAD_READY_SCHEDULED: AtomicBool = AtomicBool::new(false);

/// Initializes the boot logger once for the GUI process and emits the `boot.start` event.
///
/// Subsequent calls in the same process are no-ops, so both `main.rs` (`cargo run`),
/// `native()` (`nsh run` / `Bruk.app`), and `App()` can call this safely without duplicating
/// the boot origin. Headless UI tests that mount `AppWithDb` / `AppWithHome` never call
/// `init`, keeping OS logs quiet during `cargo test`.
pub fn init(entrypoint: &'static str) {
    let mut initialized_here = false;
    let session = SESSION.get_or_init(|| {
        initialized_here = true;
        SYNC_BOOT_ACTIVE.store(true, Ordering::Release);
        BootSession {
            origin: Instant::now(),
            #[cfg(target_os = "macos")]
            handle: macos::OsLogHandle::create(SUBSYSTEM, CATEGORY),
        }
    });

    if initialized_here {
        let pid = std::process::id();
        let detail = match process_age_ms() {
            Some(age_ms) => format!("entrypoint={entrypoint} pid={pid} proc_age_ms={age_ms:.1}"),
            None => format!("entrypoint={entrypoint} pid={pid}"),
        };
        emit_event(session, LogLevel::Default, "boot.start", &detail);
    }
}

/// Returns whether boot instrumentation has been initialized for this process.
pub(crate) fn is_initialized() -> bool {
    SESSION.get().is_some()
}

/// Returns whether the synchronous application boot phase is currently active.
pub(crate) fn is_booting() -> bool {
    SESSION.get().is_some() && SYNC_BOOT_ACTIVE.load(Ordering::Acquire)
}

/// Ends the synchronous boot phase so later runtime actions (such as toggling Demo Mode or
/// manually refreshing providers in Preferences) do not log into the `boot` category.
pub(crate) fn finish_sync_boot(detail: &str) {
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
    emit_event(session, LogLevel::Default, "boot.sync_complete", &combined);
}

/// Schedules a one-shot callback on the main AppKit thread to record when the main run loop
/// becomes responsive after initial mount.
pub(crate) fn schedule_main_thread_ready() {
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
                LogLevel::Default,
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
pub(crate) fn async_span(phase: &'static str, from_boot: bool) -> BootSpan {
    if from_boot && is_initialized() {
        start_span(phase)
    } else {
        BootSpan::inert()
    }
}

/// Starts a one-shot timed boot span guarded by `gate`, active whenever [`init`] was called
/// (even for lazy initial UI builds that run right after `App()` returns).
pub(crate) fn once_span(phase: &'static str, gate: &'static AtomicBool) -> BootSpan {
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
        emit_event(session, LogLevel::Default, name, detail);
    }
}

/// Emits a boot error event while the synchronous boot phase is active.
#[allow(dead_code)]
pub(crate) fn error(name: &'static str, detail: &str) {
    if !is_booting() {
        return;
    }
    if let Some(session) = SESSION.get() {
        emit_event(session, LogLevel::Error, name, detail);
    }
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

fn start_span(phase: &'static str) -> BootSpan {
    let Some(session) = SESSION.get() else {
        return BootSpan::inert();
    };
    #[cfg(target_os = "macos")]
    let signpost_id = session.handle.signpost_begin(phase);
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
            complete_span(active, LogLevel::Default, detail.as_ref());
        }
    }

    /// Finishes the span with an error log level and structured `detail`.
    #[allow(dead_code)]
    pub(crate) fn finish_error(mut self, detail: impl AsRef<str>) {
        if let Some(active) = self.active.take() {
            complete_span(active, LogLevel::Error, detail.as_ref());
        }
    }
}

impl Drop for BootSpan {
    fn drop(&mut self) {
        if let Some(active) = self.active.take() {
            complete_span(active, LogLevel::Default, "");
        }
    }
}

fn complete_span(active: ActiveSpan, level: LogLevel, detail: &str) {
    let Some(session) = SESSION.get() else {
        return;
    };
    let duration = active.started.elapsed();
    let since_boot = session.origin.elapsed();
    let message = format_span_message(since_boot, active.phase, duration, level, detail);

    #[cfg(target_os = "macos")]
    {
        session
            .handle
            .signpost_end(active.signpost_id, active.phase, detail);
        session.handle.emit(level, &message);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = active.signpost_id;
    }

    forward_to_rust_log(level, &message);
}

fn emit_event(session: &BootSession, level: LogLevel, name: &str, detail: &str) {
    let since_boot = session.origin.elapsed();
    let message = format_event_message(since_boot, name, level, detail);

    #[cfg(target_os = "macos")]
    {
        session.handle.signpost_event(name, detail);
        session.handle.emit(level, &message);
    }

    forward_to_rust_log(level, &message);
}

fn format_span_message(
    since_boot: Duration,
    phase: &str,
    duration: Duration,
    level: LogLevel,
    detail: &str,
) -> String {
    let since_ms = since_boot.as_secs_f64() * 1000.0;
    let dur_ms = duration.as_secs_f64() * 1000.0;
    let error_tag = matches!(level, LogLevel::Error);
    match (error_tag, detail.is_empty()) {
        (false, true) => format!("+{since_ms:.1}ms [{phase}] {dur_ms:.1}ms"),
        (false, false) => format!("+{since_ms:.1}ms [{phase}] {dur_ms:.1}ms {detail}"),
        (true, true) => format!("+{since_ms:.1}ms [{phase}] ERROR {dur_ms:.1}ms"),
        (true, false) => format!("+{since_ms:.1}ms [{phase}] ERROR {dur_ms:.1}ms {detail}"),
    }
}

fn format_event_message(since_boot: Duration, name: &str, level: LogLevel, detail: &str) -> String {
    let since_ms = since_boot.as_secs_f64() * 1000.0;
    let error_tag = matches!(level, LogLevel::Error);
    match (error_tag, detail.is_empty()) {
        (false, true) => format!("+{since_ms:.1}ms [{name}]"),
        (false, false) => format!("+{since_ms:.1}ms [{name}] {detail}"),
        (true, true) => format!("+{since_ms:.1}ms [{name}] ERROR"),
        (true, false) => format!("+{since_ms:.1}ms [{name}] ERROR {detail}"),
    }
}

fn forward_to_rust_log(level: LogLevel, message: &str) {
    match level {
        LogLevel::Default => log::info!(target: "bruk::boot", "{message}"),
        LogLevel::Info => log::debug!(target: "bruk::boot", "{message}"),
        LogLevel::Error => log::error!(target: "bruk::boot", "{message}"),
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{CString, c_char, c_void};

    use super::LogLevel;

    unsafe extern "C" {
        fn bruk_os_log_create(subsystem: *const c_char, category: *const c_char) -> *mut c_void;
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
    pub(super) struct OsLogHandle(*mut c_void);

    // SAFETY: `os_log_t` handles created by `os_log_create` are thread-safe according to Apple's
    // Unified Logging documentation and remain valid for the lifetime of the process.
    unsafe impl Send for OsLogHandle {}
    unsafe impl Sync for OsLogHandle {}

    impl OsLogHandle {
        pub(super) fn create(subsystem: &str, category: &str) -> Self {
            let sub = sanitize_c_string(subsystem);
            let cat = sanitize_c_string(category);
            // SAFETY: Both pointers are valid NUL-terminated C strings.
            let raw = unsafe { bruk_os_log_create(sub.as_ptr(), cat.as_ptr()) };
            Self(raw)
        }

        pub(super) fn emit(&self, level: LogLevel, message: &str) {
            if self.0.is_null() {
                return;
            }
            let c_msg = sanitize_c_string(message);
            // SAFETY: `self.0` is a valid `os_log_t` handle and `c_msg` is NUL-terminated.
            unsafe { bruk_os_log_emit(self.0, level as u8, c_msg.as_ptr()) };
        }

        pub(super) fn signpost_begin(&self, phase: &str) -> u64 {
            if self.0.is_null() {
                return 0;
            }
            let c_phase = sanitize_c_string(phase);
            // SAFETY: `self.0` is a valid `os_log_t` handle and `c_phase` is NUL-terminated.
            unsafe { bruk_os_signpost_begin(self.0, c_phase.as_ptr()) }
        }

        pub(super) fn signpost_end(&self, spid: u64, phase: &str, detail: &str) {
            if self.0.is_null() || spid == 0 {
                return;
            }
            let c_phase = sanitize_c_string(phase);
            let c_detail = sanitize_c_string(detail);
            // SAFETY: `self.0` is a valid `os_log_t` handle and both strings are NUL-terminated.
            unsafe { bruk_os_signpost_end(self.0, spid, c_phase.as_ptr(), c_detail.as_ptr()) };
        }

        pub(super) fn signpost_event(&self, name: &str, detail: &str) {
            if self.0.is_null() {
                return;
            }
            let c_name = sanitize_c_string(name);
            let c_detail = sanitize_c_string(detail);
            // SAFETY: `self.0` is a valid `os_log_t` handle and both strings are NUL-terminated.
            unsafe { bruk_os_signpost_event(self.0, c_name.as_ptr(), c_detail.as_ptr()) };
        }
    }

    pub(super) fn process_age_ms() -> Option<f64> {
        // SAFETY: `bruk_process_age_ms` only queries the current process's `kinfo_proc` via sysctl.
        let ms = unsafe { bruk_process_age_ms() };
        (ms >= 0.0 && ms.is_finite()).then_some(ms)
    }

    pub(super) fn sanitize_c_string(input: &str) -> CString {
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
    fn formats_span_and_event_messages_consistently() {
        assert_eq!(
            format_span_message(
                Duration::from_millis(125),
                "boot.store.db_open",
                Duration::from_micros(4_250),
                LogLevel::Default,
                "ok=true"
            ),
            "+125.0ms [boot.store.db_open] 4.2ms ok=true"
        );
        assert_eq!(
            format_span_message(
                Duration::from_millis(50),
                "boot.store.db_open",
                Duration::from_millis(2),
                LogLevel::Error,
                "err=locked"
            ),
            "+50.0ms [boot.store.db_open] ERROR 2.0ms err=locked"
        );
        assert_eq!(
            format_event_message(
                Duration::from_millis(12),
                "boot.start",
                LogLevel::Default,
                "entrypoint=cargo_main"
            ),
            "+12.0ms [boot.start] entrypoint=cargo_main"
        );
        assert_eq!(
            format_event_message(
                Duration::from_millis(12),
                "boot.main_thread_ready",
                LogLevel::Default,
                ""
            ),
            "+12.0ms [boot.main_thread_ready]"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_process_age_and_c_string_sanitization_work() {
        let age = process_age_ms();
        assert!(age.is_some_and(|ms| ms >= 0.0));

        let c = macos::sanitize_c_string("hello\0world");
        assert_eq!(c.to_str().unwrap(), "hello world");
    }
}
