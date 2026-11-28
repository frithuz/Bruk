//! Build flags: parts of the app that are compiled in but stay unreachable until they are ready.
//!
//! Each [`Flag`] maps to a Cargo feature of this crate. A flag is off unless the build enables
//! its feature (`cargo build -p factory-desktop --features machines`, or `--features unreleased`
//! for all of them), so release packaging, which passes no features, hides everything listed here.
//!
//! Gated code is still compiled and type-checked in every build. Callers check the flag where the
//! gated area becomes reachable (navigation, menus, tab lists) instead of using `#[cfg]`:
//!
//! ```ignore
//! if factory_flags::machines() {
//!     sidebar = sidebar.child(machines_item());
//! }
//! ```
//!
//! To add a flag: add a feature to `Cargo.toml` (and to `unreleased`), a [`Flag`] variant, and
//! forward the feature from `factory-app` and `factory-desktop`.

/// A group of functionality that a build can leave out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flag {
    /// Remote machines: the Machines view, its Preferences tab, and every entry point to them.
    Machines,
}

impl Flag {
    pub const ALL: &'static [Flag] = &[Self::Machines];

    /// The Cargo feature that turns this flag on.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Machines => "machines",
        }
    }

    /// Whether this build was compiled with the flag's Cargo feature.
    pub const fn compiled_in(self) -> bool {
        match self {
            Self::Machines => cfg!(feature = "machines"),
        }
    }

    /// Whether the flagged functionality should be reachable.
    ///
    /// Equal to [`Flag::compiled_in`] in every real build; tests can override it for the whole
    /// process through [`testing`].
    pub fn enabled(self) -> bool {
        #[cfg(feature = "test-override")]
        if let Some(enabled) = testing::overridden(self) {
            return enabled;
        }
        self.compiled_in()
    }
}

/// Whether the Machines functionality is reachable in this build.
pub fn machines() -> bool {
    Flag::Machines.enabled()
}

/// Process-wide flag overrides, so UI tests can cover flagged functionality in default builds.
///
/// A flag can only be forced one way per process: tests that need it on and tests that need it
/// off belong in different test binaries, and every test in a binary forces it the same way.
#[cfg(feature = "test-override")]
pub mod testing {
    use std::sync::atomic::{AtomicU8, Ordering};

    use super::Flag;

    const UNSET: u8 = 0;
    const OFF: u8 = 1;
    const ON: u8 = 2;

    static OVERRIDES: [AtomicU8; Flag::ALL.len()] =
        [const { AtomicU8::new(UNSET) }; Flag::ALL.len()];

    fn slot(flag: Flag) -> &'static AtomicU8 {
        &OVERRIDES[flag as usize]
    }

    pub(super) fn overridden(flag: Flag) -> Option<bool> {
        match slot(flag).load(Ordering::Relaxed) {
            ON => Some(true),
            OFF => Some(false),
            _ => None,
        }
    }

    /// Forces `flag` on or off for the rest of the process.
    ///
    /// # Panics
    ///
    /// If the flag was already forced the other way, since tests sharing a process would then
    /// see whichever call ran last.
    pub fn set(flag: Flag, enabled: bool) {
        let wanted = if enabled { ON } else { OFF };
        let previous = slot(flag)
            .compare_exchange(UNSET, wanted, Ordering::Relaxed, Ordering::Relaxed)
            .unwrap_or_else(|current| current);
        assert!(
            previous == UNSET || previous == wanted,
            "the `{}` flag is already forced {} in this process; put tests that need it {} in \
             another test binary",
            flag.name(),
            if enabled { "off" } else { "on" },
            if enabled { "on" } else { "off" },
        );
    }

    /// Forces `flag` on for the rest of the process.
    pub fn enable(flag: Flag) {
        set(flag, true);
    }

    /// Forces `flag` off for the rest of the process.
    pub fn disable(flag: Flag) {
        set(flag, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_follow_their_cargo_features() {
        assert_eq!(Flag::Machines.compiled_in(), cfg!(feature = "machines"));
        for flag in Flag::ALL {
            assert_eq!(flag.enabled(), flag.compiled_in(), "{}", flag.name());
        }
    }
}
