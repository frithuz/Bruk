//! Structured and streaming secret redaction using longest-first Aho-Corasick matching.
//!
//! Redaction NEVER runs raw string replacement on unparsed JSON streams; it operates on
//! parsed text fields, JSON string leaves, diffs, errors, and rolling streaming buffers.

use std::sync::Arc;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use base64::Engine as _;
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

use factory_types::DiffFileChange;

/// Types whose user-facing fields can be scrubbed in place by a [`SecretRedactor`].
pub trait Redactable {
    fn redact_with(&mut self, redactor: &SecretRedactor);
}

/// Minimum character length for a secret value to be registered for redaction (§7).
pub const MIN_REDACT_CHARS: usize = 8;

/// Multi-pattern longest-first secret redactor built per execution from resolved secret values.
#[derive(Debug, Clone)]
pub struct SecretRedactor {
    automaton: Option<AhoCorasick>,
    replacements: Vec<String>,
    max_pattern_len: usize,
}

impl Default for SecretRedactor {
    fn default() -> Self {
        Self::empty()
    }
}

impl SecretRedactor {
    /// An empty no-op redactor.
    pub fn empty() -> Self {
        Self {
            automaton: None,
            replacements: Vec::new(),
            max_pattern_len: 0,
        }
    }

    /// Builds a redactor from `(placeholder_label, secret)` pairs.
    ///
    /// Pass the Env secret's name (e.g. `"LINEAR_TOKEN"`) for Env secrets or `"internal"` for
    /// sealed Internal credentials. Values shorter than 8 characters are skipped.
    pub fn from_secrets<'a>(
        secrets: impl IntoIterator<Item = (&'a str, &'a SecretString)>,
    ) -> Self {
        let mut entries: Vec<(String, String)> = Vec::new();
        for (label, secret) in secrets {
            let raw = secret.expose_secret();
            if raw.chars().count() < MIN_REDACT_CHARS {
                continue;
            }
            let replacement = format!("[secret:{label}]");
            for variant in secret_variants(raw) {
                if variant.chars().count() >= MIN_REDACT_CHARS {
                    entries.push((variant, replacement.clone()));
                }
            }
        }
        Self::from_pattern_pairs(entries)
    }

    /// Builds a redactor from raw `(placeholder_label, value)` pairs (used in migration Step 6).
    pub fn from_raw_pairs(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut entries: Vec<(String, String)> = Vec::new();
        for (label, raw) in pairs {
            if raw.chars().count() < MIN_REDACT_CHARS {
                continue;
            }
            let replacement = format!("[secret:{label}]");
            for variant in secret_variants(&raw) {
                if variant.chars().count() >= MIN_REDACT_CHARS {
                    entries.push((variant, replacement.clone()));
                }
            }
        }
        Self::from_pattern_pairs(entries)
    }

    fn from_pattern_pairs(mut entries: Vec<(String, String)>) -> Self {
        if entries.is_empty() {
            return Self::empty();
        }
        // Sort longest-first and deduplicate so longer secrets always win over substrings.
        entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        entries.dedup_by(|a, b| a.0 == b.0);

        let max_pattern_len = entries.iter().map(|(p, _)| p.len()).max().unwrap_or(0);
        let patterns: Vec<&str> = entries.iter().map(|(p, _)| p.as_str()).collect();
        let replacements: Vec<String> = entries.iter().map(|(_, r)| r.clone()).collect();

        let automaton = AhoCorasickBuilder::new()
            .match_kind(MatchKind::LeftmostLongest)
            .build(patterns)
            .ok();

        Self {
            automaton,
            replacements,
            max_pattern_len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.automaton.is_none()
    }

    pub fn max_pattern_len(&self) -> usize {
        self.max_pattern_len
    }

    /// Redacts all registered secret variants in `text`.
    pub fn redact(&self, text: &str) -> String {
        let Some(ac) = &self.automaton else {
            return text.to_owned();
        };
        if text.is_empty() {
            return String::new();
        }
        let mut out = String::with_capacity(text.len());
        ac.replace_all_with(text, &mut out, |mat, _, dst| {
            dst.push_str(&self.replacements[mat.pattern().as_usize()]);
            true
        });
        out
    }

    /// Recursively scrubs every `Value::String` leaf in a parsed JSON value without corrupting
    /// JSON syntax or object keys.
    pub fn redact_json_value(&self, value: &mut Value) {
        if self.is_empty() {
            return;
        }
        match value {
            Value::String(s) => {
                let redacted = self.redact(s);
                if redacted != *s {
                    *s = redacted;
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.redact_json_value(item);
                }
            }
            Value::Object(map) => {
                for (key, val) in map.iter_mut() {
                    // Never rewrite internal serde discriminator tags or enum identifiers if a
                    // weak secret happens to collide with them.
                    if matches!(
                        key.as_str(),
                        "type" | "kind" | "status" | "provider" | "role"
                    ) && val.is_string()
                    {
                        continue;
                    }
                    self.redact_json_value(val);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }

    /// Redacts all diff headers and lines in place.
    pub fn redact_diffs(&self, diffs: &mut [DiffFileChange]) {
        if self.is_empty() {
            return;
        }
        for diff in diffs {
            diff.path = self.redact(&diff.path);
            diff.hunk_header = self.redact(&diff.hunk_header);
            for line in &mut diff.lines {
                line.text = self.redact(&line.text);
            }
        }
    }

    /// Runs a structured redaction pass over any [`Redactable`] domain value.
    pub fn redact_item<T: Redactable>(&self, item: &mut T) {
        if self.is_empty() {
            return;
        }
        item.redact_with(self);
    }
}

/// Rolling-tail streaming redactor for token-delta streams (e.g. Claude `--include-partial-messages`).
///
/// Buffers at most `max_pattern_len - 1` trailing bytes across chunks so secrets split across
/// arbitrarily many small token deltas never appear unredacted in the UI.
#[derive(Debug, Clone)]
pub struct StreamingRedactor {
    redactor: Arc<SecretRedactor>,
    pending: String,
}

impl StreamingRedactor {
    pub fn new(redactor: Arc<SecretRedactor>) -> Self {
        Self {
            redactor,
            pending: String::new(),
        }
    }

    /// Appends a streaming delta and returns the safe, redacted prefix ready to emit immediately.
    pub fn push(&mut self, delta: &str) -> String {
        if self.redactor.is_empty() {
            return delta.to_owned();
        }
        self.pending.push_str(delta);
        let redacted = self.redactor.redact(&self.pending);
        let hold = self.redactor.max_pattern_len().saturating_sub(1);
        if redacted.len() <= hold {
            self.pending = redacted;
            return String::new();
        }
        let mut split_at = redacted.len() - hold;
        while split_at > 0 && !redacted.is_char_boundary(split_at) {
            split_at -= 1;
        }
        let emit = redacted[..split_at].to_owned();
        self.pending = redacted[split_at..].to_owned();
        emit
    }

    /// Flushes and redacts all remaining buffered text (call at end of message or turn).
    pub fn flush(&mut self) -> String {
        if self.pending.is_empty() {
            return String::new();
        }
        let remaining = std::mem::take(&mut self.pending);
        self.redactor.redact(&remaining)
    }
}

fn secret_variants(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |s: String| {
        if s.chars().count() >= MIN_REDACT_CHARS && !out.contains(&s) {
            out.push(s);
        }
    };

    push(raw.to_owned());

    let trimmed_nl = raw.trim_end_matches(['\r', '\n']);
    if trimmed_nl != raw {
        push(trimmed_nl.to_owned());
    }
    let trimmed = raw.trim();
    if trimmed != raw && trimmed != trimmed_nl {
        push(trimmed.to_owned());
    }

    // JSON-string-escaped variant (inner content without surrounding quotes).
    for base in [raw, trimmed_nl] {
        if base.chars().count() < MIN_REDACT_CHARS {
            continue;
        }
        if let Ok(json_str) = serde_json::to_string(base)
            && json_str.len() >= 2
        {
            push(json_str[1..json_str.len() - 1].to_owned());
        }
    }

    // Standard & URL-safe base64 (with and without `=`).
    for base in [raw, trimmed_nl] {
        if base.chars().count() < MIN_REDACT_CHARS {
            continue;
        }
        let bytes = base.as_bytes();
        push(base64::engine::general_purpose::STANDARD.encode(bytes));
        push(base64::engine::general_purpose::STANDARD_NO_PAD.encode(bytes));
        push(base64::engine::general_purpose::URL_SAFE.encode(bytes));
        push(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes));

        // Percent-encoded variants (uppercase/lowercase hex, %20 and + for space).
        push(percent_encode(base, true, false));
        push(percent_encode(base, false, false));
        if base.contains(' ') {
            push(percent_encode(base, true, true));
            push(percent_encode(base, false, true));
        }
    }

    out
}

fn percent_encode(input: &str, uppercase: bool, space_as_plus: bool) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else if b == b' ' && space_as_plus {
            out.push('+');
        } else if uppercase {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push_str(&format!("%{b:02x}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_first_prevents_partial_leakage() {
        let short = SecretString::from("tok_123456".to_owned());
        let long = SecretString::from("tok_123456_extra_suffix".to_owned());
        let redactor = SecretRedactor::from_secrets([("SHORT", &short), ("LONG", &long)]);

        let out = redactor.redact("found tok_123456_extra_suffix and tok_123456");
        assert_eq!(out, "found [secret:LONG] and [secret:SHORT]");
    }

    #[test]
    fn streaming_redactor_catches_secret_split_across_chunks() {
        let sec = SecretString::from("sk_live_abcdefgh1234".to_owned());
        let redactor = Arc::new(SecretRedactor::from_secrets([("STRIPE_KEY", &sec)]));
        let mut stream = StreamingRedactor::new(redactor);

        let mut emitted = String::new();
        for chunk in ["Here is ", "sk_li", "ve_ab", "cdef", "gh1234", " done!"] {
            let piece = stream.push(chunk);
            assert!(
                !piece.contains("sk_live_abcdefgh1234"),
                "leaked raw secret in chunk: {piece}"
            );
            emitted.push_str(&piece);
        }
        emitted.push_str(&stream.flush());
        assert_eq!(emitted, "Here is [secret:STRIPE_KEY] done!");
    }

    #[test]
    fn redacts_multiline_json_escaped_base64_and_percent_encoded_variants() {
        let sec = SecretString::from("line1\nline2/a b+c".to_owned());
        let redactor = SecretRedactor::from_secrets([("PEM", &sec)]);

        let b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(sec.expose_secret());
        let json_esc = serde_json::to_string(sec.expose_secret()).unwrap();
        let inner_json = &json_esc[1..json_esc.len() - 1];
        let pct = percent_encode(sec.expose_secret(), true, false);

        assert_eq!(
            redactor.redact(&format!("raw={}", sec.expose_secret())),
            "raw=[secret:PEM]"
        );
        assert_eq!(redactor.redact(&format!("b64={b64}")), "b64=[secret:PEM]");
        assert_eq!(
            redactor.redact(&format!("json={inner_json}")),
            "json=[secret:PEM]"
        );
        assert_eq!(redactor.redact(&format!("url={pct}")), "url=[secret:PEM]");
    }

    #[test]
    fn json_value_redaction_preserves_valid_json_when_weak_secret_matches_keys() {
        let weak = SecretString::from("messages".to_owned());
        let redactor = SecretRedactor::from_secrets([("WEAK", &weak)]);

        let mut val = serde_json::json!({
            "type": "messages",
            "messages": [{"text": "hello messages world"}]
        });
        redactor.redact_json_value(&mut val);
        assert_eq!(val["type"], "messages");
        assert!(val["messages"].is_array());
        assert_eq!(val["messages"][0]["text"], "hello [secret:WEAK] world");
    }
}
