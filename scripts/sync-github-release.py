#!/usr/bin/env python3
"""Inspect and synchronize GitHub Releases for Bruk's rolling draft workflow.

Uses only the Python 3 standard library so it can run directly on ubuntu-slim
without installing extra packages or depending on jq.

Subcommands:
  check-tag    Check whether a v* tag already has a published release with assets.
  check-draft  Check whether the rolling draft release needs creation or update.
  sync         Create/update a rolling draft release or atomically publish one.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

STABLE_TAG_RE = re.compile(r"^v[0-9]+\.[0-9]+\.[0-9]+$")
PRERELEASE_TAG_RE = re.compile(r"^v[0-9]+\.[0-9]+\.[0-9]+-[0-9A-Za-z.-]+$")
FULL_SHA_RE = re.compile(r"^[0-9a-f]{40}$")
NEXT_LINK_RE = re.compile(r'<([^>]+)>;\s*rel="next"')

VERSIONED_ASSET_PATTERNS = (
    re.compile(r"^Bruk-v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?-macos-arm64\.dmg(?:\.sha256)?$"),
    re.compile(r"^Bruk-[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?-macos-aarch64\.tar\.gz(?:\.sha256)?$"),
)

RETRYABLE_HTTP_CODES = frozenset({429, 500, 502, 503, 504})
MAX_RETRIES = 3


class GitHubApiError(RuntimeError):
    """Raised on non-success GitHub API responses."""

    def __init__(self, status: int, message: str, url: str) -> None:
        super().__init__(f"GitHub API HTTP {status} for {url}: {message}")
        self.status = status
        self.url = url


class GitHubClient:
    """Minimal stdlib GitHub REST API client with retry and pagination."""

    def __init__(self, token: str | None, api_base: str | None = None) -> None:
        self.token = token or ""
        self.api_base = (api_base or os.environ.get("GITHUB_API_URL") or "https://api.github.com").rstrip("/")

    def _headers(self, content_type: str | None = None) -> dict[str, str]:
        headers = {
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "bruk-release-sync",
        }
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"
        if content_type:
            headers["Content-Type"] = content_type
        return headers

    def request_json(
        self,
        method: str,
        path_or_url: str,
        payload: dict[str, Any] | None = None,
        allow_404: bool = False,
    ) -> tuple[Any, dict[str, str]]:
        url = path_or_url if "://" in path_or_url else f"{self.api_base}/{path_or_url.lstrip('/')}"
        data: bytes | None = None
        content_type: str | None = None
        if payload is not None:
            data = json.dumps(payload).encode("utf-8")
            content_type = "application/json"

        last_err: Exception | None = None
        for attempt in range(1, MAX_RETRIES + 1):
            req = urllib.request.Request(
                url,
                data=data,
                headers=self._headers(content_type=content_type),
                method=method,
            )
            try:
                with urllib.request.urlopen(req, timeout=60) as resp:
                    raw = resp.read()
                    resp_headers = {k.lower(): v for k, v in resp.headers.items()}
                    if not raw:
                        return None, resp_headers
                    return json.loads(raw.decode("utf-8")), resp_headers
            except urllib.error.HTTPError as exc:
                body = ""
                try:
                    body = exc.read().decode("utf-8", errors="replace").strip()
                except Exception:
                    pass
                if allow_404 and exc.code == 404:
                    return None, {}
                if exc.code in RETRYABLE_HTTP_CODES and attempt < MAX_RETRIES:
                    delay = _retry_delay(exc.headers, attempt)
                    print(
                        f"Transient HTTP {exc.code} from {method} {url}; retrying in {delay:.1f}s ({attempt}/{MAX_RETRIES})...",
                        file=sys.stderr,
                    )
                    time.sleep(delay)
                    last_err = GitHubApiError(exc.code, body or exc.reason, url)
                    continue
                raise GitHubApiError(exc.code, body or str(exc.reason), url) from exc
            except urllib.error.URLError as exc:
                if attempt < MAX_RETRIES:
                    delay = float(2**attempt)
                    print(
                        f"Network error during {method} {url} ({exc.reason}); retrying in {delay:.1f}s ({attempt}/{MAX_RETRIES})...",
                        file=sys.stderr,
                    )
                    time.sleep(delay)
                    last_err = exc
                    continue
                raise RuntimeError(f"Network error during {method} {url}: {exc.reason}") from exc

        assert last_err is not None
        raise last_err

    def list_releases(self, repo: str) -> list[dict[str, Any]]:
        releases: list[dict[str, Any]] = []
        next_url: str | None = f"repos/{repo}/releases?per_page=100"
        while next_url:
            page, headers = self.request_json("GET", next_url)
            if isinstance(page, list):
                releases.extend(page)
            link = headers.get("link", "")
            match = NEXT_LINK_RE.search(link)
            next_url = match.group(1) if match else None
        return releases

    def get_published_release_by_tag(self, repo: str, tag: str) -> dict[str, Any] | None:
        encoded = urllib.parse.quote(tag, safe="")
        data, _ = self.request_json("GET", f"repos/{repo}/releases/tags/{encoded}", allow_404=True)
        return data if isinstance(data, dict) else None

    def list_release_assets(self, repo: str, release_id: int) -> list[dict[str, Any]]:
        assets: list[dict[str, Any]] = []
        next_url: str | None = f"repos/{repo}/releases/{release_id}/assets?per_page=100"
        while next_url:
            page, headers = self.request_json("GET", next_url)
            if isinstance(page, list):
                assets.extend(page)
            link = headers.get("link", "")
            match = NEXT_LINK_RE.search(link)
            next_url = match.group(1) if match else None
        return assets

    def delete_asset(self, repo: str, asset_id: int) -> None:
        self.request_json("DELETE", f"repos/{repo}/releases/assets/{asset_id}", allow_404=True)

    def create_release(self, repo: str, payload: dict[str, Any]) -> dict[str, Any]:
        data, _ = self.request_json("POST", f"repos/{repo}/releases", payload=payload)
        if not isinstance(data, dict):
            raise RuntimeError("Unexpected response from POST release endpoint")
        return data

    def update_release(self, repo: str, release_id: int, payload: dict[str, Any]) -> dict[str, Any]:
        data, _ = self.request_json("PATCH", f"repos/{repo}/releases/{release_id}", payload=payload)
        if not isinstance(data, dict):
            raise RuntimeError("Unexpected response from PATCH release endpoint")
        return data

    def upload_asset(self, repo: str, release: dict[str, Any], asset_path: Path) -> dict[str, Any]:
        release_id = int(release["id"])
        upload_template = str(release.get("upload_url") or "")
        if not upload_template:
            raise RuntimeError(f"Release {release_id} response is missing upload_url")
        upload_base = upload_template.split("{", 1)[0]
        asset_name = asset_path.name
        upload_url = f"{upload_base}?{urllib.parse.urlencode({'name': asset_name})}"
        content_type = _asset_content_type(asset_name)
        file_size = asset_path.stat().st_size

        last_err: Exception | None = None
        for attempt in range(1, MAX_RETRIES + 1):
            headers = self._headers(content_type=content_type)
            headers["Content-Length"] = str(file_size)
            try:
                with asset_path.open("rb") as fh:
                    req = urllib.request.Request(upload_url, data=fh, headers=headers, method="POST")
                    with urllib.request.urlopen(req, timeout=300) as resp:
                        raw = resp.read()
                        return json.loads(raw.decode("utf-8")) if raw else {}
            except urllib.error.HTTPError as exc:
                body = ""
                try:
                    body = exc.read().decode("utf-8", errors="replace").strip()
                except Exception:
                    pass
                # Recover from partial/zombie "starter" assets left by a prior failed upload.
                if exc.code == 422 and "already_exists" in body and attempt < MAX_RETRIES:
                    print(
                        f"Asset '{asset_name}' already exists on release {release_id}; removing stale asset before retry...",
                        file=sys.stderr,
                    )
                    for existing in self.list_release_assets(repo, release_id):
                        if existing.get("name") == asset_name and existing.get("id") is not None:
                            self.delete_asset(repo, int(existing["id"]))
                    last_err = GitHubApiError(exc.code, body or exc.reason, upload_url)
                    continue
                if exc.code in RETRYABLE_HTTP_CODES and attempt < MAX_RETRIES:
                    delay = _retry_delay(exc.headers, attempt)
                    print(
                        f"Transient HTTP {exc.code} uploading '{asset_name}'; retrying in {delay:.1f}s ({attempt}/{MAX_RETRIES})...",
                        file=sys.stderr,
                    )
                    # Clean up any starter asset created before the error.
                    for existing in self.list_release_assets(repo, release_id):
                        if existing.get("name") == asset_name and existing.get("id") is not None:
                            self.delete_asset(repo, int(existing["id"]))
                    time.sleep(delay)
                    last_err = GitHubApiError(exc.code, body or exc.reason, upload_url)
                    continue
                raise GitHubApiError(exc.code, body or str(exc.reason), upload_url) from exc
            except urllib.error.URLError as exc:
                if attempt < MAX_RETRIES:
                    delay = float(2**attempt)
                    print(
                        f"Network error uploading '{asset_name}' ({exc.reason}); retrying in {delay:.1f}s ({attempt}/{MAX_RETRIES})...",
                        file=sys.stderr,
                    )
                    for existing in self.list_release_assets(repo, release_id):
                        if existing.get("name") == asset_name and existing.get("id") is not None:
                            self.delete_asset(repo, int(existing["id"]))
                    time.sleep(delay)
                    last_err = exc
                    continue
                raise RuntimeError(f"Network error uploading '{asset_name}': {exc.reason}") from exc

        assert last_err is not None
        raise last_err


def _retry_delay(headers: Any, attempt: int) -> float:
    retry_after = headers.get("Retry-After") if headers else None
    if retry_after:
        try:
            return max(1.0, min(60.0, float(retry_after)))
        except ValueError:
            pass
    return float(2**attempt)


def _asset_content_type(filename: str) -> str:
    if filename.endswith(".json"):
        return "application/json"
    if filename.endswith((".sha256", ".sig", ".txt")):
        return "text/plain; charset=utf-8"
    return "application/octet-stream"


def is_prerelease_tag(tag: str) -> bool:
    return bool(PRERELEASE_TAG_RE.match(tag))


def expected_required_assets(tag: str, version: str) -> set[str]:
    return {
        f"Bruk-{tag}-macos-arm64.dmg",
        f"Bruk-{tag}-macos-arm64.dmg.sha256",
        f"Bruk-{version}-macos-aarch64.tar.gz",
        f"Bruk-{version}-macos-aarch64.tar.gz.sha256",
        "latest.json",
    }


def find_rolling_draft(releases: list[dict[str, Any]], *, prerelease: bool) -> dict[str, Any] | None:
    """Return the newest SemVer draft release matching the requested prerelease class."""
    tag_re = PRERELEASE_TAG_RE if prerelease else STABLE_TAG_RE
    matches = [
        rel
        for rel in releases
        if rel.get("draft") is True
        and bool(rel.get("prerelease")) is prerelease
        and isinstance(rel.get("tag_name"), str)
        and bool(tag_re.match(rel["tag_name"]))
    ]
    if not matches:
        return None
    matches.sort(key=lambda r: int(r.get("id") or 0))
    chosen = matches[-1]
    if len(matches) > 1:
        ids = ", ".join(f"{r.get('tag_name')} (id={r.get('id')})" for r in matches)
        print(
            f"::warning::Found {len(matches)} matching draft releases ({ids}); using newest draft id={chosen.get('id')}.",
            file=sys.stderr,
        )
    return chosen


def write_github_output(path: str | None, outputs: dict[str, str]) -> None:
    if not path:
        return
    with open(path, "a", encoding="utf-8") as fh:
        for key, value in outputs.items():
            fh.write(f"{key}={value}\n")


def run_git(*args: str, cwd: str | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args],
        cwd=cwd,
        check=False,
        capture_output=True,
        text=True,
    )


def git_prev_tag(head: str, cwd: str | None = None) -> str:
    res = run_git("describe", "--tags", "--abbrev=0", "--match", "v[0-9]*", head, cwd=cwd)
    if res.returncode != 0:
        return ""
    return res.stdout.strip()


def git_commit_exists(ref: str, cwd: str | None = None) -> bool:
    return run_git("cat-file", "-e", f"{ref}^{{commit}}", cwd=cwd).returncode == 0


def git_is_ancestor(ancestor: str, descendant: str, cwd: str | None = None) -> bool:
    return run_git("merge-base", "--is-ancestor", ancestor, descendant, cwd=cwd).returncode == 0


def git_range_has_version_bump(rev_range: str, cwd: str | None = None) -> bool:
    res = run_git(
        "-c",
        "trailer.separators=:#",
        "log",
        "--format=%(trailers:key=Version-Bump,valueonly,unfold)",
        rev_range,
        cwd=cwd,
    )
    if res.returncode != 0:
        raise RuntimeError(f"git log failed for range {rev_range}: {res.stderr.strip()}")
    return any(line.strip() for line in res.stdout.splitlines())


def evaluate_draft_staleness(
    releases: list[dict[str, Any]],
    *,
    expected_tag: str,
    expected_version: str,
    head: str = "HEAD",
    cwd: str | None = None,
) -> tuple[bool, str]:
    """Decide whether the rolling draft release must be created or updated."""
    prerelease = is_prerelease_tag(expected_tag)
    draft = find_rolling_draft(releases, prerelease=prerelease)
    if draft is None:
        return True, f"No draft release exists yet; creating draft {expected_tag}."

    draft_id = draft.get("id")
    draft_tag = str(draft.get("tag_name") or "")
    if draft_tag != expected_tag:
        return (
            True,
            f"Draft release (id={draft_id}) will move from {draft_tag} to {expected_tag}.",
        )

    required = expected_required_assets(expected_tag, expected_version)
    existing_assets = {
        str(a.get("name"))
        for a in (draft.get("assets") or [])
        if isinstance(a, dict) and a.get("name") and a.get("state", "uploaded") == "uploaded"
    }
    missing = sorted(required - existing_assets)
    if missing:
        return (
            True,
            f"Draft release {expected_tag} (id={draft_id}) is missing required asset(s) ({', '.join(missing)}); rebuilding.",
        )

    stale_versioned = sorted(
        name
        for name in existing_assets - required
        if any(pat.match(name) for pat in VERSIONED_ASSET_PATTERNS)
    )
    if stale_versioned:
        return (
            True,
            f"Draft release {expected_tag} (id={draft_id}) contains stale versioned asset(s) ({', '.join(stale_versioned)}); rebuilding.",
        )

    target = str(draft.get("target_commitish") or "").strip()
    if not FULL_SHA_RE.match(target):
        return (
            True,
            f"Draft release {expected_tag} (id={draft_id}) target_commitish '{target}' is not a pinned commit SHA; rebuilding.",
        )

    short_target = target[:7]
    if not git_commit_exists(target, cwd=cwd) or not git_is_ancestor(target, head, cwd=cwd):
        return (
            True,
            f"Draft release {expected_tag} (id={draft_id}) target {short_target} is not an ancestor of {head}; rebuilding.",
        )

    prev_tag = git_prev_tag(head, cwd=cwd)
    if prev_tag and not git_is_ancestor(prev_tag, target, cwd=cwd):
        return (
            True,
            f"Draft release {expected_tag} (id={draft_id}) target {short_target} predates latest tag {prev_tag}; rebuilding.",
        )

    if git_range_has_version_bump(f"{target}..{head}", cwd=cwd):
        return (
            True,
            f"New Version-Bump commit(s) landed since draft target {short_target}; updating draft {expected_tag}.",
        )

    return (
        False,
        f"Draft release {expected_tag} (id={draft_id}, target={short_target}) already includes all unreleased Version-Bump commits and required assets; skipping.",
    )


def cmd_check_tag(args: argparse.Namespace) -> int:
    token = os.environ.get("GH_TOKEN", "").strip()
    client = GitHubClient(token=token)
    published = client.get_published_release_by_tag(args.repo, args.tag)
    asset_count = 0
    if published is not None and published.get("draft") is not True:
        asset_count = len(published.get("assets") or [])

    if asset_count > 0:
        print(
            f"Published GitHub Release {args.tag} already exists with {asset_count} asset(s); skipping duplicate release."
        )
        write_github_output(args.github_output, {"should_release": "false"})
        return 0

    print(f"Tag {args.tag} has no published release assets yet; proceeding with release.")
    write_github_output(
        args.github_output,
        {
            "should_release": "true",
            "release_mode": "publish-tag",
        },
    )
    return 0


def cmd_check_draft(args: argparse.Namespace) -> int:
    token = os.environ.get("GH_TOKEN", "").strip()
    if not token:
        print(
            f"GH_TOKEN is not set; cannot inspect existing draft releases, proceeding to build draft {args.expected_tag}."
        )
        write_github_output(args.github_output, {"needs_update": "true"})
        return 0

    client = GitHubClient(token=token)
    releases = client.list_releases(args.repo)
    needs_update, reason = evaluate_draft_staleness(
        releases,
        expected_tag=args.expected_tag,
        expected_version=args.expected_version,
        head=args.head,
        cwd=args.repo_dir,
    )
    print(reason)
    write_github_output(args.github_output, {"needs_update": "true" if needs_update else "false"})
    return 0


def _read_notes(notes_file: str | None) -> str:
    if not notes_file:
        return ""
    path = Path(notes_file)
    if not path.is_file():
        return ""
    content = path.read_text(encoding="utf-8")
    return content if content.strip() else ""


def _delete_all_assets(client: GitHubClient, repo: str, release: dict[str, Any]) -> None:
    release_id = int(release["id"])
    assets = client.list_release_assets(repo, release_id)
    for asset in assets:
        asset_id = asset.get("id")
        if asset_id is None:
            continue
        print(f"Deleting stale asset '{asset.get('name')}' (id={asset_id}) from release {release_id}...")
        client.delete_asset(repo, int(asset_id))


def _delete_conflicting_assets(
    client: GitHubClient, repo: str, release_id: int, new_names: set[str]
) -> None:
    for asset in client.list_release_assets(repo, release_id):
        name = asset.get("name")
        asset_id = asset.get("id")
        if name in new_names and asset_id is not None:
            print(f"Deleting conflicting asset '{name}' (id={asset_id}) from release {release_id}...")
            client.delete_asset(repo, int(asset_id))


def _upload_all_assets(
    client: GitHubClient, repo: str, release: dict[str, Any], asset_paths: list[Path]
) -> None:
    for path in asset_paths:
        print(f"Uploading asset '{path.name}' ({path.stat().st_size} bytes)...")
        client.upload_asset(repo, release, path)


def sync_release(
    client: GitHubClient,
    *,
    repo: str,
    tag: str,
    target_sha: str,
    mode: str,
    notes: str,
    asset_paths: list[Path],
) -> dict[str, Any]:
    """Create, update, or publish the GitHub Release according to mode."""
    for path in asset_paths:
        if not path.is_file():
            raise FileNotFoundError(f"Release asset not found: {path}")

    prerelease = is_prerelease_tag(tag)
    new_asset_names = {p.name for p in asset_paths}

    if mode == "publish-tag":
        published = client.get_published_release_by_tag(repo, tag)
        if published is not None and published.get("draft") is not True:
            release_id = int(published["id"])
            print(f"Attaching assets to existing published release {tag} (id={release_id})...")
            _delete_conflicting_assets(client, repo, release_id, new_asset_names)
            _upload_all_assets(client, repo, published, asset_paths)
            return published

        releases = client.list_releases(repo)
        matching_drafts = [
            r for r in releases if r.get("draft") is True and r.get("tag_name") == tag
        ]
        if matching_drafts:
            matching_drafts.sort(key=lambda r: int(r.get("id") or 0))
            draft = matching_drafts[-1]
            release_id = int(draft["id"])
            print(f"Uploading assets to matching draft release {tag} (id={release_id}) before publishing...")
            _delete_all_assets(client, repo, draft)
            _upload_all_assets(client, repo, draft, asset_paths)
            patch_payload: dict[str, Any] = {"draft": False, "prerelease": prerelease}
            if notes and not str(draft.get("body") or "").strip():
                patch_payload["body"] = notes
            result = client.update_release(repo, release_id, patch_payload)
            print(f"Published release {tag}: {result.get('html_url', '')}")
            return result

        create_payload: dict[str, Any] = {
            "tag_name": tag,
            "target_commitish": target_sha,
            "name": tag,
            "draft": True,
            "prerelease": prerelease,
        }
        if notes:
            create_payload["body"] = notes
        else:
            create_payload["generate_release_notes"] = True
        print(f"Creating draft release {tag} to attach assets atomically before publishing...")
        draft = client.create_release(repo, create_payload)
        _upload_all_assets(client, repo, draft, asset_paths)
        result = client.update_release(repo, int(draft["id"]), {"draft": False})
        print(f"Published release {tag}: {result.get('html_url', '')}")
        return result

    if mode not in ("draft", "publish-rolling"):
        raise ValueError(f"Invalid mode '{mode}' (expected draft, publish-rolling, or publish-tag)")

    releases = client.list_releases(repo)
    existing_draft = find_rolling_draft(releases, prerelease=prerelease)

    if existing_draft is not None:
        release_id = int(existing_draft["id"])
        old_tag = str(existing_draft.get("tag_name") or "")
        old_target = str(existing_draft.get("target_commitish") or "")[:7]
        print(
            f"Updating existing draft release (id={release_id}): "
            f"tag {old_tag} -> {tag}, target {old_target} -> {target_sha[:7]}"
        )
        patch_payload = {
            "tag_name": tag,
            "target_commitish": target_sha,
            "name": tag,
            "draft": True,
            "prerelease": prerelease,
        }
        if notes:
            patch_payload["body"] = notes
        updated = client.update_release(repo, release_id, patch_payload)
        _delete_all_assets(client, repo, updated)
        _upload_all_assets(client, repo, updated, asset_paths)
        release = updated
    else:
        print(f"Creating new draft release {tag} targeting {target_sha[:7]}...")
        create_payload = {
            "tag_name": tag,
            "target_commitish": target_sha,
            "name": tag,
            "draft": True,
            "prerelease": prerelease,
        }
        if notes:
            create_payload["body"] = notes
        else:
            create_payload["generate_release_notes"] = True
        release = client.create_release(repo, create_payload)
        _upload_all_assets(client, repo, release, asset_paths)

    if mode == "publish-rolling":
        release = client.update_release(repo, int(release["id"]), {"draft": False})
        print(f"Published release {tag}: {release.get('html_url', '')}")
    else:
        print(f"Draft release {tag} ready for human approval: {release.get('html_url', '')}")
    return release


def cmd_sync(args: argparse.Namespace) -> int:
    token = os.environ.get("GH_TOKEN", "").strip()
    if not token:
        print(
            "error: GH_TOKEN (RELEASE_TOKEN) is required to synchronize GitHub Releases",
            file=sys.stderr,
        )
        return 1

    notes = _read_notes(args.notes_file)
    asset_paths = [Path(p) for p in args.asset]
    client = GitHubClient(token=token)
    sync_release(
        client,
        repo=args.repo,
        tag=args.tag,
        target_sha=args.target,
        mode=args.mode,
        notes=notes,
        asset_paths=asset_paths,
    )
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    p_tag = sub.add_parser("check-tag", help="Check if a v* tag already has a published release with assets")
    p_tag.add_argument("--repo", required=True, help="GitHub repository (owner/name)")
    p_tag.add_argument("--tag", required=True, help="Git tag name (vX.Y.Z)")
    p_tag.add_argument("--github-output", help="Optional path to $GITHUB_OUTPUT")
    p_tag.set_defaults(func=cmd_check_tag)

    p_draft = sub.add_parser("check-draft", help="Check if the rolling draft release needs creation or update")
    p_draft.add_argument("--repo", required=True, help="GitHub repository (owner/name)")
    p_draft.add_argument("--expected-tag", required=True, help="Computed next release tag (vX.Y.Z)")
    p_draft.add_argument("--expected-version", required=True, help="Computed next version (X.Y.Z)")
    p_draft.add_argument("--head", default="HEAD", help="Git ref to inspect (default: HEAD)")
    p_draft.add_argument("--repo-dir", default=None, help="Optional git working directory")
    p_draft.add_argument("--github-output", help="Optional path to $GITHUB_OUTPUT")
    p_draft.set_defaults(func=cmd_check_draft)

    p_sync = sub.add_parser("sync", help="Create/update a rolling draft release or publish a release")
    p_sync.add_argument("--repo", required=True, help="GitHub repository (owner/name)")
    p_sync.add_argument("--tag", required=True, help="Release tag name (vX.Y.Z)")
    p_sync.add_argument("--target", required=True, help="Target commit SHA")
    p_sync.add_argument(
        "--mode",
        required=True,
        choices=("draft", "publish-rolling", "publish-tag"),
        help="Synchronization mode",
    )
    p_sync.add_argument("--notes-file", default=None, help="Path to Markdown release notes")
    p_sync.add_argument("--asset", action="append", default=[], help="Asset file path (repeatable)")
    p_sync.set_defaults(func=cmd_sync)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
