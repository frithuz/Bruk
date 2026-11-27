#!/usr/bin/env bash
set -euo pipefail

# Validates `Version-Bump` and `Changelog` git trailers across a commit range.
#
# Rules:
#   1. `Version-Bump` is optional. When present, there may be at most one per
#      commit and its value must be strictly `Major`, `Minor`, or `Patch`.
#   2. When `Version-Bump` is provided, at least one non-empty `Changelog`
#      trailer must also be provided.
#   3. When `Changelog` is provided, `Version-Bump` must also be provided (so
#      curated release notes are never silently dropped).
#   4. Canonical casing (`Version-Bump:` / `Changelog:`) is required and lines
#      must live in the final git trailer block so git-cliff and git trailers
#      parse them consistently.
#
# Usage:
#   scripts/lint-commit-trailers.sh [<rev-range-or-commit>]
#
# When omitted, validates HEAD.

rev="${1:-HEAD}"

if [[ "$rev" == *".."* ]]; then
  commits="$(git rev-list --no-merges "$rev")"
else
  commits="$(git rev-list --no-merges -n 1 "$rev")"
fi

if [ -z "$commits" ]; then
  echo "No non-merge commits to lint in range: ${rev}"
  exit 0
fi

failed=0
checked=0

trim() {
  local s="$1"
  s="${s#"${s%%[![:space:]]*}"}"
  s="${s%"${s##*[![:space:]]}"}"
  printf '%s' "$s"
}

while IFS= read -r sha; do
  [ -z "$sha" ] && continue
  checked=$((checked + 1))

  short_sha="$(git rev-parse --short "$sha")"
  subject="$(git log -1 --format='%s' "$sha")"
  raw_msg="$(git log -1 --format='%B' "$sha")"
  trailers="$(git -c trailer.separators=':#' log -1 --format='%(trailers:only,unfold)' "$sha")"

  commit_errors=()

  # Count raw lines that look like Version-Bump or Changelog (case-insensitive)
  # anywhere in the commit message so misplaced or miscased lines are caught.
  raw_vb_count=0
  raw_cl_count=0
  while IFS= read -r line; do
    lower_line="$(printf '%s' "$line" | tr '[:upper:]' '[:lower:]')"
    case "$lower_line" in
      version-bump:*)
        raw_vb_count=$((raw_vb_count + 1))
        if [[ "$line" != Version-Bump:* ]]; then
          commit_errors+=("trailer key must use exact casing 'Version-Bump:' (found '${line%%:*}:')")
        fi
        ;;
      changelog:*)
        raw_cl_count=$((raw_cl_count + 1))
        if [[ "$line" != Changelog:* ]]; then
          commit_errors+=("trailer key must use exact casing 'Changelog:' (found '${line%%:*}:')")
        fi
        ;;
    esac
  done <<< "$raw_msg"

  # Parse actual git trailers from the final trailer block.
  vb_count=0
  cl_count=0
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    case "$line" in
      Version-Bump:*)
        vb_count=$((vb_count + 1))
        val="$(trim "${line#Version-Bump:}")"
        case "$val" in
          Major|Minor|Patch) ;;
          *)
            commit_errors+=("invalid Version-Bump value '${val}'; must be strictly one of: Major, Minor, Patch")
            ;;
        esac
        ;;
      Changelog:*)
        cl_count=$((cl_count + 1))
        val="$(trim "${line#Changelog:}")"
        if [ -z "$val" ]; then
          commit_errors+=("Changelog trailer must not be empty")
        fi
        ;;
    esac
  done <<< "$trailers"

  if [ "$raw_vb_count" -ne "$vb_count" ] && [ "${#commit_errors[@]}" -eq 0 ]; then
    commit_errors+=("Version-Bump line was not parsed as a git trailer (place it in the final trailer paragraph with no trailing blank paragraphs)")
  fi

  if [ "$raw_cl_count" -ne "$cl_count" ] && [ "${#commit_errors[@]}" -eq 0 ]; then
    commit_errors+=("Changelog line was not parsed as a git trailer (place it in the final trailer paragraph with no trailing blank paragraphs)")
  fi

  if [ "$vb_count" -gt 1 ]; then
    commit_errors+=("at most one Version-Bump trailer is allowed per commit (found ${vb_count})")
  fi

  if [ "$vb_count" -gt 0 ] && [ "$cl_count" -eq 0 ]; then
    commit_errors+=("Version-Bump trailer requires at least one 'Changelog: <sentence>' trailer")
  fi

  if [ "$cl_count" -gt 0 ] && [ "$vb_count" -eq 0 ]; then
    commit_errors+=("Changelog trailer requires a 'Version-Bump: Major|Minor|Patch' trailer")
  fi

  if [ "${#commit_errors[@]}" -gt 0 ]; then
    failed=1
    echo "error: commit ${short_sha} (${subject}):" >&2
    for err in "${commit_errors[@]}"; do
      echo "  - ${err}" >&2
    done
  fi
done <<< "$commits"

if [ "$failed" -ne 0 ]; then
  exit 1
fi

echo "Commit trailer lint passed (${checked} commit(s) checked)."
