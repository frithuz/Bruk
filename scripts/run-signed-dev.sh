#!/usr/bin/env bash
set -euo pipefail

# Builds factory-desktop and signs the debug binary on macOS with a stable
# identifier (`dev.frithuz.bruk`) and local dev certificate when available,
# preventing repeated macOS Keychain ACL prompts across rebuilds.

cargo build -p factory-desktop

BIN="target/debug/factory-desktop"
if [ "$(uname -s)" = "Darwin" ] && [ -x "$BIN" ]; then
  IDENTITY="${BRUK_DEV_SIGN_IDENTITY:-}"
  if [ -z "$IDENTITY" ] && security find-identity -v -p codesigning 2>/dev/null | grep -Fq '"Bruk Local Dev"'; then
    IDENTITY="Bruk Local Dev"
  fi

  if [ -n "$IDENTITY" ]; then
    codesign --force --sign "$IDENTITY" --identifier dev.frithuz.bruk --timestamp=none "$BIN"
  else
    echo "note: no local dev signing identity found (run \`task macos:setup-dev-cert\` to avoid repeated macOS Keychain prompts); signing ad hoc" >&2
    codesign --force --sign - --identifier dev.frithuz.bruk --timestamp=none "$BIN"
  fi
fi

exec "$BIN" "$@"
