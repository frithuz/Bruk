#!/usr/bin/env bash
set -euo pipefail

# Computes the highest SemVer bump requested by `Version-Bump` git trailers
# since the latest reachable `v*` tag.
#
# Output (stdout):
#   major | minor | patch | none
#
# Usage:
#   scripts/compute-release-bump.sh [<target-ref>]

target_ref="${1:-HEAD}"

prev_tag="$(git describe --tags --abbrev=0 --match 'v[0-9]*' "$target_ref" 2>/dev/null || true)"
if [ -n "$prev_tag" ]; then
  rev_range="${prev_tag}..${target_ref}"
else
  rev_range="$target_ref"
fi

# rank: 0 = none, 1 = patch, 2 = minor, 3 = major
rank=0

while IFS= read -r val; do
  val="${val#"${val%%[![:space:]]*}"}"
  val="${val%"${val##*[![:space:]]}"}"
  [ -z "$val" ] && continue

  case "$val" in
    Major)
      rank=3
      ;;
    Minor)
      if [ "$rank" -lt 2 ]; then
        rank=2
      fi
      ;;
    Patch)
      if [ "$rank" -lt 1 ]; then
        rank=1
      fi
      ;;
    *)
      echo "error: invalid Version-Bump trailer value '${val}' in ${rev_range} (expected Major, Minor, or Patch)" >&2
      exit 1
      ;;
  esac
done < <(git -c trailer.separators=':#' log --format='%(trailers:key=Version-Bump,valueonly,unfold)' "$rev_range")

case "$rank" in
  3) echo "major" ;;
  2) echo "minor" ;;
  1) echo "patch" ;;
  0) echo "none" ;;
esac
