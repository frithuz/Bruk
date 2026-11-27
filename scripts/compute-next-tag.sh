#!/usr/bin/env bash
set -euo pipefail

# Computes the next SemVer `vX.Y.Z` git tag for an explicit bump level
# (`major`, `minor`, or `patch`) based on the highest `v*` tag reachable
# from `<target-ref>` (default: HEAD).
#
# When no `v*` tags exist yet, bumps from `v0.0.0`:
#   patch -> v0.0.1
#   minor -> v0.1.0
#   major -> v1.0.0
#
# Usage:
#   scripts/compute-next-tag.sh <major|minor|patch> [<target-ref>]

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: $0 <major|minor|patch> [<target-ref>]" >&2
  exit 1
fi

bump="$1"
target_ref="${2:-HEAD}"

case "$bump" in
  major|minor|patch) ;;
  *)
    echo "error: invalid bump '${bump}' (expected major, minor, or patch)" >&2
    exit 1
    ;;
esac

# Find the highest SemVer vX.Y.Z tag reachable from target_ref.
# Prefer stable tags (vX.Y.Z); fall back to prerelease tags (vX.Y.Z-...)
# only if no stable tags exist yet.
find_latest_semver() {
  local mode="$1"
  local best_maj=-1 best_min=-1 best_pat=-1
  local tag stripped core maj min pat

  while IFS= read -r tag; do
    [ -z "$tag" ] && continue
    if [ "$mode" = "stable" ]; then
      [[ "$tag" =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)$ ]] || continue
    else
      [[ "$tag" =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)-[0-9A-Za-z.-]+$ ]] || continue
    fi

    stripped="${tag#v}"
    core="${stripped%%-*}"
    IFS='.' read -r maj min pat <<< "$core"

    maj=$((10#$maj))
    min=$((10#$min))
    pat=$((10#$pat))

    if [ "$maj" -gt "$best_maj" ] || \
       { [ "$maj" -eq "$best_maj" ] && [ "$min" -gt "$best_min" ]; } || \
       { [ "$maj" -eq "$best_maj" ] && [ "$min" -eq "$best_min" ] && [ "$pat" -gt "$best_pat" ]; }; then
      best_maj=$maj
      best_min=$min
      best_pat=$pat
    fi
  done < <(git tag --merged "$target_ref" --list 'v[0-9]*')

  if [ "$best_maj" -ge 0 ]; then
    printf '%d.%d.%d\n' "$best_maj" "$best_min" "$best_pat"
  fi
}

latest="$(find_latest_semver stable)"
if [ -z "$latest" ]; then
  latest="$(find_latest_semver prerelease)"
fi
if [ -z "$latest" ]; then
  latest="0.0.0"
fi

IFS='.' read -r major minor patch <<< "$latest"

case "$bump" in
  major)
    major=$((major + 1))
    minor=0
    patch=0
    ;;
  minor)
    minor=$((minor + 1))
    patch=0
    ;;
  patch)
    patch=$((patch + 1))
    ;;
esac

printf 'v%d.%d.%d\n' "$major" "$minor" "$patch"
