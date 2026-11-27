//! Small cross-cutting utility functions and application version constants.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Current Bruk desktop application version reported by this build.
pub const BRUK_APP_VERSION: &str = "0.9.0";

/// Current Bruk SQLite/domain schema version required by this build.
pub const BRUK_DATA_SCHEMA: u32 = 7;

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

/// Returns whether `path` points to an existing regular executable file.
pub fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && metadata_is_executable(&metadata)
}

/// Returns whether file metadata grants execute permission.
#[cfg(unix)]
pub fn metadata_is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

/// Returns whether file metadata grants execute permission.
#[cfg(not(unix))]
pub fn metadata_is_executable(_metadata: &fs::Metadata) -> bool {
    true
}

/// Clamps a finite floating-point percentage into `0..=100`.
pub fn clamp_percent(raw: f64) -> u8 {
    if !raw.is_finite() {
        return 0;
    }
    raw.round().clamp(0.0, 100.0) as u8
}

/// Maps a usage percentage into a severity string (`"normal"`, `"warning"`, `"critical"`).
pub fn default_severity_for_used(used: u8) -> &'static str {
    if used >= 90 {
        "critical"
    } else if used >= 75 {
        "warning"
    } else {
        "normal"
    }
}

/// Current Unix timestamp in whole seconds.
pub fn current_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Millionths of a dollar to cents, rounded.
pub fn micros_to_cents(micros: u64) -> u64 {
    micros.saturating_add(5_000) / 10_000
}

/// Formats cents as `$200.00`.
pub fn format_dollars_cents(cents: u64) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}

/// Formats cents as `$200` when whole dollars, or `$25.50` otherwise.
pub fn format_whole_or_cents(cents: u64) -> String {
    if cents.is_multiple_of(100) {
        format!("${}", cents / 100)
    } else {
        format_dollars_cents(cents)
    }
}

/// Splits a shell-style argument string respecting single quotes, double quotes, and escapes.
pub fn split_shell_words_raw(input: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            Some('"') => {
                if ch == '"' {
                    quote = None;
                } else if ch == '\\' {
                    if let Some(&next) = chars.peek() {
                        if matches!(next, '"' | '\\' | '$' | '`') {
                            current.push(chars.next().unwrap());
                        } else {
                            current.push('\\');
                        }
                    } else {
                        return Err("trailing backslash inside double quotes".to_owned());
                    }
                } else {
                    current.push(ch);
                }
            }
            None => match ch {
                c if c.is_whitespace() => {
                    if in_word {
                        words.push(std::mem::take(&mut current));
                        in_word = false;
                    }
                }
                '\'' | '"' => {
                    quote = Some(ch);
                    in_word = true;
                }
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        current.push(escaped);
                        in_word = true;
                    } else {
                        return Err("trailing backslash at end of arguments".to_owned());
                    }
                }
                other => {
                    current.push(other);
                    in_word = true;
                }
            },
            Some(_) => unreachable!(),
        }
    }

    if let Some(open) = quote {
        return Err(format!("unclosed quote ({open})"));
    }
    if in_word {
        words.push(current);
    }

    Ok(words)
}

/// Sends `SIGTERM` to a child process on Unix platforms.
pub fn terminate_pid(pid: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("/bin/kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}
