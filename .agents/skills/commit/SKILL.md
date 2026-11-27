---
name: commit
description: >-
  Stages changes that are ready to commit, determines and suggests release
  trailers (`Version-Bump` and `Changelog`), and creates a git commit using a
  Go-style commit message (`<scope>: <lowercase summary>`). Use when the user
  asks to commit changes, create a commit, or prepare staged work for commit.
---

# Commit (Go-Style)

Inspect the working tree, stage only the changes that are ready, determine
whether release trailers are warranted, and commit them with a Go-style commit
message.

## Steps

1. **Inspect the working tree**
   Run `git status` and review `git diff` (and `git diff --cached` if anything
   is already staged) to understand every modified and untracked file.

2. **Stage only what is ready**
   - Stage complete, coherent changes that belong together using explicit paths
     (`git add <path>...`).
   - Do **not** blindly run `git add -A` if there are unrelated edits,
     work-in-progress files, scratch files, build artifacts, or secrets in the
     working tree.
   - If the user has already curated the index (`git diff --cached`), respect
     what is staged and only add other ready files if clearly intended.
   - Verify the staged set with `git diff --cached --stat` before committing.

3. **Determine and suggest release trailers**
   Releases are versioned from `Version-Bump` trailers and assembled into
   GitHub Release notes from `Changelog` trailers via `git-cliff` (`cliff.toml`).

   - **Evaluate the staged diff** to decide what is needed:
     - **Omit both trailers** for internal-only changes with no user-facing
       impact (CI workflows, build/release scripts, tests, internal refactors,
       docs, agent skills).
     - **`Version-Bump: Patch`** for user-visible bug fixes, crash/hang fixes,
       or small UX polish.
     - **`Version-Bump: Minor`** for backwards-compatible new user-facing
       features, settings, or workflow capabilities.
     - **`Version-Bump: Major`** for breaking changes to user data,
       configuration/workflow schemas, or behavior that break compatibility or
       require migration.
   - **If the user did not specify trailer values** (and did not ask to commit
     without confirmation):
     - Make your own determination from the diff and draft clear, user-facing
       `Changelog:` sentence(s) describing the user impact (not internal
       implementation details).
     - **Suggest** your proposed trailers (or explain why no release trailers
       are needed) and ask the user to confirm or adjust before running
       `git commit`.
   - **If the user gave only a bump level or only a changelog note**, infer and
     suggest the missing counterpart so the pair is valid before committing.

4. **Write a Go-style commit message**
   Follow the Go project's commit message conventions:

   - **Subject line**: `<scope>: <short summary>`
     - `<scope>` names the primary crate, module, directory, or area changed
       (e.g., `factory-core`, `factory-app/ui`, `agents`, `docs`, or `all` for
       repo-wide changes).
     - Start `<short summary>` with a **lowercase** imperative verb (`add`,
       `fix`, `refactor`, `update`, `remove`) — unless starting with an
       identifier or proper noun.
     - No trailing period.
     - Aim for ≤ 72 characters.
   - **Body** *(include for non-trivial changes; omit for tiny, self-evident ones)*:
     - Leave one blank line after the subject.
     - Write complete sentences explaining **why** the change is made and any
       important design decisions or caveats.
     - Wrap prose around 72 characters.
   - **Trailers** *(placed as the final paragraph at the end of the commit
     message with no trailing paragraphs afterward; validated in CI by
     `scripts/lint-commit-trailers.sh`)*:
     - `Version-Bump: Major|Minor|Patch` — optional; at most one per commit;
       exact key casing (`Version-Bump:`) and exact value (`Major`, `Minor`, or
       `Patch`).
     - `Changelog: <sentence>` — **required** whenever `Version-Bump` is
       present and **forbidden** without `Version-Bump`; exact key casing
       (`Changelog:`), non-empty user-facing sentence. May be repeated when a
       single commit includes multiple distinct user-facing changes.
     - **Issue trailers** *(when applicable)*: use `Fixes: #123` / `Updates: #123`
       (with a colon so git's trailer block stays valid alongside `Version-Bump`
       and `Changelog`).

   **Examples:**

   ```text
   factory-core: skip pending steps when cancelling a run

   Cancelling an active run previously left unstarted steps in the pending
   state, making run summaries ambiguous. Mark both running and pending
   steps as skipped so terminal runs have no lingering active steps.

   Version-Bump: Patch
   Changelog: Fixed cancelled workflow runs leaving unstarted steps stuck in pending state.
   ```

   ```text
   factory-app/ui: add inline project registration form

   Version-Bump: Minor
   Changelog: Added an inline form for registering projects directly from the sidebar.
   ```

   ```text
   all: initialize Rust and Native Shell macOS workspace
   ```

5. **Commit and verify**
   - Commit with `git commit` (using `-m` paragraphs or a heredoc — never
     amend or bypass hooks unless the user explicitly asks).
   - Verify with `bash scripts/lint-commit-trailers.sh HEAD`,
     `git log -1 --stat`, and `git status --short`, and report the commit hash,
     subject, release trailers (if any), and any intentionally unstaged files.
