---
name: commit
description: >-
  Stages changes that are ready to commit and creates a git commit using a
  Go-style commit message (`<scope>: <lowercase summary>`). Use when the user
  asks to commit changes, create a commit, or prepare staged work for commit.
---

# Commit (Go-Style)

Inspect the working tree, stage only the changes that are ready, and commit
them with a Go-style commit message.

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

3. **Write a Go-style commit message**
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
     - Place issue trailers at the end when applicable (e.g., `Fixes #123`,
       `Updates #123`).

   **Examples:**

   ```text
   factory-core: skip pending steps when cancelling a run

   Cancelling an active run previously left unstarted steps in the pending
   state, making run summaries ambiguous. Mark both running and pending
   steps as skipped so terminal runs have no lingering active steps.
   ```

   ```text
   factory-app/ui: add inline project registration form
   ```

   ```text
   all: initialize Rust and Native Shell macOS workspace
   ```

4. **Commit and verify**
   - Commit with `git commit` (using `-m` paragraphs or a heredoc — never
     amend or bypass hooks unless the user explicitly asks).
   - Verify with `git log -1 --stat` and `git status --short`, and report the
     commit hash, subject, and any intentionally unstaged files.
