#!/usr/bin/env bash
set -euo pipefail

# Ephemerally stamps a release version (from a git tag) into Cargo.toml,
# nsh.json, and Cargo.lock for CI packaging. Intended for CI build runners
# only; changes should never be committed back to main.
#
# Usage:
#   scripts/stamp-version.sh <version-or-v-tag>

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <version-or-v-tag>" >&2
  exit 1
fi

version="${1#v}"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  echo "error: invalid SemVer version '${version}'" >&2
  exit 1
fi

# 1. Stamp [workspace.package].version in Cargo.toml
awk -v ver="$version" '
  BEGIN { in_pkg = 0; replaced = 0 }
  /^\[workspace\.package\][[:space:]]*$/ { in_pkg = 1; print; next }
  /^\[/ { in_pkg = 0; print; next }
  in_pkg && /^version = "/ && !replaced {
    print "version = \"" ver "\""
    replaced = 1
    next
  }
  { print }
  END {
    if (!replaced) {
      print "error: failed to locate [workspace.package] version in Cargo.toml" > "/dev/stderr"
      exit 1
    }
  }
' Cargo.toml > Cargo.toml.tmp
mv Cargo.toml.tmp Cargo.toml

# 2. Stamp .version in nsh.json
jq --arg v "$version" '.version = $v' nsh.json > nsh.json.tmp
mv nsh.json.tmp nsh.json

# 3. Refresh workspace member versions in Cargo.lock without touching external deps
if [ -f Cargo.lock ] && command -v cargo >/dev/null 2>&1; then
  cargo metadata --format-version 1 --offline >/dev/null
fi

echo "Stamped release version ${version} into Cargo.toml, nsh.json, and Cargo.lock"
