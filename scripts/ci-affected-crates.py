#!/usr/bin/env python3
"""Compute affected workspace crates and platform CI scopes from a git diff.

Parses Cargo.toml and crates/*/Cargo.toml directly (no Cargo/Rust toolchain
required) so this script can run in a lightweight ubuntu-slim planning job.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

try:
    import tomllib  # Python 3.11+
except ModuleNotFoundError:  # pragma: no cover - Python <3.11 fallback
    tomllib = None

# Cross-platform domain crates checked/tested on Linux runners.
DOMAIN_CRATES = frozenset(
    {
        "factory-types",
        "factory-flags",
        "factory-machines",
        "factory-github",
        "factory-secrets",
        "factory-providers",
        "factory-config",
        "factory-workflows",
        "factory-chat",
        "factory-runner",
        "factory-storage",
        "factory-core",
    }
)

# Domain crates that also contain #[cfg(target_os = "macos")] code paths/tests
# and should additionally be checked/tested on macOS when affected.
MACOS_SENSITIVE_DOMAIN_CRATES = frozenset(
    {
        "factory-secrets",
        "factory-chat",
    }
)

# Crates that cannot run cargo test (macOS binary-only crate with 0 tests).
TEST_EXCLUDED_CRATES = frozenset({"factory-desktop"})

# Files whose modification invalidates crate-level scoping and triggers a full
# workspace CI run.
GLOBAL_TRIGGER_FILES = frozenset(
    {
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "nsh.json",
        "scripts/ci-affected-crates.py",
        ".github/workflows/lint.yml",
        ".github/workflows/test.yml",
    }
)

GLOBAL_TRIGGER_PREFIXES = (
    ".github/actions/setup-rust/",
)

# Non-crate paths that feed directly into specific crates.
EXTRA_PATH_TO_CRATES = (
    ("assets/", ("factory-app", "factory-desktop")),
)

CRATE_NAME_RE = re.compile(r"^[a-z0-9-]+$")


def parse_toml_minimal(path: Path) -> dict:
    """Parse Cargo.toml with tomllib, or a minimal fallback for Python <3.11."""
    raw = path.read_text(encoding="utf-8")
    if tomllib is not None:
        return tomllib.loads(raw)

    data: dict = {}
    section_path: list[str] = []
    in_array_key: str | None = None
    array_vals: list[str] = []

    def ensure_section(parts: list[str]) -> dict:
        cur = data
        for part in parts:
            cur = cur.setdefault(part, {})
        return cur

    for raw_line in raw.splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue
        if in_array_key is not None:
            for m in re.finditer(r'"([^"]+)"', line):
                array_vals.append(m.group(1))
            if "]" in line:
                ensure_section(section_path)[in_array_key] = array_vals
                in_array_key = None
                array_vals = []
            continue
        if line.startswith("[[") and line.endswith("]]"):
            sec_name = line[2:-2].strip()
            lst = data.setdefault(sec_name, [])
            lst.append({})
            section_path = [sec_name]
            continue
        if line.startswith("[") and line.endswith("]"):
            header = line[1:-1].strip()
            section_path = [p.strip().strip('"').strip("'") for p in header.split(".")]
            ensure_section(section_path)
            continue
        m = re.match(r'^([A-Za-z0-9_.-]+)\s*=\s*(.+)$', line)
        if not m:
            continue
        key, val = m.group(1), m.group(2).strip()
        # Handle dotted keys like `factory-types.workspace = true`
        if "." in key:
            key = key.split(".", 1)[0]
        target = (
            data[section_path[0]][-1]
            if section_path and isinstance(data.get(section_path[0]), list)
            else ensure_section(section_path)
        )
        if val.startswith("["):
            items = [x.group(1) for x in re.finditer(r'"([^"]+)"', val)]
            if "]" in val:
                target[key] = items
            else:
                in_array_key = key
                array_vals = items
        elif val in ("true", "false"):
            target[key] = val == "true"
        elif val.startswith('"') and val.endswith('"'):
            target[key] = val[1:-1]
        else:
            target[key] = val
    return data


def load_workspace(repo_root: Path) -> tuple[dict[str, Path], dict[str, set[str]], set[str]]:
    """Return (crate_dirs, reverse_deps, testable_crates)."""
    root_toml = parse_toml_minimal(repo_root / "Cargo.toml")
    members = root_toml.get("workspace", {}).get("members", [])

    crate_dirs: dict[str, Path] = {}
    deps_by_crate: dict[str, set[str]] = {}
    testable_crates: set[str] = set()

    for member in members:
        member_path = Path(member)
        manifest = parse_toml_minimal(repo_root / member_path / "Cargo.toml")
        pkg = manifest.get("package", {})
        name = pkg.get("name")
        if not name or not CRATE_NAME_RE.match(name):
            raise ValueError(f"Invalid crate name {name!r} in {member_path}")
        crate_dirs[name] = member_path

        dep_names: set[str] = set()
        for sec in ("dependencies", "dev-dependencies", "build-dependencies"):
            dep_names.update(manifest.get(sec, {}).keys())
        for target_cfg in manifest.get("target", {}).values():
            if isinstance(target_cfg, dict):
                for sec in ("dependencies", "dev-dependencies", "build-dependencies"):
                    dep_names.update(target_cfg.get(sec, {}).keys())
        deps_by_crate[name] = dep_names

        # Determine if this crate has any testable target (lib test or tests/*.rs).
        lib_cfg = manifest.get("lib", {})
        lib_test_enabled = lib_cfg.get("test", True) is not False
        has_lib_src = (repo_root / member_path / "src" / "lib.rs").is_file()
        has_int_tests = any((repo_root / member_path / "tests").glob("*.rs"))
        if name not in TEST_EXCLUDED_CRATES and ((has_lib_src and lib_test_enabled) or has_int_tests):
            testable_crates.add(name)

    reverse_deps: dict[str, set[str]] = {c: set() for c in crate_dirs}
    for crate, deps in deps_by_crate.items():
        for dep in deps:
            if dep in reverse_deps:
                reverse_deps[dep].add(crate)

    return crate_dirs, reverse_deps, testable_crates


def expand_reverse_deps(seeds: set[str], reverse_deps: dict[str, set[str]]) -> set[str]:
    affected = set(seeds)
    stack = list(seeds)
    while stack:
        cur = stack.pop()
        for parent in reverse_deps.get(cur, ()):
            if parent not in affected:
                affected.add(parent)
                stack.append(parent)
    return affected


def git_changed_files(repo_root: Path, base: str, head: str) -> list[str] | None:
    """Return changed file paths between base and head, or None on git error."""
    for rev_spec in (f"{base}...{head}", f"{base}..{head}"):
        proc = subprocess.run(
            ["git", "diff", "--name-only", rev_spec],
            cwd=repo_root,
            capture_output=True,
            text=True,
            check=False,
        )
        if proc.returncode == 0:
            return [line.strip() for line in proc.stdout.splitlines() if line.strip()]
    return None


def pkg_flags(crates: list[str]) -> str:
    return " ".join(f"-p {c}" for c in crates)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="", help="Base git SHA/ref for PR diff")
    parser.add_argument("--head", default="HEAD", help="Head git SHA/ref for PR diff")
    parser.add_argument(
        "--full",
        action="store_true",
        help="Force full-workspace mode (used for pushes to main / non-PR runs)",
    )
    parser.add_argument(
        "--github-output",
        default="",
        help="Optional path to GitHub Actions GITHUB_OUTPUT file",
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent.parent
    crate_dirs, reverse_deps, testable_crates = load_workspace(repo_root)
    all_crates = set(crate_dirs.keys())
    domain_all = sorted(DOMAIN_CRATES & all_crates)
    domain_testable_all = sorted((DOMAIN_CRATES & testable_crates))

    full_workspace = args.full or not args.base
    reason = "explicit --full or missing base ref"
    directly_changed: set[str] = set()

    if not full_workspace:
        changed = git_changed_files(repo_root, args.base, args.head)
        if changed is None:
            full_workspace = True
            reason = f"git diff failed for {args.base}..{args.head}"
        else:
            dir_to_crate = {str(p).rstrip("/") + "/": name for name, p in crate_dirs.items()}
            for path in changed:
                if path in GLOBAL_TRIGGER_FILES or any(
                    path.startswith(prefix) for prefix in GLOBAL_TRIGGER_PREFIXES
                ):
                    full_workspace = True
                    reason = f"global trigger changed: {path}"
                    break
                matched = False
                for prefix, crate in dir_to_crate.items():
                    if path.startswith(prefix):
                        directly_changed.add(crate)
                        matched = True
                        break
                if matched:
                    continue
                for prefix, mapped_crates in EXTRA_PATH_TO_CRATES:
                    if path.startswith(prefix):
                        directly_changed.update(c for c in mapped_crates if c in all_crates)
                        break
            if not full_workspace:
                reason = (
                    f"scoped to {len(directly_changed)} changed crate(s)"
                    if directly_changed
                    else "no Rust crates affected"
                )

    if full_workspace:
        domain_clippy_pkgs = domain_all
        domain_test_pkgs = domain_testable_all
        # On full runs (e.g. main pushes), run full workspace on macOS to warm sccache
        # and verify macOS-only #[cfg] paths across all crates.
        macos_clippy_args = "--workspace"
        macos_test_args = "--workspace --exclude factory-desktop"
        run_domain_clippy = bool(domain_clippy_pkgs)
        run_domain_test = bool(domain_test_pkgs)
        run_macos_clippy = True
        run_macos_test = True
        domain_clippy_args = pkg_flags(domain_clippy_pkgs)
        domain_test_args = pkg_flags(domain_test_pkgs)
        affected_summary = sorted(all_crates)
    else:
        affected = expand_reverse_deps(directly_changed, reverse_deps)
        affected_summary = sorted(affected)

        domain_clippy_pkgs = sorted(affected & DOMAIN_CRATES)
        domain_test_pkgs = sorted(affected & DOMAIN_CRATES & testable_crates)

        macos_ui_affected = affected - DOMAIN_CRATES
        macos_sensitive_affected = affected & MACOS_SENSITIVE_DOMAIN_CRATES
        macos_clippy_pkgs = sorted(macos_ui_affected | macos_sensitive_affected)
        macos_test_pkgs = sorted(
            (macos_ui_affected | macos_sensitive_affected) & testable_crates
        )

        run_domain_clippy = bool(domain_clippy_pkgs)
        run_domain_test = bool(domain_test_pkgs)
        run_macos_clippy = bool(macos_clippy_pkgs)
        run_macos_test = bool(macos_test_pkgs)

        domain_clippy_args = pkg_flags(domain_clippy_pkgs)
        domain_test_args = pkg_flags(domain_test_pkgs)
        macos_clippy_args = pkg_flags(macos_clippy_pkgs)
        macos_test_args = pkg_flags(macos_test_pkgs)

    outputs = {
        "full_workspace": str(full_workspace).lower(),
        "run_domain_clippy": str(run_domain_clippy).lower(),
        "run_domain_test": str(run_domain_test).lower(),
        "run_macos_clippy": str(run_macos_clippy).lower(),
        "run_macos_test": str(run_macos_test).lower(),
        "domain_clippy_args": domain_clippy_args,
        "domain_test_args": domain_test_args,
        "macos_clippy_args": macos_clippy_args,
        "macos_test_args": macos_test_args,
    }

    print(f"CI plan ({reason}):")
    print(f"  affected crates ({len(affected_summary)}): {', '.join(affected_summary) or '(none)'}")
    for k, v in outputs.items():
        print(f"  {k}={v}")

    if args.github_output:
        with open(args.github_output, "a", encoding="utf-8") as fh:
            for k, v in outputs.items():
                fh.write(f"{k}={v}\n")

    return 0


if __name__ == "__main__":
    sys.exit(main())
